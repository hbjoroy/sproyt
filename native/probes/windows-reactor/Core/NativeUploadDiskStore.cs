using System.Security.Cryptography;
using System.Text;

namespace Sproyt.WindowsProbe;

// One account owns both the encrypted journal lease and immutable private
// source copies. The queue never receives an arbitrary filesystem path.
internal sealed class NativeUploadDiskStore : INativeUploadLocalStore, IDisposable
{
    internal const long MaximumSpoolBytes = 512L * 1024 * 1024;
    private readonly string _root, _spool, _account;
    private readonly WindowsProtectedJournal _journal;
    private readonly SemaphoreSlim _gate = new(1, 1);
    private readonly Func<NativeUploadTarget, string, string, Task<bool>> _attach;
    private readonly Func<IReadOnlyList<string>, Task> _reconcile;
    private NativeUploadJob[] _jobs = [];
    private readonly HashSet<string> _staging = [];
    private long _reservedSpoolBytes;
    private bool _disposed;
    private sealed record State(int Schema, string Account, NativeUploadJob[] Jobs);

    internal NativeUploadDiskStore(string directory, string account,
        Func<NativeUploadTarget, string, string, Task<bool>> attach, Func<IReadOnlyList<string>, Task> reconcile)
    {
        _account = account; _attach = attach; _reconcile = reconcile;
        string accountKey = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(account))).ToLowerInvariant();
        _root = Path.GetFullPath(Path.Combine(directory, "uploads-" + accountKey));
        _spool = Path.Combine(_root, "sources");
        Directory.CreateDirectory(_root); RequireDirectory(_root);
        Directory.CreateDirectory(_spool); RequireDirectory(_spool);
        string journal = Path.Combine(_root, "journal");
        bool exists = File.Exists(Path.Combine(journal, "state.bin"));
        _journal = new(journal, create: !exists);
        try
        {
            // The existence check before opening the lease can go stale while
            // a competing first opener creates, saves and closes the journal.
            if (!File.Exists(Path.Combine(journal, "state.bin"))) _journal.Write(new State(1, _account, []));
        }
        catch { _journal.Dispose(); throw; }
    }

    public async Task<IReadOnlyList<NativeUploadJob>> LoadAsync(CancellationToken cancellationToken)
    {
        await _gate.WaitAsync(cancellationToken);
        try
        {
            RequireOpen(); var state = _journal.Read<State>();
            if (state.Schema != 1 || state.Account != _account) throw new IOException("Opplastingslageret høyrer ikkje til denne kontoen.");
            ValidateJobs(state.Jobs); _jobs = state.Jobs;
            // The journal lease is exclusive. There can be no new admission
            // between this complete durable ID set and receipt collection.
            await _reconcile(_jobs.Select(job => job.Id).ToArray());
            CleanUnreferencedSources();
            return Array.AsReadOnly(_jobs.ToArray());
        }
        finally { _gate.Release(); }
    }

    public async Task SaveAsync(IReadOnlyList<NativeUploadJob> jobs, CancellationToken cancellationToken)
    {
        await _gate.WaitAsync(cancellationToken);
        try
        {
            RequireOpen(); ValidateJobs(jobs);
            var next = jobs.ToArray();
            _journal.Write(new State(1, _account, next));
            _jobs = next;
            _staging.RemoveWhere(id => next.Any(job => job.Source.SourceId == id));
            // Never collect a receipt until retirement is durably journalled.
            // Collection failure is recoverable on the next Load/Save; the
            // successful journal write itself must not be reported as lost.
            try { await _reconcile(next.Select(job => job.Id).ToArray()); }
            catch (Exception error) when (error is IOException or NativeProbeException or InvalidOperationException) { }
            try { CleanUnreferencedSources(); }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException) { }
        }
        finally { _gate.Release(); }
    }

    internal async Task<NativeUploadSource> StageFileAsync(string path, CancellationToken cancellationToken)
    {
        string full = Path.GetFullPath(path);
        if ((File.GetAttributes(full) & (FileAttributes.Directory | FileAttributes.ReparsePoint)) != 0)
            throw new IOException("Vel ei vanleg fil.");
        await using var source = new FileStream(full, FileMode.Open, FileAccess.Read, FileShare.Read, 65536,
            FileOptions.Asynchronous | FileOptions.SequentialScan);
        return await StageAsync(source, Path.GetFileName(full), ContentTypeFor(full), cancellationToken);
    }

    internal async Task<NativeUploadSource> StageAsync(Stream source, string fileName, string contentType, CancellationToken cancellationToken)
    {
        long expected = source.CanRead && source.CanSeek ? source.Length : 0;
        if (expected is <= 0 or > MaximumSpoolBytes)
            throw new IOException("Fila må vere mellom 1 byte og 512 MiB i denne Windows-versjonen.");
        string id = Guid.NewGuid().ToString("D"), final = SourcePath(id), temporary = final + ".part";
        await _gate.WaitAsync(cancellationToken);
        try
        {
            RequireOpen();
            if (_staging.Count + _jobs.Length >= 8) throw new IOException("Opplastingskøen er full. Fjern eller fullfør ei fil først.");
            long used = Directory.EnumerateFiles(_spool).Where(file => !file.EndsWith(".part", StringComparison.Ordinal)).Sum(file => new FileInfo(file).Length);
            if (expected > MaximumSpoolBytes - used - _reservedSpoolBytes) throw new IOException("Det lokale opplastingslageret er fullt.");
            _staging.Add(id); _reservedSpoolBytes += expected;
        }
        finally { _gate.Release(); }
        bool complete = false;
        try
        {
            long total = 0;
            using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
            await using (var output = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None, 65536,
                FileOptions.Asynchronous | FileOptions.WriteThrough))
            {
                byte[] buffer = new byte[1024 * 1024]; int count;
                while ((count = await source.ReadAsync(buffer, cancellationToken)) != 0)
                {
                    total += count;
                    if (total > expected) throw new IOException("Fila endra seg medan ho vart klargjord.");
                    hash.AppendData(buffer, 0, count); await output.WriteAsync(buffer.AsMemory(0, count), cancellationToken);
                }
                if (total != expected || source.Length != expected) throw new IOException("Fila endra seg medan ho vart klargjord.");
                await output.FlushAsync(cancellationToken); output.Flush(flushToDisk: true);
            }
            File.Move(temporary, final);
            complete = true;
            return new(id, SafeFileName(fileName), contentType, total, Convert.ToHexString(hash.GetHashAndReset()).ToLowerInvariant());
        }
        finally
        {
            if (!complete) { TryDelete(temporary); TryDelete(final); }
            // A successful private copy stays protected from collection until
            // Enqueue has journalled it or its caller explicitly releases it.
            await _gate.WaitAsync(CancellationToken.None);
            try { _reservedSpoolBytes -= expected; if (!complete) _staging.Remove(id); }
            finally { _gate.Release(); }
        }
    }

    internal async Task ReleaseStagedAsync(string sourceId)
    {
        await _gate.WaitAsync();
        try { _staging.Remove(sourceId); CleanUnreferencedSources(); }
        finally { _gate.Release(); }
    }

    public async Task<Stream> OpenReadAsync(NativeUploadSource source, CancellationToken cancellationToken)
    {
        await _gate.WaitAsync(cancellationToken);
        try
        {
            RequireOpen(); string path = SourcePath(source.SourceId);
            if (!_jobs.Any(job => job.Source == source)) throw new IOException("Fila høyrer ikkje til ei lagra opplasting.");
            RequireFile(path);
            var input = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, 65536,
                FileOptions.Asynchronous | FileOptions.SequentialScan);
            try
            {
                if (input.Length != source.SizeBytes) throw new IOException("Den lokale filkopien er endra.");
                byte[] digest = await SHA256.HashDataAsync(input, cancellationToken);
                if (!CryptographicOperations.FixedTimeEquals(digest, Convert.FromHexString(source.Sha256)))
                    throw new IOException("Den lokale filkopien er endra.");
                input.Position = 0; return input;
            }
            catch { await input.DisposeAsync(); throw; }
        }
        finally { _gate.Release(); }
    }

    public async Task<bool> AttachAsync(NativeUploadTarget target, NativeUploadedAttachment attachment, string jobId, CancellationToken cancellationToken)
    {
        // Keep the durable Ready record pinned until the receipt callback
        // finishes. Save cannot retire the job and collect its receipt midway.
        await _gate.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            RequireOpen();
            if (target.UserId != _account || !_jobs.Any(job => job.Id == jobId && job.Target == target &&
                job.State == NativeUploadState.Ready && job.Attachment == attachment))
                throw new IOException("Opplastinga samsvarer ikkje med den lagra jobben.");
            cancellationToken.ThrowIfCancellationRequested();
            return await _attach(target, jobId, attachment.Marker).ConfigureAwait(false);
        }
        finally { _gate.Release(); }
    }

    private static bool CanonicalId(string id) => Guid.TryParseExact(id, "D", out var parsed) && id == parsed.ToString("D");

    private void ValidateJobs(IReadOnlyList<NativeUploadJob> jobs)
    {
        if (jobs.Count > 8 || jobs.Select(job => job.Id).Distinct(StringComparer.Ordinal).Count() != jobs.Count ||
            jobs.Any(job => job.Target.UserId != _account || !CanonicalId(job.Id) ||
                !CanonicalId(job.Source.SourceId) || job.Source.SizeBytes is <= 0 or > MaximumSpoolBytes ||
                job.Source.Sha256.Length != 64 || !job.Source.Sha256.All(char.IsAsciiHexDigit)))
            throw new IOException("Opplastingsjournalen er ugyldig.");
    }
    private string SourcePath(string id)
    {
        if (!CanonicalId(id)) throw new IOException("Ugyldig lokal filidentitet.");
        return Path.Combine(_spool, id + ".bin");
    }
    private static void RequireDirectory(string directory)
    {
        if ((File.GetAttributes(directory) & FileAttributes.ReparsePoint) != 0) throw new IOException("Ugyldig lokalt opplastingslager.");
    }
    private static void RequireFile(string path)
    {
        if ((File.GetAttributes(path) & (FileAttributes.ReparsePoint | FileAttributes.Directory)) != 0) throw new IOException("Ugyldig lokal filkopi.");
    }
    private void RequireOpen()
    {
        ObjectDisposedException.ThrowIf(_disposed, this); RequireDirectory(_root); RequireDirectory(_spool);
    }
    private void CleanUnreferencedSources()
    {
        RequireOpen();
        var retained = _jobs.Select(job => job.Source.SourceId).Concat(_staging).ToHashSet(StringComparer.Ordinal);
        foreach (string path in Directory.EnumerateFiles(_spool))
        {
            string name = Path.GetFileName(path);
            string id = name.EndsWith(".bin.part", StringComparison.Ordinal) ? name[..^9] : name.EndsWith(".bin", StringComparison.Ordinal) ? name[..^4] : "";
            if (CanonicalId(id) && !retained.Contains(id) && (File.GetAttributes(path) & FileAttributes.ReparsePoint) == 0)
                TryDelete(path);
        }
    }
    private static void TryDelete(string path) { try { File.Delete(path); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
    internal static string SafeFileName(string name)
    {
        var safe = new StringBuilder();
        foreach (var rune in Path.GetFileName(name).EnumerateRunes())
        {
            if (Rune.IsControl(rune) || rune.Value is '/' or '\\') continue;
            if (safe.Length + rune.Utf16SequenceLength > 200) break;
            safe.Append(rune.ToString());
        }
        return safe.Length == 0 ? "Vedlegg" : safe.ToString();
    }
    internal static string ContentTypeFor(string path) => Path.GetExtension(path).ToLowerInvariant() switch
    {
        ".png" => "image/png", ".jpg" or ".jpeg" => "image/jpeg", ".gif" => "image/gif", ".webp" => "image/webp",
        ".heic" or ".heif" => "image/heic", ".avif" => "image/avif", ".mp4" => "video/mp4", ".mov" => "video/quicktime",
        ".webm" => "video/webm", ".mkv" => "video/x-matroska", ".pdf" => "application/pdf", ".zip" => "application/zip",
        ".txt" => "text/plain", ".csv" => "text/csv", _ => throw new IOException("Denne filtypen er ikkje støtta. Vel bilete, video, PDF, tekst/CSV eller ZIP.")
    };
    public void Dispose() { _disposed = true; _journal.Dispose(); }
}
