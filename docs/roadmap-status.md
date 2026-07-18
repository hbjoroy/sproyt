# Roadmap implementation status

This is the evidence index for S-01 through S-20. `Implemented` means the
repository contains the production-shaped implementation and automated
contract evidence. It does not by itself mean that the production release
gate has been signed off in the target environment.

| Item | Status | Authoritative evidence | Remaining environment gate |
| --- | --- | --- | --- |
| S-01 | Implemented | `.github/workflows/ci.yml`, pinned `rust-toolchain.toml`, green PR quality and PostgreSQL jobs | None |
| S-02 | Implemented | `config.rs`, `operations.rs`, `/healthz`, `/readyz`, metrics and shutdown tests | Observe shutdown and telemetry in the target cluster |
| S-03 | Implemented | Domain ID/sequence types, SQL migrations, protocol serialization tests | None |
| S-04 | Implemented | Shared repository contract in `db/mod.rs`, run by in-memory, SQLite and PostgreSQL adapters | None |
| S-05 | Implemented | SQLx adapters, explicit `sproyt migrate`, concurrency/restart and migration CI | Run migration Job against the target database |
| S-06 | Implemented | `ChatEngine` delegates authoritative state to `ChatRepository` and persists before publish | None |
| S-07 | Implemented | Connection-ID presence test, lag recovery test, PostgreSQL notification adapter and two-replica Helm smoke | Observe cross-replica traffic in the target cluster |
| S-08 | Implemented | `sproyt.chat.v1` envelopes plus WebSocket reconnect, idempotency, error and lag tests | None |
| S-09 | Backend slice implemented; UI replacement required | `two_users_complete_private_circle_slice_with_unread_reconnect`; DOM-safe Markdown/raw view; strict, exact-version Mermaid rendering; CSP regression | Superseded technical demonstration UI must be replaced by S-21 through S-24 |
| S-10 | Implemented | `AuthService`, deterministic development principals and production/dev rejection tests | None |
| S-11 | Implemented and active | OIDC discovery, PKCE, state, nonce, encrypted session, optional refresh-token handling, logout, key-rotation and revoked-user contracts in `auth.rs`; live Authentik login and identity verified through Cloudflare; production callback and issuer pinned; 2026-07-18 browser session remained connected through periodic provider revalidation after the no-refresh-token loop fix | Complete destructive logout/revocation and key-rotation operations as a scheduled production security drill |
| S-12 | Implemented | Central `Policy`, executable authorization matrix and mutation-audit migrations | Review target-environment audit retention/access |
| S-13 | Implemented and published | Pinned scratch ARM64 image, restricted runtime contract, SBOM and vulnerability CI; registry evidence below | Retain CI and registry evidence with the release record |
| S-14 | Implemented | Helm delivery verifier and kind install, migration, two-replica scale and rollback gate | Install in the ARM64 target cluster and provide manual Ingress |
| S-15 | Baseline implemented, sign-off pending | SLOs/runbook, Prometheus/Grafana resources, recovery CI, WebSocket capacity gate, safe MCP latency/idempotency evidence tool, owner-authorized audited circle deletion, and snapshot-consistent `sproyt.user-export.v1` self-export | Run target-sized MCP/browser load, name owners, accept privacy/retention and sign off rollout |
| S-16 | Implemented; bundled activation pending | Sproyt `ProcessGateway` contract, Heart contract and idempotent definition bootstrap at `dc8b2b1`, green `ea-heart-client` receive-message contract/CI at `9510cc4`, and optional private Heart component in the same Sprøyt Helm/Argo release | Add `HEART_DATABASE_URL` to the existing Sprøyt Secret and flip the single GitOps enable flag |
| S-17 | Implemented | Durable process link/event/outbox state, stable command receipts, Heart `Idempotency-Key`/`X-Heart-Client`, exact replay, bounded retry and unknown-outcome lookup at Heart `dc8b2b1` | Exercise restart/rolling update in the target cluster |
| S-18 | Pilot implemented; bundled rollout pending | Event-planning definition, browser/MCP flow tests, kill switch, Heart receive/idempotent-start contract, ordered migration plus immutable definition bootstrap, same-release private Service/Deployment/PDB/NetworkPolicy, and updated cluster guide | Provision the Heart database key, enable it in the existing Sprøyt Application, then exercise restart/rolling update |
| S-19 | Implemented | Agent profiles, scoped grants, expiry/revocation, database-authoritative cross-replica rate limits, provenance and audit tests | Issue and revoke a target-environment agent credential |
| S-20 | Implemented | MCP transport checks, WebSocket/MCP adapter-conformance tests, and bounded production MCP load-evidence tool | Issue a short-lived scoped credential and exercise the production endpoint |
| S-21 | Implemented and active | Production session boundary; encrypted session; periodic provider revalidation; profile/logout shell; no simulated production identity; live Authentik identity and stable production session verified 2026-07-18 | Retain logout/revocation in the scheduled security drill |
| S-22 | Implemented and active | Responsive navigation/timeline/composer; accessible mobile conversation/circle drawer; circle-grouped channels; human-readable sender labels; visible keyboard focus; bounded exponential reconnect; draft preservation; read markers/unread badges; production `# general` loaded with active composer and remained connected beyond reauthentication 2026-07-18 | Deploy the mobile drawer and complete two-user usability sign-off |
| S-23 | Implemented; rollout pending | Guided circle creation with automatic `Prat` channel; selected-circle invite action; shareable invite links; OIDC-signed return path; copy/fallback UX; link recognition and actionable invalid/expired feedback; existing two-user authorization contract plus 2026-07-17 browser journey | Exercise owner-to-fresh-user invite through production Authentik after deployment |
| S-24 | In progress | Advanced controls default off; Heart is now an optional private component of the same Sprøyt release; CSP, focus, mobile drawer, reconnect and capacity gates; immutable ARM64/GitOps rollout; 20/20 readiness; live OIDC identity and stable reconnect/reauth acceptance | Deploy and exercise the mobile drawer, then complete two-user/logout acceptance and Heart failure-isolation pass |

