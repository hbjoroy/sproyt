using System.Security.Cryptography;
using System.Text;

namespace Sproyt.WindowsProbe;

internal static class NativeUploadDiskStoreProbe
{
    internal static async Task RunAsync(string freshDirectory)
    {
        if (Directory.Exists(freshDirectory) || File.Exists(freshDirectory))
            throw new IOException("Upload store probe requires a fresh directory.");
        Directory.CreateDirectory(freshDirectory);
        string account = Guid.NewGuid().ToString("D"), jobId = Guid.NewGuid().ToString("D");
        var target = new NativeUploadTarget(account, "login-family", Guid.NewGuid().ToString("D"), Guid.NewGuid().ToString("D"));
        var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var allowReturn = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var retained = new List<string[]>();
        var limit = TimeSpan.FromSeconds(10);
        byte[] bytes = Encoding.UTF8.GetBytes("private attachment");
        NativeUploadJob ready;
        string sourcePath;
        NativeUploadDiskStore Open() => new(freshDirectory, account, async (actual, id, marker) =>
        {
            Check(actual == target && id == jobId && marker.Contains("|text/plain|"), "Wrong target or marker.");
            entered.TrySetResult(); await allowReturn.Task.WaitAsync(limit);
            return true;
        }, ids => { retained.Add(ids.ToArray()); return Task.CompletedTask; });

        using (var store = Open())
        {
            Check((await store.LoadAsync(default)).Count == 0, "New journal was not empty.");
            await RejectAsync<InvalidOperationException>(() => { using var other = Open(); return Task.CompletedTask; });
            using var moving = new MovingLengthStream(bytes);
            await RejectAsync<IOException>(() => store.StageAsync(moving, "moving.txt", "text/plain", default));
            var source = await store.StageAsync(new MemoryStream(bytes), "private-name.txt", "text/plain", default);
            Check(source.Sha256 == Convert.ToHexStringLower(SHA256.HashData(bytes)), "Incorrect staged digest.");
            var attachment = new NativeUploadedAttachment(Guid.NewGuid().ToString("D"), source.FileName, source.ContentType, source.SizeBytes, source.Sha256);
            ready = new NativeUploadJob(jobId, target, source, jobId, State: NativeUploadState.Ready, Attachment: attachment);
            await RejectAsync<IOException>(() => store.AttachAsync(target, attachment, jobId, default));
            await RejectAsync<IOException>(() => store.SaveAsync([ready with { Source = source with { SourceId = "AAAAAAAA-0000-0000-0000-000000000001" } }], default));
            await store.SaveAsync([ready], default);
            await store.ReleaseStagedAsync(source.SourceId);
            sourcePath = Directory.GetFiles(freshDirectory, source.SourceId + ".bin", SearchOption.AllDirectories).Single();
            var journal = File.ReadAllBytes(Directory.GetFiles(freshDirectory, "state.bin", SearchOption.AllDirectories).Single());
            Check(!Encoding.UTF8.GetString(journal).Contains(source.FileName), "Upload journal contained a plaintext filename.");
            using var contents = await store.OpenReadAsync(source, default);
            var read = new byte[bytes.Length]; await contents.ReadExactlyAsync(read);
            Check(read.SequenceEqual(bytes), "Private source bytes changed.");
        }
        using (var store = Open())
        {
            Check((await store.LoadAsync(default)).Single() == ready, "Durable job failed to roundtrip.");
            await File.WriteAllBytesAsync(sourcePath, new byte[bytes.Length]);
            await RejectAsync<IOException>(() => store.OpenReadAsync(ready.Source, default));
            await File.WriteAllBytesAsync(sourcePath, bytes);
            await RejectAsync<IOException>(() => store.AttachAsync(target with { FamilyStamp = "stale" }, ready.Attachment!, jobId, default));
            await RejectAsync<IOException>(() => store.AttachAsync(target, ready.Attachment! with { MediaId = Guid.NewGuid().ToString("D") }, jobId, default));
            var attach = store.AttachAsync(target, ready.Attachment!, jobId, default);
            await entered.Task.WaitAsync(limit);
            var retire = store.SaveAsync([], default);
            Check(!retire.IsCompleted && File.Exists(sourcePath), "The journal collected an active handoff.");
            allowReturn.SetResult();
            Check(await attach.WaitAsync(limit), "Handoff did not report completion.");
            await retire.WaitAsync(limit);
            Check(!File.Exists(sourcePath) && retained.Last().Length == 0, "Retirement left its source or receipt retained.");
            await RejectAsync<IOException>(() => store.AttachAsync(target, ready.Attachment!, jobId, default));
        }
    }

    private static void Check(bool ok, string message)
    {
        if (!ok) throw new InvalidOperationException(message);
    }

    private static async Task RejectAsync<T>(Func<Task> operation) where T : Exception
    {
        try { await operation(); }
        catch (T) { return; }
        throw new InvalidOperationException("Expected " + typeof(T).Name);
    }

    private sealed class MovingLengthStream(byte[] bytes) : MemoryStream(bytes)
    {
        private int _reads;
        public override long Length => Interlocked.Increment(ref _reads) == 1 ? base.Length - 1 : base.Length;
    }
}
