# Sproyt

Sproyt is a Rust chat service for private friend circles, human users, and
least-privileged agent users.

The service is container-first. SQLite keeps local development light, while
PostgreSQL is the production persistence and cross-replica fan-out contract.

## Current Status

The repository contains the complete application path for durable private
circles: SQLx repositories, `sproyt.chat.v1` WebSockets, development auth,
OIDC authorization-code flow, OCI delivery, a Helm chart, optional Heart
process orchestration, and an MCP adapter for scoped agents. The browser client
is currently a dependency-light inline view adapter; a later Leptos split must
continue to use the same HTTP/WebSocket application contracts.

Production activation still requires environment-owned evidence: the actual
Authentik provider slug/client registration, registry-pushed image digest,
cluster-specific secrets/TLS, and the operational sign-offs listed in
[`docs/release-checklist.md`](docs/release-checklist.md).

## Direction

- Rust edition 2024, pinned to Rust 1.96.0 when the local toolchain is available.
- A replaceable browser view adapter; Leptos SSR/hydration remains the intended
  frontend split when a separate app crate becomes useful.
- axum, Tokio, and Tower for the backend.
- SQLx for database access.
- SQLite as the simple local development database.
- PostgreSQL as the production database contract.
- External OIDC for identity, with an explicit development auth mode.
- WebSocket for chat realtime events.
- OCI-compatible containers that work with Podman, Docker, Rancher, and Kubernetes-style deployments.

See [ARCHITECTURE.md](ARCHITECTURE.md) for the living architecture notes.

See [docs/persistence-plan.md](docs/persistence-plan.md) for the planned persistent channel, membership, message, and agent/API interface work.

See [docs/protocol.md](docs/protocol.md) for the stable WebSocket and agent/MCP
protocol contract.

See [docs/roadmap.md](docs/roadmap.md) for the phased delivery plan from the
current prototype through durable private chat, OIDC, Kubernetes, Heart process
orchestration, and agent participation.

See [docs/roadmap-status.md](docs/roadmap-status.md) for the requirement-by-
requirement evidence index and the remaining environment-owned activation
gates.

See [docs/development.md](docs/development.md) for the local and CI quality
gates used for contributions.

For copy/paste startup instructions, including the existing cluster PostgreSQL,
private `oci.bjoroy.me` images, namespaced pull secrets, Helm, and manual
Ingress, see [docs/running.md](docs/running.md).

## Prerequisites

- Rust 1.96.0, via rustup.
- LLVM/clang is useful for Rust crates that build or bind to C/C++ code.
- An OCI runtime such as `wslc`, Docker, or Podman for image/database tests.
- Helm for local chart validation.

## Run Sproyt locally

Create the local data directory, apply migrations, and start the development
server:

```powershell
New-Item -ItemType Directory -Force .local | Out-Null
cargo run -- migrate
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

Open the same URL in multiple browser tabs and use different development
participant names. State is stored through the configured SQLx repository.

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
ws://127.0.0.1:9010/ws?participant=alice
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
| `SPROYT_SESSION_PREVIOUS_KEYS` | unset | Comma-separated prior session keys accepted only for decrypting cookies during a rotation window. |
| `SPROYT_HEART_URL` | unset | Heart API root. When unset, ordinary chat runs normally and no outbox worker calls Heart. |
| `SPROYT_MCP_ALLOWED_ORIGINS` | unset | Exact, comma-separated browser origins allowed to call `/mcp`. Requests without an `Origin` header remain valid for non-browser MCP clients; browser-origin requests fail closed when unset. |
| `SPROYT_WS_IDLE_TIMEOUT_SECONDS` | `60` | Close WebSockets with no inbound frame or application heartbeat; valid range is 5–3600 seconds. |

OIDC uses discovery and does not hard-code Authentik endpoints. Register the
exact redirect URL with the provider. Login starts at `/auth/login`, the
provider returns to `/auth/callback`, and `/auth/logout` clears the local
session. The provider must publish a `userinfo_endpoint`; authenticated HTTP
and WebSocket handshakes validate the encrypted session's access token there,
bind the response to the expected subject, and therefore fail closed for a
revoked or disabled user. Open WebSockets repeat that validation every 30
seconds and close with policy code `1008` when authentication is no longer
valid. Request the provider's `offline_access` scope so `/auth/refresh` can
rotate refresh tokens and renew the encrypted HttpOnly session. The browser
uses a cross-tab lock and schedules renewal from the access-token lifetime;
tokens are never exposed to JavaScript. Session lifetime never exceeds the
current ID/access-token lifetime, and an expired session cannot be revived
through the refresh endpoint; the user must begin a new authorization flow.
In production, startup requires an HTTPS issuer matching
`https://identity.limani-parou.com/application/o/<provider-slug>/`, an HTTPS
callback ending in `/auth/callback`, a post-logout URL on the same origin, and
unpadded URL-safe base64 session keys decoding to exactly 32 bytes. Invalid
values fail before provider discovery or socket binding.
Logout uses the discovered `end_session_endpoint` with the registered client
and post-logout redirect when the provider supports RP-initiated logout; local
cookie clearing and the configured application redirect remain the fallback.
All Kubernetes replicas can validate cookies with the same session key without
server-local session state. Rotate that key by moving the old value to
`SPROYT_SESSION_PREVIOUS_KEYS`, deploying a new `SPROYT_SESSION_KEY`, waiting
at least the configured provider token lifetime, and then removing the old key.

Database migrations run only through `sproyt migrate` (the Compose migration
service or Helm pre-install/pre-upgrade Job). Application pods do not mutate
the production schema during normal startup.
