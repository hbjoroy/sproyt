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
| S-11 | Implemented, activation pending | OIDC discovery, PKCE, state, nonce, encrypted session, refresh, logout, key-rotation and revoked-user contracts in `auth.rs`; CI-tested provider verifier; live Cloudflare issuer `https://sproyt-security.bjoroy.me/application/o/sproyt/` verified 2026-07-17; production callback pinned to `https://sproyt.bjoroy.me/auth/callback` | Confirm client ID/strict redirects in Authentik and run the browser login, refresh, logout, revocation and key-rotation contract |
| S-12 | Implemented | Central `Policy`, executable authorization matrix and mutation-audit migrations | Review target-environment audit retention/access |
| S-13 | Implemented and published | Pinned scratch ARM64 image, restricted runtime contract, SBOM and vulnerability CI; registry evidence below | Retain CI and registry evidence with the release record |
| S-14 | Implemented | Helm delivery verifier and kind install, migration, two-replica scale and rollback gate | Install in the ARM64 target cluster and provide manual Ingress |
| S-15 | Baseline implemented, sign-off pending | SLOs/runbook, Prometheus/Grafana resources, recovery CI, WebSocket capacity gate, safe MCP latency/idempotency evidence tool, owner-authorized audited circle deletion, and snapshot-consistent `sproyt.user-export.v1` self-export | Run target-sized MCP/browser load, name owners, accept privacy/retention and sign off rollout |
| S-16 | Implemented | Sproyt `ProcessGateway` contract, real Heart contract at `6d8a3b5`, and the green `ea-heart-client` receive-message contract/CI at `9510cc4` | Configure the deployed Heart endpoint |
| S-17 | Implemented | Durable process link/event/outbox state, stable command receipts, Heart `Idempotency-Key`/`X-Heart-Client`, exact replay, bounded retry and unknown-outcome lookup at Heart `6d8a3b5` | Exercise restart/rolling update in the target cluster |
| S-18 | Pilot implemented; rollout pending | Event-planning definition, browser/MCP flow tests, kill switch, merged Heart PR 3 at `6d8a3b5`, real Heart receive/idempotent-start contract, and private Heart cluster guide | Exercise restart/rolling update in the target cluster |
| S-19 | Implemented | Agent profiles, scoped grants, expiry/revocation, database-authoritative cross-replica rate limits, provenance and audit tests | Issue and revoke a target-environment agent credential |
| S-20 | Implemented | MCP transport checks, WebSocket/MCP adapter-conformance tests, and bounded production MCP load-evidence tool | Issue a short-lived scoped credential and exercise the production endpoint |
| S-21 | Implemented; rollout pending | Production session boundary; local encrypted-session validation; periodic provider revalidation; profile/logout shell; no simulated browser identity; 2026-07-17 desktop/mobile browser journey | Review OIDC redirect and logout against Authentik after deployment |
| S-22 | Implemented; rollout pending | Responsive navigation/timeline/composer; circle-grouped channels; human-readable sender labels; visible keyboard focus; bounded exponential reconnect; draft preservation; active-channel read markers/unread badges; 2026-07-17 desktop/mobile and real server-restart browser passes | Validate the same journey through production OIDC after deployment |
| S-23 | Implemented; rollout pending | Guided circle creation with automatic `Prat` channel; selected-circle invite action; shareable invite links; OIDC-signed return path; copy/fallback UX; link recognition and actionable invalid/expired feedback; existing two-user authorization contract plus 2026-07-17 browser journey | Exercise owner-to-fresh-user invite through production Authentik after deployment |
| S-24 | In progress | Advanced Heart/process controls are omitted from the normal UI by a server/Helm flag that defaults off; existing CSP, focus, mobile, reconnect and browser-capacity gates; read-only production rollout verifier | Run the production verifier and authenticated OIDC/two-user/mobile acceptance |

## Current immutable image evidence

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
`6355c13720c9bce94d3a55b9c485876c10b915d7` was subsequently imported as:

```text
oci.bjoroy.me/sproyt/sproyt:6355c13720c9bce94d3a55b9c485876c10b915d7
sha256:e8a6a49cbe85c7b2b9578261b8ec565742601b5288fe81d8d57046adf0c858ce
```

Registry inspection reports `linux/arm64`, non-root user `65532:65532`, port
9010 and the matching OCI revision label. GitOps commit
`9cb3db8ce5545c69026b5cf39c239ad286879dbf` pins this exact revision and
digest. Production rollout and authenticated browser acceptance remain to be
recorded; publication alone is not deployment evidence.

The Heart cluster image from merged Heart PR 3 revision
`6d8a3b50952e866e6500b5d325afeb03d3f3c7d7` was imported into Zot as:

```text
oci.bjoroy.me/sproyt/heart:6d8a3b50952e866e6500b5d325afeb03d3f3c7d7
sha256:2d244cf57cdaff1c18b21737d4d05b1a7b935ecf1d60cb3cdd337435bc3142cb
```

Registry inspection reports `linux/arm64`, non-root user `65532:65532`, port
3000, and the matching OCI revision label. Heart Actions run `29570890958`
passed migrations (including repeat execution), format, clippy, workspace
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
`6d8a3b50952e866e6500b5d325afeb03d3f3c7d7` passed the idempotent-start,
reconciliation, receive, migration, readiness and hardened ARM64 container
checks in PR run `29570890958` on 2026-07-17.
Do not add an implicit cross-repository token; if this check moves into CI,
configure a narrowly scoped read-only credential explicitly.

Track these as release evidence rather than weakening the automated contracts.
