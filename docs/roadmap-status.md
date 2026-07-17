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
| S-09 | Implemented | `two_users_complete_private_circle_slice_with_unread_reconnect`; DOM-safe Markdown/raw view; strict, exact-version Mermaid rendering; CSP regression | Human usability check |
| S-10 | Implemented | `AuthService`, deterministic development principals and production/dev rejection tests | None |
| S-11 | Implemented, activation pending | OIDC discovery, PKCE, state, nonce, encrypted session, refresh, logout, key-rotation and revoked-user contracts in `auth.rs` | Create the Authentik client and run the contract against `identity.limani-parou.com` |
| S-12 | Implemented | Central `Policy`, executable authorization matrix and mutation-audit migrations | Review target-environment audit retention/access |
| S-13 | Implemented and published | Pinned scratch ARM64 image, restricted runtime contract, SBOM and vulnerability CI; registry evidence below | Retain CI and registry evidence with the release record |
| S-14 | Implemented | Helm delivery verifier and kind install, migration, two-replica scale and rollback gate | Install in the ARM64 target cluster and provide manual Ingress |
| S-15 | Baseline implemented, sign-off pending | SLOs/runbook, Prometheus/Grafana resources, recovery CI, WebSocket capacity gate, owner-authorized audited circle deletion, and snapshot-consistent `sproyt.user-export.v1` self-export | Production-sized load, owners, privacy/retention and rollout sign-off |
| S-16 | Implemented | Sproyt `ProcessGateway` contract, real Heart contract at `3f06424`, and the green `ea-heart-client` receive-message contract/CI at `9510cc4` | Configure the deployed Heart endpoint |
| S-17 | Implemented | Durable process link/event/outbox state, stable command receipts, Heart `Idempotency-Key`/`X-Heart-Client`, exact replay, bounded retry and unknown-outcome lookup at Heart `3f06424` | Exercise restart/rolling update in the target cluster |
| S-18 | Pilot implemented; rollout pending | Event-planning definition, browser/MCP flow tests, kill switch and real Heart receive/idempotent-start contract | Exercise restart/rolling update in the target cluster |
| S-19 | Implemented | Agent profiles, scoped grants, expiry/revocation, database-authoritative cross-replica rate limits, provenance and audit tests | Issue and revoke a target-environment agent credential |
| S-20 | Implemented | MCP transport checks and WebSocket/MCP adapter-conformance tests | Exercise through the production endpoint after S-19 activation |

## Current immutable image evidence

On 2026-07-17, the reviewed application commit
`0fcd6fc0a87e536ed02857f4ba69576eaa6b3966` was imported into Zot as:

```text
oci.bjoroy.me/sproyt/sproyt:0fcd6fc0a87e536ed02857f4ba69576eaa6b3966
sha256:42beccf9200b9f121660474eb7af204984ddf85cfffdb75ed1f082075687dafe
```

The registry config reports `linux/arm64`, non-root user `65532:65532`, and OCI
revision label `0fcd6fc0a87e536ed02857f4ba69576eaa6b3966`. GitHub Actions run
`29509852915` passed format/lint/test, PostgreSQL contract, backup/restore,
dependency audit, ARM64 build, Helm rendering, kind install/scale/rollback,
SBOM generation and vulnerability scanning for the same application commit.
This is deployment evidence for the reviewed application code, not the final
production release designation; publish and pin a new digest if application
code changes before deployment.

## Gates that cannot be completed from this checkout

The remaining production gates are intentionally environment-owned:

1. Authentik provider/client registration and live login/logout/renewal tests.
2. Target PostgreSQL role/database provisioning and Helm migration/install.
3. Manual Ingress/TLS and external callback reachability.
4. Production-sized load, operational ownership, retention/privacy acceptance,
   rollout observation and rollback evidence.

Heart is a private repository, so Sproyt's repository-scoped GitHub token
cannot check it out in CI. The real cross-repository contract remains the
explicit `tools/test-heart-contract.ps1` pre-release check. Heart revision
`3f06424673c0ceeca6e40a528e6b94b225ce2984` passed the idempotent-start,
reconciliation and receive checks on 2026-07-16.
Do not add an implicit cross-repository token; if this check moves into CI,
configure a narrowly scoped read-only credential explicitly.

Track these as release evidence rather than weakening the automated contracts.
