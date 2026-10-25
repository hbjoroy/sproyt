using System.Net;
using System.Text;
using System.Text.Json;

// Contract-only fixtures: no live access/refresh/revoke credentials are minted.
internal static class NativeLiveClientProbe
{
    private static void Check(bool value, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0) { if (!value) throw new DesktopHandoffClient.Failure(line); }
    private static void Reject(Action action) { bool rejected = false; try { action(); } catch { rejected = true; } Check(rejected); }
    private sealed class Fixture(NativeAuthConfig config) : HttpMessageHandler
    {
        internal bool OfflineCancel;
        internal bool OversizedInfo;
        internal string? CancelProof;
        internal string? Challenge;
        private const string Transaction = "TTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTT";
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Check(request.RequestUri!.Scheme == "https" && request.RequestUri.GetLeftPart(UriPartial.Authority) == config.Broker && request.Headers.Authorization is null && !request.Headers.Contains("Cookie"));
            object? body = null; var status = HttpStatusCode.OK;
            if (request.RequestUri.AbsolutePath == "/api/v1/native/info")
            {
                if (OversizedInfo) return new HttpResponseMessage(status) { Content = new StringContent(new string('x', 9000)) };
                body = new { scope = config.Scope, environment = config.Environment, instance_id = config.Instance, issuer = config.Issuer };
            }
            else if (request.RequestUri.AbsolutePath == "/api/v1/native/login")
            {
                using var json = JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellationToken));
                Check(!json.RootElement.TryGetProperty("expected_user_id", out _));
                Challenge = json.RootElement.GetProperty("challenge").GetString(); Check(NativeClientWire.Proof(Challenge));
                Check(json.RootElement.GetProperty("callback_port").GetInt32() is > 0 and <= 65535);
                body = new { transaction_id = Transaction, authorization_url = config.Broker + "/auth/native/browser/" + Transaction };
            }
            else if (request.RequestUri.AbsolutePath == "/api/v1/native/cancel-login")
            {
                using var json = JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellationToken));
                Check(json.RootElement.GetProperty("transaction_id").GetString() == Transaction && json.RootElement.GetProperty("environment").GetString() == "canary" && json.RootElement.GetProperty("destination_id").GetString() == "windows-loopback");
                CancelProof = json.RootElement.GetProperty("verifier").GetString(); Check(NativeClientWire.Proof(CancelProof));
                if (Challenge is not null) Check(DesktopHandoffClient.Challenge(CancelProof!) == Challenge);
                if (OfflineCancel) throw new HttpRequestException("Synthetic transport unavailable.");
                status = HttpStatusCode.NoContent;
            }
            else throw new InvalidOperationException("Unexpected fixture route.");
            return new HttpResponseMessage(status) { Content = body is null ? null : new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json") };
        }
    }
    private sealed class RefreshFixture(NativeAuthConfig config, int lifetime) : HttpMessageHandler
    {
        internal const string User = "c49b194b-5cf8-41b5-82b0-02eb254945ec";
        private static readonly string Family = new('F', 43);
        private ulong generation;
        internal object Grant(int seconds) => new
        {
            scope = config.Scope, environment = config.Environment, instance_id = config.Instance,
            user_id = User, family = Family, generation, epoch = 0,
            access_token = "sproyt-access." + new string(generation == 0 ? 'A' : 'B', 43),
            refresh_token = "sproyt-refresh." + new string(generation == 0 ? 'C' : 'D', 43),
            revoke_token = "sproyt-revoke." + new string('R', 43),
            expires_at = DateTimeOffset.UtcNow.ToUnixTimeSeconds() + seconds, display_name = "Synthetic account",
        };
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Check(request.RequestUri!.GetLeftPart(UriPartial.Authority) == config.Broker && !request.Headers.Contains("Cookie"));
            object body;
            switch (request.RequestUri.AbsolutePath)
            {
                case "/api/v1/native/info":
                    body = new { scope = config.Scope, environment = config.Environment, instance_id = config.Instance, issuer = config.Issuer }; break;
                case "/api/v1/native/session":
                    Check(request.Headers.Authorization?.Parameter == "sproyt-access." + new string(generation == 0 ? 'A' : 'B', 43));
                    body = new { user_id = User, family = Family, generation, epoch = 0 }; break;
                case "/api/v1/native/refresh":
                    Check(request.Method == HttpMethod.Post && request.Headers.Authorization?.Parameter == "sproyt-refresh." + new string('C', 43));
                    using (var input = JsonDocument.Parse(await request.Content!.ReadAsStringAsync(cancellationToken)))
                        Check(input.RootElement.GetProperty("generation").GetUInt64() == generation);
                    generation++; body = Grant(lifetime); break;
                default: throw new InvalidOperationException("Unexpected refresh fixture route.");
            }
            return new HttpResponseMessage(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(body), Encoding.UTF8, "application/json") };
        }
    }
    internal static async Task<int> Run(string directory, string receipt)
    {
        Check(OperatingSystem.IsWindows() && !Directory.Exists(directory)); Directory.CreateDirectory(directory);
        var config = new NativeAuthConfig("https://native.example.invalid", new string('I', 43), "https://identity.example.invalid/application/o/sproyt/", "", "sproyt_native_v1", "canary");
        NativeAuthSession.ValidateConfig(config);
        Reject(() => NativeAuthSession.ValidateConfig(config with { Broker = "http://native.example.invalid" }));
        Reject(() => NativeAuthSession.ValidateConfig(config with { Broker = "https://native.example.invalid/elsewhere" }));
        Reject(() => NativeAuthSession.ValidateConfig(config with { Environment = "production" }));
        Reject(() => NativeAuthSession.ValidateConfig(config with { UserId = "unverified-account" }));
        Reject(() => NativeAuthSession.ValidateConfig(config with { Issuer = "http://identity.example.invalid/provider" }));
        string skewDirectory = Path.Combine(directory, "clock-skew");
        using (var owner = NativeAuthSession.Open(skewDirectory, config, handler: new RefreshFixture(config, 302)))
        {
            var initial = new RefreshFixture(config, 302);
            long ticket = await owner.BeginLoginAsync();
            await owner.CompleteLoginAsync(ticket, JsonSerializer.Serialize(new { user_id = RefreshFixture.User, native_session = initial.Grant(120) }));
            await owner.RefreshAsync();
            Check(owner.Snapshot.Phase == NativeAuthPhase.Active && owner.Snapshot.Generation == 1);
        }
        using (var owner = NativeAuthSession.Open(skewDirectory, config))
            Check(owner.Snapshot.Phase == NativeAuthPhase.Offline && owner.Snapshot.Generation == 1);
        using (var owner = NativeAuthSession.Open(Path.Combine(directory, "clock-skew-too-large"), config, handler: new RefreshFixture(config, 3600)))
        {
            var initial = new RefreshFixture(config, 3600);
            long ticket = await owner.BeginLoginAsync();
            await owner.CompleteLoginAsync(ticket, JsonSerializer.Serialize(new { user_id = RefreshFixture.User, native_session = initial.Grant(120) }));
            bool rejected = false; try { await owner.RefreshAsync(); } catch { rejected = true; }
            Check(rejected && owner.Snapshot.Phase == NativeAuthPhase.AuthRequired && owner.Snapshot.UserId is null);
        }
        string ownerDirectory = Path.Combine(directory, "owner"), loginDirectory = Path.Combine(directory, "login");
        string? originalProof;
        using (var owner = NativeAuthSession.Open(ownerDirectory, config))
        {
            Check(owner.Snapshot.Phase == NativeAuthPhase.SignedOut && owner.Snapshot.UserId is null);
            bool blocked = false;
            try { using var response = await owner.SendAuthorizedAsync(new HttpRequestMessage(HttpMethod.Post, "https://elsewhere.example.invalid/api/v1/native/commands")); } catch { blocked = true; }
            Check(blocked);
            var fixture = new Fixture(config) { OfflineCancel = true };
            var opened = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
            await using var login = new NativeLoginClient(owner, config, loginDirectory, fixture, _ => opened.TrySetResult());
            var running = login.LoginAsync(); await opened.Task.WaitAsync(TimeSpan.FromSeconds(10));
            Check(login.IsPending && File.Exists(Path.Combine(loginDirectory, "state.bin")));
            var cancellation = login.CancelAsync(); await owner.LogoutAsync(); await cancellation;
            try { await running; } catch (OperationCanceledException) { }
            Check(login.IsPending && owner.Snapshot.Phase == NativeAuthPhase.SignedOut);
            originalProof = fixture.CancelProof; Check(originalProof is not null);
            Check(!Encoding.UTF8.GetString(await File.ReadAllBytesAsync(Path.Combine(loginDirectory, "state.bin"))).Contains(originalProof!, StringComparison.Ordinal));
        }
        using (var owner = NativeAuthSession.Open(ownerDirectory, config))
        {
            var fixture = new Fixture(config); bool opened = false;
            await using var login = new NativeLoginClient(owner, config, loginDirectory, fixture, _ => opened = true);
            Check(login.IsPending); await login.ResumePendingAsync();
            Check(!login.IsPending && !opened && fixture.CancelProof == originalProof && owner.Snapshot.Phase == NativeAuthPhase.SignedOut);
        }
        using (var owner = NativeAuthSession.Open(Path.Combine(directory, "bounded-owner"), config))
        {
            bool opened = false; var fixture = new Fixture(config) { OversizedInfo = true };
            await using var login = new NativeLoginClient(owner, config, Path.Combine(directory, "bounded-login"), fixture, _ => opened = true);
            bool rejected = false; try { await login.LoginAsync(); } catch { rejected = true; }
            Check(rejected && !opened && !login.IsPending && owner.Snapshot.Phase == NativeAuthPhase.SignedOut);
        }
        var message = new { id = "36c2229f-b714-4712-9f72-14ba290aa498", channel_id = "c8940bd1-9acf-4108-8e8b-499b02a57482", sender_id = "c49b194b-5cf8-41b5-82b0-02eb254945ec", sender_display_name = "Testperson", body = "Ei ekte wireform – utan nettverksinnsending.", sent_at = "2026-10-25T12:00:00Z", sequence = 42UL };
        foreach (string type in new[] { "message_accepted", "message_edited", "message_deleted", "chat", "messages_loaded", "subscription_started" })
        {
            object payload = type switch { "chat" => new { @event = new { type = "message_accepted", message } }, "messages_loaded" => new { channel_id = message.channel_id, messages = new[] { message } }, "subscription_started" => new { channel_id = message.channel_id, history = new[] { message } }, _ => new { message } };
            using var json = JsonDocument.Parse(JsonSerializer.Serialize(new { protocol = "sproyt.chat.v1", type, payload }));
            var decoded = NativeChatClient.DecodeMessageEvents(json.RootElement);
            Check(decoded.Count == 1 && decoded[0].Id == message.id && decoded[0].ChannelId == message.channel_id && decoded[0].AuthorId == message.sender_id && decoded[0].Author == message.sender_display_name && decoded[0].Body == message.body && decoded[0].Sequence == 42 && decoded[0].SentAt == DateTimeOffset.Parse(message.sent_at, System.Globalization.CultureInfo.InvariantCulture));
        }
        using (var json = JsonDocument.Parse(JsonSerializer.Serialize(new { protocol = "wrong", type = "message_accepted", payload = new { message } }))) Reject(() => NativeChatClient.DecodeMessageEvents(json.RootElement));
        using (var json = JsonDocument.Parse(JsonSerializer.Serialize(new { protocol = "sproyt.chat.v1", type = "messages_loaded", payload = new { messages = Enumerable.Repeat(message, 1001).ToArray() } }))) Reject(() => NativeChatClient.DecodeMessageEvents(json.RootElement));
        var cached = new NativeChatSnapshot(true, false, null, new[] { new NativeChannel(message.channel_id, "Privat kanal") }, message.channel_id,
            new[] { new NativeMessage(message.id, message.channel_id, message.sender_id, message.sender_display_name, message.body, DateTimeOffset.UtcNow, 42) }, message.sender_id);
        var sameAccount = new NativeAuthSnapshot(NativeAuthPhase.Offline, message.sender_id, 3, false, false);
        Check(ReferenceEquals(NativeChatClient.FenceAccountProjection(cached, sameAccount), cached));
        var otherAccount = sameAccount with { UserId = "6f58162d-a337-416b-a413-0753d8312303" };
        var fenced = NativeChatClient.FenceAccountProjection(cached, otherAccount);
        Check(!fenced.Connected && fenced.UserId == otherAccount.UserId && fenced.Channels.Count == 0 && fenced.Messages.Count == 0 && fenced.SelectedChannel is null);
        foreach (var phase in new[] { NativeAuthPhase.SignedOut, NativeAuthPhase.SigningIn, NativeAuthPhase.AuthRequired })
        { var cleared = NativeChatClient.FenceAccountProjection(cached, sameAccount with { Phase = phase }); Check(cleared.Channels.Count == 0 && cleared.Messages.Count == 0 && cleared.SelectedChannel is null); }
        // Pause A before socket capture, exactly where durable delivery can yield.
        // B replaces the connection before A resumes; the real admission helper
        // must reject before invoking B's write delegate.
        string accountA = message.sender_id, stampA = "1:family-a:0";
        string currentAccount = accountA, currentStamp = stampA; object currentSocket = new(); int bFrames = 0;
        var paused = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var delayedA = Task.Run(async () =>
        {
            paused.SetResult(); await release.Task;
            bool rejected = false;
            try { NativeChatClient.AdmitSocket(accountA, stampA, currentAccount, currentStamp, currentSocket, currentSocket, () => ++bFrames); }
            catch { rejected = true; }
            return rejected;
        });
        await paused.Task.WaitAsync(TimeSpan.FromSeconds(2));
        currentAccount = otherAccount.UserId!; currentStamp = "3:family-b:0"; currentSocket = new object(); release.SetResult();
        Check(await delayedA.WaitAsync(TimeSpan.FromSeconds(2)) && bFrames == 0);
        Reject(() => NativeChatClient.AdmitSocket(accountA, stampA, accountA, "3:family-a-new:0", currentSocket, currentSocket, () => ++bFrames));
        var capturedSocket = new object();
        Reject(() => NativeChatClient.AdmitSocket(accountA, stampA, accountA, stampA, currentSocket, capturedSocket, () => ++bFrames));
        Check(bFrames == 0);
        Check(NativeChatClient.AdmitSocket(accountA, stampA, accountA, stampA, currentSocket, currentSocket, () => 1) == 1);
        await File.WriteAllTextAsync(receipt, JsonSerializer.Serialize(new { passed = true, contract_fixtures_only = true, no_live_credentials = true, https_origin_scope_fences = true, no_alternate_transport_origin = true, durable_proof_before_browser = true, offline_proof_cancel = true, cancel_restart_same_proof = true, no_resume_browser_reopen = true, response_bound = true, immutable_message_wire_projection = true, accepted_and_broadcast_wire = true, history_wire_bound = true, offline_account_projection_fence = true, signing_in_projection_fence = true, delayed_account_a_no_account_b_frame = true, same_account_new_family_fence = true, captured_socket_replacement_fence = true }));
        Console.WriteLine("PASS native live client contract probe"); return 0;
    }
}
