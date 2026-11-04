using System.Collections.ObjectModel;

namespace Sproyt.WindowsProbe;

public interface IWorkspaceStore : IAsyncDisposable
{
    Task<StoreSnapshot> SnapshotAsync();
    Task<long> SaveDraftAsync(string channel, string body);
    Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId, long expectedDraftRevision);
}

public interface IWorkspaceUploadStore
{
    Task<long> SaveComposerAsync(string key, string body, DraftComposer composer);
    Task<UploadDraftResult> AttachUploadAsync(string composerKey, string jobId, string operationId, string body, string marker, DraftComposer composer);
    Task<UploadDraftResult?> ReadUploadReceiptAsync(string key, string jobId, string operationId, string marker);
    Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> activeJobIds);
}

public sealed class NativeWorkspaceStoreAdapter(NativeSession session) : IWorkspaceStore, IWorkspaceUploadStore
{
    public Task<StoreSnapshot> SnapshotAsync() => session.SnapshotAsync();
    public Task<long> SaveDraftAsync(string channel, string body)
    {
        var target = ComposerTarget.FromKey(channel);
        return session.SaveDraftAsync(target.ChannelId, body, target.ParentMessageId);
    }
    public Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId, long expectedDraftRevision)
    {
        var target = ComposerTarget.FromKey(channel);
        return session.EnqueueAsync(target.ChannelId, body, requestId, expectedDraftRevision, target.ParentMessageId);
    }
    public Task<long> SaveComposerAsync(string key, string body, DraftComposer composer)
    {
        var target = ComposerTarget.FromKey(key);
        return session.SaveDraftAsync(target.ChannelId, body, target.ParentMessageId, composer);
    }
    public Task<UploadDraftResult> AttachUploadAsync(string composerKey, string jobId, string operationId, string body, string marker, DraftComposer composer) =>
        session.AttachUploadAsync(ComposerTarget.FromKey(composerKey), jobId, operationId, body, marker, composer);
    public async Task<UploadDraftResult?> ReadUploadReceiptAsync(string key, string jobId, string operationId, string marker)
    {
        try { return await session.AttachUploadAsync(ComposerTarget.FromKey(key), jobId, operationId, "", marker, receiptOnly: true).ConfigureAwait(false); }
        catch (NativeProbeException error) when (error.Code == "receipt_not_found") { return null; }
    }
    public Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> activeJobIds) => session.ReconcileUploadReceiptsAsync(activeJobIds);
    public ValueTask DisposeAsync() => session.DisposeAsync();
}

public sealed record DraftView(string Body, long Revision, long SavedRevision)
{
    public string ComposerText { get; init; } = Body;
    public IReadOnlyList<string> Attachments { get; init; } = [];
}
public sealed record WorkspaceSnapshot(bool Ready, bool Closing, bool Sending, bool NeedsRetry, string? ErrorCode,
    string? ErrorMessage, IReadOnlyDictionary<string, DraftView> Drafts, IReadOnlyList<QueuedMessage> Outbox, bool Terminal = false);

// One pump owns all store operations. Edits only replace bounded channel state
// and set a coalesced signal; they never allocate or enqueue a persistence Task.
public sealed class ChatWorkspace
{
    private readonly object _gate = new();
    private readonly IWorkspaceStore _store;
    private readonly Dictionary<string, DraftState> _drafts;
    private readonly SemaphoreSlim _signal = new(0, 1);
    private readonly Task _pump;
    private List<QueuedMessage> _outbox = [];
    private TaskCompletionSource? _initialize;
    private bool _initializePending;
    private TaskCompletionSource? _shutdown;
    private Capture? _pendingSend;
    private Capture? _retry;
    private readonly Dictionary<string, UploadCapture> _uncertainUploads = new(StringComparer.Ordinal);
    private UploadWork? _upload;
    private TaskCompletionSource? _sendCompletion;
    private bool _ready, _closing, _sending, _disposed, _terminal;
    private long _lastRequestTicks;
    private string? _errorCode, _errorMessage;

