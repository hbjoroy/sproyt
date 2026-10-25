using System.Text.Json;
using System.Net.Http.Json;

namespace Sproyt.WindowsProbe;

// The UI sees only state and actions. The shared owner alone retains credentials.
internal sealed class NativeAuthentication : IAsyncDisposable
{
    private readonly DesktopConfig _desktop;
    private readonly string _directory;
    private readonly NativeAuthSession _owner;
    private readonly CancellationTokenSource _lifetime = new();
    private readonly WindowsProtectedJournal _cancellations;
    private readonly SemaphoreSlim _actions = new(1, 1);
    private readonly SemaphoreSlim _cleanup = new(1, 1);
    private CancellationQueue _pending;
    private sealed record CancellationQueue(int Schema, string Instance, string UserId, long[] Tickets);
    private Task? _login;
    private CancellationTokenSource? _loginCancellation;
    private bool _disposed;
    internal string UserId { get; }
    internal NativeAuthSnapshot Snapshot
    {
        get { var snapshot = _owner.Snapshot; return snapshot with { PendingRevocation = snapshot.PendingRevocation || _pending.Tickets.Length != 0 }; }
    }
    internal string? Notice { get; private set; }
    internal event Action? Changed;
    internal Func<Task>? BeforeLoginAdmissionForProbe { get; set; }

    internal NativeAuthentication(string configPath, string directory)
    {
        if (new FileInfo(configPath).Length is <= 0 or > 4096) throw new InvalidOperationException("Invalid session laboratory configuration.");
        _desktop = JsonSerializer.Deserialize<DesktopConfig>(File.ReadAllText(configPath), DesktopHandoffClient.Json)
            ?? throw new InvalidOperationException("Missing laboratory configuration.");
        var broker = DesktopHandoffClient.Loopback(_desktop.Broker, "/");
        DesktopHandoffClient.Loopback(_desktop.Callback, "/return");
        // This opt-in shell cannot point at Authentik or production Sprøyt.
        if (!_desktop.SessionLab || _desktop.Issuer is not null || _desktop.Subjects is not null ||
            _desktop.Accounts.Length != 1 || _desktop.Accounts[0] != "alice")
            throw new InvalidOperationException("Only the synthetic Alice laboratory is supported.");
        string issuer = broker.GetLeftPart(UriPartial.Authority) + "/provider";
        UserId = DesktopHandoffClient.UserId(issuer + ":alice").ToString();
        _directory = Path.GetFullPath(directory);
        _owner = NativeAuthSession.Open(Path.Combine(_directory, "owner"),
            new(_desktop.Broker, _desktop.Instance, issuer, UserId));
        try
        {
            string cancels = Path.Combine(_directory, "cancelled-logins");
            bool exists = File.Exists(Path.Combine(cancels, "state.bin"));
            _cancellations = new(cancels, create: !exists);
            _pending = exists ? _cancellations.Read<CancellationQueue>() : new(1, _desktop.Instance, UserId, []);
            if (_pending.Schema != 1 || _pending.Instance != _desktop.Instance || _pending.UserId != UserId ||
                _pending.Tickets.Length > 32 || _pending.Tickets.Any(ticket => ticket < 0) || _pending.Tickets.Distinct().Count() != _pending.Tickets.Length)
                throw new InvalidOperationException("Invalid pending login cancellations.");
            if (!exists) _cancellations.Write(_pending);
        }
        catch { _cancellations?.Dispose(); _owner.Dispose(); throw; }
        _owner.Changed += Publish;
    }

    private void Publish() { if (!_disposed) Changed?.Invoke(); }

    internal Task LoginAsync()
    {
        if (_disposed) return Task.CompletedTask;
        if (_login is { IsCompleted: false }) return _login;
        return _login = LoginCoreAsync();
    }

