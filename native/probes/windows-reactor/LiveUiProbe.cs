using Microsoft.UI.Reactor;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Automation;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Storage;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Synthetic HTTP/view fixture, without live grants or production data.
internal sealed class LiveUiFixture : ILiveApplication
{
    private const string User = "00000000-0000-0000-0000-000000000001";
    private string _selected = "channel-a";
    private readonly Dictionary<string, DraftView> _drafts = new() { ["channel-a"] = new("",0,0), ["channel-b"] = new("",0,0) };
    private readonly List<NativeMessage> _messages = [];
    private readonly List<QueuedMessage> _outbox = [];
    private readonly NativeMediaViewFixture _mediaFixture = new(User);
    private readonly NativeNavigationController _navigation = new();
    private readonly NativeThreadFeatureController _threads = new();
    private readonly List<NativeMessageReaction> _reactions = [];
    private readonly Dictionary<string, ulong> _threadLastRead = [];
    private NativeThreadSnapshot? _thread;
    private long _threadGeneration;
    internal bool FailNextMutationAfterCommit { get; set; }
    internal int MutationExecutions { get; private set; }
    internal int MutationReadBacks { get; private set; }
    internal QueuedMessage? LastEnqueuedReply { get; private set; }
    internal NativeMessage? Message(string id) => _messages.FirstOrDefault(message => message.Id == id);
    internal List<(string Root, ulong Sequence)> ThreadReadReports { get; } = [];
    private readonly Dictionary<string, ulong> _lastRead = [];
    private readonly List<ulong> _readReports = [];
    private bool _historyMode, _loadingOlder;
    private ulong _historyStart = 301;
    private long _historyGeneration = 1;
    internal int OlderCalls { get; private set; }
    internal IReadOnlyList<ulong> ReadReports => _readReports.ToArray();
    internal LiveUiFixture()
    {
        _drafts["channel-dm"] = new("", 0, 0);
        SyncNavigation();
        _navigation.Changed += () => Changed?.Invoke();
        _threads.BindScope(Authentication.UserId, Authentication.SessionStamp);
        _threads.Changed += () => Changed?.Invoke();
        var initial = _navigation.Select(new(_selected));
        if (initial is not null) _navigation.Complete(initial);
    }
    public event Action? Changed;
    public NativeAuthSnapshot Authentication => _mediaFixture.Owner.Snapshot;
    private IReadOnlyList<NativeChannel> Channels =>
        [new("channel-a", "Prat", "UI-prøve", "fixture-circle", LastReadSequence: _lastRead.GetValueOrDefault("channel-a"), LatestSequence: _messages.Where(item => item.ChannelId == "channel-a").Select(item => item.Sequence).DefaultIfEmpty(0UL).Max()),
         new("channel-b", "Idear", "UI-prøve", "fixture-circle", LastReadSequence: _lastRead.GetValueOrDefault("channel-b"), LatestSequence: _messages.Where(item => item.ChannelId == "channel-b").Select(item => item.Sequence).DefaultIfEmpty(0UL).Max()),
         new("channel-dm", "Testven", Kind: "direct", IsDirect: true)];
    public NativeChatSnapshot Chat => new(true, false, null, Channels, _selected,
        _messages.Where(message => message.ChannelId == _selected && message.ParentMessageId is null).OrderBy(item => item.Sequence).ToArray(), User,
        new(_selected, _historyGeneration, _loadingOlder, _historyMode && _selected == "channel-a" && _historyStart > 1,
            false, null, _messages.Where(item => item.ChannelId == _selected).Select(item => (ulong?)item.Sequence).Min(),
            _messages.Count(item => item.ChannelId == _selected && item.ParentMessageId is null)),
        new(_thread, ThreadSummaries(), _reactions.ToArray(), ReactionsLoaded: true, ThreadSummariesLoaded: true));
    public WorkspaceSnapshot Workspace => new(true, false, false, false, null, null, _drafts, _outbox.ToArray());
    private void SyncNavigation()
    {
        _navigation.BindScope(Authentication.UserId, Authentication.SessionStamp);
        _navigation.UpdateChannels(Authentication.UserId!, Authentication.SessionStamp!, Channels);
        _threads.BindScope(Authentication.UserId, Authentication.SessionStamp);
    }
    public LiveSnapshot Capture() { SyncNavigation(); return new(Authentication, Chat, Workspace, true, null, Navigation: _navigation.Capture(), Threads: _threads.Capture()); }
    public Task LoginAsync() => Task.CompletedTask;
    public NativeMediaLibrary? Media => _mediaFixture.Library;
    internal void AddMedia()
    {
        _messages.Add(new(Guid.NewGuid().ToString(),_selected,User,"UI-prøve","Her er eit bilete\n[[media:"+NativeMediaViewFixture.Id+"|image/gif|probe.gif]]\n[[invite:"+new string('A',43)+"]]",DateTimeOffset.Now,2));
        Changed?.Invoke();
    }
    internal void RenameMedia() { _messages[^1]=_messages[^1] with {Body=_messages[^1].Body.Replace("probe.gif","renamed.gif",StringComparison.Ordinal)}; Changed?.Invoke(); }
    internal void AddGallery()
    {
        _messages.Clear();
        _outbox.Clear();
        for (int index=0;index<24;index++)
            _messages.Add(new(Guid.NewGuid().ToString(),_selected,User,"UI-prøve",
                "[[media:"+NativeMediaViewFixture.GalleryId(index)+"|image/png|gallery-"+index+".png]]",DateTimeOffset.Now,(ulong)index+1));
        Changed?.Invoke();
    }
    internal void AddRejectedMessage()
    {
        _outbox.Add(new("fixture-rejected", _selected, "Denne meldinga vart avvist", "rejected"));
        Changed?.Invoke();
    }
    public Task LogoutAsync() => Task.CompletedTask;
    public Task ResumeAsync() => Task.CompletedTask;
    public Task RetryPendingSendsAsync(string expectedUser, string expectedStamp) => Task.CompletedTask;
    public Task ConfirmPendingSendsAsync(string expectedUser, string expectedStamp) => Task.CompletedTask;
    public Task DiscardRejectedAsync(string expectedUser, string expectedStamp, string requestId)
    {
        if (Authentication.UserId == expectedUser && Authentication.SessionStamp == expectedStamp)
            _outbox.RemoveAll(item => item.RequestId == requestId && item.State == "rejected");
        Changed?.Invoke(); return Task.CompletedTask;
    }
    public Task SelectAsync(string channel) => NavigateAsync(new(channel));
    public Task NavigateAsync(NativeNavigationTarget target) { SyncNavigation(); return ApplyVisit(_navigation.Select(target)); }
    public Task BackAsync() => ApplyVisit(_navigation.Back());
    public Task ForwardAsync() => ApplyVisit(_navigation.Forward());
    private Task ApplyVisit(NativeNavigationVisit? visit)
    {
        if (visit is null) return Task.CompletedTask;
        _selected = visit.Target.ChannelId; _historyGeneration++;
        _thread = null;
        if (visit.Target.ThreadId is { } root)
        {
            var threadVisit = _threads.Open(_selected, root);
            EnsureComposer(_selected, root);
            _thread = BuildThread(root, ++_threadGeneration);
            if (threadVisit is not null) _threads.ApplyThread(threadVisit, _thread);
        }
        else _threads.Close();
        _navigation.Complete(visit); Changed?.Invoke(); return Task.CompletedTask;
    }
    public void SetNavigationQuery(string query) => _navigation.SetQuery(query);
    public void SetNavigationScope(NativeNavigationDirectoryScope scope) => _navigation.SetDirectoryScope(scope);
    public void SetNavigationInteraction(bool interacting) => _navigation.SetInteraction(interacting);
    public void RememberViewport(NativeNavigationVisit visit, NativeScrollAnchor anchor) => _navigation.SaveAnchor(visit, anchor);
    public Task RefreshConversationAsync(NativeMessageActionScope scope) { if (ScopeCurrent(scope)) Changed?.Invoke(); return Task.CompletedTask; }
    public Task OpenThreadDirectoryAsync(NativeMessageActionScope rootScope)
    {
        if (!ScopeCurrent(rootScope) || rootScope.ThreadRootId is not null) return Task.CompletedTask;
        var visit = _threads.OpenDirectory(_selected);
        if (visit is not null) _threads.ApplyDirectory(visit, ThreadSummaries(), _messages.Where(message => message.ChannelId == _selected && message.ParentMessageId is null).ToArray());
        return Task.CompletedTask;
    }
    public Task CloseThreadAsync(NativeThreadVisit expected)
    {
        if (!_threads.IsCurrent(expected) || _threads.Capture().Visit is not { } visit) return Task.CompletedTask;
        if (visit.IsDirectory)
        {
            _threads.Close(visit);
            if (_navigation.Capture().Current?.Target.ThreadId is { } existing) return NavigateAsync(new(_selected, ThreadId: existing));
            return Task.CompletedTask;
        }
        return NavigateAsync(new(_selected));
    }
    public Task RetryThreadAsync(NativeThreadVisit expected)
    {
        if (!_threads.IsCurrent(expected)) return Task.CompletedTask;
        if (expected.RootMessageId is { } root) return NavigateAsync(new(_selected, ThreadId: root));
        var visit = _threads.OpenDirectory(_selected);
        if (visit is not null) _threads.ApplyDirectory(visit, ThreadSummaries(), _messages.Where(message => message.ChannelId == _selected && message.ParentMessageId is null).ToArray());
        return Task.CompletedTask;
    }
    public void RememberThreadViewport(NativeThreadVisit visit, NativeScrollAnchor anchor) => _threads.SaveAnchor(visit, anchor);
    public Task MarkThreadVisibleReadAsync(NativeThreadVisit visit, long viewGeneration, long threadGeneration, IReadOnlyList<string> messageIds)
    {
        if (!Program.LiveWindowActive || !_threads.IsCurrent(visit) || _thread is not { } thread || thread.ViewGeneration != viewGeneration || thread.Generation != threadGeneration) return Task.CompletedTask;
        ulong highest = thread.Replies.Where(message => messageIds.Contains(message.Id)).Select(message => message.Sequence).DefaultIfEmpty(0UL).Max();
        if (highest > _threadLastRead.GetValueOrDefault(thread.RootMessageId))
        {
            _threadLastRead[thread.RootMessageId] = highest; ThreadReadReports.Add((thread.RootMessageId, highest)); RefreshThread(); Changed?.Invoke();
        }
        return Task.CompletedTask;
    }
    private IReadOnlyList<NativeThreadSummary> ThreadSummaries() => _messages.Where(message => message.ChannelId == _selected && message.ParentMessageId is not null)
        .GroupBy(message => message.ParentMessageId!).Select(group => new NativeThreadSummary(group.Key,
            (uint)group.Count(message => message.DeletedAt is null),
            (uint)group.Count(message => message.DeletedAt is null && message.AuthorId != User && message.Sequence > _threadLastRead.GetValueOrDefault(group.Key)),
            group.Max(message => message.Sequence))).ToArray();
    private NativeThreadSnapshot BuildThread(string root, long generation) => new(_selected, root, _historyGeneration, generation,
        _messages.FirstOrDefault(message => message.ChannelId == _selected && message.Id == root && message.ParentMessageId is null),
        _messages.Where(message => message.ChannelId == _selected && message.ParentMessageId == root).OrderBy(message => message.Sequence).ToArray(),
        LastReadSequence: _threadLastRead.GetValueOrDefault(root));
    private void RefreshThread()
    {
        if (_threads.Capture().Visit is { RootMessageId: string root } visit && _thread is { } previous)
        { _thread = BuildThread(root, previous.Generation); _threads.ApplyThread(visit, _thread); }
    }
    private bool ScopeCurrent(NativeMessageActionScope scope)
    {
        var visit = _navigation.Capture().Current; var thread = _threads.Capture().Visit;
        return Authentication.UserId == scope.UserId && Authentication.SessionStamp == scope.SessionStamp &&
            _selected == scope.ChannelId && visit?.Generation == scope.ChannelVisit &&
            (scope.ThreadRootId is null ? visit.Target.ThreadId is null && thread is null :
                visit.Target.ThreadId == scope.ThreadRootId && thread?.RootMessageId == scope.ThreadRootId && thread.Generation == scope.ThreadVisit);
    }
    public Task ExecuteMessageMutationAsync(NativeMessageMutation mutation)
    {
        if (!ScopeCurrent(mutation.Scope)) throw new OperationCanceledException("Fixture visit was replaced.");
        int index = _messages.FindIndex(message => message.Id == mutation.MessageId && message.ChannelId == mutation.Scope.ChannelId);
        if (index < 0) throw new InvalidOperationException("Fixture message missing.");
        var message = _messages[index]; MutationExecutions++;
        switch (mutation.Kind)
        {
            case NativeMessageMutationKind.Edit:
                if (message.AuthorId != User) throw new InvalidOperationException("Fixture permission denied.");
                _messages[index] = message with { Body = mutation.Value!, EditedAt = DateTimeOffset.UtcNow }; break;
            case NativeMessageMutationKind.Delete:
                if (message.AuthorId != User) throw new InvalidOperationException("Fixture permission denied.");
                _messages[index] = message with { Body = "", DeletedAt = DateTimeOffset.UtcNow };
                _reactions.RemoveAll(reaction => reaction.MessageId == message.Id); break;
            case NativeMessageMutationKind.ToggleReaction:
                var reaction = _reactions.FirstOrDefault(item => item.MessageId == message.Id && item.Emoji == mutation.Value);
                if (reaction is not null) _reactions.Remove(reaction);
                bool mine = reaction?.ReactedByMe != true;
                uint count = mine ? (reaction?.Count ?? 0) + 1 : reaction!.Count - 1;
                if (count > 0) _reactions.Add(new(message.Id, mutation.Value!, count, mine, mine ? [User] : []));
                break;
        }
        RefreshThread(); Changed?.Invoke();
        if (FailNextMutationAfterCommit) { FailNextMutationAfterCommit = false; throw new IOException("Fixture lost the mutation acknowledgement after commit."); }
        return Task.CompletedTask;
    }
    public Task<bool?> ReadBackMessageMutationAsync(NativeMessageMutation mutation)
    {
        if (!ScopeCurrent(mutation.Scope)) return Task.FromResult<bool?>(null);
        MutationReadBacks++;
        var message = _messages.FirstOrDefault(message => message.Id == mutation.MessageId);
        bool? result = message is null ? null : mutation.Kind switch
        {
            NativeMessageMutationKind.Edit => message.DeletedAt is null && message.Body == mutation.Value,
            NativeMessageMutationKind.Delete => message.DeletedAt is not null,
            _ => _reactions.Any(reaction => reaction.MessageId == message.Id && reaction.Emoji == mutation.Value && reaction.ReactedByMe) == mutation.ExpectedMine
        };
        return Task.FromResult(result);
    }
    private string EnsureComposer(string channel, string? parent)
    {
        string key = new ComposerTarget(channel, parent).Key;
        if (!_drafts.ContainsKey(key)) _drafts[key] = new("", 0, 0);
        return key;
    }
    public void EditComposer(NativeMessageActionScope scope, string body)
    {
        if (!ScopeCurrent(scope)) return;
        Edit(EnsureComposer(scope.ChannelId, scope.ThreadRootId), body);
    }
    public Task SendComposerAsync(NativeMessageActionScope scope)
    {
        if (!ScopeCurrent(scope)) return Task.CompletedTask;
        string key = EnsureComposer(scope.ChannelId, scope.ThreadRootId), body = _drafts[key].Body;
        if (string.IsNullOrWhiteSpace(body)) return Task.CompletedTask;
        string id = Guid.NewGuid().ToString();
        LastEnqueuedReply = new(id, scope.ChannelId, body, "queued", scope.ThreadRootId);
        _messages.Add(new(id, scope.ChannelId, User, "UI-prøve", body, DateTimeOffset.UtcNow,
            _messages.Where(message => message.ChannelId == scope.ChannelId).Select(message => message.Sequence).DefaultIfEmpty(0UL).Max() + 1, scope.ThreadRootId));
        Edit(key, ""); RefreshThread(); Changed?.Invoke(); return Task.CompletedTask;
    }
    internal void PrepareConversations()
    {
        _historyMode = false; _messages.Clear(); _outbox.Clear(); _reactions.Clear(); _lastRead.Clear(); _readReports.Clear();
        _threadLastRead.Clear(); ThreadReadReports.Clear(); MutationExecutions = MutationReadBacks = 0; LastEnqueuedReply = null;
        _thread = null; _threads.Close(); _drafts.Clear();
        foreach (string channel in new[] { "channel-a", "channel-b", "channel-dm" }) _drafts[channel] = new("", 0, 0);
        _selected = "channel-a"; _historyGeneration++;
        _messages.Add(new("conversation-root-one", _selected, User, "Du", "Opphavleg melding ein", DateTimeOffset.UtcNow, 1));
        _messages.Add(new("conversation-root-two", _selected, User, "Du", "Opphavleg melding to", DateTimeOffset.UtcNow, 2));
        _messages.Add(new("conversation-reply-one", _selected, User, "Du", "Eit tidlegare svar", DateTimeOffset.UtcNow, 3, "conversation-root-one"));
        _messages.Add(new("conversation-reply-two", _selected, User, "Du", "Svar i andre tråden", DateTimeOffset.UtcNow, 4, "conversation-root-two"));
        _messages.Add(new("conversation-reply-other", _selected, "other-user", "Testven", "Eit svar frå ein annan", DateTimeOffset.UtcNow, 5, "conversation-root-one"));
        SyncNavigation(); var visit = _navigation.Select(new(_selected)); if (visit is not null) _navigation.Complete(visit);
        Changed?.Invoke();
    }
    public async Task LoadOlderAsync(string channel, long viewGeneration, string expectedUser, string expectedStamp, NativeNavigationVisit visit)
    {
        if (!_historyMode || channel != "channel-a" || viewGeneration != _historyGeneration || !_navigation.IsCurrent(visit) ||
            Authentication.UserId != expectedUser || Authentication.SessionStamp != expectedStamp || _loadingOlder || _historyStart <= 1) return;
        _loadingOlder = true; Changed?.Invoke();
        await Task.Delay(60);
        if (_navigation.IsCurrent(visit) && viewGeneration == _historyGeneration)
        {
            ulong first = _historyStart > 50 ? _historyStart - 50 : 1;
            for (ulong sequence = first; sequence < _historyStart; sequence++) _messages.Add(HistoryMessage(channel, sequence));
            _historyStart = first; OlderCalls++;
        }
        _loadingOlder = false; Changed?.Invoke();
    }
    public Task MarkVisibleReadAsync(string channel, long viewGeneration, IReadOnlyList<string> messageIds, string expectedUser, string expectedStamp, NativeNavigationVisit visit)
    {
        if (!Program.LiveWindowActive || !_navigation.IsCurrent(visit) || _selected != channel || _historyGeneration != viewGeneration ||
            Authentication.UserId != expectedUser || Authentication.SessionStamp != expectedStamp) return Task.CompletedTask;
        ulong highest = _messages.Where(item => item.ChannelId == channel && messageIds.Contains(item.Id)).Select(item => item.Sequence).DefaultIfEmpty(0UL).Max();
        if (highest > _lastRead.GetValueOrDefault(channel))
        {
            _lastRead[channel] = highest; _readReports.Add(highest); Changed?.Invoke();
        }
        return Task.CompletedTask;
    }
    internal void PrepareHistory()
    {
        _historyMode = true; _messages.Clear(); _outbox.Clear(); _lastRead.Clear(); _readReports.Clear(); OlderCalls = 0;
        _historyStart = 301; _historyGeneration++;
        for (ulong sequence = 301; sequence <= 350; sequence++) _messages.Add(HistoryMessage("channel-a", sequence));
        for (ulong sequence = 1; sequence <= 80; sequence++) _messages.Add(HistoryMessage("channel-b", sequence));
        SyncNavigation();
        _selected = "channel-a";
        var visit = _navigation.Select(new("channel-a"));
        if (visit is not null) _navigation.Complete(visit);
        Changed?.Invoke();
    }
    private static NativeMessage HistoryMessage(string channel, ulong sequence)
    {
        string body = sequence % 7 == 0
            ? "Bilete i historikken\n[[media:" + NativeMediaViewFixture.GalleryId((int)(sequence % 24)) + "|image/png|history.png]]"
            : string.Join("\n", Enumerable.Repeat("Historisk melding " + sequence, 1 + (int)(sequence % 5)));
        return new("history-" + channel + "-" + sequence, channel, User, "UI-prøve", body, DateTimeOffset.UtcNow.AddMinutes(-(long)(350 - Math.Min(350UL, sequence))), sequence);
    }
    internal void ExpandMessage(string id)
    {
        int index = _messages.FindIndex(item => item.Id == id);
        if (index >= 0) _messages[index] = _messages[index] with { Body = _messages[index].Body + "\n" + string.Join("\n", Enumerable.Repeat("Meir innhald før leseposisjonen", 40)) };
        Changed?.Invoke();
    }
    public void Edit(string channel, string body) { var old = _drafts[channel]; _drafts[channel] = new(body, old.Revision+1, old.Revision+1); Changed?.Invoke(); }
    public Task SendAsync(string channel)
    {
        string body = _drafts[channel].Body;
        _messages.Add(new(Guid.NewGuid().ToString(), channel, User, "UI-prøve", body, DateTimeOffset.Now, (ulong)_messages.Count+1));
        Edit(channel, ""); return Task.CompletedTask;
    }
    public ValueTask DisposeAsync() { _mediaFixture.Dispose(); return ValueTask.CompletedTask; }
}

