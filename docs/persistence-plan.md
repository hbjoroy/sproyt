# Persistent Chat Plan

This plan describes the next architectural step for Sproyt: moving from an in-memory chat loop to a persistent chat domain that still keeps the mailbox core, functional design style, and typed client/agent contract.

## Intent

The chat engine should treat the database as the durable source of truth and the mailbox/WebSocket layer as a live delivery mechanism. UI clients, agent clients, and future MCP tools should all use documented domain commands and events rather than coupling to browser-specific behavior.

## Design Principles

- Keep the chat core independent from UI rendering.
- Model the domain with small Rust types instead of raw strings.
- Represent operations as typed commands and results.
- Keep state transitions explicit and easy to test.
- Prefer pure functions for validation and domain decisions.
- Put side effects behind ports/traits: persistence, clock, identity lookup, and event publication.
- Persist before publishing realtime events.
- Use bounded queues for backpressure and report lag instead of blocking unrelated channels.
- Avoid `unsafe`.

## Domain Types

Initial strong types:

```rust
struct UserId(Uuid);
struct ChannelId(Uuid);
struct MessageId(Uuid);
struct ChannelSlug(String);
struct DisplayName(String);
struct MessageBody(String);
struct ChannelSequence(i64);
```

Enums:

```rust
enum PrincipalKind {
    Human,
    Agent,
}

enum ChannelKind {
    Public,
    Local,
    Private,
}

enum MembershipRole {
    Owner,
    Moderator,
    Member,
    Observer,
}
```

Validation should happen at construction time where practical. For example, `MessageBody::new` rejects empty content, `ChannelSlug::new` normalizes and validates allowed characters, and role transitions should be represented as domain functions rather than scattered conditionals.

## Persistent Model

First durable tables:

```text
users
  id
  kind
  display_name
  external_subject nullable
  created_at

channels
  id
  slug
  name
  kind
  created_by
  created_at

channel_memberships
  channel_id
  user_id
  role
  joined_at
  last_read_sequence

messages
  id
  channel_id
  sender_id
  body
  sequence
  created_at
```

Important constraints:

- `channels.slug` should be unique.
- `(channel_id, user_id)` should be unique in `channel_memberships`.
- `(channel_id, sequence)` should be unique in `messages`.
- Message sequence is per channel, not global.
- `last_read_sequence` is preferred over `last_read_message_id` because reconnect and catch-up are sequence-oriented.

## Migration Strategy

Use dialect-specific migrations with the same logical model:

```text
migrations/postgres/
migrations/sqlite/
```

The initial migration files live in those directories. Keep schema changes additive and explicit; do not edit an already-applied migration after it has been shared.

PostgreSQL should use `uuid`, `timestamptz`, foreign keys, and row-level sequence allocation through a `channel_sequences` row per channel.

SQLite should map ids and timestamps to `TEXT`, sequences to `INTEGER`, and must enable foreign keys per connection with:

```sql
PRAGMA foreign_keys = ON;
```

Do not try to hide every PostgreSQL/SQLite difference inside one perfect SQL file. Keep one domain model and repository contract, then let each SQLx repository handle the database dialect cleanly.

First PostgreSQL shape:

```sql
create table users (
  id uuid primary key,
  kind text not null check (kind in ('human', 'agent')),
  display_name text not null check (length(display_name) between 1 and 120),
  external_provider text null,
  external_subject text null,
  created_at timestamptz not null default now(),
  unique (external_provider, external_subject)
);

create table channels (
  id uuid primary key,
  slug text not null unique,
  name text not null check (length(name) between 1 and 120),
  kind text not null check (kind in ('public', 'local', 'private')),
  created_by uuid not null references users(id),
  created_at timestamptz not null default now()
);

create table channel_memberships (
  channel_id uuid not null references channels(id) on delete cascade,
  user_id uuid not null references users(id) on delete cascade,
  role text not null check (role in ('owner', 'moderator', 'member', 'observer')),
  joined_at timestamptz not null default now(),
  last_read_sequence bigint not null default 0,
  primary key (channel_id, user_id)
);

create table channel_sequences (
  channel_id uuid primary key references channels(id) on delete cascade,
  next_sequence bigint not null default 1 check (next_sequence >= 1)
);

create table messages (
  id uuid primary key,
  channel_id uuid not null references channels(id) on delete cascade,
  sender_id uuid not null references users(id),
  sequence bigint not null check (sequence >= 1),
  body text not null check (length(body) > 0),
  created_at timestamptz not null default now(),
  unique (channel_id, sequence)
);
```

Sequence allocation should happen in the same transaction as message insert. PostgreSQL can use:

```sql
update channel_sequences
set next_sequence = next_sequence + 1
where channel_id = $1
returning next_sequence - 1 as sequence;
```

SQLite can use update plus select inside the same transaction.

## Command Surface

The internal application command surface should be stable enough to document and expose through HTTP, WebSocket, and later MCP.

```rust
enum ChatCommand {
    CreateChannel(CreateChannel),
    JoinChannel(JoinChannel),
    LeaveChannel(LeaveChannel),
    ListMyChannels(ListMyChannels),
    LoadRecentMessages(LoadRecentMessages),
    SendMessage(SendMessage),
    MarkRead(MarkRead),
}
```

