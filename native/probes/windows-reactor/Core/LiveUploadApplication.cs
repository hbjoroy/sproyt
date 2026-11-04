namespace Sproyt.WindowsProbe;

internal sealed partial class LiveApplication
{
    private NativeUploadQueue? _uploads;
    private NativeUploadDiskStore? _uploadDisk;
    private readonly SemaphoreSlim _uploadAdmission = new(1, 1);
    private readonly object _uploadScopeGate = new();
    private sealed record UploadScopeRegistration(NativeUploadQueue? Queue, NativeMediaScope? Scope, CancellationTokenSource? Staging);
    private UploadScopeRegistration? _uploadScopeRegistration;
    private bool _uploadInitializationFailed;
    private sealed record UploadIssue(string UserId, string Family, string Message);
    private UploadIssue? _uploadIssue;

    private bool UploadAdmitted(NativeUploadTarget target) => _authentication.ProjectSnapshot(auth =>
        !_closing && auth.Phase == NativeAuthPhase.Active && auth.UserId == target.UserId && _account == target.UserId &&
        SendFamily(auth.SessionStamp) == target.FamilyStamp && _workspace?.State.Ready == true &&
        Chat.UserId == target.UserId && Chat.Channels.Any(channel => channel.Id == target.ChannelId));

    private void BindUploadScope()
    {
        CancellationTokenSource? cancel = null;
        var registration = _authentication.ProjectSnapshot(auth =>
        {
            var queue = _uploads;
            NativeMediaScope? scope = auth.UserId == _account && auth.UserId is string user && SendFamily(auth.SessionStamp) is string family &&
                auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline ? new(user, family) : null;
            bool staging = !_closing && queue is not null && scope is not null && auth.Phase == NativeAuthPhase.Active && _workspace?.State.Ready == true;
            lock (_uploadScopeGate)
            {
                var previous = _uploadScopeRegistration;
                if (previous is not null && ReferenceEquals(previous.Queue, queue) && previous.Scope == scope && (previous.Staging is not null) == staging)
                    return previous;
                cancel = previous?.Staging;
                var next = new UploadScopeRegistration(queue, scope, staging ? new() : null);
                Volatile.Write(ref _uploadScopeRegistration, next);
                return next;
            }
        });
        CancelUploadStaging(cancel);
        // The predicate is evaluated under the queue gate and reads only this
        // immutable registration. No auth lock is held while cancellation or
        // observer callbacks run, and a late A callback cannot overwrite B.
        registration.Queue?.BindScope(registration.Scope, () => ReferenceEquals(Volatile.Read(ref _uploadScopeRegistration), registration));
        registration.Queue?.RecheckAdmission();
    }
    private static void CancelUploadStaging(CancellationTokenSource? cancellation)
    {
        if (cancellation is null) return;
        try { cancellation.Cancel(); }
        catch (ObjectDisposedException) { }
        finally { cancellation.Dispose(); }
    }
    private CancellationTokenSource? CaptureStaging(NativeUploadTarget target, NativeUploadQueue queue, NativeUploadDiskStore disk) =>
        _authentication.ProjectSnapshot(_ =>
        {
            if (!ReferenceEquals(queue, _uploads) || !ReferenceEquals(disk, _uploadDisk) || !UploadAdmitted(target)) return null;
            lock (_uploadScopeGate)
            {
                var registration = _uploadScopeRegistration;
                if (registration?.Staging is not { } staging || !ReferenceEquals(registration.Queue, queue) || registration.Scope != target.Scope) return null;
                // Linking occurs while the scope lease is pinned. Retirement
                // replaces the registration before cancelling outside the locks.
                return CancellationTokenSource.CreateLinkedTokenSource(staging.Token, _lifetime.Token);
            }
        });

    private async Task InitializeUploadsAsync(string user, ChatWorkspace workspace)
    {
        if (_uploads is not null || _uploadInitializationFailed) return;
        NativeUploadDiskStore? disk = null;
        try
        {
            disk = new(_directory, user,
                (target, job, marker) => AttachCompletedUploadAsync(workspace, target, job, marker),
                workspace.ReconcileUploadReceiptsAsync);
            var queue = await NativeUploadQueue.OpenAsync(disk, new NativeUploadHttpClient(_authentication), UploadAdmitted);
            _uploadDisk = disk; _uploads = queue; queue.Changed += Notify;
            BindUploadScope();
        }
        catch
        {
            disk?.Dispose(); _uploadInitializationFailed = true;
            var auth = Authentication;
            if (auth.UserId == user && SendFamily(auth.SessionStamp) is string family)
                _uploadIssue = new(user, family, "Opplastingslageret kunne ikkje opnast. Prøv Oppdater samband.");
        }
        Notify();
    }

