namespace Sproyt.WindowsProbe;

public static class WorkspaceProbe
{
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(10);
    private static readonly string[] Channels = ["probe-general", "probe-team"];

    public static async Task<string> RunAsync(string database, string libraryPath)
    {
        string path = Path.GetFullPath(database);
        Check(!File.Exists(path) && !File.Exists(path + ".closing") && !File.Exists(path + ".terminal") &&
            !File.Exists(path + ".beforecommit"), "Workspace probe refuses existing test databases.");
        var forwarding = new ForwardingStore(await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false));
        var workspace = new ChatWorkspace(forwarding, Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline).ConfigureAwait(false);
        workspace.EditDraft(Channels[0], "Same body after away and back");
        workspace.EditDraft(Channels[1], "Independent channel draft");
        await SavedAsync(workspace).ConfigureAwait(false);

        // A body comparison would wrongly clear the newer away/back revision.
        var firstGate = new AsyncGate();
        forwarding.EnqueueGate = firstGate;
        var firstSend = workspace.SendAsync(Channels[0]);
        await firstGate.Entered.WaitAsync(Deadline).ConfigureAwait(false);
        int savesBeforeTyping = forwarding.SaveCount;
        long capturedRevision = workspace.State.Drafts[Channels[0]].Revision;
        workspace.EditDraft(Channels[0], "Away");
        workspace.EditDraft(Channels[0], "Same body after away and back");
        for (int index = 0; index < 100; index++) workspace.EditDraft(Channels[1], $"Coalesced edit {index}");
        workspace.EditDraft(Channels[1], "Independent channel draft");
        Check(forwarding.SaveCount == savesBeforeTyping, "Typing bypassed the single coordinator during an active send.");
        firstGate.Release();
        await firstSend.WaitAsync(Deadline).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        var afterTyping = workspace.State;
        Check(afterTyping.Drafts[Channels[0]].Revision == capturedRevision + 2 &&
            afterTyping.Drafts[Channels[0]].Body == "Same body after away and back", "Send cleared a newer away/back revision.");
        Check(afterTyping.Drafts[Channels[1]].Body == "Independent channel draft", "Sending one channel lost the other draft.");
        Check(forwarding.SaveCount == savesBeforeTyping + 2, "Rapid edits were not coalesced to one latest save per channel.");

        // The real enqueue commits, but its result is lost once at the adapter.
        // Retry must retain the ID, avoid an optimistic row, and never rewrite
        // captured A over newer B that has already reached durable storage.
        forwarding.UnknownAfterCommit = 1;
        await ExpectAsync("unknown_outcome", () => workspace.SendAsync(Channels[0])).ConfigureAwait(false);
        Check(workspace.State.NeedsRetry && workspace.State.Outbox.Count == 1 &&
            workspace.State.Drafts[Channels[0]].Body != "", "An unknown send outcome discarded its capture or fabricated a queued row.");
        var attempted = forwarding.RequestIds;
        string unknownId = attempted[^1];
        const string newerBody = "Newer B must remain durable throughout retry";
        workspace.EditDraft(Channels[0], newerBody);
        await SavedAsync(workspace).ConfigureAwait(false);
        forwarding.FailNextEnqueue = 1;
        await ExpectAsync("storage_error", workspace.RetryAsync).ConfigureAwait(false);
        Check(workspace.State.NeedsRetry && workspace.State.Outbox.Count == 1 &&
            forwarding.RequestIds.Length == attempted.Length + 1 && forwarding.RequestIds[^1] == unknownId,
            "A definite retry enqueue failure discarded the unresolved capture or changed its ID.");
        try
        {
            await workspace.SendAsync(Channels[0]).WaitAsync(Deadline).ConfigureAwait(false);
            throw new InvalidOperationException("An unresolved capture allowed a new request ID.");
        }
        catch (InvalidOperationException exception) when (exception.Message == "Finish or retry the captured send before admitting another.") { }
        await ExpectAsync("unknown_outcome", workspace.ShutdownAsync).ConfigureAwait(false);
        Check(workspace.State.NeedsRetry && workspace.State.Ready && !workspace.State.Closing && forwarding.DisposeCount == 0,
            "A failed retry enabled closing over an unresolved enqueue.");
        var retryGate = new AsyncGate();
        forwarding.EnqueueGate = retryGate;
        int savesBeforeRetry = forwarding.SaveCount;
        var retrying = workspace.RetryAsync();
        await retryGate.Entered.WaitAsync(Deadline).ConfigureAwait(false);
        var duringRetry = await forwarding.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(forwarding.SaveCount == savesBeforeRetry && duringRetry.Drafts.Single(item => item.Channel == Channels[0]).Body == newerBody,
            "Retry overwrote newer durable B with its older A capture before enqueue reconciliation.");
        retryGate.Release();
        await retrying.WaitAsync(Deadline).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        Check(forwarding.RequestIds[^1] == unknownId && workspace.State.Outbox.Count == 2 && !workspace.State.NeedsRetry,
            "Retry changed request identity or duplicated the queued model.");
        var actual = await forwarding.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(actual.Outbox.Count(item => item.RequestId == unknownId) == 1 &&
            actual.Drafts.Single(item => item.Channel == Channels[0]).Body == newerBody && workspace.State.Drafts[Channels[0]].Body == newerBody,
            "Retry cleared or lost the newer B revision while reconciling A.");

