using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using System.Text.RegularExpressions;
using static DesktopHandoffClient;

internal static class AuthentikRevocationProbe
{
    private static readonly string[] Slots = ["alice-one", "alice-two", "bob"];
    private sealed record Observation(bool AccessActive, bool RefreshActive, bool RefreshAccepted,
        bool RequiresReauthentication, bool? RenewedAccessActive, bool OldCookieAuthorized,
        long TokenElapsedMilliseconds, long SsoElapsedMilliseconds = 0);

    internal static async Task<int> Run(HttpClient api, JsonElement environment, JsonElement policy,
        string lab, string output, bool httpOnly, Func<string, string, Task<DesktopState>> login,
        Func<string, DesktopState> read)
    {
        string prefix = environment.GetProperty("prefix").GetString()!;
        Check(Regex.IsMatch(prefix, "^sproyt-n01c-[a-f0-9]{8}$", RegexOptions.CultureInvariant));
        string origin = environment.GetProperty("origin").GetString()!;
        Check(origin == "http://127.0.0.1:19090");
        string issuer = origin + "/application/o/n01-lab/";
        var scenarios = new List<object>();
        using var fixtureTimeout = new CancellationTokenSource(TimeSpan.FromMinutes(6));

        async Task<JsonElement> Fixture(string action)
        {
            Check(action is "prepare" or "disable-alice" or "restore-alice" or "delete-alice-one");
            var start = new ProcessStartInfo(OperatingSystem.IsWindows() ? "wslc" : "docker")
            {
                UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden,
                RedirectStandardOutput = true, RedirectStandardError = true
            };
            // Fixed script/action and a validated, owned container. No secrets on argv.
            foreach (string value in new[] { "exec", "--env", "N01D_ACTION=" + action, prefix + "-worker", "ak", "shell", "-c",
                "exec(compile(open('/lab-code/revocation-fixture.py').read(), '/lab-code/revocation-fixture.py', 'exec'))" }) start.ArgumentList.Add(value);
            using var process = Process.Start(start)!;
            var stdout = process.StandardOutput.ReadToEndAsync(fixtureTimeout.Token);
            var stderr = process.StandardError.ReadToEndAsync(fixtureTimeout.Token);
            try { await process.WaitForExitAsync(fixtureTimeout.Token); }
            finally { if (!process.HasExited) { process.Kill(true); await process.WaitForExitAsync(); } }
            string text = await stdout; await stderr;
            Check(process.ExitCode == 0 && text.Length < 65536);
            string[] lines = text.Split('\n').Select(s => s.Trim()).Where(s => s.StartsWith("N01D_FIXTURE ", StringComparison.Ordinal)).ToArray();
            Check(lines.Length == 1);
            using var result = JsonDocument.Parse(lines[0]["N01D_FIXTURE ".Length..]);
            Check(result.RootElement.GetProperty("applied").GetBoolean() && result.RootElement.GetProperty("action").GetString() == action);
            if (action == "prepare")
                await File.WriteAllTextAsync(Path.Combine(lab, "private", "revocation-sessions.json"), result.RootElement.GetRawText());
            return result.RootElement.Clone();
        }

        async Task<JsonElement> Control(DesktopAttempt attempt, string action)
        {
            using var result = JsonDocument.Parse(attempt.Result!);
            using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/upstream-check")
            { Content = JsonContent.Create(new { user_id = result.RootElement.GetProperty("user_id").GetString(), transaction_id = attempt.Transaction, action }) };
            request.Headers.Add("X-Probe-Control", "1");
            using var response = await api.SendAsync(request);
            Check(response.IsSuccessStatusCode);
            await response.Content.LoadIntoBufferAsync(1024);
            using var body = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
            return body.RootElement.Clone();
        }

        async Task<bool> OldCookieAuthorizes(string slot, JsonElement fixture)
        {
            string account = slot == "bob" ? "bob" : "alice";
            string expectedUser = UserId(issuer + ":" + policy.GetProperty("subjects").GetProperty(account).GetString()).ToString();
            string verifier = Random();
            using var begin = await api.PostAsJsonAsync("/probe/start", new { challenge = Challenge(verifier), platform = "windows",
                environment = "n01-lab", destination_id = "windows-loopback", expected_user_id = Guid.Parse(expectedUser) });
            Check(begin.IsSuccessStatusCode);
            using var started = JsonDocument.Parse(await begin.Content.ReadAsStringAsync());
            string transaction = started.RootElement.GetProperty("transaction_id").GetString()!;
            Check(Proof(transaction));
            var jar = new CookieContainer();
            jar.Add(new Uri(origin), new Cookie(fixture.GetProperty("cookie_name").GetString()!,
                fixture.GetProperty("sessions").GetProperty(slot).GetProperty("cookie").GetString()!, "/"));
            using var browser = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseProxy = false, CookieContainer = jar })
                { Timeout = TimeSpan.FromSeconds(20) };
            var target = new Uri(api.BaseAddress!, "/probe/browser/" + transaction);
            for (int hop = 0; hop < 16; hop++)
            {
                Check(target.Scheme == "http" && target.Host == "127.0.0.1" && target.Port is 19090 or 19191);
                using var response = await browser.GetAsync(target);
                if (response.StatusCode is HttpStatusCode.Found or HttpStatusCode.SeeOther or HttpStatusCode.TemporaryRedirect)
                {
                    var next = new Uri(target, response.Headers.Location!);
                    if (next.Scheme == "http" && next.Host == "127.0.0.1" && next.Port == 19192 && next.AbsolutePath == "/return")
                    {
                        var query = Microsoft.AspNetCore.WebUtilities.QueryHelpers.ParseQuery(next.Query);
                        Check(query.Count == 2 && query["transaction_id"].Count == 1 && query["transaction_id"].ToString() == transaction
                            && query["code"].Count == 1 && Proof(query["code"].ToString()));
                        using var redeemed = await api.PostAsJsonAsync("/probe/exchange", new { transaction_id = transaction,
                            code = query["code"].ToString(), verifier, redemption_id = Guid.NewGuid().ToString("N"),
                            environment = "n01-lab", destination_id = "windows-loopback" });
                        Check(redeemed.IsSuccessStatusCode);
                        using var result = JsonDocument.Parse(await redeemed.Content.ReadAsStringAsync());
                        Check(result.RootElement.GetProperty("user_id").GetString() == expectedUser);
                        return true;
                    }
                    target = next;
                    continue;
                }
                // A rendered authentication flow requires user interaction. This
                // is not a claim about locally validated JWTs or token invalidation.
                Check(response.IsSuccessStatusCode && target.Port == 19090 && target.AbsolutePath.StartsWith("/if/flow/", StringComparison.Ordinal));
                return false;
            }
            Check(false); return false;
        }

        try
        {
            foreach (string stimulus in new[] { "revoke-refresh", "delete-alice-one", "disable-alice" })
            {
                var fixture = await Fixture("prepare");
                var attempts = new Dictionary<string, DesktopAttempt>();
                var names = new Dictionary<string, string>();
                foreach (string slot in Slots)
                {
                    string name = stimulus + "-" + slot;
                    names.Add(slot, name);
                    attempts.Add(slot, (await login(name, slot)).Attempts.Single());
                }
                // Fresh sessions/grants for each stimulus, refreshed before the
                // baseline. Never reuse a grant after a previous destructive test.
                // SSO itself issues new grants. Complete all SSO checks before the
                // final token baseline, and all token measurements before new SSO.
                foreach (string slot in Slots) Check(await OldCookieAuthorizes(slot, fixture));
                foreach (string slot in Slots)
                {
                    var refreshed = await Control(attempts[slot], "refresh");
                    Check(refreshed.GetProperty("accepted").GetBoolean());
                    Check((await Control(attempts[slot], "introspect-access")).GetProperty("active").GetBoolean());
                    Check((await Control(attempts[slot], "introspect-refresh")).GetProperty("active").GetBoolean());
                }
                var observedAt = DateTimeOffset.UtcNow;
                var elapsed = Stopwatch.StartNew();
                if (stimulus == "revoke-refresh") Check((await Control(attempts["alice-one"], stimulus)).GetProperty("accepted").GetBoolean());
                else await Fixture(stimulus);
                var outcomes = new Dictionary<string, Observation>();
                foreach (string slot in Slots)
                {
                    bool access = (await Control(attempts[slot], "introspect-access")).GetProperty("active").GetBoolean();
                    bool refresh = (await Control(attempts[slot], "introspect-refresh")).GetProperty("active").GetBoolean();
                    var renewal = await Control(attempts[slot], "refresh");
                    bool accepted = renewal.GetProperty("accepted").GetBoolean();
                    bool reauthenticate = renewal.GetProperty("requires_reauthentication").GetBoolean();
                    Check(accepted || reauthenticate); // A network failure is not a revocation result.
                    bool? renewedAccess = accepted ? (await Control(attempts[slot], "introspect-access")).GetProperty("active").GetBoolean() : null;
                    outcomes.Add(slot, new(access, refresh, accepted, reauthenticate, renewedAccess, false, elapsed.ElapsedMilliseconds));
                    if (!httpOnly) Check(read(names[slot]).Attempts.Single().Result == attempts[slot].Result);
                }
                foreach (string slot in Slots)
                {
                    bool sso = await OldCookieAuthorizes(slot, fixture);
                    outcomes[slot] = outcomes[slot] with { OldCookieAuthorized = sso, SsoElapsedMilliseconds = elapsed.ElapsedMilliseconds };
                }
                static bool AllActive(Observation value) => value is { AccessActive: true, RefreshActive: true, RefreshAccepted: true,
                    RequiresReauthentication: false, RenewedAccessActive: true, OldCookieAuthorized: true };
                static bool AllDenied(Observation value) => value is { AccessActive: false, RefreshActive: false, RefreshAccepted: false,
                    RequiresReauthentication: true, RenewedAccessActive: null, OldCookieAuthorized: false };
                Check(AllActive(outcomes["bob"]));
                switch (stimulus)
                {
                    case "revoke-refresh":
                        Check(outcomes["alice-one"] is { AccessActive: true, RefreshActive: false, RefreshAccepted: false,
                            RequiresReauthentication: true, RenewedAccessActive: null, OldCookieAuthorized: true });
                        Check(AllActive(outcomes["alice-two"]));
                        break;
                    case "delete-alice-one":
                        Check(outcomes["alice-one"] is { AccessActive: false, RefreshActive: true, RefreshAccepted: true,
                            RequiresReauthentication: false, RenewedAccessActive: true, OldCookieAuthorized: false });
                        Check(AllActive(outcomes["alice-two"]));
                        break;
                    case "disable-alice":
                        Check(AllDenied(outcomes["alice-one"]) && AllDenied(outcomes["alice-two"]));
                        break;
                }
                scenarios.Add(new { stimulus, observedAt, baselineAllActive = true, outcomes });
                Console.WriteLine("PASS N01d measured " + stimulus);
            }
        }
        finally { await Fixture("restore-alice"); }

        await File.WriteAllTextAsync(Path.Combine(output, "receipt.json"), JsonSerializer.Serialize(new
        {
            passed = true, scope = httpOnly ? "n01d_http_contract" : "n01d_windows_lab", nativeWindowsHandoff = !httpOnly,
            sourceRevision = environment.GetProperty("sourceRevision").GetString(),
            authentikImage = environment.GetProperty("authImage").GetString(), postgresImage = environment.GetProperty("postgresImage").GetString(),
            authentication = "preauthenticated_synthetic_session_fixture", policy, scenarios,
            localStatePreserved = httpOnly ? (bool?)null : true,
            limitations = new[] { "isolated issuer and synthetic accounts", "admin stimuli via public API with preauthenticated test admin",
                "immediate sequential observations, not a propagation-time bound", "introspection is not offline JWT validation",
                "session deletion is admin revocation, not RP-initiated logout", "no passkey, recovery or production session acceptance" }
        }, Json));
        Console.WriteLine("PASS isolated Authentik revocation matrix");
        return 0;
    }
}
