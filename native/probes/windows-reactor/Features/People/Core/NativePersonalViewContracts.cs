namespace Sproyt.WindowsProbe;

internal sealed record NativePeopleSnapshot(NativePersonalScope? Scope, string Query, IReadOnlyList<NativePerson> People,
    bool Loading, bool Loaded, string? OpeningUserId, string? ReadError, string? OpenError, bool HasUncertainDirect);

internal sealed record NativeProfileSnapshot(NativePersonalScope? Scope, NativePerson? Profile,
    string NameDraft, string StatusTextDraft, string StatusEmojiDraft, bool Loading, bool Loaded,
    bool Saving, bool HasUncertainWrite, bool CanSave, string? Error, string? Notice);

// Bind is the shell's admission boundary. Every callback also carries its exact
// surface visit, so a retained old control cannot edit a newly opened surface.
internal abstract class NativePersonalController
{
    protected sealed class Operation(NativePersonalScope scope, string kind)
    {
        internal NativePersonalScope Scope { get; } = scope;
        internal string Kind { get; } = kind;
        internal CancellationTokenSource Cancellation { get; } = new();
        internal CancellationToken Token => Cancellation.Token;
    }
    protected readonly object Gate = new();
    protected NativePersonalScope? Scope;
    protected Operation? Active;
    private string? _owner;
    internal event Action? Changed;
    internal void Bind(NativePersonalScope? scope)
    {
        Operation? retired;
        lock (Gate)
        {
            if (Scope == scope) return;
            retired = Active; Active = null; Scope = scope;
            if (scope is not null)
            {
                int split = scope.SessionStamp.LastIndexOf(':');
                string family = split > 0 ? scope.SessionStamp[..split] : scope.SessionStamp;
                string owner = scope.UserId + "/" + family;
                if (_owner != owner) { _owner = owner; ResetOwner(); }
            }
            ResetProjection();
        }
        try { retired?.Cancellation.Cancel(); } catch (ObjectDisposedException) { }
        Notify();
    }
    protected abstract void ResetOwner();
    protected abstract void ResetProjection();
    protected bool Matches(NativePersonalScope scope) => Scope == scope;
    protected Operation? Begin(NativePersonalScope scope, string kind)
    { if (!Matches(scope) || Active is not null) return null; return Active = new(scope, kind); }
    protected bool Current(Operation operation) => ReferenceEquals(Active, operation) && Scope == operation.Scope;
    protected void Finish(Operation operation)
    {
        bool changed;
        lock (Gate) { changed = Current(operation); if (changed) Active = null; }
        operation.Cancellation.Dispose(); if (changed) Notify();
    }
    protected void Notify() { try { Changed?.Invoke(); } catch { /* Presentation observers cannot change command outcomes. */ } }
    protected static bool Rejected(Exception error, string requestId) =>
        error is NativeCommandException { IsDefinitiveRejection: true } command && command.RequestId == requestId;
}
