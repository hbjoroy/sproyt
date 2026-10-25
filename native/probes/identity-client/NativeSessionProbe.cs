using System.Diagnostics;
using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Net.Sockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.WebUtilities;

internal static class NativeSessionProbe
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private static void Check(bool condition, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0) { if (!condition) throw new DesktopHandoffClient.Failure(line); }
    private static int Port() { var listener = new TcpListener(IPAddress.Loopback, 0); listener.Start(); int port = ((IPEndPoint)listener.LocalEndpoint).Port; listener.Stop(); return port; }
    private static string Random() => DesktopHandoffClient.Random();
    private static string User(string issuer, string account) => DesktopHandoffClient.UserId(issuer + ":" + account).ToString("D");
    private sealed class Broker(string executable, string database, int port, int callback) : IDisposable
    {
        private Process? process;
        internal string Origin => $"http://127.0.0.1:{port}";
        internal async Task Start()
        {
            var start = new ProcessStartInfo(executable) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, RedirectStandardOutput = true, RedirectStandardError = true };
            foreach (string argument in new[] { "--database", database, "--callback-uri", $"http://127.0.0.1:{callback}/return", "--session-lab", "--listen-port", port.ToString(System.Globalization.CultureInfo.InvariantCulture) }) start.ArgumentList.Add(argument);
            process = Process.Start(start)!;
            _ = process.StandardError.ReadToEndAsync();
            string? line = await process.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(20));
            Check(line is not null && line.StartsWith("N01_READY ", StringComparison.Ordinal));
            _ = process.StandardOutput.ReadToEndAsync();
        }
        internal void Stop() { if (process is not null) { if (!process.HasExited) { process.Kill(entireProcessTree: true); process.WaitForExit(); } process.Dispose(); process = null; } }
        public void Dispose() => Stop();
    }
    private sealed class FaultHandler : DelegatingHandler
    {
        internal bool DropOnce;
        internal bool CancelOnce;
        internal bool Offline = false;
        internal FaultHandler() : base(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false }) { }
        protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            if (Offline) throw new HttpRequestException("Synthetic transport unavailable.");
            if (CancelOnce && request.RequestUri!.AbsolutePath == "/session/refresh") { CancelOnce = false; throw new OperationCanceledException("Synthetic transport unavailable."); }
            if (DropOnce && request.RequestUri!.AbsolutePath == "/session/refresh") { DropOnce = false; request.Headers.Add("X-Probe-Drop-Ack", "1"); }
            return base.SendAsync(request, cancellationToken);
        }
    }
    private static HttpClient Client(string origin, bool browser = false) => new(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = browser, UseProxy = false }) { BaseAddress = new Uri(origin), Timeout = TimeSpan.FromSeconds(12) };
    private sealed class Handoff
    {
        internal required string Transaction;
        internal required string Verifier;
        internal required string Code;
        internal string Redemption = Guid.NewGuid().ToString("N");
    }
    private static async Task<Handoff?> Prepare(HttpClient app, string issuer, string account, string? expected = null, bool reject = false)
    {
        string verifier = Random();
        using var start = await app.PostAsJsonAsync("/probe/start", new { challenge = DesktopHandoffClient.Challenge(verifier), platform = "windows", environment = "n01-lab", destination_id = "windows-loopback", expected_user_id = expected ?? User(issuer, account) });
        Check(start.StatusCode == HttpStatusCode.OK);
        using var attempt = JsonDocument.Parse(await start.Content.ReadAsStringAsync());
        string transaction = attempt.RootElement.GetProperty("transaction_id").GetString()!;
        var next = new Uri(attempt.RootElement.GetProperty("authorization_url").GetString()!);
        using var browser = Client(app.BaseAddress!.AbsoluteUri, true);
        string? code = null;
        for (int count = 0; count < 8; count++)
        {
            Check(next.Scheme == "http" && next.Host == "127.0.0.1");
            if (next.AbsolutePath == "/return") { var query = QueryHelpers.ParseQuery(next.Query); code = query["code"].ToString(); Check(query["transaction_id"] == transaction); break; }
            using var request = new HttpRequestMessage(HttpMethod.Get, next);
            if (next.AbsolutePath == "/provider/authorize") request.Headers.Add("X-Probe-Account", account);
            using var response = await browser.SendAsync(request);
            if (reject && response.StatusCode == HttpStatusCode.BadRequest) return null;
            Check(response.StatusCode == HttpStatusCode.Found);
            next = response.Headers.Location!.IsAbsoluteUri ? response.Headers.Location : new Uri(next, response.Headers.Location);
        }
        Check(!reject && code is not null);
        return new Handoff { Transaction = transaction, Verifier = verifier, Code = code! };
    }
    private static async Task<HttpResponseMessage> Exchange(HttpClient app, Handoff handoff, bool drop = false)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/exchange") { Content = JsonContent.Create(new { transaction_id = handoff.Transaction, code = handoff.Code, verifier = handoff.Verifier, redemption_id = handoff.Redemption, environment = "n01-lab", destination_id = "windows-loopback" }) };
        if (drop) request.Headers.Add("X-Probe-Drop-Ack", "1");
        return await app.SendAsync(request);
    }
    private static async Task Cancel(HttpClient app, Handoff handoff, bool wrongProof = false)
    {
        using var response = await app.PostAsJsonAsync("/session/cancel-login", new { transaction_id = handoff.Transaction, verifier = wrongProof ? Random() : handoff.Verifier, environment = "n01-lab", destination_id = "windows-loopback" });
        Check(response.StatusCode == (wrongProof ? HttpStatusCode.Unauthorized : HttpStatusCode.NoContent));
    }
    private static async Task<string> Login(HttpClient app, string issuer, string account, string? expected = null, bool reject = false)
    {
        var handoff = await Prepare(app, issuer, account, expected, reject);
        if (handoff is null) { Check(reject); return "rejected"; }
        using var exchange = await Exchange(app, handoff);
        Check(exchange.StatusCode == HttpStatusCode.OK);
        return await exchange.Content.ReadAsStringAsync();
    }
    private static JsonElement Grant(string exchange) { using var document = JsonDocument.Parse(exchange); return document.RootElement.GetProperty("native_session").Clone(); }
    private static async Task<HttpResponseMessage> Auth(HttpClient app, HttpMethod method, string path, string token, object? body = null, bool drop = false)
    {
        using var request = new HttpRequestMessage(method, path);
        request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token);
        if (body is not null) request.Content = JsonContent.Create(body);
        if (drop) request.Headers.Add("X-Probe-Drop-Ack", "1");
        return await app.SendAsync(request);
    }
    private static async Task Control(HttpClient app, string path, object body)
    { using var request = new HttpRequestMessage(HttpMethod.Post, path) { Content = JsonContent.Create(body) }; request.Headers.Add("X-Probe-Control", "1"); using var response = await app.SendAsync(request); Check(response.IsSuccessStatusCode); }
    private static async Task Install(NativeAuthSession owner, string exchange)
    { var ticket = await owner.BeginLoginAsync(); await owner.CompleteLoginAsync(ticket, exchange); Check(owner.Snapshot.Phase == NativeAuthPhase.Active); }
    private static async Task Probe(HttpClient app, string exchange, HttpStatusCode expected)
    { using var response = await Auth(app, HttpMethod.Get, "/session/probe", Grant(exchange).GetProperty("access_token").GetString()!); Check(response.StatusCode == expected); }
    private static void Rejected(Action action) { bool rejected = false; try { action(); } catch { rejected = true; } Check(rejected); }

    internal static async Task<int> Run(string executable, string directory, string receipt)
    {
        Check(OperatingSystem.IsWindows() && !Directory.Exists(directory)); Directory.CreateDirectory(directory);
        using var server = new Broker(executable, Path.Combine(directory, "broker.sqlite"), Port(), Port()); await server.Start();
        using var app = Client(server.Origin);
        using var info = await app.GetFromJsonAsync<JsonDocument>("/session/info");
        var config = new NativeAuthConfig(server.Origin, info!.RootElement.GetProperty("instance_id").GetString()!, info.RootElement.GetProperty("issuer").GetString()!, User(server.Origin + "/provider", "alice"));
        Check(info.RootElement.GetProperty("scope").GetString() == "n03a_session_lab");
        var cancelBefore = (await Prepare(app, config.Issuer, "alice"))!;
        await Cancel(app, cancelBefore); await Cancel(app, cancelBefore);
        using (var response = await Exchange(app, cancelBefore)) Check(response.StatusCode == HttpStatusCode.BadRequest);
        var cancelWrongProof = (await Prepare(app, config.Issuer, "alice"))!;
        await Cancel(app, cancelWrongProof, wrongProof: true);
        string cancelKnown;
        using (var response = await Exchange(app, cancelWrongProof)) { Check(response.IsSuccessStatusCode); cancelKnown = await response.Content.ReadAsStringAsync(); }
        await Probe(app, cancelKnown, HttpStatusCode.OK); await Cancel(app, cancelWrongProof);
        await Probe(app, cancelKnown, HttpStatusCode.Unauthorized); await Cancel(app, cancelWrongProof, wrongProof: true); await Cancel(app, cancelWrongProof);
        var cancelLostReply = (await Prepare(app, config.Issuer, "alice"))!;
        bool lost = false; try { using var response = await Exchange(app, cancelLostReply, drop: true); } catch (HttpRequestException) { lost = true; }
        Check(lost); await Cancel(app, cancelLostReply); await Cancel(app, cancelLostReply);
        using (var response = await Exchange(app, cancelLostReply)) Check(response.StatusCode == HttpStatusCode.BadRequest);
        Check(await Login(app, config.Issuer, "bob", config.UserId, reject: true) == "rejected");
        string initial = await Login(app, config.Issuer, "alice"); var original = Grant(initial);
        string access = original.GetProperty("access_token").GetString()!, refresh = original.GetProperty("refresh_token").GetString()!, revoke = original.GetProperty("revoke_token").GetString()!;
        using (var r = await Auth(app, HttpMethod.Get, "/session/probe", refresh)) Check(r.StatusCode == HttpStatusCode.Unauthorized);
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", access, new { operation_id = Guid.NewGuid().ToString("N"), generation = 0 })) Check(r.StatusCode == HttpStatusCode.Unauthorized);
        string operation = Guid.NewGuid().ToString("N");
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", refresh, new { operation_id = operation, generation = 0 }, drop: true)) Check(r.StatusCode == HttpStatusCode.ServiceUnavailable);
        string rotated;
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", refresh, new { operation_id = operation, generation = 0 })) { Check(r.IsSuccessStatusCode); rotated = await r.Content.ReadAsStringAsync(); }
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", refresh, new { operation_id = operation, generation = 0 })) Check(await r.Content.ReadAsStringAsync() == rotated);
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", refresh, new { operation_id = Guid.NewGuid().ToString("N"), generation = 0 })) Check(r.StatusCode == HttpStatusCode.Unauthorized);
        using (var r = await Auth(app, HttpMethod.Post, "/session/revoke", revoke)) Check(r.StatusCode == HttpStatusCode.NoContent);
        using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", refresh, new { operation_id = operation, generation = 0 })) Check(r.StatusCode == HttpStatusCode.Unauthorized);
        using (var r = await Auth(app, HttpMethod.Post, "/session/revoke", revoke)) Check(r.StatusCode == HttpStatusCode.NoContent);

        foreach (string fault in new[] { "receipt", "epoch" })
        {
            var grant = Grant(await Login(app, config.Issuer, "alice")); string op = Guid.NewGuid().ToString("N");
            using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", grant.GetProperty("refresh_token").GetString()!, new { operation_id = op, generation = 0 }, drop: true)) Check(r.StatusCode == HttpStatusCode.ServiceUnavailable);
            if (fault == "receipt") await Control(app, "/fixture/session/expire", new { family = grant.GetProperty("family").GetString(), scope = "receipt" });
            else await Control(app, "/fixture/session/epoch", new { user_id = config.UserId });
            using (var r = await Auth(app, HttpMethod.Post, "/session/refresh", grant.GetProperty("refresh_token").GetString()!, new { operation_id = op, generation = 0 })) Check(r.StatusCode == HttpStatusCode.Unauthorized);
        }

        string ownerDirectory = Path.Combine(directory, "owner"); var transport = new FaultHandler();
        using (var owner = NativeAuthSession.Open(ownerDirectory, config, handler: transport))
        {
            Rejected(() => NativeAuthSession.Open(ownerDirectory, config).Dispose());
            string alice = await Login(app, config.Issuer, "alice");
            var ticket = await owner.BeginLoginAsync();
            string bob = await Login(app, config.Issuer, "bob");
            bool rejected = false; try { await owner.CompleteLoginAsync(ticket, bob); } catch { rejected = true; } Check(rejected && owner.Snapshot.Phase != NativeAuthPhase.Active);
            await owner.CompleteLoginAsync(ticket, alice); Check(owner.Snapshot.Phase == NativeAuthPhase.Active);
            transport.DropOnce = true; await owner.RefreshAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Offline && owner.Snapshot.Generation == 0);
            await owner.RefreshAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.Generation == 1);
            transport.CancelOnce = true; await owner.RefreshAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Offline && owner.Snapshot.Generation == 1);
            await owner.RefreshAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.Generation == 2);
        }
        using (var owner = NativeAuthSession.Open(ownerDirectory, config))
        { Check(owner.Snapshot.Phase == NativeAuthPhase.Offline); await owner.ResumeAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.Generation == 2); }
        server.Stop();
        using (var owner = NativeAuthSession.Open(ownerDirectory, config))
        {
            await owner.ResumeAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Offline);
            await owner.LogoutAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && owner.Snapshot.PendingRevocation);
            bool rejected = false; try { _ = await owner.BeginLoginAsync(); } catch { rejected = true; } Check(rejected);
        }
        await server.Start();
        await Cancel(app, cancelBefore); await Cancel(app, cancelLostReply);
        using (var owner = NativeAuthSession.Open(ownerDirectory, config))
        {
            await owner.RetryRevocationAsync(); Check(!owner.Snapshot.PendingRevocation && owner.Snapshot.Phase == NativeAuthPhase.SignedOut);
            string alice = await Login(app, config.Issuer, "alice"); await Install(owner, alice);
            await Control(app, "/fixture/session/expire", new { family = Grant(alice).GetProperty("family").GetString(), scope = "access" });
            await owner.ResumeAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.Generation == 1);
            await Control(app, "/fixture/session/epoch", new { user_id = config.UserId });
            await owner.RefreshAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.AuthRequired && owner.Snapshot.UserId is null);
        }
        using (var owner = NativeAuthSession.Open(ownerDirectory, config)) { Check(owner.Snapshot.Phase == NativeAuthPhase.AuthRequired); await owner.RetryRevocationAsync(); }

        string pendingLoginDirectory = Path.Combine(directory, "pending-login"); long loginTicket;
        string pendingExchange = await Login(app, config.Issuer, "alice");
        using (var owner = NativeAuthSession.Open(pendingLoginDirectory, config)) { loginTicket = await owner.BeginLoginAsync(); Check(owner.Snapshot.LoginTicket == loginTicket); }
        using (var owner = NativeAuthSession.Open(pendingLoginDirectory, config))
        { Check(owner.Snapshot.Phase == NativeAuthPhase.SigningIn && owner.Snapshot.LoginTicket == loginTicket); await owner.CompleteLoginAsync(loginTicket, pendingExchange); Check(owner.Snapshot.Phase == NativeAuthPhase.Active); await owner.LogoutAsync(); }
        string staleLoginDirectory = Path.Combine(directory, "stale-login");
        string staleExchange = await Login(app, config.Issuer, "alice");
        await Control(app, "/fixture/session/epoch", new { user_id = config.UserId });
        using (var owner = NativeAuthSession.Open(staleLoginDirectory, config))
        { long ticket = await owner.BeginLoginAsync(); await owner.CompleteLoginAsync(ticket, staleExchange); Check(owner.Snapshot.Phase == NativeAuthPhase.AuthRequired); }

        var received = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously); var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        using (var owner = NativeAuthSession.Open(Path.Combine(directory, "race"), config, async phase => { if (phase == "rotation-received") { received.SetResult(); await release.Task; } }))
        {
            string alice = await Login(app, config.Issuer, "alice"); await Install(owner, alice);
            var refreshTask = owner.RefreshAsync(); await received.Task.WaitAsync(TimeSpan.FromSeconds(15));
            await owner.LogoutAsync(); release.SetResult(); await refreshTask;
            Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && owner.Snapshot.UserId is null && !owner.Snapshot.PendingRevocation);
            long ticket = await owner.BeginLoginAsync(); await owner.LogoutAsync();
            // This family is issued after local logout, so it cannot already be revoked.
            string freshLate = await Login(app, config.Issuer, "alice"); await Probe(app, freshLate, HttpStatusCode.OK);
            await owner.CompleteLoginAsync(ticket, freshLate); Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && !owner.Snapshot.PendingRevocation);
            await Probe(app, freshLate, HttpStatusCode.Unauthorized);
        }
        string lateDirectory = Path.Combine(directory, "offline-late-login"); var lateTransport = new FaultHandler(); string offlineLate;
        using (var owner = NativeAuthSession.Open(lateDirectory, config, handler: lateTransport))
        {
            long ticket = await owner.BeginLoginAsync(); await owner.LogoutAsync();
            offlineLate = await Login(app, config.Issuer, "alice"); await Probe(app, offlineLate, HttpStatusCode.OK);
            lateTransport.Offline = true; await owner.CompleteLoginAsync(ticket, offlineLate);
            Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && owner.Snapshot.PendingRevocation && owner.Snapshot.UserId is null);
        }
        using (var owner = NativeAuthSession.Open(lateDirectory, config))
        { Check(owner.Snapshot.PendingRevocation && owner.Snapshot.Phase == NativeAuthPhase.SignedOut); await owner.RetryRevocationAsync(); Check(!owner.Snapshot.PendingRevocation); }
        await Probe(app, offlineLate, HttpStatusCode.Unauthorized);

        string queueDirectory = Path.Combine(directory, "newer-active-late-login"); var queueTransport = new FaultHandler(); string queueLate; string newerActive;
        using (var owner = NativeAuthSession.Open(queueDirectory, config, handler: queueTransport))
        {
            long oldTicket = await owner.BeginLoginAsync(); await owner.LogoutAsync();
            newerActive = await Login(app, config.Issuer, "alice"); await Install(owner, newerActive);
            queueLate = await Login(app, config.Issuer, "alice"); await Probe(app, queueLate, HttpStatusCode.OK);
            queueTransport.Offline = true; await owner.CompleteLoginAsync(oldTicket, queueLate);
            Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.UserId == config.UserId && owner.Snapshot.PendingRevocation);
        }
        using (var owner = NativeAuthSession.Open(queueDirectory, config, handler: new FaultHandler { Offline = true }))
        { Check(owner.Snapshot.Phase == NativeAuthPhase.Offline && owner.Snapshot.PendingRevocation); await owner.LogoutAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && owner.Snapshot.PendingRevocation); }
        using (var owner = NativeAuthSession.Open(queueDirectory, config))
        { await owner.RetryRevocationAsync(); Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && !owner.Snapshot.PendingRevocation); }
        await Probe(app, queueLate, HttpStatusCode.Unauthorized); await Probe(app, newerActive, HttpStatusCode.Unauthorized);
        string corrupt = Path.Combine(directory, "corrupt"); using (var owner = NativeAuthSession.Open(corrupt, config)) { }
        await File.WriteAllBytesAsync(Path.Combine(corrupt, "state.bin"), RandomNumberGenerator.GetBytes(64)); Rejected(() => NativeAuthSession.Open(corrupt, config).Dispose());
        var configPath = Path.Combine(directory, "config.json"); await File.WriteAllTextAsync(configPath, JsonSerializer.Serialize(config, Json));
        foreach (string phase in new[] { "rotation-received", "rotation-durable" })
        {
            string childDirectory = Path.Combine(directory, phase);
            using (var owner = NativeAuthSession.Open(childDirectory, config)) await Install(owner, await Login(app, config.Issuer, "alice"));
            var start = new ProcessStartInfo("dotnet") { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, RedirectStandardOutput = true, RedirectStandardError = true };
            foreach (string argument in new[] { typeof(Program).Assembly.Location, "--session-child", childDirectory, configPath, phase }) start.ArgumentList.Add(argument);
            using var child = Process.Start(start)!; _ = child.StandardError.ReadToEndAsync();
            try { Check(await child.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(20)) == "SESSION_CHECKPOINT"); }
            finally { if (!child.HasExited) { child.Kill(entireProcessTree: true); child.WaitForExit(); } }
            using var resumed = NativeAuthSession.Open(childDirectory, config); await resumed.ResumeAsync(); Check(resumed.Snapshot.Phase == NativeAuthPhase.Active && resumed.Snapshot.Generation == 1);
            await resumed.LogoutAsync();
        }
        await File.WriteAllTextAsync(receipt, JsonSerializer.Serialize(new { passed = true, synthetic_session_lab = true, dpapi = true, scope_fences = true, rotation_retry = true, expired_receipt_fence = true, epoch_receipt_fence = true, revocation_receipt_fence = true, owner_restart = true, broker_restart = true, offline_logout = true, pending_revocation = true, epoch_fence = true, stale_login_epoch_fence = true, pending_login_restart = true, delayed_completion_fences = true, fresh_post_logout_family_revoked = true, offline_late_family_durable_revoke = true, late_family_preserves_newer_active = true, multiple_pending_families_durable_revoke = true, cancel_before_exchange = true, cancel_wrong_proof_fence = true, cancel_after_issued_grant = true, cancel_lost_exchange_reply = true, cancellation_survives_broker_restart = true, corrupt_journal_fence = true, exclusive_owner = true, actual_process_death_before_commit = true, actual_process_death_after_commit = true }, Json));
        Console.WriteLine("PASS native synthetic session owner probe"); return 0;
    }
    internal static async Task<int> Child(string directory, string configPath, string phase)
    {
        Check(phase is "rotation-received" or "rotation-durable");
        Check(new FileInfo(configPath).Length < 4096);
        var config = JsonSerializer.Deserialize<NativeAuthConfig>(await File.ReadAllTextAsync(configPath), Json)!;
        using var owner = NativeAuthSession.Open(directory, config, async checkpoint => { if (checkpoint == phase) { Console.WriteLine("SESSION_CHECKPOINT"); await Task.Delay(Timeout.InfiniteTimeSpan); } });
        await owner.ResumeAsync(); await owner.RefreshAsync(); return 1;
    }
}
