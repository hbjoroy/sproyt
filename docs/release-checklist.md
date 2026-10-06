# Production release gate

Attach durable links or artifacts for every checked item. A release is not
production-ready based only on a successful build.

Before building a new release, manually dispatch `CI` from the exact `main`
commit with `publish_image` enabled, or push its reviewed `v*` release tag.
Manual publish dispatches from other refs are rejected. The publish job waits
for all selected checks and publishes the tested ARM64 image under the
immutable commit tag. Record the digest from the
`registry-evidence-<commit>` artifact. Normal pull requests and `main` pushes
deliberately run only the fast quality and PostgreSQL gates; the full
ARM64/SBOM/recovery gate also runs weekly as a regression sentinel without
publishing an image.

## Choose checks from the change

Promotion from a healthy canary uses the same immutable image digest and chart
revision. Validate the changed production values and Application, then verify
Argo, ready replicas, public readiness and revision after rollout. Do not
rebuild the image or repeat application UI/restore CI for this promotion.
Migration, backup, authentication or network changes require their own checks.

For a new release, pass `base_revision` as the last verified application
revision. CI examines **all** changes between that ancestor and the release:

- Backend feature changes run the Rust and PostgreSQL contracts without a full
  browser suite unless shared HTTP/UI/protocol contracts changed.
- Imagegen frontend changes run imagegen, media-draft and composer-validation
  browser contracts, including the original `/imagegen` validation behavior.
- Shared frontend, HTTP, protocol and legacy HTML changes run full Chromium and
  WebKit contracts.
- Database, migrations, domain persistence, runtime operations, dependencies,
  build/configuration and unclassified changes retain the backup/restore drill.
  Ordinary imagegen or frontend changes do not need that drill.
- Missing/invalid/non-ancestor baselines, version tags, weekly CI and
  `force_full=true` retain the complete browser and recovery regression gate.
- Changes confined to the CI selector/workflow run selector regression and
  workflow validation; they do not require application UI or database restore
  testing. Mixing policy changes with application code retains the full gate.

Every published image still passes compilation, lint, unit/PostgreSQL tests,
Helm delivery validation, ARM64 identity checks, SBOM and vulnerability scanning.
The workflow summary records selected checks. Publication accepts a skipped
recovery job only when scope selection explicitly says it was unnecessary;
failed or cancelled checks never permit publication.

```powershell
gh workflow run ci.yml --ref main -f publish_image=true -f base_revision=<last-verified-40-character-revision>
```

Observe long CI waits with a bounded watcher and report meaningful stage changes.
Avoid duplicate watchers, minute-by-minute model polling, redundant local full
test runs and repeated narrative updates while an external queue is unchanged.

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
