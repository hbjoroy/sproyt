using System.Net;

namespace Sproyt.WindowsProbe;

internal sealed record NativeMediaScope(string UserId, string FamilyStamp);
internal sealed record NativeAuthSnapshot(NativeMediaScope Scope);

// A synthetic authority and byte source. The subject is the linked production
// library, including its real file operations, lease release and event ordering.
internal sealed class NativeAuthSession(NativeMediaScope initial)
{
    private readonly object _authGate = new();
    private NativeMediaScope _scope = initial;
    private Action? _afterCheck;
    internal event Action? Changed;

    internal void AfterNextScopeCheck(Action callback) => _afterCheck = callback;
    internal bool MediaScopeCurrent(NativeMediaScope scope)
    {
        bool current;
        lock (_authGate) current = scope == _scope;
        Interlocked.Exchange(ref _afterCheck, null)?.Invoke();
        return current;
    }

    internal bool AuthGateHeld => Monitor.IsEntered(_authGate);
    internal NativeMediaScope MediaScope() { lock (_authGate) return _scope; }
    internal T ProjectSnapshot<T>(Func<NativeAuthSnapshot, T> projection)
    { lock (_authGate) return projection(new(_scope)); }

    internal void Switch(NativeMediaScope next, Action? beforeNotification = null)
    {
        lock (_authGate) _scope = next;
        beforeNotification?.Invoke();
        Changed?.Invoke();
    }

    internal Task<HttpResponseMessage> ReadMediaAsync(string id, bool preview, NativeMediaScope scope, CancellationToken token)
    {
        token.ThrowIfCancellationRequested();
        if (!MediaScopeCurrent(scope)) throw new OperationCanceledException();
        var response = new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(scope.UserId) };
        response.Content.Headers.ContentType = new("text/plain");
        return Task.FromResult(response);
    }
}
