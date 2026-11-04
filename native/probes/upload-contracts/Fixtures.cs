using System.Security.Cryptography;
using Sproyt.WindowsProbe;

internal static partial class Program
{
    private sealed class Local : INativeUploadLocalStore
    {
        internal NativeUploadJob[] Saved = [];
        internal readonly Dictionary<string, byte[]> Bytes = [];
        internal readonly Dictionary<string, (NativeUploadTarget Target, NativeUploadedAttachment Attachment)> Attached = [];
        internal Func<IReadOnlyList<NativeUploadJob>, bool>? FailSave;
        internal bool LoseAttachAfterCommit, AllowAttach = true, HoldAttach, FailSaveBefore, HoldSave;
        internal readonly TaskCompletionSource AttachEntered = new(TaskCreationOptions.RunContinuationsAsynchronously), ReleaseAttach = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal readonly TaskCompletionSource SaveEntered = new(TaskCreationOptions.RunContinuationsAsynchronously), ReleaseSave = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal int AttachCalls, MaximumRead;
        internal NativeUploadSource Source(int size)
        {
            byte[] bytes = Enumerable.Range(0, size).Select(index => (byte)(index % 251)).ToArray(); string id = Guid.NewGuid().ToString("D"); Bytes[id] = bytes;
            return new(id, "prøve%20 | [fil].png", "image/png", size, Convert.ToHexStringLower(SHA256.HashData(bytes)));
        }
        public Task<IReadOnlyList<NativeUploadJob>> LoadAsync(CancellationToken cancellationToken) => Task.FromResult<IReadOnlyList<NativeUploadJob>>(Saved.ToArray());
        public async Task SaveAsync(IReadOnlyList<NativeUploadJob> jobs, CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (HoldSave) { HoldSave = false; SaveEntered.TrySetResult(); await ReleaseSave.Task.WaitAsync(cancellationToken); }
            if (FailSaveBefore) { FailSaveBefore = false; throw new IOException("Injected pre-commit journal failure."); }
            Saved = jobs.ToArray();
            if (FailSave?.Invoke(jobs) == true) { FailSave = null; throw new IOException("Injected post-commit journal failure."); }
        }
        public Task<Stream> OpenReadAsync(NativeUploadSource source, CancellationToken cancellationToken) =>
            Task.FromResult<Stream>(new Tracked(Bytes[source.SourceId], count => MaximumRead = Math.Max(MaximumRead, count)));
        public async Task<bool> AttachAsync(NativeUploadTarget target, NativeUploadedAttachment attachment, string jobId, CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested(); AttachCalls++; AttachEntered.TrySetResult();
            if (HoldAttach) await ReleaseAttach.Task.WaitAsync(cancellationToken);
            if (!AllowAttach) return false;
            if (!Attached.ContainsKey(jobId)) Attached.Add(jobId, (target, attachment));
            if (LoseAttachAfterCommit) { LoseAttachAfterCommit = false; throw new IOException("Injected lost draft commit result."); }
            return true;
        }
    }
    private sealed class Wire(Local local) : INativeUploadTransport
    {
        private sealed class Upload(NativeUploadJob job)
        { internal readonly string Id = Guid.NewGuid().ToString("D"); internal readonly NativeUploadJob Job = job; internal readonly List<byte> Bytes = []; internal readonly List<string> Hashes = []; internal bool Complete; }
        private readonly Dictionary<string, Upload> _uploads = [];
        internal bool LoseCreate, LoseChunk, LoseComplete, HoldChunk;
        internal int Creates, Statuses, ChunkCalls, UniqueChunks, Completes, Aborts, MaximumChunk;
        internal bool FirstDispatchWasJournaled = true;
        internal readonly TaskCompletionSource ChunkEntered = new(TaskCreationOptions.RunContinuationsAsynchronously), ReleaseChunk = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal readonly TaskCompletionSource FirstHttp = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public Task<NativeUploadConfiguration> ConfigurationAsync(NativeUploadTarget target, CancellationToken cancellationToken)
        {
            FirstHttp.TrySetResult();
            return Task.FromResult(new NativeUploadConfiguration(true, NativeUploadHttpClient.MaximumFileBytes, NativeUploadHttpClient.ChunkBytes, [Channel]));
        }
        public Task<NativeUploadStatus> CreateAsync(NativeUploadJob job, CancellationToken cancellationToken)
        {
            Creates++; FirstDispatchWasJournaled &= local.Saved.Any(saved => saved.Id == job.Id && saved.RequestId == job.RequestId && saved.Target == job.Target);
            if (!_uploads.TryGetValue(job.RequestId, out var upload)) { upload = new(job); _uploads.Add(job.RequestId, upload); }
            if (LoseCreate) { LoseCreate = false; throw new IOException("Lost create reply after commit."); }
            return Task.FromResult(Status(upload));
        }
        private Upload Find(NativeUploadJob job) => _uploads.Values.Single(upload => upload.Id == job.UploadId);
        public Task<NativeUploadStatus> StatusAsync(NativeUploadJob job, CancellationToken cancellationToken) { Statuses++; return Task.FromResult(Status(Find(job))); }
        public async Task<NativeUploadStatus> PutChunkAsync(NativeUploadJob job, byte[] bytes, int count, string sha256, CancellationToken cancellationToken)
        {
            ChunkCalls++; MaximumChunk = Math.Max(MaximumChunk, count); ChunkEntered.TrySetResult();
            if (HoldChunk) await ReleaseChunk.Task.WaitAsync(cancellationToken);
            cancellationToken.ThrowIfCancellationRequested(); var upload = Find(job);
            Require(sha256 == Convert.ToHexStringLower(SHA256.HashData(bytes.AsSpan(0, count))), "Wire hash mismatch.");
            if (job.NextChunk < upload.Hashes.Count) Require(upload.Hashes[job.NextChunk] == sha256, "Retry changed committed chunk bytes.");
            else { Require(job.NextChunk == upload.Hashes.Count, "Chunk skipped a gap."); upload.Hashes.Add(sha256); upload.Bytes.AddRange(bytes.AsSpan(0, count).ToArray()); UniqueChunks++; }
            if (LoseChunk) { LoseChunk = false; throw new IOException("Lost chunk reply after commit."); }
            return Status(upload);
        }
        public Task<NativeUploadedAttachment> CompleteAsync(NativeUploadJob job, CancellationToken cancellationToken)
        {
            Completes++; var upload = Find(job); Require(upload.Bytes.Count == job.Source.SizeBytes, "Completed a partial file."); upload.Complete = true;
            if (LoseComplete) { LoseComplete = false; throw new IOException("Lost completion reply after commit."); }
            return Task.FromResult(new NativeUploadedAttachment(Media, job.Source.FileName, job.Source.ContentType, upload.Bytes.Count, Convert.ToHexStringLower(SHA256.HashData(upload.Bytes.ToArray()))));
        }
        public Task AbortAsync(NativeUploadJob job, CancellationToken cancellationToken) { Aborts++; return Task.CompletedTask; }
        private static NativeUploadStatus Status(Upload upload) => new(upload.Id, upload.Job.Target.ChannelId, upload.Job.Source.FileName,
            upload.Job.Source.ContentType, upload.Job.Source.SizeBytes, upload.Bytes.Count, upload.Hashes.Count, upload.Complete ? "complete" : "open", upload.Complete ? Media : null, NativeUploadHttpClient.ChunkBytes);
    }
    private sealed class Tracked(byte[] bytes, Action<int> read) : MemoryStream(bytes, writable: false)
    {
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default) { read(buffer.Length); return base.ReadAsync(buffer, cancellationToken); }
    }
    private sealed class NonSeekable(byte[] bytes) : Stream
    {
        private readonly MemoryStream _stream = new(bytes);
        public override bool CanRead => true; public override bool CanSeek => false; public override bool CanWrite => false;
        public override long Length => throw new NotSupportedException(); public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
        public override int Read(byte[] buffer, int offset, int count) => _stream.Read(buffer, offset, count);
        public override ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default) => _stream.ReadAsync(buffer, cancellationToken);
        public override void Flush() => throw new NotSupportedException(); public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException(); public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        protected override void Dispose(bool disposing) { if (disposing) _stream.Dispose(); base.Dispose(disposing); }
    }
    private sealed class ImmediateClock : TimeProvider
    {
        public override ITimer CreateTimer(TimerCallback callback, object? state, TimeSpan dueTime, TimeSpan period) => new ImmediateTimer(callback, state, dueTime);
        private sealed class ImmediateTimer(TimerCallback callback, object? state, TimeSpan due) : ITimer
        {
            private int _disposed;
            private readonly Task _start = Start(callback, state, due);
            private static Task Start(TimerCallback callback, object? state, TimeSpan due) => due == Timeout.InfiniteTimeSpan ? Task.CompletedTask : Task.Run(() => callback(state));
            public bool Change(TimeSpan dueTime, TimeSpan period) { if (Volatile.Read(ref _disposed) != 0) return false; _ = Start(callback, state, dueTime); return true; }
            public void Dispose() => Interlocked.Exchange(ref _disposed, 1);
            public ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }
        }
    }
}
