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
    "message_id": "019c1e71-4f6a-7000-8000-000000000001",
    "channel_id": "channel-1",
    "sequence": 42,
    "sender_id": "alice",
    "body": "Hei"
  }
}
```

`request_id` is important for agents because they may issue several commands concurrently and need deterministic correlation.

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

Current prototype note: the running WebSocket endpoint still accepts a simpler message:

```json
{ "type": "send", "body": "Hei" }
```

That is a temporary browser adapter shape. It should be replaced by the envelope above when the persistent command surface lands.

## Events

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
