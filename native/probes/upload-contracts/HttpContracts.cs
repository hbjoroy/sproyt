using System.Net;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Sproyt.WindowsProbe;

internal static partial class Program
{
    private static async Task HttpContracts()
    {
        var local = new Local(); var source = local.Source(29); var wire = new Wire(local);
        var job = new NativeUploadJob(Guid.NewGuid().ToString("D"), Target, source, Guid.NewGuid().ToString("D"));
        bool wrongOwner = false, oversized = false;
        var client = new NativeUploadHttpClient(async (request, scope, token) =>
        {
            Require(scope == Target.Scope && request.RequestUri is { IsAbsoluteUri: false }, "HTTP adapter changed owner or emitted an absolute URL.");
            string path = request.RequestUri!.OriginalString; object payload;
            if (oversized) return new(HttpStatusCode.OK) { Content = new StreamContent(new NonSeekable(new byte[NativeUploadHttpClient.MaximumReplyBytes + 1])) };
            if (path.EndsWith("/config", StringComparison.Ordinal)) payload = new { enabled = true, max_file_bytes = 1024 * 1024 * 1024, chunk_bytes = NativeUploadHttpClient.ChunkBytes, allowed_channel_ids = new[] { Channel } };
            else if (path == "/api/v1/native/channels/" + Channel + "/media-uploads")
            {
                using var body = JsonDocument.Parse(await request.Content!.ReadAsByteArrayAsync(token));
                Require(body.RootElement.GetProperty("request_id").GetString() == job.RequestId && body.RootElement.GetProperty("filename").GetString() == source.FileName,
                    "Create changed stable request identity or filename."); payload = StatusJson(await wire.CreateAsync(job, token));
            }
            else if (path.Contains("/chunks/", StringComparison.Ordinal))
            {
                byte[] bytes = await request.Content!.ReadAsByteArrayAsync(token); string hash = request.Headers.GetValues("x-chunk-sha256").Single();
                Require(bytes.Length <= NativeUploadHttpClient.ChunkBytes && hash == Convert.ToHexStringLower(SHA256.HashData(bytes)), "HTTP chunk body/hash mismatch.");
                payload = StatusJson(await wire.PutChunkAsync(job, bytes, bytes.Length, hash, token));
            }
            else if (path.EndsWith("/complete", StringComparison.Ordinal))
            {
                var attachment = await wire.CompleteAsync(job, token);
                payload = new { media = new { id = attachment.MediaId, owner_id = wrongOwner ? Channel : User, channel_id = Channel,
                    original_filename = attachment.FileName, content_type = attachment.ContentType, size_bytes = attachment.SizeBytes, sha256 = attachment.Sha256 }, url = "/api/v1/media/" + attachment.MediaId };
            }
            else throw new InvalidOperationException("Unexpected HTTP fixture route.");
            return Json(payload);
        });
        await client.ConfigurationAsync(Target, default); var created = await client.CreateAsync(job, default); job = job with { UploadId = created.Id };
        byte[] content = local.Bytes[source.SourceId]; var status = await client.PutChunkAsync(job, content, content.Length, source.Sha256, default);
        job = job with { NextChunk = status.NextChunk, UploadedBytes = status.ReceivedBytes };
        Require((await client.CompleteAsync(job, default)).Sha256 == source.Sha256, "HTTP completion lost original bytes.");
        wrongOwner = true; await Throws<IOException>(() => client.CompleteAsync(job, default));
        oversized = true; await Throws<IOException>(() => client.ConfigurationAsync(Target, default));
    }
    private static object StatusJson(NativeUploadStatus status) => new { id = status.Id, channel_id = status.ChannelId, original_filename = status.FileName,
        declared_type = status.ContentType, size_bytes = status.SizeBytes, received_bytes = status.ReceivedBytes, next_chunk = status.NextChunk,
        state = status.State, media_id = status.MediaId, chunk_bytes = status.ChunkBytes, expires_at = DateTimeOffset.UtcNow.ToUnixTimeSeconds() + 3600 };
    private static HttpResponseMessage Json(object payload) => new(HttpStatusCode.OK) { Content = new StringContent(JsonSerializer.Serialize(payload), Encoding.UTF8, "application/json") };

