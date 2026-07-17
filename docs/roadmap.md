# Sproyt delivery roadmap

This roadmap turns Sproyt from the current in-memory chat prototype into a
small-circle chat service with durable messaging, OIDC identity, agent users,
and optional Heart process orchestration. The plan favours vertical,
deployable increments: every phase should leave the service runnable and make
the next phase additive rather than requiring a rewrite.

## Product boundary

Sproyt owns conversations: circles, channels, membership, messages, read
state, presence, permissions, and the presentation of human, agent, and
process activity.

Heart owns processes: definitions, instances, transitions, receive points,
work items, retries, and process metadata. Sproyt links conversations to Heart
instances through stable identifiers; chat messages are never stored only as
Heart metadata.

## Delivery principles

- Build one production-shaped code path with replaceable adapters. SQLite and
  development authentication are adapters, not alternate application logic.
- Keep domain commands independent of HTTP, WebSocket, SQLx, OIDC, and Heart.
- Persist before publishing realtime events.
- Make every externally triggered mutation idempotent or correlated by a
  stable request identifier.
- Prefer expand-and-contract schema and protocol changes. Do not edit shared
  migrations after release.
- Keep the application stateless outside PostgreSQL and explicit external
  services so that replicas can be added in Kubernetes.
- Ship observability, health checks, migrations, backup/restore notes, and
  rollback behaviour together with features rather than as a final hardening
  phase.
- Use feature flags for incomplete user-facing capabilities, not long-lived
  branches.

## Target architecture

```text
Browser / agent / MCP
        |
HTTP + WebSocket adapters
        |
Application command service ---- AuthProvider
        |                         Dev / OIDC
        |
ChatRepository + EventPublisher ---- ProcessGateway
        |                             heart-client
PostgreSQL / SQLite                   Heart API
        |
Postgres LISTEN/NOTIFY initially; broker only when measurements require it
```

The mailbox remains useful for bounded local work and backpressure, but the
database is authoritative. A mailbox must not be the only owner of channel
state in a multi-replica deployment.

## Phase 0: Reliable development baseline

Goal: make changes cheap and failures visible before domain integration work.

### S-01: Establish the workspace and CI quality gate

- Split the project when useful into `domain`, `application`, `db`, `auth`,
  `server`, and later `app`; avoid a mechanical split with no boundary.
- Add `cargo fmt --check`, `cargo clippy --all-targets --all-features`, unit
  tests, dependency auditing, and a PostgreSQL migration/repository job.
- Cache Rust dependencies in CI and keep the no-database test suite fast.
- Document the supported Rust toolchain and local commands.

Acceptance: a pull request cannot merge when formatting, linting, unit tests,
or PostgreSQL contract tests fail.

### S-02: Add typed configuration, tracing, and operational endpoints

- Validate all configuration at startup.
- Emit structured tracing with request/correlation IDs and no message bodies,
  tokens, or secrets.
- Separate liveness (`/healthz`) from readiness (`/readyz`). Readiness checks
  required dependencies with a bounded timeout.
- Add graceful shutdown and basic latency/error metrics.

Acceptance: invalid configuration fails before binding the port; shutdown
drains HTTP and WebSocket work; probes express distinct liveness/readiness.

## Phase 1: Unify and persist the chat core

Goal: remove the two competing in-memory implementations and make one durable
command path authoritative.

### S-03: Align domain identifiers, sequences, and timestamps

- Use durable UUIDs for users, channels, and messages.
- Use `ChannelSequence` consistently for read markers and catch-up.
- Replace `last_read_message_id` with `last_read_sequence`.
- Use an interoperable timestamp representation (`DateTime<Utc>` on the
  server and RFC 3339 in protocols).
- Define overflow and invalid-zero behaviour for sequences and limits.

Acceptance: Rust types, migrations, JSON fixtures, and documentation describe
the same model; compatibility tests cover serialization.

### S-04: Complete the repository contract and conformance suite

- Add user, membership, leave, mark-read, and channel lookup operations.
- Define transaction boundaries and typed conflict/not-found/permission
  errors.
