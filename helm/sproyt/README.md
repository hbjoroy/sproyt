# Sproyt Helm chart

`config.oidcIssuer` and `config.oidcClientId` are required. The production
issuer is pinned to the discovery-capable Authentik provider URL
`https://sproyt-security.bjoroy.me/application/o/sproyt/`.

Create the referenced Secret before installation. It must contain
`DATABASE_URL`, `SPROYT_OIDC_CLIENT_SECRET`, and `SPROYT_SESSION_KEY`.
During session-key rotation it may also contain the optional
`SPROYT_SESSION_PREVIOUS_KEYS` value.
Owner-created enrollment links are optional. To enable them, add a dedicated
least-privilege Authentik token as `SPROYT_AUTHENTIK_API_TOKEN` and set
`config.authentikEnrollmentFlowId`; see
[`docs/authentik.md`](../../docs/authentik.md#invite-a-new-sprøyt-user).
Private registries can be configured with `imagePullSecrets`; each referenced
Secret must exist in the release namespace. See
[`docs/running.md`](../../docs/running.md) for the complete external PostgreSQL,
`oci.bjoroy.me`, manual-Ingress installation path.

```sh
helm upgrade --install sproyt ./helm/sproyt \
  --namespace sproyt --create-namespace \
  --set image.digest=sha256:<registry-digest> \
  --set ingress.enabled=true \
  --set ingress.host=sproyt.bjoroy.me \
  --set config.oidcClientId=<client-id> \
  --set config.oidcRedirectUrl=https://sproyt.bjoroy.me/auth/callback \
  --set config.oidcPostLogoutRedirectUrl=https://sproyt.bjoroy.me/
```

The pre-install/pre-upgrade Job applies additive SQLx migrations before the
Deployment rolls. Application pods never mutate schema at startup. Use a
registry digest in production; `image.tag` is retained for local and kind
workflows with `config.environment=development`. Production rendering fails
when `image.digest` is empty. The same rendered digest is used by both the
migration Job and the Deployment. The Job intentionally receives
only `DATABASE_URL` and the log format: it does not initialize OIDC and remains
available during provider outages, client-secret changes, and session-key
rotation.

`config.dbMaxConnections` bounds the one PostgreSQL SQLx pool shared by chat,
processes, agents, integrations, notifications, and optional image generation.
The default is 4 per application pod; the accepted range is 1–32. The realtime
PostgreSQL listener keeps one dedicated connection outside that pool, so budget
`dbMaxConnections + 1` per pod (plus the separate migration Job during release).
Idle pooled connections close after 60 seconds and pool acquisition times out
after 5 seconds. SQLite operation is unchanged.

Heart is an optional internal component of this same Helm release, not a
separate ArgoCD application. Enabling `heart.enabled` creates a private
ClusterIP Service, Deployment, migration Job, PDB, and NetworkPolicy in the
Sprøyt namespace. A second ordered hook idempotently bootstraps the immutable
`sproyt/sproyt-event-planning/1.0.0` definition after migrations and before the
Deployment rolls. It creates no Ingress. Pin `heart.image.digest` and add
`HEART_DATABASE_URL` to the existing Secret before enabling it:

```yaml
heart:
  enabled: true
  image:
    digest: sha256:<reviewed-heart-digest>
```

Unless `config.heartUrl` is explicitly overridden, Sprøyt is configured with
the release-local service URL automatically. Disabling Heart removes only the
process component; ordinary chat remains available.

The default NetworkPolicy permits HTTPS egress, DNS, and PostgreSQL only to
pods in the release namespace. For a database in another namespace set
`networkPolicy.databaseSameNamespace=false` and provide namespace/pod selectors;
for an external database provide the narrow `databaseCidrs`. Set
`networkPolicy.ingressNamespaceSelector` for an ingress controller outside the
release namespace. The bundled Heart component is admitted automatically only
from the matching Sprøyt pods. When `config.heartUrl` instead points to an in-cluster service on a
non-HTTPS port, set `networkPolicy.heartSameNamespace` or the Heart
namespace/pod selectors (or a narrow `heartCidrs` entry) and `heartPort`.
ConfigMap changes automatically roll pods. When the contents of the externally
managed Secret change, update `secret.rolloutChecksum` to an opaque new value
so Kubernetes performs the same controlled rollout.
