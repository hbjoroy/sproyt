using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Hosting.Server;
using Microsoft.AspNetCore.Hosting.Server.Features;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

internal static class Program
{
    private static void Require(bool condition, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0) { if (!condition) throw new ProbeFailure(line); }
    private sealed class ProbeFailure(int line = 0) : Exception { public int Line { get; } = line; }
    private sealed record Attempt(string Transaction, string Authorization, string Verifier);
    private sealed record Handoff(string Transaction, string Code);
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private static string B64(byte[] bytes) => Convert.ToBase64String(bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    public static async Task<int> Main(string[] args)
    {
        try
        {
            if (args.Length == 4 && args[0] == "--desktop-child") return await DesktopHandoffClient.Run(args[1], args[2], args[3]);
            if (args.Length == 4 && args[0] is "--desktop-test" or "--desktop-browser") return await DesktopHandoffProbe.Run(args[1], args[2], args[3], args[0] == "--desktop-browser");
            Require(args.Length == 3); await Run(args[0], args[1], args[2]); Console.WriteLine("PASS isolated identity HTTP probe"); return 0;
        }
        catch (DesktopHandoffClient.Failure failure) { Console.Error.WriteLine($"FAIL desktop identity assertion line {failure.Line}"); return 1; }
        catch (ProbeFailure failure) { Console.Error.WriteLine($"FAIL isolated identity HTTP probe: assertion line {failure.Line}"); return 1; }
        catch { Console.Error.WriteLine("FAIL isolated identity HTTP probe (details suppressed to protect transient proofs)"); return 1; }
    }
    private static async Task Run(string executable, string database, string receipt)
    {
        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(k => k.Listen(IPAddress.Loopback, 0));
        await using var listener = builder.Build();
        var returns = new List<Handoff>();
        listener.MapGet("/return", (HttpContext context) => {
            var handoff = new Handoff(context.Request.Query["transaction_id"].ToString(), context.Request.Query["code"].ToString());
            lock (returns) returns.Add(handoff);
            return Results.Text("Synthetic login return received.");
        });
        await listener.StartAsync();
        var callback = listener.Services.GetRequiredService<IServer>().Features.Get<IServerAddressesFeature>()!.Addresses.Single() + "/return";
        var start = new ProcessStartInfo(executable) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (var value in new[] { "--database", database, "--callback-uri", callback }) start.ArgumentList.Add(value);
        using var child = Process.Start(start)!;
        // Drain without forwarding server output: neither URLs nor transient proofs belong in logs.
        var stderr = child.StandardError.ReadToEndAsync();
        try
        {
            using var readyTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
            var line = new StringBuilder();
            var unit = new char[1];
            while (true) { Require(await child.StandardOutput.ReadAsync(unit.AsMemory(), readyTimeout.Token) == 1); if (unit[0] == '\n') break; line.Append(unit[0]); Require(line.Length <= 4096); }
            Require(line.ToString().StartsWith("N01_READY ", StringComparison.Ordinal));
            using var ready = JsonDocument.Parse(line.ToString()[10..]);
            var address = ready.RootElement.GetProperty("address").GetString()!;
            var issuer = ready.RootElement.GetProperty("issuer").GetString()!;
            var origin = new Uri(address); Require(origin.Scheme == "http" && origin.Host == "127.0.0.1" && origin.AbsolutePath == "/");
            var stdout = child.StandardOutput.ReadToEndAsync();
            using var app = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false }) { BaseAddress = origin, Timeout = TimeSpan.FromSeconds(15) };
            using var browser = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, CookieContainer = new CookieContainer(), UseProxy = false }) { Timeout = TimeSpan.FromSeconds(15) };
            async Task<Attempt> Begin(string? expected = null)
            {
                var verifier = B64(RandomNumberGenerator.GetBytes(32));
                using var response = await app.PostAsJsonAsync("/probe/start", new { challenge = B64(SHA256.HashData(Encoding.ASCII.GetBytes(verifier))), platform = "windows", environment = "n01-lab", destination_id = "windows-loopback", expected_user_id = expected });
                Require(response.StatusCode == HttpStatusCode.OK);
                using var body = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
                return new(body.RootElement.GetProperty("transaction_id").GetString()!, body.RootElement.GetProperty("authorization_url").GetString()!, verifier);
            }
            async Task Reject(HttpResponseMessage response)
            {
                using (response) { Require(response.StatusCode == HttpStatusCode.BadRequest); Require((await response.Content.ReadAsStringAsync()).Trim() == "{\"error\":\"probe_rejected\"}"); }
            }
            async Task<Uri> BrowserStart(Attempt attempt)
            {
                var entry = new Uri(attempt.Authorization);
                Require(entry.Scheme == origin.Scheme && entry.Authority == origin.Authority && entry.AbsolutePath == "/probe/browser/" + attempt.Transaction && entry.Query.Length == 0);
                using var response = await browser.GetAsync(entry);
                Require(response.StatusCode == HttpStatusCode.Found);
                return response.Headers.Location!.IsAbsoluteUri ? response.Headers.Location : new Uri(origin, response.Headers.Location);
            }
            async Task<Handoff?> Follow(Uri url, string account, string? fault = null, bool reject = false)
            {
                for (var count = 0; count < 8; count++)
                {
                    Require(url.Scheme == "http" && url.Host == "127.0.0.1" && (url.Port == origin.Port || url.Port == new Uri(callback).Port));
                    if (url.AbsolutePath == "/return")
                    {
                        var returnQuery = Microsoft.AspNetCore.WebUtilities.QueryHelpers.ParseQuery(url.Query);
                        Require(url.GetLeftPart(UriPartial.Path) == callback && returnQuery.Count == 2 && returnQuery.ContainsKey("code") && returnQuery.ContainsKey("transaction_id"));
                        Require(returnQuery["code"].Count == 1 && returnQuery["transaction_id"].Count == 1);
                    }
                    using var request = new HttpRequestMessage(HttpMethod.Get, url);
                    if (url.AbsolutePath == "/provider/authorize") { request.Headers.Add("X-Probe-Account", account); if (fault != null) request.Headers.Add("X-Probe-Fault", fault); }
                    var response = await browser.SendAsync(request);
                    if (reject && response.StatusCode == HttpStatusCode.BadRequest) { await Reject(response); return null; }
                    using (response)
                    {
                        if (response.StatusCode == HttpStatusCode.Found) { url = response.Headers.Location!.IsAbsoluteUri ? response.Headers.Location : new Uri(url, response.Headers.Location); continue; }
                        Require(!reject && response.StatusCode == HttpStatusCode.OK && url.AbsolutePath == "/return");
                        lock (returns) return returns.Last();
                    }
                }
                throw new ProbeFailure();
            }
            async Task<Handoff> Login(Attempt attempt, string account = "alice") => (await Follow(await BrowserStart(attempt), account))!;
            async Task<HttpResponseMessage> Exchange(Attempt a, Handoff h, string id, string? verifier = null, string environment = "n01-lab", string destination = "windows-loopback", bool drop = false)
            {
                using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/exchange") { Content = JsonContent.Create(new { transaction_id = a.Transaction, code = h.Code, verifier = verifier ?? a.Verifier, redemption_id = id, environment, destination_id = destination }) };
                if (drop) request.Headers.Add("X-Probe-Drop-Ack", "1");
                return await app.SendAsync(request);
            }
            async Task<string> Good(Attempt a, Handoff h, string id, string account)
            {
                using var response = await Exchange(a, h, id); Require(response.StatusCode == HttpStatusCode.OK);
                var text = await response.Content.ReadAsStringAsync(); using var body = JsonDocument.Parse(text);
                Require(body.RootElement.GetProperty("kind").GetString() == "non_api_fixture");
                Require(Guid.Parse(body.RootElement.GetProperty("user_id").GetString()!) == UserId(issuer + ":" + account));
                Require(body.RootElement.GetProperty("credential").GetString()!.Length > 0); return text;
            }
            async Task Expire(Attempt a, string scope) { using var r = await app.PostAsJsonAsync("/fixture/expire", new { transaction_id = a.Transaction, scope }); Require(r.IsSuccessStatusCode); }
            async Task<int> Families() { using var r = await app.GetAsync("/probe/status"); using var b = JsonDocument.Parse(await r.Content.ReadAsStringAsync()); return b.RootElement.GetProperty("session_families").GetInt32(); }
            var a = await Begin(); var b = await Begin();
            var au = await BrowserStart(a); var bu = await BrowserStart(b); // Both cookies exist before provider authorization.
            async Task<Uri> ProviderCallback(Uri authorization, string account)
            {
                Require(authorization.Authority == origin.Authority && authorization.AbsolutePath == "/provider/authorize");
                using var request = new HttpRequestMessage(HttpMethod.Get, authorization);
                request.Headers.Add("X-Probe-Account", account);
                using var response = await browser.SendAsync(request);
                Require(response.StatusCode == HttpStatusCode.Found);
                var target = response.Headers.Location!.IsAbsoluteUri ? response.Headers.Location : new Uri(authorization, response.Headers.Location);
                Require(target.Authority == origin.Authority && target.AbsolutePath == "/probe/callback");
                return target;
            }
            var callbackA = await ProviderCallback(au, "alice");
            var callbackB = await ProviderCallback(bu, "bob");
            var callbackQueryA = Microsoft.AspNetCore.WebUtilities.QueryHelpers.ParseQuery(callbackA.Query);
            var callbackQueryB = Microsoft.AspNetCore.WebUtilities.QueryHelpers.ParseQuery(callbackB.Query);
            callbackQueryA["code"] = callbackQueryB["code"];
            var crossedCallback = new Uri(Microsoft.AspNetCore.WebUtilities.QueryHelpers.AddQueryString(callbackA.GetLeftPart(UriPartial.Path), callbackQueryA.ToDictionary(p => p.Key, p => (string?)p.Value.ToString())));
            await Reject(await browser.GetAsync(crossedCallback));
            await Reject(await app.GetAsync(callbackA));
            using (var wrongCookie = new HttpRequestMessage(HttpMethod.Get, callbackA))
            {
                wrongCookie.Headers.Add("Cookie", "probe_" + a.Transaction + "=wrong");
                await Reject(await app.SendAsync(wrongCookie));
            }
            var ah = (await Follow(callbackA, "alice"))!; var bh = (await Follow(callbackB, "bob"))!;
            await Reject(await Exchange(a, ah, Guid.NewGuid().ToString("N"), verifier: b.Verifier));
            await Reject(await Exchange(a, bh, Guid.NewGuid().ToString("N")));
            await Reject(await Exchange(a, ah, Guid.NewGuid().ToString("N"), environment: "other"));
            await Reject(await Exchange(a, ah, Guid.NewGuid().ToString("N"), destination: "other"));
            var aid = Guid.NewGuid().ToString("N"); var result = await Good(a, ah, aid, "alice");
            Require(result == await Good(a, ah, aid, "alice"));
            await Reject(await Exchange(a, ah, Guid.NewGuid().ToString("N")));
            await Good(b, bh, Guid.NewGuid().ToString("N"), "bob");
            Require(UserId(issuer + ":alice") != UserId(issuer + ":bob"));
            var accountMismatch = await Begin(UserId(issuer + ":bob").ToString());
            await Follow(await BrowserStart(accountMismatch), "alice", reject: true);
            var state = await Begin(); var stateUrl = await BrowserStart(state);
            using var stateRequest = new HttpRequestMessage(HttpMethod.Get, stateUrl);
            stateRequest.Headers.Add("X-Probe-Account", "alice");
            using var stateResponse = await browser.SendAsync(stateRequest);
            Require(stateResponse.StatusCode == HttpStatusCode.Found);
            var brokerCallback = stateResponse.Headers.Location!.IsAbsoluteUri ? stateResponse.Headers.Location : new Uri(stateUrl, stateResponse.Headers.Location);
            Require(brokerCallback.Authority == origin.Authority && brokerCallback.AbsolutePath == "/probe/callback");
            var q = Microsoft.AspNetCore.WebUtilities.QueryHelpers.ParseQuery(brokerCallback.Query);
            q["state"] = B64(RandomNumberGenerator.GetBytes(32));
            var tampered = new Uri(Microsoft.AspNetCore.WebUtilities.QueryHelpers.AddQueryString(brokerCallback.GetLeftPart(UriPartial.Path), q.ToDictionary(p => p.Key, p => (string?)p.Value.ToString())));
            await Follow(tampered, "alice", reject: true);
            foreach (var fault in new[] { "nonce", "audience", "issuer", "signature" }) { var f = await Begin(); await Follow(await BrowserStart(f), "alice", fault, true); }
            var expiredLogin = await Begin(); var expiredUrl = await BrowserStart(expiredLogin); await Expire(expiredLogin, "login"); await Follow(expiredUrl, "alice", reject: true);
            var expiredHandoff = await Begin(); var eh = await Login(expiredHandoff); await Expire(expiredHandoff, "handoff"); await Reject(await Exchange(expiredHandoff, eh, Guid.NewGuid().ToString("N")));
            var expiredReceipt = await Begin(); var er = await Login(expiredReceipt); var erid = Guid.NewGuid().ToString("N"); await Good(expiredReceipt, er, erid, "alice"); await Expire(expiredReceipt, "receipt"); await Reject(await Exchange(expiredReceipt, er, erid));
            var lost = await Begin(); var lh = await Login(lost); var lid = Guid.NewGuid().ToString("N"); var before = await Families();
            var broken = false;
            try { using var r = await Exchange(lost, lh, lid, drop: true); _ = await r.Content.ReadAsStringAsync(); } catch (HttpRequestException) { broken = true; }
            Require(broken); var recovered = await Good(lost, lh, lid, "alice"); Require(recovered == await Good(lost, lh, lid, "alice")); Require(await Families() == before + 1); await Reject(await Exchange(lost, lh, Guid.NewGuid().ToString("N")));
            var concurrent = await Begin(); var ch = await Login(concurrent); var cid = Guid.NewGuid().ToString("N"); before = await Families();
            var pair = await Task.WhenAll(Good(concurrent, ch, cid, "alice"), Good(concurrent, ch, cid, "alice")); Require(pair[0] == pair[1]); Require(await Families() == before + 1);
            await File.WriteAllTextAsync(receipt, JsonSerializer.Serialize(new { passed = true, listenerOwnedBeforeServer = true, separateBrowserAndApp = true, concurrentCookies = true, issuerSubjectIdentity = true, equalEmailDistinctUsers = true, bindingRejections = true, providerValidation = true, expiryScopes = true, lostAcknowledgementRetry = true, concurrentAtomicRedemption = true, limitations = new[] { "synthetic provider and identities", "simulated browser", "no physical mobile or passkey acceptance", "no production API credentials" } }, Json));
        }
        finally { if (!child.HasExited) child.Kill(true); await child.WaitForExitAsync(); await stderr; await listener.StopAsync(); }
    }
    private static Guid UserId(string name)
    {
        var ns = Convert.FromHexString("6BA7B8119DAD11D180B400C04FD430C8");
        var hash = SHA1.HashData(ns.Concat(Encoding.UTF8.GetBytes(name)).ToArray());
        hash[6] = (byte)((hash[6] & 15) | 0x50); hash[8] = (byte)((hash[8] & 63) | 0x80);
        return Guid.ParseExact(Convert.ToHexString(hash.AsSpan(0, 16)), "N");
    }
}
