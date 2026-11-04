using System.Net;

namespace Sproyt.WindowsProbe;

internal sealed class NativePreviewCapacityException() : InvalidOperationException("Trykk Last førehandsvising når andre bilete er ute av visinga.");
internal sealed class NativePreviewLease(byte[] bytes, Action releasePipeline, Action releaseSlot) : IDisposable
{
    private readonly object _gate = new();
    private Action? _pipeline = releasePipeline, _slot = releaseSlot;
    private bool _decoding, _disposed;
    internal byte[] Bytes { get; private set; } = bytes;
    internal bool BeginDecode() { lock (_gate) { if (_disposed) return false; _decoding = true; return true; } }
    internal void Decoded()
    {
        Action? release;
        lock (_gate) { Bytes = []; _decoding = false; release = _pipeline; _pipeline = null; }
        release?.Invoke();
    }
    public void Dispose()
    {
        Action? pipeline = null, slot;
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true; Bytes = []; slot = _slot; _slot = null;
            // Removing a control must not admit another decode until the old
            // native async decoder has actually finished/cancelled.
            if (!_decoding) { pipeline = _pipeline; _pipeline = null; }
        }
        pipeline?.Invoke(); slot?.Invoke();
    }
}

internal sealed class NativeMediaFile(string path, string contentType, Action<string> release) : IDisposable
{
    internal string Path { get; } = path;
    internal string ContentType { get; } = contentType;
    public void Dispose() => release(Path);
}

