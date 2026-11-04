namespace Sproyt.WindowsProbe;

// Sequence is the server's channel sequence (including thread replies), never
// an index into the currently materialized root-message list.
internal sealed record NativeScrollAnchor(string? MessageId = null, ulong? Sequence = null,
    double PixelOffset = 0, bool AtEnd = true, double AbsoluteOffset = 0)
{
    internal bool IsValid => double.IsFinite(PixelOffset) && double.IsFinite(AbsoluteOffset) && AbsoluteOffset >= 0 &&
        (MessageId is null || (MessageId.Length is > 0 and <= 128 && !MessageId.Any(char.IsControl)));
}

internal sealed record NativeNavigationTarget(string ChannelId, string? MessageId = null,
    ulong? Sequence = null, string? ThreadId = null)
{
    // IDs select resources, not their current display names. Fixture IDs are
    // allowed as well as production UUIDs; this model never builds a URL/SQL.
    internal bool IsValid => Identifier(ChannelId) && (MessageId is null || Identifier(MessageId)) &&
        (ThreadId is null || Identifier(ThreadId));
    private static bool Identifier(string? value) => value is { Length: > 0 and <= 128 } && !value.Any(char.IsControl);
}

internal enum NativeNavigationDirectoryScope { Circles, Direct }

internal sealed record NativeNavigationGroup(string Id, string Name, IReadOnlyList<NativeChannel> Channels);

// A concrete callback captures the full session stamp AND the monotonic visit
// generation. The family prefix alone is only for preserving local navigation
// across normal access-token rotation, never for admitting an async result.
internal sealed record NativeNavigationVisit(string UserId, string SessionStamp, long Generation,
    NativeNavigationTarget Target, NativeScrollAnchor Anchor, bool ChannelChanged);

internal sealed record NativeNavigationSnapshot(string? UserId, string Query,
    NativeNavigationDirectoryScope DirectoryScope, IReadOnlyList<NativeChannel> Channels,
    IReadOnlyList<NativeNavigationGroup> Groups, NativeNavigationVisit? Current,
    bool CanBack, bool CanForward, bool Pending, string? Error, string? PreferenceError,
    int CircleMatches, int DirectMatches);

internal sealed record NativeNavigationPreferences(NativeNavigationDirectoryScope DirectoryScope = NativeNavigationDirectoryScope.Circles);

internal interface INativeNavigationPreferenceStore
{
    NativeNavigationPreferences Read(string userId);
    bool Write(string userId, NativeNavigationPreferences preferences);
}
