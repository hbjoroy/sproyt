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
TypeScript change. It gives clearer diagnostics than the bundler.

The portable browser-policy core is checked natively by the workspace tests and
is compiled to WebAssembly by the application build. Install the target once
for local Rust builds and keep both targets green:

```powershell
rustup target add wasm32-unknown-unknown
cargo test -p sproyt-client-core
cargo check -p sproyt-client-core --target wasm32-unknown-unknown
```

Cargo embeds the core at a fingerprinted, immutable `application/wasm` route.
The authenticated shell publishes that URL as metadata and the TypeScript
adapter loads it without inline script. Send admission uses the Rust export
after initialization and retains the fixture-verified TypeScript policy only
as a compatibility fallback if the asset or ABI cannot load. Session startup,
refresh classification and authentication recovery use the same shared WASM
instance. HTTP requests, locks, storage, timers, visibility and navigation
remain TypeScript effects. Do not add browser APIs or credentials to the Rust
crate; browser effects stay in typed TypeScript adapters.

The browser page loads the fingerprinted `app.js` module and client-policy WASM.
`client-store.js` is built and served temporarily only for pages that were
already open during the rollout; new pages do not request it. `app.ts` is a
deliberately behaviour-preserving extraction of the former inline module. The
next frontend stage splits it into typed state, transport, and DOM modules; do
not add new behaviour to the transitional file.

## Browser contracts

The Playwright smoke contract starts the ordinary Rust server with development
authentication and a dedicated local SQLite database. It verifies that the
browser receives the CSP-protected module, establishes a real WebSocket
connection, and sends a message through the rendered UI.

Install Chromium once. For a focused run, build the frontend and pass the
relevant Playwright spec to the runner from `frontend/`.

```powershell
npx --prefix frontend playwright install chromium
Push-Location frontend
npm run build
node scripts/test-e2e.mjs smoke.spec.ts --project chromium
Pop-Location
```

Each run reserves a free local port and creates an isolated database below
`frontend/.playwright/run-*/sproyt.sqlite`. The harness stops its complete
server process tree and removes that directory after success or failure. The
full local entry point is `npm --prefix frontend run test:e2e`. CI installs the
engine each job uses and collects timing reports, plus traces on failures.

## Local and pull request checks

Choose local checks for the changed area using the
[CI change matrix](ci-iteration-strategy.md#change-matrix). For a Rust change,
start with formatting and the relevant package or test filter. To reproduce
the selected backend CI gate locally, use the full commands below.

```powershell
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
```

The test suite keeps tests that do not need an external database fast and
isolated. Chat-domain conformance runs against the in-memory adapter, the full
chat/process/agent contract runs against SQLite locally, and the selected
PostgreSQL CI job runs it against PostgreSQL 17.

The `CI` workflow selects pull request checks from the full change range and
runs the identity-provider contract scripts in Quality after successful
selection. Its push trigger is limited to `v*` tags. The separate native
interoperability workflow runs on `main` pushes that match its path filters. See the
[delivery phases](ci-iteration-strategy.md#delivery-phases) for scheduled
regression, release tags and manual dispatch, and the
[release checklist](release-checklist.md) for publication of an exact revision.

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

The CI security audit runs `cargo audit` against `Cargo.lock` for manual
dispatches, weekly CI and `v*` release tags. The local equivalent uses the
same advisory exception.

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
