using System.Collections.Concurrent;
using System.Net;
using System.Net.WebSockets;
using System.Text.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Hosting.Server;
using Microsoft.AspNetCore.Hosting.Server.Features;
using Microsoft.AspNetCore.Http;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;

internal sealed class FixtureServer : IAsyncDisposable
{
    internal const string User = "11111111-1111-4111-8111-111111111111", Peer = "22222222-2222-4222-8222-222222222222";
    internal const string Channel = "33333333-3333-4333-8333-333333333333", Direct = "44444444-4444-4444-8444-444444444444";
    internal const string Message = "55555555-5555-4555-8555-555555555555", Root = "66666666-6666-4666-8666-666666666666";
    internal const string TaskId = "77777777-7777-4777-8777-777777777777";
    private readonly WebApplication _app;
    private bool _direct, _read, _done;
    private string _name = "Owner", _status = "", _emoji = "";
    internal Uri SocketUri { get; }
    internal ConcurrentQueue<(string Type, string Id, JsonElement Payload)> Received { get; } = new();
    internal Func<string, (string Type, object Payload)?>? Override;
    internal bool SendDecoy;
    internal string? Hold;
    internal TaskCompletionSource Held { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

    private FixtureServer(WebApplication app, Uri uri) { _app = app; SocketUri = uri; }
    internal static async Task<FixtureServer> OpenAsync()
    {
        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(o => o.Listen(IPAddress.Loopback, 0));
        var app = builder.Build(); app.UseWebSockets();
        FixtureServer? server = null;
        app.Map("/wire", async context =>
        {
            using var socket = await context.WebSockets.AcceptWebSocketAsync();
            try { await server!.ServeAsync(socket, context.RequestAborted); }
            catch (Exception e) when (e is WebSocketException or OperationCanceledException or IOException) { }
        });
        await app.StartAsync();
        string address = app.Services.GetRequiredService<IServer>().Features.Get<IServerAddressesFeature>()!.Addresses.Single();
        server = new(app, new UriBuilder(address) { Scheme = "ws", Path = "wire" }.Uri);
        return server;
    }

    private async Task ServeAsync(WebSocket socket, CancellationToken token)
    {
        byte[] buffer = new byte[4096];
        while (socket.State == WebSocketState.Open)
        {
            using var input = new MemoryStream();
            WebSocketReceiveResult result;
            do
            {
                result = await socket.ReceiveAsync(buffer, token);
                if (result.MessageType == WebSocketMessageType.Close) return;
                input.Write(buffer, 0, result.Count);
            } while (!result.EndOfMessage);
            using var doc = JsonDocument.Parse(input.ToArray());
            var envelope = doc.RootElement;
            string id = envelope.GetProperty("request_id").GetString()!, type = envelope.GetProperty("type").GetString()!;
            var payload = envelope.GetProperty("payload");
            Received.Enqueue((type, id, payload.Clone()));
            if (type == Hold) { Held.TrySetResult(); await Release.Task.WaitAsync(token); }
            if (SendDecoy)
            {
                SendDecoy = false;
                await ReplyAsync(socket, null, "channels_listed", new { channels = Array.Empty<object>() }, token);
                await ReplyAsync(socket, null, "mention_read", new { message_id = Root }, token);
                await ReplyAsync(socket, "unrelated", "error", new { code = "permission_denied", message = "Unrelated request" }, token);
            }
            var response = Override?.Invoke(type) ?? Reply(type, payload);
            await ReplyAsync(socket, id, response.Type, response.Payload, token);
        }
    }

    private static Task ReplyAsync(WebSocket socket, string? id, string type, object payload, CancellationToken token) =>
        socket.SendAsync(new ArraySegment<byte>(JsonSerializer.SerializeToUtf8Bytes(new { protocol = "sproyt.chat.v1", request_id = id, type, payload })),
            WebSocketMessageType.Text, true, token);

    internal object Person(string id) => new { id, kind = "human", display_name = id == User ? _name : "Peer", handle = id == User ? "owner" : "peer",
        external_provider = "PRIVATE-PROVIDER", external_subject = "PRIVATE-SUBJECT", status_text = _status, status_emoji = _emoji, status_expires_at = (string?)null };
    internal object TaskRow(string assignee, string taskId = TaskId) => new { id = taskId, source_message_id = Message, channel_id = Channel,
        channel_name = "Prat", assignee_id = assignee, title = "Task", status = _done ? "done" : "open", completed_at = _done ? "2026-11-04T17:00:00Z" : null };
    private (string Type, object Payload) Reply(string type, JsonElement payload)
    {
        switch (type)
        {
            case "hello": return ("hello", new { participant_id = User });
            case "list_my_circles": return ("circles_listed", new { circles = Array.Empty<object>() });
            case "list_my_channels":
                var channels = new List<object> { new { id = Channel, name = "Prat", kind = "public" } };
                if (_direct) channels.Add(new { id = Direct, name = "Peer", kind = "private", is_direct = true, direct_user_id = Peer, role = "member" });
                return ("channels_listed", new { channels });
            case "subscribe_channel": return ("subscription_started", new { channel_id = Channel, history = Array.Empty<object>() });
            case "load_recent_messages": return ("messages_loaded", new { channel_id = Channel, messages = Array.Empty<object>() });
            case "list_channel_reactions": return ("channel_reactions_listed", new { channel_id = Channel, reactions = Array.Empty<object>() });
            case "ping": return ("pong", new { });
            case "list_users": return ("users_listed", new { users = new[] { Person(User), Person(Peer) } });
            case "open_direct_channel": _direct = true; return ("direct_channel_opened", new { channel = new { id = Direct, name = "Peer", kind = "private" } });
            case "list_mentions": return ("mentions_listed", new { mentions = new[] { new { channel_name = "Prat", read = _read, message = new {
                id = Message, channel_id = Channel, parent_message_id = Root, sender_id = Peer, sender_display_name = "Peer", body = "@owner", sequence = 3UL, sent_at = "2026-11-04T12:00:00Z" } } } });
            case "mark_mention_read": _read = true; return ("mention_read", new { message_id = payload.GetProperty("message_id").GetString()! });
            case "list_tasks": return ("tasks_listed", new { tasks = new[] { TaskRow(User) } });
            case "set_task_done": _done = payload.GetProperty("done").GetBoolean(); return ("task_updated", new { task = TaskRow(User) });
            case "update_profile": _name = payload.GetProperty("display_name").GetString()!; return ("profile_updated", new { profile = Person(User) });
            case "set_status":
                _status = payload.GetProperty("text").GetString()!; _emoji = payload.GetProperty("emoji").GetString()!;
                return ("status_updated", new { profile = Person(User) });
            default: return ("error", new { code = "validation_error", message = "Unknown fixture command" });
        }
    }
    public async ValueTask DisposeAsync() { Release.TrySetResult(); await _app.StopAsync(); await _app.DisposeAsync(); }
}
