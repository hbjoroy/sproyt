namespace Sproyt.WindowsProbe;

public sealed record Draft(string Channel, string Body);

public sealed record QueuedMessage(string RequestId, string Channel, string Body, string State);

public sealed record StoreSnapshot(IReadOnlyList<Draft> Drafts, IReadOnlyList<QueuedMessage> Outbox);

public sealed class NativeProbeException : Exception
{
    public string Code { get; }

    public NativeProbeException(string code, string message, Exception? innerException = null)
        : base(message, innerException) => Code = code;
}
