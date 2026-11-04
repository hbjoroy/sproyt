namespace Sproyt.WindowsProbe;

internal sealed partial class LiveApplication
{
    private readonly NativeThreadFeatureController _threads = new();
    private long _directoryReadyGeneration = -1;
    private long _metadataVisit = -1, _metadataView = -1;

    private void SynchronizeThreadProjection(NativeChatSnapshot chat)
    {
        var feature = _threads.Capture();
        if (feature.Visit is not { } visit) return;
        if (visit.ChannelId != chat.SelectedChannel || !chat.Channels.Any(item => item.Id == visit.ChannelId))
        { _threads.Close(visit); return; }
        if (chat.Conversation is not { } conversation) return;
        if (visit.IsDirectory && _directoryReadyGeneration == visit.Generation && conversation.ThreadSummariesLoaded)
            _threads.ApplyDirectory(visit, conversation.ThreadSummaries, chat.Messages);
        else if (!visit.IsDirectory && feature.Thread is not null && conversation.Thread is { } thread && thread.RootMessageId == visit.RootMessageId)
            _threads.ApplyThread(visit, thread);
    }

    private async Task RefreshConversationMetadataAsync(NativeNavigationVisit visit)
    {
        if (!_navigation.IsCurrent(visit) || Chat.History is not { } history) return;
        try
        {
            await Task.WhenAll(
                _chat.ListThreadSummariesAsync(visit.Target.ChannelId, history.ViewGeneration, visit.UserId, visit.SessionStamp, _lifetime.Token),
                _chat.ListChannelReactionsAsync(visit.Target.ChannelId, history.ViewGeneration, visit.UserId, visit.SessionStamp, _lifetime.Token));
        }
        catch { /* Explicit feature retry/readback owns errors; no implicit write retry. */ }
    }

    public Task RefreshConversationAsync(NativeMessageActionScope scope) =>
        CurrentActionScope(scope) && _navigation.Capture().Current is { } navigation && navigation.Generation == scope.ChannelVisit
            ? RefreshConversationMetadataAsync(navigation) : Task.CompletedTask;

    public Task OpenThreadDirectoryAsync(NativeMessageActionScope scope)
    {
        if (scope.ThreadRootId is not null || _navigation.Capture().Current is not { } navigation) return Task.CompletedTask;
        NativeThreadVisit? directory = null; long view = -1;
        CancellationTokenSource? retired = null;
        AdmitNavigation(navigation, () =>
        {
            if (!CurrentActionScope(scope) || Chat.History is not { } history) return;
            retired = Interlocked.Exchange(ref _conversationVisitCancellation, new());
            view = history.ViewGeneration; directory = _threads.OpenDirectory(scope.ChannelId);
        });
        retired?.Cancel(); retired?.Dispose();
        return directory is null ? Task.CompletedTask : LoadDirectoryAsync(directory, navigation, view);
    }

    private async Task LoadDirectoryAsync(NativeThreadVisit visit, NativeNavigationVisit navigation, long view)
    {
        try
        {
            var summaries = await _chat.ListThreadSummariesAsync(visit.ChannelId, view, visit.UserId, visit.SessionStamp, _lifetime.Token);
            AdmitNavigation(navigation, () =>
            {
                if (!_threads.IsCurrent(visit) || Chat.History?.ViewGeneration != view) return;
                _directoryReadyGeneration = visit.Generation;
                _threads.ApplyDirectory(visit, summaries, Chat.Messages);
            });
        }
        catch { _threads.Fail(visit, "Trådoversikta kunne ikkje hentast. Prøv igjen."); }
    }

