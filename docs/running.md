# Run Sproyt locally and in Kubernetes

This guide keeps development simple and makes the cluster path reproducible.
The Kubernetes instructions assume that commands are run after SSH-ing to a
host with `kubectl` and Helm access to the cluster. No cluster credentials are
required on the development machine.

## Local development

### Native Rust with SQLite

Prerequisites are Rust 1.96, LLVM/clang, and PowerShell. From the repository
root:

```powershell
New-Item -ItemType Directory -Force .local | Out-Null
$env:CARGO_TARGET_DIR='S:\Source\sproyt-target'
cargo run --locked -- migrate
cargo run --locked
```

Open <http://127.0.0.1:9010/>. Development authentication is intentionally
simple and uses the deterministic local `guest` identity. Production never
accepts a selectable or query-string identity; it redirects anonymous users to
OIDC. The database is `.local/sproyt.sqlite` unless `DATABASE_URL` is set.

Heart/process diagnostics are excluded from the ordinary UI by default. Set
`SPROYT_UI_ADVANCED_ENABLED=true` only in an explicitly approved pilot
environment; the Helm value is `config.uiAdvancedEnabled` and remains `false`
for the private beta.

Verify the running service:

```powershell
Invoke-RestMethod http://127.0.0.1:9010/healthz
Invoke-RestMethod http://127.0.0.1:9010/readyz
Invoke-RestMethod http://127.0.0.1:9010/metrics
```

### Local PostgreSQL profile

`compose.yaml` starts PostgreSQL, runs migrations once, and then starts Sproyt.
Use Docker Compose or a compatible Compose implementation:

```powershell
$env:SPROYT_VCS_REF=(git rev-parse HEAD)
docker compose up --build
```

If `wslc` is the available OCI runtime, the native SQLite workflow above is
the shortest development loop. It can also build and run the production image
without Compose:

```powershell
$revision=git rev-parse HEAD
wslc build --build-arg "VCS_REF=$revision" -t "sproyt:$revision" .
wslc volume create sproyt-data
wslc run --rm --user 0:0 -v sproyt-data:/data `
  alpine:3.23@sha256:fd791d74b68913cbb027c6546007b3f0d3bc45125f797758156952bc2d6daf40 `
  chown 65532:65532 /data
wslc run --rm `
  -e SPROYT_ENV=development `
  -e SPROYT_AUTH_MODE=development `
  -e DATABASE_URL=sqlite:///data/sproyt.sqlite `
  -v sproyt-data:/data `
  "sproyt:$revision" migrate
wslc run --rm -p 9010:9010 `
  -e SPROYT_ADDR=0.0.0.0:9010 `
  -e SPROYT_ENV=development `
  -e SPROYT_AUTH_MODE=development `
  -e DATABASE_URL=sqlite:///data/sproyt.sqlite `
  -v sproyt-data:/data `
  "sproyt:$revision"
```

The named volume keeps the SQLite database across container restarts.

Run the repository quality gate before publishing:

```powershell
$env:CARGO_TARGET_DIR='S:\Source\sproyt-target'
cargo fmt -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```

## Build and publish the cluster image

Build only from a reviewed, committed revision, use that revision as the
immutable tag and OCI label, and record the digest read back from the registry.
Zot authentication currently works reliably through `regctl`; `wslc push` may
return `unauthorized` even after a successful interactive `wslc login`.

The Dockerfile compiles through the pinned `cargo-zigbuild` and Zig toolchain.
BuildKit keeps the builder on its native `BUILDPLATFORM` and produces a static
musl binary for the requested target, so an AMD64 CI runner can create the
cluster's ARM64 image without QEMU. Delivery CI checks that the release image
is `linux/arm64`; a separate native AMD64 image exercises the ephemeral kind
cluster through the Dockerfile's `native` smoke stage and is never published.

```powershell
$revision=git rev-parse HEAD
$image="oci.bjoroy.me/sproyt/sproyt:$revision"
wslc build --build-arg "VCS_REF=$revision" -t $image .
wslc save --output ".local/sproyt-$revision.tar" $image
wslc volume create sproyt-regctl-config

# Run once per credential change. Enter the Zot password at the prompt.
wslc run --rm -it --user 0:0 `
  -e HOME=/config `
  -v sproyt-regctl-config:/config `
  regclient/regctl:v0.11.5 `
  registry login --user hbjoroy oci.bjoroy.me

$localDir=(Resolve-Path .local).Path
wslc run --rm --user 0:0 `
  -e HOME=/config `
  -v sproyt-regctl-config:/config `
  -v "${localDir}:/work:ro" `
  regclient/regctl:v0.11.5 `
  image import $image "/work/sproyt-$revision.tar"