## Current immutable image evidence

The current production chat revision
`2c84c16c7a9bc40d61903b19f96d655168f4f578` is published as:

```text
oci.bjoroy.me/sproyt/sproyt:2c84c16c7a9bc40d61903b19f96d655168f4f578
sha256:5ecc4ad52ed8c033dbf4d19d7055b3f0cf470f673e8dfea8b2a67c816151a26e
```

GitOps pins the binary digest and chart revision
`71f20bfe9f03fe54bc3338a1c029dc65b2423dbb`. On 2026-07-18 the authenticated
production client loaded the correct Authentik identity and `# general`, kept
the composer enabled, and remained `Tilkopla` through the 30-second provider
reauthentication boundary. The browser source proved the previous proactive
refresh redirect loop absent.

On 2026-07-17, the reviewed application merge commit
`b28dea405b725e798ceb2a2fc32445dde272b6d6` was imported into Zot as:

```text
oci.bjoroy.me/sproyt/sproyt:b28dea405b725e798ceb2a2fc32445dde272b6d6
sha256:c4bf3bfd80566777c7abaa63f17a3d9a066158a04d9c2b6e5763b0b020b7150f
```

The registry config reports `linux/arm64`, non-root user `65532:65532`, and OCI
revision label `b28dea405b725e798ceb2a2fc32445dde272b6d6`. GitHub Actions run
`29574475877` passed format/lint/test, PostgreSQL contract, backup/restore,
dependency audit, ARM64 build, Helm rendering, kind install/scale/rollback,
SBOM generation and vulnerability scanning for the same application commit.
This is deployment evidence for the reviewed application code, not the final
production release designation; publish and pin a new digest if application
code changes before deployment.

The current chat release revision
`eec2d792780622fac50caabe62e213106cb547e4` was imported as:

```text
oci.bjoroy.me/sproyt/sproyt:eec2d792780622fac50caabe62e213106cb547e4
sha256:08ca736359ed085ff0ba5ed103a48f76ea35ac39d7a03a8166507ce59e8a0ae2
```

Registry inspection reports `linux/arm64`, non-root user `65532:65532`, port
9010 and the matching OCI revision label. GitOps commit
`62bcdee990d3350611253e33db77f6f015be91ba` pins this exact revision and
digest. Argo reconciled it automatically on 2026-07-17. The real public
Cloudflare route then passed 20/20 readiness samples with zero failures and a
maximum response time of 1.884 seconds; the new global CSP, no-sniff, referrer
and permissions headers proved the new application active. Authenticated
browser acceptance remains to be recorded.

The current Heart cluster image from merged Heart PR 4 revision
`dc8b2b139088fa982d4c559097ea4b4e05a6469e` was imported into Zot as:

```text
oci.bjoroy.me/sproyt/heart:dc8b2b139088fa982d4c559097ea4b4e05a6469e
sha256:6361420a90a18b07f7e4a14a135a0f527a006ccfb503f6ccb44e36ce45b27bc2
```

Registry inspection reports `linux/arm64`, non-root user `65532:65532`, port
3000, and the matching OCI revision label. Heart Actions run `29635618696`
passed migrations, idempotent immutable definition deployment, format, clippy, workspace
tests, API readiness/container smoke, SBOM generation and the high/critical
vulnerability gate. The static scratch runtime is approximately 11 MB and the
matching local Grype 0.110.0 scan reported no vulnerabilities. Target-cluster
activation and rolling-update acceptance remain.

## Gates that cannot be completed from this checkout

The remaining production gates are intentionally environment-owned:

1. Authentik client/redirect confirmation and live login/logout/renewal tests.
2. Target PostgreSQL role/database provisioning and Helm migration/install.
3. Manual Ingress/TLS and external callback reachability.
4. Production-sized load, operational ownership, retention/privacy acceptance,
   rollout observation and rollback evidence.

Heart is a private repository, so Sproyt's repository-scoped GitHub token
cannot check it out in CI. The real cross-repository contract remains the
explicit `tools/test-heart-contract.ps1` pre-release check. Heart revision
`dc8b2b139088fa982d4c559097ea4b4e05a6469e` passed the idempotent-start,
reconciliation, receive, migration, readiness and hardened ARM64 container
checks in PR run `29635618696` on 2026-07-18.
Do not add an implicit cross-repository token; if this check moves into CI,
configure a narrowly scoped read-only credential explicitly.

Track these as release evidence rather than weakening the automated contracts.