    public ChatWorkspace(IWorkspaceStore store, IReadOnlyList<string> channels, int maximumChannels = 2)
    {
        ArgumentNullException.ThrowIfNull(store);
        ArgumentNullException.ThrowIfNull(channels);
        if (maximumChannels is < 1 or > 512 || channels.Count < 1 || channels.Count > maximumChannels || channels.Any(string.IsNullOrWhiteSpace) || channels.Distinct(StringComparer.Ordinal).Count() != channels.Count)
            throw new ArgumentException("The bounded pilot requires one or two distinct channels.", nameof(channels));
        _store = store;
        _drafts = channels.ToDictionary(channel => channel, _ => new DraftState(), StringComparer.Ordinal);
        _pump = PumpAsync();
    }

    // Notifications may come from a background thread; the UI dispatches them.
    public event Action? Changed;

    internal void EnsureChannels(IEnumerable<string> channels)
    {
        lock (_gate)
        {
            var requested = channels.ToHashSet(StringComparer.Ordinal);
            if (requested.Any(string.IsNullOrWhiteSpace)) throw new ArgumentException("Invalid channel.");
            if (!_sending && _retry is null && requested.Any(ComposerTarget.IsThreadKey))
            {
                // Visits to empty threads do not consume a slot forever. A
                // captured admission, unsaved edit or nonempty draft stays.
                foreach (var idle in _drafts.Where(pair => ComposerTarget.IsThreadKey(pair.Key) && !requested.Contains(pair.Key) &&
                    pair.Value.Body.Length == 0 && pair.Value.Revision == pair.Value.SavedRevision && !UploadHeld(pair.Key))
                    .Select(pair => pair.Key).ToArray())
                    _drafts.Remove(idle);
            }
            foreach (string channel in requested)
            {
                EnsureTarget(channel);
            }
        }
    }

    private void EnsureTarget(string key)
    {
        if (_drafts.ContainsKey(key)) return;
        // 512 directory entries, eight persisted targets and the active thread.
        if (_drafts.Count >= 512 + 8 + 1) throw new InvalidOperationException("Composer capacity reached.");
        _drafts.Add(key, new DraftState());
    }

    internal void ServerAcknowledged(string requestId)
    {
        lock (_gate) _outbox.RemoveAll(item => item.RequestId == requestId);
        Notify();
    }

    // The delivery owner calls this only after persisting the terminal state.
    // Do not replace the whole outbox with an older delivery snapshot.
    internal void ServerRejected(string requestId)
    {
        bool changed = false;
        lock (_gate)
        {
            int index = _outbox.FindIndex(item => item.RequestId == requestId);
            if (index >= 0 && _outbox[index].State != "rejected")
            {
                _outbox[index] = _outbox[index] with { State = "rejected" };
                changed = true;
            }
        }
        if (changed) Notify();
    }

    public WorkspaceSnapshot State
    {
        get
        {
            lock (_gate)
                return new(_ready, _closing, _sending, _retry is not null, _errorCode, _errorMessage,
                    new ReadOnlyDictionary<string, DraftView>(_drafts.ToDictionary(pair => pair.Key,
                        pair => new DraftView(pair.Value.Body, pair.Value.Revision, pair.Value.SavedRevision) { ComposerText = pair.Value.Composer.Text, Attachments = pair.Value.Composer.Attachments }, StringComparer.Ordinal)),
                    Array.AsReadOnly(_outbox.ToArray()), _terminal);
        }
    }

    public Task InitializeAsync()
    {
        lock (_gate)
        {
            if (_disposed || _closing) return Task.FromException(new ObjectDisposedException(nameof(ChatWorkspace)));
            if (_ready) return Task.CompletedTask;
            if (_initialize is not null) return _initialize.Task;
            _initialize = Completion();
            _initializePending = true;
            Signal();
            return _initialize.Task;
        }
    }

    public void EditDraft(string channel, string body) => EditComposer(channel, body);
    public void EditComposer(string key, string text) => EditComposer(key, text, null);
    public void EditComposer(string key, string text, IReadOnlyList<string>? keptAttachments)
    {
        ArgumentNullException.ThrowIfNull(text);
        lock (_gate)
        {
            EnsureEditable();
            var draft = GetDraft(key);
            var markers = draft.Composer.Attachments;
            if (keptAttachments is not null)
            {
                // Text edits may retain or remove an ordered subset. Only
                // committed upload handoffs introduce new attachment chips.
                int index = 0;
                foreach (var marker in keptAttachments)
                {
                    while (index < markers.Count && markers[index] != marker) index++;
                    if (index++ == markers.Count) throw new ArgumentException("Unknown draft attachment.", nameof(keptAttachments));
                }
                markers = Array.AsReadOnly(keptAttachments.ToArray());
            }
            ReplaceComposer(draft, new(text, markers));
        }
        Notify();
    }

