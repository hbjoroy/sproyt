# Production release gate

Attach durable links or artifacts for every checked item. A release is not
production-ready based only on a successful build.

## Build and security

- [ ] Format, Clippy, unit, SQLite, and PostgreSQL contract tests pass.
- [ ] OCI image is addressed by digest and built from the reviewed commit.
- [ ] SBOM is retained and no unaccepted high/critical finding remains.
- [ ] OIDC discovery, callback, logout, invalid state/nonce, and key rotation
      are tested against the configured Authentik provider.
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