Suggested command payloads:

```rust
struct CreateChannel {
    actor: UserId,
    slug: ChannelSlug,
    name: DisplayName,
    kind: ChannelKind,
}

struct JoinChannel {
    actor: UserId,
    channel: ChannelRef,
}

struct ListMyChannels {
    actor: UserId,
}

struct LoadRecentMessages {
    actor: UserId,
    channel: ChannelId,
    limit: MessageLimit,
    after: Option<ChannelSequence>,
}

struct SendMessage {
    actor: UserId,
    channel: ChannelId,
    body: MessageBody,
}
```

`ChannelRef` can support lookup by id or slug without making the rest of the domain stringly typed.

## Event Surface

Events should be documented and serializable. WebSocket should emit the same event model that future agent clients can consume.

```rust
enum ChatEvent {
    ChannelCreated(ChannelCreated),
    MembershipJoined(MembershipJoined),
    MembershipLeft(MembershipLeft),
    MessageAccepted(MessageAccepted),
    ReadMarkerUpdated(ReadMarkerUpdated),
}
```

Events should include stable ids and channel sequence where relevant. Clients should be able to reconnect and request missed messages by channel sequence.

## Repository Port

Persistence should be behind a trait so the chat core can be tested without a database and backed by SQLx in production.

```rust
trait ChatRepository {
    async fn create_channel(&self, command: CreateChannel) -> Result<Channel, RepositoryError>;
    async fn join_channel(&self, command: JoinChannel) -> Result<Membership, RepositoryError>;
    async fn list_channels_for_user(&self, actor: UserId) -> Result<Vec<ChannelSummary>, RepositoryError>;
    async fn load_recent_messages(&self, query: LoadRecentMessages) -> Result<Vec<Message>, RepositoryError>;
    async fn append_message(&self, command: SendMessage) -> Result<Message, RepositoryError>;
}
```

The SQLx implementation can start in-process. Later we can split repository implementations for SQLite and PostgreSQL if dialect differences become awkward.

## Message Flow

Sending a message should follow this order:

1. WebSocket, HTTP, MCP, or UI adapter receives a request.
2. Adapter authenticates the caller into a typed `UserId`/principal.
3. Adapter creates a typed `SendMessage` command.
4. Chat mailbox accepts the command into a bounded queue.
5. Chat actor validates membership/permissions through domain services/repository.
6. Repository appends the message and assigns channel sequence in a transaction.
7. Chat actor publishes `MessageAccepted`.
8. WebSocket subscribers receive the event.
9. Slow subscribers may lag and then resync with `LoadRecentMessages { after }`.

## Channel Creation And Membership

Channels should not remain purely implicit. First version should support:

- Create channel.
- Join channel.
- List channels I have joined.
- Leave channel.
- Load recent messages for a joined channel.

For development mode, we can auto-create a default `general` channel and auto-join the dev user. That should be seeded behavior, not a hidden rule inside message sending.

## External Interfaces

Document the protocol in `docs/protocol.md` as the command/event names stabilize.

Initial interface layers:

- Browser UI over WebSocket.
- HTTP endpoints for channel creation and listing.
- WebSocket for live events and sending messages.
- Future MCP server exposing tools like:
  - `sproyt_create_channel`
  - `sproyt_join_channel`
  - `sproyt_list_channels`
  - `sproyt_send_message`
  - `sproyt_read_channel`

The MCP server should be an adapter over the same command surface, not a separate chat implementation.

WebSocket should move toward a versioned envelope with `protocol`, `request_id`, `type`, and `payload`, so browser clients and agent clients can share the same wire contract.

## Phased Implementation

### Phase 1: Type And Port Skeleton

- Introduce domain modules for users, channels, memberships, messages, commands, and events.
- Replace primitive string ids in the chat core with strong types where possible.
- Define repository trait and in-memory implementation for tests.
- Keep current WebSocket behavior working.

### Phase 2: SQLx Persistence

- Add SQLx dependencies and configuration.
- Add migrations for SQLite and PostgreSQL-compatible schema.
- Implement repository methods for create channel, join channel, list user channels, load recent messages, and append message.
- Seed development user and `general` channel.

### Phase 3: Persistent Chat Flow

- Change `subscribe` to load recent persisted messages.
- Change `send_message` to append to database before broadcasting.
- Add channel list endpoint for the current user.
- Add basic channel create/join UI.

### Phase 4: Documented Protocol

- Expand `docs/protocol.md`.
- Document WebSocket commands and events.
- Document HTTP endpoints.
- Add examples for browser clients, agent clients, and future MCP tools.

### Phase 5: Agent And MCP Readiness

- Add service layer methods that are ergonomic for non-browser clients.
- Design MCP tool names, input schemas, and output schemas.
- Keep MCP as an adapter over the same domain command/event model.

## Open Questions

- Should local channels be scoped by organization, deployment, project, or something else?
- What should agents be allowed to do by default?
- Do we need private DMs as channels with special membership rules?
- Should message edits/deletes exist in the first persistent version?
- Should markdown rendering policy be stored per message, per channel, or remain purely client-side?