    public void RemoveDraftAttachment(string key, int index, string expectedMarker)
    {
        lock (_gate)
        {
            EnsureEditable();
            var draft = GetDraft(key);
            var markers = draft.Composer.Attachments;
            if ((uint)index >= (uint)markers.Count || markers[index] != expectedMarker) return;
            ReplaceComposer(draft, new(draft.Composer.Text, Array.AsReadOnly(markers.Where((_, i) => i != index).ToArray())));
        }
        Notify();
    }

    private void ReplaceComposer(DraftState draft, DraftComposer composer)
    {
        draft.Composer = composer;
        draft.Revision = checked(draft.Revision + 1);
        draft.BlockedRevision = null;
        _errorCode = _errorMessage = null;
        Signal();
    }

    public Task SendAsync(string channel) => SendAsync(channel, null);
    public Task SendAsync(string channel, long? expectedComposerRevision)
    {
        Task task;
        lock (_gate)
        {
            EnsureEditable();
            if (_sending || _retry is not null) throw new InvalidOperationException("Finish or retry the captured send before admitting another.");
            if (UploadHeld(channel)) throw new NativeProbeException("upload_pending", "Finish attaching the captured upload before sending this draft.");
            var draft = GetDraft(channel);
            if (expectedComposerRevision is { } expected && draft.Revision != expected)
                throw new NativeProbeException("draft_changed", "The composer changed before send admission.");
            _lastRequestTicks = Math.Max(DateTime.UtcNow.Ticks, checked(_lastRequestTicks + 1));
            string requestId = FormattableString.Invariant($"windows-{_lastRequestTicks:D19}-{Guid.NewGuid():N}");
            task = AdmitSend(new(channel, draft.Revision, draft.Body, requestId, draft.Composer));
        }
        Notify();
        return task;
    }

    public Task RetryAsync()
    {
        Task task;
        lock (_gate)
        {
            EnsureEditable();
            if (_sending || _retry is null) throw new InvalidOperationException("There is no unknown send outcome to retry.");
            task = AdmitSend(_retry);
        }
        Notify();
        return task;
    }

    private bool UploadHeld(string key) => _upload?.Capture.Key == key || _uncertainUploads.ContainsKey(key);
    public Task AttachUploadAsync(string composerKey, string jobId, string marker)
    {
        lock (_gate)
        {
            EnsureEditable();
            if (_store is not IWorkspaceUploadStore) throw new NotSupportedException("This workspace does not support upload drafts.");
            if (_upload is { } running)
            {
                if (!running.ReceiptOnly && running.Capture.Key == composerKey && running.Capture.JobId == jobId && running.Capture.Marker == marker) return running.Completion!.Task;
                throw new NativeProbeException("workspace_busy", "Another attachment is in progress.");
            }
            if (_sending || _retry?.Channel == composerKey)
                throw new NativeProbeException("workspace_busy", "A send is using the captured composer.");
            // Attaching in the background must not evict the visible empty composer.
            EnsureTarget(composerKey);
            var draft = GetDraft(composerKey);
            if (!_uncertainUploads.TryGetValue(composerKey, out var capture))
                capture = new(composerKey, jobId, marker, Guid.NewGuid().ToString("N"), draft.Body, draft.Revision, draft.Composer);
            else if (capture.JobId != jobId || capture.Marker != marker)
                throw new NativeProbeException("upload_pending", "Reconcile the captured upload first.");
            var work = new UploadWork(capture, Completion());
            _upload = work; Signal();
            return work.Completion!.Task;
        }
    }

