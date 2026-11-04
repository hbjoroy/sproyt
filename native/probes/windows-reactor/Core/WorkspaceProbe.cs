namespace Sproyt.WindowsProbe;

public static class WorkspaceProbe
{
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(10);
    private static readonly string[] Channels = ["probe-general", "probe-team"];

    public static async Task<string> RunAsync(string database, string libraryPath)
    {
        Check(LiveApplication.SameSendFamily("7:family-a", "7:family-a:2") &&
            !LiveApplication.SameSendFamily("7:family-a", "8:family-a:1") &&
            !LiveApplication.SameSendFamily("7:family-a", "7:family-b:2") &&
            !LiveApplication.SameSendFamily(null, "7:family-a:1"),
            "Old queued sends must require approval after a new login, while same-family rotation preserves admission.");
        string path = Path.GetFullPath(database);
        using (var approval = new SendAdmissionStore(path + ".send-consent", "instance", "account"))
            approval.Approve("7:family-a");
        using (var afterRestart = new SendAdmissionStore(path + ".send-consent", "instance", "account"))
            Check(!LiveApplication.SameSendFamily(afterRestart.Family, "8:family-b:1"),
                "Restart must not approve old queued sends for a new authentication family.");
        Check(!File.Exists(path) && !File.Exists(path + ".closing") && !File.Exists(path + ".terminal") &&
            !File.Exists(path + ".beforecommit") && !File.Exists(path + ".delivery") && !File.Exists(path + ".threads") && !File.Exists(path + ".uploads"), "Workspace probe refuses existing test databases.");
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
        await VerifyDeliveryReconciliationAsync(path + ".delivery", libraryPath).ConfigureAwait(false);
        await VerifyThreadTargetsAsync(path + ".threads", libraryPath).ConfigureAwait(false);
        await VerifyUploadDraftAsync(path + ".uploads", libraryPath).ConfigureAwait(false);
        return $"PASS Windows workspace: coalesced revisions, durable retry capture, capacity, recoverable flush and terminal cleanup; database={path}";
    }

