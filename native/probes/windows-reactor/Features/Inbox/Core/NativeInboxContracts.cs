namespace Sproyt.WindowsProbe;

internal enum NativeInboxTab { Unread, Mentions, Tasks }
internal sealed record NativeInboxDestination(NativeNavigationTarget Target, bool Exact);
internal sealed record NativeInboxActionState(bool Busy = false, bool NeedsReadBack = false, string? Error = null);
internal sealed record NativeInboxUnread(NativeChannel Channel, ulong Count);
internal sealed record NativeInboxMentionRow(NativeMention Mention, NativeInboxActionState Action,
    NativeInboxDestination? Destination, bool Retained = false);
internal sealed record NativeInboxTaskRow(NativeAssignedTask Task, NativeInboxActionState Action,
    NativeInboxDestination? Destination, bool Retained = false);
internal sealed record NativeInboxSnapshot(NativePersonalScope? Scope, NativeInboxTab Tab,
    bool Loading, bool Ready, string? Error, bool AtCapacity,
    IReadOnlyList<NativeInboxUnread> Unread, IReadOnlyList<NativeInboxMentionRow> Mentions,
    IReadOnlyList<NativeInboxTaskRow> Tasks)
{
    internal bool Open => Scope is not null;
}