    private async Task<bool> AttachCompletedUploadAsync(ChatWorkspace workspace, NativeUploadTarget target, string jobId, string marker)
    {
        try
        {
            Task? accepted = _authentication.ProjectSnapshot(_ =>
            {
                if (!ReferenceEquals(workspace, _workspace) || !UploadAdmitted(target)) return null;
                string key = new ComposerTarget(target.ChannelId, target.ParentMessageId).Key;
                workspace.EnsureChannels([key]);
                return workspace.AttachUploadAsync(key, jobId, marker);
            });
            if (accepted is null) return false;
            await accepted;
            return true;
        }
        catch (NativeProbeException failure) when (failure.Code is "text_too_large" or "capacity" or "workspace_busy")
        {
            // These failures promise that no attachment committed. Keep a
            // retryable Ready job, allowing the user to free composer space.
            // Unknown outcomes continue through the receipt/journal path.
            UploadFailed(target, "Utkastet har ikkje plass. Forkort eller send det før du prøver vedlegget igjen.");
            return false;
        }
    }

    public NativeUploadTarget? CaptureUploadTarget(NativeMessageActionScope scope) => _authentication.ProjectSnapshot(_ =>
    {
        if (!CurrentActionScope(scope) || SendFamily(scope.SessionStamp) is not string family) return null;
        var target = new NativeUploadTarget(scope.UserId, family, scope.ChannelId, scope.ThreadRootId);
        return UploadAdmitted(target) ? target : null;
    });
    private void UploadFailed(NativeUploadTarget target, string message)
    {
        _authentication.ProjectSnapshot(auth =>
        {
            if (auth.UserId == target.UserId && SendFamily(auth.SessionStamp) == target.FamilyStamp)
                _uploadIssue = new(target.UserId, target.FamilyStamp, message);
            return true;
        });
        Notify();
    }
    private void ClearUploadIssue(NativeUploadTarget target) => _authentication.ProjectSnapshot(auth =>
    {
        if (auth.UserId == target.UserId && SendFamily(auth.SessionStamp) == target.FamilyStamp &&
            _uploadIssue?.UserId == target.UserId && _uploadIssue.Family == target.FamilyStamp) _uploadIssue = null;
        return true;
    });
    private static string UploadFailureText(Exception failure) => failure switch
    {
        NativeUploadRejectedException rejected => rejected.Message,
        FileNotFoundException or DirectoryNotFoundException => "Fila er flytta eller sletta. Vel henne på nytt.",
        UnauthorizedAccessException => "Vi har ikkje lesetilgang til fila. Vel ei anna fil.",
        _ when failure.Message is { Length: < 240 } text && !text.Contains(':') && !text.Contains('\\') && !text.Contains('/') => text,
        _ => "Fila kunne ikkje klargjerast. Kontroller lesetilgang og ledig diskplass. Maks lokal filstorleik er 512 MiB."
    };

    public async Task AddUploadFilesAsync(NativeUploadTarget target, IReadOnlyList<string> paths)
    {
        var queue = _uploads; var disk = _uploadDisk;
        if (queue is null || disk is null || paths.Count == 0 || !UploadAdmitted(target)) return;
        if (paths.Count > NativeUploadQueue.MaximumJobs) { UploadFailed(target, "Vel høgst åtte filer om gongen."); return; }
        using var staging = CaptureStaging(target, queue, disk);
        if (staging is null) return;
        bool admitted = false;
        try
        {
            await _uploadAdmission.WaitAsync(staging.Token); admitted = true;
            foreach (string path in paths)
            {
                if (!ReferenceEquals(queue, _uploads) || !UploadAdmitted(target)) return;
                NativeUploadSource? source = null;
                try
                {
                    source = await disk.StageFileAsync(path, staging.Token);
                    if (!ReferenceEquals(queue, _uploads) || !UploadAdmitted(target)) throw new OperationCanceledException();
                    await queue.EnqueueAsync(target, source, staging.Token);
                    ClearUploadIssue(target);
                }
                catch (OperationCanceledException) { return; }
                catch (Exception failure)
                {
                    UploadFailed(target, UploadFailureText(failure));
                    return;
                }
                finally
                {
                    // A journal failure may leave an admitted in-memory job.
                    // Its private source must survive for explicit storage retry,
                    // even when the UI scope has since signed out.
                    if (source is not null && !queue.RetainsSource(source.SourceId)) await disk.ReleaseStagedAsync(source.SourceId);
                }
            }
        }
        catch (OperationCanceledException) { }
        finally { if (admitted) _uploadAdmission.Release(); Notify(); }
    }