    private static async Task VerifyUploadDraftAsync(string path, string libraryPath)
    {
        const string marker = "[[media:00000000-0000-0000-0000-000000000001|image/jpeg|A%20B.jpg]]";
        string channel = Channels[0];
        string target = new ComposerTarget(channel, "upload-thread").Key;
        string sibling = new ComposerTarget(channel, "other-thread").Key;
        var session = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var store = new ForwardingStore(session);
        var workspace = new ChatWorkspace(store, Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline);
        workspace.EditDraft(channel, "initial");
        foreach (var key in new[] { target, sibling })
        {
            workspace.EnsureChannels([key]);
            workspace.EditDraft(key, "initial");
        }
        await SavedAsync(workspace);
        var gate = new AsyncGate();
        store.UploadGate = gate;
        store.UnknownAfterUpload = 1;
        var attaching = workspace.AttachUploadAsync(target, "upload-job", marker);
        await gate.Entered.WaitAsync(Deadline);
        workspace.EditDraft(target, "newer");
        await ExpectAsync("upload_pending", () => workspace.SendAsync(target));
        gate.Release();
        await ExpectAsync("unknown_outcome", () => attaching);
        await ExpectAsync("upload_pending", () => workspace.SendAsync(target));
        await ExpectAsync("upload_pending", () => workspace.AttachUploadAsync(target, "different-job", marker));
        await workspace.ReconcileUploadReceiptsAsync([]);
        await workspace.SendAsync(channel).WaitAsync(Deadline);
        var snapshot = await session.SnapshotAsync();
        Check(snapshot.Drafts.Single(d => d.Key == target).Body == "initial\n" + marker &&
            snapshot.Drafts.Single(d => d.Key == sibling).Body == "initial" &&
            snapshot.Outbox.Single().ParentMessageId is null && workspace.State.Drafts[target].Body == "newer",
            "An uncertain attachment overwrote newer text or blocked a different composer.");
        await workspace.AttachUploadAsync(target, "upload-job", marker).WaitAsync(Deadline);
        await SavedAsync(workspace);
        var merged = "newer\n" + marker;
        Check((await session.SnapshotAsync()).Drafts.Single(d => d.Key == target).Body == merged,
            "Retry lost an intervening edit or duplicated the marker.");
        await workspace.SendAsync(target).WaitAsync(Deadline);
        workspace.EditDraft(target, "newer");
        await SavedAsync(workspace);
        await workspace.ShutdownAsync().WaitAsync(Deadline);

        var reopened = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var restoredStore = new ForwardingStore(reopened) { UnknownAfterUpload = 1 };
        var restored = new ChatWorkspace(restoredStore, Channels);
        await restored.InitializeAsync().WaitAsync(Deadline);
        await ExpectAsync("unknown_outcome", () => restored.AttachUploadAsync(target, "upload-job", marker));
        await restored.AttachUploadAsync(target, "upload-job", marker).WaitAsync(Deadline);
        await SavedAsync(restored);
        var result = await reopened.SnapshotAsync();
        Check(result.Drafts.Single(d => d.Key == target).Body == "newer" &&
            result.Outbox.Single(q => q.ParentMessageId == "upload-thread").Body == merged &&
            restored.State.Drafts[target].Body == "newer", "A replay resurrected a sent marker in its replacement draft.");
        var replay = await reopened.AttachUploadAsync(ComposerTarget.FromKey(target), "upload-job", "readback-test",
            new string('x', 128 * 1024), marker);
        Check(replay.AlreadyApplied && replay.Body is null, "A huge replacement editor body blocked receipt lookup.");
        const string openCode = "A literal marker:\n```text\n" + marker + "\n";
        restored.EditComposer(target, openCode);
        await restored.AttachUploadAsync(target, "chip-one", marker).WaitAsync(Deadline);
        await restored.AttachUploadAsync(target, "chip-two", marker).WaitAsync(Deadline);
        await SavedAsync(restored);
        var editor = restored.State.Drafts[target];
        Check(editor.ComposerText == openCode && editor.Attachments.SequenceEqual(new[] { marker, marker }) &&
            editor.Body == openCode + marker + "\n" + marker, "An open fence, literal marker or trailing newline was reinterpreted.");
        await restored.ReconcileUploadReceiptsAsync([]);
        await restored.ShutdownAsync().WaitAsync(Deadline);

        var last = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var composer = new ChatWorkspace(new NativeWorkspaceStoreAdapter(last), Channels);
        await composer.InitializeAsync().WaitAsync(Deadline);
        var restoredEditor = composer.State.Drafts[target];
        Check(restoredEditor.ComposerText == openCode && restoredEditor.Attachments.SequenceEqual(editor.Attachments),
            "Receipt collection or restart lost exact editor text or duplicate chips.");
        composer.RemoveDraftAttachment(target, 1, "stale-marker");
        Check(composer.State.Drafts[target].Attachments.Count == 2, "Stale removal changed the draft.");
        composer.RemoveDraftAttachment(target, 0, marker);
        string closedCode = openCode + "```\n\n";
        composer.EditComposer(target, closedCode);
        await SavedAsync(composer);
        var one = (await last.SnapshotAsync()).Drafts.Single(d => d.Key == target);
        Check(one.ComposerText == closedCode && one.Attachments.SequenceEqual(new[] { marker }) &&
            one.Body == closedCode + marker, "Removal or editing changed the literal code or the remaining chip.");
        await ExpectAsync("draft_changed", () => composer.SendAsync(target, restoredEditor.Revision));
        await composer.SendAsync(target, composer.State.Drafts[target].Revision).WaitAsync(Deadline);
        await SavedAsync(composer);
        Check(composer.State.Drafts[target].Attachments.Count == 0 &&
            (await last.SnapshotAsync()).Outbox.Any(q => q.Body == closedCode + marker && q.ParentMessageId == "upload-thread"),
            "Sending did not atomically consume the revision's chips and preserve the exact wire body.");
        await composer.ShutdownAsync().WaitAsync(Deadline);
        var closingSession = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var closingStore = new ForwardingStore(closingSession);
        var closing = new ChatWorkspace(closingStore, Channels);
        await closing.InitializeAsync().WaitAsync(Deadline);
        closing.EnsureChannels([target]);
        var retiringGate = new AsyncGate();
        closingStore.UploadGate = retiringGate; closingStore.UnknownAfterUpload = 1;
        var running = closing.AttachUploadAsync(target, "held-family-retire", marker);
        await retiringGate.Entered.WaitAsync(Deadline);
        var settlement = closing.SettleUploadAsync("held-family-retire");
        Check(!settlement.IsCompleted, "Held-family retirement passed an active handoff.");
        closing.EditComposer(target, "edited during retirement");
        retiringGate.Release();
        await ExpectAsync("unknown_outcome", () => running);
        await settlement.WaitAsync(Deadline);
        await SavedAsync(closing);
        Check(closing.State.Drafts[target].Attachments.SequenceEqual(new[] { marker }),
            "Read-only retirement lost a committed marker.");

        closingStore.UnknownAfterUpload = 1;
        await ExpectAsync("unknown_outcome", () => closing.AttachUploadAsync(target, "closing-after-commit", marker));
        closing.EditComposer(target, "newer before logout");
        closingStore.UnknownBeforeUpload = 1;
        await ExpectAsync("unknown_outcome", () => closing.AttachUploadAsync(channel, "closing-before-commit", marker));
        closing.EditComposer(channel, "different dirty draft");
        await closing.ShutdownAsync().WaitAsync(Deadline);
        await using var verified = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var closed = await verified.SnapshotAsync();
        var present = closed.Drafts.Single(d => d.Key == target);
        var absent = closed.Drafts.Single(d => d.Key == channel);
        Check(present.ComposerText == "newer before logout" && present.Attachments.SequenceEqual(new[] { marker, marker }) &&
            absent.ComposerText == "different dirty draft" && absent.Attachments.Count == 0,
            "Shutdown did not reconcile committed and missing receipts without starting a new attachment.");
        await ExpectAsync("receipt_not_found", () => verified.AttachUploadAsync(new(channel), "closing-before-commit",
            "verify", "", marker, receiptOnly: true));

    }

