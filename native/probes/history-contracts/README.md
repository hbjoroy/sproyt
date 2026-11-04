# Native older history and measured reading

This standalone .NET 10.0.301 fixture compiles the production native chat
transport and the pure conversation visibility policy. Kestrel serves synthetic
`sproyt.chat.v1` commands over a real loopback WebSocket. Its session owner is
explicitly synthetic; it does not mint credentials, contact Canary or prove
physical WinUI viewport geometry.

```powershell
dotnet run --project native/probes/history-contracts/HistoryContracts.csproj --artifacts-path S:/Build/native-parity-history/history-contracts -- S:/Build/native-parity-history/history-contracts.json
dotnet run --project native/probes/client-contracts/ClientContracts.csproj --artifacts-path S:/Build/native-parity-history/existing-contracts -- S:/Build/native-parity-history/existing-contracts.json
```

The new snapshot is an optional trailing `NativeChatSnapshot.History`:

```csharp
NativeHistorySnapshot(string? ChannelId, long ViewGeneration,
    bool LoadingOlder, bool HasOlder, bool AtCapacity, string? Error,
    ulong? OldestSequence, int LoadedCount)
```

`LoadOlderAsync(channelId, viewGeneration, userId, sessionStamp, cancellation)`
loads one older page. Calling it again after failure retries the unchanged
cursor. Duplicate requests while loading do not send another frame. Selected
channel, view generation, account, session and captured socket are checked
before writes and before applying a page. Selection/reconnect/lag repair creates
a new view generation. Successful read confirmations do not change it.

UI navigation calls `SelectChannelAsync(channelId, cancellationToken,
expectedUserId: capturedUser, expectedSessionStamp: capturedFullStamp)`.
Both optional identity arguments must be supplied together. Calls without them
capture the current scope immediately, before waiting for the selection gate.
After the wait, the original scope is revalidated before changing selection,
and its exact fence follows every dispatch. A queued action from account A
cannot select a shared channel as account B. The fixture keeps an old selection
paused while account B completes its new connection/directory and verifies that
the queued A action emits no B-channel history request. The shared synthetic
session helper's new `SwitchAccount` method is test-only.

The server's existing `load_recent_messages` returns roots **and** thread
replies. Pages use 50 raw rows, with `before` equal to the earliest raw sequence.
Only root messages enter this timeline. A reply-only page advances its cursor
and retains `HasOlder`; the user can continue without reply rows appearing in
the channel. An empty/short page proves the older boundary. Pages are validated
atomically for channel, ascending sequence, duplicate IDs, limit and strict
`before` boundary. Delayed pages from an earlier A–B–A visit are discarded.
Newer edit/delete events dominate older page data; tombstones cannot resurrect.
Background lag repair retains its captured channel and generation and rechecks
them after waiting for the selection semaphore. A queued repair cannot override
a later user selection, including A–B–A. Every disconnect invalidates the view
generation before a new socket can become connected; a fully received old page
waiting to merge is rejected even in the next socket's hello/bootstrap gap.

This is a bounded in-memory online view, not a persistent history or delta-sync
implementation. It retains at most 2000 roots with no silent eviction. A page
that would exceed the bound is not partially committed; `AtCapacity` and a
readable error explain the boundary. Reopening explicitly fetches recent
history again. The count bound implies at most 128 MiB of UTF-8 message bodies
at the server's 64 KiB maximum, plus managed strings and metadata. This is not a
measured working-set target. The bounded wire-message limit is 20 MiB to admit
the server's 50-message subscription history even with JSON escaping. The
fixture actually reads three legal 64 KiB bodies whose escaped frame exceeds
the former 1 MiB limit.

`MarkVisibleReadAsync(channelId, viewGeneration, measuredRootIds, userId,
sessionStamp, cancellation)` chooses the highest sequence among the supplied,
currently loaded roots. Empty, pending, unknown and thread IDs never imply the
latest sequence. Read state changes only after a correlated membership ACK or
an own-account `chat.read_marker_updated` event, monotonically; another user's
read event is ignored. A stale channel list cannot lower a confirmed marker.
Nothing marks read on selection or history loading.
Permission/not-found denial of either history or reading clears the matching
active projection. A late denial from a previous view neither clears the new
channel nor publishes its old error into the new view.

The WinUI owner must measure actual visible rows and call only when its window
and conversation surface are active, after anchor restoration settles. The
separate `Features/Conversation/Core/ConversationHistoryPolicy.cs` takes these
facts and returns `ConversationReadReport`, or null when inactive, offscreen,
restoring, loading or without measured root IDs. It owns no transport or token.
The integrator owns actual WinUI measurement and anchor preservation by stable
message ID, sequence and relative row Y, with absolute offset as fallback.

The fixture proves paging/retry/boundaries, reply-only pages, immutable IDs and
ordering, malformed-page rejection without partial mutation, no optimistic
read update before ACK, external read-event filtering, duplicate load suppression,
late A–B–A pages, stale read generation, navigation/revoke exactly before socket
write, revoke during an outstanding page, newer edit/delete preservation, the
2000-root limit, explicit reopen and permission-loss projection clearing.
Deterministic scheduling tests also queue repair behind a newer selection, deny
a held old-view read, and pause a completed old page before merge while the next
connection's hello is held. These exercise actual transport/semaphore paths,
without adding injectable hooks to the production session owner.
The prior command-contract fixture remains the regression check for definitive
rejections, uncertain delivery, reconnect and wire replay. Physical keyboard,
touch, viewport anchoring, media resize and real server revocation remain
separate integration/live acceptance points.