        for (int index = 0; index < 6; index++)
        {
            workspace.EditDraft(Channels[0], $"Fiktiv melding {index}");
            await workspace.SendAsync(Channels[0]).WaitAsync(Deadline).ConfigureAwait(false);
            await SavedAsync(workspace).ConfigureAwait(false);
        }
        var createdIds = forwarding.RequestIds.Distinct(StringComparer.Ordinal).ToArray();
        Check(createdIds.SequenceEqual(createdIds.OrderBy(id => id, StringComparer.Ordinal)), "Request IDs did not retain send creation order.");
        workspace.EditDraft(Channels[1], "Preserve the ninth draft");
        await ExpectAsync("capacity", () => workspace.SendAsync(Channels[1])).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        Check(workspace.State.Outbox.Count == 8 && workspace.State.Drafts[Channels[1]].Body == "Preserve the ninth draft" &&
            !workspace.State.NeedsRetry, "Known capacity rejection lost the draft or became an unknown retry.");

        string oversized = new('ø', 513);
        workspace.EditDraft(Channels[0], oversized);
        await ExpectAsync("text_too_large", workspace.ShutdownAsync).ConfigureAwait(false);
        Check(workspace.State.Ready && !workspace.State.Closing && workspace.State.Drafts[Channels[0]].Body == oversized &&
            forwarding.DisposeCount == 0, "A failed close hid unsavable text or disposed the store.");
        workspace.EditDraft(Channels[0], "Corrected after failed close");
        await workspace.ShutdownAsync().WaitAsync(Deadline).ConfigureAwait(false);
        await workspace.ShutdownAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(forwarding.DisposeCount == 1, "Corrected close did not dispose exactly once.");
        await using (var reopened = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false))
        {
            var snapshot = await reopened.SnapshotAsync().ConfigureAwait(false);
            Check(snapshot.Outbox.Count == 8 && snapshot.Drafts.Single(item => item.Channel == Channels[0]).Body == "Corrected after failed close",
                "Corrected text did not survive the successful close.");
            Check(snapshot.Outbox.Select(item => item.RequestId).SequenceEqual(createdIds), "Reopening changed request creation order.");
        }

        await VerifyCloseDuringSendAsync(path + ".closing", libraryPath).ConfigureAwait(false);
        await VerifyTerminalCleanupAsync(path + ".terminal", libraryPath).ConfigureAwait(false);
        await VerifyRetryFirstAdmissionAsync(path + ".beforecommit", libraryPath).ConfigureAwait(false);
        return $"PASS Windows workspace: coalesced revisions, durable retry capture, capacity, recoverable flush and terminal cleanup; database={path}";
    }

    private static async Task VerifyRetryFirstAdmissionAsync(string path, string libraryPath)
    {
        var store = new ForwardingStore(await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false));
        var workspace = new ChatWorkspace(store, Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline).ConfigureAwait(false);
        const string body = "Newer away/back A remains a draft";
        workspace.EditDraft(Channels[0], body);
        store.UnknownBeforeEnqueue = 1;
        await ExpectAsync("unknown_outcome", () => workspace.SendAsync(Channels[0])).ConfigureAwait(false);
        long capturedRevision = workspace.State.Drafts[Channels[0]].Revision;
        string capturedId = store.RequestIds.Single();
        workspace.EditDraft(Channels[0], "Away before first actual enqueue");
        workspace.EditDraft(Channels[0], body);
        await SavedAsync(workspace).ConfigureAwait(false);
        var beforeRetry = await store.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(beforeRetry.Outbox.Count == 0 && beforeRetry.Drafts.Single(item => item.Channel == Channels[0]).Body == body,
            "Unknown-before-commit fixture unexpectedly enqueued or lost the newer draft.");
        // The first actual enqueue now commits but loses its acknowledgement.
        // Even with NeedsRetry retained, the pump must restore newer equal-body
        // text that native's body-only clear removed during that unknown call.
        store.UnknownAfterCommit = 1;
        await ExpectAsync("unknown_outcome", workspace.RetryAsync).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        var afterSecondUnknown = await store.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(workspace.State.NeedsRetry && afterSecondUnknown.Outbox.Count == 1 &&
            afterSecondUnknown.Drafts.Single(item => item.Channel == Channels[0]).Body == body,
            "A committed retry with a lost acknowledgement failed to restore the newer equal-body draft.");
        await workspace.RetryAsync().WaitAsync(Deadline).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        Check(workspace.State.Drafts[Channels[0]].Revision == capturedRevision + 2 &&
            workspace.State.Drafts[Channels[0]].Body == body && store.RequestIds.All(id => id == capturedId),
            "First actual retry admission lost the newer revision or stable identity.");
        var reconciled = await store.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(reconciled.Outbox.Count == 1 && reconciled.Drafts.Single(item => item.Channel == Channels[0]).Body == body,
            "Native's matching-body draft clear was not followed by saving the preserved newer revision.");
        await workspace.ShutdownAsync().WaitAsync(Deadline).ConfigureAwait(false);
        await using var reopened = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false);
        var persisted = await reopened.SnapshotAsync().ConfigureAwait(false);
        Check(persisted.Outbox.Single().RequestId == capturedId && persisted.Drafts.Single(item => item.Channel == Channels[0]).Body == body,
            "The newer equal-body draft did not survive retry reconciliation and reopen.");
    }

    private static async Task VerifyTerminalCleanupAsync(string path, string libraryPath)
    {
        var store = new ForwardingStore(await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false));
        var workspace = new ChatWorkspace(store, Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline).ConfigureAwait(false);
        workspace.EditDraft(Channels[0], "Queued before terminal cleanup");
        await workspace.SendAsync(Channels[0]).WaitAsync(Deadline).ConfigureAwait(false);
        workspace.EditDraft(Channels[1], "Visible saved draft after cleanup failure");
        await SavedAsync(workspace).ConfigureAwait(false);
        store.FailAfterDispose = 1;
        await ExpectAsync("cleanup_failed", workspace.ShutdownAsync).ConfigureAwait(false);
        var terminal = workspace.State;
        Check(terminal.Terminal && !terminal.Ready && !terminal.Closing && terminal.ErrorCode == "cleanup_failed" &&
            terminal.Outbox.Count == 1 && terminal.Drafts[Channels[1]].Body == "Visible saved draft after cleanup failure",
            "Cleanup failure did not retain a terminal read-only snapshot.");
        int callsAfterCleanup = store.CallCount;
        try
        {
            workspace.EditDraft(Channels[1], "Must never be admitted");
            throw new InvalidOperationException("Terminal workspace allowed editing.");
        }
        catch (InvalidOperationException exception) when (exception.Message == "The workspace store is terminal after cleanup failed.") { }
        try
        {
            await workspace.SendAsync(Channels[0]).WaitAsync(Deadline).ConfigureAwait(false);
            throw new InvalidOperationException("Terminal workspace admitted a send.");
        }
        catch (InvalidOperationException exception) when (exception.Message == "The workspace store is terminal after cleanup failed.") { }
        try
        {
            await workspace.InitializeAsync().WaitAsync(Deadline).ConfigureAwait(false);
            throw new InvalidOperationException("Terminal workspace reinitialized its dead store.");
        }
        catch (ObjectDisposedException) { }
        await workspace.ShutdownAsync().WaitAsync(Deadline).ConfigureAwait(false);
        await workspace.ShutdownAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(store.CallCount == callsAfterCleanup && store.DisposeCount == 1 && workspace.State.Terminal,
            "Acknowledging terminal cleanup touched or disposed the dead store again.");
        await using var reopened = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false);
        var actual = await reopened.SnapshotAsync().ConfigureAwait(false);
        Check(actual.Outbox.Count == 1 && actual.Drafts.Single(item => item.Channel == Channels[1]).Body == terminal.Drafts[Channels[1]].Body,
            "Terminal cleanup lost flushed data or left the real native store open.");
    }

    private static async Task VerifyCloseDuringSendAsync(string path, string libraryPath)
    {
        var store = new ForwardingStore(await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false));
        var workspace = new ChatWorkspace(store, Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline).ConfigureAwait(false);
        workspace.EditDraft(Channels[0], "Captured before window close");
        var gate = new AsyncGate();
        store.EnqueueGate = gate;
        var sending = workspace.SendAsync(Channels[0]);
        await gate.Entered.WaitAsync(Deadline).ConfigureAwait(false);
        var closing = workspace.ShutdownAsync();
        Check(ReferenceEquals(closing, workspace.ShutdownAsync()) && !closing.IsCompleted && store.DisposeCount == 0,
            "Close did not share its task or wait for the admitted send.");
        try
        {
            workspace.EditDraft(Channels[0], "Must be frozen");
            throw new InvalidOperationException("Close allowed a new edit.");
        }
        catch (InvalidOperationException exception) when (exception.Message == "The workspace is closing.") { }
        gate.Release();
        await sending.WaitAsync(Deadline).ConfigureAwait(false);
        await closing.WaitAsync(Deadline).ConfigureAwait(false);
        Check(workspace.State.Outbox.Count == 1 && workspace.State.Drafts[Channels[0]].Body == "" && store.DisposeCount == 1,
            "Close raced the send's model update or native disposal.");
        await using var reopened = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false);
        var snapshot = await reopened.SnapshotAsync().ConfigureAwait(false);
        Check(snapshot.Outbox.Single().State == "queued" && snapshot.Drafts.Single(item => item.Channel == Channels[0]).Body == "",
            "Close failed to flush the post-send empty draft before disposal.");
    }

    private static Task SavedAsync(ChatWorkspace workspace) => WaitStateAsync(workspace, state =>
        state.Drafts.Values.All(draft => draft.Revision == draft.SavedRevision) && !state.Sending);

    private static async Task WaitStateAsync(ChatWorkspace workspace, Func<WorkspaceSnapshot, bool> predicate)
    {
        var ready = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        void Changed() { if (predicate(workspace.State)) ready.TrySetResult(); }
        workspace.Changed += Changed;
        try { Changed(); await ready.Task.WaitAsync(Deadline).ConfigureAwait(false); }
        finally { workspace.Changed -= Changed; }
    }

    private static async Task ExpectAsync(string code, Func<Task> operation)
    {
        try { await operation().WaitAsync(Deadline).ConfigureAwait(false); }
        catch (NativeProbeException exception) when (exception.Code == code) { return; }
        throw new InvalidOperationException($"Expected native error {code}.");
    }

    private static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private sealed class AsyncGate
    {
        private readonly TaskCompletionSource _entered = new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly TaskCompletionSource _released = new(TaskCreationOptions.RunContinuationsAsynchronously);
        public Task Entered => _entered.Task;
        public async Task HoldAsync()
        {
            _entered.TrySetResult();
            await _released.Task.WaitAsync(Deadline).ConfigureAwait(false);
        }
        public void Release() => _released.TrySetResult();
    }

    private sealed class ForwardingStore(NativeSession session) : IWorkspaceStore
    {
        private readonly object _gate = new();
        private readonly List<string> _requestIds = [];
        private int _disposeCount;
        private int _saveCount;
        private int _callCount;
        public AsyncGate? EnqueueGate;
        public int UnknownAfterCommit;
        public int UnknownBeforeEnqueue;
        public int FailNextEnqueue;
        public int FailAfterDispose;
        public int DisposeCount => Volatile.Read(ref _disposeCount);
        public int SaveCount => Volatile.Read(ref _saveCount);
        public int CallCount => Volatile.Read(ref _callCount);
        public string[] RequestIds { get { lock (_gate) return _requestIds.ToArray(); } }
        public Task<StoreSnapshot> SnapshotAsync()
        {
            Interlocked.Increment(ref _callCount);
            return session.SnapshotAsync();
        }
        public Task SaveDraftAsync(string channel, string body)
        {
            Interlocked.Increment(ref _saveCount);
            Interlocked.Increment(ref _callCount);
            return session.SaveDraftAsync(channel, body);
        }
        public async Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId)
        {
            Interlocked.Increment(ref _callCount);
            lock (_gate) _requestIds.Add(requestId);
            if (Interlocked.Exchange(ref FailNextEnqueue, 0) != 0)
                throw new NativeProbeException("storage_error", "The forwarding adapter rejected one retry enqueue without reconciling its original outcome.");
            if (Interlocked.Exchange(ref UnknownBeforeEnqueue, 0) != 0)
                throw new NativeProbeException("unknown_outcome", "The forwarding adapter lost the request outcome before the native enqueue ran.");
            var gate = Interlocked.Exchange(ref EnqueueGate, null);
            if (gate is not null) await gate.HoldAsync().ConfigureAwait(false);
            var queued = await session.EnqueueAsync(channel, body, requestId).ConfigureAwait(false);
            if (Interlocked.Exchange(ref UnknownAfterCommit, 0) != 0)
                throw new NativeProbeException("unknown_outcome", "The test forwarding adapter lost one committed result.");
            return queued;
        }
        public async ValueTask DisposeAsync()
        {
            Interlocked.Increment(ref _callCount);
            await session.DisposeAsync().ConfigureAwait(false);
            Interlocked.Increment(ref _disposeCount);
            if (Interlocked.Exchange(ref FailAfterDispose, 0) != 0)
                throw new NativeProbeException("cleanup_failed", "The forwarding adapter reported failure after closing the real native store.");
        }
    }
}
