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

internal sealed class HistoryServer : IAsyncDisposable
{
    internal const string A = FixtureServer.Channel;
    internal const string B = "d0fa6389-d652-4058-9535-e06a05d2d97d";
    internal const string User = FixtureServer.User;
    private readonly WebApplication app;
    private readonly SemaphoreSlim writes = new(1, 1);
    private readonly ConcurrentBag<Task> replies = [];
    private WebSocket? current;
    internal Uri SocketUri { get; }
    internal ConcurrentQueue<(string Channel, ulong? Before)> Pages { get; } = new();
    internal ConcurrentQueue<ulong> Reads { get; } = new();
    internal ulong Total { get; set; } = 155;
    internal string SessionUser { get; set; } = User;
    internal Func<ulong, bool> IsReply { get; set; } = n => n is >= 56 and <= 105;
    internal Func<ulong, string> Body { get; set; } = n => $"Message {n}";
    internal bool FailNext { get; set; }
    internal bool DenyNext { get; set; }
    internal bool MalformedNext { get; set; }
    private bool holdNextPage;
    internal bool HoldNextPage { get => holdNextPage; set { holdNextPage = value; if (value) { Held = new(TaskCreationOptions.RunContinuationsAsynchronously); Release = new(TaskCreationOptions.RunContinuationsAsynchronously); } } }
    internal bool HoldNextRead { get; set; }
    internal bool DenyNextRead { get; set; }
    internal bool HoldNextHello { get; set; }
    internal TaskCompletionSource HelloHeld { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource HelloRelease { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource ReadHeld { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource ReadRelease { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource Held { get; private set; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal TaskCompletionSource Release { get; private set; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private HistoryServer(WebApplication app, Uri uri) { this.app = app; SocketUri = uri; }
    internal void AbortSocket() => current?.Abort();
    internal static string Id(ulong sequence, string channel = A) => $"{sequence:x8}-0000-4000-8000-{(channel == A ? "000000000001" : "000000000002")}";
    private object Message(ulong n, string channel) => new { id = Id(n, channel), channel_id = channel, sender_id = User,
        sender_display_name = "History fixture", body = Body(n), sent_at = "2026-11-03T12:00:00Z", sequence = n,
        parent_message_id = IsReply(n) ? Id(1, channel) : null, edited_at = (string?)null, deleted_at = (string?)null };
    internal static async Task<HistoryServer> StartAsync()
    {
        var builder = WebApplication.CreateSlimBuilder(); builder.Logging.ClearProviders();
        builder.WebHost.ConfigureKestrel(options => options.Listen(IPAddress.Loopback, 0));
        var app = builder.Build(); app.UseWebSockets(); HistoryServer? fixture = null;
        app.Map("/history", async context =>
        {
            using var socket = await context.WebSockets.AcceptWebSocketAsync(); fixture!.current = socket;
            try { await fixture.Serve(socket, context.RequestAborted); }
            catch (Exception ex) when (ex is WebSocketException or IOException or OperationCanceledException) { }
        });
        await app.StartAsync();
        string address = app.Services.GetRequiredService<IServer>().Features.Get<IServerAddressesFeature>()!.Addresses.Single();
        return fixture = new(app, new UriBuilder(address) { Scheme = "ws", Path = "/history" }.Uri);
    }
    private async Task Send(WebSocket socket, string type, object payload, string? id = null)
    {
        byte[] bytes = JsonSerializer.SerializeToUtf8Bytes(new { protocol = "sproyt.chat.v1", request_id = id, type, payload });
        await writes.WaitAsync();
        try { if (socket.State == WebSocketState.Open) await socket.SendAsync(bytes, WebSocketMessageType.Text, true, CancellationToken.None); }
        finally { writes.Release(); }
    }
    internal Task ReadEvent(ulong sequence, string? user = null) => Send(current!, "chat", new { @event = new { type = "read_marker_updated", channel_id = A, user_id = user ?? User, sequence } });
    internal Task EditEvent(ulong sequence, bool deleted) => Send(current!, "chat", new { @event = new { type = deleted ? "message_deleted" : "message_edited", message = new
    {
        id = Id(sequence), channel_id = A, sender_id = User, sender_display_name = "History fixture", body = deleted ? "" : "Edited body",
        sent_at = "2026-11-03T12:00:00Z", sequence, parent_message_id = (string?)null,
        edited_at = deleted ? (string?)null : "2026-11-04T12:00:00Z", deleted_at = deleted ? "2026-11-04T13:00:00Z" : null,
    } } });
    private async Task Serve(WebSocket socket, CancellationToken cancellationToken)
    {
        while (socket.State == WebSocketState.Open)
        {
            using var input = new MemoryStream(); var buffer = new byte[4096]; WebSocketReceiveResult part;
            do { part = await socket.ReceiveAsync(buffer, cancellationToken); if (part.MessageType == WebSocketMessageType.Close) return; input.Write(buffer, 0, part.Count); } while (!part.EndOfMessage);
            using var json = JsonDocument.Parse(input.ToArray()); var request = json.RootElement;
            string id = request.GetProperty("request_id").GetString()!, type = request.GetProperty("type").GetString()!;
            var p = request.GetProperty("payload");
            switch (type)
            {
                case "hello":
                    if (HoldNextHello) { HoldNextHello = false; HelloHeld.SetResult(); await HelloRelease.Task; }
                    await Send(socket, "hello", new { participant_id = SessionUser }, id); break;
                case "list_my_circles": await Send(socket, "circles_listed", new { circles = Array.Empty<object>() }, id); break;
                case "list_my_channels": await Send(socket, "channels_listed", new { channels = new[] { A, B }.Select(channel => new { id = channel, name = channel, kind = "private", last_read_sequence = 0UL, latest_sequence = Total }).ToArray() }, id); break;
                case "subscribe_channel": await Send(socket, "subscription_started", new { channel_id = p.GetProperty("channel_id").GetString(), history = Array.Empty<object>() }, id); break;
                case "unsubscribe_channel": await Send(socket, "subscription_ended", new { channel_id = p.GetProperty("channel_id").GetString() }, id); break;
                case "ping": await Send(socket, "pong", new { }, id); break;
                case "load_recent_messages":
                    string channel = p.GetProperty("channel_id").GetString()!;
                    ulong? before = p.GetProperty("before").ValueKind == JsonValueKind.Null ? null : p.GetProperty("before").GetUInt64();
                    int limit = p.GetProperty("limit").GetInt32(); Pages.Enqueue((channel, before));
                    if (FailNext || DenyNext) { string code = DenyNext ? "permission_denied" : "storage_error"; FailNext = DenyNext = false; await Send(socket, "error", new { code, message = "Synthetic history failure" }, id); break; }
                    ulong end = Math.Min(Total, before.HasValue ? before.Value - 1 : Total);
                    ulong start = end > (ulong)limit ? end - (ulong)limit + 1 : 1;
                    object[] rows = end == 0 ? [] : Enumerable.Range(0, checked((int)(end - start + 1))).Select(i => Message(start + (ulong)i, channel)).ToArray();
                    if (MalformedNext) { MalformedNext = false; rows = rows.Reverse().ToArray(); }
                    if (HoldNextPage)
                    {
                        HoldNextPage = false;
                        var held = Held; var release = Release; held.SetResult();
                        replies.Add(Task.Run(async () => { await release.Task; await Send(socket, "messages_loaded", new { channel_id = channel, messages = rows }, id); }));
                    }
                    else await Send(socket, "messages_loaded", new { channel_id = channel, messages = rows }, id);
                    break;
                case "mark_read":
                    ulong sequence = p.GetProperty("sequence").GetUInt64(); Reads.Enqueue(sequence);
                    string readType = DenyNextRead ? "error" : "read_marker_updated";
                    object read = DenyNextRead ? new { code = "permission_denied", message = "Synthetic read access revoked" } :
                        new { membership = new { channel_id = p.GetProperty("channel_id").GetString(), user_id = User, role = "member", last_read_sequence = sequence } };
                    DenyNextRead = false;
                    if (HoldNextRead) { HoldNextRead = false; ReadHeld.SetResult(); replies.Add(Task.Run(async () => { await ReadRelease.Task; await Send(socket, readType, read, id); })); }
                    else await Send(socket, readType, read, id);
                    break;
                default: throw new InvalidOperationException("Unexpected history command: " + type);
            }
        }
    }
    public async ValueTask DisposeAsync() { Release.TrySetResult(); ReadRelease.TrySetResult(); HelloRelease.TrySetResult(); current?.Abort(); await Task.WhenAll(replies); await app.StopAsync(); await app.DisposeAsync(); }
}
