using System.Net.Http.Json;
using System.Text;
using System.Text.Json;

namespace Sproyt.WindowsProbe;

// Coordinates the existing durable draft/outbox owner with the live transport.
// No token or provider credential is projected into the UI.
internal sealed class LiveApplication : ILiveApplication
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
    public event Action? Changed;
    public NativeAuthSnapshot Authentication => _authentication.Snapshot;
    public NativeChatSnapshot Chat => _chat.Snapshot;
    public WorkspaceSnapshot? Workspace => _workspace?.State;
    public NativeMediaLibrary Media { get; }
    internal string? Error => _error ?? Chat.Error ?? Workspace?.ErrorMessage;
    internal bool HasAccount => _workspace is not null && Authentication.UserId == _account;
    internal bool ShowConversation => HasAccount && Authentication.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline;
    public LiveSnapshot Capture()
    {
        var auth = Authentication; var workspace = _workspace; string? account = _account;
        var local = workspace?.State; var chat = Chat; var after = Authentication;
        bool same = ReferenceEquals(workspace, _workspace) && auth.UserId is not null && auth.UserId == account &&
            auth.UserId == after.UserId && auth.Phase == after.Phase && chat.UserId == auth.UserId;
        return new(auth, chat, same ? local : null, same && local?.Ready == true && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline, Error,
            same && local?.Outbox.Any(item => item.State != "rejected") == true && !SameSendFamily(_admittedFamily, auth.SessionStamp));
    }

    private sealed record Authority(string Broker, string Instance, string Issuer);
    private LiveApplication(string directory, NativeAuthSession owner, NativeAuthConfig config)
    {
        _directory = directory; _authentication = owner;
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
    private void ChangedState() { Signal(); Notify(); }

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
        try { _error = null; if (_workspace?.State.NeedsRetry == true) await _workspace.RetryAsync(); await _authentication.ResumeAsync(); }
        catch { _error = "Vi fekk ikkje kontakt. Utkast og ventande meldingar er tekne vare på."; }
        ChangedState();
    }
    public async Task SelectAsync(string channel)
    {
        if (_workspace is null || !ShowConversation || !Chat.Channels.Any(item => item.Id == channel)) return;
        _workspace.EnsureChannels([channel]);
        try { _error = null; await _chat.SelectChannelAsync(channel, _lifetime.Token); }
        catch { _error = "Kanalen kunne ikkje lastast. Prøv igjen."; }
        ChangedState();
    }
    public void Edit(string channel, string body)
    {
        if (_closing || !ShowConversation || _workspace is null) return;
        _workspace.EnsureChannels([channel]); _workspace.EditDraft(channel, body);
    }
    public async Task SendAsync(string channel)
    {
        if (_closing || !ShowConversation || _workspace is null || Authentication.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) return;
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
        if (!_closing && auth.UserId == expectedUser && auth.SessionStamp == expectedStamp && auth.UserId == _account && auth.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline)
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
        if (_closing || store is null || workspace is null || auth.UserId != expectedUser ||
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
                _error = null;
            }
        }
        catch
        {
            if (ReferenceEquals(workspace, _workspace) && Authentication.UserId == expectedUser)
                _error = "Den avviste meldinga kunne ikkje fjernast. Teksten er teken vare på; prøv igjen.";
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
        ReconcileDeliveries(workspace, published, snapshot.Outbox);
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
                    () => _chat.SendAsync(item.Channel, item.Body, item.RequestId, account, stamp, _lifetime.Token),
                    Admitted, _lifetime.Token);
                if (result.Outcome == LiveDeliveryOutcome.Acknowledged)
                {
                    _error = null;
                    workspace.ServerAcknowledged(item.RequestId);
                }
                else
                {
                    if (result.Outcome == LiveDeliveryOutcome.Rejected)
                    {
                        // Refresh the same store owner so the UI never labels a
                        // terminal server rejection as waiting for a receipt.
                        workspace.ServerRejected(item.RequestId);
                        _error = result.Message is { Length: > 0 } message
                            ? "Meldinga vart ikkje send: " + message
                            : "Meldinga vart avvist. Teksten er teken vare på og blir ikkje send automatisk.";
                    }
                    else if (result.Outcome == LiveDeliveryOutcome.Uncertain)
                        _error = "Vi manglar sendekvittering. Meldinga ligg trygt i kø med same ID.";
                    Notify();
                    if (result.Outcome != LiveDeliveryOutcome.Rejected) break;
                }
            }
            catch
            {
                _error = "Sendestatusen kunne ikkje lagrast. Meldinga er teken vare på; prøv igjen.";
                Notify(); break;
            }
        }
    }
    internal static void ReconcileDeliveries(ChatWorkspace workspace, IReadOnlySet<string> published, IReadOnlyList<QueuedMessage> persisted)
    {
        // Only reconcile IDs visible before the store snapshot was requested.
        // A newer local enqueue must not be removed by an older snapshot.
        var durable = persisted.ToDictionary(item => item.RequestId);
        foreach (string id in published)
        {
            if (!durable.TryGetValue(id, out var item)) workspace.ServerAcknowledged(id);
            else if (item.State == "rejected") workspace.ServerRejected(id);
        }
    }
    private async Task CloseStoreAsync()
    {
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
        Media.Dispose(); await _login.DisposeAsync(); await _chat.DisposeAsync();
        await _pump; await CloseStoreAsync();
        _authentication.Changed -= ChangedState; _login.Changed -= ChangedState; _chat.Changed -= ChangedState;
        _authentication.Dispose(); _lifetime.Dispose();
    }
}
