# Architecture

This is the living architecture document for Sproyt. Keep it updated as decisions become code.

## Goals

Sproyt is a chat application for channels, human users, and agentic users. It should run well locally, in containers, and later in production environments managed by Docker, Podman, Rancher, or Kubernetes-style orchestration.

The main engineering bias is to use Rust idiomatically, lean on the type system, keep ownership and borrowing explicit, and avoid `unsafe` unless there is a clear and reviewed need.

## Baseline Decisions

### Language

- Rust edition 2024.
- Intended pinned toolchain: Rust 1.96.0.
- `unsafe` is not part of the normal application design.
- Prefer small domain types over stringly typed application state.

### Frontend

- Use Leptos for Rust/WASM frontend development.
- Default to SSR plus hydration, not pure CSR.
- Use modern browser assumptions: recent Edge, Safari, Chrome, Brave, and preferably Firefox.
- Avoid browser-only APIs in code that can run during SSR.
- Keep hydration-safe HTML.
- Chat message bodies are stored and transported as text. Rendering concerns such as Markdown, code blocks, and Mermaid diagrams belong in clients/view adapters.
- Clients should offer both rendered view mode and raw/source mode.

### Backend

- Use axum on Tokio.
- Use Tower/Tower HTTP for middleware concerns.
- Keep application state typed and explicit.
- Prefer ordinary HTTP APIs and WebSocket endpoints over framework magic when domain boundaries matter.

### Realtime

- Use WebSocket for bidirectional chat traffic.
- Persist messages before broadcasting realtime events.
- Treat in-process broadcast channels as an optimization, not durable delivery.
- Leave room for PostgreSQL `LISTEN/NOTIFY`, NATS, Redis, or another broker if multi-instance fanout needs it later.

### Chat Core

- The chat core is independent of the user interface.
- The first implementation uses a mailbox/actor pattern: external clients send typed commands to a bounded Tokio `mpsc` queue, and the chat actor serializes state changes per process.
- Each channel has a bounded broadcast queue for outbound events and a bounded in-memory recent history buffer.
- WebSocket clients, future Leptos UI code, and agent clients should all connect through the same chat engine contract.
- Slow consumers are allowed to lag; the first version reports skipped broadcast events instead of blocking the entire channel.
- This is intentionally single-process for now. Durable storage, replay from database, and cross-process fanout belong in later milestones.

### Database

- Use SQLx.
- SQLite is allowed for fast local development.
- PostgreSQL is the production contract.
- CI should eventually test migrations and repository behavior against PostgreSQL even if SQLite is used locally.
- Avoid assuming SQLite and PostgreSQL SQL dialects are identical.
- Durable chat state should be modeled as users, channels, memberships, and messages.
- Message sequence is per channel, so clients and agents can reconnect and request missed messages after a known sequence.

### Identity

- Production identity is external OIDC.
- Development identity should be a separate provider implementation, selected by configuration.
- Do not mix development shortcuts into production OIDC flows.
- Secrets should come from files or orchestrator secrets where possible, not hard-coded configuration.

### External Interfaces

- WebSocket, HTTP, future Leptos server functions, and future MCP tools should be adapters over the same typed chat command/event surface.
- Protocol documentation belongs in `docs/protocol.md` once the first persistent command/event names stabilize.
- MCP should expose chat capabilities as tools over the domain service, not as a separate implementation.

### Containers

- Use OCI-compatible images.
- Keep the main image definition portable across Podman and Docker.
- Prefer a primary `Dockerfile` for broad ecosystem compatibility.
- Use Compose for local and prod-like orchestration, not as the production source of truth.
- Runtime containers should be stateless except for explicitly mounted dev data.

### Native Build Tooling

- LLVM/clang is welcome in the local and container build toolchain.
- Do not make application code depend on local workstation-only tools.
- Prefer Rust-native crates, but keep clang available for crates that need C/C++ compilation or bindgen-style workflows.

## Proposed Workspace Shape

The likely long-term workspace shape:

```text
crates/
  app/          Leptos UI and shared frontend routes/components
  server/       axum server, HTTP/WebSocket endpoints, SSR integration
  domain/       core chat types and business rules
  db/           SQLx repositories and migrations
  auth/         OIDC and development auth providers
  config/       typed configuration loading
```

This may start simpler and split only when the code earns the boundary.

## First Milestones

1. Hello Chat: minimal runnable Rust server.
2. Chat mailbox core: typed commands, bounded queues, multi-channel fanout, WebSocket adapter.
3. Leptos shell: SSR plus hydration with a static first chat screen.
4. Database skeleton: SQLx, migrations, SQLite and PostgreSQL profiles.
5. Auth skeleton: dev identity and OIDC configuration shape.
6. Chat MVP: durable channels, message persistence, WebSocket updates.
7. Container baseline: portable multi-stage image and Compose files.

See [docs/persistence-plan.md](docs/persistence-plan.md) for the detailed persistent chat and external interface plan.

## Open Questions

- Exact channel model: public, local, private, membership, moderation.
- Agent user model: permissions, auditability, provenance, and rate limits.
- Message model: edits, deletes, reactions, attachments, threads.
- Retention and compliance requirements.
- Deployment target: single node, Kubernetes/Rancher, or managed platform.
