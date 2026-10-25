using System.Collections.ObjectModel;

namespace Sproyt.WindowsProbe;

public interface IWorkspaceStore : IAsyncDisposable
{
    Task<StoreSnapshot> SnapshotAsync();
    Task SaveDraftAsync(string channel, string body);
    Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId);
}

public sealed class NativeWorkspaceStoreAdapter(NativeSession session) : IWorkspaceStore
{
    public Task<StoreSnapshot> SnapshotAsync() => session.SnapshotAsync();
    public Task SaveDraftAsync(string channel, string body) => session.SaveDraftAsync(channel, body);
    public Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId) => session.EnqueueAsync(channel, body, requestId);
    public ValueTask DisposeAsync() => session.DisposeAsync();
}

public sealed record DraftView(string Body, long Revision, long SavedRevision);
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
            foreach (string channel in channels)
            {
                if (string.IsNullOrWhiteSpace(channel)) throw new ArgumentException("Invalid channel.");
                if (_drafts.ContainsKey(channel)) continue;
                if (_drafts.Count == 512) throw new InvalidOperationException("Channel capacity reached.");
                _drafts.Add(channel, new DraftState());
            }
        }
    }

    internal void ServerAcknowledged(string requestId)
    {
        lock (_gate) _outbox.RemoveAll(item => item.RequestId == requestId);
        Notify();
    }

    public WorkspaceSnapshot State
    {
        get
        {
            lock (_gate)
                return new(_ready, _closing, _sending, _retry is not null, _errorCode, _errorMessage,
                    new ReadOnlyDictionary<string, DraftView>(_drafts.ToDictionary(pair => pair.Key,
                        pair => new DraftView(pair.Value.Body, pair.Value.Revision, pair.Value.SavedRevision), StringComparer.Ordinal)),
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

    public void EditDraft(string channel, string body)
    {
        ArgumentNullException.ThrowIfNull(body);
        lock (_gate)
        {
            EnsureEditable();
            var draft = GetDraft(channel);
            draft.Body = body;
            draft.Revision = checked(draft.Revision + 1);
            draft.BlockedRevision = null;
            _errorCode = _errorMessage = null;
            Signal();
        }
        Notify();
    }

    public Task SendAsync(string channel)
    {
        Task task;
        lock (_gate)
        {
            EnsureEditable();
            if (_sending || _retry is not null) throw new InvalidOperationException("Finish or retry the captured send before admitting another.");
            var draft = GetDraft(channel);
            _lastRequestTicks = Math.Max(DateTime.UtcNow.Ticks, checked(_lastRequestTicks + 1));
            string requestId = FormattableString.Invariant($"windows-{_lastRequestTicks:D19}-{Guid.NewGuid():N}");
            task = AdmitSend(new(channel, draft.Revision, draft.Body, requestId));
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
                DraftCapture? save = null;
                bool initialize = false, close = false;
                lock (_gate)
                {
                    if (_initializePending) { _initializePending = false; initialize = true; }
                    else if (_pendingSend is not null) { send = _pendingSend; _pendingSend = null; }
                    else if (_closing && _retry is not null)
                        FailShutdown(new NativeProbeException("unknown_outcome", "Retry the captured request before closing."));
                    else
                    {
                        var dirty = _drafts.FirstOrDefault(pair => pair.Value.Revision != pair.Value.SavedRevision &&
                            (_closing || pair.Value.BlockedRevision != pair.Value.Revision));
                        if (dirty.Value is not null) save = new(dirty.Key, dirty.Value.Revision, dirty.Value.Body);
                        else close = _closing;
                    }
                }
                if (initialize) await InitializeStoreAsync().ConfigureAwait(false);
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
                    if (_drafts.TryGetValue(draft.Channel, out var state)) state.Body = draft.Body;
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

    private async Task SaveCapturedAsync(DraftCapture capture)
    {
        try
        {
            await _store.SaveDraftAsync(capture.Channel, capture.Body).ConfigureAwait(false);
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
        try
        {
            bool retry;
            lock (_gate) retry = _retry?.RequestId == capture.RequestId;
            // The initial attempt journals its capture first. A retry must not
            // overwrite newer durable draft text while reconciling an old ID.
            if (!retry)
            {
                await _store.SaveDraftAsync(capture.Channel, capture.Body).ConfigureAwait(false);
                lock (_gate) GetDraft(capture.Channel).SavedRevision = capture.Revision;
            }
            var queued = await _store.EnqueueAsync(capture.Channel, capture.Body, capture.RequestId).ConfigureAwait(false);
            lock (_gate)
            {
                _outbox.RemoveAll(item => item.RequestId == queued.RequestId);
                _outbox.Add(queued);
                var draft = GetDraft(capture.Channel);
                if (draft.Revision == capture.Revision)
                {
                    draft.Body = "";
                    draft.Revision = checked(draft.Revision + 1);
                    draft.BlockedRevision = null;
                    // Empty is dirty too: reconcile the visible draft even when
                    // native's existing-ID fast path made no draft change.
                }
                else
                {
                    MarkNewerDraftDirty(capture);
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
                if (exception is NativeProbeException { Code: "unknown_outcome" })
                {
                    _retry = capture;
                    // An enqueue may have committed and cleared matching text
                    // before its result was lost. Repair newer visible text on
                    // this same pump while retaining the unresolved request ID.
                    MarkNewerDraftDirty(capture);
                }
                SetError(exception);
                _sendCompletion!.TrySetException(exception);
            }
        }
    }

    private void MarkNewerDraftDirty(Capture capture)
    {
        var draft = GetDraft(capture.Channel);
        if (draft.Revision == capture.Revision) return;
        // Native knows bodies, not managed revisions. A first actual retry can
        // clear a newer equal-body draft on either success or an unknown result.
        // Re-save it after the call settles. This clear/re-save gap is not atomic
        // across hard process termination; this source probe is not a shipping SDK.
        draft.SavedRevision = Math.Min(draft.SavedRevision, capture.Revision);
        draft.BlockedRevision = null;
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
        public string Body = "";
        public long Revision, SavedRevision;
        public long? BlockedRevision;
    }
    private sealed record Capture(string Channel, long Revision, string Body, string RequestId);
    private sealed record DraftCapture(string Channel, long Revision, string Body);
}
