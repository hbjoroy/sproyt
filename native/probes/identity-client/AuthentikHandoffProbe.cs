using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;
using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Net.Sockets;
using System.Text.Json;
using static DesktopHandoffClient;

internal static class AuthentikHandoffProbe
{
    internal static async Task<int> Run(string executable, string lab, string output, bool systemBrowser, bool httpOnly = false, bool revocation = false)
    {
        Check(httpOnly || OperatingSystem.IsWindows());
        Check(!httpOnly || !systemBrowser);
        Check(!revocation || !systemBrowser);
        using var environment = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(lab, "environment.json")));
        Check(environment.RootElement.GetProperty("environment").GetString() == "sproyt-n01c-local" && environment.RootElement.GetProperty("status").GetString() == "ready");
        string origin = environment.RootElement.GetProperty("origin").GetString()!;
        Check(origin == "http://127.0.0.1:19090");
        using var policy = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(lab, "provider-policy.json")));
        var subjects = policy.RootElement.GetProperty("subjects").EnumerateObject().ToDictionary(p => p.Name, p => p.Value.GetString()!);
        Check(subjects.Count == 2 && subjects.Keys.All(a => a is "alice" or "bob") && subjects.Values.All(v => Guid.TryParse(v, out _)));
        Directory.CreateDirectory(output);
        string issuer = origin + "/application/o/n01-lab/", callback = "http://127.0.0.1:19192/return";
        var reservation = new TcpListener(IPAddress.Loopback, 19192); reservation.Start();
        var start = new ProcessStartInfo(executable) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (var value in new[] { "--database", Path.Combine(output, "broker.sqlite"), "--callback-uri", callback, "--authentik-config", Path.Combine(lab, "private", "broker.json"), "--interactive" }) start.ArgumentList.Add(value);
        using var broker = Process.Start(start)!;
        var errors = broker.StandardError.ReadToEndAsync();
        try
        {
            string? line = await broker.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(25));
            Check(line is not null && line.StartsWith("N01_READY ", StringComparison.Ordinal) && line.Length < 4096);
            using var ready = JsonDocument.Parse(line![10..]);
            string address = ready.RootElement.GetProperty("address").GetString()!, instance = ready.RootElement.GetProperty("instance_id").GetString()!;
            Check(address == "http://127.0.0.1:19191" && ready.RootElement.GetProperty("issuer").GetString() == issuer);
            reservation.Stop();
            using var api = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false }) { BaseAddress = new Uri(address), Timeout = TimeSpan.FromSeconds(20) };
            var cookieJar = new CookieContainer();
            using var browser = new HttpClient(new HttpClientHandler { CookieContainer = cookieJar, AllowAutoRedirect = false, UseProxy = false }) { Timeout = TimeSpan.FromSeconds(20) };
            DesktopState Read(string folder) { using var journal = new WindowsProtectedJournal(folder, false); return journal.Read<DesktopState>(); }
            async Task FollowAuth(string[] transactions, string[] accounts, string? slot = null)
            {
                    using var sessions = JsonDocument.Parse(await File.ReadAllTextAsync(Path.Combine(lab, "private", slot is null ? "browser-sessions.json" : "revocation-sessions.json")));
                    string cookieName = sessions.RootElement.GetProperty("cookie_name").GetString()!;
                    for (int index = 0; index < accounts.Length; index++)
                    {
                        string cookie = slot is null ? sessions.RootElement.GetProperty("cookies").GetProperty(accounts[index]).GetString()! : sessions.RootElement.GetProperty("sessions").GetProperty(slot).GetProperty("cookie").GetString()!;
                        cookieJar.Add(new Uri(origin), new Cookie(cookieName, cookie, "/"));
                        var target = new Uri(address + "/probe/browser/" + transactions[index]);
                        bool returned = false;
                        for (int hop = 0; hop < 16; hop++)
                        {
                            Check(target.Scheme == "http" && target.Host == "127.0.0.1" && target.Port is 19090 or 19191 or 19192);
                            using var response = await browser.GetAsync(target);
                            if (response.StatusCode is HttpStatusCode.Found or HttpStatusCode.SeeOther or HttpStatusCode.TemporaryRedirect)
                            {
                                if (target.AbsolutePath == "/return") { returned = true; break; }
                                target = new Uri(target, response.Headers.Location!); continue;
                            }
                            if (target.AbsolutePath == "/done" && response.IsSuccessStatusCode) { returned = true; break; }
                            Console.WriteLine("N01c unexpected browser step: " + target.AbsolutePath + " HTTP " + (int)response.StatusCode);
                            Check(false);
                        }
                        Check(returned);
                    }
            }
            async Task<(DesktopState State, string[] Transactions)> HttpLogin(string[] accounts, string? slot)
            {
                // Deliberately RAM-only: this mode proves the HTTP contract on Linux,
                // and provides no native journal, restart, or process-death evidence.
                var attempts = new List<DesktopAttempt>();
                var builder = WebApplication.CreateSlimBuilder(); builder.Logging.ClearProviders();
                builder.WebHost.ConfigureKestrel(k => { k.Listen(IPAddress.Loopback, 19192); k.Limits.MaxRequestLineSize = 2048; k.Limits.MaxRequestBodySize = 8192; });
                await using var listener = builder.Build();
                using var gate = new SemaphoreSlim(1, 1);
                listener.MapGet("/return", async (HttpContext context) =>
                {
                    context.Response.Headers.CacheControl = "no-store";
                    context.Response.Headers["Referrer-Policy"] = "no-referrer";
                    if (context.Request.Host.Host != "127.0.0.1" || context.Request.Host.Port != 19192 || context.Request.Query.Count != 2 || context.Request.Query["transaction_id"].Count != 1 || context.Request.Query["code"].Count != 1) return Results.BadRequest();
                    string transaction = context.Request.Query["transaction_id"].ToString(), code = context.Request.Query["code"].ToString();
                    if (!Proof(transaction) || !Proof(code)) return Results.BadRequest();
                    await gate.WaitAsync();
                    try
                    {
                        int index = attempts.FindIndex(a => a.Transaction == transaction);
                        if (index < 0) return Results.BadRequest();
                        var attempt = attempts[index];
                        if (attempt.Result is not null) return attempt.Code == code ? Results.Redirect("/done") : Results.BadRequest();
                        using var response = await api.PostAsJsonAsync("/probe/exchange", new { transaction_id = transaction, code, verifier = attempt.Verifier, redemption_id = attempt.Redemption, environment = "n01-lab", destination_id = "windows-loopback" });
                        Check(response.IsSuccessStatusCode);
                        await response.Content.LoadIntoBufferAsync(8192);
                        string result = await response.Content.ReadAsStringAsync();
                        using var body = JsonDocument.Parse(result);
                        Check(body.RootElement.GetProperty("kind").GetString() == "non_api_fixture" &&
                            body.RootElement.GetProperty("user_id").GetString() == UserId(issuer + ":" + subjects[attempt.Account]).ToString() &&
                            Proof(body.RootElement.GetProperty("session_family").GetString()!) &&
                            body.RootElement.GetProperty("credential").GetString() is string credential && credential.StartsWith("fixture-only.", StringComparison.Ordinal) && Proof(credential[13..]));
                        attempts[index] = attempt with { Code = code, Result = result };
                        return Results.Redirect("/done");
                    }
                    catch { return Results.StatusCode(503); }
                    finally { gate.Release(); }
                });
                listener.MapGet("/done", () => Results.Text("HTTP contract login received."));
                await listener.StartAsync();
                try
                {
                    foreach (var account in accounts)
                    {
                        string verifier = Random();
                        using var response = await api.PostAsJsonAsync("/probe/start", new { challenge = Challenge(verifier), platform = "windows", environment = "n01-lab", destination_id = "windows-loopback", expected_user_id = UserId(issuer + ":" + subjects[account]) });
                        Check(response.IsSuccessStatusCode);
                        using var body = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
                        string transaction = body.RootElement.GetProperty("transaction_id").GetString()!;
                        Check(Proof(transaction) && body.RootElement.GetProperty("authorization_url").GetString() == address + "/probe/browser/" + transaction);
                        attempts.Add(new(transaction, account, verifier, Guid.NewGuid().ToString("N")));
                    }
                    string[] transactions = attempts.Select(a => a.Transaction).ToArray();
                    await FollowAuth(transactions, accounts, slot);
                    Check(attempts.All(a => a.Result is not null));
                    return (new DesktopState(1, "n01-lab", new DesktopConfig(address, instance, callback, accounts, false, issuer, subjects), DateTimeOffset.UtcNow.AddMinutes(8), attempts.ToArray()), transactions);
                }
                finally { await listener.StopAsync(); }
            }
            async Task<(DesktopState State, string[] Transactions)> Login(string name, string[] accounts, string? slot = null)
            {
                if (httpOnly) return await HttpLogin(accounts, slot);
                string folder = Path.Combine(output, name), config = Path.Combine(output, name + ".json");
                await File.WriteAllTextAsync(config, JsonSerializer.Serialize(new DesktopConfig(address, instance, callback, accounts, systemBrowser, issuer, subjects), Json));
                await using var client = new DesktopHandoffProbe.Client(folder, config, "none");
                var transactions = new List<string>();
                foreach (string account in accounts)
                {
                    var signal = await client.Phase("ready");
                    Check(signal.GetProperty("account").GetString() == account);
                    transactions.Add(signal.GetProperty("transaction").GetString()!);
                }
                if (systemBrowser) Console.WriteLine("N01c Edge: sign in as synthetic alice using the private lab credential.");
                else await FollowAuth(transactions.ToArray(), accounts, slot);
                await client.Process.WaitForExitAsync().WaitAsync(TimeSpan.FromMinutes(systemBrowser ? 9 : 1));
                await client.Exit(0);
                DesktopState state = Read(folder);
                Check(state.Attempts.All(a => a.Result is not null));
                return (state, transactions.ToArray());
            }
            if (revocation)
            {
                return await AuthentikRevocationProbe.Run(api, environment.RootElement, policy.RootElement, lab, output, httpOnly,
                    async (name, slot) => (await Login(name, [slot == "bob" ? "bob" : "alice"], slot)).State,
                    name => Read(Path.Combine(output, name)));
            }
            var first = await Login("initial", systemBrowser ? ["alice"] : ["alice", "bob"]);
            var account = first.State.Attempts.Single(a => a.Account == "alice");
            string userId = UserId(issuer + ":" + subjects["alice"]).ToString();
            async Task<JsonElement> Refresh(string action, string transaction, string user)
            {
                using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/upstream-check")
                { Content = JsonContent.Create(new { user_id = user, transaction_id = transaction, action }) };
                request.Headers.Add("X-Probe-Control", "1");
                using var response = await api.SendAsync(request); Check(response.IsSuccessStatusCode);
                using var json = JsonDocument.Parse(await response.Content.ReadAsStringAsync()); return json.RootElement.Clone();
            }
            using (var spoof = new HttpRequestMessage(HttpMethod.Post, "/probe/upstream-check") { Content = JsonContent.Create(new { user_id = userId, transaction_id = account.Transaction, action = "refresh" }) })
            { using var denied = await api.SendAsync(spoof); Check(denied.StatusCode == HttpStatusCode.BadRequest); }
            using (var fake = await api.GetAsync("/provider/.well-known/openid-configuration")) Check(fake.StatusCode == HttpStatusCode.BadRequest);
            var normal = await Refresh("refresh", account.Transaction, userId);
            Check(normal.GetProperty("accepted").GetBoolean() && normal.GetProperty("expires_in").GetInt32() > 0);
            var dropped = await Refresh("drop-refresh-result", account.Transaction, userId);
            Check(dropped.GetProperty("accepted").GetBoolean());
            var replay = await Refresh("retry-old", account.Transaction, userId);
            // This is an observation, not a claim that a usable grant was repaired.
            Check(replay.GetProperty("accepted").GetBoolean() || replay.GetProperty("requires_reauthentication").GetBoolean());
            if (!httpOnly) Check(Read(Path.Combine(output, "initial")).Attempts.Single(a => a.Account == "alice").Result == account.Result);
            bool stableIdentity = false;
            if (!systemBrowser)
            {
                var again = await Login("same-account", ["alice"]);
                using var previous = JsonDocument.Parse(account.Result!); using var next = JsonDocument.Parse(again.State.Attempts.Single().Result!);
                Check(previous.RootElement.GetProperty("user_id").GetString() == next.RootElement.GetProperty("user_id").GetString());
                stableIdentity = true;
            }
            await File.WriteAllTextAsync(Path.Combine(output, "receipt.json"), JsonSerializer.Serialize(new { passed = true,
                scope = httpOnly ? "http_contract" : "n01c_authentik_loopback", nativeWindowsHandoff = !httpOnly, sourceRevision = environment.RootElement.TryGetProperty("sourceRevision", out var rev) ? rev.GetString() : null,
                authentikImage = environment.RootElement.GetProperty("authImage").GetString(), postgresImage = environment.RootElement.GetProperty("postgresImage").GetString(),
                authentication = systemBrowser ? "external_system_browser" : "preauthenticated_synthetic_session_fixture",
                oidcVerified = true, stableIssuerSubject = stableIdentity, independentAccounts = first.State.Attempts.Length == 2,
                normalRefresh = normal, discardedRefreshResponse = dropped, oldRefreshRetryObservation = replay,
                localStatePreserved = httpOnly ? (bool?)null : true, policy = policy.RootElement.Clone(),
                limitations = new[] { "isolated local issuer, not production identity", "no passkey/recovery/revocation acceptance", "broker upstream tokens held only in memory", "retry-old does not install the recovered response", "not a production API session" } }, Json));
            Console.WriteLine("PASS real Authentik OIDC/refresh laboratory");
            return 0;
        }
        finally
        {
            reservation.Stop(); if (!broker.HasExited) broker.Kill(true); await broker.WaitForExitAsync(); await errors;
        }
    }
}
