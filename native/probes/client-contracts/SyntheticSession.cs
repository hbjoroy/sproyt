using System.Net.WebSockets;

// Only this standalone test project compiles this synthetic owner. Production
// continues to compile the real DPAPI/session owner, with no injectable socket.
internal enum NativeAuthPhase { SignedOut, SigningIn, Active, Offline, AuthRequired }
internal sealed record NativeAuthSnapshot(NativeAuthPhase Phase, string? UserId, ulong Generation, bool PendingRevocation, bool Busy,
    long? LoginTicket = null, string? DisplayName = null, string? SessionStamp = null);
internal sealed record NativeSocketConnection(ClientWebSocket Socket, string UserId, string SessionStamp, ulong Generation);
internal sealed class NativeAuthSession(Uri socketUri)
{
    private readonly object gate = new();
    private NativeAuthSnapshot state = new(NativeAuthPhase.Active, FixtureServer.User, 0, false, false, SessionStamp: "synthetic:family:0");
    internal event Action? Changed;
    internal Action? BeforeAdmission { get; set; }
    internal NativeAuthSnapshot Snapshot { get { lock (gate) return state; } }
    internal Task ResumeAsync() => Task.CompletedTask;
    internal async Task<NativeSocketConnection> ConnectBoundWebSocketAsync(CancellationToken cancellationToken)
    {
        var captured = Snapshot;
        var socket = new ClientWebSocket();
        await socket.ConnectAsync(socketUri, cancellationToken);
        return new(socket, captured.UserId!, captured.SessionStamp!, captured.Generation);
    }
    internal T AdmitTransport<T>(string expectedUserId, string expectedSessionStamp, Func<T> operation)
    {
        BeforeAdmission?.Invoke();
        lock (gate)
        {
            if (state.Phase != NativeAuthPhase.Active || state.UserId != expectedUserId || state.SessionStamp != expectedSessionStamp)
                throw new InvalidOperationException("Synthetic account admission rejected.");
            return operation();
        }
    }
    internal void RequireAuthentication()
    {
        lock (gate) state = state with { Phase = NativeAuthPhase.AuthRequired, UserId = null, Generation = state.Generation + 1, SessionStamp = null };
        Changed?.Invoke();
    }
}
