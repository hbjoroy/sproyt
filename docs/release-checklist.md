# Production release gate

Attach durable links or artifacts for every checked item. A release is not
production-ready based only on a successful build.

For the read-only cluster and public-boundary portion, run from a current
checkout and pass the deployed application and GitOps revisions explicitly:

```bash
bash tools/verify-production-rollout.sh \
  eec2d792780622fac50caabe62e213106cb547e4 \
  sha256:08ca736359ed085ff0ba5ed103a48f76ea35ac39d7a03a8166507ce59e8a0ae2 \
  62bcdee990d3350611253e33db77f6f015be91ba
```

The verifier does not read Secrets or mutate the cluster. It verifies internal
Kubernetes state and uses the real public Cloudflare path for health and
readiness; Kubernetes API-server service proxy traffic may correctly be denied
by NetworkPolicy and is not a production client path. Retain the JSON output
with the release evidence. It deliberately does not replace the authenticated
two-user browser journey below.

## Build and security

- [ ] Format, Clippy, unit, SQLite, and PostgreSQL contract tests pass.
- [ ] OCI image is addressed by digest and built from the reviewed commit.
- [ ] SBOM is retained and no unaccepted high/critical finding remains.
- [ ] OIDC discovery, callback, logout, invalid state/nonce, and key rotation
      are tested against the configured Authentik provider using the evidence
      procedure in `authentik.md`.
- [ ] Kubernetes secrets are external to source and rendered CI artifacts.
- [ ] Log/trace sample contains no private content, credentials, or tokens.

## Recovery and compatibility

- [ ] Fresh install, migration, two-replica smoke test, and rolling upgrade pass.
- [ ] Previous application image works after the forward migration.
- [ ] Backup restore drill records recovery duration and integrity checks.
- [ ] Feature kill switches and ordinary-chat behaviour without Heart pass.

## Performance and operations

- [ ] The CI WebSocket capacity/reconnect baseline passes, and pre-release
      two-replica load/reconnect evidence meets the objectives in
      `operations.md`.
- [ ] Dashboard, alerts, on-call owner, and incident channel are recorded.
- [ ] Capacity headroom covers at least twice the measured beta peak.
- [ ] Retention, backup deletion lag, export access policy, and privacy owner are accepted.
- [ ] Release owner records image digest, chart version, migration set, rollout
      observation, and rollback target.