- Create a reusable repository conformance suite.
- Run it against the in-memory adapter, SQLite, and PostgreSQL.

Acceptance: the same behavioural suite passes for every repository adapter.

### S-05: Implement SQLx repositories and migration execution

- Implement SQLite for local development and PostgreSQL as the production
  contract.
- Allocate a channel sequence and insert its message in one transaction.
- Enable SQLite foreign keys for every connection.
- Make migration execution an explicit command suitable for a Kubernetes Job;
  application replicas must not race to mutate schema at startup.

Acceptance: restart loses no accepted message; concurrent writers produce
unique contiguous per-channel sequences; fresh and upgraded databases pass CI.

### S-06: Inject persistence into the application command service

- Replace the private command enum in the current actor with the public typed
  command surface.
- Authorize membership before reading or mutating a channel.
- Persist before realtime publication.
- Make channels explicit resources rather than creating one by subscribing.
- Map repository errors to stable application errors.

Acceptance: the running HTTP/WebSocket service uses no private in-memory chat
history as source of truth and enforces membership on every operation.

### S-07: Correct connection presence and realtime fan-out

- Track unique connection/session IDs rather than a `HashSet<UserId>`.
- Derive user presence from one or more active sessions with expiry/heartbeat.
- On broadcast lag, return the last seen and latest known sequence.
- Add a PostgreSQL `LISTEN/NOTIFY` publisher for cross-replica wake-up while
  loading authoritative events from the database.

Acceptance: closing one of two user sessions does not mark the user offline;
two service replicas deliver new messages and clients can recover from lag.

## Phase 2: Stable client protocol and usable chat

Goal: expose one versioned interface that browsers and agents can use safely.

### S-08: Implement `sproyt.chat.v1`

- Add the documented envelope with `protocol`, `request_id`, `type`, and
  `payload`.
- Support hello, channel lifecycle, list, subscribe/unsubscribe, catch-up,
  send, mark-read, ping, and structured errors.
- Correlate command acknowledgement and make `send_message` idempotent for a
  `(principal, request_id)` pair.
- Keep temporary protocol compatibility behind a short-lived feature flag.

Acceptance: protocol fixtures and end-to-end tests cover reconnect, duplicate
submission, unknown fields/events, authorization failure, and lag recovery.

### S-09: Deliver the first small-circle chat slice

- Create a private circle, invite/join members, create channels, send/read
  messages, and show unread state.
- Add explicit owner/member roles first; add moderator/observer only with a
  real use case.
- Keep rendering in the client with safe Markdown and raw/source view.

Acceptance: a fresh user can create a circle, invite another development user,
exchange durable messages, reconnect, and see correct unread state.

## Phase 3: Identity and authorization

Goal: use the same application identity model in development and production.

### S-10: Introduce the `AuthProvider` boundary and development auth

- Define an authenticated principal containing internal user ID, issuer,
  subject, display claims, and principal kind.
- Add an explicit `dev` provider selected only by configuration.
- Make dev users deterministic and easy to select in local/e2e tests.
- Refuse to start with dev auth when the deployment environment is marked
  production.

Acceptance: application handlers never trust participant IDs from query
parameters; tests can authenticate without an external provider; production
mode rejects dev auth.

### S-11: Integrate the Cloudflare-exposed Authentik issuer

- Configure an Authentik OIDC provider for Sproyt. Use the pinned issuer
  `https://sproyt-security.bjoroy.me/application/o/sproyt/` and load
  discovery metadata from that issuer rather than hard-coding endpoints.
- Implement Authorization Code flow with PKCE, state, nonce, secure cookies,
  callback validation, logout, and key rotation.
- Map `(issuer, subject)` to the internal user; treat email/display name as
  mutable profile claims, not identity keys.
- Store client secrets in Kubernetes Secrets or an external secret provider.
- Create or confirm the provider slug, client type, signing key, scopes, claims,
  redirect URIs, logout support, and audience rules during implementation.

Acceptance: login, refresh/session renewal, logout, invalid state/nonce,
expired token, rotated signing key, and revoked/disabled-user behaviour are
covered in integration tests.

