using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Net.WebSockets;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;

internal sealed record NativeAuthConfig(string Broker, string Instance, string Issuer, string UserId,
    string Scope = "n03a_session_lab", string Environment = "n01-lab");
internal enum NativeAuthPhase { SignedOut, SigningIn, Active, Offline, AuthRequired }
internal sealed record NativeAuthSnapshot(NativeAuthPhase Phase, string? UserId, ulong Generation, bool PendingRevocation, bool Busy, long? LoginTicket = null, string? DisplayName = null, string? SessionStamp = null);
internal sealed record NativeSocketConnection(ClientWebSocket Socket, string UserId, string SessionStamp, ulong Generation);

// This owner deliberately has no credential getter. UI consumers only receive Snapshot.
// The shared DPAPI journal supplies exclusive process ownership and atomic durable writes.
internal sealed class NativeAuthSession : IDisposable
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private readonly object gate = new();
    private readonly SemaphoreSlim refreshGate = new(1, 1);
    private readonly SemaphoreSlim revokeGate = new(1, 1);
    private readonly WindowsProtectedJournal journal;
    private readonly HttpClient http;
    private readonly NativeAuthConfig config;
    private readonly Func<string, Task>? checkpoint;
    private Stored state;
    private bool disposed;
    private bool storageFailed;
    private int busy;
    internal event Action? Changed;

    // Classes, rather than positional records, prevent accidental token-bearing ToString().
    private sealed class Grant
    {
        public string Scope { get; set; } = "";
        public string Environment { get; set; } = "";
        [JsonPropertyName("instance_id")] public string InstanceId { get; set; } = "";
        [JsonPropertyName("user_id")] public string UserId { get; set; } = "";
        public string Family { get; set; } = "";
        public ulong Generation { get; set; }
        public ulong Epoch { get; set; }
        [JsonPropertyName("access_token")] public string AccessToken { get; set; } = "";
        [JsonPropertyName("refresh_token")] public string RefreshToken { get; set; } = "";
        [JsonPropertyName("revoke_token")] public string RevokeToken { get; set; } = "";
        [JsonPropertyName("expires_at")] public long ExpiresAt { get; set; }
        [JsonPropertyName("display_name")] public string? DisplayName { get; set; }
    }
    private sealed class Stored
    {
        public int Schema { get; set; } = 1;
        public NativeAuthConfig Config { get; set; } = new("", "", "", "");
        public long Lifecycle { get; set; }
        public NativeAuthPhase Phase { get; set; }
        public Grant? Credential { get; set; }
        public string? Operation { get; set; }
        public string? PendingRevoke { get; set; }
        public List<string> AdditionalRevocations { get; set; } = [];
    }

    internal static NativeAuthSession Open(string directory, NativeAuthConfig config, Func<string, Task>? checkpoint = null, HttpMessageHandler? handler = null)
    {
        WindowsProtectedJournal? journal = null;
        try
        {
            ValidateConfig(config);
            bool exists = File.Exists(Path.Combine(directory, "state.bin"));
            journal = new WindowsProtectedJournal(directory, create: !exists);
            var stored = exists ? journal.Read<Stored>() : new Stored { Config = config };
            Require(stored.Schema == 1 && stored.Config == config && stored.Lifecycle >= 0 && Enum.IsDefined(stored.Phase));
            Require(stored.PendingRevoke is null || Token(stored.PendingRevoke, "revoke", config));
            Require(stored.AdditionalRevocations is not null && stored.AdditionalRevocations.Count <= 7 && stored.AdditionalRevocations.All(token => Token(token, "revoke", config)) &&
                stored.AdditionalRevocations.Distinct(StringComparer.Ordinal).Count() == stored.AdditionalRevocations.Count &&
                (stored.PendingRevoke is not null || stored.AdditionalRevocations.Count == 0) && !stored.AdditionalRevocations.Contains(stored.PendingRevoke!));
            Require(stored.Operation is null || (stored.Operation.Length == 32 && stored.Operation.All(char.IsAsciiHexDigit) && stored.Credential is not null));
            if (stored.Credential is not null)
            {
                Validate(stored.Credential, config);
                Require(stored.Phase is NativeAuthPhase.Active or NativeAuthPhase.Offline && stored.PendingRevoke != stored.Credential.RevokeToken && !stored.AdditionalRevocations!.Contains(stored.Credential.RevokeToken));
                // Opening a journal never claims online authentication before server validation.
                stored.Phase = NativeAuthPhase.Offline;
            }
            else Require(stored.Phase is NativeAuthPhase.SignedOut or NativeAuthPhase.SigningIn or NativeAuthPhase.AuthRequired);
            journal.Write(stored);
            return new(journal, config, stored, checkpoint, handler);
        }
        catch { journal?.Dispose(); throw Failure(); }
    }
    private NativeAuthSession(WindowsProtectedJournal journal, NativeAuthConfig config, Stored state, Func<string, Task>? checkpoint, HttpMessageHandler? handler)
    {
        this.journal = journal; this.config = config; this.state = state; this.checkpoint = checkpoint;
        http = new HttpClient(handler ?? new HttpClientHandler { AllowAutoRedirect = false, UseCookies = false, UseProxy = config.Scope == "sproyt_native_v1" })
        { BaseAddress = new Uri(config.Broker), Timeout = TimeSpan.FromSeconds(12) };
    }
    internal NativeAuthSnapshot Snapshot
    {
        get { lock (gate) return new(state.Phase, state.Credential?.UserId, state.Credential?.Generation ?? 0, state.PendingRevoke is not null, busy > 0, state.Phase == NativeAuthPhase.SigningIn ? state.Lifecycle : null, state.Credential?.DisplayName, state.Credential is null ? null : Stamp(state.Lifecycle, state.Credential)); }
    }
    private void Notify() { try { Changed?.Invoke(); } catch { /* Observers cannot alter authentication results. */ } }
    private void Live() { if (disposed || storageFailed) throw Failure(); }
    private void Save()
    {
        try { journal.Write(state); }
        catch { storageFailed = true; state.Phase = NativeAuthPhase.AuthRequired; state.Credential = null; state.Operation = null; state.Lifecycle++; Notify(); throw Failure(); }
    }
    private bool Current(long lifecycle, Grant? grant = null) => !disposed && !storageFailed && state.Lifecycle == lifecycle && (grant is null || ReferenceEquals(state.Credential, grant));
    private void EnqueueRevocation(string? token)
    {
        if (token is null || state.PendingRevoke == token || state.AdditionalRevocations.Contains(token)) return;
        if (state.PendingRevoke is null) state.PendingRevoke = token;
        else { Require(state.AdditionalRevocations.Count < 7); state.AdditionalRevocations.Add(token); }
    }
    internal async Task<long> BeginLoginAsync(CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        await RetryRevocationAsync();
        long ticket;
        lock (gate)
        {
            Live(); cancellationToken.ThrowIfCancellationRequested(); Require(state.PendingRevoke is null && state.Credential is null);
            ticket = state.Lifecycle = checked(state.Lifecycle + 1); state.Phase = NativeAuthPhase.SigningIn; Save();
        }
        Notify(); return ticket;
    }
    internal async Task CompleteLoginAsync(long ticket, string originalExchangeJson)
    {
        try
        {
            Require(Encoding.UTF8.GetByteCount(originalExchangeJson) <= 8192);
            using var document = JsonDocument.Parse(originalExchangeJson);
            var root = document.RootElement;
            if (config.Scope == "n03a_session_lab") Require(root.GetProperty("kind").GetString() == "non_api_fixture");
            var grant = root.GetProperty("native_session").Deserialize<Grant>(Json)!; Validate(grant, config);
            Require(root.GetProperty("user_id").GetString() == grant.UserId);
            if (config.Scope == "n03a_session_lab") Require(root.GetProperty("session_family").GetString() == grant.Family);
            bool stale;
            lock (gate)
            {
                Live(); stale = !Current(ticket) || state.Phase != NativeAuthPhase.SigningIn;
                if (stale)
                {
                    // A cancelled browser handoff can still issue a fresh server family.
                    // Persist its cleanup before any HTTP, without replacing a newer session.
                    if (state.Credential?.Family == grant.Family) return;
                    EnqueueRevocation(grant.RevokeToken); Save();
                }
                else
                {
                    Require(state.PendingRevoke != grant.RevokeToken && !state.AdditionalRevocations.Contains(grant.RevokeToken));
                    state.Credential = grant; state.Operation = null; state.Phase = NativeAuthPhase.Offline; Save();
                }
            }
            Notify(); if (stale) await RetryRevocationAsync(); else await ResumeAsync();
        }
        catch { throw Failure(); }
    }
    private void Fence(long lifecycle)
    {
        lock (gate)
        {
            if (!Current(lifecycle)) return;
            state.Lifecycle = checked(state.Lifecycle + 1); state.Phase = NativeAuthPhase.AuthRequired;
            EnqueueRevocation(state.Credential?.RevokeToken); state.Credential = null; state.Operation = null; Save();
        }
        Notify();
    }
    private void Offline(long lifecycle)
    {
        lock (gate) { if (!Current(lifecycle) || state.Credential is null) return; state.Phase = NativeAuthPhase.Offline; Save(); }
        Notify();
    }
    private async Task<bool> ServerIdentity(long lifecycle)
    {
        using var response = await http.GetAsync(Endpoint("info"), HttpCompletionOption.ResponseHeadersRead);
        if (!response.IsSuccessStatusCode) { if (response.StatusCode == HttpStatusCode.Unauthorized) Fence(lifecycle); else Offline(lifecycle); return false; }
        using var document = JsonDocument.Parse(await Read(response)); var root = document.RootElement;
        if (!IdentityMatches(root)) { Fence(lifecycle); return false; }
        return true;
    }
    internal async Task ResumeAsync()
    {
        await RetryRevocationAsync();
        Grant grant; long lifecycle;
        lock (gate) { Live(); if (state.Credential is null) return; grant = state.Credential; lifecycle = state.Lifecycle; busy++; }
        Notify(); bool refresh = false;
        try
        {
            if (!await ServerIdentity(lifecycle)) return;
            lock (gate) { if (!Current(lifecycle, grant)) return; refresh = state.Operation is not null; }
            if (refresh) return;
            using var request = Authorized(HttpMethod.Get, Endpoint("probe"), grant.AccessToken);
            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead);
            if (response.StatusCode == HttpStatusCode.Unauthorized) { refresh = true; return; }
            if (!response.IsSuccessStatusCode) { Offline(lifecycle); return; }
            using var document = JsonDocument.Parse(await Read(response)); var root = document.RootElement;
            Require(root.GetProperty("user_id").GetString() == grant.UserId && root.GetProperty("family").GetString() == grant.Family && root.GetProperty("generation").GetUInt64() == grant.Generation && root.GetProperty("epoch").GetUInt64() == grant.Epoch);
            lock (gate) { if (!Current(lifecycle, grant)) return; state.Phase = NativeAuthPhase.Active; Save(); }
        }
        catch (Exception ex) when (Network(ex)) { Offline(lifecycle); }
        catch { Fence(lifecycle); throw Failure(); }
        finally { lock (gate) busy--; Notify(); if (refresh) await RefreshAsync(); }
    }
    internal async Task RefreshAsync()
    {
        await refreshGate.WaitAsync();
        Grant? grant = null; long lifecycle = -1;
        try
        {
            string operation;
            lock (gate)
            {
                Live(); if (state.Credential is null) return;
                grant = state.Credential; lifecycle = state.Lifecycle; busy++;
                state.Operation ??= Convert.ToHexString(RandomNumberGenerator.GetBytes(16)).ToLowerInvariant(); operation = state.Operation; Save();
            }
            Notify(); if (!await ServerIdentity(lifecycle)) return;
            lock (gate) if (!Current(lifecycle, grant)) return;
            using var request = Authorized(HttpMethod.Post, Endpoint("refresh"), grant.RefreshToken);
            request.Content = JsonContent.Create(new { operation_id = operation, generation = grant.Generation });
            using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead);
            if (response.StatusCode == HttpStatusCode.Unauthorized) { Fence(lifecycle); return; }
            if (!response.IsSuccessStatusCode) { Offline(lifecycle); return; }
            var next = JsonSerializer.Deserialize<Grant>(await Read(response), Json)!; Validate(next, config);
            Require(next.UserId == grant.UserId && next.Family == grant.Family && next.Generation == checked(grant.Generation + 1) && next.Epoch == grant.Epoch && next.RevokeToken == grant.RevokeToken);
            if (checkpoint is not null) await checkpoint("rotation-received");
            lock (gate)
            {
                if (!Current(lifecycle, grant)) return;
                state.Credential = next; state.Operation = null; state.Phase = NativeAuthPhase.Active; Save();
            }
            if (checkpoint is not null) await checkpoint("rotation-durable");
        }
        catch (Exception ex) when (Network(ex)) { if (lifecycle >= 0) Offline(lifecycle); }
        catch { if (lifecycle >= 0) Fence(lifecycle); throw Failure(); }
        finally { if (grant is not null) { lock (gate) busy--; } refreshGate.Release(); Notify(); }
    }
    internal async Task LogoutAsync()
    {
        lock (gate)
        {
            Live(); state.Lifecycle = checked(state.Lifecycle + 1); EnqueueRevocation(state.Credential?.RevokeToken);
            state.Credential = null; state.Operation = null; state.Phase = NativeAuthPhase.SignedOut; Save();
        }
        Notify(); await RetryRevocationAsync();
    }
    internal async Task RetryRevocationAsync()
    {
        await revokeGate.WaitAsync(); bool countedBusy = false;
        try
        {
            lock (gate) { Live(); if (state.PendingRevoke is null) return; busy++; countedBusy = true; }
            Notify();
            using (var info = await http.GetAsync(Endpoint("info"), HttpCompletionOption.ResponseHeadersRead))
            {
                if (!info.IsSuccessStatusCode) return;
                using var document = JsonDocument.Parse(await Read(info)); var root = document.RootElement;
                if (!IdentityMatches(root)) return;
            }
            // At most eight outstanding families; new login is blocked until this drains.
            for (int count = 0; count < 8; count++)
            {
                string token;
                lock (gate) { if (disposed || storageFailed || state.PendingRevoke is null) return; token = state.PendingRevoke; }
                using var request = Authorized(HttpMethod.Post, Endpoint("revoke"), token);
                using var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead);
                // Only a positive server acknowledgement releases durable revocation work.
                if (response.StatusCode != HttpStatusCode.NoContent) return;
                lock (gate)
                {
                    if (disposed || storageFailed || state.PendingRevoke != token) return;
                    state.PendingRevoke = state.AdditionalRevocations.FirstOrDefault();
                    if (state.AdditionalRevocations.Count > 0) state.AdditionalRevocations.RemoveAt(0);
                    Save();
                }
            }
        }
        catch (Exception ex) when (Network(ex)) { }
        catch { throw Failure(); }
        finally { if (countedBusy) { lock (gate) busy--; } revokeGate.Release(); Notify(); }
    }
    private static HttpRequestMessage Authorized(HttpMethod method, string path, string token)
    { var request = new HttpRequestMessage(method, path); request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", token); return request; }
    private static async Task<string> Read(HttpResponseMessage response)
    {
        Require(response.Content.Headers.ContentLength is null or <= 8192);
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(12));
        using var input = await response.Content.ReadAsStreamAsync(timeout.Token); using var output = new MemoryStream(); var buffer = new byte[1024]; int count;
        while ((count = await input.ReadAsync(buffer, timeout.Token)) != 0) { Require(output.Length + count <= 8192); output.Write(buffer, 0, count); }
        return Encoding.UTF8.GetString(output.ToArray());
    }
    private static bool Network(Exception ex) => ex is HttpRequestException or IOException or OperationCanceledException;
    private static bool Proof(string? text) => text is { Length: 43 } && text.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
    private static bool Token(string? text, string kind, NativeAuthConfig config)
    { string prefix = (config.Scope == "sproyt_native_v1" ? "sproyt-" : "n03a-") + kind + "."; return text is not null && text.StartsWith(prefix, StringComparison.Ordinal) && Proof(text[prefix.Length..]); }
    private static bool CanonicalUser(string text) => Guid.TryParseExact(text, "D", out var id) && id.ToString("D") == text;
    private static void Validate(Grant grant, NativeAuthConfig config)
    {
        // This is a shape bound, not authorization. Allow small client/server
        // clock differences; the server still enforces the actual five-minute expiry.
        const int clockSkewSeconds = 30;
        Require(grant is not null && grant.Scope == config.Scope && grant.Environment == config.Environment && grant.InstanceId == config.Instance && CanonicalUser(grant.UserId) && (config.UserId == "" || grant.UserId == config.UserId) && Proof(grant.Family) && Token(grant.AccessToken, "access", config) && Token(grant.RefreshToken, "refresh", config) && Token(grant.RevokeToken, "revoke", config) && grant.ExpiresAt > 0 && (config.Scope != "sproyt_native_v1" || grant.ExpiresAt <= DateTimeOffset.UtcNow.ToUnixTimeSeconds() + 300 + clockSkewSeconds));
    }
    internal static void ValidateConfig(NativeAuthConfig config)
    {
        var broker = new Uri(config.Broker, UriKind.Absolute);
        Require(broker.AbsolutePath == "/" && broker.Query == "" && broker.Fragment == "" && broker.UserInfo == "" && broker.AbsoluteUri.TrimEnd('/') == config.Broker.TrimEnd('/') && Proof(config.Instance));
        if (config.Scope == "n03a_session_lab")
            Require(config.Environment == "n01-lab" && broker.Scheme == "http" && broker.Host == "127.0.0.1" && broker.Port > 0 && config.Issuer == config.Broker.TrimEnd('/') + "/provider" && CanonicalUser(config.UserId));
        else
        {
            var issuer = new Uri(config.Issuer, UriKind.Absolute);
            Require(config.Scope == "sproyt_native_v1" && config.Environment == "canary" && broker.Scheme == "https" && issuer.Scheme == "https" && issuer.AbsoluteUri == config.Issuer && issuer.UserInfo == "" && issuer.Query == "" && issuer.Fragment == "" && (config.UserId == "" || CanonicalUser(config.UserId)));
        }
    }
    private string Endpoint(string operation) => config.Scope == "sproyt_native_v1" ? "/api/v1/native/" + (operation == "probe" ? "session" : operation) : "/session/" + operation;
    private bool IdentityMatches(JsonElement root) => root.GetProperty("scope").GetString() == config.Scope && root.GetProperty("environment").GetString() == config.Environment && root.GetProperty("instance_id").GetString() == config.Instance && root.GetProperty("issuer").GetString() == config.Issuer;

    private async Task<(Grant Grant, long Lifecycle)> TransportCredential(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested(); Require(config.Scope == "sproyt_native_v1");
        bool resume; bool refresh;
        lock (gate) { Live(); resume = state.Phase != NativeAuthPhase.Active; refresh = state.Credential is not null && state.Credential.ExpiresAt <= DateTimeOffset.UtcNow.ToUnixTimeSeconds() + 30; }
        if (resume) await ResumeAsync().WaitAsync(cancellationToken);
        else if (refresh) await RefreshAsync().WaitAsync(cancellationToken);
        lock (gate) { Live(); Require(state.Phase == NativeAuthPhase.Active && state.Credential is not null); return (state.Credential!, state.Lifecycle); }
    }
    internal async Task<HttpResponseMessage> SendAuthorizedAsync(HttpRequestMessage request, CancellationToken cancellationToken = default)
    {
        Require(request.RequestUri is { IsAbsoluteUri: false } && request.RequestUri.OriginalString == "/api/v1/native/commands" && request.Method == HttpMethod.Post && !request.Headers.Contains("Authorization") && !request.Headers.Contains("Proxy-Authorization") && request.Headers.Host is null && !request.Headers.Contains("Cookie") && request.Content?.Headers.Contains("Cookie") != true);
        var credential = await TransportCredential(cancellationToken);
        lock (gate) { Live(); Require(Current(credential.Lifecycle, credential.Grant)); request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", credential.Grant.AccessToken); }
        try
        {
            var response = await http.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, cancellationToken);
            request.Headers.Authorization = null;
            lock (gate) { if (!Current(credential.Lifecycle, credential.Grant)) { response.Dispose(); throw Failure(); } }
            if (response.StatusCode == HttpStatusCode.Unauthorized) Fence(credential.Lifecycle);
            return response;
        }
        catch (Exception ex) when (Network(ex)) { Offline(credential.Lifecycle); throw Failure(); }
        finally { request.Headers.Authorization = null; }
    }
    internal async Task<ClientWebSocket> ConnectWebSocketAsync(CancellationToken cancellationToken = default) => (await ConnectBoundWebSocketAsync(cancellationToken)).Socket;
    internal async Task<NativeSocketConnection> ConnectBoundWebSocketAsync(CancellationToken cancellationToken = default)
    {
        var credential = await TransportCredential(cancellationToken); var socket = new ClientWebSocket();
        try
        {
            socket.Options.SetRequestHeader("Authorization", "Bearer " + credential.Grant.AccessToken);
            socket.Options.CollectHttpResponseDetails = true;
            socket.Options.KeepAliveInterval = TimeSpan.FromSeconds(20);
            var endpoint = new UriBuilder(config.Broker) { Scheme = "wss", Path = "/api/v1/native/ws", Query = "", Fragment = "" }.Uri;
            await socket.ConnectAsync(endpoint, http, cancellationToken);
            lock (gate) { Live(); Require(Current(credential.Lifecycle, credential.Grant)); }
            return new(socket, credential.Grant.UserId, Stamp(credential.Lifecycle, credential.Grant), credential.Grant.Generation);
        }
        catch { bool unauthorized = socket.HttpStatusCode == HttpStatusCode.Unauthorized; socket.Dispose(); if (unauthorized) Fence(credential.Lifecycle); else Offline(credential.Lifecycle); throw Failure(); }
    }
    private static string Stamp(long lifecycle, Grant grant) => lifecycle.ToString(System.Globalization.CultureInfo.InvariantCulture) + ":" + grant.Family + ":" + grant.Generation.ToString(System.Globalization.CultureInfo.InvariantCulture);
    // Admission holds the owner gate only while a caller selects/starts work;
    // callers must return an asynchronous operation and await it outside the gate.
    internal T AdmitTransport<T>(string expectedUserId, string expectedSessionStamp, Func<T> operation)
    {
        lock (gate)
        {
            Live(); Require(state.Phase == NativeAuthPhase.Active && state.Credential is not null && state.Credential.UserId == expectedUserId && Stamp(state.Lifecycle, state.Credential) == expectedSessionStamp);
            return operation();
        }
    }
    private static void Require(bool condition) { if (!condition) throw Failure(); }
    private static InvalidOperationException Failure() => new("Native session operation failed.");
    public void Dispose()
    {
        lock (gate) { if (disposed) return; disposed = true; state.Credential = null; state.Operation = null; state.PendingRevoke = null; state.AdditionalRevocations.Clear(); journal.Dispose(); http.Dispose(); }
    }
}