    public async Task AddUploadStreamAsync(NativeUploadTarget target, Stream stream, string fileName, string contentType)
    {
        var queue = _uploads; var disk = _uploadDisk;
        if (queue is null || disk is null || !UploadAdmitted(target)) return;
        using var staging = CaptureStaging(target, queue, disk);
        if (staging is null) return;
        bool admitted = false;
        NativeUploadSource? source = null;
        try
        {
            await _uploadAdmission.WaitAsync(staging.Token); admitted = true;
            if (!ReferenceEquals(queue, _uploads) || !UploadAdmitted(target)) return;
            source = await disk.StageAsync(stream, fileName, contentType, staging.Token);
            if (!ReferenceEquals(queue, _uploads) || !UploadAdmitted(target)) throw new OperationCanceledException();
            await queue.EnqueueAsync(target, source, staging.Token); ClearUploadIssue(target);
        }
        catch (OperationCanceledException) { }
        catch (Exception failure) { UploadFailed(target, UploadFailureText(failure)); }
        finally
        {
            if (source is not null && !queue.RetainsSource(source.SourceId)) await disk.ReleaseStagedAsync(source.SourceId);
            if (admitted) _uploadAdmission.Release(); Notify();
        }
    }

    private async Task ChangeUploadAsync(string user, string stamp, string id, Func<NativeUploadQueue, string, Task> change)
    {
        var auth = Authentication; var queue = _uploads;
        if (queue is null || auth.UserId != user || auth.SessionStamp != stamp || _account != user ||
            !queue.Capture().Jobs.Any(job => job.Id == id && job.Target.UserId == user)) return;
        try { await change(queue, id); if (SendFamily(stamp) is string family) ClearUploadIssue(new(user, family, "")); }
        catch (OperationCanceledException) { }
        catch (Exception failure)
        {
            if (SendFamily(stamp) is string family) UploadFailed(new(user, family, ""), UploadFailureText(failure));
        }
        Notify();
    }
    public Task RetryUploadAsync(string user, string stamp, string id) => ChangeUploadAsync(user, stamp, id, (queue, job) => queue.RetryAsync(job));
    public void RemoveDraftAttachment(NativeMessageActionScope scope, int index, string expectedMarker) =>
        WithComposer(scope, (workspace, key) => workspace.RemoveDraftAttachment(key, index, expectedMarker));
    public Task CancelUploadAsync(string user, string stamp, string id) => ChangeUploadAsync(user, stamp, id, (queue, job) => queue.CancelAsync(job));
    public Task RemoveUploadAsync(string user, string stamp, string id) => ChangeUploadAsync(user, stamp, id, async (queue, job) =>
    {
        var workspace = _workspace;
        // A previous login may have a claimed local handoff whose reply was
        // lost. Resolve only its existing receipt in the original account
        // before retiring the journal record; never admit a new attachment.
        if (workspace is not null && _account == user && queue.Capture().Jobs.Any(item => item.Id == job && item.HandoffStarted))
            await workspace.SettleUploadAsync(job);
        if (ReferenceEquals(queue, _uploads) && ReferenceEquals(workspace, _workspace) && Authentication.UserId == user)
            await queue.RemoveAsync(job);
    });

    private async Task CloseUploadsAsync()
    {
        CancellationTokenSource? cancel = null;
        var queue = _authentication.ProjectSnapshot(_ =>
        {
            var closing = Interlocked.Exchange(ref _uploads, null);
            lock (_uploadScopeGate)
            {
                cancel = _uploadScopeRegistration?.Staging;
                Volatile.Write(ref _uploadScopeRegistration, null);
            }
            return closing;
        });
        CancelUploadStaging(cancel);
        if (queue is not null) { queue.Changed -= Notify; await queue.DisposeAsync(); }
        // Waiting for staging avoids disposing a journal/source lease under its
        // final Enqueue call during an account switch or orderly app close.
        await _uploadAdmission.WaitAsync();
        try { _uploadDisk?.Dispose(); _uploadDisk = null; _uploadIssue = null; _uploadInitializationFailed = false; }
        finally { _uploadAdmission.Release(); }
    }
}
