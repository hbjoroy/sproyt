# Development

Sproyt uses the Rust toolchain pinned in `rust-toolchain.toml`. `rustup` selects
it automatically from the repository root.

## Frontend build

The browser client is written in TypeScript under `frontend/` and is compiled
to a local, generated asset before Rust embeds it. Install the pinned Node
major version from `.node-version`, then run:

```powershell
npm --prefix frontend ci
npm --prefix frontend run check
npm --prefix frontend run build
```

`frontend/dist/` is generated only for explicit frontend builds and is
deliberately not committed. Cargo directs its automatic frontend build into its
private `OUT_DIR`, avoiding generated files and parallel-build races in the
working tree. When developing locally, install the exact Node.js and npm
versions declared by the frontend once before starting the Rust server:

```powershell
npm --prefix frontend ci
cargo run
```

Cargo runs the local frontend build automatically whenever its TypeScript
sources or lockfile change. Run the explicit TypeScript check before sharing a
change; it gives clearer diagnostics than the bundler.

## Browser contracts

The Playwright smoke contract starts the ordinary Rust server with development
authentication and a dedicated local SQLite database. It verifies that the
browser receives the CSP-protected module, establishes a real WebSocket
connection, and sends a message through the rendered UI.

Install Chromium once, then run the contract:

```powershell
npx --prefix frontend playwright install chromium
npm --prefix frontend run test:e2e
```

Each run reserves a free local port and creates an isolated database below
`frontend/.playwright/run-*/sproyt.sqlite`. The harness stops its complete
server process tree and removes that directory after success or failure. CI
installs Chromium with its Linux dependencies and uploads trace/report material
only after a failing browser contract.

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
contract. Non-documentation pushes to `main` run the same fast gates, so an
ordinary merge does not wait for two image builds and an ephemeral cluster.
The dependency audit, backup/restore drill, ARM64 and native image builds, Helm
verification, kind install/scale/rollback, SBOM generation, and vulnerability
scan form the full release gate. It runs every Monday at 03:17 UTC, for `v*`
release tags, and when an operator starts the CI workflow manually. Run it
explicitly for the exact revision being considered for a production release;
scheduled evidence is a regression sentinel, not authorization to deploy a
different commit.

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