### S-12: Centralize circle and channel authorization

- Express policy decisions as application/domain functions, not handler
  conditionals.
- Cover invite, join, leave, view history, send, moderate, start process, and
  invite agent.
- Add audit records for membership, permission, agent, and process mutations.

Acceptance: an authorization matrix is executable as tests and every external
adapter uses the same policy service.

## Phase 4: Kubernetes-ready delivery

Goal: run the durable chat slice safely in a cluster before adding process
features.

### S-13: Build a reproducible OCI image and local production profile

- Add a pinned multi-stage image, non-root runtime user, read-only root
  filesystem support, minimal runtime contents, and SBOM/image scanning.
- Add Compose/Podman setup for Sproyt plus PostgreSQL and an optional Heart
  endpoint.
- Keep secrets out of images and checked-in configuration.

Acceptance: one immutable image runs locally and under a restricted container
security context; vulnerability policy is enforced in CI.

### S-14: Add Kubernetes manifests or a small Helm chart

- Deployment, Service, Ingress, ConfigMap, Secret references, migration Job,
  PodDisruptionBudget, NetworkPolicy, resource requests/limits, probes, and
  topology-aware replica placement.
- Use rolling updates with backwards-compatible schema/protocol changes.
- Document TLS assumptions and the OIDC callback/issuer configuration.

Acceptance: a disposable namespace install, migration, smoke test, rolling
upgrade, rollback, and scale-to-two-replicas test all succeed.

### S-15: Define the production quality and operations gate

- Set initial SLOs for availability, accepted-message durability, send latency,
  and reconnect recovery.
- Add dashboards/alerts, database backup and restore drill, incident/runbook,
  retention defaults, and capacity/load tests.
- Verify that logs and traces contain neither secrets nor private message
  bodies.

Acceptance: restore and rollback drills are documented and tested; the release
checklist has named evidence for security, performance, and recovery.

## Phase 5: Heart process orchestration

Goal: add one useful process flow without coupling the chat domain to Heart.

### S-16: Add missing Heart client capabilities and `ProcessGateway`

- Add the Heart `POST /api/v1/messages` operation to `ea-heart-client` for
  receive-node correlation.
- Wrap Heart behind a Sproyt-owned `ProcessGateway` trait.
- Propagate correlation and tracing IDs; use timeouts, bounded retries, and
  typed error classification.
- Use a fake gateway in domain/application tests.

Acceptance: contract tests cover start, inspect, correlate receive message,
  failure, timeout, and retry behaviour without exposing Heart HTTP types to
  Sproyt domain code.

### S-17: Persist conversation-to-process links and process events

- Add `process_links` with channel/thread, Heart instance, definition,
  initiator, visibility, and timestamps.
- Store process activity as structured event type plus JSON payload; render it
  in clients instead of baking presentation into stored Markdown.
- Use an outbox/idempotency record so database commits and Heart calls can be
  retried safely.
- Reconcile uncertain external outcomes rather than assuming a timed-out call
  failed.

Acceptance: retries cannot create duplicate visible process starts or process
  events; every process action is attributable to a principal and instance.

### S-18: Ship one vertical group-process pilot

- Choose a bounded friend-circle workflow such as event planning, a group
  decision, or a shared checklist.
- Start it from a channel, show current state, collect an authorized human
  response through a Heart receive node, and post completion/failure.
- Protect the capability with a per-circle feature flag and kill switch.

Acceptance: the complete flow works after service restarts and rolling updates;
  disabling Heart leaves ordinary chat fully available.

## Phase 6: Agent participation

Goal: make agents explicit, least-privileged and auditable participants.

### S-19: Add agent identity, provenance, and grants

- Record agent owner/inviter, provider, service identity, declared purpose,
  scopes, rate limits, expiry, and revocation.
- Mark generated, delegated, and human-approved activity distinctly.
- Require separate grants for reading history, sending messages, starting
  processes, and completing process work.

Acceptance: removing or expiring a grant takes effect immediately; audit data
answers who or what caused every agent/process mutation.

