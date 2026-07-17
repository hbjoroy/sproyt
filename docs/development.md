# Development

Sproyt uses the Rust toolchain pinned in `rust-toolchain.toml`. `rustup` selects
it automatically from the repository root.

## Fast local quality gate

Run before opening a pull request:

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
```

The test suite keeps tests that do not need an external database fast and
isolated. Chat-domain conformance runs against the in-memory adapter, the full
chat/process/agent contract runs against SQLite locally, and CI also executes
that full contract against PostgreSQL 17.

Pull requests run the fast quality gate plus the PostgreSQL repository
contract. After merge, every accepted `main` revision runs the dependency
audit, backup/restore drill, ARM64 and native image builds, Helm verification,
kind install/scale/rollback, SBOM generation, and vulnerability scan. The full
gate can also be started manually from the CI workflow. A newer `main` revision
cancels an obsolete full run so delivery evidence follows the latest code.

## PostgreSQL schema check

Start a PostgreSQL 17 instance, then apply the dialect-specific migrations in
order with errors enabled. For example:

```powershell
$env:PGPASSWORD = 'sproyt-dev'
psql --host 127.0.0.1 --username sproyt --dbname sproyt --set ON_ERROR_STOP=1 --file migrations/postgres/0001_chat_core.sql
```

Shared migrations are immutable. Add a new numbered migration for every schema
change after a migration has reached `main`.

The application exposes an explicit migration command used locally, by
Compose, and by the Helm pre-install/pre-upgrade migration Job:

```powershell
cargo run -- migrate
```

The migration command reads only `DATABASE_URL` and `SPROYT_LOG_FORMAT`; it
does not require server bind, environment, auth, OIDC, Heart, or session
configuration.

Application replicas do not apply schema changes implicitly when starting.

## Dependency audit

CI runs `cargo audit` against `Cargo.lock`. To run the same check locally:

```powershell
cargo install cargo-audit --locked
cargo audit --ignore RUSTSEC-2023-0071
```

The single accepted advisory is scoped and reviewed in
[`security-exceptions.md`](security-exceptions.md). Do not add another ignore
without documenting its dependency path, reachable operation, severity,
compensating controls, owner, and removal condition.

Do not suppress an advisory without a documented risk assessment and a linked
tracking issue.