    private void WithThreadVisit(NativeThreadVisit visit, Action<NativeNavigationVisit> action)
    {
        if (_navigation.Capture().Current is not { } navigation || navigation.UserId != visit.UserId ||
            navigation.SessionStamp != visit.SessionStamp || navigation.Target.ChannelId != visit.ChannelId ||
            navigation.Target.ThreadId != visit.RootMessageId) return;
        AdmitNavigation(navigation, () => { if (_threads.IsCurrent(visit)) action(navigation); });
    }
    public Task CloseThreadAsync(NativeThreadVisit visit)
    {
        NativeNavigationVisit? next = null;
        WithThreadVisit(visit, _ =>
        {
            _threads.Close(visit);
            next = visit.IsDirectory ? _navigation.RestartCurrent() : _navigation.Select(new(visit.ChannelId));
        });
        return NavigateVisitAsync(next);
    }
    public Task RetryThreadAsync(NativeThreadVisit visit)
    {
        NativeNavigationVisit? retry = null, parent = null; NativeThreadVisit? directory = null; long view = -1;
        WithThreadVisit(visit, navigation =>
        {
            if (!visit.IsDirectory) { retry = _navigation.RestartCurrent(); return; }
            if (Chat.History is not { } history) return;
            parent = navigation; view = history.ViewGeneration; directory = _threads.OpenDirectory(visit.ChannelId);
        });
        return directory is not null && parent is not null ? LoadDirectoryAsync(directory, parent, view) : NavigateVisitAsync(retry);
    }
    public void RememberThreadViewport(NativeThreadVisit visit, NativeScrollAnchor anchor) =>
        WithThreadVisit(visit, navigation => { if (_threads.SaveAnchor(visit, anchor)) _navigation.SaveAnchor(navigation, anchor); });
    public async Task MarkThreadVisibleReadAsync(NativeThreadVisit visit, long viewGeneration, long threadGeneration, IReadOnlyList<string> messageIds)
    {
        // Visibility is measured and gated in the UI. The shared owner policy
        // checks projection provenance and excludes roots and pending rows.
        var report = _threads.VisibleRead(visit, threadGeneration, true, true, false, messageIds);
        if (report is null || report.ViewGeneration != viewGeneration) return;
        if (visit.RootMessageId is not string root || _navigation.Capture().Current is not { } navigation) return;
        var scope = new NativeMessageActionScope(visit.UserId, visit.SessionStamp, visit.ChannelId, navigation.Generation, root, visit.Generation);
        try
        {
            using var operation = AdmitMessageAction(scope);
            await _chat.MarkThreadVisibleReadAsync(visit.ChannelId, root, viewGeneration, threadGeneration, report.MessageIds,
                visit.UserId, visit.SessionStamp, operation.Token);
        }
        catch { /* Only confirmed server progress is projected. */ }
    }