    private static async Task VerifyThreadTargetsAsync(string path, string libraryPath)
    {
        const string body = "Identical text in three composers";
        string channel = Channels[0];
        var first = new ComposerTarget(channel, "thread-one");
        var second = new ComposerTarget(channel, "thread-two");
        var session = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var workspace = new ChatWorkspace(new NativeWorkspaceStoreAdapter(session), Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline);
        workspace.EditDraft(channel, body);
        workspace.EnsureChannels([first.Key]);
        workspace.EditDraft(first.Key, body);
        workspace.EnsureChannels([second.Key]);
        workspace.EditDraft(second.Key, body);
        await SavedAsync(workspace);
        await workspace.SendAsync(first.Key).WaitAsync(Deadline);
        await SavedAsync(workspace);
        var queued = workspace.State.Outbox.Single();
        Check(queued.Channel == channel && queued.ParentMessageId == first.ParentMessageId && queued.Body == body,
            "Reply admission lost the canonical channel, parent or body.");
        await session.DeliveryAsync(queued.RequestId, "dispatch");
        await workspace.ShutdownAsync().WaitAsync(Deadline);

        var reopened = await NativeSession.OpenAsync(path, libraryPath, clearEmptyDraft: true);
        var restored = new ChatWorkspace(new NativeWorkspaceStoreAdapter(reopened), Channels);
        await restored.InitializeAsync().WaitAsync(Deadline);
        var state = restored.State;
        Check(state.Outbox.Single() == (queued with { State = "uncertain" }) && state.Drafts[channel].Body == body &&
            state.Drafts[second.Key].Body == body && !state.Drafts.ContainsKey(first.Key),
            "Restart lost a thread target or consumed another composer's identical draft.");
        for (int visit = 0; visit < 530; visit++)
            restored.EnsureChannels([new ComposerTarget(channel, $"unwritten-{visit}").Key]);
        Check(restored.State.Drafts.Count == Channels.Length + 2 && restored.State.Drafts[second.Key].Body == body,
            "Clean thread visits accumulated or displaced a nonempty draft.");
        await restored.ShutdownAsync().WaitAsync(Deadline);
    }

