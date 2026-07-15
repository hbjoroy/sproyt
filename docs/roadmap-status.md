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
| S-09 | Implemented | `two_users_complete_private_circle_slice_with_unread_reconnect` and safe Markdown/raw browser view | Human usability check |
| S-10 | Implemented | `AuthService`, deterministic development principals and production/dev rejection tests | None |
| S-11 | Implemented, activation pending | OIDC discovery, PKCE, state, nonce, encrypted session, refresh, logout, key-rotation and revoked-user contracts in `auth.rs` | Create the Authentik client and run the contract against `identity.limani-parou.com` |
| S-12 | Implemented | Central `Policy`, executable authorization matrix and mutation-audit migrations | Review target-environment audit retention/access |
| S-13 | Implemented and published | Pinned scratch ARM64 image, restricted runtime contract, SBOM and vulnerability CI; registry evidence below | Publish the final reviewed commit and retain its CI artifacts |
| S-14 | Implemented | Helm delivery verifier and kind install, migration, two-replica scale and rollback gate | Install in the ARM64 target cluster and provide manual Ingress |
| S-15 | Baseline implemented, sign-off pending | SLOs/runbook, Prometheus/Grafana resources, recovery CI and WebSocket capacity gate | Production-sized load, owners, privacy/retention and rollout sign-off |
| S-16 | Implemented | Sproyt `ProcessGateway` contract and `ea-heart-client` receive API at commit `2b55ae4` | Configure the deployed Heart endpoint |
| S-17 | Implemented | Process-link/event/outbox migrations, idempotency and reconciliation tests | Observe retry/reconciliation during target rollout |
| S-18 | Implemented, rollout pending | Event-planning definition, browser/MCP flow tests, kill switch and real Heart receive contract | Exercise restart/rolling update in the target cluster |
| S-19 | Implemented | Agent profiles, scoped grants, expiry/revocation, rate limits, provenance and audit tests | Issue and revoke a target-environment agent credential |
| S-20 | Implemented | MCP transport checks and WebSocket/MCP adapter-conformance tests | Exercise through the production endpoint after S-19 activation |

## Current immutable image evidence

On 2026-07-15, the reviewed application commit
`f599dc110a56f55797a21f7145210297e297062f` was imported into Zot as:

```text
oci.bjoroy.me/sproyt/sproyt:f599dc110a56f55797a21f7145210297e297062f
sha256:50501369801c7612f7d95e3c7f1957f84aa87c9553fe8ff50967c9d00d471ed7
```

The registry manifest reports `linux/arm64`. This is deployment evidence for
the reviewed application code, not the final production release designation;
publish and pin a new digest if application code changes before deployment.

## Gates that cannot be completed from this checkout

The remaining production gates are intentionally environment-owned:

1. Authentik provider/client registration and live login/logout/renewal tests.
2. Target PostgreSQL role/database provisioning and Helm migration/install.
3. Manual Ingress/TLS and external callback reachability.
4. Production-sized load, operational ownership, retention/privacy acceptance,
   rollout observation and rollback evidence.

Track these as release evidence rather than weakening the automated contracts.
