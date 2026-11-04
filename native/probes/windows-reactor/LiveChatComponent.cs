using Microsoft.UI.Input;
using Microsoft.UI.Reactor;
using Microsoft.UI.Reactor.Core;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using System.Text;
using Windows.System;
using Windows.UI.Core;
using static Microsoft.UI.Reactor.Factories;

namespace Sproyt.WindowsProbe;

// Uses real server projections. No demo channels or manufactured chat history.
internal sealed partial class LiveChatComponent : Component
{
    internal static Func<Task>? SendForProbe;
    internal static Func<string, Task>? DiscardForProbe;
    internal static ContentDialog? DiscardDialogForProbe;
    internal static NativeTimelineViewport? TimelineForProbe;
    internal static ScrollViewer? ScrollForProbe;
    internal static Func<Task>? LoadOlderForProbe;
    internal static Func<NativeNavigationTarget, Task>? NavigateForProbe;
    internal static Func<Task>? BackForProbe, ForwardForProbe;
    internal static NativeTimelineViewport? ThreadTimelineForProbe;
    internal static ScrollViewer? ThreadScrollForProbe;
    internal static Func<Task>? SendThreadForProbe;
    internal static Action? WindowActivityChanged;
    internal static NativeFileDrop? RootDropForProbe, ThreadDropForProbe;
    private ContentDialog? _discardDialog;
    private NativeTimelineViewport? _timeline;
    private NativeTimelineViewport? _threadTimeline;
    private NativeTimelineContext? _threadTimelineContext;
    private NativeScrollAnchor? _threadRequestedAnchor;
    private long _lastThreadVisit = -1, _lastThreadGeneration = -1;
    private bool _threadSurfaceVisible;
    private readonly NativeMessageActionController _rootActions = new(), _threadActions = new();
    private readonly NativeRawMessageState _rawMessages = new();
    private bool _uploadChoosing;
    private readonly Dictionary<NativeUploadTarget, string> _uploadUiErrors = [];
    private NativeFileDropContext? _rootDropContext, _threadDropContext;
    private IDisposable? _rootDropBinding, _threadDropBinding;
    private readonly Dictionary<FrameworkElement, IDisposable> _dropBindings = [];
    private NativeTimelineContext? _timelineContext;
    private NativeScrollAnchor? _requestedAnchor;
    private bool _surfaceVisible, _previousSurfaceVisible;
    private long _lastVisitGeneration = -1, _lastViewGeneration = -1;
    private int _revision;