$digest=wslc run --rm --user 0:0 `
  -e HOME=/config `
  -v sproyt-regctl-config:/config `
  regclient/regctl:v0.11.5 `
  image digest $image
$digest
```

Use the registry digest (`sha256:...`), not the mutable tag, in Helm. The chart
rejects a production render without a digest.

## Prepare the existing cluster PostgreSQL

These assumptions match the current cluster description:

- namespace: `database`
- service: `postgres-postgresql`
- administrator Secret: `postgres-postgresql`
- administrator password key: `postgres-password`

First verify names without decoding or printing the password:

```sh
kubectl -n database get service postgres-postgresql
kubectl -n database get secret postgres-postgresql \
  -o jsonpath='{.data.postgres-password}' | grep -q .
```

The following creates a dedicated login, database, and schema. The temporary
bootstrap pod receives both passwords from Kubernetes Secrets; neither is
written to this repository. A hexadecimal application password avoids URL
escaping problems in `DATABASE_URL`.

```sh
kubectl create namespace sproyt --dry-run=client -o yaml | kubectl apply -f -

SPROYT_DB_PASSWORD="$(openssl rand -hex 32)"
kubectl -n database create secret generic sproyt-db-bootstrap \
  --from-literal=app-password="$SPROYT_DB_PASSWORD"

cat <<'YAML' | kubectl apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: sproyt-db-bootstrap
  namespace: database
spec:
  restartPolicy: Never
  containers:
    - name: psql
      image: postgres:17-alpine
      command: ["sleep", "3600"]
      env:
        - name: PGPASSWORD
          valueFrom:
            secretKeyRef:
              name: postgres-postgresql
              key: postgres-password
        - name: APP_PASSWORD
          valueFrom:
            secretKeyRef:
              name: sproyt-db-bootstrap
              key: app-password
YAML

kubectl -n database wait --for=condition=Ready pod/sproyt-db-bootstrap --timeout=120s
kubectl -n database exec -i sproyt-db-bootstrap -- sh -c \
  'psql -h postgres-postgresql -U postgres -d postgres -v ON_ERROR_STOP=1 -v app_password="$APP_PASSWORD"' <<'SQL'
select format('create role sproyt login password %L', :'app_password')
where not exists (select from pg_roles where rolname = 'sproyt') \gexec
select format('alter role sproyt password %L', :'app_password') \gexec
select 'create database sproyt owner sproyt'
where not exists (select from pg_database where datname = 'sproyt') \gexec
\connect sproyt
create schema if not exists sproyt authorization sproyt;
alter schema sproyt owner to sproyt;
alter role sproyt in database sproyt set search_path = sproyt, public;
SQL

kubectl -n database delete pod sproyt-db-bootstrap --wait=true
kubectl -n database delete secret sproyt-db-bootstrap
```

Keep `SPROYT_DB_PASSWORD` in the current shell until the application Secret is
created. If the shell is closed, reset the role password with the same
bootstrap procedure and a newly generated value.

## Prepare namespaced Secrets

Image pull Secrets are namespaced. Copy the existing registry credential into
the `sproyt` namespace. This command requires `jq`; change the source namespace
if the usable copy is elsewhere:

```sh
kubectl -n database get secret oci-pull-secret -o json \
  | jq 'del(.metadata.creationTimestamp,.metadata.resourceVersion,.metadata.uid,.metadata.managedFields) | .metadata.namespace="sproyt"' \
  | kubectl apply -f -
```

Create and verify the provider with [authentik.md](authentik.md), then set its
values for Sproyt. The issuer must be the exact discovery-capable provider URL,
including the provider slug and trailing slash:

```sh
OIDC_ISSUER='https://sproyt-security.bjoroy.me/application/o/sproyt/'
OIDC_CLIENT_ID='<client-id>'
OIDC_CLIENT_SECRET='<client-secret>'
PUBLIC_ORIGIN='https://sproyt.bjoroy.me'
SPROYT_SESSION_KEY="$(openssl rand -base64 32 | tr '+/' '-_' | tr -d '=')"

kubectl -n sproyt create secret generic sproyt \
  --from-literal=DATABASE_URL="postgresql://sproyt:${SPROYT_DB_PASSWORD}@postgres-postgresql.database.svc.cluster.local:5432/sproyt" \
  --from-literal=SPROYT_OIDC_CLIENT_SECRET="$OIDC_CLIENT_SECRET" \
  --from-literal=SPROYT_SESSION_KEY="$SPROYT_SESSION_KEY" \
  --dry-run=client -o yaml | kubectl apply -f -
