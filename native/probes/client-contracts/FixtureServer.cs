using System.Collections.Concurrent;
using System.Net;
using System.Net.WebSockets;
using System.Text;
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
    internal const string User = "c49b194b-5cf8-41b5-82b0-02eb254945ec";
    internal const string Channel = "c8940bd1-9acf-4108-8e8b-499b02a57482";
    private readonly WebApplication app;
    internal Uri SocketUri { get; }
    internal ConcurrentDictionary<string, string> Failures { get; } = new();
    internal ConcurrentDictionary<string, List<string>> Frames { get; } = new();
    internal ConcurrentDictionary<string, object> Accepted { get; } = new();
    internal string? DropFirst { get; set; }
    internal string? Hold { get; set; }
    internal TaskCompletionSource Held { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource Release { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);

    private FixtureServer(WebApplication app, Uri socketUri) { this.app = app; SocketUri = socketUri; }
    internal static async Task<FixtureServer> StartAsync()
    {
        var builder = WebApplication.CreateSlimBuilder();
        builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(options => options.Listen(IPAddress.Loopback, 0));
        var app = builder.Build();
        app.UseWebSockets();
        FixtureServer? fixture = null;
        app.Map("/fixture", async context =>
        {
            if (!context.WebSockets.IsWebSocketRequest) { context.Response.StatusCode = 400; return; }
            using var socket = await context.WebSockets.AcceptWebSocketAsync();
            try { await fixture!.ServeAsync(socket, context.RequestAborted); }
            catch (Exception exception) when (exception is WebSocketException or OperationCanceledException or IOException) { }
        });
        await app.StartAsync();
        var address = app.Services.GetRequiredService<IServer>().Features.Get<IServerAddressesFeature>()!.Addresses.Single();
        fixture = new(app, new UriBuilder(address) { Scheme = "ws", Path = "/fixture" }.Uri);
        return fixture;
    }
    internal static object Message(string body) => new
    {
        id = Guid.NewGuid().ToString(), channel_id = Channel, sender_id = User, sender_display_name = "Synthetic sender",
        body, sent_at = "2026-11-03T12:00:00Z", sequence = 42UL,
    };
    private async Task ServeAsync(WebSocket socket, CancellationToken cancellationToken)
    {
        while (socket.State == WebSocketState.Open)
        {
            using var input = new MemoryStream();
            var buffer = new byte[4096]; WebSocketReceiveResult result;
            do
            {
                result = await socket.ReceiveAsync(buffer, cancellationToken);
                if (result.MessageType == WebSocketMessageType.Close) return;
                input.Write(buffer, 0, result.Count);
            } while (!result.EndOfMessage);
            string wire = Encoding.UTF8.GetString(input.ToArray());
            using var json = JsonDocument.Parse(wire);
            var request = json.RootElement;
            string id = request.GetProperty("request_id").GetString()!;
            string type = request.GetProperty("type").GetString()!;
            object payload;
            string responseType;
            switch (type)
            {
                case "hello": responseType = "hello"; payload = new { participant_id = User }; break;
                case "list_my_circles": responseType = "circles_listed"; payload = new { circles = Array.Empty<object>() }; break;
                case "list_my_channels":
                    responseType = "channels_listed";
                    payload = new { channels = new[] { new { id = Channel, name = "Synthetic channel", kind = "private", slug = "contract-channel", description = "Fixture", role = "moderator", last_read_sequence = 7UL, latest_sequence = 42UL } } };
                    break;
                case "subscribe_channel": responseType = "subscription_started"; payload = new { channel_id = Channel, history = Array.Empty<object>() }; break;
                case "unsubscribe_channel": responseType = "subscription_ended"; payload = new { channel_id = Channel }; break;
                case "load_recent_messages": responseType = "messages_loaded"; payload = new { channel_id = Channel, messages = Array.Empty<object>() }; break;
                case "ping": responseType = "pong"; payload = new { }; break;
                case "send_message":
                    var frames = Frames.GetOrAdd(id, _ => []);
                    int count; lock (frames) { frames.Add(wire); count = frames.Count; }
                    if (id == Hold) { Held.TrySetResult(); await Release.Task.WaitAsync(cancellationToken); }
                    if (Failures.TryGetValue(id, out var code)) { responseType = "error"; payload = new { code, message = "Synthetic rejection\u0000" }; }
                    else
                    {
                        var message = Accepted.GetOrAdd(id, _ => Message(request.GetProperty("payload").GetProperty("body").GetString()!));
                        if (id == DropFirst && count == 1) { socket.Abort(); return; }
                        responseType = "message_accepted"; payload = new { message };
                    }
                    break;
                default: throw new InvalidOperationException("Unexpected fixture command.");
            }
            byte[] output = JsonSerializer.SerializeToUtf8Bytes(new { protocol = "sproyt.chat.v1", request_id = id, type = responseType, payload });
            await socket.SendAsync(output, WebSocketMessageType.Text, true, cancellationToken);
        }
    }
    public async ValueTask DisposeAsync() { Release.TrySetResult(); await app.StopAsync(); await app.DisposeAsync(); }
}