    private static async Task VerifyDeliveryReconciliationAsync(string path, string libraryPath)
    {
        var store = await NativeSession.OpenAsync(path, libraryPath);
        var workspace = new ChatWorkspace(new NativeWorkspaceStoreAdapter(store), Channels);
        await workspace.InitializeAsync().WaitAsync(Deadline);
        workspace.EditDraft(Channels[0], "Rejected after commit");
        await workspace.SendAsync(Channels[0]).WaitAsync(Deadline);
        await SavedAsync(workspace);
        string rejected = workspace.State.Outbox.Single().RequestId;
        await store.DeliveryAsync(rejected, "dispatch");
        await store.DeliveryAsync(rejected, "reject");
        // The server/store commit completed but its local publication was lost.
        var published = workspace.State.Outbox.Select(item => item.RequestId).ToHashSet();
        var persisted = await store.SnapshotAsync();
        var settled = LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        Check(settled.SetEquals([rejected]), "A committed reject did not retire its retry notice.");
        Check(workspace.State.Outbox.Single().State == "rejected", "Lost reject result left a pending UI row.");
        int duplicateNotifications = 0;
        void Changed() => duplicateNotifications++;
        workspace.Changed += Changed;
        LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        workspace.Changed -= Changed;
        Check(duplicateNotifications == 0, "Unchanged rejected reconciliation restarted the application pump.");

        workspace.EditDraft(Channels[0], "Acknowledged and forgotten after commit");
        await workspace.SendAsync(Channels[0]).WaitAsync(Deadline);
        await SavedAsync(workspace);
        string acknowledged = workspace.State.Outbox.Single(item => item.RequestId != rejected).RequestId;
        await store.DeliveryAsync(acknowledged, "dispatch");
        await store.DeliveryAsync(acknowledged, "acknowledge");
        published = workspace.State.Outbox.Select(item => item.RequestId).ToHashSet();
        persisted = await store.SnapshotAsync();
        settled = LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        Check(!settled.Contains(acknowledged), "Acknowledgement still awaiting forget lost its local storage retry.");
        await store.DeliveryAsync(acknowledged, "forget");
        published = workspace.State.Outbox.Select(item => item.RequestId).ToHashSet();
        persisted = await store.SnapshotAsync();
        // A new enqueue after capture must survive the older reconciliation.
        workspace.EditDraft(Channels[1], "Newer queued message");
        await workspace.SendAsync(Channels[1]).WaitAsync(Deadline);
        await SavedAsync(workspace);
        settled = LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        Check(settled.SetEquals([rejected, acknowledged]), "Post-commit reconciliation resolved the wrong request's retry.");
        Check(workspace.State.Outbox.Count == 2 && workspace.State.Outbox.All(item => item.RequestId != acknowledged) &&
            workspace.State.Outbox.Any(item => item.Channel == Channels[1] && item.State == "queued"),
            "Lost forget result left a ghost or reconciliation removed a newer enqueue.");
        await store.DeliveryAsync(rejected, "discard_rejected");
        published = workspace.State.Outbox.Select(item => item.RequestId).ToHashSet();
        persisted = await store.SnapshotAsync();
        LiveApplication.ReconcileDeliveries(workspace, published, persisted.Outbox);
        Check(workspace.State.Outbox.Count == 1 && workspace.State.Outbox[0].Channel == Channels[1],
            "Committed rejected discard was not reconciled independently of newer pending work.");
        await workspace.ShutdownAsync().WaitAsync(Deadline);
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
        // The newer equal-body revision must remain in SQLite itself: no
        // coordinator save is allowed to repair a clear/re-save crash window.
        int savesBeforeRetry = store.SaveCount;
        long durableRevision = beforeRetry.Drafts.Single(item => item.Channel == Channels[0]).Revision;
        store.UnknownAfterCommit = 1;
        await ExpectAsync("unknown_outcome", workspace.RetryAsync).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        var afterSecondUnknown = await store.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(workspace.State.NeedsRetry && afterSecondUnknown.Outbox.Count == 1 &&
            afterSecondUnknown.Drafts.Single(item => item.Channel == Channels[0]).Body == body &&
            afterSecondUnknown.Drafts.Single(item => item.Channel == Channels[0]).Revision == durableRevision && store.SaveCount == savesBeforeRetry,
            "A committed retry changed the newer equal-body draft or required a non-atomic repair save.");
        await workspace.RetryAsync().WaitAsync(Deadline).ConfigureAwait(false);
        await SavedAsync(workspace).ConfigureAwait(false);
        Check(workspace.State.Drafts[Channels[0]].Revision == capturedRevision + 2 &&
            workspace.State.Drafts[Channels[0]].Body == body && store.RequestIds.All(id => id == capturedId),
            "First actual retry admission lost the newer revision or stable identity.");
        var reconciled = await store.SnapshotAsync().WaitAsync(Deadline).ConfigureAwait(false);
        Check(reconciled.Outbox.Count == 1 && reconciled.Drafts.Single(item => item.Channel == Channels[0]).Body == body,
            "Revision-bound admission changed the preserved newer draft.");
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

    private sealed class ForwardingStore(NativeSession session) : IWorkspaceStore, IWorkspaceUploadStore
    {
        private readonly object _gate = new();
        private readonly List<string> _requestIds = [];
        private int _disposeCount;
        private int _saveCount;
        private int _callCount;
        public AsyncGate? EnqueueGate;
        public AsyncGate? UploadGate;
        public int UnknownAfterUpload;
        public int UnknownBeforeUpload;
        public Task<UploadDraftResult?> ReadUploadReceiptAsync(string key, string jobId, string operationId, string marker) =>
            _backing.ReadUploadReceiptAsync(key, jobId, operationId, marker);
        private readonly NativeWorkspaceStoreAdapter _backing = new(session);
        public Task<long> SaveComposerAsync(string key, string body, DraftComposer composer) => _backing.SaveComposerAsync(key, body, composer);
        public async Task<UploadDraftResult> AttachUploadAsync(string key, string job, string operation, string body, string marker, DraftComposer composer)
        {
            if (Interlocked.Exchange(ref UnknownBeforeUpload, 0) != 0)
                throw new NativeProbeException("unknown_outcome", "Lost the handoff before any native write.");
            var result = await _backing.AttachUploadAsync(key, job, operation, body, marker, composer);
            var gate = Interlocked.Exchange(ref UploadGate, null);
            if (gate is not null) await gate.HoldAsync();
            if (Interlocked.Exchange(ref UnknownAfterUpload, 0) != 0)
                throw new NativeProbeException("unknown_outcome", "Lost the result of attaching a completed upload.");
            return result;
        }
        public Task ReconcileUploadReceiptsAsync(IReadOnlyList<string> ids) => _backing.ReconcileUploadReceiptsAsync(ids);
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
        public Task<long> SaveDraftAsync(string channel, string body)
        {
            Interlocked.Increment(ref _saveCount);
            Interlocked.Increment(ref _callCount);
            return _backing.SaveDraftAsync(channel, body);
        }
        public async Task<QueuedMessage> EnqueueAsync(string channel, string body, string requestId, long expectedDraftRevision)
        {
            Interlocked.Increment(ref _callCount);
            lock (_gate) _requestIds.Add(requestId);
            if (Interlocked.Exchange(ref FailNextEnqueue, 0) != 0)
                throw new NativeProbeException("storage_error", "The forwarding adapter rejected one retry enqueue without reconciling its original outcome.");
            if (Interlocked.Exchange(ref UnknownBeforeEnqueue, 0) != 0)
                throw new NativeProbeException("unknown_outcome", "The forwarding adapter lost the request outcome before the native enqueue ran.");
            var gate = Interlocked.Exchange(ref EnqueueGate, null);
            if (gate is not null) await gate.HoldAsync().ConfigureAwait(false);
            var queued = await _backing.EnqueueAsync(channel, body, requestId, expectedDraftRevision).ConfigureAwait(false);
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