internal static class LiveUiProbe
{
    private static int _started;
    internal static void Attach(FrameworkElement root)
    {
        if (Program.LiveUiProbeDirectory is null) return;
        root.Loaded += (_, _) => { if (Interlocked.Exchange(ref _started,1)==0) root.DispatcherQueue.TryEnqueue(() => Run(root)); };
        if (root.IsLoaded && Interlocked.Exchange(ref _started,1)==0) root.DispatcherQueue.TryEnqueue(() => Run(root));
    }
    private static async void Run(FrameworkElement root)
    {
        string folder = Program.LiveUiProbeDirectory!;
        var window = ReactorApp.PrimaryWindow!;
        window.Closed += (_,_) => File.WriteAllText(Path.Combine(folder,"live-ui-closed.txt"),"closed");
        try
        {
            await Until(() => root.ActualWidth > 500);
            var input = Find<TextBox>(root,"live-composer"); input.Text = "Eit utkast i første kanal";
            await Until(() => Program.Live!.Workspace!.Drafts["channel-a"].Body == input.Text);
            var search = Find<TextBox>(root, "live-navigation-search");
            search.Text = "Idear";
            await Until(() => Program.Live!.Capture().Navigation is { Query: "Idear", Channels.Count: 3 } filtered &&
                filtered.Groups.SelectMany(group => group.Channels).Select(item => item.Id).SequenceEqual(["channel-b"]));
            if (Program.Live!.Chat.SelectedChannel != "channel-a" || Program.Live.Workspace!.Drafts["channel-a"].Body != "Eit utkast i første kanal")
                throw new InvalidOperationException("Navigation search changed the selected conversation or its draft.");
            _ = Find<Button>(root, "live-channel-channel-b");
            search.Text = "";
            Program.Live.SetNavigationScope(NativeNavigationDirectoryScope.Direct);
            await Until(() => Program.Live.Capture().Navigation?.DirectoryScope == NativeNavigationDirectoryScope.Direct &&
                Find<Button>(root, "live-channel-channel-dm").IsLoaded);
            Program.Live.SetNavigationScope(NativeNavigationDirectoryScope.Circles);
            await Until(() => Find<Button>(root, "live-channel-channel-a").IsLoaded);
            await Program.Live!.SelectAsync("channel-b");
            await Until(() => Find<TextBox>(root,"live-composer").Text == "");
            Find<TextBox>(root,"live-composer").Text = "Eit anna utkast";
            await Until(() => Program.Live.Workspace!.Drafts["channel-b"].Body == "Eit anna utkast");
            await Program.Live.SelectAsync("channel-a");
            await Until(() => Find<TextBox>(root,"live-composer").Text == "Eit utkast i første kanal");
            await LiveChatComponent.SendForProbe!();
            await Until(() => Find<TextBox>(root,"live-composer").Text == "" && Program.Live.Chat.Messages.Count == 1);
            ((LiveUiFixture)Program.Live).AddMedia();
            ((LiveUiFixture)Program.Live).AddRejectedMessage();
            await Until(() => Find<TextBlock>(root,"live-delivery-pending:fixture-rejected").Text == "Ikkje sendt — avvist");
            if (Program.Live.Capture().PendingSends != 0 || Program.Live.Capture().RejectedSends != 1 ||
                Find<Button>(root,"live-delivery-retry").Visibility == Visibility.Visible)
                throw new InvalidOperationException("A rejected send was presented as waiting for retry.");
            var discard = LiveChatComponent.DiscardForProbe!("fixture-rejected");
            await Until(() => LiveChatComponent.DiscardDialogForProbe?.IsLoaded == true);
            LiveChatComponent.DiscardDialogForProbe!.Hide();
            await discard;
            if (Program.Live.Capture().RejectedSends != 1) throw new InvalidOperationException("Cancelling discard removed the rejected text.");
            await Program.Live.DiscardRejectedAsync(Program.Live.Authentication.UserId!, Program.Live.Authentication.SessionStamp!, "fixture-rejected");
            await Until(() => Program.Live.Capture().RejectedSends == 0);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.Id).Source is not null);
            if(NativeMessageView.ScopeFor(Program.Live.Media,Program.Live.Authentication with { UserId="ffffffff-ffff-ffff-ffff-ffffffffffff" }) is not null ||
                NativeMessageView.ScopeFor(Program.Live.Media,Program.Live.Authentication with { SessionStamp="999:another-family:0" }) is not null)
                throw new InvalidOperationException("Media presentation accepted the wrong viewer snapshot.");
            ((LiveUiFixture)Program.Live).RenameMedia();
            await Until(() => Find<TextBlock>(root,"native-media-name-"+NativeMediaViewFixture.Id).Text=="renamed.gif" && Find<Image>(root,"native-image-"+NativeMediaViewFixture.Id).Source is not null);
            if(Find<TextBlock>(root,"live-group-circle:fixture-circle").Text!="UI-PRØVE") throw new InvalidOperationException("Circle heading missing.");
            _=Find<StackPanel>(root,"native-content-invite");
            var held = new List<NativePreviewLease>();
            try
            {
                for(int index=0;index<7;index++) { var lease=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None); lease.Decoded(); held.Add(lease); }
                bool bounded=false; try { using var extra=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None); } catch(NativePreviewCapacityException) { bounded=true; }
                if(!bounded)throw new InvalidOperationException("Preview bitmap admission is unbounded.");
                held[0].Dispose();
                using var replacement=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            }
            finally { foreach(var lease in held)lease.Dispose(); }
            var decodingA=await Program.Live.Media!.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            var decodingB=await Program.Live.Media.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
            try
            {
                if(!decodingA.BeginDecode() || !decodingB.BeginDecode())throw new InvalidOperationException("Decode fixture admission failed.");
                decodingA.Dispose(); decodingB.Dispose();
                var queued=Program.Live.Media.PreviewAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),CancellationToken.None);
                await Task.Delay(50);
                if(queued.IsCompleted)throw new InvalidOperationException("Unmount admitted too many native decoders.");
                decodingA.Decoded();
                using var admitted=await queued.WaitAsync(TimeSpan.FromSeconds(2));
            }
            finally { decodingA.Decoded(); decodingB.Decoded(); decodingA.Dispose(); decodingB.Dispose(); }
            string temporary;
            using(var original=await Program.Live.Media!.OriginalAsync(NativeMediaViewFixture.Id,Program.Live.Media.Capture(),null,CancellationToken.None))
            { temporary=original.Path; if(!File.Exists(temporary) || original.ContentType!="image/gif")throw new InvalidOperationException("Original fixture failed."); }
            if(File.Exists(temporary))throw new InvalidOperationException("Media file lease was not cleaned.");
            // Realized rows can outlive their viewport. More than eight attachments
            // must not let cached rows starve the visible images of bitmap slots.
            ((LiveUiFixture)Program.Live).AddGallery();
            var history=Find<ScrollViewer>(root,"live-history");
            await Until(() =>
            {
                root.UpdateLayout(); history.ChangeView(null,history.ScrollableHeight,null,true);
                return Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)).Source is not null;
            });
            root.UpdateLayout();
            var lastPreview=Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23));
            CheckImage(lastPreview,.5,320);
            double height=lastPreview.ActualHeight;
            history.ChangeView(null,0,null,true);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(0)).Source is not null);
            root.UpdateLayout();
            CheckImage(Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(0)),2,320);
            await Until(() => PreviewReleased(root,NativeMediaViewFixture.GalleryId(23)));
            root.UpdateLayout();
            // ItemsRepeater may refine its total extent as different rows are
            // realized. Measure the retained image control, not that estimate.
            if(lastPreview.IsLoaded && Math.Abs(lastPreview.ActualHeight-height)>2)
                throw new InvalidOperationException("Releasing a retained preview changed its reserved height.");
            // Rapidly leave/re-enter while admission/decoding can still be pending.
            history.ChangeView(null,history.ScrollableHeight,null,true); await Task.Delay(20);
            history.ChangeView(null,0,null,true); await Task.Delay(20);
            history.ChangeView(null,history.ScrollableHeight,null,true);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)).Source is not null);
            await Until(() => PreviewReleased(root,NativeMediaViewFixture.GalleryId(0)));
            // The original viewer uses the same aspect-preserving decode path,
            // with a larger pixel budget, including for portrait photographs.
            foreach(int index in new[]{0,23})
            {
                using var original=await Program.Live.Media!.OriginalAsync(NativeMediaViewFixture.GalleryId(index),Program.Live.Media.Capture(),null,CancellationToken.None);
                var file=await StorageFile.GetFileFromPathAsync(original.Path); using var stream=await file.OpenReadAsync();
                var decoded=await NativeMessageView.DecodeBitmapAsync(stream,1600,CancellationToken.None);
                double ratio=index==0 ? 2 : .5;
                if(Math.Abs(decoded.Bitmap.DecodePixelWidth/(double)decoded.Bitmap.DecodePixelHeight-ratio)>.01 ||
                    decoded.Bitmap.DecodePixelWidth>1600 || decoded.Bitmap.DecodePixelHeight>1600)
                    throw new InvalidOperationException("Original decode changed image proportions or exceeded its pixel budget.");
            }
            window.SetSize(360,720);
            await Until(() => root.ActualWidth < 800 && Find<TextBox>(root,"live-composer").ActualWidth > 200);
            await Until(() => Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)).Source is not null);
            root.UpdateLayout();
            CheckImage(Find<Image>(root,"native-image-"+NativeMediaViewFixture.GalleryId(23)),.5,Math.Min(320,root.ActualWidth-32));
            await LiveTimelineUiProbe.RunAsync(root, (LiveUiFixture)Program.Live!);
            await LiveConversationUiProbe.RunAsync(root, (LiveUiFixture)Program.Live!);
            await Program.PrepareShutdownAsync();
            File.WriteAllText(Path.Combine(folder,"live-ui-result.json"),JsonSerializer.Serialize(new
            {passed=true,scope="live_chat_view_fixture",actualWinUi=true,channelDrafts=true,sendBinding=true,compactComposer=true,circleGroups=true,decodedMediaPreview=true,boundedPreviewLeases=true,viewportPreviewGallery=24,viewportRelease=true,stablePreviewGeometry=true,rapidPreviewReentry=true,portraitAndLandscape=true,originalAspectRatio=true,responsivePreviewWidth=true,macroCard=true,temporaryOriginalCleanup=true,
                timelinePaging=true,stableMessageAnchor=true,visibleReadOnly=true,navigationSearch=true,navigationDirectoryScopes=true,navigationBackForward=true,independentNavigationDrafts=true,variableHeightAnchor=true,
                threadOpenClose=true,threadDirectory=true,independentThreadDrafts=true,parentBoundReply=true,threadVisibleReadOnly=true,messageEdit=true,messageDeleteConfirmation=true,deletedThreadParent=true,reactionToggle=true,uncertainReactionReadback=true,
                productionAuthentication=false,actualTransport=false,storageAcceptance=false}));
            Program.CloseAfterShutdown(window);
        }
        catch (Exception error)
        {
            File.WriteAllText(Path.Combine(folder,"live-ui-failure.txt"), error.GetType().Name + ": " + error.Message);
            try { await Program.PrepareShutdownAsync(); } catch { }
            Environment.ExitCode=1; Program.CloseAfterShutdown(window);
        }
    }
    private static async Task Until(Func<bool> condition)
    {
        for(int attempt=0;attempt<400;attempt++)
        {
            try { if(condition())return; } catch(KeyNotFoundException) { /* The next native render may create the control. */ }
            await Task.Delay(25);
        }
        throw new InvalidOperationException("Live view fixture did not settle.");
    }
    private static T Find<T>(DependencyObject root,string id) where T:FrameworkElement
    {
        if(root is T found && AutomationProperties.GetAutomationId(found)==id)return found;
        for(int index=0;index<VisualTreeHelper.GetChildrenCount(root);index++)
        { try{return Find<T>(VisualTreeHelper.GetChild(root,index),id);}catch(KeyNotFoundException){} }
        throw new KeyNotFoundException(id);
    }
    private static bool PreviewReleased(DependencyObject root,string id)
    {
        try { return Find<Image>(root,"native-image-"+id).Source is null; }
        catch(KeyNotFoundException) { return true; } // Fully recycled is also released.
    }
    private static void CheckImage(Image image,double ratio,double maximumWidth)
    {
        if(image.Source is not BitmapImage bitmap || Math.Abs(bitmap.DecodePixelWidth/(double)bitmap.DecodePixelHeight-ratio)>.01 ||
            bitmap.DecodePixelWidth>720 || bitmap.DecodePixelHeight>720 || Math.Abs(image.ActualWidth/image.ActualHeight-ratio)>.01 ||
            image.ActualWidth>maximumWidth+1 || image.ActualWidth<100)
            throw new InvalidOperationException($"Preview proportions/bounds failed: pixels {bitmapSize(image.Source)}, view {image.ActualWidth}x{image.ActualHeight}, ratio {ratio}, max {maximumWidth}.");
        static string bitmapSize(ImageSource? source)=>source is BitmapImage value ? $"{value.PixelWidth}x{value.PixelHeight}" : "none";
    }
}
