using System.Net.Http.Json;
using System.Text;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Coordinates the existing durable draft/outbox owner with the live transport.
// No token or provider credential is projected into the UI.
internal sealed partial class LiveApplication : ILiveApplication
{
    private readonly NativeAuthSession _authentication;
    private readonly NativeLoginClient _login;
    private readonly NativeChatClient _chat;
    private readonly string _directory;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly SemaphoreSlim _signal = new(0, 1);
    private readonly Task _pump;
    private NativeSession? _store;
    private ChatWorkspace? _workspace;
    private string? _account;
    private Task? _connection;
    private bool _closing;
    private string? _error;
    private string? _admittedFamily;
    private readonly string _instance;
    private SendAdmissionStore? _replayConsent;
    private LiveDeliveryCoordinator _delivery = new();
    private readonly LiveDeliveryRetryWake _deliveryWake;
    private sealed record ManualSendRetry(string UserId, string Family, string RequestId);
    private ManualSendRetry? _manualSendRetry;
    private sealed record DeliveryWakeTarget(string Scope, string RequestId);
    private DeliveryWakeTarget? _deliveryWakeTarget;
    private sealed record DeliveryNotice(string Account, string? Family, string RequestId, string Message, bool Rejected = false)
    {
        internal bool Matches(NativeAuthSnapshot auth) => auth.UserId == Account && SameSendFamily(Family, auth.SessionStamp);
    }
    private DeliveryNotice? _deliveryNotice;
    private readonly NativeNavigationController _navigation;
    private CancellationTokenSource? _navigationOperation;
    private CancellationTokenSource _conversationVisitCancellation = new();
    private string? _navigationStamp;
    private NativeNavigationTarget? _navigationResume;
    public event Action? Changed;
    public NativeAuthSnapshot Authentication => _authentication.Snapshot;
    public NativeChatSnapshot Chat => _chat.Snapshot;
    public WorkspaceSnapshot? Workspace => _workspace?.State;
    public NativeMediaLibrary Media { get; }
    private string? ErrorFor(NativeAuthSnapshot auth, NativeChatSnapshot chat, WorkspaceSnapshot? local) => _error ?? (_deliveryNotice is { } issue && issue.Matches(auth) ? issue.Message : null)
        ?? chat.Error ?? local?.ErrorMessage;
    internal bool HasAccount => _workspace is not null && Authentication.UserId == _account;
    internal bool ShowConversation => HasAccount && Authentication.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline;
    public LiveSnapshot Capture()
    {
        SynchronizeNavigation();
        return _authentication.ProjectSnapshot(auth =>
        {
            var workspace = _workspace; var local = workspace?.State; var rawChat = Chat;
            var chat = NativeChatClient.FenceAccountProjection(rawChat, auth);
            bool same = ReferenceEquals(workspace, _workspace) && auth.UserId is not null && auth.UserId == _account && rawChat.UserId == auth.UserId;
            var navigation = _navigation.Capture();
            if (_navigationStamp != auth.SessionStamp || navigation.UserId != auth.UserId) navigation = null;
            var threads = same ? _threads.Capture() : null;
            if (threads?.UserId != auth.UserId || threads?.Visit is { } visit && (visit.SessionStamp != auth.SessionStamp ||
                navigation?.Current?.Target.ChannelId != visit.ChannelId || navigation.Current.Target.ThreadId != visit.RootMessageId))
                threads = null;
            var uploadIssue = same ? CurrentUploadIssue(auth) : null;
            return new LiveSnapshot(auth, chat, same ? local : null, same && local?.Ready == true &&
                auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline, ErrorFor(auth, chat, same ? local : null),
                same && local?.Outbox.Any(item => item.State != "rejected") == true && !SameSendFamily(_admittedFamily, auth.SessionStamp),
                same && _deliveryWakeTarget?.Scope == auth.UserId + "/" + SendFamily(auth.SessionStamp), navigation, threads,
                same && _uploads is { } uploads ? uploads.Capture() with { ActiveFamilyStamp = SendFamily(auth.SessionStamp) } : null,
                uploadIssue?.Message, uploadIssue?.Target);
        });
    }

