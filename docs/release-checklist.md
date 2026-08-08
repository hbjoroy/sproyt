# Production release gate

Attach durable links or artifacts for every checked item. A release is not
production-ready based only on a successful build.

Before release, manually dispatch `CI` from the exact `main` commit with
`publish_image` enabled, or push its reviewed `v*` release tag. Manual publish
dispatches from other refs are rejected. The publish job runs only after every
full release job passes and publishes the already-tested ARM64 image under the
immutable commit tag. Record the digest from the
`registry-evidence-<commit>` artifact. Normal pull requests and `main` pushes
deliberately run only the fast quality and PostgreSQL gates; the full
ARM64/kind/SBOM/recovery gate also runs weekly as a regression sentinel without
publishing an image.

For the read-only cluster and public-boundary portion, run from a current
checkout and pass the deployed application and GitOps revisions explicitly:

```bash
bash tools/verify-production-rollout.sh \
  4abef10a7435cd549d749b8e4a1f08d46f106234 \
  sha256:54f862678ddc780ea0811d4f19aac95749ceb52ecf89a1f194004ae224091e92 \
  4e9823d718c928a8ef2b43d6dbd3ef370e2aae9b
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
- [ ] Browser WebSocket, session-refresh and upload outcome panels show no
      unexplained regression during the rollout observation window.
- [ ] Capacity headroom covers at least twice the measured beta peak.
- [ ] Retention, backup deletion lag, export access policy, and privacy owner are accepted.
- [ ] Release owner records image digest, chart version, migration set, rollout
      observation, and rollback target.
