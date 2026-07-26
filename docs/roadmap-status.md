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
| S-07 | Implemented; rollout pending | Stable connection IDs, PostgreSQL presence leases with heartbeat/expiry, advisory-lock protected handoff, recoverable cluster-wide `LISTEN/NOTIFY`, atomic presence handoff and full two-process live-message/restart/catch-up regressions, lag recovery and two-replica Helm smoke | Deploy migration 0017 and verify two-user presence and messages through a session refresh in the target cluster |
| S-08 | Implemented | `sproyt.chat.v1` envelopes plus WebSocket reconnect, idempotency, error and lag tests | None |
| S-09 | Backend slice implemented; UI replacement required | `two_users_complete_private_circle_slice_with_unread_reconnect`; DOM-safe Markdown/raw view; strict, exact-version Mermaid rendering; CSP regression | Superseded technical demonstration UI must be replaced by S-21 through S-24 |
| S-10 | Implemented | `AuthService`, deterministic development principals and production/dev rejection tests | None |
| S-11 | Implemented and active | OIDC discovery, PKCE, state, nonce, encrypted session, optional refresh-token handling, logout, key-rotation and revoked-user contracts in `auth.rs`; live Authentik login and identity verified through Cloudflare; production callback and issuer pinned; 2026-07-18 browser session remained connected through periodic provider revalidation after the no-refresh-token loop fix | Complete destructive logout/revocation and key-rotation operations as a scheduled production security drill |
| S-12 | Implemented | Central `Policy`, executable authorization matrix and mutation-audit migrations | Review target-environment audit retention/access |
| S-13 | Implemented and published | Pinned scratch ARM64 image, restricted runtime contract, SBOM and vulnerability CI; registry evidence below | Retain CI and registry evidence with the release record |
| S-14 | Implemented | Helm delivery verifier and kind install, migration, two-replica scale and rollback gate | Install in the ARM64 target cluster and provide manual Ingress |
| S-15 | Baseline implemented, sign-off pending | SLOs/runbook, Prometheus/Grafana resources, recovery CI, SQLite capacity/restart gate, PostgreSQL two-process realtime/restart/catch-up gate, safe MCP latency/idempotency evidence tool, owner-authorized audited circle deletion, and snapshot-consistent `sproyt.user-export.v1` self-export | Run target-sized MCP/browser load, name owners, accept privacy/retention and sign off rollout |
| S-16 | Implemented and active | Sproyt `ProcessGateway` contract, clean-container Heart start/replay/reconciliation/receive/completion contract passed 2026-07-19 against `dc8b2b1`, green `ea-heart-client` contract/CI at `9510cc4`, and private Heart component active in the same Sprøyt Helm/Argo release | Exercise a real process start and status round-trip in the target cluster |
| S-17 | Implemented | Durable process link/event/outbox state, stable command receipts, Heart `Idempotency-Key`/`X-Heart-Client`, exact replay, bounded retry and unknown-outcome lookup at Heart `dc8b2b1` | Exercise restart/rolling update in the target cluster |
| S-18 | Pilot active; environment acceptance pending | Event-planning definition, browser/MCP flow tests, kill switch, Heart receive/idempotent-start contract, HTTP 503 failure-isolation regression proving ordinary chat remains durable and queued process work recovers exactly once, ordered migration plus immutable definition bootstrap, same-release private Service/Deployment/PDB/NetworkPolicy, and successful target-cluster activation after database/schema ownership repair on 2026-07-19 | Exercise the pilot end to end and verify restart/rolling-update in the target cluster |
| S-19 | Implemented and production-exercised | Agent profiles, scoped grants, expiry/revocation, database-authoritative cross-replica rate limits, provenance and audit tests, plus an owner-visible 30-minute channel-bounded credential/revocation flow with no-store responses; on 2026-07-21 an owner issued and revoked a production credential on revision `41dd37d`, and the revoked credential immediately returned HTTP 401 | Retain the credential/revocation exercise in recurring release evidence |
| S-20 | Implemented; read-only production exercise complete | MCP transport checks, WebSocket/MCP adapter-conformance tests, bounded production MCP load-evidence tool, and owner UI granting only read-history/send-message scopes to the active moderated channel; on 2026-07-21 the short-lived credential listed exactly its granted private channel before revocation, without writing to a real friends channel | Run the bounded write/load tool in a dedicated target-environment channel and retain non-secret latency/idempotency evidence |
| S-21 | Implemented; renewed-session acceptance pending | Production session boundary; encrypted session; refresh about 60 seconds before expiry; overlapping WebSocket handoff after cookie rotation; PostgreSQL-backed presence keeps the old and replacement connection in one atomic global lease set; periodic provider revalidation; profile/logout shell; no simulated production identity; live Authentik identity verified | Deploy migration 0016 and verify two-user message/presence flow through a complete refresh interval, then retain logout/revocation in the scheduled security drill |
| S-22 | Implemented and active | Responsive navigation/timeline/composer; durable human-readable sender snapshots; circle-grouped channels; visible keyboard focus; bounded exponential reconnect; draft preservation; read markers/unread badges; accessible mobile drawer | Complete two-user usability and presence sign-off after the overlapping session-handoff rollout |
| S-23 | Implemented; rollout pending | Guided circle creation with automatic `Prat` channel; selected-circle invite action; shareable invite links; OIDC-signed return path; copy/fallback UX; link recognition and actionable invalid/expired feedback; creator-only initial channel membership; discoverable open circle channels; explicitly invited private-channel members; global `general` membership; two-user authorization contracts | Exercise channel creation, open joining and private invitation through production Authentik |
| S-24 | In progress | Heart/process and agent credential controls have independent server-rendered flags that default off; Heart is an active private component of the same Sprøyt release; CSP, focus, mobile drawer, reconnect and capacity gates; immutable ARM64/GitOps rollout; live OIDC identity | Complete two-user/session acceptance, Heart end-to-end and failure-isolation passes, dedicated-channel MCP load evidence, then sign off private beta |

