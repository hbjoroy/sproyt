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
alter database heart owner to heart;
grant connect, create, temporary on database heart to heart;
\connect heart
alter schema public owner to heart;
grant usage, create on schema public to heart;
create schema if not exists heart authorization heart;
alter schema heart owner to heart;
grant usage, create on schema heart to heart;
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

## Rollout budget and acceptance

The [bounded rollout contract](heart-rollout-budget.md) replaces rolling Heart
updates with Recreate and serial maintenance. It includes the two shared-role
releases, transition/rollback steps and remaining isolated Kubernetes acceptance.
The historical database-creation example above is not the current shared-role
budget. Before running the commands below, both releases must use the new
contract, the preflight must pass and rollout must be authorized. These commands
exercise a real service pause; ordinary chat and its durable process outbox must
remain available.


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
# Upgrading Heart for v2 external workers

Pin the canary Heart image by registry digest in the GitOps repository. The
Heart migration Job and runtime both use `sproyt.heart.image`. Preserve the
current database, Service, v2 setting and pool budget. Confirm a restorable
backup for the target database and run the pinned `heart-api migrate` on its
restored copy, including the new external-job migration. Retain the verified
image and database checkpoints for recovery.

**Shared database ordering.** On 18 November 2026, the canary Heart pod and
both production Heart pods used the same database. Migrations 001–007 matched
the SQLx source checksums. Recheck the migration ledger and database identities
before rollout. Serialize the two releases. First deploy the pinned image in
canary with external admission off. During this window keep the old production
migration hook idle and avoid restarting an old release through its hook.
Verify an existing v1/v2 integration. Then deploy the identical digest in
production, still with external admission off, and verify every old Heart
replica has stopped. An old scheduler could pick an activation containing a
new node once work is admitted. To run a pilot before upgrading both releases,
use an isolated Heart database. To enable the pilot on the shared database, configure
an existing private Secret and the worker's own service token, authorize the
exact namespace/type/version in the credential file, and issue a TLS certificate
for the private Service DNS name used by the SDK. Put the certificate chain
and key in a TLS Secret. Give workers the issuing CA through their trust store.
Check the hostname and certificate verification from the worker pod.

```yaml
heart:
  runtimeV2:
    enabled: true
    externalJobs:
      credentialsSecret: heart-worker-credentials
      credentialsSecretKey: credentials.json
      newWorkEnabled: true
      tls:
        existingSecret: heart-worker-serving-tls
```

Heart reads the mounted file on startup. To rotate credentials, add an
overlapping token under the same principal, roll out the updated credential
file (use a versioned Secret name), update workers, then remove the old token
in a subsequent rollout. Grant same-namespace worker pods access to the
worker-only Service port 3001 using the release's instance label and
`app.kubernetes.io/component: heart-external-worker`. Apply the worker's
own matching egress policy. Application/tenant identity headers do not grant
worker access.

Before promoting the same digest, run the SDK pilot against synthetic jobs,
verify the joined result and duplicate-command receipt, and inspect queue age,
expired grants and terminal failures. To drain, turn off `newWorkEnabled` while
keeping runtime v2, the credential file and workers available. Preserve the new
schema and use a compatible runtime to reconcile accepted work. Database restore
requires separate reconciliation of effects already accepted downstream.

## Heart for «Tilbakemeldingar Sprøyt»

`config.feedbackEnabled` aktiverer både kanalarbeidaren og synkroniseringa av
`sproyt/feedback-issue/1.0.0`. Aktiver dette i éi installasjon når canary og
produksjon les den same Sprøyt-databasen. Kontroller før aktivering kva
`SPROYT_HEART_URL` løyser til frå denne installasjonen, kva Heart-database
den tenesta bruker, og at v2 er aktivert der. Dei andre installasjonane skal ha
`config.feedbackEnabled: false`.

Sprøyt registrerer definisjonen via v2-API-et ved første start, med kjeldemeldinga
som idempotensnøkkel. Produkteigaren lagrar ei vurdering med «Utført». Heart
går så til `await-github`, ei oppgåve tildelt kanalagenten. Når eit GitHub-oppslag
starta etter vurderinga stadfestar lukking, får Heart sluttårsaka. GitHub
`completed` fører til «Løyst», og `not_planned` fører til «Avvist».

Ved flytting av arbeidaren må mottakaren nå den same varige Heart-databasen,
og den gamle arbeidaren må vere stoppa først. Dersom installasjonane skal ha
kvar si Heart-database, stopp nytt inntak og la aktive instansar fullføre i
den opphavlege installasjonen før du flyttar arbeidet. GitHub-kvitteringar,
vurderingar og Heart-instans-ID-ar ligg i den delte Sprøyt-databasen.
