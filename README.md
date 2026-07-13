# Sproyt

Sproyt is an early Rust chat application for human and agentic users.

The project direction is Rust end to end: a Rust backend, a Rust/WASM frontend with Leptos, container-first delivery, and a database setup that is light in development but production-ready with PostgreSQL.

## Current Status

This repository is at the "Hello Chat" stage. The current runnable program uses axum, Tokio, WebSocket, and a small mailbox-based chat core. Leptos, SQLx, OIDC, and containers are planned next.

## Direction

- Rust edition 2024, pinned to Rust 1.96.0 when the local toolchain is available.
- Leptos for the frontend, using SSR plus hydration as the default architecture.
- axum, Tokio, and Tower for the backend.
- SQLx for database access.
- SQLite as the simple local development database.
- PostgreSQL as the production database contract.
- External OIDC for identity, with an explicit development auth mode.
- WebSocket for chat realtime events.
- OCI-compatible containers that work with Podman, Docker, Rancher, and Kubernetes-style deployments.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the living architecture notes.

See [docs/persistence-plan.md](docs/persistence-plan.md) for the planned persistent channel, membership, message, and agent/API interface work.

See [docs/protocol.md](docs/protocol.md) for the first WebSocket/agent/MCP protocol sketch.

See [docs/roadmap.md](docs/roadmap.md) for the phased delivery plan from the
current prototype through durable private chat, OIDC, Kubernetes, Heart process
orchestration, and agent participation.

See [docs/development.md](docs/development.md) for the local and CI quality
gates used for contributions.

## Prerequisites

- Rust 1.96.0, via rustup.
- The `wasm32-unknown-unknown` Rust target.
- LLVM/clang is useful for Rust crates that build or bind to C/C++ code.
- Podman or Docker.
- Later: `cargo-leptos` for the Leptos SSR/WASM build.

On this workstation, Rust 1.96.0, Podman 5.8.2, and LLVM/clang 22.1.6 are available.

## Run Hello Chat

Start the development server:

```powershell
cargo run
```

Then open:

```text
http://127.0.0.1:9010/
```

Health check:

```text
http://127.0.0.1:9010/healthz
```

Readiness and Prometheus-compatible operational metrics:

```text
http://127.0.0.1:9010/readyz
http://127.0.0.1:9010/metrics
```

Open the same URL in multiple browser tabs, choose the same channel, and use different participant names to try the in-memory chat loop.

Messages render in view mode by default. The current browser client supports:

- Markdown headings, paragraphs, blockquotes, ordered and unordered lists.
- Inline code with backticks.
- Fenced code blocks.
- Mermaid diagrams in fenced `mermaid` blocks.
- Raw mode for inspecting the exact message text.

Example message:

````markdown
# Plan

- Write the core
- Render the view

```mermaid
flowchart LR
  A[Client] --> B[Mailbox]
  B --> C[Channel]
```
````

The WebSocket endpoint is:

```text
ws://127.0.0.1:9010/ws?channel=general&participant=alice
```

To use another local port:

```powershell
$env:SPROYT_ADDR='127.0.0.1:9011'
cargo run
```

## Configuration

Runtime configuration is read from environment variables:

| Variable | Default | Meaning |
|---|---|---|
| `SPROYT_ADDR` | `127.0.0.1:9010` | HTTP/WebSocket bind address. |
| `DATABASE_URL` | `sqlite://.local/sproyt.sqlite` | Database URL. Supports `sqlite:`, `postgres://`, and `postgresql://` profiles. |
| `SPROYT_ENV` | `development` | Deployment mode: `development`, `test`, or `production`. |
| `SPROYT_LOG_FORMAT` | `pretty` | Structured log output: `pretty` or `json`. |
| `RUST_LOG` | `sproyt=info` | Tracing filter. |
| `SPROYT_AUTH_MODE` | `development` | `development` or `oidc`; development auth is rejected in production. |
| `SPROYT_OIDC_ISSUER` | required for OIDC | Authentik issuer, normally `https://identity.limani-parou.com/application/o/<provider-slug>/`. |
| `SPROYT_OIDC_CLIENT_ID` | required for OIDC | Authentik OAuth2/OIDC client ID. |
| `SPROYT_OIDC_CLIENT_SECRET` | required for OIDC | Confidential client secret; supply through a secret store. |
| `SPROYT_OIDC_REDIRECT_URL` | required for OIDC | Absolute callback URL ending in `/auth/callback`. |
| `SPROYT_OIDC_POST_LOGOUT_REDIRECT_URL` | required for OIDC | Safe redirect after local logout. |
| `SPROYT_SESSION_KEY` | required for OIDC | URL-safe base64 encoding of exactly 32 random bytes. |
| `SPROYT_HEART_URL` | unset | Heart API root. When unset, ordinary chat runs normally and no outbox worker calls Heart. |
| `SPROYT_MCP_ALLOWED_ORIGINS` | unset | Exact, comma-separated browser origins allowed to call `/mcp`. Requests without an `Origin` header remain valid for non-browser MCP clients; browser-origin requests fail closed when unset. |

OIDC uses discovery and does not hard-code Authentik endpoints. Register the
exact redirect URL with the provider. Login starts at `/auth/login`, the
provider returns to `/auth/callback`, and `/auth/logout` clears the local
session. Authentication transactions and sessions are encrypted cookies, so
all Kubernetes replicas can validate them with the same session key without
server-local session state.

The database URL is detected and typed at startup, but the current chat loop still uses the in-memory repository until the SQLx implementation is wired in.

## Near-Term Roadmap

1. Replace the dependency-free HTTP server with axum.
2. Add a Leptos SSR plus hydration shell with `cargo-leptos`.
3. Add configuration loading and structured tracing.
4. Add SQLx with SQLite dev mode and PostgreSQL prod-like mode.
5. Add durable chat domain storage: users, agent users, channels, memberships, and messages.
6. Add OIDC and a clearly separate development auth provider.
7. Add container and compose files for dev and prod-like runs.