    private async Task LoginCoreAsync()
    {
        using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(_lifetime.Token);
        _loginCancellation = cancellation;
        try
        {
            Notice = null;
            await RetryCancellationsAsync();
            if (_pending.Tickets.Length != 0) throw new InvalidOperationException("A cancelled login still awaits its server acknowledgement.");
            long ticket;
            if (Program.SessionUiProbeDirectory is not null && BeforeLoginAdmissionForProbe is not null)
                await BeforeLoginAdmissionForProbe();
            await _actions.WaitAsync();
            try
            {
                cancellation.Token.ThrowIfCancellationRequested();
                if (_disposed || _pending.Tickets.Length != 0) throw new InvalidOperationException("Login admission closed.");
                ticket = _owner.Snapshot.LoginTicket ?? await _owner.BeginLoginAsync();
            }
            finally { _actions.Release(); }
            string handoff = Path.Combine(_directory, "handoff-" + ticket.ToString(System.Globalization.CultureInfo.InvariantCulture));
            Directory.CreateDirectory(handoff);
            string settings = Path.Combine(handoff, "configuration.json");
            await File.WriteAllTextAsync(settings, JsonSerializer.Serialize(_desktop, DesktopHandoffClient.Json), cancellation.Token);
            Action<Uri>? testBrowser = Program.SessionUiProbeDirectory is null ? null : address =>
            {
                _ = SessionLabBrowser.FollowAsync(address, new Uri(_desktop.Broker), new Uri(_desktop.Callback), cancellation.Token)
                    .ContinueWith(task => { if (task.IsFaulted) cancellation.Cancel(); }, TaskScheduler.Default);
            };
            int result = await DesktopHandoffClient.Run(handoff, settings, "none", cancellation.Token, testBrowser);
            if (result != 0) throw new InvalidOperationException("Login did not complete.");
            using var journal = new WindowsProtectedJournal(handoff, create: false);
            var state = journal.Read<DesktopState>();
            if (state.Attempts.Length != 1 || state.Attempts[0].Result is not string exchange)
                throw new InvalidOperationException("Login did not produce a protected receipt.");
            await _owner.CompleteLoginAsync(ticket, exchange);
        }
        catch (OperationCanceledException) when (cancellation.IsCancellationRequested) { }
        catch
        {
            Notice = "Innlogginga vart ikkje fullført. Prøv igjen; utkastet ditt er teke vare på.";
            if (!_disposed && _owner.Snapshot.Phase == NativeAuthPhase.SigningIn)
                await LogoutAsync();
            Publish();
        }
        finally
        {
            if (ReferenceEquals(_loginCancellation, cancellation)) _loginCancellation = null;
            await RetryCancellationsAsync();
        }
    }

    internal async Task ResumeAsync()
    {
        if (_disposed) return;
        try { Notice = null; await RetryCancellationsAsync(); await _owner.ResumeAsync(); }
        catch { Notice = "Vi fekk ikkje kontrollert økta. Utkastet ditt er teke vare på."; Publish(); }
    }

    internal async Task LogoutAsync()
    {
        if (_disposed) return;
        try
        {
            Notice = null;
            await _actions.WaitAsync();
            try
            {
                if (_owner.Snapshot.LoginTicket is long ticket && !_pending.Tickets.Contains(ticket))
                {
                    if (_pending.Tickets.Length == 32) throw new InvalidOperationException("Pending cancellation capacity reached.");
                    var next = _pending with { Tickets = [.. _pending.Tickets, ticket] };
                    _cancellations.Write(next); _pending = next;
                }
                // Durable cancellation intent precedes the local logout fence.
                var logout = _owner.LogoutAsync();
                _loginCancellation?.Cancel();
                await logout;
            }
            finally { _actions.Release(); }
            await RetryCancellationsAsync();
        }
        catch { Notice = "Vi fekk ikkje fullført utlogginga. Prøv igjen før du forlèt appen."; Publish(); }
    }

    private async Task RetryCancellationsAsync()
    {
        if (_disposed) return;
        await _cleanup.WaitAsync();
        try
        {
            foreach (long ticket in _pending.Tickets.ToArray())
            {
                string handoff = Path.Combine(_directory, "handoff-" + ticket.ToString(System.Globalization.CultureInfo.InvariantCulture));
                try
                {
                    // A live helper holds this journal exclusively. Retry after it exits.
                    if (Directory.Exists(handoff) && File.Exists(Path.Combine(handoff, "state.bin")))
                    {
                        using var journal = new WindowsProtectedJournal(handoff, create: false);
                        var state = journal.Read<DesktopState>();
                        if (state.Environment != "n01-lab" || JsonSerializer.Serialize(state.Config, DesktopHandoffClient.Json) !=
                            JsonSerializer.Serialize(_desktop, DesktopHandoffClient.Json)) throw new InvalidOperationException("Cancellation binding mismatch.");
                        using var http = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false })
                            { BaseAddress = new Uri(_desktop.Broker), Timeout = TimeSpan.FromSeconds(12) };
                        foreach (var attempt in state.Attempts)
                        {
                            using var response = await http.PostAsJsonAsync("/session/cancel-login", new
                            {
                                transaction_id = attempt.Transaction, verifier = attempt.Verifier,
                                environment = "n01-lab", destination_id = "windows-loopback",
                            });
                            if (response.StatusCode != System.Net.HttpStatusCode.NoContent) throw new InvalidOperationException("Cancellation not acknowledged.");
                        }
                    }
                    else if (_login is { IsCompleted: false }) continue;
                    await _actions.WaitAsync();
                    try
                    {
                        var next = _pending with { Tickets = _pending.Tickets.Where(value => value != ticket).ToArray() };
                        _cancellations.Write(next); _pending = next;
                    }
                    finally { _actions.Release(); }
                    Publish();
                }
                catch { /* Retain the intent and proof until a positive server acknowledgement. */ }
            }
        }
        finally { _cleanup.Release(); }
    }

    public async ValueTask DisposeAsync()
    {
        if (_disposed) return;
        _disposed = true;
        _lifetime.Cancel();
        try { if (_login is not null) await _login.WaitAsync(TimeSpan.FromSeconds(20)); }
        finally
        {
            await _cleanup.WaitAsync();
            try { _owner.Changed -= Publish; _owner.Dispose(); _cancellations.Dispose(); _lifetime.Dispose(); }
            finally { _cleanup.Release(); }
        }
    }
}
