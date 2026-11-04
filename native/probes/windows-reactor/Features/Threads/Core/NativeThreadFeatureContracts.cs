namespace Sproyt.WindowsProbe;

// A concrete UI operation must retain the full credential stamp and this visit
// generation. A root ID alone cannot distinguish A-B-A or reconnect callbacks.
internal sealed record NativeThreadVisit(string UserId, string SessionStamp, long Generation,
    string ChannelId, string? RootMessageId)
{
    internal bool IsDirectory => RootMessageId is null;
}

internal sealed record NativeThreadDirectoryItem(NativeThreadSummary Summary, NativeMessage? Root);

internal sealed record NativeThreadFeatureSnapshot(string? UserId, NativeThreadVisit? Visit,
    bool Loading, string? Error, bool AtCapacity, IReadOnlyList<NativeThreadDirectoryItem> Directory,
    NativeThreadSnapshot? Thread, NativeScrollAnchor Anchor)
{
    internal bool DirectoryOpen => Visit?.IsDirectory == true;
    internal bool DetailOpen => Visit is { IsDirectory: false };
}

internal sealed record NativeThreadReadReport(string ChannelId, string RootMessageId,
    long ViewGeneration, long ThreadGeneration, IReadOnlyList<string> MessageIds);
