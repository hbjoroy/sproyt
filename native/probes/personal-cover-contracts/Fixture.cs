namespace Sproyt.WindowsProbe;

// The cover, watermark, auth admission and compare/exchange are the linked
// production partial. This fixture supplies only the surrounding app boundary.
internal sealed partial class LiveApplication
{
    private sealed record Auth(string? UserId, string? SessionStamp);
    private sealed class TestOwner
    {
        private readonly object _gate = new();
        private Auth _snapshot = new(null, null);
        internal T ProjectSnapshot<T>(Func<Auth, T> action) { lock (_gate) return action(_snapshot); }
        internal void Bind(string? user, string? stamp) { lock (_gate) _snapshot = new(user, stamp); }
    }
    private readonly TestOwner _authentication = new();
    private string? _account;
    private IPersonalCommands _chat => throw new NotSupportedException("Cover tests never invoke personal commands.");
    internal void BindForProbe(string? account, string? user, string? stamp)
    { _account = account; _authentication.Bind(user, stamp); }
    internal bool CoveredForProbe => ConversationCovered;

    // Same signature/semantics as the delivery owner: admitted is already
    // a family prefix, while stamp is a full rotating access credential stamp.
    private static string? SendFamily(string? stamp)
    {
        int last = stamp?.LastIndexOf(':') ?? -1;
        return last > 0 ? stamp![..last] : null;
    }
    private static bool SameSendFamily(string? admitted, string? stamp) => admitted is not null && admitted == SendFamily(stamp);
}
