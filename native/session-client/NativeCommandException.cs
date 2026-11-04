// A correlated server rejection is different from a missing acknowledgement.
// Only known domain errors prove that this command did not commit. In
// particular, storage/server/unknown errors must retain the original send ID.
internal sealed class NativeCommandException : InvalidOperationException
{
    internal string RequestId { get; }
    internal string? Code { get; }
    internal bool IsDefinitiveRejection { get; }

    private NativeCommandException(string requestId, string? code, string message, bool definitive)
        : base(message)
    {
        RequestId = requestId;
        Code = code;
        IsDefinitiveRejection = definitive;
    }

    internal static NativeCommandException FromServer(string requestId, string code, string message) =>
        new(requestId, code, message, code is "validation_error" or "permission_denied" or "not_found" or "conflict" or "quota");

    internal static NativeCommandException Uncertain(string requestId) =>
        new(requestId, null, "Vi fekk ikkje stadfesta førespurnaden. Prøv igjen med same førespurnads-ID.", false);
}
