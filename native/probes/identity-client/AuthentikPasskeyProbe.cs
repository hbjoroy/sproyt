using System.Net.Http.Json;
using System.Text.Json;
using static DesktopHandoffClient;

// This receipt proves the OIDC/native handoff and its identity comparison only.
// The external device-use fixture must separately verify actual WebAuthn use.
internal static class AuthentikPasskeyProbe
{
    internal static async Task<int> Complete(string lab, string output, string issuer, string accountId, DesktopState state, JsonElement environment, bool passkey)
    {
        Check(OperatingSystem.IsWindows() && issuer == "http://localhost:19090/application/o/n01-lab/");
        Check(state.Attempts.Length == 1 && state.Attempts[0].Account == "alice" && state.Attempts[0].Result is not null);
        using var result = JsonDocument.Parse(state.Attempts[0].Result!);
        Check(result.RootElement.GetProperty("kind").GetString() == "non_api_fixture" && result.RootElement.GetProperty("user_id").GetString() == accountId);
        // Login read this state from its committed DPAPI journal before returning.
        Check(state.Config.SystemBrowser && state.Config.Issuer == issuer);
        string baselinePath = Path.Combine(lab, "passkey-baseline.json");
        string? sourceRevision = environment.TryGetProperty("sourceRevision", out var revision) ? revision.GetString() : null;
        string? baselineRevision = null;
        string? accessFingerprint = null;
        if (passkey)
        {
            Check(new FileInfo(baselinePath).Length is > 0 and <= 4096);
            using var baseline = JsonDocument.Parse(await File.ReadAllTextAsync(baselinePath));
            Check(baseline.RootElement.GetProperty("passed").GetBoolean() &&
                baseline.RootElement.GetProperty("scope").GetString() == "n01e_password_baseline" &&
                baseline.RootElement.GetProperty("issuer").GetString() == issuer &&
                baseline.RootElement.GetProperty("accountId").GetString() == accountId &&
                !baseline.RootElement.GetProperty("passkeyVerified").GetBoolean());
            baselineRevision = baseline.RootElement.GetProperty("sourceRevision").GetString();
            // Reconnect only to the broker that produced this DPAPI-persisted grant.
            // The transaction remains in the protected journal and control request;
            // the public handoff receipt contains only its non-capability digest.
            var broker = Loopback(state.Config.Broker, "/");
            Check(broker.AbsoluteUri == "http://127.0.0.1:19191/" && Proof(state.Config.Instance));
            using var api = new HttpClient(new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = false }) { BaseAddress = broker, Timeout = TimeSpan.FromSeconds(12) };
            using (var statusResponse = await api.GetAsync("/probe/status"))
            {
                Check(statusResponse.IsSuccessStatusCode);
                await statusResponse.Content.LoadIntoBufferAsync(4096);
                using var status = JsonDocument.Parse(await statusResponse.Content.ReadAsStringAsync());
                Check(status.RootElement.GetProperty("instance_id").GetString() == state.Config.Instance && status.RootElement.GetProperty("issuer").GetString() == issuer);
            }
            using var request = new HttpRequestMessage(HttpMethod.Post, "/probe/upstream-check")
            { Content = JsonContent.Create(new { transaction_id = state.Attempts[0].Transaction, user_id = accountId, action = "access-fingerprint" }) };
            request.Headers.Add("X-Probe-Control", "1");
            using var response = await api.SendAsync(request);
            Check(response.IsSuccessStatusCode);
            await response.Content.LoadIntoBufferAsync(1024);
            using var binding = JsonDocument.Parse(await response.Content.ReadAsStringAsync());
            Check(binding.RootElement.EnumerateObject().Count() == 1);
            accessFingerprint = binding.RootElement.GetProperty("access_fingerprint").GetString();
            Check(accessFingerprint is { Length: 64 } && accessFingerprint.All(c => c is >= '0' and <= '9' or >= 'a' and <= 'f'));
        }
        var receipt = new
        {
            passed = true,
            scope = passkey ? "n01e_passkey_handoff" : "n01e_password_baseline",
            issuer,
            accountId,
            sourceRevision,
            baselineSourceRevision = baselineRevision,
            baselineComparison = passkey,
            accessFingerprint,
            nativeWindowsHandoff = true,
            localStatePreserved = true,
            oidcVerified = true,
            passkeyVerified = false,
            limitations = new[] { "isolated localhost issuer and synthetic identity", "actual WebAuthn device use requires separate fixture verification", "no production API session" }
        };
        string json = JsonSerializer.Serialize(receipt, Json);
        await File.WriteAllTextAsync(Path.Combine(output, "receipt.json"), json);
        if (!passkey)
        {
            // Nonsecret identity metadata only; no code, proof, credential, or grant.
            var baseline = new { passed = true, scope = "n01e_password_baseline", issuer, accountId, sourceRevision, passkeyVerified = false };
            string temporary = Path.Combine(lab, ".passkey-baseline-" + Guid.NewGuid().ToString("N") + ".tmp");
            try
            {
                await File.WriteAllTextAsync(temporary, JsonSerializer.Serialize(baseline, Json));
                File.Move(temporary, baselinePath, overwrite: true);
            }
            finally { if (File.Exists(temporary)) File.Delete(temporary); }
        }
        Console.WriteLine(passkey ? "PASS N01e native handoff and baseline identity comparison; device proof remains external" : "PASS N01e password baseline native handoff");
        return 0;
    }
}