    public override Element Render()
    {
        var app = Program.Live;
        var (_, invalidate) = UseState(0, threadSafe: true);
        PilotComponent.InvalidateForClose = () => invalidate(Interlocked.Increment(ref _revision));
        var (dark, setDark) = UseState(Application.Current.RequestedTheme == ApplicationTheme.Dark);
        var (showList, setShowList) = UseState(false);
        var (showUploads, setShowUploads) = UseState(false);
        bool wide = UseBreakpoint(800);
        UseEffect(() =>
        {
            void Changed() => invalidate(Interlocked.Increment(ref _revision));
            Program.LiveStateChanged += Changed;
            if (app is not null) app.Changed += Changed;
            else _ = Program.OpenLiveAsync();
            _rootActions.Changed += Changed; _threadActions.Changed += Changed;
            _rawMessages.Changed += Changed;
            return () => { Program.LiveStateChanged -= Changed; if (app is not null) app.Changed -= Changed;
                _rootActions.Changed -= Changed; _threadActions.Changed -= Changed;
                _rawMessages.Changed -= Changed;
                _timeline?.Dispose(); _threadTimeline?.Dispose(); _discardDialog?.Hide(); WindowActivityChanged = null;
                _rootDropContext = _threadDropContext = null;
                foreach (var binding in _dropBindings.Values) binding.Dispose();
                _dropBindings.Clear(); _rootDropBinding = _threadDropBinding = null;
                RootDropForProbe = ThreadDropForProbe = null;
                if (_personalSurface.Scope is { } personal) _personalApp?.UncoverConversation(personal);
                _personalSurface.Close(); RetirePersonalControllers();
                OpenPersonalForProbe = null; ClosePersonalForProbe = null; PersonalScopeForProbe = null; };
        }, app is null ? "starting" : "ready");
        if (app is null)
        {
            var startup = dark ? SproytPalette.Dark : SproytPalette.Light;
            async void TryOpen() => await Program.OpenLiveAsync();
            return Grid([GridSize.Star()], [GridSize.Star()], VStack(16,
                TextBlock("Sprøyt").FontSize(32).Bold().Foreground(startup.Ink),
                TextBlock(Program.LiveError ?? "Koplar til Canary …").TextWrapping().Foreground(startup.Muted),
                Button("Prøv igjen", TryOpen).MinHeight(44).IsVisible(Program.LiveError is not null)
            ).Padding(32).VAlign(VerticalAlignment.Center)).Background(startup.Paper)
                .OnMount(element => PilotComponent.RootForClose = element).AutomationId("live-startup");
        }
        var state = app.Capture(); var auth = state.Authentication; var chat = state.Chat;
        SynchronizePersonal(app, state, () => invalidate(Interlocked.Increment(ref _revision)));
        var navigation = state.Navigation;
        var threads = state.Threads;
        bool threadDetail = threads?.DetailOpen == true, threadDirectory = threads?.DirectoryOpen == true;
        bool featureOpen = threadDetail || threadDirectory;
        var palette = dark ? SproytPalette.Dark : SproytPalette.Light;
        string? channel = state.ShowConversation ? navigation?.Current?.Target.ChannelId ?? chat.SelectedChannel : null;
        var selected = chat.Channels.FirstOrDefault(item => item.Id == channel);
        var rows = state.ShowConversation && chat.SelectedChannel == channel ? chat.Messages.Concat((state.Workspace?.Outbox ?? [])
            .Where(item => item.Channel == channel && item.ParentMessageId is null).Select(item => new NativeMessage("pending:" + item.RequestId,
                item.Channel, auth.UserId ?? "", auth.DisplayName ?? "Du", item.Body, DateTimeOffset.MinValue, ulong.MaxValue))).ToArray() : [];
        var draft = channel is not null && state.Workspace?.Drafts.TryGetValue(channel, out var current) == true ? current : null;
        NativeMessageActionScope? RootScope() => auth.UserId is string user && auth.SessionStamp is string stamp && channel is not null && navigation?.Current is { } visit
            ? new(user, stamp, channel, visit.Generation) : null;
        var rootScope = !featureOpen ? RootScope() : null;
        NativeMessageActionScope? threadScope = threadDetail && RootScope() is { } currentScope && threads?.Visit is { RootMessageId: { } rootId } threadVisit
            ? currentScope with { ThreadRootId = rootId, ThreadVisit = threadVisit.Generation } : null;
        var threadRows = threads?.Thread is { } loadedThread
            ? (loadedThread.Root is { } parent ? new[] { parent } : Array.Empty<NativeMessage>()).Concat(loadedThread.Replies).ToArray() : [];
        _rootActions.Bind(rootScope); if (rootScope is not null) _rootActions.Observe(rootScope, rows);
        _threadActions.Bind(threadScope); if (threadScope is not null) _threadActions.Observe(threadScope, threadRows);
        _rootActions.SetCovered(PersonalOpen); _threadActions.SetCovered(PersonalOpen);
        var messageCommands = new NativeMessageActionCommands(app.ExecuteMessageMutationAsync, app.ReadBackMessageMutationAsync);
        string? family = auth.SessionStamp is { } currentStamp && currentStamp.LastIndexOf(':') > 0 ? currentStamp[..currentStamp.LastIndexOf(':')] : null;
        _rawMessages.Bind(auth.UserId, family);
        foreach (var retired in _uploadUiErrors.Keys.Where(target => target.UserId != auth.UserId || target.FamilyStamp != family).ToArray())
            _uploadUiErrors.Remove(retired);
        void ReportUploadIssue(NativeUploadTarget target, string? error)
        {
            var latest = app.Authentication;
            string? currentFamily = latest.SessionStamp is { } stamp && stamp.LastIndexOf(':') > 0 ? stamp[..stamp.LastIndexOf(':')] : null;
            if (!ReferenceEquals(Program.Live, app) || latest.UserId != target.UserId || currentFamily != target.FamilyStamp) return;
            if (error is null) _uploadUiErrors.Remove(target);
            else
            {
                if (!_uploadUiErrors.ContainsKey(target) && _uploadUiErrors.Count >= 8) _uploadUiErrors.Remove(_uploadUiErrors.Keys.First());
                _uploadUiErrors[target] = error;
            }
            if (error is null) setShowUploads(true);
            invalidate(Interlocked.Increment(ref _revision));
        }
        bool DropEnabled(NativeMessageActionScope? scope) => !PersonalOpen && scope is not null && state.Uploads is not null && chat.Connected &&
            auth.Phase == NativeAuthPhase.Active && !Program.CloseAdmissionClosed && !_uploadChoosing && navigation is { Pending: false, Error: null };
        _rootDropContext = DropEnabled(rootScope) ? new(app, rootScope!, ReportUploadIssue) : null;
        _threadDropContext = DropEnabled(threadScope) ? new(app, threadScope!, ReportUploadIssue) : null;
        bool UploadBlocked(string targetChannel, string? parent) => NativeUploadPresentation.BlocksSend(state.Uploads, auth.UserId, family, targetChannel, parent);
        bool canSend = !PersonalOpen && !state.ConfirmPendingSends && navigation is { Pending: false, Error: null } && chat.SelectedChannel == channel && draft is not null && !string.IsNullOrWhiteSpace(draft.Body) && Encoding.UTF8.GetByteCount(draft.Body) <= 1024 &&
            NativeRichText.AttachmentSuffixVisible(draft.Body, draft.Attachments) && channel is not null && !UploadBlocked(channel, null) &&
            !Program.CloseAdmissionClosed && state.Workspace is { Ready: true, Closing: false, Sending: false, NeedsRetry: false } &&
            state.Workspace.Outbox.Count < 8 && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline;

        async void Login() { if (!Program.CloseAdmissionClosed) await app.LoginAsync(); }
        async void Logout() { if (!Program.CloseAdmissionClosed) await app.LogoutAsync(); }
        async void Retry() { if (!Program.CloseAdmissionClosed) await app.ResumeAsync(); }
        async Task ChooseUpload(NativeMessageActionScope captured, bool paste)
        {
            if (PersonalOpen || _uploadChoosing || Program.CloseAdmissionClosed) return;
            _uploadChoosing = true;
            invalidate(Interlocked.Increment(ref _revision));
            try
            {
                setShowUploads(true);
                if (paste) await NativeUploadShell.PasteAsync(app, captured); else await NativeUploadShell.PickAsync(app, captured);
            }
            catch
            {
                var latest = app.Authentication;
                if (latest.UserId == captured.UserId && latest.SessionStamp is { } stamp && stamp.LastIndexOf(':') > 0 &&
                    captured.SessionStamp.LastIndexOf(':') > 0 && stamp[..stamp.LastIndexOf(':')] == captured.SessionStamp[..captured.SessionStamp.LastIndexOf(':')])
                    ReportUploadIssue(new(captured.UserId, stamp[..stamp.LastIndexOf(':')], captured.ChannelId, captured.ThreadRootId),
                        "Fila eller biletet kunne ikkje hentast. Prøv Legg ved på nytt.");
            }
            finally { _uploadChoosing = false; invalidate(Interlocked.Increment(ref _revision)); }
        }
        Element UploadTools(NativeMessageActionScope? scope, DraftView? composerDraft, bool thread)
        {
            var target = scope is not null && family is not null ? new NativeUploadTarget(scope.UserId, family, scope.ChannelId, scope.ThreadRootId) : null;
            string prefix = thread ? "live-thread-" : "live-";
            bool enabled = scope is not null && state.Uploads is not null && chat.Connected && auth.Phase == NativeAuthPhase.Active &&
                !Program.CloseAdmissionClosed && !_uploadChoosing && navigation is { Pending: false, Error: null };
            var chips = (composerDraft?.Attachments ?? []).Select((marker, index) =>
            {
                var attachment = NativeUploadPresentation.Attachment(marker);
                return Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto],
                    TextBlock(attachment.Text).TextWrapping().Foreground(palette.Ink).VAlign(VerticalAlignment.Center).Grid(column: 0),
                    Button("Fjern", () => { if (scope is not null) app.RemoveDraftAttachment(scope, index, marker); }).MinHeight(36)
                        .AutomationName("Fjern vedlegget " + attachment.Text).AutomationId(prefix + "draft-attachment-remove-" + index).Grid(column: 1))
                    .AutomationId(prefix + "draft-attachment-" + index);
            }).ToArray();
            string? error = state.UploadError is not null && (state.UploadErrorTarget is null || state.UploadErrorTarget == target)
                ? state.UploadError : target is not null ? _uploadUiErrors.GetValueOrDefault(target) : null;
            return VStack(4,
                HStack(8,
                    Button(_uploadChoosing ? "Klargjer …" : "Legg ved", async () => { if (scope is not null) await ChooseUpload(scope, false); })
                        .MinHeight(36).AutomationName("Vel bilete, video eller vedlegg").AutomationId(prefix + "attach-files").IsEnabled(enabled),
                    Button("Lim inn", async () => { if (scope is not null) await ChooseUpload(scope, true); })
                        .MinHeight(36).AutomationName("Lim inn bilete eller filer").AutomationId(prefix + "paste-files").IsEnabled(enabled),
                    Button("Opplastingar" + (state.Uploads?.Jobs.Count > 0 ? " (" + state.Uploads.Jobs.Count + ")" : ""), () => setShowUploads(!showUploads))
                        .MinHeight(36).AutomationName("Vis opplastingskøen").AutomationId(prefix + "uploads-toggle")
                        .IsVisible(state.Uploads?.Jobs.Count > 0 || state.Uploads?.StorageError is not null)),
                ScrollViewer(VStack(2, chips)).MaxHeight(96).IsVisible(chips.Length > 0),
                TextBlock(error ?? "").TextWrapping().Foreground(palette.Error).IsVisible(error is not null).AutomationId(prefix + "upload-error"),
                Button("Prøv opplasting på nytt", Retry).MinHeight(36).IsVisible(state.Uploads is null && state.UploadError is not null)
                    .AutomationName("Opne opplastingslageret på nytt"),
                TextBlock("Avslutt kodeblokka eller HTML-teksten før du sender vedlegg.").TextWrapping().Foreground(palette.Error)
                    .IsVisible(composerDraft is not null && !NativeRichText.AttachmentSuffixVisible(composerDraft.Body, composerDraft.Attachments)),
                state.Uploads is { } uploads ? ScrollViewer(NativeUploadView.Render(uploads, target, palette,
                    new(id => { if (auth.UserId is string user && auth.SessionStamp is string stamp) _ = app.RetryUploadAsync(user, stamp, id); },
                        id => { if (auth.UserId is string user && auth.SessionStamp is string stamp) _ = app.CancelUploadAsync(user, stamp, id); },
                        id => { if (auth.UserId is string user && auth.SessionStamp is string stamp) _ = app.RemoveUploadAsync(user, stamp, id); })))
                    .MaxHeight(120).IsVisible(showUploads && (uploads.Jobs.Count > 0 || uploads.StorageError is not null)) : Empty());
        }
        Element DropHost(Element element, bool thread) => element.AutomationId(thread ? "live-thread-file-drop" : "live-file-drop")
            .OnMount(host =>
            {
                var binding = NativeFileDrop.Attach(host, () => thread ? _threadDropContext : _rootDropContext);
                _dropBindings[host] = binding;
                if (thread) { _threadDropBinding = binding;
                    if (Program.LiveUiProbeDirectory is not null) ThreadDropForProbe = binding as NativeFileDrop; }
                else { _rootDropBinding = binding;
                    if (Program.LiveUiProbeDirectory is not null) RootDropForProbe = binding as NativeFileDrop; }
            })
            .OnUnmount(host =>
            {
                if (!_dropBindings.Remove(host, out var binding)) return;
                binding.Dispose();
                if (thread && ReferenceEquals(_threadDropBinding, binding)) { _threadDropBinding = null; ThreadDropForProbe = null; }
                if (!thread && ReferenceEquals(_rootDropBinding, binding)) { _rootDropBinding = null; RootDropForProbe = null; }
            });
        void RememberViewport()
        {
            var current = app.Capture().Navigation?.Current;
            if (threads?.Visit is { IsDirectory: false } thread && _threadTimelineContext is { } threadContext &&
                current?.Generation == threadContext.Visit.Generation && thread.Generation == threadContext.ThreadGeneration)
            { app.RememberThreadViewport(thread, _threadTimeline?.Capture() ?? threads.Anchor); return; }
            if (current is not null && _timelineContext is { } context && current.UserId == context.UserId &&
                current.Target.ChannelId == context.ChannelId && current.Generation == context.Visit.Generation)
                app.RememberViewport(current, _timeline?.Capture() ?? current.Anchor);
        }
        async Task Navigate(NativeNavigationTarget target)
        {
            if (!CurrentNavigationRender()) return;
            _personalReturn = null; _personalNavigationSerial++;
            RememberViewport(); setShowList(false); await app.NavigateAsync(target);
        }
        async Task Back()
        {
            if (!CurrentNavigationRender()) return;
            if (await ReturnPersonalAsync(app)) return;
            _personalReturn = null; _personalNavigationSerial++;
            RememberViewport(); setShowList(false); await app.BackAsync();
        }
        async Task Forward()
        {
            if (!CurrentNavigationRender()) return;
            _personalReturn = null; _personalNavigationSerial++;
            RememberViewport(); setShowList(false); await app.ForwardAsync();
        }
        bool CurrentNavigationRender()
        {
            var latest = app.Capture();
            return !PersonalOpen && !Program.CloseAdmissionClosed && latest.Authentication.UserId == auth.UserId && latest.Authentication.SessionStamp == auth.SessionStamp &&
                latest.Navigation?.Current?.Generation == navigation?.Current?.Generation;
        }
        async Task LoadOlder()
        {
            var current = app.Capture();
            if (Program.CloseAdmissionClosed || current.Navigation is not { Pending: false, Error: null, Current: { } visit } ||
                current.Chat.History is not { LoadingOlder: false, HasOlder: true, AtCapacity: false } history ||
                current.Authentication.UserId is not string user || current.Authentication.SessionStamp is not string stamp) return;
            RememberViewport();
            if (_timeline is not null) _timeline.Restore(_timeline.Capture());
            await app.LoadOlderAsync(visit.Target.ChannelId, history.ViewGeneration, user, stamp, visit);
        }
        async void Observed(NativeViewportObservation observation)
        {
            try
            {
                var current = app.Capture(); var context = observation.Context;
                if (current.Navigation is not { Pending: false, Error: null, Current: { } visit } || visit.Generation != context.Visit.Generation ||
                    current.Authentication.UserId != context.UserId || current.Authentication.SessionStamp != context.SessionStamp ||
                    current.Chat.SelectedChannel != context.ChannelId || current.Chat.History?.ViewGeneration != context.ViewGeneration) return;
                app.RememberViewport(visit, observation.Anchor);
                var report = ConversationHistoryPolicy.VisibleRead(current.Chat, Program.LiveWindowActive, _surfaceVisible,
                    _timeline?.Restoring != false || _timeline.AnchorUnavailable, observation.VisibleMessageIds);
                if (report is not null) await app.MarkVisibleReadAsync(report.ChannelId, report.ViewGeneration, report.MessageIds, context.UserId, context.SessionStamp, visit);
            }
            catch { /* Closing or a replaced account may invalidate a measured view. */ }
        }
        async void ThreadObserved(NativeViewportObservation observation)
        {
            try
            {
                var latest = app.Capture(); var context = observation.Context;
                if (latest.Threads is not { Visit: { } visit, Thread: { } thread } || latest.Navigation is not { Pending: false, Error: null, Current: { } nav } ||
                    nav.Generation != context.Visit.Generation || visit.Generation != context.ThreadGeneration ||
                    visit.UserId != context.UserId || visit.SessionStamp != context.SessionStamp || visit.RootMessageId != context.ThreadRootId ||
                    thread.ViewGeneration != context.ViewGeneration || thread.Generation != context.ThreadTransportGeneration) return;
                app.RememberThreadViewport(visit, observation.Anchor);
                if (!Program.LiveWindowActive || !_threadSurfaceVisible || _threadTimeline?.Restoring != false || _threadTimeline.AnchorUnavailable ||
                    latest.Threads.Loading || latest.Threads.Error is not null || !latest.Chat.Connected) return;
                var replies = thread.Replies.Select(message => message.Id).ToHashSet();
                var ids = observation.VisibleMessageIds.Where(replies.Contains).ToArray();
                if (ids.Length != 0) await app.MarkThreadVisibleReadAsync(visit, context.ViewGeneration, thread.Generation, ids);
            }
            catch { /* A recycled thread observation cannot enter another visit. */ }
        }
        WindowActivityChanged = () => { _timeline?.ObserveSoon(); _threadTimeline?.ObserveSoon(); };
        if (Program.LiveUiProbeDirectory is not null) { LoadOlderForProbe = LoadOlder; NavigateForProbe = Navigate; BackForProbe = Back; ForwardForProbe = Forward; }
        async Task SendCurrent()
        {
            var latest = app.Capture();
            if (PersonalOpen || channel is null || latest.Chat.SelectedChannel != channel || !latest.ShowConversation || Program.CloseAdmissionClosed ||
                latest.ConfirmPendingSends || latest.Navigation is not { Pending: false, Error: null } || latest.Workspace is not { Ready: true, Closing: false, Sending: false, NeedsRetry: false } ||
                !latest.Workspace.Drafts.TryGetValue(channel, out var text) || string.IsNullOrWhiteSpace(text.Body) ||
                Encoding.UTF8.GetByteCount(text.Body) > 1024 || latest.Workspace.Outbox.Count >= 8 ||
                latest.Authentication.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) return;
            if (rootScope is null || !NativeRichText.AttachmentSuffixVisible(text.Body, text.Attachments) ||
                NativeUploadPresentation.BlocksSend(latest.Uploads, rootScope.UserId, family, rootScope.ChannelId, null)) return;
            await app.SendComposerAsync(rootScope);
            if (channel == app.Chat.SelectedChannel && app.Workspace?.Drafts[channel].Body == "")
                _timeline?.RevealEnd();
        }
        async void Send() => await SendCurrent();
        if (Program.LiveUiProbeDirectory is not null) SendForProbe = SendCurrent;
        async Task DiscardRejected(string requestId)
        {
            if (PersonalOpen || Program.CloseAdmissionClosed || _discardDialog is not null || auth.UserId is not string user ||
                auth.SessionStamp is not string stamp || PilotComponent.RootForClose?.XamlRoot is not { } root) return;
            bool SameAccount() => app.Authentication.UserId == user && app.Authentication.SessionStamp == stamp && !Program.CloseAdmissionClosed;
            var dialog = new ContentDialog { XamlRoot = root, Title = "Fjerne den avviste meldinga?",
                Content = "Teksten blir fjerna frå sendekøen på denne eininga. Inga melding blir send.",
                PrimaryButtonText = "Fjern", CloseButtonText = "Behald", DefaultButton = ContentDialogButton.Close };
            _discardDialog = dialog;
            if (Program.LiveUiProbeDirectory is not null) DiscardDialogForProbe = dialog;
            void AccountChanged() { if (!SameAccount()) dialog.DispatcherQueue.TryEnqueue(() => dialog.Hide()); }
            app.Changed += AccountChanged;
            try
            {
                if (!SameAccount()) return;
#pragma warning disable REACTOR_DIALOG_001
                var choice = await dialog.ShowAsync();
#pragma warning restore REACTOR_DIALOG_001
                if (choice == ContentDialogResult.Primary && SameAccount()) await app.DiscardRejectedAsync(user, stamp, requestId);
            }
            finally { app.Changed -= AccountChanged; _discardDialog = null; DiscardDialogForProbe = null; }
        }
        if (Program.LiveUiProbeDirectory is not null) DiscardForProbe = DiscardRejected;

        string status = auth.Phase switch
        {
            NativeAuthPhase.Active when chat.Connected => "● Tilkopla",
            NativeAuthPhase.Active => "◌ Koplar til samtalen …",
            NativeAuthPhase.Offline => "◌ Fråkopla · utkast og sendekø er tekne vare på",
            NativeAuthPhase.SigningIn => "◌ Fullfør innlogginga i nettlesaren",
            NativeAuthPhase.AuthRequired => "○ Logg inn på nytt · utkastet er bevart",
            _ => "○ Logga ut",
        };
        bool loginVisible = auth.Phase is NativeAuthPhase.SignedOut or NativeAuthPhase.SigningIn or NativeAuthPhase.AuthRequired;
        var masthead = VStack(0, Grid([GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto], [GridSize.Auto],
            TextBlock("Sprøyt").FontSize(23).Bold().Foreground(palette.Ink).VAlign(VerticalAlignment.Center).Grid(column: 0),
            VStack(2, TextBlock(_canonicalSelf?.DisplayName ?? auth.DisplayName ?? "Canary").AutomationId("live-personal-display-name").FontSize(13).SemiBold().Foreground(palette.Ink),
                TextBlock(status).FontSize(12).Foreground(palette.Muted)).Margin(16, 0).Grid(column: 1),
            Button(auth.Phase == NativeAuthPhase.SigningIn ? "Avbryt" : loginVisible ? "Logg inn" : "Logg ut", () =>
                { if (auth.Phase == NativeAuthPhase.SigningIn) Logout(); else if (loginVisible) Login(); else Logout(); })
                .AutomationId("live-authentication").AutomationName("Innlogging og utlogging").MinHeight(44).Grid(column: 2),
            Button("◐", () => setDark(!dark)).AutomationName("Byt lyst og mørkt tema").Size(44, 44).Margin(8, 0, 0, 0).Grid(column: 3)
        ).Padding(16, 8).Background(palette.Chrome), PersonalToolbar(app, state, palette)).Grid(columnSpan: 2);

        var list = (navigation is not null ? NativeNavigationView.Render(navigation with { CanBack = navigation.CanBack || CanReturnPersonal(app) }, palette,
            new(app.SetNavigationQuery, app.SetNavigationScope, target => { _ = Navigate(target); }, () => { _ = Back(); }, () => { _ = Forward(); }, app.SetNavigationInteraction))
            : ScrollViewer(TextBlock("Lastar samtalar …"))).Background(palette.Chrome).IsVisible(!PersonalOpen && state.ShowConversation && (wide || showList))
            .Grid(row: 1, column: 0, columnSpan: wide ? 1 : 2);

        bool IsRejected(NativeMessage message) => message.Id.StartsWith("pending:", StringComparison.Ordinal) &&
            state.Workspace?.Outbox.Any(item => item.RequestId == message.Id[8..] && item.State == "rejected") == true;
        string MessageStatus(NativeMessage message) => IsRejected(message) ? "Ikkje sendt — avvist" :
            message.Id.StartsWith("pending:", StringComparison.Ordinal) ? "Lagra på denne eininga" : message.SentAt.ToLocalTime().ToString("HH:mm");
        var authorNames = rows.Concat(threadRows).GroupBy(message => message.AuthorId).ToDictionary(group => group.Key, group => group.First().Author);
        var chipsByMessage = (chat.Conversation?.Reactions ?? []).GroupBy(reaction => reaction.MessageId).ToDictionary(group => group.Key,
            group => (IReadOnlyList<NativeReactionChip>)group.Select(item => new NativeReactionChip(item.Emoji, item.Count, item.ReactedByMe,
                item.UserIds.Select(user => user == auth.UserId ? auth.DisplayName ?? "Du" : authorNames.GetValueOrDefault(user) ?? "Brukar " + user).ToArray())).ToArray());
        var summariesByMessage = (chat.Conversation?.ThreadSummaries ?? []).ToDictionary(summary => summary.RootMessageId);
        string ActionState(NativeMessage message, NativeMessageActionScope? scope, bool inThread)
        {
            if (scope is null) return "unavailable";
            var owner = inThread ? _threadActions : _rootActions;
            bool canOpen = owner.CanOpen(scope, message);
            if (owner.MessageId != message.Id) return canOpen ? "idle-enabled" : "idle-disabled";
            return System.Text.Json.JsonSerializer.Serialize(new { owner.Mode, owner.Input, owner.Error, owner.Busy, owner.NeedsReadBack, owner.HasAttachments, owner.CanSave, canOpen });
        }
        NativeMessagePresentation Present(NativeMessage message, bool inThread = false) => new(message, _rawMessages.IsRaw(message.Id), auth,
            palette, inThread ? threadScope : rootScope, IsRejected(message), auth.Phase == NativeAuthPhase.Active && chat.Connected &&
                navigation is { Pending: false, Error: null } && (!inThread || threads is { Loading: false, Error: null, AtCapacity: false }),
            chipsByMessage.GetValueOrDefault(message.Id) ?? [], chat.Conversation?.ReactionsLoaded == true,
            summariesByMessage.TryGetValue(message.Id, out var summary) ? $"Tråd · {summary.ReplyCount} svar" : "Svar i tråd",
            ActionState(message, inThread ? threadScope : rootScope, inThread));
        Element Message(NativeMessagePresentation presentation, bool inThread = false)
        {
            var message = presentation.Message; var viewer = presentation.Viewer; var rowPalette = presentation.Palette;
            return VStack(5,
            Border(Empty()).Height(1).Width(56).Background(palette.Rule).HAlign(HorizontalAlignment.Left),
            HStack(8, TextBlock(message.Author).FontSize(14).SemiBold().Foreground(palette.Ink),
                TextBlock(presentation.Rejected ? "Ikkje sendt - avvist" : MessageStatus(message)).FontSize(12).Foreground(presentation.Rejected ? rowPalette.Error : rowPalette.Muted)
                    .AutomationId("live-delivery-" + message.Id)),
            message.DeletedAt is null ? NativeMessageView.Render(app, message, rowPalette, viewer, _rawMessages) : TextBlock("Meldinga er sletta.").Foreground(rowPalette.Muted),
            TextBlock("Redigert").FontSize(12).Foreground(palette.Muted).IsVisible(message.EditedAt is not null && message.DeletedAt is null),
            presentation.ActionScope is { } actionScope
                ? NativeMessageActionsView.Render(inThread ? _threadActions : _rootActions, actionScope, message, presentation.Reactions, rowPalette, messageCommands,
                    presentation.ReactionsLoaded, () => { _ = app.RefreshConversationAsync(actionScope); })
                    .IsEnabled(presentation.Interactive)
                : Empty(),
            Button(presentation.ThreadLabel, async () => await Navigate(new(message.ChannelId, ThreadId: message.Id)))
                .AutomationName("Opne tråden til meldinga").AutomationId("live-open-thread-" + message.Id).MinHeight(36)
                .IsVisible(!inThread && !message.Id.StartsWith("pending:", StringComparison.Ordinal)).IsEnabled(chat.Connected && navigation is { Pending: false }),
            Button("Fjern avvist", async () => await DiscardRejected(message.Id[8..])).MinHeight(44)
                .AutomationId("live-discard-" + message.Id).IsVisible(IsRejected(message))
        ).Padding(16, 8).AutomationId("live-message-" + message.Id).WithKey(message.Id);
        }
        var presentations = rows.Select(message => Present(message)).ToArray();
        var history = LazyVStack<NativeMessagePresentation>(presentations, item => item.Message.Id, (item, _) => Message(item))
            .AutomationId("live-history").AutomationName("Meldingar")
            .OnMount(element =>
            {
                if (element is not ScrollViewer scroller) return;
                _timeline = new(scroller, () => !Program.CloseAdmissionClosed && Program.LiveWindowActive && _surfaceVisible && _discardDialog is null, Observed);
                _timeline.Update(_timelineContext, _requestedAnchor ?? navigation?.Current?.Anchor);
                _requestedAnchor = null;
                if (Program.LiveUiProbeDirectory is not null) { TimelineForProbe = _timeline; ScrollForProbe = scroller; }
            })
            .OnUnmount(_ => { _timeline?.Dispose(); _timeline = null; TimelineForProbe = null; ScrollForProbe = null; })
            .WithKey("live-history").Grid(row: 1);
        _surfaceVisible = !PersonalOpen && state.ShowConversation && (wide || !showList) && !featureOpen;
        var currentVisit = navigation?.Current;
        long viewGeneration = chat.History?.ViewGeneration ?? 0;
        if (currentVisit is not null && (_lastVisitGeneration != currentVisit.Generation || _lastViewGeneration != viewGeneration || (!_previousSurfaceVisible && _surfaceVisible)))
            _requestedAnchor = currentVisit.Anchor;
        _timelineContext = channel is not null && currentVisit is not null && auth.UserId is string viewer && auth.SessionStamp is string session && _surfaceVisible
            ? new(viewer, session, channel, viewGeneration, currentVisit, rows, chat.Loading || navigation!.Pending || navigation.Error is not null || chat.History?.LoadingOlder == true)
            : null;
        _timeline?.Update(_timelineContext, _requestedAnchor);
        if (_timeline is not null) _requestedAnchor = null;
        _lastVisitGeneration = currentVisit?.Generation ?? -1; _lastViewGeneration = viewGeneration; _previousSurfaceVisible = _surfaceVisible;

        var editor = TextBox(draft?.ComposerText ?? "", text =>
        {
            if (channel is not null && app.Workspace?.Drafts.TryGetValue(channel, out var latest) == true && latest.ComposerText != text)
                if (rootScope is not null) app.EditComposer(rootScope, text);
        }, placeholderText: "Skriv ei melding …").AcceptsReturn(true).TextWrapping(TextWrapping.Wrap).MinHeight(44).MaxHeight(180)
            .AutomationId("live-composer").AutomationName("Skriv ei melding")
            .IsEnabled(draft is not null && !Program.CloseAdmissionClosed)
            .OnMount(element =>
            {
                if (element is not TextBox input) return;
                PilotComponent.EditorForClose = input;
                input.PreviewKeyDown += (_, args) =>
                {
                    if (args.Key == VirtualKey.Enter && (InputKeyboardSource.GetKeyStateForCurrentThread(VirtualKey.Control) & CoreVirtualKeyStates.Down) != 0)
                    { args.Handled = true; Send(); }
                };
            }).WithKey($"live-composer-{channel}-{rootScope?.UserId}-{rootScope?.SessionStamp}-{rootScope?.ChannelVisit}");
        string notice = state.ConfirmPendingSends ? "Du har lagra meldingar frå førre innlogging. Stadfest før dei blir sende." : state.Error ?? (Encoding.UTF8.GetByteCount(draft?.Body ?? "") > 1024 ? "Meldinga er for lang for denne første Windows-versjonen." :
            state.RejectedSends > 0 ? $"{state.RejectedSends} meldingar vart avviste. Teksten er teken vare på og blir ikkje send automatisk." :
            state.DeliveryRetryPending ? "Sendinga ventar før eit nytt forsøk. Teksten er teken vare på." :
            state.PendingSends > 0 ? $"{state.PendingSends} meldingar ventar på sendekvittering." : "");
        bool canRetryDelivery = state.ConfirmPendingSends || state.PendingSends > 0 || state.Error is not null;
        var composer = DropHost(Border(VStack(4, UploadTools(rootScope, draft, false), Grid([GridSize.Star(), GridSize.Auto], [GridSize.Auto], editor.Grid(column: 0),
            Button("↑", Send).AutomationId("live-send").AutomationName("Send meldinga").Size(48, 48).Margin(8, 0, 0, 0)
                .Foreground(palette.AccentInk).Background(palette.Accent).IsEnabled(canSend).Grid(column: 1)
        ))).Padding(12).Background(palette.Chrome).BorderThickness(new Thickness(0, 2, 0, 0)).BorderBrush(palette.Rule).Grid(row: 3), false);
        var conversation = Grid([GridSize.Star()], [GridSize.Auto, GridSize.Star(), GridSize.Auto, GridSize.Auto],
            VStack(4, HStack(12, Button("←", () => { RememberViewport(); setShowList(true); }).AutomationName("Vis samtalar").Size(44, 44).IsVisible(!wide),
                VStack(2, TextBlock(selected?.IsDirect == true ? "Direkte" : selected?.CircleName ?? "Felles").FontSize(12).Foreground(palette.Muted),
                    TextBlock(selected is null ? "Samtalar" : (selected.IsDirect ? "" : "# ") + selected.Name).FontSize(21).Bold().Foreground(palette.Ink)))
                .Padding(12, 8),
                HStack(8, Button("Trådar", async () => { RememberViewport(); if (rootScope is not null) await app.OpenThreadDirectoryAsync(rootScope); })
                    .MinHeight(36).AutomationName("Vis trådar i samtalen").AutomationId("live-thread-directory-open").IsEnabled(chat.Connected && rootScope is not null),
                    Button("Oppdater meldingsval", async () => { if (rootScope is not null) await app.RefreshConversationAsync(rootScope); })
                        .MinHeight(36).AutomationName("Hent meldingsval på nytt").AutomationId("live-refresh-message-actions")
                        .IsVisible(chat.Conversation?.ReactionsLoaded != true || chat.Conversation?.ReactionError is not null)).Margin(12, 0),
                Grid([GridSize.Auto, GridSize.Star()], [GridSize.Auto], Button(chat.History?.LoadingOlder == true ? "Lastar eldre …" : "Last eldre meldingar", async () => await LoadOlder())
                    .MinHeight(44).AutomationName("Last eldre meldingar").AutomationId("live-load-older").IsEnabled(chat.Connected && chat.History is { HasOlder: true, LoadingOlder: false, AtCapacity: false } && navigation is { Pending: false }).Grid(column: 0),
                    TextBlock(navigation?.Pending == true ? "Finn leseposisjonen …" : navigation?.Error ?? chat.History?.Error ??
                        (chat.History?.AtCapacity == true ? "Historikkgrensa for denne visinga er nådd." : chat.History?.HasOlder == false && rows.Length > 0 ? "Starten av samtalen er lasta." : ""))
                        .FontSize(12).Foreground(palette.Muted).TextWrapping().Margin(12, 0, 0, 0).Grid(column: 1)).Padding(12, 0, 12, 4)).Grid(row: 0), history,
            VStack(4, TextBlock(notice).TextWrapping().FontSize(12).Foreground(palette.Muted), HStack(8,
                Button("Prøv sending", async () =>
                    { if (auth.UserId is string user && auth.SessionStamp is string stamp) await app.RetryPendingSendsAsync(user, stamp); })
                    .AutomationId("live-send-retry").AutomationName("Prøv ventande sending på nytt").MinHeight(44)
                    .IsVisible(!state.ConfirmPendingSends && state.PendingSends > 0 && auth.Phase == NativeAuthPhase.Active && chat.Connected),
                Button(state.ConfirmPendingSends ? "Send ventande" : "Oppdater samband", async () =>
                    { if (state.ConfirmPendingSends && auth.UserId is string user && auth.SessionStamp is string stamp)
                        await app.ConfirmPendingSendsAsync(user, stamp); else Retry(); })
                    .AutomationId("live-delivery-retry").AutomationName("Stadfest ventande meldingar eller oppdater samband").MinHeight(44).IsVisible(notice.Length > 0 && canRetryDelivery)))
                .Padding(12, 4).IsVisible(notice.Length > 0).Grid(row: 2), composer
        ).IsVisible(!PersonalOpen && state.ShowConversation && (wide || !showList) && !featureOpen).Grid(row: 1, column: 1);

        async Task SendThread()
        {
            if (PersonalOpen || threadScope is null || Program.CloseAdmissionClosed) return;
            var latest = app.Capture(); string key = new ComposerTarget(threadScope.ChannelId, threadScope.ThreadRootId).Key;
            if (latest.Threads?.Visit?.Generation != threadScope.ThreadVisit || latest.Authentication.UserId != threadScope.UserId ||
                latest.Authentication.SessionStamp != threadScope.SessionStamp || latest.Navigation is not { Pending: false, Error: null, Current: { } nav } ||
                nav.Generation != threadScope.ChannelVisit || latest.Workspace is not { Ready: true, Closing: false, Sending: false, NeedsRetry: false } workspace ||
                !workspace.Drafts.TryGetValue(key, out var body) || string.IsNullOrWhiteSpace(body.Body) || Encoding.UTF8.GetByteCount(body.Body) > 1024 ||
                workspace.Outbox.Count >= 8 || latest.ConfirmPendingSends || !NativeRichText.AttachmentSuffixVisible(body.Body, body.Attachments) ||
                NativeUploadPresentation.BlocksSend(latest.Uploads, threadScope.UserId, family, threadScope.ChannelId, threadScope.ThreadRootId)) return;
            await app.SendComposerAsync(threadScope);
            if (app.Capture().Threads?.Visit?.Generation == threadScope.ThreadVisit && app.Workspace?.Drafts.GetValueOrDefault(key)?.Body == "")
                _threadTimeline?.RevealEnd();
        }
        async Task CloseThread()
        {
            if (PersonalOpen) return;
            RememberViewport(); if (threads?.Visit is { } visit) await app.CloseThreadAsync(visit);
        }
        if (Program.LiveUiProbeDirectory is not null) SendThreadForProbe = SendThread;
        Element feature = Empty();
        _threadSurfaceVisible = !PersonalOpen && threadDetail && state.ShowConversation && (wide || !showList);
        if (threads?.Visit is { } featureVisit)
        {
            async void RetryThread() => await app.RetryThreadAsync(featureVisit);
            if (featureVisit.IsDirectory)
                feature = NativeThreadView.RenderDirectory(threads, palette, new(() => { _ = CloseThread(); }, RetryThread,
                    root => { _ = Navigate(new(featureVisit.ChannelId, ThreadId: root)); }));
            else
            {
                string key = new ComposerTarget(featureVisit.ChannelId, featureVisit.RootMessageId).Key;
                var threadDraft = state.Workspace?.Drafts.GetValueOrDefault(key);
                var pending = (state.Workspace?.Outbox ?? []).Where(item => item.Channel == featureVisit.ChannelId && item.ParentMessageId == featureVisit.RootMessageId)
                    .Select(item => new NativeMessage("pending:" + item.RequestId, item.Channel, auth.UserId ?? "", auth.DisplayName ?? "Du", item.Body,
                        DateTimeOffset.MinValue, ulong.MaxValue, item.ParentMessageId)).ToArray();
                bool enabled = !PersonalOpen && threadScope is not null && !threads.Loading && threads.Error is null && !threads.AtCapacity && threadDraft is not null &&
                    state.Workspace is { Ready: true, Closing: false } && !Program.CloseAdmissionClosed;
                bool sendReply = enabled && !state.ConfirmPendingSends && navigation is { Pending: false, Error: null } &&
                    state.Workspace is { Sending: false, NeedsRetry: false } && state.Workspace.Outbox.Count < 8 &&
                    !string.IsNullOrWhiteSpace(threadDraft!.Body) && Encoding.UTF8.GetByteCount(threadDraft.Body) <= 1024 &&
                    NativeRichText.AttachmentSuffixVisible(threadDraft.Body, threadDraft.Attachments) && !UploadBlocked(featureVisit.ChannelId, featureVisit.RootMessageId);
                if (_lastThreadVisit != featureVisit.Generation || _lastThreadGeneration != threads.Thread?.Generation)
                    _threadRequestedAnchor = threads.Anchor;
                _threadTimelineContext = threadScope is not null && navigation?.Current is { } nav && _threadSurfaceVisible
                    ? new(threadScope.UserId, threadScope.SessionStamp, threadScope.ChannelId, chat.History?.ViewGeneration ?? 0, nav,
                        threadRows.Concat(pending).ToArray(), threads.Loading || threads.Error is not null || navigation.Pending,
                        featureVisit.RootMessageId, featureVisit.Generation, threads.Thread?.Generation ?? 0) : null;
                _threadTimeline?.Update(_threadTimelineContext, _threadRequestedAnchor);
                if (_threadTimeline is not null) _threadRequestedAnchor = null;
                _lastThreadVisit = featureVisit.Generation; _lastThreadGeneration = threads.Thread?.Generation ?? -1;
                feature = NativeThreadView.RenderDetail(threads, palette, new((presentation, _) => Message(presentation, true), threadDraft?.ComposerText ?? "",
                    text => { if (threadScope is not null) app.EditComposer(threadScope, text); }, () => { _ = SendThread(); }, () => { _ = CloseThread(); }, RetryThread,
                    sendReply, enabled, pending,
                    scroller =>
                    {
                        _threadTimeline = new(scroller, () => _threadSurfaceVisible && Program.LiveWindowActive && !Program.CloseAdmissionClosed && _discardDialog is null, ThreadObserved);
                        _threadTimeline.Update(_threadTimelineContext, _threadRequestedAnchor ?? threads.Anchor); _threadRequestedAnchor = null;
                        if (Program.LiveUiProbeDirectory is not null) { ThreadTimelineForProbe = _threadTimeline; ThreadScrollForProbe = scroller; }
                    },
                    () => { _threadTimeline?.Dispose(); _threadTimeline = null; ThreadTimelineForProbe = null; ThreadScrollForProbe = null; },
                    input => PilotComponent.EditorForClose = input,
                    VStack(4,
                        TextBlock(state.ConfirmPendingSends ? "Stadfest dei lagra meldingane frå førre innlogging før du sender." : state.Error ??
                            (Encoding.UTF8.GetByteCount(threadDraft?.Body ?? "") > 1024 ? "Svaret er for langt for denne Windows-versjonen." :
                            state.DeliveryRetryPending ? "Sendinga ventar før eit nytt forsøk." : state.PendingSends > 0 ? "Meldingar ventar på sendekvittering." :
                            state.RejectedSends > 0 ? "Ei melding vart avvist. Teksten er teken vare på." : ""))
                            .TextWrapping().Foreground(palette.Muted),
                        HStack(8,
                            Button("Prøv sending", async () => { if (auth.UserId is string user && auth.SessionStamp is string stamp) await app.RetryPendingSendsAsync(user, stamp); })
                                .AutomationName("Prøv ventande sending på nytt").AutomationId("live-thread-send-retry").MinHeight(44)
                                .IsVisible(state.PendingSends > 0 && !state.ConfirmPendingSends && chat.Connected),
                            Button(state.ConfirmPendingSends ? "Send ventande" : "Oppdater samband", async () =>
                                { if (state.ConfirmPendingSends && auth.UserId is string user && auth.SessionStamp is string stamp) await app.ConfirmPendingSendsAsync(user, stamp); else Retry(); })
                                .AutomationName("Stadfest ventande meldingar eller oppdater samband").AutomationId("live-thread-delivery-retry").MinHeight(44)
                                .IsVisible(state.ConfirmPendingSends || state.Error is not null)))
                        .Padding(12, 4).IsVisible(state.ConfirmPendingSends || state.PendingSends > 0 || state.RejectedSends > 0 || state.Error is not null || Encoding.UTF8.GetByteCount(threadDraft?.Body ?? "") > 1024),
                    UploadTools(threadScope, threadDraft, true), message => Present(message, true), composer => DropHost(composer, true)));
            }
        }
        else { _threadTimelineContext = null; _threadTimeline?.Update(null); _lastThreadVisit = _lastThreadGeneration = -1; }
        feature = feature.IsVisible(!PersonalOpen && state.ShowConversation && (wide || !showList) && featureOpen).Grid(row: 1, column: 1);

        string invitation = auth.Phase == NativeAuthPhase.SigningIn ? "Vel kontoen din i nettlesaren og stadfest innlogginga."
            : auth.Phase == NativeAuthPhase.AuthRequired ? "Logg inn igjen. Utkast og ventande meldingar ventar på deg."
            : auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline ? "Lastar samtalane dine …" : "Logg inn med den vanlege Sprøyt-kontoen din.";
        var welcome = VStack(16, TextBlock("Velkomen til Sprøyt").FontSize(28).Bold().Foreground(palette.Ink),
            TextBlock(invitation).TextWrapping().FontSize(16).Foreground(palette.Muted),
            TextBlock(state.Error ?? "").TextWrapping().Foreground(palette.Muted),
            Button("Logg inn med Sprøyt", Login).MinHeight(44).IsVisible(loginVisible).AutomationName("Logg inn med Sprøyt")
        ).Padding(32).VAlign(VerticalAlignment.Center).IsVisible(!state.ShowConversation).Grid(row: 1, columnSpan: 2);
        var personal = PersonalView(app, state, palette).IsVisible(PersonalOpen && state.ShowConversation).Grid(row: 1, columnSpan: 2);
        return Grid([wide ? GridSize.Px(244) : GridSize.Px(0), GridSize.Star()], [GridSize.Auto, GridSize.Star()], masthead, list, conversation, feature, welcome, personal)
            .Background(palette.Paper).RequestedTheme(dark ? ElementTheme.Dark : ElementTheme.Light)
            .OnMount(element => { PilotComponent.RootForClose = element; Program.Keyboard?.Attach(element as Grid); LiveUiProbe.Attach(element); LiveCanaryAcceptance.Attach(element); element.GotFocus += ConversationGotFocus; })
            .OnUnmount(element => { element.GotFocus -= ConversationGotFocus; PilotComponent.RootForClose = null; }).AutomationId("sproyt-live-root");
    }

}
