# Native conversation transport contracts

This headless runner links the actual `NativeChatClient` against a local Kestrel
WebSocket server and a synthetic session owner. It does not use real credentials
or establish live Canary, UI, physical input or durable-journal acceptance.

Run from this directory with the pinned SDK:

```powershell
dotnet run --project ConversationContracts.csproj --artifacts-path S:/Build/native-conversation-transport/build -- S:/Build/native-conversation-transport/receipt.json
```

The fixture covers separate root/reply projections, thread summaries, visible
reply IDs and ACK-only read progress, old-thread/page/permission fences, newer
edit/tombstone preservation, validated mutation ACKs, authoritative readback
without changing the selected thread, reaction snapshot/live-event races and
duplicate events, uncertain toggle without replay, scope clearing, and bounded
thread capacity. Existing history/client contracts remain regression checks.

`NativeChatSnapshot.Conversation` contains the selected `NativeThreadSnapshot`,
server summaries and reaction summaries. An independent thread generation fences
old A-B-A loads and read callbacks inside the same channel view. Each operation
also captures user, full session stamp, channel and root-history view generation
before starting its socket write and before merging a response. Closing a thread
invalidates its callbacks. Account/access changes remove the matching projection.

Only measured loaded replies can advance thread reading. Root IDs and unknown IDs
are ignored. Selection and loading never mark read; the UI must additionally gate
on actual active/visible geometry after restoring its anchor. A thread read ACK
cannot overwrite a summary updated by an intervening live reply; a coalesced
maintenance read obtains the authoritative counts instead. Reply/deletion events
also refresh server summaries without asking a UI snapshot getter to do network
work. Another thread's late permission denial cannot clear a newer thread.

`ReactionsLoaded` starts false and becomes true only after a validated full list.
UI must not interpret a missing reaction as `reacted_by_me=false` before that.
Snapshot reads crossing live updates retry within a fixed bound. Changes use an
absolute count and a user-ID set so duplicate ACK/broadcast delivery is idempotent.
A count/user mismatch schedules authoritative repair. Snapshot/error flags let
the UI display retry rather than manufacturing counts or identities.

Edit/delete/toggle commands have no server request-ID idempotency contract. This
transport sends each call once and never automatically replays a mutation after
an uncertain result. The action owner must do explicit readback and require a
fresh user action for a further write. `ReadMessageAsync(channel, rootId,
messageId, viewGeneration, user, stamp)` uses authorized `load_thread`; a reply
requires its captured parent/root ID. Readback does not open a thread or inject a
cold directory root into the contiguous root timeline. Newer admitted edits and
tombstones win over an older response. A separate bounded metadata cache protects
an in-flight root history page without creating gaps in that timeline.

`SendAsync` has a trailing optional `parentMessageId`, preserving the older call
shape. Root and reply sends retain stable IDs and payloads through the existing
durable owner. The transport checks channel/author/parent and the server-normalized
body before accepting a send ACK. Thread journal/schema acceptance is tested by
the integrating application, not this synthetic fixture.

The pilot accepts at most 2000 replies plus their root, and at most **20 MiB per
WebSocket frame**. These are independent limits: a thread of 321 legal 64 KiB
messages can exceed the frame budget. The fixture transmits that valid fragmented
JSON response and verifies explicit capacity status, bounded receive memory and
an intact socket with no automatic reopen/reconnect loop. Oversized frames are
drained without parsing; because their request cannot be safely correlated,
pending metadata reads fail closed and pending mutation outcomes remain uncertain.
An explicit later small-thread retry succeeds on the same connection. Large
thread paging requires a future server protocol; this batch does not claim all
2000 maximum-size messages are supported. Reaction/summary lists have an explicit
10000-record bound and do not silently accept a partial snapshot.