    // Explicit removal of a held login-family job must first settle any
    // local handoff from that same account. This never admits a new upload.
    public Task SettleUploadAsync(string jobId)
    {
        lock (_gate)
        {
            EnsureEditable();
            if (_upload is { } current && current.Capture.JobId == jobId)
                return current.ReceiptOnly ? current.Completion!.Task : SettleAfterAsync(current.Completion!.Task, jobId);
            var capture = _uncertainUploads.Values.FirstOrDefault(item => item.JobId == jobId);
            if (capture is null) return Task.CompletedTask;
            if (_upload is not null) throw new NativeProbeException("workspace_busy", "Another attachment is in progress.");
            _upload = new(capture, Completion(), ReceiptOnly: true);
            var task = _upload.Completion!.Task;
            Signal(); return task;
        }
    }
    private async Task SettleAfterAsync(Task pending, string jobId)
    {
        try { await pending.ConfigureAwait(false); }
        catch (NativeProbeException error) when (error.Code == "unknown_outcome") { }
        await SettleUploadAsync(jobId).ConfigureAwait(false);
    }

    public Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> activeJobIds)
    {
        lock (_gate)
        {
            EnsureEditable();
            if (_store is not IWorkspaceUploadStore store) throw new NotSupportedException("This workspace does not support upload drafts.");
            if (activeJobIds.Count > 8) throw new ArgumentOutOfRangeException(nameof(activeJobIds));
            // The journal owner supplies its entire committed account set. Pin local
            // in-flight receipts too. NativeSession queues this call on its sole worker.
            var keep = activeJobIds.ToHashSet(StringComparer.Ordinal);
            foreach (var held in _uncertainUploads.Values) keep.Add(held.JobId);
            if (_upload is { } current) keep.Add(current.Capture.JobId);
            return store.ReconcileUploadReceiptsAsync(keep.ToArray());
        }
    }

    public Task ShutdownAsync()
    {
        Task task;
        lock (_gate)
        {
            // Cleanup already ran in the terminal case. A second explicit
            // close acknowledges its reported failure without using dead state.
            if (_terminal) return Task.CompletedTask;
            if (_disposed) return _shutdown?.Task ?? Task.CompletedTask;
            if (_shutdown is not null) return _shutdown.Task;
            _closing = true;
            _shutdown = Completion();
            task = _shutdown.Task;
            Signal();
        }
        Notify();
        return task;
    }

    private Task AdmitSend(Capture capture)
    {
        _pendingSend = capture;
        _sendCompletion = Completion();
        _sending = true;
        _errorCode = _errorMessage = null;
        Signal();
        return _sendCompletion.Task;
    }

    private async Task PumpAsync()
    {
        while (true)
        {
            await _signal.WaitAsync().ConfigureAwait(false);
            while (true)
            {
                Capture? send = null;
                DraftCapture? save = null; UploadWork? upload = null;
                bool initialize = false, close = false;
                lock (_gate)
                {
                    if (_initializePending) { _initializePending = false; initialize = true; }
                    else if (_upload is not null) upload = _upload;
                    else if (_pendingSend is not null) { send = _pendingSend; _pendingSend = null; }
                    else if (_closing && _retry is not null)
                        FailShutdown(new NativeProbeException("unknown_outcome", "Retry the captured request before closing."));
                    else if (_closing && _uncertainUploads.Values.FirstOrDefault() is { } held)
                        upload = _upload = new(held, null, ReceiptOnly: true);
                    else
                    {
                        var dirty = _drafts.FirstOrDefault(pair => pair.Value.Revision != pair.Value.SavedRevision &&
                            !UploadHeld(pair.Key) && (_closing || pair.Value.BlockedRevision != pair.Value.Revision));
                        if (dirty.Value is not null) save = new(dirty.Key, dirty.Value.Revision, dirty.Value.Body, dirty.Value.Composer);
                        else close = _closing;
                    }
                }
                if (initialize) await InitializeStoreAsync().ConfigureAwait(false);
                else if (upload is not null) await AttachCapturedAsync(upload).ConfigureAwait(false);
                else if (send is not null) await SendCapturedAsync(send).ConfigureAwait(false);
                else if (save is not null) await SaveCapturedAsync(save).ConfigureAwait(false);
                else if (close)
                {
                    try
                    {
                        await _store.DisposeAsync().ConfigureAwait(false);
                        lock (_gate) { _disposed = true; _ready = false; _closing = false; _shutdown!.TrySetResult(); }
                    }
                    catch (Exception exception)
                    {
                        lock (_gate)
                        {
                            // NativeSession closes its worker and unloads the
                            // library even when cleanup reports failure. It is
                            // unsafe to unfreeze this store or retry disposal.
                            _terminal = _disposed = true;
                            _ready = _closing = false;
                            SetError(exception);
                            _shutdown!.TrySetException(exception);
                            _shutdown = null;
                        }
                    }
                    Notify();
                    lock (_gate) { if (_disposed) return; }
                    continue;
                }
                else { Notify(); break; }
                Notify();
            }
        }
    }

    private async Task InitializeStoreAsync()
    {
        try
        {
            var snapshot = await _store.SnapshotAsync().ConfigureAwait(false);
            lock (_gate)
            {
                foreach (var draft in snapshot.Drafts)
                {
                    if (!_drafts.TryGetValue(draft.Key, out var state))
                        _drafts.Add(draft.Key, state = new DraftState());
                    state.Composer = new(draft.ComposerText, draft.Attachments);
                }
                _outbox = snapshot.Outbox.ToList();
                foreach (var item in _outbox)
                    if (item.RequestId.Length == 60 && item.RequestId.StartsWith("windows-", StringComparison.Ordinal) &&
                        item.RequestId[27] == '-' && long.TryParse(item.RequestId.AsSpan(8, 19), out long ticks))
                        _lastRequestTicks = Math.Max(_lastRequestTicks, ticks);
                _ready = true;
                _errorCode = _errorMessage = null;
                _initialize!.TrySetResult();
            }
        }
        catch (Exception exception)
        {
            lock (_gate)
            {
                SetError(exception);
                _initialize!.TrySetException(exception);
                _initialize = null;
                if (_closing) FailShutdown(exception);
            }
        }
    }

    private async Task AttachCapturedAsync(UploadWork work)
    {
        var capture = work.Capture;
        try
        {
            var store = (IWorkspaceUploadStore)_store;
            var result = work.ReceiptOnly
                ? await store.ReadUploadReceiptAsync(capture.Key, capture.JobId, capture.OperationId, capture.Marker).ConfigureAwait(false)
                : await store.AttachUploadAsync(capture.Key, capture.JobId, capture.OperationId, capture.Body, capture.Marker, capture.Composer).ConfigureAwait(false);
            lock (_gate)
            {
                // A previous process may have attached and sent (or manually
                // removed) this marker already. Its receipt retires the job only.
                if (result is not null && result.OperationId == capture.OperationId && result.Body is not null)
                {
                    var draft = GetDraft(capture.Key);
                    bool unchanged = draft.Revision == capture.Revision;
                    draft.Composer = new(draft.Composer.Text, Array.AsReadOnly(draft.Composer.Attachments.Append(capture.Marker).ToArray()));
                    draft.Revision = checked(draft.Revision + 1);
                    if (unchanged) draft.SavedRevision = draft.Revision;
                    draft.BlockedRevision = null;
                }
                _uncertainUploads.Remove(capture.Key);
                _upload = null;
                _errorCode = _errorMessage = null;
                work.Completion?.TrySetResult();
            }
        }
        catch (Exception exception)
        {
            lock (_gate)
            {
                if (exception is NativeProbeException { Code: "unknown_outcome" }) _uncertainUploads[capture.Key] = capture;
                // Both checks follow the receipt lookup. A definitive budget
                // rejection proves that this job had not previously committed.
                else if (exception is NativeProbeException { Code: "text_too_large" or "capacity" or "receipt_capacity" })
                    _uncertainUploads.Remove(capture.Key);
                _upload = null;
                SetError(exception);
                work.Completion?.TrySetException(exception);
                if (work.ReceiptOnly && _closing) FailShutdown(exception);
            }
        }
    }

    private Task<long> SaveComposerAsync(string key, string body, DraftComposer composer) =>
        composer.Attachments.Count == 0 ? _store.SaveDraftAsync(key, body) :
        ((IWorkspaceUploadStore)_store).SaveComposerAsync(key, body, composer);

    private async Task SaveCapturedAsync(DraftCapture capture)
    {
        try
        {
            await SaveComposerAsync(capture.Channel, capture.Body, capture.Composer).ConfigureAwait(false);
            lock (_gate) GetDraft(capture.Channel).SavedRevision = capture.Revision;
        }
        catch (Exception exception)
        {
            lock (_gate)
            {
                GetDraft(capture.Channel).BlockedRevision = capture.Revision;
                SetError(exception);
                if (_closing) FailShutdown(exception);
            }
        }
    }

    private async Task SendCapturedAsync(Capture capture)
    {
        bool enqueueStarted = false;
        try
        {
            bool retry;
            lock (_gate) retry = _retry?.RequestId == capture.RequestId;
            // The initial attempt journals its capture first. A retry must not
            // overwrite newer durable draft text while reconciling an old ID.
            if (!retry)
            {
                long revision = await SaveComposerAsync(capture.Channel, capture.Body, capture.Composer).ConfigureAwait(false);
                capture = capture with { DurableRevision = revision };
                lock (_gate) GetDraft(capture.Channel).SavedRevision = capture.Revision;
            }
            long expectedRevision = capture.DurableRevision ?? throw new InvalidOperationException("A send must retain its durable draft revision.");
            enqueueStarted = true;
            var queued = await _store.EnqueueAsync(capture.Channel, capture.Body, capture.RequestId, expectedRevision).ConfigureAwait(false);
            lock (_gate)
            {
                _outbox.RemoveAll(item => item.RequestId == queued.RequestId);
                _outbox.Add(queued);
                var draft = GetDraft(capture.Channel);
                if (draft.Revision == capture.Revision)
                {
                    draft.Composer = new("", []);
                    draft.Revision = checked(draft.Revision + 1);
                    draft.BlockedRevision = null;
                    // Empty is dirty too: reconcile the visible draft even when
                    // native's existing-ID fast path made no draft change.
                }
                _retry = null;
                _sending = false;
                _errorCode = _errorMessage = null;
                _sendCompletion!.TrySetResult();
            }
        }
        catch (Exception exception)
        {
            lock (_gate)
            {
                _sending = false;
                // A definite failure does not reconcile an earlier unknown
                // enqueue. Only success clears that outstanding identity.
                if (enqueueStarted && exception is NativeProbeException { Code: "unknown_outcome" })
                {
                    _retry = capture;
                    // Retry retains the saved revision as well as ID/body.
                    // Native admission cannot clear a newer durable revision,
                    // even when the old transaction had not committed yet.
                }
                SetError(exception);
                _sendCompletion!.TrySetException(exception);
            }
        }
    }

    private void FailShutdown(Exception exception)
    {
        SetError(exception);
        _closing = false;
        _shutdown!.TrySetException(exception);
        _shutdown = null; // A corrected draft may start a new close attempt.
    }

    private void SetError(Exception exception)
    {
        _errorCode = exception is NativeProbeException native ? native.Code : "workspace_error";
        _errorMessage = exception.Message;
    }

    private void EnsureEditable()
    {
        if (_terminal) throw new InvalidOperationException("The workspace store is terminal after cleanup failed.");
        if (_closing || _disposed) throw new InvalidOperationException("The workspace is closing.");
        if (!_ready) throw new InvalidOperationException("The workspace has not loaded.");
    }

    private DraftState GetDraft(string channel) => _drafts.TryGetValue(channel, out var draft)
        ? draft : throw new ArgumentException("Unknown pilot channel.", nameof(channel));
    private static TaskCompletionSource Completion() => new(TaskCreationOptions.RunContinuationsAsynchronously);
    private void Signal() { try { _signal.Release(); } catch (SemaphoreFullException) { } }
    private void Notify()
    {
        foreach (var subscriber in Changed?.GetInvocationList() ?? [])
            try { ((Action)subscriber)(); } catch (Exception exception) { System.Diagnostics.Debug.WriteLine(exception); }
    }

    private sealed class DraftState
    {
        public DraftComposer Composer = new("", []);
        public string Body => NativeSession.JoinComposer(Composer.Text, Composer.Attachments);
        public long Revision, SavedRevision;
        public long? BlockedRevision;
    }
    private sealed record Capture(string Channel, long Revision, string Body, string RequestId, DraftComposer Composer, long? DurableRevision = null);
    private sealed record UploadCapture(string Key, string JobId, string Marker, string OperationId, string Body, long Revision, DraftComposer Composer);
    private sealed record UploadWork(UploadCapture Capture, TaskCompletionSource? Completion, bool ReceiptOnly = false);
    private sealed record DraftCapture(string Channel, long Revision, string Body, DraftComposer Composer);
}
