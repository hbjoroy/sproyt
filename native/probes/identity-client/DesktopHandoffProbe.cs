using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Net.Sockets;
using System.Text.Json;
using System.Text.RegularExpressions;
using static DesktopHandoffClient;

internal static class DesktopHandoffProbe
{
    private sealed class Client : IAsyncDisposable
    {
        internal readonly Process Process;
        private readonly Task<string> errors;
        internal Client(string folder, string config, string fault) { Process = System.Diagnostics.Process.Start(Child(folder, config, fault))!; errors = Process.StandardError.ReadToEndAsync(); }
        internal async Task<JsonElement> Phase(string wanted)
        {
            using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
            for (int count = 0; count < 20; count++)
            {
                string? line = await Process.StandardOutput.ReadLineAsync(timeout.Token);
                Check(line is not null && line.Length < 2048);
                using var body = JsonDocument.Parse(line!);
                if (body.RootElement.GetProperty("phase").GetString() == wanted) return body.RootElement.Clone();
            }
            throw new InvalidOperationException("Desktop phase not reached.");
        }
        internal async Task Exit(int expected)
        {
            await Process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(20));
            Check(Process.ExitCode == expected); Check((await errors).Length < 4096);
        }
        internal async Task Kill()
        {
            if (!Process.HasExited) Process.Kill(entireProcessTree: true);
            await Process.WaitForExitAsync(); await errors;
        }
        public async ValueTask DisposeAsync() { await Kill(); Process.Dispose(); }
    }

    internal static async Task<int> Run(string server, string folder, string receiptPath, bool systemBrowser)
    {
        Check(OperatingSystem.IsWindows()); Directory.CreateDirectory(folder);
        var reservation = new TcpListener(IPAddress.Loopback, 0); reservation.Start();
        string callback = "http://127.0.0.1:" + ((IPEndPoint)reservation.LocalEndpoint).Port + "/return";
        var serverStart = new ProcessStartInfo(server) { UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden, RedirectStandardOutput = true, RedirectStandardError = true };
        foreach (string value in new[] { "--database", Path.Combine(folder, "broker.sqlite"), "--callback-uri", callback }) serverStart.ArgumentList.Add(value);
        if (systemBrowser) serverStart.ArgumentList.Add("--interactive");
        using var brokerProcess = Process.Start(serverStart)!;
        var serverErrors = brokerProcess.StandardError.ReadToEndAsync();
        try
        {
            string? ready = await brokerProcess.StandardOutput.ReadLineAsync().WaitAsync(TimeSpan.FromSeconds(25));
            Check(ready is not null && ready.StartsWith("N01_READY ", StringComparison.Ordinal) && ready.Length <= 4096);
            using var info = JsonDocument.Parse(ready![10..]);
            string broker = info.RootElement.GetProperty("address").GetString()!, instance = info.RootElement.GetProperty("instance_id").GetString()!;
            var origin = Loopback(broker, "/"); Check(Proof(instance));
            var serverOutput = brokerProcess.StandardOutput.ReadToEndAsync();
            reservation.Stop();
            using var app = new HttpClient(new HttpClientHandler { UseCookies = false, UseProxy = false, AllowAutoRedirect = false }) { BaseAddress = origin, Timeout = TimeSpan.FromSeconds(15) };
            using var browser = new HttpClient(new HttpClientHandler { CookieContainer = new CookieContainer(), UseProxy = false, AllowAutoRedirect = false }) { Timeout = TimeSpan.FromSeconds(25) };
            async Task<int> Families() { using var status = await app.GetFromJsonAsync<JsonDocument>("/probe/status"); return status!.RootElement.GetProperty("session_families").GetInt32(); }
            DesktopState Read(string path) { using var j = new WindowsProtectedJournal(path, false); return j.Read<DesktopState>(); }
            async Task<string> Configure(string name, string[] accounts, bool realBrowser = false)
            {
                string path = Path.Combine(folder, name + ".config.json");
                await File.WriteAllTextAsync(path, JsonSerializer.Serialize(new DesktopConfig(broker, instance, callback, accounts, realBrowser), Json)); return path;
            }
            async Task<Uri> Follow(string transaction, string account)
            {
                Uri url = new(origin, "/probe/browser/" + transaction);
                for (int hop = 0; hop < 8; hop++)
                {
                    Check(url.Scheme == "http" && url.Host == "127.0.0.1" && (url.Port == origin.Port || url.Port == new Uri(callback).Port));
                    if (url.AbsolutePath == "/return") return url;
                    using var response = await browser.GetAsync(url);
                    if (response.StatusCode == HttpStatusCode.Found) { url = new Uri(url, response.Headers.Location!); continue; }
                    Check(response.IsSuccessStatusCode && url.AbsolutePath == "/provider/authorize");
                    await response.Content.LoadIntoBufferAsync(8192);
                    Check(response.Headers.TryGetValues("Referrer-Policy", out var referrerPolicies) && referrerPolicies.Single() == "origin");
                    Check(response.Headers.TryGetValues("Content-Security-Policy", out var cspPolicies) &&
                        cspPolicies.Single().Contains("form-action 'self' " + new Uri(callback).GetLeftPart(UriPartial.Authority) + ";", StringComparison.Ordinal));
                    string page = await response.Content.ReadAsStringAsync();
                    var match = Regex.Match(page, "name=request value=\"([A-Za-z0-9_-]{43})\"", RegexOptions.CultureInvariant); Check(match.Success);
                    var fields = new Dictionary<string, string> { ["request"] = match.Groups[1].Value, ["account"] = account };
                    using (var missingOrigin = await browser.PostAsync(new Uri(origin, "/provider/choose"), new FormUrlEncodedContent(fields))) Check(missingOrigin.StatusCode == HttpStatusCode.BadRequest);
                    using (var missingCookie = new HttpRequestMessage(HttpMethod.Post, "/provider/choose") { Content = new FormUrlEncodedContent(fields) })
                    {
                        missingCookie.Headers.Add("Origin", broker);
                        using var rejected = await app.SendAsync(missingCookie); Check(rejected.StatusCode == HttpStatusCode.BadRequest);
                    }
                    using var selection = new HttpRequestMessage(HttpMethod.Post, new Uri(origin, "/provider/choose"))
                    { Content = new FormUrlEncodedContent(fields) };
                    selection.Headers.Add("Origin", broker);
                    using var selected = await browser.SendAsync(selection);
                    Check(selected.StatusCode == HttpStatusCode.Found); url = new Uri(url, selected.Headers.Location!);
                }
                throw new InvalidOperationException("Browser redirect limit.");
            }
            async Task Deliver(Uri handoff, bool interrupted = false)
            {
                try { using var response = await browser.GetAsync(handoff); if (!interrupted) Check(response.StatusCode == HttpStatusCode.Found); }
                catch (HttpRequestException) when (interrupted) { }
            }
            async Task<string> Expected(DesktopState state)
            {
                var a = state.Attempts.Single();
                using var response = await app.PostAsJsonAsync("/probe/exchange", new { transaction_id = a.Transaction, code = a.Code, verifier = a.Verifier, redemption_id = a.Redemption, environment = "n01-lab", destination_id = "windows-loopback" });
                Check(response.IsSuccessStatusCode); return await response.Content.ReadAsStringAsync();
            }

            if (systemBrowser)
            {
                string config = await Configure("browser", ["alice", "bob"], true), path = Path.Combine(folder, "browser");
                await using var client = new Client(path, config, "none");
                var a = await client.Phase("ready"); var b = await client.Phase("ready");
                Console.WriteLine("Desktop browser laboratory: choose Alice in the first tab and Bob in the second. Synthetic accounts only.");
                // The child, not this probe harness, launches the actual OS browser.
                await client.Process.WaitForExitAsync().WaitAsync(TimeSpan.FromMinutes(9)); await client.Exit(0);
                Check(Read(path).Attempts.All(x => x.Result is not null) && await Families() == 2);
                await File.WriteAllTextAsync(receiptPath, JsonSerializer.Serialize(new { passed = true, systemBrowserLaunched = true, twoAccountsCompleted = true, osProtectedCompletion = true, limitations = new[] { "synthetic provider", "not Authentik/passkey", "browser UI observation recorded separately" } }, Json));
                Console.WriteLine("PASS desktop system-browser handoff");
                return 0;
            }

            foreach (string fault in new[] { "return-durable", "exchange-unknown", "result-received", "result-durable" })
            {
                Console.WriteLine("Testing desktop app restart: " + fault);
                string path = Path.Combine(folder, fault), config = await Configure(fault, ["alice"]);
                int before = await Families();
                await using (var client = new Client(path, config, fault))
                {
                    var readyAttempt = await client.Phase("ready");
                    string transaction = readyAttempt.GetProperty("transaction").GetString()!;
                    // A second process cannot mutate the journal or replace its proof.
                    await using (var contender = new Client(path, config, "none")) await contender.Exit(1);
                    var target = await Follow(transaction, "alice");
                    var delivery = Deliver(target, interrupted: true);
                    await client.Phase(fault); await client.Kill(); await delivery;
                }
                DesktopState durable = Read(path);
                Check(durable.Attempts.Single().Code is not null);
                string? expected = fault == "return-durable" ? null : durable.Attempts.Single().Result ?? await Expected(durable);
                if (fault == "result-durable")
                {
                    using var expired = await app.PostAsJsonAsync("/fixture/expire", new { transaction_id = durable.Attempts.Single().Transaction, scope = "receipt" }); Check(expired.IsSuccessStatusCode);
                }
                await using (var resumed = new Client(path, config, "none")) { await resumed.Phase("completed"); await resumed.Exit(0); }
                DesktopState completed = Read(path);
                Check(completed.Attempts.Single().Result is not null && completed.Attempts.Single().Redemption == durable.Attempts.Single().Redemption && await Families() == before + 1);
                if (expected is not null) Check(completed.Attempts.Single().Result == expected);
                var ciphertext = await File.ReadAllBytesAsync(Path.Combine(path, "state.bin"));
                Check(!System.Text.Encoding.UTF8.GetString(ciphertext).Contains(completed.Attempts.Single().Verifier, StringComparison.Ordinal));
                Console.WriteLine("PASS desktop app restart: " + fault);
            }

            // Deliver B while resumed A owns the state across its exchange.
            // A deterministic gate proves that both protected commits survive.
            string racePath = Path.Combine(folder, "resume-race"), raceConfig = await Configure("resume-race", ["alice", "bob"]);
            int raceBefore = await Families();
            Uri targetDuringResume;
            await using (var initial = new Client(racePath, raceConfig, "return-durable"))
            {
                var a = await initial.Phase("ready"); var b = await initial.Phase("ready");
                var target = await Follow(a.GetProperty("transaction").GetString()!, "alice");
                targetDuringResume = await Follow(b.GetProperty("transaction").GetString()!, "bob");
                var delivery = Deliver(target, interrupted: true);
                await initial.Phase("return-durable"); await initial.Kill(); await delivery;
            }
            await using (var resumed = new Client(racePath, raceConfig, "resume-gated"))
            {
                await resumed.Phase("resume-gated");
                var delivery = Deliver(targetDuringResume);
                await resumed.Phase("callback-queued");
                using var release = new HttpRequestMessage(HttpMethod.Post, new Uri(new Uri(callback), "/fixture/resume-release"));
                release.Headers.Add("X-Probe-Control", "1");
                using var response = await browser.SendAsync(release); Check(response.IsSuccessStatusCode);
                await delivery; await resumed.Phase("completed"); await resumed.Exit(0);
            }
            Check(Read(racePath).Attempts.All(a => a.Result is not null) && await Families() == raceBefore + 2);

            // Two live attempts share browser cookies. Forged callbacks and a
            // duplicate for A must leave A/B able to complete independently.
            string dualPath = Path.Combine(folder, "dual"), dualConfig = await Configure("dual", ["alice", "bob"]);
            int dualBefore = await Families();
            await using (var client = new Client(dualPath, dualConfig, "none"))
            {
                var a = await client.Phase("ready"); var b = await client.Phase("ready");
                string ta = a.GetProperty("transaction").GetString()!, tb = b.GetProperty("transaction").GetString()!;
                using (var bad = await browser.GetAsync(callback + "?transaction_id=" + Random() + "&code=" + Random())) Check(bad.StatusCode == HttpStatusCode.BadRequest);
                using (var bad = await browser.PostAsync(callback, null)) Check(bad.StatusCode == HttpStatusCode.MethodNotAllowed);
                using (var bad = await browser.GetAsync(callback + "?transaction_id=" + ta + "&code=" + Random() + "&extra=1")) Check(bad.StatusCode == HttpStatusCode.BadRequest);
                using (var bad = await browser.GetAsync(callback + "?transaction_id=" + ta + "&code=" + Random())) Check(bad.StatusCode == HttpStatusCode.BadRequest);
                var targetA = await Follow(ta, "alice"); var targetB = await Follow(tb, "bob");
                await Deliver(targetA); await Deliver(targetA); await Deliver(targetB);
                await client.Phase("completed"); await client.Exit(0);
            }
            DesktopState dual = Read(dualPath);
            Check(dual.Attempts.All(a => a.Result is not null) && await Families() == dualBefore + 2);
            // Closed/invalid storage must fail rather than fall back to a new login.
            async Task RejectState(string name, DesktopState? replacement, bool corrupt = false, string? configOverride = null)
            {
                string path = Path.Combine(folder, name); Directory.CreateDirectory(path);
                using (var j = new WindowsProtectedJournal(path, true)) j.Write(replacement ?? dual);
                if (corrupt) { var bytes = await File.ReadAllBytesAsync(Path.Combine(path, "state.bin")); bytes[bytes.Length / 2] ^= 0x80; await File.WriteAllBytesAsync(Path.Combine(path, "state.bin"), bytes); }
                int before = await Families();
                await using (var client = new Client(path, configOverride ?? dualConfig, "none")) await client.Exit(1);
                Check(await Families() == before);
            }
            await RejectState("corrupt", null, corrupt: true);
            await RejectState("expired", dual with { Expires = DateTimeOffset.UtcNow.AddSeconds(-1) });
            await RejectState("wrong-environment", dual with { Environment = "other" });
            string wrongConfig = Path.Combine(folder, "wrong-instance.config.json");
            await File.WriteAllTextAsync(wrongConfig, JsonSerializer.Serialize(dual.Config with { Instance = Random() }, Json));
            await RejectState("wrong-instance", null, configOverride: wrongConfig);
            string blocked = Path.Combine(folder, "blocked"); Directory.CreateDirectory(blocked);
            using (var j = new WindowsProtectedJournal(blocked, true)) j.Write(dual);
            using (var locked = new FileStream(Path.Combine(blocked, "state.bin"), FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            { await using var client = new Client(blocked, dualConfig, "none"); await client.Exit(1); }
            await File.WriteAllTextAsync(receiptPath, JsonSerializer.Serialize(new { passed = true, currentUserDpapi = true, realProcessKills = 4, durableRedemptionIdentity = true,
                oneFamilyPerAttempt = true, exactRetryResult = true, exclusiveProcessOwner = true, forgedReturnRecovery = true, concurrentCookieAccounts = true, resumeCallbackRace = true,
                corruptUnavailableExpiredStorageRejected = true, environmentAndBrokerBinding = true, limitations = new[] { "simulated browser", "broker stays alive", "same Windows user is trusted", "not Authentik/passkey/API credentials" } }, Json));
            Console.WriteLine("PASS desktop protected handoff and app-process death");
            return 0;
        }
        finally
        {
            reservation.Stop(); if (!brokerProcess.HasExited) brokerProcess.Kill(entireProcessTree: true);
            await brokerProcess.WaitForExitAsync(); await serverErrors;
        }
    }
}