## Current immutable image evidence

The current publicly observed chat revision
`d945e62c9eee3b9470383bc416e301b878e5c836` is published as:

```text
oci.bjoroy.me/sproyt/sproyt:d945e62c9eee3b9470383bc416e301b878e5c836
sha256:7a3cbef9264ee7e0818cb99fdf820e7dc7abdaca771807494ae45d392435a1c0
```

GitOps commit `148e1ab` pins this binary digest and chart revision. Main CI run
`29765526172` passed format/lint/test, PostgreSQL contract including the atomic
two-replica presence handoff, backup/restore,
dependency audit, ARM64/native image delivery, Helm/kind rollout, SBOM and
vulnerability gates. The registry digest was read back after authenticated Zot
import, and the GitOps Helm render pins it for both the migration Job and the
application pods. Public `/versionz` reported the exact application revision
after Argo autosync. Following the rollout transition, 12 consecutive public
`/versionz` and `/healthz` pairs returned HTTP 200 in 0.18–0.46 seconds. The
release overlaps old and refreshed WebSockets and keeps
both connections in one PostgreSQL-backed presence lease set, so the new
connection can restore subscriptions before the old connection retires without
a false leave transition. Direct target-cluster Argo health and two-user
production acceptance through a complete refresh interval remain open and are
not inferred from CI or single-client public readiness.

On 2026-07-20, the public production boundary returned `ok` from `/healthz`,
`ready` from `/readyz`, redirected anonymous traffic to `/auth/login`, and
redirected login to the configured Authentik authorization endpoint with PKCE.
OIDC discovery reported the exact configured issuer plus authorization-code
and refresh-token support. This proves the public/OIDC boundary, but does not
replace direct Argo health or two-user presence acceptance.

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
2. Target-cluster Heart restart, rolling-update and failure-isolation evidence.
3. Manual Ingress/TLS and external callback reachability.
4. Production-sized load, operational ownership, retention/privacy acceptance,
   rollout observation and rollback evidence.

Heart is a private repository, so Sproyt's repository-scoped GitHub token
cannot check it out in CI. The real cross-repository contract remains the
explicit `tools/test-heart-contract.ps1` pre-release check. Heart revision
`dc8b2b139088fa982d4c559097ea4b4e05a6469e` passed the idempotent-start,
reconciliation, receive, migration, readiness and hardened ARM64 container
checks in PR run `29635618696` on 2026-07-18.
The checkout-level contract was rerun from forced-clean containers on
2026-07-19 and completed an idempotent process start, unknown-outcome lookup,
receive correlation and final completion. The contract tool now bounds
PostgreSQL/API readiness and every HTTP operation, rejects failed `wslc`
creation steps, and removes stale containers before collecting evidence.
Do not add an implicit cross-repository token; if this check moves into CI,
configure a narrowly scoped read-only credential explicitly.

Track these as release evidence rather than weakening the automated contracts.
