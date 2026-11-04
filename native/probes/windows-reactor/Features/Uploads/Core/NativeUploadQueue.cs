using System.Security.Cryptography;
using System.Text;

namespace Sproyt.WindowsProbe;

internal sealed class NativeUploadQueue : IAsyncDisposable
{
    internal const int MaximumJobs = 8;
    private sealed class JournalFailure : IOException { }
    private readonly INativeUploadLocalStore _local;
    private readonly INativeUploadTransport _transport;
    private readonly Func<NativeUploadTarget, bool> _admitted;
    private readonly TimeProvider _clock;
    private readonly object _gate = new();
    private readonly SemaphoreSlim _operations = new(1, 1), _signal = new(0, 1);
    private readonly CancellationTokenSource _lifetime = new();
    private readonly List<NativeUploadJob> _jobs;
    private readonly HashSet<string> _cancelRequested = [];
    private NativeMediaScope? _scope;
    private CancellationTokenSource? _activeCancellation;
    private string? _activeId, _storageError;
    private bool _disposed;
    private readonly Task _pump;
    internal event Action? Changed;

    private NativeUploadQueue(INativeUploadLocalStore local, INativeUploadTransport transport,
        Func<NativeUploadTarget, bool> admitted, TimeProvider clock, IReadOnlyList<NativeUploadJob> jobs)
    {
        _local = local; _transport = transport; _admitted = admitted; _clock = clock; _jobs = jobs.ToList();
        _pump = PumpAsync();
    }
    internal static async Task<NativeUploadQueue> OpenAsync(INativeUploadLocalStore local, INativeUploadTransport transport,
        Func<NativeUploadTarget, bool> admitted, TimeProvider? clock = null, CancellationToken cancellationToken = default)
    {
        var jobs = await local.LoadAsync(cancellationToken);
        if (jobs.Count > MaximumJobs || jobs.Select(job => job.Id).Distinct(StringComparer.Ordinal).Count() != jobs.Count ||
            jobs.Select(job => job.RequestId).Distinct(StringComparer.Ordinal).Count() != jobs.Count || jobs.Any(job => !ValidJob(job)))
            throw new InvalidOperationException("Den lokale opplastingsjournalen er ugyldig.");
        return new(local, transport, admitted, clock ?? TimeProvider.System, jobs);
    }
    internal void BindScope(NativeMediaScope? scope, Func<bool>? stillCurrent = null)
    {
        CancellationTokenSource? cancel;
        lock (_gate)
        {
            // Optional registration check is a short immutable-reference
            // comparison only: no auth lock, I/O, network or asynchronous work.
            if (_disposed || stillCurrent?.Invoke() == false || _scope == scope) return;
            _scope = scope; cancel = _activeCancellation;
        }
        try { cancel?.Cancel(); } catch (ObjectDisposedException) { }
        Notify(); Signal();
    }
    internal NativeUploadSnapshot Capture()
    {
        lock (_gate)
        {
            var visible = _scope is null ? [] : _jobs.Where(job => job.Target.UserId == _scope.UserId).ToArray();
            bool busy = _activeId is not null && visible.Any(job => job.Id == _activeId && job.Target.FamilyStamp == _scope?.FamilyStamp);
            return new(Array.AsReadOnly(visible), _storageError, busy, _scope?.FamilyStamp);
        }
    }
    // Used only by staging cleanup after an ambiguous Enqueue save result;
    // unlike the UI projection this remains valid after logout/account change.
    internal bool RetainsSource(string sourceId) { lock (_gate) return _jobs.Any(job => job.Source.SourceId == sourceId); }
    internal void RecheckAdmission() => Signal();
    internal async Task<string> EnqueueAsync(NativeUploadTarget target, NativeUploadSource source, CancellationToken cancellationToken = default)
    {
        if (!ValidTarget(target) || !ValidSource(source)) throw new ArgumentException("Ugyldig opplastingsfil eller mål.");
        if (!Allowed(target)) throw new OperationCanceledException("Innlogginga vart endra før opplastinga.");
        await _operations.WaitAsync(cancellationToken);
        string id = Guid.NewGuid().ToString("D");
        try
        {
            if (!Allowed(target)) throw new OperationCanceledException("Innlogginga vart endra før opplastinga.");
            lock (_gate)
            {
                if (_disposed || _scope != target.Scope) throw new OperationCanceledException("Innlogginga vart endra før opplastinga.");
                if (_storageError is not null) throw new InvalidOperationException("Opplastingsjournalen må lagrast før fleire filer kan leggjast til.");
                if (_jobs.Count >= MaximumJobs) throw new InvalidOperationException("Opplastingskøen er full. Fjern ei ferdig eller avbroten opplasting først.");
                _jobs.Add(new(id, target, source, id));
            }
            await PersistAsync(cancellationToken);
        }
        finally { _operations.Release(); Notify(); }
        Signal(); return id;
    }
    internal async Task RetryAsync(string id, CancellationToken cancellationToken = default)
    {
        await _operations.WaitAsync(cancellationToken);
        try
        {
            var job = Find(id);
            if (job is null || !Allowed(job.Target)) throw new OperationCanceledException("Opplastinga høyrer til ei anna innlogging. Fjern henne og vel fila på nytt.");
            if (job.State == NativeUploadState.Cancelled || !job.Retryable) return;
            Replace(job with { Error = null, State = job.State is NativeUploadState.Ready or NativeUploadState.Attached ? job.State :
                job.UploadId is null ? NativeUploadState.Queued : NativeUploadState.Uploading });
            await PersistAsync(cancellationToken);
        }
        finally { _operations.Release(); Notify(); }
        Signal();
    }
    internal async Task CancelAsync(string id, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        CancellationTokenSource? cancel; NativeUploadJob? job;
        lock (_gate)
        {
            job = _jobs.FirstOrDefault(item => item.Id == id);
            if (job is null || _disposed || _scope?.UserId != job.Target.UserId || job.State == NativeUploadState.Ready && job.HandoffStarted) return;
            _cancelRequested.Add(id); cancel = _activeId == id ? _activeCancellation : null;
        }
        try { cancel?.Cancel(); } catch (ObjectDisposedException) { }
        // Once the claim wins, finish its local journal transition even if
        // the UI caller cancels or the application begins closing.
        await _operations.WaitAsync(CancellationToken.None);
        try
        {
            job = Find(id); if (job is null) return;
            Replace(job with { State = NativeUploadState.Cancelled, Error = null });
            await PersistAsync(CancellationToken.None);
        }
        finally { _operations.Release(); Notify(); }
        if (job.UploadId is not null && Allowed(job.Target))
        {
            using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _lifetime.Token); timeout.CancelAfter(TimeSpan.FromSeconds(5));
            try { await _transport.AbortAsync(job, timeout.Token); } catch { /* Server TTL owns eventual abandoned-upload cleanup. */ }
        }
        Signal();
    }
    internal async Task RemoveAsync(string id, CancellationToken cancellationToken = default)
    {
        await _operations.WaitAsync(cancellationToken);
        try
        {
            var job = Find(id); if (job is null || !SameUser(job.Target)) return;
            bool held = Held(job.Target);
            if (!held && job.HandoffStarted && job.State != NativeUploadState.Attached)
                throw new InvalidOperationException("Kontroller det ferdige vedlegget før du fjernar opplastinga frå køen.");
            if (!held && job.State is not (NativeUploadState.Cancelled or NativeUploadState.Attached or NativeUploadState.Failed))
                throw new InvalidOperationException("Avbryt opplastinga før du fjernar henne.");
            lock (_gate) { _jobs.RemoveAll(item => item.Id == id); _cancelRequested.Remove(id); }
            try { await PersistAsync(cancellationToken); }
            catch { lock (_gate) { if (_jobs.All(item => item.Id != id)) _jobs.Add(job); } throw; }
        }
        finally { _operations.Release(); Notify(); }
        Signal();
    }

    private async Task<NativeUploadJob?> SelectReadyAsync(CancellationTokenSource cancellation)
    {
        // A scope or membership notification can wake the pump during Enqueue.
        // Wait for the journal writer before observing its in-memory mutations.
        await _operations.WaitAsync(cancellation.Token).ConfigureAwait(false);
        try
        {
            NativeUploadJob[] candidates;
            lock (_gate)
                candidates = _storageError is null && _scope is not null
                    ? _jobs.Where(item => item.Target.Scope == _scope && !_cancelRequested.Contains(item.Id) &&
                        item.Error is null && item.State is NativeUploadState.Queued or NativeUploadState.Uploading or
                            NativeUploadState.Finalizing or NativeUploadState.Ready or NativeUploadState.Attached).ToArray() : [];
            var job = candidates.FirstOrDefault(candidate => Allowed(candidate.Target));
            lock (_gate)
            {
                if (job is null || _disposed || _storageError is not null || _scope != job.Target.Scope ||
                    _cancelRequested.Contains(job.Id)) return null;
                _activeId = job.Id;
                _activeCancellation = cancellation;
                return job;
            }
        }
        finally { _operations.Release(); }
    }

    private async Task PumpAsync()
    {
        try
        {
            while (true)
            {
                await _signal.WaitAsync(_lifetime.Token).ConfigureAwait(false);
                while (!_lifetime.IsCancellationRequested)
                {
                    using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
                    var job = await SelectReadyAsync(cancellation).ConfigureAwait(false);
                    if (job is null) break;
                    Notify();
                    try { await ProcessAsync(job, cancellation.Token); }
                    catch (JournalFailure) { }
                    catch (OperationCanceledException) when (cancellation.IsCancellationRequested || !Allowed(job.Target) || CancelledOrRemoved(job.Id)) { }
                    catch (Exception error)
                    {
                        try
                        {
                            await UpdateAsync(job.Id, item => item with { State = NativeUploadState.Failed,
                                Error = BoundError(error is NativeUploadRejectedException ? error.Message : "Fila kunne ikkje lastast opp. Kontroller sambandet og prøv igjen."),
                                Retryable = error is not NativeUploadRejectedException rejected || rejected.Retryable }, _lifetime.Token);
                        }
                        catch (JournalFailure) { }
                        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested || !Allowed(job.Target) || CancelledOrRemoved(job.Id)) { }
                    }
                    finally
                    {
                        lock (_gate) { if (_activeId == job.Id) { _activeId = null; _activeCancellation = null; } }
                        Notify();
                    }
                }
            }
        }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
    }
    private async Task ProcessAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        Check(job, cancellationToken);
        if (job.State == NativeUploadState.Attached) { await RemoveAttachedAsync(job, cancellationToken); return; }
        if (job.State != NativeUploadState.Ready)
        {
            if (job.UploadId is null)
            {
                var config = await RetryOperationAsync(job, token => _transport.ConfigurationAsync(job.Target, token), cancellationToken);
                if (!config.Enabled || config.AllowedChannelIds is not null && !config.AllowedChannelIds.Contains(job.Target.ChannelId))
                    throw new NativeUploadRejectedException("Opplasting er ikkje aktivert for denne samtalen.", true);
                if (config.ChunkBytes != NativeUploadHttpClient.ChunkBytes || job.Source.SizeBytes > config.MaximumFileBytes)
                    throw new NativeUploadRejectedException("Fila er større enn opplastingsgrensa.");
                var status = await RetryOperationAsync(job, token => _transport.CreateAsync(job, token), cancellationToken);
                job = await ApplyStatusAsync(job, status, cancellationToken);
            }
            else
            {
                var status = await RetryOperationAsync(job, token => _transport.StatusAsync(job, token), cancellationToken);
                job = await ApplyStatusAsync(job, status, cancellationToken);
            }
            if (job.UploadedBytes < job.Source.SizeBytes)
            {
                Stream opened;
                try { opened = await _local.OpenReadAsync(job.Source, cancellationToken); }
                catch (Exception error) when (error is FileNotFoundException or DirectoryNotFoundException)
                { throw new NativeUploadRejectedException("Den lokale opplastingsfila manglar. Fjern opplastinga og vel fila på nytt."); }
                await using var source = opened;
                if (!source.CanRead || !source.CanSeek || source.Length != job.Source.SizeBytes)
                    throw new NativeUploadRejectedException("Den lokale opplastingsfila manglar eller er endra. Vel fila på nytt.");
                source.Position = job.UploadedBytes;
                byte[] buffer = new byte[NativeUploadHttpClient.ChunkBytes];
                while (job.UploadedBytes < job.Source.SizeBytes)
                {
                    Check(job, cancellationToken);
                    int count = (int)Math.Min(buffer.Length, job.Source.SizeBytes - job.UploadedBytes);
                    await source.ReadExactlyAsync(buffer.AsMemory(0, count), cancellationToken);
                    string hash = Convert.ToHexStringLower(SHA256.HashData(buffer.AsSpan(0, count)));
                    var sending = job;
                    var status = await RetryOperationAsync(sending, token => _transport.PutChunkAsync(sending, buffer, count, hash, token), cancellationToken);
                    if (status.ReceivedBytes != job.UploadedBytes + count || status.NextChunk != job.NextChunk + 1)
                        throw new IOException("Tenesta stadfesta feil opplastingsblokk.");
                    job = await ApplyStatusAsync(job, status, cancellationToken);
                }
            }
            job = await UpdateAsync(job.Id, item => item with { State = NativeUploadState.Finalizing }, cancellationToken);
            var completing = job;
            var attachment = await RetryOperationAsync(completing, token => _transport.CompleteAsync(completing, token), cancellationToken);
            if (attachment.Sha256 != job.Source.Sha256 || attachment.SizeBytes != job.Source.SizeBytes || !NativeUploadHttpClient.CanonicalId(attachment.MediaId))
                throw new IOException("Det fullførte vedlegget stemmer ikkje med den valde fila.");
            job = await UpdateAsync(job.Id, item => item with { State = NativeUploadState.Ready, Attachment = attachment }, cancellationToken);
        }
        Check(job, cancellationToken);
        // Journal the handoff's admission before the host's atomic draft merge.
        // Cancel either claims the job first or observes this non-cancellable
        // boundary; an unknown committed merge must be reconciled by job ID.
        if (!job.HandoffStarted)
            job = await UpdateAsync(job.Id, item => item with { HandoffStarted = true }, cancellationToken);
        bool attached;
        try { attached = await _local.AttachAsync(job.Target, job.Attachment!, job.Id, cancellationToken); }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested || !Allowed(job.Target)) { throw; }
        catch { StorageFailed(); throw new JournalFailure(); }
        if (!attached)
        {
            await UpdateAsync(job.Id, item => item with { State = NativeUploadState.Ready,
                HandoffStarted = false, Error = "Vedlegget er klart, men kunne ikkje leggjast til dette utkastet. Kontroller plass og tilgang, og prøv igjen." }, cancellationToken);
            return;
        }
        job = await UpdateAsync(job.Id, item => item with { State = NativeUploadState.Attached }, cancellationToken);
        await RemoveAttachedAsync(job, cancellationToken);
    }
    private async Task<NativeUploadJob> ApplyStatusAsync(NativeUploadJob job, NativeUploadStatus status, CancellationToken cancellationToken)
    {
        if (!NativeUploadHttpClient.CanonicalId(status.Id) || status.ChannelId != job.Target.ChannelId || status.FileName != job.Source.FileName ||
            status.ContentType != job.Source.ContentType || status.SizeBytes != job.Source.SizeBytes || status.ChunkBytes != NativeUploadHttpClient.ChunkBytes ||
            status.ReceivedBytes < job.UploadedBytes || status.ReceivedBytes > job.Source.SizeBytes || status.NextChunk < job.NextChunk ||
            status.NextChunk != (status.ReceivedBytes + NativeUploadHttpClient.ChunkBytes - 1) / NativeUploadHttpClient.ChunkBytes ||
            (status.ReceivedBytes != status.SizeBytes && status.ReceivedBytes % NativeUploadHttpClient.ChunkBytes != 0) ||
            status.State is not ("open" or "sealed" or "complete") ||
            (job.UploadId is not null && status.Id != job.UploadId)) throw new IOException("Opplastingsstatusen stemmer ikkje med den valde fila.");
        return await UpdateAsync(job.Id, item => item with { UploadId = status.Id, NextChunk = status.NextChunk, UploadedBytes = status.ReceivedBytes,
            State = status.ReceivedBytes < job.Source.SizeBytes ? NativeUploadState.Uploading : NativeUploadState.Finalizing }, cancellationToken);
    }
    private async Task<T> RetryOperationAsync<T>(NativeUploadJob job, Func<CancellationToken, Task<T>> operation, CancellationToken cancellationToken)
    {
        for (int attempt = 0; ; attempt++)
        {
            Check(job, cancellationToken);
            try
            {
                var result = await operation(cancellationToken); Check(job, cancellationToken); return result;
            }
            catch (NativeUploadRejectedException) { throw; }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested || !Allowed(job.Target)) { throw; }
            catch when (attempt < 4)
            {
                Check(job, cancellationToken);
                await Task.Delay(TimeSpan.FromMilliseconds(500 * (1 << attempt)), _clock, cancellationToken);
            }
        }
    }
    private async Task<NativeUploadJob> UpdateAsync(string id, Func<NativeUploadJob, NativeUploadJob> update, CancellationToken cancellationToken)
    {
        await _operations.WaitAsync(cancellationToken);
        try
        {
            var current = Find(id) ?? throw new OperationCanceledException(); Check(current, cancellationToken);
            NativeUploadJob next;
            lock (_gate)
            {
                int index = _jobs.FindIndex(item => item.Id == id);
                if (index < 0 || _disposed || _scope != current.Target.Scope || _cancelRequested.Contains(id) ||
                    _jobs[index].State == NativeUploadState.Cancelled) throw new OperationCanceledException();
                // This gate is also Cancel's claim gate. In particular there
                // is no check/replace gap when HandoffStarted becomes true.
                next = update(_jobs[index]); _jobs[index] = next;
            }
            await PersistAsync(cancellationToken); return next;
        }
        finally { _operations.Release(); Notify(); }
    }
    private async Task RemoveAttachedAsync(NativeUploadJob job, CancellationToken cancellationToken)
    {
        await _operations.WaitAsync(cancellationToken);
        try
        {
            Check(job, cancellationToken);
            lock (_gate) _jobs.RemoveAll(item => item.Id == job.Id && item.State == NativeUploadState.Attached);
            try { await PersistAsync(cancellationToken); }
            catch { lock (_gate) { if (_jobs.All(item => item.Id != job.Id)) _jobs.Add(job); } throw; }
        }
        finally { _operations.Release(); Notify(); }
    }
    private async Task PersistAsync(CancellationToken cancellationToken)
    {
        NativeUploadJob[] jobs; lock (_gate) jobs = _jobs.ToArray();
        try { await _local.SaveAsync(Array.AsReadOnly(jobs), cancellationToken); lock (_gate) _storageError = null; }
        catch { StorageFailed(); throw new JournalFailure(); }
    }
    private void StorageFailed() { lock (_gate) _storageError = "Opplastingsstatusen kunne ikkje lagrast. Kontroller lagringa og prøv igjen."; Notify(); }
    private void Check(NativeUploadJob job, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        lock (_gate) { if (_storageError is not null) throw new JournalFailure(); }
        var current = Find(job.Id);
        if (current is null || CancelledOrRemoved(job.Id) || !Allowed(job.Target)) throw new OperationCanceledException();
    }
    private bool Allowed(NativeUploadTarget target)
    {
        bool allowed; lock (_gate) allowed = !_disposed && _scope == target.Scope;
        return allowed && _admitted(target);
    }
    private bool SameUser(NativeUploadTarget target) { lock (_gate) return !_disposed && _scope?.UserId == target.UserId; }
    private bool Held(NativeUploadTarget target) { lock (_gate) return _scope?.UserId == target.UserId && _scope.FamilyStamp != target.FamilyStamp; }
    private bool CancelledOrRemoved(string id) { lock (_gate) return _cancelRequested.Contains(id) || _jobs.FirstOrDefault(job => job.Id == id) is null or { State: NativeUploadState.Cancelled }; }
    private NativeUploadJob? Find(string id) { lock (_gate) return _jobs.FirstOrDefault(item => item.Id == id); }
    private void Replace(NativeUploadJob job) { lock (_gate) { int index = _jobs.FindIndex(item => item.Id == job.Id); if (index < 0) throw new OperationCanceledException(); _jobs[index] = job; } }
    private void Signal() { try { _signal.Release(); } catch (SemaphoreFullException) { } }
    private void Notify() { try { Changed?.Invoke(); } catch { } }
    private static bool ValidTarget(NativeUploadTarget target) => NativeUploadHttpClient.CanonicalId(target.UserId) && NativeUploadHttpClient.CanonicalId(target.ChannelId) &&
        (target.ParentMessageId is null || NativeUploadHttpClient.CanonicalId(target.ParentMessageId)) && target.FamilyStamp.Length is > 0 and <= 128 &&
        target.FamilyStamp.All(character => char.IsAsciiLetterOrDigit(character) || character is ':' or '-' or '_');
    private static bool ValidSource(NativeUploadSource source) => NativeUploadHttpClient.CanonicalId(source.SourceId) && source.SizeBytes is > 0 and <= NativeUploadHttpClient.MaximumFileBytes &&
        source.FileName.Length is > 0 and <= 200 && !source.FileName.Any(char.IsControl) && Encoding.UTF8.GetByteCount(source.FileName) <= 800 &&
        NativeUploadHttpClient.AllowedContentType(source.ContentType) && NativeUploadHttpClient.Digest(source.Sha256);
    private static bool ValidJob(NativeUploadJob job) => NativeUploadHttpClient.CanonicalId(job.Id) && NativeUploadHttpClient.CanonicalId(job.RequestId) &&
        ValidTarget(job.Target) && ValidSource(job.Source) && (job.UploadId is null || NativeUploadHttpClient.CanonicalId(job.UploadId)) &&
        job.NextChunk >= 0 && job.UploadedBytes >= 0 && job.UploadedBytes <= job.Source.SizeBytes && Enum.IsDefined(job.State) &&
        job.NextChunk == (job.UploadedBytes + NativeUploadHttpClient.ChunkBytes - 1) / NativeUploadHttpClient.ChunkBytes &&
        (job.UploadedBytes == job.Source.SizeBytes || job.UploadedBytes % NativeUploadHttpClient.ChunkBytes == 0) &&
        (job.Error is null || job.Error.EnumerateRunes().Count() <= 128 && !job.Error.Any(char.IsControl)) &&
        (job.State is not (NativeUploadState.Ready or NativeUploadState.Attached) || job.Attachment is { } attachment &&
            NativeUploadHttpClient.CanonicalId(attachment.MediaId) && attachment.Sha256 == job.Source.Sha256 && attachment.SizeBytes == job.Source.SizeBytes &&
            attachment.FileName == job.Source.FileName && NativeUploadHttpClient.AllowedContentType(attachment.ContentType));
    private static string BoundError(string error) => string.Concat(error.EnumerateRunes().Take(128).Select(rune => System.Text.Rune.IsControl(rune) ? " " : rune.ToString()));
    public async ValueTask DisposeAsync()
    {
        CancellationTokenSource? cancel;
        lock (_gate) { if (_disposed) return; _disposed = true; cancel = _activeCancellation; _scope = null; }
        _lifetime.Cancel(); try { cancel?.Cancel(); } catch (ObjectDisposedException) { }
        await _pump;
        await _operations.WaitAsync(CancellationToken.None); _operations.Release();
        _lifetime.Dispose();
    }
}
