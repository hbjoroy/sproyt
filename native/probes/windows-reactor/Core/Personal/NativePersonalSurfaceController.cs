namespace Sproyt.WindowsProbe;

internal enum NativePersonalSurfaceKind { People, Unread, Mentions, Tasks, Profile }

internal sealed class NativePersonalSurfaceController
{
    private string? _user, _stamp, _family;
    private static long _lastVisit;
    internal NativePersonalSurfaceKind? Kind { get; private set; }
    internal NativePersonalScope? Scope { get; private set; }

    internal bool Bind(string? userId, string? fullStamp)
    {
        int split = fullStamp?.LastIndexOf(':') ?? -1;
        if (string.IsNullOrWhiteSpace(userId) || fullStamp is null || split <= 0 || split == fullStamp.Length - 1)
        {
            userId = null;
            fullStamp = null;
        }
        string? family = fullStamp is null ? null : fullStamp[..split];
        bool changed = userId != _user || family != _family;
        if (changed) Close();
        bool rotated = !changed && fullStamp != _stamp;
        _user = userId; _stamp = fullStamp; _family = family;
        if (rotated && Kind is { } open) Open(open);
        return changed;
    }

    internal NativePersonalScope? Open(NativePersonalSurfaceKind kind)
    {
        if (_user is null || _stamp is null) return null;
        // A retired component may still have callbacks in flight when the
        // same user mounts a replacement controller under the same stamp.
        long previous;
        do
        {
            previous = Interlocked.Read(ref _lastVisit);
            if (previous == long.MaxValue) { Close(); return null; }
        } while (Interlocked.CompareExchange(ref _lastVisit, previous + 1, previous) != previous);
        Kind = kind;
        Scope = new(_user, _stamp, previous + 1);
        return Scope;
    }

    internal void Close() { Kind = null; Scope = null; }
    internal bool Current(NativePersonalScope scope) => Scope is not null && Scope == scope;
}
