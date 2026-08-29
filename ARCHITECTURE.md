# Architecture

This is the living architecture document for Sproyt. It describes the code
that runs today; update it when an architectural decision becomes code.

## Goals

Sproyt is a Rust chat application for private friend circles, human users, and
least-privileged agent users. It runs locally, in OCI containers, and in a
Kubernetes-style production deployment.

The engineering bias is idiomatic Rust, explicit ownership and typed domain
boundaries. `unsafe` is not part of the normal application design.

## Runtime shape

```text
Browser client / WebSocket client / MCP client
                    |
              axum adapters
                    |
      typed services and domain policy
                    |
 SQLx repositories / auth provider / Heart gateway
                    |
       SQLite (local) or PostgreSQL (production)
```

`src/server.rs` owns application construction, routing, middleware, process
gateway construction, tracing and graceful shutdown. `AppState` holds the
typed services used by adapters. It is intentionally not a second application
layer: HTTP, WebSocket and MCP adapters call the same chat, agent, process and
notification services.

## Source layout

`src/main.rs` is the small binary entry point and hosts the included test
modules. Runtime assembly lives in `src/server.rs`; protocol and domain
implementation remain outside the HTTP adapter tree.

```text
src/
  agent.rs, auth.rs, chat.rs, notification.rs, operations.rs, process.rs
                                      typed application services
  domain/                            identifiers, models, policy, repositories
  db/                                SQLite and PostgreSQL SQLx repositories
  protocol.rs, ws.rs                 WebSocket protocol and socket runtime
  server.rs                          AppState, router, middleware, lifecycle
  web/
    assets.rs                        compile-in HTML, JavaScript and PWA assets
    browser.rs                       browser shell and invitation handling
    auth.rs                          login, callback, logout and session routes
    socket.rs                        WebSocket upgrade adapter
    system.rs                        version, readiness and security middleware
    http.rs                          shared HTTP auth/query/error contract
    account.rs                       export, client events and push preferences
    media.rs                         media upload, download and preview routes
    processes.rs                     Heart process and feature routes
    agents.rs                        agent grant and approval routes
    mcp.rs                           MCP HTTP adapter and tool dispatch
```

The browser client is dependency-light and compiled into the binary from
`web/assets.rs`. It serves the HTML shell, client-store JavaScript, service
worker, manifest and logos; `web/browser.rs` fills the authenticated shell
with its nonce, client asset revision and feature visibility. A future client
split must retain the same HTTP, WebSocket and typed-service contracts rather
than duplicate domain logic.

### Browser client migration to Rust/WASM

The browser is split at the browser capability boundary. TypeScript owns DOM,
focus, fetch, WebSocket objects, HttpOnly-cookie requests, IndexedDB, browser
locks, broadcasts, timers and PWA lifecycle events. Rust owns deterministic
policy and state transitions. Credentials are never passed into WASM.

`crates/sproyt-client-core` is the first portable client-policy slice. It owns
the durable-send admission and lifecycle model and builds both natively and for
`wasm32-unknown-unknown`. A fingerprinted immutable WASM route exposes its
send-admission decision through a deliberately small integer ABI. The
TypeScript adapter retains a fixture-verified fallback for browsers that cannot
load the asset, but Rust is the primary policy owner. It re-checks transport,
subscription and session-handoff state after the asynchronous IndexedDB write,
so a message is either dispatched with its original request id or remains
durably queued; the composer is never left waiting for a request that did not
reach a socket.

The migration is deliberately staged:

1. Complete: fingerprinted, CSP-safe WASM asset and binding pipeline; send
   admission now uses the Rust export with a bounded compatibility fallback.
2. Complete: refresh, startup and recovery decisions are Rust policy. TypeScript
   still executes `/auth/session`, `/auth/refresh`, locks, timers, broadcasts,
   visibility checks and navigation as browser effects. A failed session probe
   is distinct from an explicit authentication rejection, so a network outage
   after wake cannot become an interactive login or page replacement.
3. Move reconnect, socket-generation, handoff and desired-subscription policy
   into the same core while TypeScript retains the concrete WebSocket handles.
4. Move durable-command lifecycle and selected protocol validation only after
   browser/PWA tests prove resume, token rotation and idempotent replay.

DOM rendering and browser capability adapters are not migration targets unless
measurements show a concrete benefit. The goal is one testable owner for client
state, not a maximal WASM bundle.

`assets/client-store.js` owns the application-state/mailbox boundary, while
`assets/index.html` contains the current view and interaction layer. The
fingerprinted client module is immutable; the compatibility URL and service
worker are revalidated, and the authenticated HTML shell is never cached.

## HTTP and realtime adapters

Routes are declared once in `server.rs`. The route-specific code belongs in
`web/`, with `web/http.rs` providing the common authenticated query contract
and stable conversion of auth, chat and repository failures to HTTP responses.
The global security, request-ID, tracing, metrics and body-limit middleware is
assembled in `server.rs`.

WebSocket is the bidirectional chat interface. Messages are persisted before
realtime publication; in-process broadcast queues optimize delivery but are
not the durable source of truth. PostgreSQL `LISTEN/NOTIFY` supports
cross-replica wake-up and clients recover from durable channel sequence data.

MCP is an HTTP adapter for scoped agents, not a second chat implementation.
Its tools authorize through the same agent grants and call the same chat and
process services as other adapters.

Dependencies point inward: web adapters depend on typed services and domain
contracts; services depend on domain types and repository traits; database
adapters implement those traits; `server.rs` composes the graph. Domain code
must not depend on HTTP, browser or database-specific types, and handlers must
not contain SQL.

## Persistence and identity

SQLx is the persistence boundary. SQLite is supported for local development;
PostgreSQL is the production contract. Repository behaviour is exercised
against the supported backends and database migrations run as an explicit
command or deployment job, never during normal application startup.

Production identity is external OIDC. Development identity is an explicit
auth mode and is rejected in production. Encrypted, HttpOnly session cookies
are validated by HTTP and WebSocket authentication; refresh tokens are never
exposed to browser JavaScript.

## Tests and quality gates

Unit and adapter-capacity tests are included from `src/main_tests/` so they
exercise the assembled router and the compile-in browser assets. The suite
covers HTTP, WebSocket, MCP, browser asset contracts, repository behaviour,
security headers, auth/session behaviour, media and process flows.

Before a refactor or release, run:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

See [docs/development.md](docs/development.md) for the full local and CI gate,
and [docs/protocol.md](docs/protocol.md) for the stable WebSocket and MCP
contracts.

## Deployment

OCI-compatible images support local Podman/Docker-style use and Helm-managed
deployment. Runtime containers are stateless apart from explicitly mounted
local development data; PostgreSQL and explicit external services hold durable
production state. Operational procedures, readiness, metrics, migration,
backup/restore and rollback evidence live with the release documentation.
