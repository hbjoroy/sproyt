# Production release gate

Attach durable links or artifacts for every checked item. Use the
[CI change matrix](ci-iteration-strategy.md#change-matrix) to determine which
checks apply and record the evidence for the exact revision or digest.

## Publish a new release

Use a `CI` dispatch from `main` with `publish_image=true`, or a reviewed `v*`
release tag. The workflow rejects publish dispatches from other refs.
Confirm the run SHA is the reviewed release commit. For a dispatch, supply the
last verified application revision as `base_revision` so selection covers the
entire change range. See the
[delivery phases](ci-iteration-strategy.md#delivery-phases) for full-regression
triggers and the checks required for publication.

```bash
gh workflow run ci.yml --ref main -f publish_image=true -f base_revision="$LAST_VERIFIED_REVISION"
```

Set `LAST_VERIFIED_REVISION` to the full 40-character revision. Retain the scope
summary and delivery evidence. Record the registry manifest digests for the
application and media worker from `registry-evidence-<commit>`.

Observe long CI waits with a bounded watcher and report meaningful stage changes.
Avoid duplicate watchers, minute-by-minute model polling, redundant local full
test runs and repeated narrative updates while an external queue is unchanged.

## Promote the verified canary

Use the healthy canary's immutable image digests and chart revision. Validate
the changed production values and Argo CD Application, then verify Argo CD,
ready replicas, public readiness and revision after rollout. Reuse the existing
application CI evidence for those digests. Apply the matrix and the
[rollout and rollback policy](ci-iteration-strategy.md#rollout-and-rollback)
to any changed migration, backup, authentication, network or rollout behaviour.

For the read-only cluster and public-boundary portion, run from a current
checkout and pass the deployed application revision, digest and GitOps revision
explicitly.

```bash
bash tools/verify-production-rollout.sh "$APP_REVISION" "$APP_DIGEST" "$GITOPS_REVISION"
```

The verifier does not read Secrets or mutate the cluster. It verifies internal
Kubernetes state and uses the real public Cloudflare path for health and
readiness. NetworkPolicy may deny Kubernetes API-server service proxy traffic.
Retain the JSON output with the release evidence. Record worker rollout,
migration Job status when applicable, and authenticated browser smoke checks
separately using the [deployment runbook](operations.md#deployment-rollback-and-incident-response).

## Build and security

- [ ] The publish workflow passed all selected checks. The scope summary
      accounts for skipped backend, browser and restore checks.
- [ ] OCI image is addressed by digest and built from the reviewed commit.
- [ ] SBOM is retained and no unaccepted high/critical finding remains.
- [ ] For authentication or provider changes, test OIDC discovery, callback,
      logout, invalid state/nonce and key rotation against the configured
      Authentik provider using the [evidence procedure](authentik.md#live-acceptance).
- [ ] Kubernetes secrets are external to source and rendered CI artifacts.
- [ ] Log/trace sample contains no private content, credentials, or tokens.

## Recovery and compatibility

- [ ] For changes that affect chart, migration, runtime or rollout and rollback
      behaviour, record the applicable isolated install, migration, two-replica
      rolling upgrade and application rollback evidence. Verify that the
      previous image works after any forward migration. Use the
      [deployment runbook](operations.md#deployment-rollback-and-incident-response).
- [ ] For changes affecting backup or recovery compatibility, record recovery
      duration and integrity checks from an isolated restore, or link still-valid
      evidence. Ordinary additive migrations require upgrade and compatibility
      checks; a restore drill is required only when recovery assumptions change. Use the
      [backup and restore runbook](operations.md#backup-and-restore-drill).
- [ ] Feature kill switches and ordinary-chat behaviour without Heart pass.

## Performance and operations

- [ ] Retain selected CI WebSocket capacity/reconnect evidence. Before a
      material traffic increase, record two-replica load/reconnect evidence
      against the [operations objectives](operations.md#initial-service-objectives).
- [ ] Dashboard, alerts, on-call owner, and incident channel are recorded.
- [ ] Browser WebSocket, session-refresh and upload outcome panels show no
      unexplained regression during the rollout observation window.
- [ ] Capacity headroom covers at least twice the measured beta peak.
- [ ] Retention, backup deletion lag, export access policy, and privacy owner are accepted.
- [ ] Release owner records image digest, chart version, migration set, rollout
      observation, and rollback target.
