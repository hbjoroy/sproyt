# Heart in the target cluster

Heart is an internal dependency for the optional event-planning pilot. Ordinary
chat does not depend on it. Heart is deployed by the same Sprøyt Helm release
and ArgoCD Application, not as a separate Argo application. Its API remains a
private ClusterIP service in the `sproyt` namespace and has no Ingress.

These commands assume the existing PostgreSQL service and administrator Secret
described in [running.md](running.md): namespace `database`, Service
`postgres-postgresql`, Secret `postgres-postgresql`, key `postgres-password`.

## Create the Heart database identity

Run this once through the cluster's web SSH session. It creates a dedicated
login and database; neither Sprøyt nor Heart receives the PostgreSQL
administrator password.

```sh
HEART_DB_PASSWORD="$(openssl rand -hex 32)"
kubectl -n database create secret generic heart-db-bootstrap \
  --from-literal=app-password="$HEART_DB_PASSWORD"

cat <<'YAML' | kubectl apply -f -
apiVersion: v1
kind: Pod
metadata:
  name: heart-db-bootstrap
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
              name: heart-db-bootstrap
              key: app-password
YAML

kubectl -n database wait --for=condition=Ready pod/heart-db-bootstrap --timeout=120s
kubectl -n database exec -i heart-db-bootstrap -- sh -c \
  'psql -h postgres-postgresql -U postgres -d postgres -v ON_ERROR_STOP=1 -v app_password="$APP_PASSWORD"' <<'SQL'
select format('create role heart login password %L', :'app_password')
where not exists (select from pg_roles where rolname = 'heart') \gexec
select format('alter role heart password %L', :'app_password') \gexec
select 'create database heart owner heart'
where not exists (select from pg_database where datname = 'heart') \gexec
\connect heart
alter schema public owner to heart;
grant usage, create on schema public to heart;
SQL

HEART_DATABASE_URL="postgresql://heart:${HEART_DB_PASSWORD}@postgres-postgresql.database.svc.cluster.local:5432/heart"
HEART_DATABASE_URL_B64="$(printf '%s' "$HEART_DATABASE_URL" | base64 | tr -d '\n')"
kubectl -n sproyt patch secret sproyt --type merge \
  -p "{\"data\":{\"HEART_DATABASE_URL\":\"${HEART_DATABASE_URL_B64}\"}}"

kubectl -n database delete pod heart-db-bootstrap --wait=true
kubectl -n database delete secret heart-db-bootstrap
unset HEART_DB_PASSWORD HEART_DATABASE_URL HEART_DATABASE_URL_B64
```

The existing `oci-pull-secret` in `sproyt` is reused by both components.

## Enable the bundled component through GitOps

Pin the reviewed ARM64 digest from [roadmap-status.md](roadmap-status.md) in the
existing `values/sproyt/values.yaml` file in `rocket-applications`:

```yaml
heart:
  enabled: true
  image:
    digest: sha256:6361420a90a18b07f7e4a14a135a0f527a006ccfb503f6ccb44e36ce45b27bc2
```

Do not set `config.heartUrl`: the chart derives
`http://sproyt-sproyt-heart:3000` automatically. ArgoCD runs the Heart migration
hook before rolling the private Deployment. The same Sprøyt Application owns
the Service, Deployment, migration Job, PDB and NetworkPolicy.

After sync, verify the release-local resources:

```sh
kubectl -n sproyt get deployment,service,pdb,networkpolicy \
  -l app.kubernetes.io/part-of=sproyt
kubectl -n sproyt rollout status deployment/sproyt-sproyt-heart --timeout=180s
kubectl -n sproyt run heart-check --rm -i --restart=Never --image=curlimages/curl -- \
  curl --fail --silent http://sproyt-sproyt-heart:3000/ready
```

## Rolling-update acceptance

Start one event-planning process, restart one Sprøyt pod while its durable
outbox item is leased, and roll both Deployments:

```sh
kubectl -n sproyt rollout restart deployment/sproyt-sproyt
kubectl -n sproyt rollout status deployment/sproyt-sproyt --timeout=180s
kubectl -n sproyt rollout restart deployment/sproyt-sproyt-heart
kubectl -n sproyt rollout status deployment/sproyt-sproyt-heart --timeout=180s
```

The process view must contain exactly one `process.started` event and continue
to completion. Scale Heart to zero and send an ordinary chat message; chat must
remain available. Restore Heart and confirm the queued operation completes
without a duplicate instance.
