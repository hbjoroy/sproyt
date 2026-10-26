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

internal sealed class NativeMediaFile(string path, string contentType) : IDisposable
{
    internal string Path { get; } = path;
    internal string ContentType { get; } = contentType;
    public void Dispose() { try { File.Delete(Path); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
}

// Previews are bounded in memory. Originals are fetched only after an explicit
// Open/Play and streamed to a private temporary file, never a whole-video buffer.
internal sealed class NativeMediaLibrary : IDisposable
{
    private const long MaxFileBytes = 512L * 1024 * 1024;
    private readonly NativeAuthSession _owner;
    private readonly string _folder;
    private readonly SemaphoreSlim _previews = new(2, 2), _previewSlots = new(8, 8), _originals = new(1, 1);
    private CancellationTokenSource _scopeCancellation = new();
    private NativeMediaScope? _scope;
    private readonly object _gate = new();
    private bool _disposed;
    internal event Action? Invalidated;
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
        if (!Current(scope)) throw new OperationCanceledException();
        lock (_gate)
        {
            if (_disposed) throw new OperationCanceledException();
            _scope ??= scope;
            if (_scope != scope) throw new OperationCanceledException();
            return CancellationTokenSource.CreateLinkedTokenSource(cancellation, _scopeCancellation.Token);
        }
    }
    private void OwnerChanged()
    {
        NativeMediaScope? captured;
        lock (_gate) captured = _scope;
        if (captured is null || Current(captured)) return;
        lock (_gate)
        {
            if (_scope != captured) return;
            _scopeCancellation.Cancel(); _scopeCancellation.Dispose(); _scopeCancellation = new(); _scope = null;
        }
        try { Invalidated?.Invoke(); } catch { }
        CleanFiles();
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
            var lease = new NativePreviewLease(result.ToArray(), () => _previews.Release(), () => _previewSlots.Release());
            handedOff = true; return lease;
        }
        finally { if (!handedOff) { if (pipeline) _previews.Release(); _previewSlots.Release(); } }
    }
    internal async Task<NativeMediaFile> OriginalAsync(string id, NativeMediaScope scope, IProgress<long>? progress, CancellationToken cancellation)
    {
        using var linked = Link(scope, cancellation); linked.CancelAfter(TimeSpan.FromMinutes(10));
        await _originals.WaitAsync(linked.Token);
        string file = System.IO.Path.Combine(_folder, Guid.NewGuid().ToString("N"));
        try
        {
            long used = Directory.GetFiles(_folder).Sum(path => new FileInfo(path).Length);
            using var response = await _owner.ReadMediaAsync(id, false, scope, linked.Token); response.EnsureSuccessStatusCode();
            if (response.Content.Headers.ContentLength is long length && length > MaxFileBytes - used) throw new IOException("Ikkje plass i det mellombelse medielageret. Lukk førre vedlegg først.");
            string type = response.Content.Headers.ContentType?.MediaType ?? "application/octet-stream";
            if (!type.StartsWith("image/", StringComparison.Ordinal) && !type.StartsWith("video/", StringComparison.Ordinal)) throw new InvalidOperationException("Denne vedleggstypen må opnast i nettappen.");
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
            return new(file, type);
        }
        catch { try { File.Delete(file); } catch (IOException) { } throw; }
        finally { _originals.Release(); }
    }
    private void CleanFiles() { foreach (string file in Directory.GetFiles(_folder)) try { File.Delete(file); } catch (IOException) { } catch (UnauthorizedAccessException) { } }
    public void Dispose()
    {
        lock (_gate) { if (_disposed) return; _disposed = true; _scopeCancellation.Cancel(); }
        _owner.Changed -= OwnerChanged; try { Invalidated?.Invoke(); } catch { } CleanFiles();
        try { Directory.Delete(_folder); } catch (IOException) { } catch (UnauthorizedAccessException) { }
    }
}
