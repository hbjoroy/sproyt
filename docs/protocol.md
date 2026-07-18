# Sproyt Chat Protocol

This document is the first protocol sketch for browser clients, agent clients, and future MCP adapters. The protocol should stay aligned with the typed Rust command and event model in `src/domain`.

## Version

Initial protocol id:

```text
sproyt.chat.v1
```

Rules:

- Every client command and server message should include `protocol`.
- Clients should ignore unknown event types and unknown fields.
- Existing fields should not change meaning within the same protocol version.
- Breaking changes require a new protocol id.

## Envelope

Future WebSocket messages should use a stable envelope instead of binding one channel to the socket URL.

Client command:

```json
{
  "protocol": "sproyt.chat.v1",
  "request_id": "request-123",
  "type": "send_message",
  "payload": {
    "channel": { "type": "id", "value": "channel-1" },
    "body": "Hei"
  }
}
```

Server response or event:

```json
{
  "protocol": "sproyt.chat.v1",
  "request_id": "request-123",
  "type": "message_accepted",
  "payload": {
    "message": {
      "id": "019c1e71-4f6a-7000-8000-000000000001",
      "channel_id": "019c1e71-4f6a-7000-8000-000000000002",
      "sender_id": "019c1e71-4f6a-7000-8000-000000000003",
      "sender_display_name": "Alice",
      "body": "Hei",
      "sequence": 42,
      "sent_at": "2026-07-18T08:30:00Z"
    }
  }
}
```

`request_id` is important for agents because they may issue several commands concurrently and need deterministic correlation.
`sender_display_name` is persisted with the message as an audit-safe historical
snapshot. A later profile rename affects new messages, not existing history.

## Commands

First stable command set:

- `hello`
- `create_channel`
- `join_channel`
- `leave_channel`
- `list_my_channels`
- `load_recent_messages`
- `subscribe_channel`
- `unsubscribe_channel`
- `send_message`
- `mark_read`
- `ping`

Clients should send `ping` more frequently than the configured
`SPROYT_WS_IDLE_TIMEOUT_SECONDS` value. The browser sends one every 20 seconds.
Any inbound WebSocket frame renews the idle deadline; an expired connection is
closed with WebSocket code 1001 and reason `idle timeout`, after which clients
reconnect and catch up by sequence.

The running WebSocket endpoint accepts only the versioned `sproyt.chat.v1`
envelope. Unknown protocol versions and command types return stable structured
errors; there is no parallel legacy command path.

## Events

Sequence `0` is reserved for an unread/catch-up cursor before the first
message. Persisted messages always start at `1`. Allocation is checked and an
exhausted sequence fails the command rather than wrapping.

`load_recent_messages` without `after` returns the most recent page in
ascending sequence order. With `after`, it returns the next ascending page
immediately after that cursor. Clients repeat the latter until the last loaded
sequence reaches `latest_known_sequence`; the browser deduplicates message IDs
while live delivery and durable catch-up overlap.

Channel summaries carry both `last_read_sequence` and `latest_sequence`, so
clients can compute unread count after reconnect without loading message
bodies. Joining a channel owned by a circle requires current circle membership;
knowing a private channel ID or slug is not authorization.

First event set:

- `channel_created`
- `participant_joined`
- `participant_left`
- `message_accepted`
- `read_marker_updated`
- `subscription_started`
- `subscription_ended`
- `lagged`
- `error`

Lag events should include enough information for resync:

```json
{
  "protocol": "sproyt.chat.v1",
  "type": "lagged",
  "payload": {
    "channel_id": "channel-1",
    "last_seen_sequence": 40,
    "latest_known_sequence": 55,
    "hint": "load_recent_messages_after"
  }
}
```

## MCP Adapter

MCP should be an adapter over the same command surface, not a separate implementation.

Candidate tools:

- `sproyt_list_channels`
- `sproyt_create_channel`
- `sproyt_join_channel`
- `sproyt_read_channel`
- `sproyt_send_message`
- `sproyt_mark_read`

Later candidates:

- `sproyt_watch_channel`
- `sproyt_search_messages`
- `sproyt_list_members`
- `sproyt_invite_agent`
- `sproyt_get_channel_state`

MCP tools should return the same domain concepts as WebSocket and HTTP, but shaped as tool-friendly JSON.

Agent provisioning and revocation remain owner-authenticated HTTP control-plane
operations. `POST /api/v1/agents/{id}/revoke` atomically revokes the profile,
all bearer credentials and all grants; existing MCP credentials stop working
immediately and the mutation is audited as `agent.revoked`.