    private static async Task AuthContracts(string directory)
    {
        var config = new NativeAuthConfig("https://native.example.invalid", new string('I', 43), "https://identity.example.invalid/", "", "sproyt_native_v1", "canary");
        var handler = new AuthHandler(config);
        using var owner = NativeAuthSession.Open(Path.Combine(directory, "owner-" + Guid.NewGuid().ToString("N")), config, handler: handler);
        long ticket = await owner.BeginLoginAsync(); await owner.CompleteLoginAsync(ticket, JsonSerializer.Serialize(new { user_id = User, native_session = handler.Grant }));
        var scope = owner.MediaScope(); var client = new NativeUploadHttpClient(owner);
        await client.ConfigurationAsync(Target with { FamilyStamp = scope.FamilyStamp }, default);
        Require(handler.UploadRequests == 1 && handler.LastRequest?.Headers.Authorization is null && !handler.LastRequest!.Headers.Contains("X-Sproyt-Account"),
            "Bearer/account authority leaked through a returned request.");
        foreach (string path in new[] { "/api/v1/native/commands", "/api/v1/media/" + Media, "/api/v1/native/media-uploads/config?extra=1", "https://evil.invalid/api/v1/native/media-uploads/config" })
        {
            using var request = new HttpRequestMessage(HttpMethod.Get, path); await Throws<InvalidOperationException>(() => owner.SendUploadAsync(request, scope, default));
        }
        await owner.RefreshAsync(); await client.ConfigurationAsync(Target with { FamilyStamp = scope.FamilyStamp }, default);
        Require(handler.UploadRequests == 2 && handler.UploadGenerations.SequenceEqual([0, 1]), "Rotation did not use a fresh concrete grant for the same upload family.");
        handler.Hold = true; handler.Entered = new(TaskCreationOptions.RunContinuationsAsynchronously);
        var pending = client.ConfigurationAsync(Target with { FamilyStamp = scope.FamilyStamp }, default);
        await handler.Entered.Task.WaitAsync(TimeSpan.FromSeconds(5)); await owner.LogoutAsync(); handler.Release.TrySetResult();
        await Throws<InvalidOperationException>(() => pending); int dispatched = handler.UploadRequests;
        await Throws<InvalidOperationException>(() => client.ConfigurationAsync(Target with { FamilyStamp = scope.FamilyStamp }, default));
        Require(handler.UploadRequests == dispatched, "Logged-out upload dispatched with another credential.");
    }
    private sealed class AuthHandler(NativeAuthConfig config) : HttpMessageHandler
    {
        private const string Family = "FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF";
        private int _generation;
        internal int UploadRequests;
        internal bool Hold;
        internal HttpRequestMessage? LastRequest;
        internal readonly List<int> UploadGenerations = [];
        internal TaskCompletionSource Entered = new(TaskCreationOptions.RunContinuationsAsynchronously), Release = new(TaskCreationOptions.RunContinuationsAsynchronously);
        internal object Grant => new { scope = config.Scope, environment = config.Environment, instance_id = config.Instance, user_id = User,
            family = Family, generation = _generation, epoch = 0, display_name = "Upload fixture", access_token = "sproyt-access." + new string(_generation == 0 ? 'A' : 'B', 43),
            refresh_token = "sproyt-refresh." + new string('R', 43), revoke_token = "sproyt-revoke." + new string('V', 43), expires_at = DateTimeOffset.UtcNow.ToUnixTimeSeconds() + 120 };
        protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
        {
            Require(request.RequestUri!.Host == "native.example.invalid" && !request.Headers.Contains("Cookie"), "Session dispatched outside its configured origin or used cookies.");
            object payload;
            switch (request.RequestUri.AbsolutePath)
            {
                case "/api/v1/native/info": payload = new { scope = config.Scope, environment = config.Environment, instance_id = config.Instance, issuer = config.Issuer }; break;
                case "/api/v1/native/session": payload = new { user_id = User, family = Family, generation = _generation, epoch = 0 }; break;
                case "/api/v1/native/refresh": _generation++; payload = Grant; break;
                case "/api/v1/native/revoke": return new(HttpStatusCode.NoContent);
                case "/api/v1/native/media-uploads/config":
                    Require(request.Headers.Authorization?.Parameter == "sproyt-access." + new string(_generation == 0 ? 'A' : 'B', 43) &&
                        request.Headers.GetValues("X-Sproyt-Account").SequenceEqual([User]), "Upload did not bind bearer/account to the current owner.");
                    UploadRequests++; UploadGenerations.Add(_generation); LastRequest = request; Entered.TrySetResult();
                    if (Hold) await Release.Task.WaitAsync(cancellationToken);
                    payload = new { enabled = true, max_file_bytes = 1024 * 1024 * 1024, chunk_bytes = NativeUploadHttpClient.ChunkBytes }; break;
                default: throw new InvalidOperationException("Unexpected session fixture route.");
            }
            var response = Json(payload); response.Headers.Add("X-Sproyt-Account", User); return response;
        }
    }
}