```

Do not reuse the PostgreSQL root credential in `DATABASE_URL`. Sproyt needs only
its dedicated `sproyt` role. Register `${PUBLIC_ORIGIN}/auth/callback` and
`${PUBLIC_ORIGIN}/` as the callback and post-logout URLs in Authentik.

## Install with Helm

Create a values file on the SSH host. Replace all angle-bracket placeholders.
Ingress stays disabled because it will be managed manually.

```sh
cat > /tmp/sproyt-values.yaml <<EOF
replicaCount: 2

image:
  repository: oci.bjoroy.me/sproyt/sproyt
  # Reviewed application commit b28dea405b725e798ceb2a2fc32445dde272b6d6
  digest: sha256:c4bf3bfd80566777c7abaa63f17a3d9a066158a04d9c2b6e5763b0b020b7150f
  pullPolicy: IfNotPresent

imagePullSecrets:
  - name: oci-pull-secret

ingress:
  enabled: false

config:
  environment: production
  logFormat: json
  authMode: oidc
  oidcIssuer: ${OIDC_ISSUER}
  oidcClientId: ${OIDC_CLIENT_ID}
  oidcRedirectUrl: ${PUBLIC_ORIGIN}/auth/callback
  oidcPostLogoutRedirectUrl: ${PUBLIC_ORIGIN}/
  heartUrl: ""
  mcpAllowedOrigins: ${PUBLIC_ORIGIN}

secret:
  existingSecret: sproyt

networkPolicy:
  enabled: true
  databaseSameNamespace: false
  databaseNamespaceSelector:
    matchLabels:
      kubernetes.io/metadata.name: database
  databasePodSelector: {}
  ingressNamespaceSelector:
    matchLabels:
      kubernetes.io/metadata.name: <ingress-controller-namespace>
EOF

helm upgrade --install sproyt ./helm/sproyt \
  --namespace sproyt \
  --values /tmp/sproyt-values.yaml \
  --wait --timeout 5m
```

The pre-install/pre-upgrade migration Job uses the same digest and dedicated
database role before the Deployment rolls. It uses the namespace's default
ServiceAccount with token automount disabled because Helm hooks run before the
chart-managed application ServiceAccount exists.

Verify the installation before exposing it:

```sh
kubectl -n sproyt rollout status deployment/sproyt-sproyt --timeout=180s
kubectl -n sproyt get pods,service,pdb,networkpolicy
kubectl -n sproyt port-forward service/sproyt-sproyt 9010:80
```

In another shell:

```sh
curl --fail http://127.0.0.1:9010/healthz
curl --fail http://127.0.0.1:9010/readyz
```

## Add ingress manually

Create the Ingress in the `sproyt` namespace and route it to Service
`sproyt-sproyt` on port `80`. A minimal starting point is:

```yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: sproyt
  namespace: sproyt
spec:
  ingressClassName: <class-name>
  tls:
    - hosts: [sproyt.bjoroy.me]
      secretName: <tls-secret>
  rules:
    - host: sproyt.bjoroy.me
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: sproyt-sproyt
                port:
                  number: 80
```

The ingress controller namespace must match
`networkPolicy.ingressNamespaceSelector`. Confirm external OIDC login, logout,
WebSocket reconnect, and `/readyz` after applying the Ingress.

Heart is an optional internal component of the same Sprøyt Helm release and
ArgoCD Application. After creating `HEART_DATABASE_URL` as described in
[`heart-cluster.md`](heart-cluster.md), enable the reviewed digest:

```yaml
heart:
  enabled: true
  image:
    digest: sha256:<reviewed-heart-digest>
```

The chart creates no Heart Ingress and automatically sets the internal service
URL. Its pre-upgrade hook completes Heart migrations before the bundled
Deployment rolls. Sprøyt scopes
starts with `X-Heart-Client: sproyt`, uses its durable outbox UUID as
`Idempotency-Key`, and reconciles a timed-out accepted start through Heart's
`/api/v1/instance-starts/{key}` endpoint.

The chart generates matching least-privilege policies between the Sprøyt and
Heart pods. Ordinary chat remains available when `heart.enabled` is false or
Heart is temporarily unavailable.

## Upgrade and rollback

Build and push a new commit-tagged image, replace only `image.digest`, and run
the same `helm upgrade --install` command. To rotate an application Secret,
also change `secret.rolloutChecksum` to a new opaque value so pods roll.

```sh
helm history sproyt --namespace sproyt
helm rollback sproyt <revision> --namespace sproyt --wait --timeout 5m
```

Database migrations are forward-only and additive. Keep the previous image
digest and Helm revision until the post-deployment checks and OIDC flow pass.
