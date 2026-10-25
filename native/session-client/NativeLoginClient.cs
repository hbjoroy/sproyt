using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

// The login journal owns proofs, callback binding, and cancellation work. The
// session journal owns accepted credentials. Neither is shared with a browser.
internal sealed class NativeLoginClient : IAsyncDisposable
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private readonly object gate = new();
    private readonly SemaphoreSlim loginGate = new(1, 1);
    private readonly SemaphoreSlim cancelGate = new(1, 1);
    private readonly NativeAuthSession owner;
    private readonly NativeAuthConfig config;
    private readonly WindowsProtectedJournal journal;
    private readonly HttpClient http;
    private readonly Action<Uri> openBrowser;
    private readonly CancellationTokenSource lifetime = new();
    private CancellationTokenSource? inFlight;
    private Stored state;
    private bool disposed;
    private bool failed;
    internal event Action? Changed;
    private sealed class Attempt
    {
        public long Ticket { get; set; }
        public int CallbackPort { get; set; }
        public string Verifier { get; set; } = "";
        public string Redemption { get; set; } = "";
        public string? Transaction { get; set; }
        public string? Code { get; set; }
        public string? Result { get; set; }
        public bool CancelRequested { get; set; }
        public long Expires { get; set; }
    }
    private sealed class Stored
    {
        public int Schema { get; set; } = 1;
        public NativeAuthConfig Config { get; set; } = new("", "", "", "");
        public Attempt? Attempt { get; set; }
    }
    internal NativeLoginClient(NativeAuthSession owner, NativeAuthConfig config, string directory, HttpMessageHandler? handler = null, Action<Uri>? openBrowser = null)
    {
        WindowsProtectedJournal? acquired = null;
        try
        {
            NativeAuthSession.ValidateConfig(config); NativeClientWire.Require(config.Scope == "sproyt_native_v1");
            this.owner = owner; this.config = config;
            acquired = new WindowsProtectedJournal(directory, create: !File.Exists(Path.Combine(directory, "state.bin")));
            state = File.Exists(Path.Combine(directory, "state.bin")) ? acquired.Read<Stored>() : new Stored { Config = config };
            NativeClientWire.Require(state.Schema == 1 && state.Config == config);
            if (state.Attempt is { } attempt)
            {
                NativeClientWire.Require(attempt.Ticket > 0 && attempt.CallbackPort is > 0 and <= 65535 && NativeClientWire.Proof(attempt.Verifier) && Guid.TryParseExact(attempt.Redemption, "N", out _) &&
                    (attempt.Transaction is null || NativeClientWire.Proof(attempt.Transaction)) && (attempt.Code is null || (attempt.Transaction is not null && NativeClientWire.Proof(attempt.Code))) &&
                    (attempt.Result is null || (attempt.Code is not null && Encoding.UTF8.GetByteCount(attempt.Result) <= 8192)));
            }
            acquired.Write(state); journal = acquired; acquired = null;
            http = new HttpClient(handler ?? new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false }) { BaseAddress = new Uri(config.Broker), Timeout = TimeSpan.FromSeconds(15) };
            this.openBrowser = openBrowser ?? (uri => { using var browser = Process.Start(new ProcessStartInfo(uri.AbsoluteUri) { UseShellExecute = true }); });
        }
        catch { acquired?.Dispose(); throw NativeClientWire.Failure(); }
    }
    internal bool IsPending { get { lock (gate) return state.Attempt is not null; } }
    private void Notify() { try { Changed?.Invoke(); } catch { } }
    private void Live() { if (disposed || failed) throw NativeClientWire.Failure(); }
    private void Save() { try { journal.Write(state); } catch { failed = true; throw NativeClientWire.Failure(); } }
    private bool Current(Attempt attempt) => !disposed && !failed && ReferenceEquals(state.Attempt, attempt);
    internal Task LoginAsync(CancellationToken cancellationToken = default) => RunAsync(create: true, cancellationToken);
    internal Task ResumePendingAsync(CancellationToken cancellationToken = default) => RunAsync(create: false, cancellationToken);

    private async Task RunAsync(bool create, CancellationToken cancellationToken)
    {
        await loginGate.WaitAsync(cancellationToken);
        TcpListener? listener = null; Attempt? attempt = null;
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, lifetime.Token);
        linked.CancelAfter(TimeSpan.FromMinutes(5));
        try
        {
            lock (gate)
            {
                Live(); attempt = state.Attempt;
                // Acquiring loginGate proves no registration response is still in flight.
                if (attempt is { CancelRequested: true, Transaction: null }) { state.Attempt = null; Save(); attempt = null; }
                inFlight = linked;
            }
            if (attempt is { CancelRequested: true }) { await CancelAsync(cancellationToken); if (!create || IsPending) return; attempt = null; }
            await ValidateServerAsync(linked.Token);
            if (attempt is null)
            {
                if (!create) return;
                listener = new TcpListener(IPAddress.Loopback, 0); listener.Start();
                var ticket = owner.Snapshot.LoginTicket ?? await owner.BeginLoginAsync(linked.Token);
                linked.Token.ThrowIfCancellationRequested();
                attempt = new Attempt { Ticket = ticket, CallbackPort = ((IPEndPoint)listener.LocalEndpoint).Port,
                    Verifier = Random(), Redemption = Guid.NewGuid().ToString("N"), Expires = DateTimeOffset.UtcNow.AddMinutes(5).ToUnixTimeSeconds() };
                // Proof and the exact listener binding are durable before registering an attempt.
                lock (gate) { Live(); NativeClientWire.Require(state.Attempt is null); state.Attempt = attempt; Save(); }
                Notify();
            }
            if (attempt.Result is not null)
            {
                // A crash after owner commit must not cancel an already accepted family.
                await owner.CompleteLoginAsync(attempt.Ticket, attempt.Result);
                if (owner.Snapshot.Phase == NativeAuthPhase.Offline) await owner.ResumeAsync();
                Clear(attempt); return;
            }
            if (owner.Snapshot.LoginTicket != attempt.Ticket || attempt.Expires <= DateTimeOffset.UtcNow.ToUnixTimeSeconds())
            { await CancelAsync(cancellationToken); return; }
            if (attempt.Code is null && listener is null)
            {
                try { listener = new TcpListener(IPAddress.Loopback, attempt.CallbackPort); listener.Start(); }
                catch { await CancelAsync(cancellationToken); throw NativeClientWire.Failure(); }
            }
            if (attempt.Transaction is null)
            {
                linked.Token.ThrowIfCancellationRequested();
                var start = new Dictionary<string, object> { ["challenge"] = Challenge(attempt.Verifier), ["destination_id"] = "windows-loopback", ["callback_port"] = attempt.CallbackPort };
                if (config.UserId != "") start["expected_user_id"] = config.UserId;
                using var response = await http.PostAsJsonAsync("/api/v1/native/login", start, Json, linked.Token);
                NativeClientWire.Require(response.StatusCode == HttpStatusCode.OK);
                using var document = JsonDocument.Parse(await NativeClientWire.ReadAsync(response, 8192, linked.Token));
                string transaction = document.RootElement.GetProperty("transaction_id").GetString()!;
                var authorization = new Uri(document.RootElement.GetProperty("authorization_url").GetString()!, UriKind.Absolute);
                NativeClientWire.Require(NativeClientWire.Proof(transaction) && authorization.AbsoluteUri == config.Broker.TrimEnd('/') + "/auth/native/browser/" + transaction);
                lock (gate) { Live(); NativeClientWire.Require(Current(attempt)); attempt.Transaction = transaction; Save(); }
                // A cancellation racing the response must keep the proof and must never open a browser.
                if (attempt.CancelRequested || linked.IsCancellationRequested) { await CancelAsync(CancellationToken.None); return; }
                OpenBrowser(attempt, authorization, linked.Token);
            }
            // An existing system browser may still finish its callback. The
            // server's one-time browser entry must never be reopened on resume.
            if (attempt.Code is null)
            {
                string code = await ReceiveReturn(listener!, attempt, linked.Token);
                lock (gate) { Live(); NativeClientWire.Require(Current(attempt)); attempt.Code = code; Save(); }
            }
            linked.Token.ThrowIfCancellationRequested();
            using var exchange = await http.PostAsJsonAsync("/api/v1/native/exchange", new { transaction_id = attempt.Transaction, code = attempt.Code, verifier = attempt.Verifier, redemption_id = attempt.Redemption, environment = config.Environment, destination_id = "windows-loopback" }, Json, linked.Token);
            NativeClientWire.Require(exchange.StatusCode == HttpStatusCode.OK);
            string result = await NativeClientWire.ReadAsync(exchange, 8192, linked.Token);
            lock (gate) { Live(); NativeClientWire.Require(Current(attempt)); attempt.Result = result; Save(); }
            // Even a stale owner ticket receives the result so its family is durably revoked.
            await owner.CompleteLoginAsync(attempt.Ticket, result);
            if (attempt.CancelRequested) await CancelAsync(CancellationToken.None); else Clear(attempt);
        }
        catch (OperationCanceledException) when (linked.IsCancellationRequested) { throw new OperationCanceledException(cancellationToken); }
        catch { throw NativeClientWire.Failure(); }
        finally
        {
            listener?.Stop(); lock (gate) if (ReferenceEquals(inFlight, linked)) inFlight = null;
            loginGate.Release(); Notify();
        }
    }
    private void Clear(Attempt attempt)
    {
        lock (gate) { Live(); if (!Current(attempt) || attempt.CancelRequested) return; state.Attempt = null; Save(); }
        Notify();
    }
    private void OpenBrowser(Attempt attempt, Uri authorization, CancellationToken cancellationToken)
    {
        lock (gate) { Live(); NativeClientWire.Require(Current(attempt) && !attempt.CancelRequested); cancellationToken.ThrowIfCancellationRequested(); openBrowser(authorization); }
    }
    private async Task ValidateServerAsync(CancellationToken cancellationToken)
    {
        using var response = await http.GetAsync("/api/v1/native/info", HttpCompletionOption.ResponseHeadersRead, cancellationToken);
        NativeClientWire.Require(response.StatusCode == HttpStatusCode.OK);
        using var document = JsonDocument.Parse(await NativeClientWire.ReadAsync(response, 8192, cancellationToken)); var root = document.RootElement;
        NativeClientWire.Require(root.GetProperty("scope").GetString() == config.Scope && root.GetProperty("environment").GetString() == config.Environment && root.GetProperty("instance_id").GetString() == config.Instance && root.GetProperty("issuer").GetString() == config.Issuer);
    }
    internal async Task CancelAsync(CancellationToken cancellationToken = default)
    {
        Attempt? attempt;
        lock (gate)
        {
            Live(); attempt = state.Attempt; if (attempt is null) { inFlight?.Cancel(); return; }
            attempt.CancelRequested = true; Save(); inFlight?.Cancel();
        }
        Notify(); await cancelGate.WaitAsync(cancellationToken);
        try
        {
            lock (gate) { Live(); if (!Current(attempt)) return; }
            if (attempt.Transaction is null)
            {
                // Do not clear while login registration can still return a transaction.
                lock (gate) { if (inFlight is not null) return; state.Attempt = null; Save(); }
                return;
            }
            using var response = await http.PostAsJsonAsync("/api/v1/native/cancel-login", new { transaction_id = attempt.Transaction, verifier = attempt.Verifier, environment = config.Environment, destination_id = "windows-loopback" }, Json, cancellationToken);
            if (response.StatusCode != HttpStatusCode.NoContent) return;
            if (attempt.Result is not null) await owner.CompleteLoginAsync(attempt.Ticket, attempt.Result);
            lock (gate) { if (!Current(attempt)) return; state.Attempt = null; Save(); }
        }
        catch (Exception error) when (error is HttpRequestException or IOException or OperationCanceledException) { }
        catch { throw NativeClientWire.Failure(); }
        finally { cancelGate.Release(); Notify(); }
    }
    private static string Random() => Convert.ToBase64String(RandomNumberGenerator.GetBytes(32)).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    private static string Challenge(string verifier) => Convert.ToBase64String(SHA256.HashData(Encoding.ASCII.GetBytes(verifier))).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    private static async Task<string> ReceiveReturn(TcpListener listener, Attempt attempt, CancellationToken cancellationToken)
    {
        while (true)
        {
            using var client = await listener.AcceptTcpClientAsync(cancellationToken);
            using var stream = client.GetStream(); using var requestTimeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken); requestTimeout.CancelAfter(TimeSpan.FromSeconds(4));
            string? code = null;
            try
            {
                using var bytes = new MemoryStream(); var unit = new byte[1];
                while (bytes.Length < 8192)
                {
                    if (await stream.ReadAsync(unit, requestTimeout.Token) == 0) break; bytes.WriteByte(unit[0]);
                    var buffer = bytes.GetBuffer(); int n = (int)bytes.Length;
                    if (n >= 4 && buffer[n - 4] == 13 && buffer[n - 3] == 10 && buffer[n - 2] == 13 && buffer[n - 1] == 10) break;
                }
                string header = Encoding.ASCII.GetString(bytes.ToArray()); string[] lines = header.Split("\r\n", StringSplitOptions.None); string[] first = lines[0].Split(' ');
                if (first.Length == 3 && first[0] == "GET" && first[2] == "HTTP/1.1" && lines.Count(line => line.StartsWith("Host:", StringComparison.OrdinalIgnoreCase)) == 1 &&
                    lines.Any(line => line.Equals("Host: 127.0.0.1:" + attempt.CallbackPort, StringComparison.OrdinalIgnoreCase)))
                {
                    var uri = new Uri("http://127.0.0.1:" + attempt.CallbackPort + first[1]);
                    var query = uri.Query.TrimStart('?').Split('&').Select(value => value.Split('=', 2)).ToArray();
                    if (uri.AbsolutePath == "/return" && uri.Fragment == "" && query.Length == 2 && query.All(value => value.Length == 2) && query.Select(value => value[0]).Distinct().Count() == 2)
                    {
                        var values = query.ToDictionary(value => value[0], value => Uri.UnescapeDataString(value[1]));
                        if (values.TryGetValue("transaction_id", out string? transaction) && transaction == attempt.Transaction && values.TryGetValue("code", out string? candidate) && NativeClientWire.Proof(candidate)) code = candidate;
                    }
                }
            }
            catch (Exception error) when (error is IOException or OperationCanceledException or UriFormatException or ArgumentException) { }
            byte[] body = Encoding.UTF8.GetBytes(code is null ? "Invalid login return." : "Innlogging motteken. Du kan vende tilbake til Sprøyt.");
            byte[] response = Encoding.UTF8.GetBytes($"HTTP/1.1 {(code is null ? "400 Bad Request" : "200 OK")}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {body.Length}\r\n\r\n");
            try { await stream.WriteAsync(response, cancellationToken); await stream.WriteAsync(body, cancellationToken); } catch (IOException) { }
            if (code is not null) return code;
        }
    }
    public async ValueTask DisposeAsync()
    {
        lock (gate) { if (disposed) return; lifetime.Cancel(); inFlight?.Cancel(); }
        await loginGate.WaitAsync();
        try { lock (gate) { disposed = true; journal.Dispose(); http.Dispose(); lifetime.Dispose(); } }
        finally { loginGate.Release(); }
    }
}