    private sealed record Authority(string Broker, string Instance, string Issuer);
    private LiveApplication(string directory, NativeAuthSession owner, NativeAuthConfig config)
    {
        _directory = directory; _authentication = owner;
        _deliveryWake = new(Signal);
        _navigation = new(new NativeNavigationPreferenceStore(Path.Combine(directory, "navigation")));
        _navigation.Changed += Notify;
        _threads.Changed += Notify;
        Media = new(owner, directory);
        _instance = config.Instance;
        _login = new(owner, config, Path.Combine(directory, "login"));
        _chat = new(owner);
        _authentication.Changed += ChangedState; _login.Changed += ChangedState; _chat.Changed += ChangedState;
        _pump = PumpAsync();
        Signal();
    }

    internal static async Task<LiveApplication> OpenAsync(string server, string directory)
    {
        var origin = new Uri(server, UriKind.Absolute);
        if (origin.AbsoluteUri != "https://sproyt-canary.bjoroy.me/")
            throw new InvalidOperationException("This Windows pilot is configured for Sprøyt Canary.");
        string root = Path.GetFullPath(directory);
        string authorityPath = Path.Combine(root, "authority");
        bool exists = File.Exists(Path.Combine(authorityPath, "state.bin"));
        using var authority = new WindowsProtectedJournal(authorityPath, create: !exists);
        Authority settings;
        try
        {
            using var http = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false }) { BaseAddress = origin, Timeout = TimeSpan.FromSeconds(12) };
            using var response = await http.GetAsync("/api/v1/native/info", HttpCompletionOption.ResponseHeadersRead);
            if (response.StatusCode == System.Net.HttpStatusCode.NotFound)
                throw new InvalidOperationException("Native Windows-pilot er ikkje aktivert på Canary enno.");
            response.EnsureSuccessStatusCode();
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(12));
            await response.Content.LoadIntoBufferAsync(8192, timeout.Token);
            using var json = JsonDocument.Parse(await response.Content.ReadAsStringAsync(timeout.Token));
            var info = json.RootElement;
            if (info.GetProperty("scope").GetString() != "sproyt_native_v1" || info.GetProperty("environment").GetString() != "canary")
                throw new InvalidOperationException("Unexpected native server scope.");
            settings = new(origin.AbsoluteUri, info.GetProperty("instance_id").GetString()!, info.GetProperty("issuer").GetString()!);
            if (exists && authority.Read<Authority>() != settings) throw new InvalidOperationException("Native server identity changed.");
            authority.Write(settings);
        }
        catch (Exception exception) when (exists && exception is HttpRequestException or TaskCanceledException)
        {
            settings = authority.Read<Authority>();
            if (settings.Broker != origin.AbsoluteUri) throw new InvalidOperationException("Cached native server identity mismatch.");
        }
        var config = new NativeAuthConfig(settings.Broker, settings.Instance, settings.Issuer, "", "sproyt_native_v1", "canary");
        var owner = NativeAuthSession.Open(Path.Combine(root, "session"), config);
        return new(root, owner, config);
    }

    private void Notify() { try { Changed?.Invoke(); } catch { } }
    private void Signal() { try { _signal.Release(); } catch (SemaphoreFullException) { } }
    private void ChangedState()
    {
        SynchronizeNavigation();
        BindUploadScope();
        var auth = Authentication;
        string? scope = auth.UserId is string user && SendFamily(auth.SessionStamp) is string family ? user + "/" + family : null;
        if (_deliveryNotice is { } issue && !issue.Matches(auth)) Interlocked.CompareExchange(ref _deliveryNotice, null, issue);
        if (auth.Phase != NativeAuthPhase.Active || !Chat.Connected || (_deliveryWakeTarget is { } target && target.Scope != scope))
            CancelDeliveryWake();
        Signal(); Notify();
    }
    private void SynchronizeNavigation()
    {
        CancellationTokenSource? cancel = null;
        NativeNavigationVisit? refresh = null;
        var resume = _authentication.ProjectSnapshot(auth =>
        {
            if (auth.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline) || auth.UserId is null || auth.SessionStamp is null)
            {
                _navigation.BindScope(null, null); _navigationStamp = null; _navigationResume = null;
                _threads.BindScope(null, null);
                cancel = Interlocked.Exchange(ref _navigationOperation, null);
                return (NativeNavigationVisit?)null;
            }
            var previous = _navigation.Capture().Current;
            if (_navigationStamp != auth.SessionStamp)
            {
                _navigationResume = previous?.UserId == auth.UserId && SendFamily(previous.SessionStamp) == SendFamily(auth.SessionStamp)
                    ? previous.Target : null;
                _navigationStamp = auth.SessionStamp;
                cancel = Interlocked.Exchange(ref _navigationOperation, null);
            }
            _navigation.BindScope(auth.UserId, auth.SessionStamp);
            _threads.BindScope(auth.UserId, auth.SessionStamp);
            var chat = Chat;
            if (chat.UserId != auth.UserId || !chat.Connected)
            {
                _navigationResume = _navigation.Capture().Current?.Target;
                return null;
            }
            if (chat.Loading || auth.UserId != _account || _workspace?.State.Ready != true) return null;
            _navigation.UpdateChannels(auth.UserId, auth.SessionStamp, chat.Channels);
            SynchronizeThreadProjection(chat);
            var navigation = _navigation.Capture(); var current = navigation.Current;
            if (_navigationResume is { } wanted)
            {
                _navigationResume = null;
                if (current?.Target == wanted) return _navigation.RestartCurrent();
            }
            if (current is null && chat.SelectedChannel is string selected && chat.Channels.Any(item => item.Id == selected))
            {
                var initial = _navigation.Select(new(selected));
                return initial;
            }
            if (!navigation.Pending && navigation.Error is null && current is not null && current.Target.ChannelId == chat.SelectedChannel &&
                chat.History is { } history)
            {
                if (_metadataVisit == current.Generation && _metadataView != history.ViewGeneration)
                {
                    // An in-place history repair retired all transport projections.
                    // Replace the visit once and reload its exact target and anchor.
                    _metadataVisit = -1;
                    return _navigation.RestartCurrent();
                }
                if (_metadataVisit != current.Generation)
                {
                    _metadataVisit = current.Generation; _metadataView = history.ViewGeneration;
                    refresh = current;
                }
            }
            return null;
        });
        try { cancel?.Cancel(); } catch (ObjectDisposedException) { }
        if (resume is not null) _ = NavigateVisitAsync(resume);
        else if (refresh is not null) _ = RefreshConversationMetadataAsync(refresh);
    }
    private bool AdmitNavigation(NativeNavigationVisit visit, Action admission) => _authentication.ProjectSnapshot(auth =>
        !_closing && auth.UserId == visit.UserId && auth.SessionStamp == visit.SessionStamp && _account == auth.UserId &&
        _navigation.AdmitCurrent(visit, admission));

    private void CancelDeliveryWake()
    {
        _deliveryWake.Cancel(); _deliveryWakeTarget = null;
        Interlocked.Exchange(ref _manualSendRetry, null);
    }
    private void CompleteDeliveryWake(string requestId)
    {
        if (_deliveryWakeTarget?.RequestId == requestId)
        {
            _deliveryWake.Cancel(); _deliveryWakeTarget = null;
        }
        var manual = Volatile.Read(ref _manualSendRetry);
        if (manual?.RequestId == requestId) Interlocked.CompareExchange(ref _manualSendRetry, null, manual);
    }
    private void ScheduleDeliveryWake(string account, string stamp, string requestId, TimeSpan delay)
    {
        var auth = Authentication;
        if (_closing || auth.Phase != NativeAuthPhase.Active || !Chat.Connected || auth.UserId != account || account != _account ||
            SendFamily(stamp) is not string family || !SameSendFamily(family, auth.SessionStamp) || !SameSendFamily(_admittedFamily, auth.SessionStamp)) return;
        var target = new DeliveryWakeTarget(_account + "/" + family, requestId);
        _deliveryWakeTarget = target;
        _deliveryWake.Schedule(target.Scope, delay);
    }

    public async Task LoginAsync()
    {
        try { _error = null; if (_login.IsPending) await _login.ResumePendingAsync(_lifetime.Token); else await _login.LoginAsync(_lifetime.Token); }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch { _error = "Innlogginga vart ikkje fullført. Prøv igjen; utkastet er teke vare på."; }
        ChangedState();
    }
    public async Task LogoutAsync()
    {
        try { _error = null; var cancel = _login.CancelAsync(); await _authentication.LogoutAsync(); await cancel; }
        catch { _error = "Utlogginga kunne ikkje fullførast. Prøv igjen."; }
        ChangedState();
    }
    public async Task ResumeAsync()
    {
        _uploadInitializationFailed = false;
        try { _error = null; if (_workspace?.State.NeedsRetry == true) await _workspace.RetryAsync(); await _authentication.ResumeAsync(); }
        catch { _error = "Vi fekk ikkje kontakt. Utkast og ventande meldingar er tekne vare på."; }
        ChangedState();
    }
    public Task RetryPendingSendsAsync(string expectedUser, string expectedStamp)
    {
        var auth = Authentication;
        if (!_closing && !ConversationCovered && auth.Phase == NativeAuthPhase.Active && Chat.Connected && auth.UserId == expectedUser &&
            auth.SessionStamp == expectedStamp && auth.UserId == _account && SameSendFamily(_admittedFamily, expectedStamp) &&
            _workspace?.State.Outbox.FirstOrDefault(item => item.State is not ("rejected" or "acknowledged")) is { } item &&
            SendFamily(expectedStamp) is string family)
        {
            Interlocked.Exchange(ref _manualSendRetry, new(expectedUser, family, item.RequestId));
            _deliveryWake.Cancel(); _deliveryWakeTarget = null;
            Signal();
        }
        return Task.CompletedTask;
    }
    public Task SelectAsync(string channel) => NavigateAsync(new(channel));
    public Task NavigateAsync(NativeNavigationTarget target)
    {
        if (ConversationCovered) return Task.CompletedTask;
        SynchronizeNavigation();
        return NavigateVisitAsync(_navigation.Select(target));
    }
    public Task BackAsync() { if (ConversationCovered) return Task.CompletedTask; SynchronizeNavigation(); return NavigateVisitAsync(_navigation.Back()); }
    public Task ForwardAsync() { if (ConversationCovered) return Task.CompletedTask; SynchronizeNavigation(); return NavigateVisitAsync(_navigation.Forward()); }
    public void SetNavigationQuery(string query) => _navigation.SetQuery(query);
    public void SetNavigationScope(NativeNavigationDirectoryScope scope) => _navigation.SetDirectoryScope(scope);
    public void SetNavigationInteraction(bool interacting) => _navigation.SetInteraction(interacting);
    public void RememberViewport(NativeNavigationVisit visit, NativeScrollAnchor anchor) => _navigation.SaveAnchor(visit, anchor);

    private async Task NavigateVisitAsync(NativeNavigationVisit? visit)
    {
        if (visit is null || _closing || _workspace is null || !ShowConversation) return;
        var operation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        CancellationTokenSource? previous = null;
        CancellationTokenSource? previousVisit = null;
        if (!AdmitNavigation(visit, () =>
            {
                previous = Interlocked.Exchange(ref _navigationOperation, operation);
                previousVisit = Interlocked.Exchange(ref _conversationVisitCancellation, new());
            }))
        {
            operation.Dispose(); return;
        }
        try { previous?.Cancel(); } catch (ObjectDisposedException) { }
        previousVisit?.Cancel(); previousVisit?.Dispose();
        NativeThreadVisit? threadVisit = null;
        try
        {
            if (!AdmitNavigation(visit, () => { _threads.Close(); _workspace.EnsureChannels([visit.Target.ChannelId]); })) return;
            var before = Chat;
            if (_navigation.Capture().Pending || before.SelectedChannel != visit.Target.ChannelId || before.Loading || before.Error is not null)
                await _chat.SelectChannelAsync(visit.Target.ChannelId, operation.Token, expectedUserId: visit.UserId, expectedSessionStamp: visit.SessionStamp);
            if (!_navigation.IsCurrent(visit)) return;
            if (visit.Target.ThreadId is string rootId)
            {
                if (!AdmitNavigation(visit, () =>
                {
                    threadVisit = _threads.Open(visit.Target.ChannelId, rootId);
                    if (threadVisit is null) return;
                    _workspace.EnsureChannels([new ComposerTarget(visit.Target.ChannelId, rootId).Key]);
                    _threads.SaveAnchor(threadVisit, visit.Anchor);
                }) || threadVisit is null) return;
                var thread = await _chat.LoadThreadAsync(visit.Target.ChannelId, rootId, Chat.History!.ViewGeneration,
                    visit.UserId, visit.SessionStamp, operation.Token);
                if (!_navigation.IsCurrent(visit) || !_threads.IsCurrent(threadVisit)) return;
                _threads.ApplyThread(threadVisit, thread);
                bool found = visit.Target.MessageId is null && visit.Target.Sequence is null ||
                    (thread.Root is null ? thread.Replies : new[] { thread.Root }.Concat(thread.Replies)).Any(item =>
                        (visit.Target.MessageId is null || item.Id == visit.Target.MessageId) &&
                        (visit.Target.Sequence is null || item.Sequence == visit.Target.Sequence));
                if (thread.Error is not null || !found) _navigation.Fail(visit, thread.Error ?? "Meldinga finst ikkje i tråden.");
                else _navigation.Complete(visit);
                return;
            }
            // Recover an older saved position from contiguous pages before
            // enabling visible-read reporting. No jump over an unknown gap.
            var wanted = visit.Target.Sequence ?? (!visit.Anchor.AtEnd ? visit.Anchor.Sequence : null);
            var wantedMessage = visit.Target.MessageId ?? (!visit.Anchor.AtEnd ? visit.Anchor.MessageId : null);
            if (wanted.HasValue || wantedMessage is not null)
            {
                for (int page = 0; page < 200 && _navigation.IsCurrent(visit); page++)
                {
                    var current = Chat;
                    if ((wantedMessage is not null && current.Messages.Any(item => wantedMessage == item.Id)) ||
                        (wantedMessage is null && wanted is ulong known && current.Messages.Any(item => item.Sequence == known)) ||
                        current.History is not { HasOlder: true, AtCapacity: false, OldestSequence: { } oldest } history ||
                        (wanted is ulong sequence && oldest <= sequence)) break;
                    await _chat.LoadOlderAsync(visit.Target.ChannelId, history.ViewGeneration, visit.UserId, visit.SessionStamp, operation.Token);
                    if (Chat.History?.OldestSequence == oldest) break;
                }
            }
            bool precise = visit.Target.MessageId is not null || visit.Target.Sequence is not null;
            if (precise && !Chat.Messages.Any(item =>
                (visit.Target.MessageId is null || item.Id == visit.Target.MessageId) &&
                (visit.Target.Sequence is null || item.Sequence == visit.Target.Sequence)))
                _navigation.Fail(visit, Chat.History?.AtCapacity == true
                    ? "Meldinga ligg utanfor historikkgrensa i denne Windows-versjonen."
                    : "Meldinga er ikkje tilgjengeleg i samtalen.");
            else _navigation.Complete(visit);
        }
        catch (OperationCanceledException) when (operation.IsCancellationRequested) { }
        catch
        {
            AdmitNavigation(visit, () =>
            {
                if (threadVisit is not null) _threads.Fail(threadVisit, "Tråden kunne ikkje hentast. Prøv igjen.");
                _navigation.Fail(visit, "Samtalen kunne ikkje opnast. Oppdater sambandet og prøv igjen.");
            });
        }
        finally
        {
            Interlocked.CompareExchange(ref _navigationOperation, null, operation);
            operation.Dispose(); Notify();
        }
    }

    public async Task LoadOlderAsync(string channel, long viewGeneration, string expectedUser, string expectedStamp, NativeNavigationVisit visit)
    {
        var auth = Authentication;
        if (_closing || !_navigation.IsCurrent(visit) || auth.UserId != expectedUser || auth.SessionStamp != expectedStamp ||
            _navigation.Capture() is not { Pending: false, Error: null, Current: { } current } || current.Target.ChannelId != channel) return;
        try { await _chat.LoadOlderAsync(channel, viewGeneration, expectedUser, expectedStamp, _lifetime.Token); }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch { /* The scoped history snapshot retains its local retry/error. */ }
        Notify();
    }
    public async Task MarkVisibleReadAsync(string channel, long viewGeneration, IReadOnlyList<string> messageIds, string expectedUser, string expectedStamp, NativeNavigationVisit visit)
    {
        var scope = new NativeMessageActionScope(expectedUser, expectedStamp, channel, visit.Generation);
        try
        {
            using var operation = AdmitMessageAction(scope);
            await _chat.MarkVisibleReadAsync(channel, viewGeneration, messageIds, expectedUser, expectedStamp, operation.Token);
        }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch { /* Unconfirmed read progress remains unconfirmed and retryable. */ }
    }
    public void Edit(string channel, string body)
    {
        if (_closing || ConversationCovered || !ShowConversation || _workspace is null) return;
        _workspace.EnsureChannels([channel]); _workspace.EditDraft(channel, body);
    }
    public async Task SendAsync(string channel)
    {
        if (_closing || ConversationCovered || !ShowConversation || _workspace is null || Authentication.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) return;
        try
        {
            ApproveEmptyQueue();
            if (Capture().ConfirmPendingSends) return;
            _error = null; await _workspace.SendAsync(channel);
        }
        catch { _error = "Meldinga kunne ikkje leggjast i sendekøen. Teksten er teken vare på."; }
        ChangedState();
    }
    public Task ConfirmPendingSendsAsync(string expectedUser, string expectedStamp)
    {
        var auth = Authentication;
        if (!_closing && !ConversationCovered && auth.UserId == expectedUser && auth.SessionStamp == expectedStamp && auth.UserId == _account && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline)
        {
            try
            {
                if (SendFamily(auth.SessionStamp) is string family && _replayConsent is { } consent)
                    { consent.Approve(family); _admittedFamily = family; }
            }
            catch { _error = "Stadfestinga kunne ikkje lagrast. Ingen gamle kømeldingar blir sende."; }
        }
        ChangedState(); return Task.CompletedTask;
    }
    public async Task DiscardRejectedAsync(string expectedUser, string expectedStamp, string requestId)
    {
        var store = _store; var workspace = _workspace; var auth = Authentication;
        if (_closing || ConversationCovered || store is null || workspace is null || auth.UserId != expectedUser ||
            auth.SessionStamp != expectedStamp || _account != expectedUser ||
            auth.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline) ||
            !workspace.State.Outbox.Any(item => item.RequestId == requestId && item.State == "rejected")) return;
        try
        {
            // This confirmed local operation targets the captured account's
            // store. It needs no active network credential and can run offline.
            await store.DeliveryAsync(requestId, "discard_rejected");
            if (ReferenceEquals(workspace, _workspace) && Authentication.UserId == expectedUser)
            {
                workspace.ServerAcknowledged(requestId);
                if (_deliveryNotice?.RequestId == requestId) _deliveryNotice = null;
            }
        }
        catch
        {
            if (ReferenceEquals(workspace, _workspace) && Authentication.UserId == expectedUser)
                _deliveryNotice = new(expectedUser, SendFamily(expectedStamp), requestId, "Den avviste meldinga kunne ikkje fjernast. Teksten er teken vare på; prøv igjen.", Rejected: true);
        }
        ChangedState();
    }
    private static string? SendFamily(string? stamp)
    {
        if (stamp is null) return null;
        int last = stamp.LastIndexOf(':');
        return last > 0 ? stamp[..last] : null;
    }
    internal static bool SameSendFamily(string? admitted, string? stamp) => admitted is not null && admitted == SendFamily(stamp);
    private void ApproveEmptyQueue()
    {
        var auth = Authentication;
        if (auth.UserId != _account || auth.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline) ||
            _workspace is null || _workspace.State.Outbox.Any(item => item.State != "rejected") ||
            SendFamily(auth.SessionStamp) is not string family || family == _admittedFamily || _replayConsent is null) return;
        _replayConsent.Approve(family);
        _admittedFamily = family;
    }

    private async Task PumpAsync()
    {
        try
        {
            while (!_lifetime.IsCancellationRequested)
            {
                await _signal.WaitAsync(_lifetime.Token);
                try
                {
                var auth = Authentication;
                if (auth.UserId is string user && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline)
                {
                    if (user != _account || _workspace is null || !_workspace.State.Ready)
                    {
                        await CloseStoreAsync();
                        var store = await NativeSession.OpenAsync(Path.Combine(_directory, "account-" + user + ".sqlite"), Program.LibraryPath, user, clearEmptyDraft: true);
                        var snapshot = await store.SnapshotAsync();
                        var channels = Chat.Channels.Select(item => item.Id).Concat(snapshot.Drafts.Select(item => item.Channel))
                            .Concat(snapshot.Outbox.Select(item => item.Channel)).Append("__pending__").Distinct().Take(512).ToArray();
                        _store = store; _account = user;
                        _replayConsent = new(Path.Combine(_directory, "send-consent-" + user), _instance, user);
                        _admittedFamily = _replayConsent.Family;
                        _workspace = new(new NativeWorkspaceStoreAdapter(store), channels, maximumChannels: 512);
                        await _workspace.InitializeAsync();
                        _workspace.Changed += ChangedState;
                    }
                    ApproveEmptyQueue();
                    if (_uploads is null && !_uploadInitializationFailed) await InitializeUploadsAsync(user, _workspace!);
                    BindUploadScope();
                    // The transport resumes stored credentials itself. Start it
                    // for an offline restored session too, before window activation.
                    if (_connection is null) _connection = ConnectAsync();
                    _workspace!.EnsureChannels(Chat.Channels.Select(item => item.Id));
                    if (Chat.Connected && !Chat.Loading && Chat.SelectedChannel is null && Chat.Channels.FirstOrDefault() is { } first)
                        await _chat.SelectChannelAsync(first.Id, _lifetime.Token);
                    if (Authentication.Phase == NativeAuthPhase.Active && Chat.Connected && Authentication.UserId == _account)
                        await DeliverAsync();
                }
                Notify();
                }
                catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { throw; }
                catch { _error = "Vi fekk ikkje oppdatert samtalen. Utkastet er teke vare på."; Notify(); }
            }
        }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch { _error = "Vi fekk ikkje oppdatert samtalen. Utkastet er teke vare på."; Notify(); }
    }
    private async Task ConnectAsync()
    {
        try { await _chat.StartAsync(_lifetime.Token); }
        catch (OperationCanceledException) when (_lifetime.IsCancellationRequested) { }
        catch { _error = "Samtalen ventar på nett. Prøv igjen."; Notify(); }
    }
    private async Task DeliverAsync()
    {
        var store = _store!; var workspace = _workspace!; string account = _account!;
        var admission = Authentication;
        if (admission.UserId != account || admission.SessionStamp is not string stamp || !SameSendFamily(_admittedFamily, stamp)) return;
        var published = workspace.State.Outbox.Select(item => item.RequestId).ToHashSet();
        var snapshot = await store.SnapshotAsync();
        if (!ReferenceEquals(workspace, _workspace) || Authentication.UserId != account) return;
        _delivery.Reconcile(snapshot.Outbox);
        var settled = ReconcileDeliveries(workspace, published, snapshot.Outbox);
        if (_deliveryWakeTarget is { } wake && settled.Contains(wake.RequestId)) CompleteDeliveryWake(wake.RequestId);
        if (_deliveryNotice is { } issue && settled.Contains(issue.RequestId) &&
            (!issue.Rejected || !snapshot.Outbox.Any(item => item.RequestId == issue.RequestId)))
            _deliveryNotice = null;
        foreach (var item in snapshot.Outbox.Where(item => published.Contains(item.RequestId)))
        {
            if (Authentication.UserId != account || Authentication.Phase != NativeAuthPhase.Active || !Chat.Connected) break;
            if (item.State == "rejected") continue;
            try
            {
                bool Admitted()
                {
                    var current = Authentication;
                    return current.UserId == account && current.Phase == NativeAuthPhase.Active &&
                        SameSendFamily(_admittedFamily, current.SessionStamp) && Chat.Connected;
                }
                var result = await _delivery.DispatchAsync(item, store.DeliveryAsync,
                    () => _chat.SendAsync(item.Channel, item.Body, item.RequestId, account, stamp, _lifetime.Token, item.ParentMessageId),
                    Admitted, _lifetime.Token, ConsumeManualRetry(item.RequestId, account, stamp));
                if (result.Outcome == LiveDeliveryOutcome.Acknowledged)
                {
                    CompleteDeliveryWake(item.RequestId);
                    if (_deliveryNotice?.RequestId == item.RequestId) _deliveryNotice = null;
                    workspace.ServerAcknowledged(item.RequestId);
                }
                else
                {
                    if (result.Outcome == LiveDeliveryOutcome.Rejected)
                    {
                        CompleteDeliveryWake(item.RequestId);
                        // Refresh the same store owner so the UI never labels a
                        // terminal server rejection as waiting for a receipt.
                        workspace.ServerRejected(item.RequestId);
                        _deliveryNotice = new(account, SendFamily(stamp), item.RequestId, result.Message is { Length: > 0 } message
                            ? "Meldinga vart ikkje send: " + message
                            : "Meldinga vart avvist. Teksten er teken vare på og blir ikkje send automatisk.", Rejected: true);
                    }
                    else if (result.Outcome == LiveDeliveryOutcome.Uncertain)
                        _deliveryNotice = new(account, SendFamily(stamp), item.RequestId, "Vi manglar sendekvittering. Meldinga ligg trygt i kø med same ID.");
                    if (result.RetryAfter is { } retryAfter) ScheduleDeliveryWake(account, stamp, item.RequestId, retryAfter);
                    Notify();
                    if (result.Outcome != LiveDeliveryOutcome.Rejected) break;
                }
            }
            catch (LiveDeliveryPersistenceException failure)
            {
                _deliveryNotice = new(account, SendFamily(stamp), item.RequestId, "Sendestatusen kunne ikkje lagrast. Meldinga er teken vare på; vi prøver igjen.");
                ScheduleDeliveryWake(account, stamp, item.RequestId, failure.RetryAfter);
                Notify(); break;
            }
            catch
            {
                _deliveryNotice = new(account, SendFamily(stamp), item.RequestId, "Sendestatusen kunne ikkje lagrast. Meldinga er teken vare på; prøv igjen.");
                Notify(); break;
            }
        }
    }
    private bool ConsumeManualRetry(string requestId, string user, string stamp)
    {
        var manual = Volatile.Read(ref _manualSendRetry);
        return manual is not null && manual.UserId == user && manual.RequestId == requestId &&
            manual.Family == SendFamily(stamp) && ReferenceEquals(Interlocked.CompareExchange(ref _manualSendRetry, null, manual), manual);
    }
    internal static IReadOnlySet<string> ReconcileDeliveries(ChatWorkspace workspace, IReadOnlySet<string> published, IReadOnlyList<QueuedMessage> persisted)
    {
        // Only reconcile IDs visible before the store snapshot was requested.
        // A newer local enqueue must not be removed by an older snapshot.
        var durable = persisted.ToDictionary(item => item.RequestId);
        var settled = new HashSet<string>();
        foreach (string id in published)
        {
            if (!durable.TryGetValue(id, out var item)) workspace.ServerAcknowledged(id);
            else if (item.State == "rejected") workspace.ServerRejected(id);
            else continue;
            settled.Add(id);
        }
        return settled;
    }
    private async Task CloseStoreAsync()
    {
        await CloseUploadsAsync();
        CancelDeliveryWake();
        _deliveryNotice = null;
        if (_workspace is not null) { _workspace.Changed -= ChangedState; await _workspace.ShutdownAsync(); }
        else if (_store is not null) await _store.DisposeAsync();
        _replayConsent?.Dispose(); _replayConsent = null; _admittedFamily = null;
        _delivery = new();
        _workspace = null; _store = null; _account = null;
    }
    public async ValueTask DisposeAsync()
    {
        if (_closing) return;
        _closing = true; _lifetime.Cancel();
        _conversationVisitCancellation.Cancel(); _conversationVisitCancellation.Dispose();
        var navigation = Interlocked.Exchange(ref _navigationOperation, null);
        try { navigation?.Cancel(); } catch (ObjectDisposedException) { }
        _deliveryWake.Dispose();
        Media.Dispose(); await _login.DisposeAsync(); await _chat.DisposeAsync();
        await _pump; await CloseStoreAsync();
        _authentication.Changed -= ChangedState; _login.Changed -= ChangedState; _chat.Changed -= ChangedState;
        _navigation.Changed -= Notify;
        _threads.Changed -= Notify;
        _authentication.Dispose(); _lifetime.Dispose();
    }
}
