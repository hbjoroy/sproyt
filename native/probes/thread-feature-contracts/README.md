# Native thread feature contracts

This executable links the production credential-free thread presentation owner.
It checks account/session/visit fences, independent transport generations,
bounded and atomic directory/detail projections, per-thread anchors and actual-
visible-reply admission without optimistic read advancement.

```powershell
dotnet run --project ThreadFeatureContracts.csproj --artifacts-path S:/Build/native-thread-feature/managed -- S:/Build/native-thread-feature/owner-tests
```

Run from this directory to use its pinned .NET SDK. Output belongs in a separate
test-owned directory. The runner does not open a real session, send a message or
claim WinUI, draft-storage or server acceptance.

The application binds the verified user and full session stamp with `BindScope`,
then starts each directory/detail operation using `OpenDirectory`/`Open`. Each
returns a new `NativeThreadVisit`, even when reopening the same root. Retain that
visit across all awaits and viewport observations. Only its corresponding
`ApplyDirectory`, `ApplyThread`, `Fail`, `SaveAnchor` and `VisibleRead` calls can
affect the current view. Token rotation preserves the desired target and anchors
within the same login family, but invalidates old data and concrete callbacks.
Account or login-family changes clear private state.

The host supplies authoritative root previews to `ApplyDirectory` when already
loaded. Unknown older roots remain explicitly unknown until fetched; the owner
never fabricates content. Directory rows retain the server's reply/unread counts.
Both directory rows and thread replies are bounded at 2000. Oversized or malformed
sets fail atomically with a visible error rather than being silently truncated.

`NativeThreadView` receives the host's same message renderer and actions for both
the parent and replies. It also receives a durable, target-bound draft and send
callback. It creates no store, token owner or transport. `PendingReplies` must
carry the correct parent ID; rows from other targets are excluded. The native
composer has an independent key and Ctrl+Enter calls the host, which must recheck
current admission rather than rely on the render's old `CanSend` value.

The actual `ScrollViewer` exposed by `ViewportMounted` contains parent and replies
and has automation ID `live-thread-history`. The application owns viewport
measurement and restoration. Pass the captured feature visit, transport thread
generation, active-window/surface/restoration facts and measured IDs to
`VisibleRead`. Its report includes only loaded reply IDs, excluding the parent,
pending and unknown rows. The application then invokes the fenced transport
read command. Only server ACK/event projection may advance confirmed read state.

The host must keep the channel mounted underneath a narrow thread pane while
marking it invisible to read reporting, and restore its channel draft/anchor when
closing. Wide layout may report independently measured channel and thread rows.
