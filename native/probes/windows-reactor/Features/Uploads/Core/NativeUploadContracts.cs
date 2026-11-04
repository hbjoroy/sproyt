namespace Sproyt.WindowsProbe;

internal sealed record NativeUploadTarget(string UserId, string FamilyStamp, string ChannelId, string? ParentMessageId = null)
{
    internal NativeMediaScope Scope => new(UserId, FamilyStamp);
}
internal sealed record NativeUploadSource(string SourceId, string FileName, string ContentType, long SizeBytes, string Sha256);
internal sealed record NativeUploadedAttachment(string MediaId, string FileName, string ContentType, long SizeBytes, string Sha256)
{
    internal string Marker
    {
        get
        {
            string prefix = "[[media:" + MediaId + "|" + ContentType + "|";
            int budget = Math.Min(512, 1024 - System.Text.Encoding.UTF8.GetByteCount(prefix) - 2);
            var name = new System.Text.StringBuilder();
            foreach (var rune in FileName.EnumerateRunes().Take(120))
            {
                string encoded = Uri.EscapeDataString(System.Text.Rune.IsControl(rune) ? "_" : rune.ToString());
                if (name.Length + encoded.Length > budget) break;
                name.Append(encoded);
            }
            return prefix + name + "]]";
        }
    }
}
internal enum NativeUploadState { Queued, Uploading, Finalizing, Ready, Attached, Cancelled, Failed }
internal sealed record NativeUploadJob(string Id, NativeUploadTarget Target, NativeUploadSource Source, string RequestId,
    string? UploadId = null, int NextChunk = 0, long UploadedBytes = 0, NativeUploadState State = NativeUploadState.Queued,
    NativeUploadedAttachment? Attachment = null, string? Error = null, bool Retryable = true, bool HandoffStarted = false);
internal sealed record NativeUploadSnapshot(IReadOnlyList<NativeUploadJob> Jobs, string? StorageError, bool Busy, string? ActiveFamilyStamp = null);

// The host owns private immutable spooling and atomic journal/draft writes.
// SaveAsync includes every login family for this account. The host supplies an
// account-owned store; Capture never exposes another user's file metadata.
internal interface INativeUploadLocalStore
{
    Task<IReadOnlyList<NativeUploadJob>> LoadAsync(CancellationToken cancellationToken);
    Task SaveAsync(IReadOnlyList<NativeUploadJob> jobs, CancellationToken cancellationToken);
    Task<Stream> OpenReadAsync(NativeUploadSource source, CancellationToken cancellationToken);
    // Idempotent by jobId; merge into precisely target's durable draft and its
    // receipt before returning true. False keeps Ready for explicit retry.
    Task<bool> AttachAsync(NativeUploadTarget target, NativeUploadedAttachment attachment, string jobId, CancellationToken cancellationToken);
}

internal sealed record NativeUploadConfiguration(bool Enabled, long MaximumFileBytes, int ChunkBytes, IReadOnlyList<string>? AllowedChannelIds);
internal sealed record NativeUploadStatus(string Id, string ChannelId, string FileName, string ContentType, long SizeBytes,
    long ReceivedBytes, int NextChunk, string State, string? MediaId, int ChunkBytes);
internal interface INativeUploadTransport
{
    Task<NativeUploadConfiguration> ConfigurationAsync(NativeUploadTarget target, CancellationToken cancellationToken);
    Task<NativeUploadStatus> CreateAsync(NativeUploadJob job, CancellationToken cancellationToken);
    Task<NativeUploadStatus> StatusAsync(NativeUploadJob job, CancellationToken cancellationToken);
    Task<NativeUploadStatus> PutChunkAsync(NativeUploadJob job, byte[] bytes, int count, string sha256, CancellationToken cancellationToken);
    Task<NativeUploadedAttachment> CompleteAsync(NativeUploadJob job, CancellationToken cancellationToken);
    Task AbortAsync(NativeUploadJob job, CancellationToken cancellationToken);
}

internal sealed class NativeUploadRejectedException(string message, bool retryable = false) : IOException(message)
{
    internal bool Retryable { get; } = retryable;
}
