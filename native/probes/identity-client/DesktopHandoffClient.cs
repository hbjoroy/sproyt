using System.Diagnostics;
using System.Net;
using System.Net.Http.Json;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.Logging;

internal sealed record DesktopConfig(string Broker, string Instance, string Callback, string[] Accounts, bool SystemBrowser);
internal sealed record DesktopAttempt(string Transaction, string Account, string Verifier, string Redemption, string? Code = null, string? Result = null);
internal sealed record DesktopState(int Schema, string Environment, DesktopConfig Config, DateTimeOffset Expires, DesktopAttempt[] Attempts);

internal static class DesktopHandoffClient
{
    internal sealed class Failure(int line) : Exception { internal int Line { get; } = line; }
    internal static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web) { UnmappedMemberHandling = System.Text.Json.Serialization.JsonUnmappedMemberHandling.Disallow };
    internal static void Check(bool condition, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0) { if (!condition) throw new Failure(line); }
    internal static string Random() => Convert.ToBase64String(RandomNumberGenerator.GetBytes(32)).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    internal static string Challenge(string proof) => Convert.ToBase64String(SHA256.HashData(Encoding.ASCII.GetBytes(proof))).TrimEnd('=').Replace('+', '-').Replace('/', '_');
    internal static bool Proof(string value) => value.Length == 43 && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
    internal static Uri Loopback(string value, string path)
    {
        var uri = new Uri(value, UriKind.Absolute);
        Check(uri.Scheme == "http" && uri.Host == "127.0.0.1" && uri.Port > 0 && uri.AbsolutePath == path &&
            uri.Query == "" && uri.Fragment == "" && uri.UserInfo == "" && uri.AbsoluteUri.TrimEnd('/') == value.TrimEnd('/'));
        return uri;
    }
    internal static Guid UserId(string name)
    {
        byte[] hash = SHA1.HashData(Convert.FromHexString("6BA7B8119DAD11D180B400C04FD430C8").Concat(Encoding.UTF8.GetBytes(name)).ToArray());
        hash[6] = (byte)((hash[6] & 15) | 0x50); hash[8] = (byte)((hash[8] & 63) | 0x80);
        return Guid.ParseExact(Convert.ToHexString(hash.AsSpan(0, 16)), "N");
    }
    internal static ProcessStartInfo Child(string directory, string config, string fault) => new("dotnet")
    {
        UseShellExecute = false, CreateNoWindow = true, WindowStyle = ProcessWindowStyle.Hidden,
        RedirectStandardOutput = true, RedirectStandardError = true,
        ArgumentList = { typeof(Program).Assembly.Location, "--desktop-child", directory, config, fault }
    };
    private static void Signal(string phase, string? transaction = null, string? account = null) =>
        Console.WriteLine(JsonSerializer.Serialize(new { phase, transaction, account }, Json));
    private static void ValidateResult(DesktopConfig config, DesktopAttempt attempt, string result)
    {
        Check(result.Length <= 8192);
        using var body = JsonDocument.Parse(result);
        Check(body.RootElement.GetProperty("kind").GetString() == "non_api_fixture" &&
            Guid.Parse(body.RootElement.GetProperty("user_id").GetString()!) == UserId(config.Broker.TrimEnd('/') + "/provider:" + attempt.Account) &&
            body.RootElement.GetProperty("session_family").GetString() is string family && Proof(family) &&
            body.RootElement.GetProperty("credential").GetString() is string credential && credential.StartsWith("fixture-only.", StringComparison.Ordinal) && Proof(credential[13..]));
    }

    internal static async Task<int> Run(string directory, string configPath, string fault)
    {
        Check(OperatingSystem.IsWindows());
        Check(new FileInfo(configPath).Length <= 4096);
        var config = JsonSerializer.Deserialize<DesktopConfig>(await File.ReadAllTextAsync(configPath), Json)!;
        var broker = Loopback(config.Broker, "/"); var callback = Loopback(config.Callback, "/return");
        Check(Proof(config.Instance) && config.Accounts.Length is >= 1 and <= 2 && config.Accounts.Distinct().Count() == config.Accounts.Length && config.Accounts.All(a => a is "alice" or "bob"));
        Check(fault is "none" or "return-durable" or "exchange-unknown" or "result-received" or "result-durable" or "resume-gated");
        bool resume = File.Exists(Path.Combine(directory, "state.bin"));
        using var journal = new WindowsProtectedJournal(directory, create: !resume);
        using var app = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false }) { BaseAddress = broker, Timeout = TimeSpan.FromSeconds(12) };
        using (var status = await app.GetFromJsonAsync<JsonDocument>("/probe/status"))
            Check(status!.RootElement.GetProperty("instance_id").GetString() == config.Instance);
        DesktopState state;
        if (resume)
        {
            state = journal.Read<DesktopState>();
            Check(state.Schema == 1 && state.Environment == "n01-lab" && state.Expires > DateTimeOffset.UtcNow &&
                JsonSerializer.Serialize(state.Config, Json) == JsonSerializer.Serialize(config, Json) && state.Attempts.Length == config.Accounts.Length);
            Check(state.Attempts.Select(a => a.Transaction).Distinct().Count() == state.Attempts.Length &&
                state.Attempts.Select(a => a.Account).Order().SequenceEqual(config.Accounts.Order()) &&
                state.Attempts.All(a => config.Accounts.Contains(a.Account) && Proof(a.Transaction) && Proof(a.Verifier) && Guid.TryParseExact(a.Redemption, "N", out _) && (a.Code is null || Proof(a.Code)) && (a.Result is null || a.Code is not null)));
            foreach (var attempt in state.Attempts) if (attempt.Result is not null) ValidateResult(config, attempt, attempt.Result);
        }
        else state = new(1, "n01-lab", config, DateTimeOffset.UtcNow.AddMinutes(8), []);
        // Bind before starting/opening login. If an old callback port cannot
        // be reclaimed after death, fail safely rather than use a new port.
        var builder = WebApplication.CreateSlimBuilder(); builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(k => { k.Listen(IPAddress.Loopback, callback.Port); k.Limits.MaxRequestLineSize = 2048; k.Limits.MaxRequestBodySize = 8192; });
        await using var listener = builder.Build();
        var gate = new SemaphoreSlim(1, 1);
        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var resumeRelease = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        async Task Checkpoint(string phase)
        {
            Signal(phase);
            if (fault == phase) await Task.Delay(Timeout.InfiniteTimeSpan); // Parent kills a real process.
        }
        void Save(int index, DesktopAttempt attempt)
        {
            var next = state.Attempts.ToArray(); next[index] = attempt;
            var updated = state with { Attempts = next };
            journal.Write(updated); state = updated;
        }
        async Task<bool> Redeem(int index)
        {
            var attempt = state.Attempts[index]; Check(attempt.Code is not null);
            using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/exchange") { Content = JsonContent.Create(new
            { transaction_id = attempt.Transaction, code = attempt.Code, verifier = attempt.Verifier, redemption_id = attempt.Redemption, environment = "n01-lab", destination_id = "windows-loopback" }) };
            if (fault == "exchange-unknown") request.Headers.Add("X-Probe-Drop-Ack", "1");
            string result;
            try
            {
                using var response = await app.SendAsync(request);
                if (response.StatusCode == HttpStatusCode.BadRequest) { Save(index, attempt with { Code = null }); return false; }
                Check(response.IsSuccessStatusCode && response.Content.Headers.ContentLength is null or <= 8192);
                await response.Content.LoadIntoBufferAsync(8192);
                result = await response.Content.ReadAsStringAsync(); Check(result.Length <= 8192);
            }
            catch (HttpRequestException) { await Checkpoint("exchange-unknown"); throw; }
            catch (TaskCanceledException) { await Checkpoint("exchange-unknown"); throw; }
            ValidateResult(config, attempt, result);
            await Checkpoint("result-received");
            Save(index, attempt with { Result = result });
            await Checkpoint("result-durable");
            return true;
        }
        listener.Use(async (context, next) =>
        {
            context.Response.Headers.CacheControl = "no-store"; context.Response.Headers["Referrer-Policy"] = "no-referrer";
            context.Response.Headers["Content-Security-Policy"] = "default-src 'none'; frame-ancestors 'none'";
            await next(context);
        });
        listener.MapGet("/done", () => Results.Content("<!doctype html><meta charset=utf-8><title>Sprøyt · prøve fullført</title><h1>Prøveinnlogging fullført</h1><p>Berre fiktiv legitimasjon. Du kan lukke denne fana.</p>", "text/html"));
        listener.MapPost("/fixture/resume-release", (HttpContext context) =>
        {
            if (config.SystemBrowser || fault != "resume-gated" || context.Request.Headers["X-Probe-Control"] != "1") return Results.BadRequest();
            resumeRelease.TrySetResult(); return Results.Ok();
        });
        listener.MapGet("/return", async (HttpContext context) =>
        {
            if (context.Request.Host.Host != "127.0.0.1" || context.Request.Host.Port != callback.Port ||
                context.Request.Query.Count != 2 || context.Request.Query["transaction_id"].Count != 1 || context.Request.Query["code"].Count != 1) return Results.BadRequest();
            string transaction = context.Request.Query["transaction_id"].ToString(), code = context.Request.Query["code"].ToString();
            if (!Proof(transaction) || !Proof(code)) return Results.BadRequest();
            if (fault == "resume-gated") Signal("callback-queued");
            await gate.WaitAsync();
            try
            {
                int index = Array.FindIndex(state.Attempts, a => a.Transaction == transaction);
                if (index < 0) return Results.BadRequest();
                var attempt = state.Attempts[index];
                if (attempt.Result is not null) return attempt.Code == code ? Results.Redirect("/done") : Results.BadRequest();
                if (attempt.Code is not null && attempt.Code != code) return Results.Conflict();
                Save(index, attempt with { Code = code });
                await Checkpoint("return-durable");
                if (!await Redeem(index)) return Results.BadRequest();
                if (state.Attempts.All(a => a.Result is not null)) done.TrySetResult();
                return Results.Redirect("/done");
            }
            catch { done.TrySetException(new InvalidOperationException("Desktop exchange incomplete.")); return Results.StatusCode(503); }
            finally { gate.Release(); }
        });
        await listener.StartAsync();
        try
        {
            // Startup/resume and incoming callbacks share one state owner,
            // including across network awaits and protected journal commits.
            await gate.WaitAsync();
            try
            {
                if (!resume)
                {
                    var attempts = new List<DesktopAttempt>();
                    foreach (var account in config.Accounts)
                    {
                        string verifier = Random();
                        // The proof is encrypted durably before any browser opens.
                        using var response = await app.PostAsJsonAsync("/probe/start", new { challenge = Challenge(verifier), platform = "windows", environment = "n01-lab", destination_id = "windows-loopback", expected_user_id = UserId(config.Broker.TrimEnd('/') + "/provider:" + account) });
                        Check(response.IsSuccessStatusCode);
                        using var body = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
                        string transaction = body.RootElement.GetProperty("transaction_id").GetString()!; Check(Proof(transaction));
                        Check(body.RootElement.GetProperty("authorization_url").GetString() == config.Broker.TrimEnd('/') + "/probe/browser/" + transaction);
                        attempts.Add(new(transaction, account, verifier, Guid.NewGuid().ToString("N")));
                    }
                    state = state with { Attempts = attempts.ToArray() }; journal.Write(state);
                }
                for (int index = 0; index < state.Attempts.Length; index++)
                {
                    var attempt = state.Attempts[index];
                    if (attempt.Result is not null) continue;
                    if (attempt.Code is not null)
                    {
                        if (fault == "resume-gated") { Signal("resume-gated"); await resumeRelease.Task.WaitAsync(TimeSpan.FromSeconds(20)); }
                        Check(await Redeem(index));
                    }
                    else
                    {
                        Signal("ready", attempt.Transaction, attempt.Account);
                        if (config.SystemBrowser)
                        {
                            // Reconstruct from pinned origin and validated ID; do
                            // not shell-open a received or persisted URL.
                            string address = broker.GetLeftPart(UriPartial.Authority) + "/probe/browser/" + attempt.Transaction;
                            using var opened = Process.Start(new ProcessStartInfo(address) { UseShellExecute = true });
                        }
                    }
                }
            }
            finally { gate.Release(); }
            if (!state.Attempts.All(a => a.Result is not null)) await done.Task.WaitAsync(TimeSpan.FromMinutes(config.SystemBrowser ? 8 : 3));
            // Completion is emitted only after a successful protected commit.
            Signal("completed");
            await Task.Delay(600); // Let the browser load the code-free completion page.
            return 0;
        }
        finally { await listener.StopAsync(); gate.Dispose(); }
    }
}
