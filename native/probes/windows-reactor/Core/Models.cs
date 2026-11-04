namespace Sproyt.WindowsProbe;

public sealed record Draft(string Channel, string Body, long Revision = 0, string? ParentMessageId = null)
{
    public string Key => new ComposerTarget(Channel, ParentMessageId).Key;
    public string ComposerText { get; init; } = Body;
    public IReadOnlyList<string> Attachments { get; init; } = [];
}

public sealed record QueuedMessage(string RequestId, string Channel, string Body, string State, string? ParentMessageId = null);

// Storage and wire use explicit channel/parent fields. This collision-free key
// only indexes the existing in-memory workspace; valid server IDs cannot
// contain the leading control character used to distinguish a thread target.
public sealed record ComposerTarget(string ChannelId, string? ParentMessageId = null)
{
    public string Key => ParentMessageId is null ? ChannelId : "\u001f" + System.Text.Json.JsonSerializer.Serialize(new[] { ChannelId, ParentMessageId });
    internal static bool IsThreadKey(string key) => key.StartsWith('\u001f');
    public static ComposerTarget FromKey(string key)
    {
        if (!IsThreadKey(key)) return new(key);
        var parts = System.Text.Json.JsonSerializer.Deserialize<string[]>(key[1..]);
        if (parts is not { Length: 2 } || parts.Any(string.IsNullOrEmpty) ||
            parts.Any(part => System.Text.Encoding.UTF8.GetByteCount(part) > 128 || part.Any(char.IsControl)))
            throw new ArgumentException("Invalid composer target.", nameof(key));
        return new(parts[0], parts[1]);
    }
}

public sealed record DraftComposer(string Text, IReadOnlyList<string> Attachments);

public sealed record UploadDraftResult(bool AlreadyApplied, string OperationId, string? Body, long Revision);

public sealed record StoreSnapshot(IReadOnlyList<Draft> Drafts, IReadOnlyList<QueuedMessage> Outbox);

public sealed class NativeProbeException : Exception
{
    public string Code { get; }

    public NativeProbeException(string code, string message, Exception? innerException = null)
        : base(message, innerException) => Code = code;
}