// Previews are bounded in memory. Originals are fetched only after an explicit
// Open/Play and streamed to a private temporary file, never a whole-video buffer.
internal sealed class NativeMediaLibrary : IDisposable
{
    private const long MaxFileBytes = 512L * 1024 * 1024;
    private readonly NativeAuthSession _owner;
    private readonly string _folder;
    private readonly SemaphoreSlim _previews = new(2, 2), _previewSlots = new(8, 8), _originals = new(1, 1);
    private sealed class Family(NativeMediaScope scope)
    {
        internal NativeMediaScope Scope { get; } = scope;
        internal CancellationTokenSource Cancellation { get; } = new();
        internal HashSet<string> Files { get; } = [];
    }
    private Family? _family;
    private readonly object _gate = new();
    private volatile bool _disposed;
    internal event Action? Invalidated;
    internal event Action? PreviewSlotAvailable;
    internal bool HasPreviewCapacity => _previewSlots.CurrentCount > 0;
    private void ReleasePreviewSlot()
    {
        _previewSlots.Release();
        PreviewSlotAvailable?.Invoke();
    }
    internal NativeMediaLibrary(NativeAuthSession owner, string directory)
    {
        _owner = owner;
        string root = System.IO.Path.GetFullPath(System.IO.Path.Combine(directory, "media"));
        Directory.CreateDirectory(root);
        foreach (string stale in Directory.GetDirectories(root, "media-tmp-*"))
        {
            string full = System.IO.Path.GetFullPath(stale);
            if (full.StartsWith(root + System.IO.Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase) &&
                !new DirectoryInfo(full).Attributes.HasFlag(FileAttributes.ReparsePoint) &&
                Guid.TryParseExact(System.IO.Path.GetFileName(full)["media-tmp-".Length..], "N", out _))
                try { Directory.Delete(full, true); } catch (IOException) { } catch (UnauthorizedAccessException) { }
        }
        _folder = System.IO.Path.Combine(root, "media-tmp-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(_folder);
        _owner.Changed += OwnerChanged;
    }
    internal NativeMediaScope Capture() => _owner.MediaScope();
    internal NativeMediaScope? TryCapture() { try { return Capture(); } catch (InvalidOperationException) { return null; } }
    internal bool Current(NativeMediaScope scope) => !_disposed && _owner.MediaScopeCurrent(scope);
    private CancellationTokenSource Link(NativeMediaScope scope, CancellationToken cancellation)
    {
        Family? retired = null;
        var linked = _owner.ProjectSnapshot(_ =>
        {
            // Bind the cancellation source in the same auth admission as the
            // family check. Logout cannot run between those two operations.
            if (!Current(scope)) throw new OperationCanceledException();
            lock (_gate)
            {
                if (_disposed) throw new OperationCanceledException();
                if (_family?.Scope != scope)
                {
                    retired = _family;
                    _family = new(scope);
                }
                return CancellationTokenSource.CreateLinkedTokenSource(cancellation, _family.Cancellation.Token);
            }
        });
        if (retired is not null) Retire(retired);
        return linked;
    }
    private void OwnerChanged()
    {
        var retired = _owner.ProjectSnapshot(_ =>
        {
            lock (_gate)
            {
                var family = _family;
                if (family is null || _owner.MediaScopeCurrent(family.Scope)) return null;
                _family = null;
                return family;
            }
        });
        if (retired is not null) Retire(retired);
    }
    private void Retire(Family family)
    {
        // Once detached, the family's file set is immutable. A subscriber may
        // open a newer family's original during this callback.
        try { family.Cancellation.Cancel(); }
        catch (AggregateException) { /* A failed cancellation observer cannot veto retirement. */ }
        finally
        {
            family.Cancellation.Dispose();
            try { Invalidated?.Invoke(); } catch { }
            foreach (string path in family.Files) DeleteFile(path);
        }
    }
    private static bool DeleteFile(string path)
    {
        try { File.Delete(path); return true; }
        catch (IOException) { return false; }
        catch (UnauthorizedAccessException) { return false; }
    }
    private void ReleaseFile(string path)
    {
        if (!DeleteFile(path)) return;
        lock (_gate) _family?.Files.Remove(path);
        if (_disposed) RemoveEmptyFolder();
    }
    private void RemoveEmptyFolder()
    {
        try { Directory.Delete(_folder); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }
    internal async Task<NativePreviewLease> PreviewAsync(string id, NativeMediaScope scope, CancellationToken cancellation)
    {
        using var linked = Link(scope, cancellation); linked.CancelAfter(TimeSpan.FromSeconds(20));
        // A slot survives fetching and decoding, until its displayed bitmap is
        // cleared/unmounted. There is no unbounded queue of preview requests.
        if (!_previewSlots.Wait(0)) throw new NativePreviewCapacityException();
        bool pipeline = false, handedOff = false;
        try
        {
            await _previews.WaitAsync(linked.Token); pipeline = true;
            using var response = await _owner.ReadMediaAsync(id, true, scope, linked.Token);
            if (response.StatusCode == HttpStatusCode.Accepted) throw new InvalidOperationException("Førehandsvising blir laga. Prøv igjen om litt.");
            response.EnsureSuccessStatusCode();
            string type = response.Content.Headers.ContentType?.MediaType ?? "";
            if (type is not ("image/jpeg" or "image/png" or "image/webp" or "image/gif")) throw new InvalidOperationException("Bruk Opne for å sjå vedlegget.");
            if (response.Content.Headers.ContentLength is > 8 * 1024 * 1024) throw new InvalidOperationException("Bruk Opne for dette store biletet.");
            using var input = await response.Content.ReadAsStreamAsync(linked.Token); using var result = new MemoryStream();
            byte[] buffer = new byte[32768]; int count;
            while ((count = await input.ReadAsync(buffer, linked.Token)) != 0)
            { if (!Current(scope)) throw new OperationCanceledException(); if (result.Length + count > 8 * 1024 * 1024) throw new IOException("Preview too large."); await result.WriteAsync(buffer.AsMemory(0, count), linked.Token); }
            if (!Current(scope)) throw new OperationCanceledException();
            var lease = new NativePreviewLease(result.ToArray(), () => _previews.Release(), ReleasePreviewSlot);
            handedOff = true; return lease;
        }
        finally { if (!handedOff) { if (pipeline) _previews.Release(); ReleasePreviewSlot(); } }
    }
    internal async Task<NativeMediaFile> OriginalAsync(string id, NativeMediaScope scope, IProgress<long>? progress, CancellationToken cancellation)
    {
        using var linked = Link(scope, cancellation); linked.CancelAfter(TimeSpan.FromMinutes(10));
        await _originals.WaitAsync(linked.Token);
        string file = System.IO.Path.Combine(_folder, Guid.NewGuid().ToString("N"));
        try
        {
            lock (_gate)
            {
                linked.Token.ThrowIfCancellationRequested();
                if (_disposed || _family?.Scope != scope) throw new OperationCanceledException();
                _family.Files.Add(file);
            }
            long used = 0;
            foreach (var existing in new DirectoryInfo(_folder).EnumerateFiles())
                if (existing.Exists) used += existing.Length;
            using var response = await _owner.ReadMediaAsync(id, false, scope, linked.Token); response.EnsureSuccessStatusCode();
            if (response.Content.Headers.ContentLength is long length && length > MaxFileBytes - used) throw new IOException("Ikkje plass i det mellombelse medielageret. Lukk førre vedlegg først.");
            string type = response.Content.Headers.ContentType?.MediaType ?? "application/octet-stream";
            if (!type.StartsWith("image/", StringComparison.Ordinal) && !type.StartsWith("video/", StringComparison.Ordinal) &&
                type is not ("application/pdf" or "application/zip" or "text/plain" or "text/csv"))
                throw new InvalidOperationException("Denne vedleggstypen er ikkje støtta.");
            using var input = await response.Content.ReadAsStreamAsync(linked.Token);
            await using (var output = new FileStream(file, FileMode.CreateNew, FileAccess.Write, FileShare.None, 65536, FileOptions.Asynchronous))
            {
                byte[] buffer = new byte[65536]; int count; long total = 0, reported = -1;
                while ((count = await input.ReadAsync(buffer, linked.Token)) != 0)
                {
                    if (!Current(scope)) throw new OperationCanceledException(); total += count;
                    if (total > MaxFileBytes - used) throw new IOException("Vedlegget er for stort for det mellombelse lageret.");
                    await output.WriteAsync(buffer.AsMemory(0, count), linked.Token);
                    if (total / (1024 * 1024) != reported) { reported = total / (1024 * 1024); progress?.Report(total); }
                }
            }
            if (!Current(scope)) throw new OperationCanceledException();
            return new(file, type, ReleaseFile);
        }
        catch { ReleaseFile(file); throw; }
        finally { _originals.Release(); }
    }
    public void Dispose()
    {
        Family? retired;
        lock (_gate)
        {
            if (_disposed) return;
            _disposed = true;
            retired = _family;
            _family = null;
        }
        _owner.Changed -= OwnerChanged;
        if (retired is not null) Retire(retired);
        else { try { Invalidated?.Invoke(); } catch { } }
        RemoveEmptyFolder();
    }
}