### S-20: Add an agent/MCP adapter over the command service

- Expose the existing application commands as tools; do not create a second
  chat implementation.
- Apply the same authentication, authorization, idempotency, limits, and audit
  rules as browser clients.
- Begin with list/read/send/mark-read and add process tools only after the
  permission model is proven.

Acceptance: adapter conformance tests demonstrate identical domain outcomes
for WebSocket/HTTP and agent/MCP calls.

## Phase 7: Turn the technical client into a private chat product

Goal: replace the protocol demonstration screen with a small, understandable
chat experience before exposing process and agent capabilities to beta users.

### S-21: Ship an authenticated application shell

- Require an OIDC session for the production application route and redirect
  anonymous browsers to login.
- Derive browser and WebSocket identity only from the authenticated session;
  keep selectable identities exclusively in development/E2E adapters.
- Show the signed-in profile, logout, loading, disconnected and actionable
  error states without exposing protocol controls.

Acceptance: an anonymous production browser cannot reach the chat shell; an
authenticated user sees their own identity and cannot impersonate another
participant through URL, form or WebSocket parameters.

### S-22: Build the responsive conversation experience

- Add circle/channel navigation, message history, unread indicators, a message
  composer, reconnect/catch-up and useful empty states.
- Make the primary path usable on phone and desktop with keyboard navigation,
  focus management, semantic labels and accessible status announcements.
- Keep rendering safe and preserve raw/source view as a secondary action.

Acceptance: a signed-in user can select a conversation, exchange durable
messages, reconnect and recover unread state without seeing test controls.

### S-23: Add understandable circle onboarding

- Present circle creation, channel creation, invitations and membership as
  guided dialogs with role-aware actions and confirmations.
- Provide a useful first-run state for a user with no circles and a clear flow
  for accepting an invitation.
- Keep destructive and owner-only operations explicit and audited.

Acceptance: two fresh beta users can create/join a circle and exchange messages
without knowledge of slugs, UUIDs, tokens beyond the invitation link, or the
wire protocol.

### S-24: Gate advanced capabilities and sign off the beta UX

- Hide Heart, agent, MCP and diagnostic controls behind server-authoritative
  feature flags; ordinary chat remains useful when every advanced flag is off.
- Add browser E2E coverage for OIDC/session, WebSocket, mobile layout,
  reconnect, logout and failure recovery.
- Deploy immutable ARM64 increments through GitOps and record a human usability
  pass in the target cluster.

Acceptance: the private beta passes its browser journey on mobile and desktop;
advanced failures cannot prevent login, navigation or ordinary chat.

## Recommended GitHub structure

Use milestones rather than one large project phase branch:

1. `M0 Reliable core` — S-01 through S-07
2. `M1 Private chat` — S-08 through S-12
3. `M2 Cluster beta` — S-13 through S-15
4. `M3 Heart pilot` — S-16 through S-18
5. `M4 Agent pilot` — S-19 through S-20
6. `M5 Private beta UX` — S-21 through S-24

Recommended labels:

- `area/domain`, `area/db`, `area/protocol`, `area/auth`, `area/ops`,
  `area/heart`, `area/agent`
- `type/feature`, `type/quality`, `type/security`, `type/docs`
- `blocked`, `decision-needed`

Keep issues independently reviewable. An issue should normally produce one
small pull request or a short ordered series stated in its checklist. Close an
issue only when its acceptance criteria are automated or linked to durable
operational evidence.

## Dependency order

```text
S-01 -> S-02
  |       |
  +-> S-03 -> S-04 -> S-05 -> S-06 -> S-07
                                  |
                                  +-> S-08 -> S-09
                                              |
                         S-10 -> S-11 -> S-12
                                              |
                                  S-13 -> S-14 -> S-15
                                              |
                                  S-16 -> S-17 -> S-18
                                              |
                                          S-19 -> S-20
```

The first sensible cluster deployment is after S-14, but production traffic
should wait for the S-15 quality gate. Heart and agent features are deliberately
later: they then inherit durable messaging, real identity, authorization,
observability, and safe deployment instead of creating parallel mechanisms.