    private bool CurrentActionScope(NativeMessageActionScope scope, bool requireConnection = true)
    {
        var auth = Authentication; var navigation = _navigation.Capture(); var chat = Chat;
        if (_closing || ConversationCovered || auth.UserId != scope.UserId || auth.SessionStamp != scope.SessionStamp || auth.UserId != _account ||
            auth.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline) ||
            (requireConnection && (auth.Phase != NativeAuthPhase.Active || !chat.Connected)) ||
            navigation is not { Pending: false, Error: null, Current: { } current } || current.Generation != scope.ChannelVisit ||
            current.UserId != scope.UserId || current.SessionStamp != scope.SessionStamp || chat.UserId != scope.UserId ||
            current.Target.ChannelId != scope.ChannelId || chat.SelectedChannel != scope.ChannelId || chat.Loading) return false;
        var feature = _threads.Capture();
        return scope.ThreadRootId is null ? current.Target.ThreadId is null && feature.Visit is null :
            current.Target.ThreadId == scope.ThreadRootId && feature is { Loading: false, Error: null, AtCapacity: false, Visit: { } visit } &&
            visit.Generation == scope.ThreadVisit && visit.RootMessageId == scope.ThreadRootId &&
            visit.UserId == scope.UserId && visit.SessionStamp == scope.SessionStamp &&
            (!requireConnection && !chat.Connected || feature.Thread is { } shown && chat.Conversation?.Thread is { Loading: false } actual &&
            actual.ChannelId == scope.ChannelId && actual.RootMessageId == scope.ThreadRootId && actual.ViewGeneration == chat.History?.ViewGeneration &&
            actual.ViewGeneration == shown.ViewGeneration && actual.Generation == shown.Generation);
    }

    private NativeMessage CurrentMutationMessage(NativeMessageMutation mutation)
    {
        if (!CurrentActionScope(mutation.Scope)) throw new OperationCanceledException("The conversation visit was replaced.");
        var chat = Chat;
        IEnumerable<NativeMessage> messages = mutation.Scope.ThreadRootId is null ? chat.Messages :
            chat.Conversation?.Thread is { Root: { } root } thread ? new[] { root }.Concat(thread.Replies) : [];
        return messages.FirstOrDefault(message => message.Id == mutation.MessageId && message.ChannelId == mutation.Scope.ChannelId)
            ?? throw new OperationCanceledException("The message is no longer available in this visit.");
    }
    public async Task ExecuteMessageMutationAsync(NativeMessageMutation mutation)
    {
        using var operation = AdmitMessageAction(mutation.Scope);
        var message = CurrentMutationMessage(mutation);
        if (mutation.Kind is NativeMessageMutationKind.Edit or NativeMessageMutationKind.Delete && message.AuthorId != mutation.Scope.UserId)
            throw NativeCommandException.FromServer("local", "permission_denied", "Du kan berre endre dine eigne meldingar.");
        if (message.DeletedAt is not null) throw new OperationCanceledException("The message was deleted.");
        var scope = mutation.Scope; long generation = Chat.History!.ViewGeneration;
        string request = "windows-action-" + Guid.NewGuid().ToString("N");
        switch (mutation.Kind)
        {
            case NativeMessageMutationKind.Edit:
                await _chat.EditMessageAsync(scope.ChannelId, message.Id, mutation.Value!, request, generation, scope.UserId, scope.SessionStamp, operation.Token); break;
            case NativeMessageMutationKind.Delete:
                await _chat.DeleteMessageAsync(scope.ChannelId, message.Id, request, generation, scope.UserId, scope.SessionStamp, operation.Token); break;
            case NativeMessageMutationKind.ToggleReaction:
                if (Chat.Conversation is not { ReactionsLoaded: true }) throw new InvalidOperationException("Reaction state must be loaded first.");
                bool mine = Chat.Conversation.Reactions.Any(item => item.MessageId == message.Id && item.Emoji == mutation.Value && item.ReactedByMe);
                if (mine == mutation.ExpectedMine) return;
                var change = await _chat.ToggleReactionAsync(scope.ChannelId, message.Id, mutation.Value!, request, generation, scope.UserId, scope.SessionStamp, operation.Token);
                if (change.Added != mutation.ExpectedMine) throw new IOException("The reaction changed concurrently; read back its confirmed state.");
                break;
        }
    }
    public async Task<bool?> ReadBackMessageMutationAsync(NativeMessageMutation mutation)
    {
        using var operation = AdmitMessageAction(mutation.Scope);
        var message = CurrentMutationMessage(mutation); var scope = mutation.Scope; long generation = Chat.History!.ViewGeneration;
        if (mutation.Kind == NativeMessageMutationKind.ToggleReaction)
        {
            var reactions = await _chat.ListChannelReactionsAsync(scope.ChannelId, generation, scope.UserId, scope.SessionStamp, operation.Token);
            if (!CurrentActionScope(scope)) return null;
            return reactions.Any(item => item.MessageId == message.Id && item.Emoji == mutation.Value && item.ReactedByMe) == mutation.ExpectedMine;
        }
        var read = await _chat.ReadMessageAsync(scope.ChannelId, message.ParentMessageId ?? message.Id, message.Id,
            generation, scope.UserId, scope.SessionStamp, operation.Token);
        if (!CurrentActionScope(scope) || read is null) return null;
        return mutation.Kind == NativeMessageMutationKind.Delete ? read.DeletedAt is not null : read.DeletedAt is null && read.Body == mutation.Value;
    }

    private void WithComposer(NativeMessageActionScope scope, Action<ChatWorkspace, string> action)
    {
        if (_navigation.Capture().Current is not { } visit || visit.Generation != scope.ChannelVisit) return;
        AdmitNavigation(visit, () =>
        {
            if (_workspace is { } workspace && CurrentActionScope(scope, requireConnection: false))
                action(workspace, new ComposerTarget(scope.ChannelId, scope.ThreadRootId).Key);
        });
    }
    public void EditComposer(NativeMessageActionScope scope, string body) => WithComposer(scope, (workspace, key) =>
    {
        workspace.EnsureChannels([key]);
        workspace.EditComposer(key, body);
    });
    private CancellationTokenSource AdmitMessageAction(NativeMessageActionScope scope) => _authentication.ProjectSnapshot(_ =>
    {
        CancellationTokenSource? admitted = null;
        if (_navigation.Capture().Current is { } visit)
            _navigation.AdmitCurrent(visit, () =>
            {
                if (CurrentActionScope(scope)) admitted = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token, _conversationVisitCancellation.Token);
            });
        return admitted ?? throw new OperationCanceledException("The message action belongs to an earlier visit.");
    });
    public async Task SendComposerAsync(NativeMessageActionScope scope)
    {
        try
        {
            Task? admitted = null;
            WithComposer(scope, (workspace, key) =>
            {
                ApproveEmptyQueue();
                if (workspace.State.Outbox.Any(item => item.State != "rejected") && !SameSendFamily(_admittedFamily, scope.SessionStamp)) return;
                if (NativeUploadPresentation.BlocksSend(_uploads?.Capture(), scope.UserId, SendFamily(scope.SessionStamp), scope.ChannelId, scope.ThreadRootId)) return;
                var draft = workspace.State.Drafts.GetValueOrDefault(key);
                if (draft is null || !NativeRichText.AttachmentSuffixVisible(draft.Body, draft.Attachments)) return;
                admitted = workspace.SendAsync(key, draft.Revision);
            });
            if (admitted is null) return;
            await admitted;
        }
        catch
        {
            if (_navigation.Capture().Current is { } visit) AdmitNavigation(visit, () =>
            {
                if (CurrentActionScope(scope, requireConnection: false))
                    _error = "Meldinga kunne ikkje leggjast i sendekøen. Utkastet er teke vare på.";
            });
        }
        ChangedState();
    }
}
