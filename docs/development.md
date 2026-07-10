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

The test suite must keep tests that do not need a database fast and isolated.
Repository conformance tests will run against the in-memory adapter and SQLite
locally, while CI also runs the PostgreSQL contract.

## PostgreSQL schema check

Start a PostgreSQL 17 instance, then apply the dialect-specific migrations in
order with errors enabled. For example:

```powershell
$env:PGPASSWORD = 'sproyt-dev'
psql --host 127.0.0.1 --username sproyt --dbname sproyt --set ON_ERROR_STOP=1 --file migrations/postgres/0001_chat_core.sql
```

Shared migrations are immutable. Add a new numbered migration for every schema
change after a migration has reached `main`.

The application exposes an explicit migration command for local use and the
future Kubernetes migration Job:

```powershell
cargo run -- migrate
```

Application replicas do not apply schema changes implicitly when starting.

## Dependency audit

CI runs `cargo audit` against `Cargo.lock`. To run the same check locally:

```powershell
cargo install cargo-audit --locked
cargo audit
```

Do not suppress an advisory without a documented risk assessment and a linked
tracking issue.
