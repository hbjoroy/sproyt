using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

internal sealed record NativeChannel(string Id, string Name, string? CircleName = null, string? CircleId = null, string Kind = "private", bool IsDirect = false);
internal sealed record NativeMessage(string Id, string ChannelId, string AuthorId, string Author, string Body, DateTimeOffset SentAt, ulong Sequence);
internal sealed record NativeChatSnapshot(bool Connected, bool Loading, string? Error, IReadOnlyList<NativeChannel> Channels, string? SelectedChannel, IReadOnlyList<NativeMessage> Messages, string? UserId = null);

// A transport projection of server events. Drafts and durable outbox work belong
// to the application; this client never manufactures an accepted chat message.
internal sealed class NativeChatClient : IAsyncDisposable
{
    private const string Protocol = "sproyt.chat.v1";
    private readonly object gate = new();
    private readonly NativeAuthSession owner;
    private readonly CancellationTokenSource lifetime = new();
    private readonly SemaphoreSlim sendGate = new(1, 1);
    private readonly SemaphoreSlim selectionGate = new(1, 1);
    private readonly SemaphoreSlim maintenance = new(0, 1);
    private readonly Dictionary<string, Pending> pending = new(StringComparer.Ordinal);
    private readonly Dictionary<string, NativeMessage> messages = new(StringComparer.Ordinal);
    private readonly Dictionary<string, string> circles = new(StringComparer.Ordinal);
    private NativeChannel[] channels = [];
    private string? selected;
    private bool connected;
    private bool loading;
    private bool disposed;
    private bool reloadChannels;
    private bool reloadHistory;
    private string? error;
    private string? connectedUser;
    private string? connectedStamp;
    private string? projectionUser;
    private ulong connectedGeneration;
    private ClientWebSocket? socket;
    private Task? runner;
    private readonly TaskCompletionSource firstConnection = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal event Action? Changed;
    private sealed class Pending(string wire)
    {
        internal string Wire { get; } = wire;
        internal int Waiters { get; set; }
        internal TaskCompletionSource<JsonElement> Completion { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    }
    internal NativeChatClient(NativeAuthSession owner) { this.owner = owner; owner.Changed += OwnerChanged; }
    internal NativeChatSnapshot Snapshot
    {
        get
        {
            lock (gate) return new(connected, loading, error, Array.AsReadOnly(channels.ToArray()), selected,
                Array.AsReadOnly(messages.Values.Where(message => message.ChannelId == selected).OrderBy(message => message.Sequence).ThenBy(message => message.Id, StringComparer.Ordinal).ToArray()), projectionUser);
        }
    }
    private void Notify() { try { Changed?.Invoke(); } catch { } }
    private void OwnerChanged()
    {
        var auth = owner.Snapshot;
        lock (gate)
        {
            if (disposed) return;
            if (socket is not null && (auth.Phase != NativeAuthPhase.Active || auth.UserId != connectedUser || auth.SessionStamp != connectedStamp)) socket.Abort();
            var before = Snapshot; var fenced = FenceAccountProjection(before, auth);
            if (!ReferenceEquals(before, fenced))
            { channels = []; messages.Clear(); circles.Clear(); selected = null; connected = false; loading = false; error = null; projectionUser = fenced.UserId; }
        }
        Notify();
    }
    internal async Task StartAsync(CancellationToken cancellationToken = default)
    {
        lock (gate) { NativeClientWire.Require(!disposed); runner ??= Task.Run(RunAsync); }
        try { await firstConnection.Task.WaitAsync(TimeSpan.FromSeconds(25), cancellationToken); }
        catch (OperationCanceledException) { throw; }
        catch { throw new InvalidOperationException(Snapshot.Error ?? "Vi fekk ikkje kontakt med samtaletenesta."); }
    }
    private async Task RunAsync()
    {
        int backoff = 1;
        while (!lifetime.IsCancellationRequested)
        {
            ClientWebSocket? active = null; Task? receiver = null;
            try
            {
                var auth = owner.Snapshot;
                if (auth.Phase is not (NativeAuthPhase.Active or NativeAuthPhase.Offline)) { await Task.Delay(500, lifetime.Token); continue; }
                await owner.ResumeAsync().WaitAsync(lifetime.Token);
                if (owner.Snapshot.Phase != NativeAuthPhase.Active) throw NativeClientWire.Failure();
                var connection = await owner.ConnectBoundWebSocketAsync(lifetime.Token); active = connection.Socket;
                owner.AdmitTransport(connection.UserId, connection.SessionStamp, () =>
                {
                    lock (gate) { socket = active; connectedUser = connection.UserId; connectedStamp = connection.SessionStamp; projectionUser = connection.UserId; connectedGeneration = connection.Generation; connected = true; loading = true; error = null; }
                    return true;
                });
                Notify(); receiver = ReceiveAsync(active, lifetime.Token);
                await CommandAsync("hello", null, null, lifetime.Token);
                await CommandAsync("list_my_circles", null, null, lifetime.Token);
                await CommandAsync("list_my_channels", null, null, lifetime.Token);
                string? target;
                lock (gate) target = selected is not null && channels.Any(channel => channel.Id == selected) ? selected : channels.FirstOrDefault()?.Id;
                if (target is not null) await SelectChannelAsync(target, lifetime.Token);
                lock (gate) loading = false;
                Notify(); firstConnection.TrySetResult(); backoff = 1;
                while (!receiver.IsCompleted)
                {
                    using var tick = CancellationTokenSource.CreateLinkedTokenSource(lifetime.Token);
                    var wake = maintenance.WaitAsync(TimeSpan.FromSeconds(20), tick.Token);
                    if (await Task.WhenAny(receiver, wake) == receiver) { tick.Cancel(); break; }
                    bool requested = await wake; bool updateChannels; bool updateHistory; string? historyChannel;
                    lock (gate) { updateChannels = reloadChannels; updateHistory = reloadHistory; reloadChannels = false; reloadHistory = false; historyChannel = selected; }
                    if (updateChannels)
                    {
                        await CommandAsync("list_my_circles", null, null, lifetime.Token);
                        await CommandAsync("list_my_channels", null, null, lifetime.Token);
                    }
                    if (updateHistory && historyChannel is not null)
                        await CommandAsync("load_recent_messages", new { channel_id = historyChannel, limit = 100, after = (ulong?)null, before = (ulong?)null }, null, lifetime.Token);
                    if (!requested) { await owner.ResumeAsync().WaitAsync(lifetime.Token); await CommandAsync("ping", null, null, lifetime.Token); }
                }
                await receiver;
                throw NativeClientWire.Failure();
            }
            catch (OperationCanceledException) when (lifetime.IsCancellationRequested) { break; }
            catch
            {
                var phase = owner.Snapshot.Phase;
                lock (gate) { if (phase is NativeAuthPhase.Active or NativeAuthPhase.Offline) error ??= "Sambandet vart brote. Vi prøver å kople til att."; }
            }
            finally
            {
                active?.Abort(); active?.Dispose();
                if (receiver is not null) { try { await receiver; } catch { } }
                lock (gate)
                {
                    if (ReferenceEquals(socket, active)) socket = null;
                    connected = false; loading = false;
                    foreach (var request in pending.Values) request.Completion.TrySetException(new InvalidOperationException("Vi fekk ikkje stadfesta førespurnaden. Prøv igjen med same førespurnads-ID."));
                    pending.Clear();
                }
                Notify();
            }
            try { await Task.Delay(TimeSpan.FromSeconds(backoff), lifetime.Token); } catch (OperationCanceledException) { break; }
            backoff = Math.Min(15, backoff * 2);
        }
    }
    internal async Task SelectChannelAsync(string channelId, CancellationToken cancellationToken = default)
    {
        NativeClientWire.Require(Guid.TryParseExact(channelId, "D", out _));
        await selectionGate.WaitAsync(cancellationToken);
        try
        {
            string? previous;
            lock (gate) { NativeClientWire.Require(!disposed && connected && channels.Any(channel => channel.Id == channelId)); previous = selected; selected = channelId; if (previous != channelId) messages.Clear(); loading = true; error = null; }
            Notify();
            if (previous is not null && previous != channelId) await CommandAsync("unsubscribe_channel", new { channel_id = previous }, null, cancellationToken);
            await CommandAsync("subscribe_channel", new { channel_id = channelId }, null, cancellationToken);
            await CommandAsync("load_recent_messages", new { channel_id = channelId, limit = 100, after = (ulong?)null, before = (ulong?)null }, null, cancellationToken);
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception failure) { SetError(SafeError(failure.Message)); throw new InvalidOperationException(Snapshot.Error); }
        finally { lock (gate) loading = false; selectionGate.Release(); Notify(); }
    }
    internal async Task<NativeMessage> SendAsync(string channelId, string body, string requestId, string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        NativeClientWire.Require(Guid.TryParseExact(channelId, "D", out _) && !string.IsNullOrWhiteSpace(body) && Encoding.UTF8.GetByteCount(body) <= 1024 && RequestId(requestId));
        NativeClientWire.Require(Guid.TryParseExact(expectedUserId, "D", out _) && !string.IsNullOrEmpty(expectedSessionStamp));
        var response = await CommandAsync("send_message", new { channel_id = channelId, parent_message_id = (string?)null, body }, requestId, cancellationToken, new SendFence(expectedUserId, expectedSessionStamp));
        NativeClientWire.Require(response.GetProperty("type").GetString() == "message_accepted");
        var message = ParseMessage(response.GetProperty("payload").GetProperty("message"));
        NativeClientWire.Require(message.ChannelId == channelId); return message;
    }
    private sealed record SendFence(string UserId, string SessionStamp);
    private async Task<JsonElement> CommandAsync(string type, object? payload, string? requestId, CancellationToken cancellationToken, SendFence? fence = null)
    {
        requestId ??= Guid.NewGuid().ToString("N");
        string wire = JsonSerializer.Serialize(new { protocol = Protocol, request_id = requestId, type, payload });
        (Pending Request, ClientWebSocket Socket, bool Transmit) Capture()
        {
            lock (gate)
            {
                NativeClientWire.Require(!disposed && connected && socket is { State: WebSocketState.Open });
                if (fence is not null) AdmitSocket(fence.UserId, fence.SessionStamp, connectedUser, connectedStamp, socket, socket, () => true);
                bool transmit = false; Pending request;
                if (pending.TryGetValue(requestId, out var existing)) { NativeClientWire.Require(existing.Wire == wire); request = existing; }
                else { NativeClientWire.Require(pending.Count < 64); request = new Pending(wire); pending.Add(requestId, request); transmit = true; }
                request.Waiters++; return (request, socket!, transmit);
            }
        }
        var captured = fence is null ? Capture() : owner.AdmitTransport(fence.UserId, fence.SessionStamp, Capture);
        var request = captured.Request; var active = captured.Socket;
        using var timeout = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, lifetime.Token); timeout.CancelAfter(TimeSpan.FromSeconds(20));
        try
        {
            if (captured.Transmit)
            {
                await sendGate.WaitAsync(timeout.Token);
                try
                {
                    Task StartWrite()
                    {
                        lock (gate)
                        {
                            NativeClientWire.Require(!disposed && connected && ReferenceEquals(socket, active));
                            return fence is null
                                ? active.SendAsync(new ArraySegment<byte>(Encoding.UTF8.GetBytes(wire)), WebSocketMessageType.Text, true, timeout.Token)
                                : AdmitSocket(fence.UserId, fence.SessionStamp, connectedUser, connectedStamp, socket, active,
                                    () => active.SendAsync(new ArraySegment<byte>(Encoding.UTF8.GetBytes(wire)), WebSocketMessageType.Text, true, timeout.Token));
                        }
                    }
                    // The owner gate spans only admission and starting the captured
                    // socket write. Network completion is awaited without either gate.
                    var writing = fence is null ? StartWrite() : owner.AdmitTransport(fence.UserId, fence.SessionStamp, StartWrite);
                    await writing;
                }
                finally { sendGate.Release(); }
            }
            var response = await request.Completion.Task.WaitAsync(timeout.Token);
            if (response.GetProperty("type").GetString() == "error")
            {
                string message = SafeError(response.GetProperty("payload").GetProperty("message").GetString() ?? "Tenesta avviste førespurnaden.");
                SetError(message); throw new InvalidOperationException(message);
            }
            return response;
        }
        catch (OperationCanceledException) when (!cancellationToken.IsCancellationRequested && !lifetime.IsCancellationRequested)
        { SetError("Vi fekk ikkje stadfesta førespurnaden. Prøv igjen med same førespurnads-ID."); throw new InvalidOperationException(Snapshot.Error); }
        finally { lock (gate) if (--request.Waiters == 0 && pending.TryGetValue(requestId, out var current) && ReferenceEquals(current, request)) pending.Remove(requestId); }
    }
    private async Task ReceiveAsync(ClientWebSocket active, CancellationToken cancellationToken)
    {
        var buffer = new byte[8192];
        while (active.State == WebSocketState.Open && !cancellationToken.IsCancellationRequested)
        {
            using var bytes = new MemoryStream(); WebSocketReceiveResult result;
            do
            {
                result = await active.ReceiveAsync(buffer, cancellationToken);
                if (result.MessageType == WebSocketMessageType.Close) return;
                NativeClientWire.Require(result.MessageType == WebSocketMessageType.Text && bytes.Length + result.Count <= 1024 * 1024);
                bytes.Write(buffer, 0, result.Count);
            } while (!result.EndOfMessage);
            using var document = JsonDocument.Parse(bytes.ToArray()); var envelope = document.RootElement;
            NativeClientWire.Require(envelope.GetProperty("protocol").GetString() == Protocol);
            if (!Apply(envelope)) return;
            if (envelope.TryGetProperty("request_id", out var requestId) && requestId.ValueKind == JsonValueKind.String)
            {
                lock (gate) if (pending.TryGetValue(requestId.GetString()!, out var request)) request.Completion.TrySetResult(envelope.Clone());
            }
        }
    }
    private bool Apply(JsonElement envelope)
    {
        string type = envelope.GetProperty("type").GetString()!;
        JsonElement payload = envelope.TryGetProperty("payload", out var value) ? value : default;
        var auth = owner.Snapshot;
        bool wake = false;
        lock (gate)
        {
            if (disposed || !connected || auth.Phase != NativeAuthPhase.Active || auth.UserId != connectedUser || auth.Generation != connectedGeneration || auth.SessionStamp != connectedStamp) return false;
            if (type == "hello") NativeClientWire.Require(payload.GetProperty("participant_id").GetString() == connectedUser);
            else if (type == "circles_listed")
            {
                circles.Clear(); foreach (var tuple in payload.GetProperty("circles").EnumerateArray()) { var circle = tuple[0]; circles[circle.GetProperty("id").GetString()!] = circle.GetProperty("name").GetString()!; }
                channels = channels.Select(channel => channel with { CircleName = channel.CircleId is not null && circles.TryGetValue(channel.CircleId, out var name) ? name : null }).ToArray();
            }
            else if (type == "channels_listed")
            {
                var listed = payload.GetProperty("channels"); NativeClientWire.Require(listed.GetArrayLength() <= 2000);
                channels = listed.EnumerateArray().Select(channel =>
                {
                    string? circle = channel.TryGetProperty("circle_id", out var id) && id.ValueKind == JsonValueKind.String ? id.GetString() : null;
                    bool direct = channel.TryGetProperty("is_direct", out var directValue) && directValue.ValueKind == JsonValueKind.True;
                    direct |= channel.TryGetProperty("direct_user_id", out var directUser) && directUser.ValueKind == JsonValueKind.String;
                    return new NativeChannel(channel.GetProperty("id").GetString()!, channel.GetProperty("name").GetString()!, circle is not null && circles.TryGetValue(circle, out var name) ? name : null, circle, channel.GetProperty("kind").GetString()!, direct);
                }).ToArray();
                if (selected is not null && !channels.Any(channel => channel.Id == selected)) { selected = null; messages.Clear(); }
            }
            else if (type is "messages_loaded" or "subscription_started")
            {
                if (payload.GetProperty("channel_id").GetString() == selected)
                    foreach (var message in DecodeMessageEvents(envelope)) Merge(message);
            }
            else if (type is "message_accepted" or "message_edited" or "message_deleted" or "chat") foreach (var message in DecodeMessageEvents(envelope)) Merge(message);
            else if (type == "error") error = SafeError(payload.GetProperty("message").GetString() ?? "Tenesta avviste førespurnaden.");
            if (type is "circles_changed" or "circle_deleted" or "circle_left" or "membership_left") { reloadChannels = true; wake = true; }
            if (type == "lagged" && payload.GetProperty("channel_id").GetString() == selected) { reloadHistory = true; wake = true; }
            if (type == "chat" && payload.GetProperty("event").GetProperty("type").GetString() is "channel_created" or "participant_joined" or "participant_left") { reloadChannels = true; wake = true; }
        }
        if (wake) { try { maintenance.Release(); } catch (SemaphoreFullException) { } }
        Notify(); return true;
    }
    private void Merge(NativeMessage message)
    {
        if (message.ChannelId != selected) return; messages[message.Id] = message;
        if (messages.Count > 500) foreach (string id in messages.Values.OrderByDescending(value => value.Sequence).ThenByDescending(value => value.Id, StringComparer.Ordinal).Skip(500).Select(value => value.Id).ToArray()) messages.Remove(id);
    }
    private static NativeMessage ParseMessage(JsonElement value) => new(value.GetProperty("id").GetString()!, value.GetProperty("channel_id").GetString()!, value.GetProperty("sender_id").GetString()!, value.GetProperty("sender_display_name").GetString()!, value.GetProperty("body").GetString()!, value.GetProperty("sent_at").GetDateTimeOffset(), value.GetProperty("sequence").GetUInt64());
    internal static IReadOnlyList<NativeMessage> DecodeMessageEvents(JsonElement envelope)
    {
        NativeClientWire.Require(envelope.GetProperty("protocol").GetString() == Protocol);
        string? type = envelope.GetProperty("type").GetString();
        var payload = envelope.TryGetProperty("payload", out var value) ? value : default;
        if (type is "messages_loaded" or "subscription_started")
        {
            var source = payload.GetProperty(type == "messages_loaded" ? "messages" : "history"); NativeClientWire.Require(source.GetArrayLength() <= 1000);
            return Array.AsReadOnly(source.EnumerateArray().Select(ParseMessage).ToArray());
        }
        if (type == "chat") { payload = payload.GetProperty("event"); type = payload.GetProperty("type").GetString(); }
        return type is "message_accepted" or "message_edited" or "message_deleted" ? Array.AsReadOnly(new[] { ParseMessage(payload.GetProperty("message")) }) : Array.Empty<NativeMessage>();
    }
    internal static NativeChatSnapshot FenceAccountProjection(NativeChatSnapshot current, NativeAuthSnapshot auth) =>
        current.UserId != auth.UserId || auth.Phase is NativeAuthPhase.SignedOut or NativeAuthPhase.SigningIn or NativeAuthPhase.AuthRequired
            ? new(false, false, null, Array.Empty<NativeChannel>(), null, Array.Empty<NativeMessage>(), auth.UserId)
            : current;
    internal static T AdmitSocket<T>(string expectedUserId, string expectedSessionStamp, string? currentUserId, string? currentSessionStamp, object? currentSocket, object? capturedSocket, Func<T> start)
    {
        NativeClientWire.Require(expectedUserId == currentUserId && expectedSessionStamp == currentSessionStamp && currentSocket is not null && ReferenceEquals(currentSocket, capturedSocket));
        return start();
    }
    private static bool RequestId(string value) => value is { Length: >= 1 and <= 128 } && value.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_');
    private static string SafeError(string value) => new(value.Where(c => !char.IsControl(c) || c == '\n').Take(512).ToArray());
    private void SetError(string value) { lock (gate) error = value; Notify(); }
    public async ValueTask DisposeAsync()
    {
        Task? active;
        lock (gate) { if (disposed) return; disposed = true; owner.Changed -= OwnerChanged; lifetime.Cancel(); firstConnection.TrySetCanceled(lifetime.Token); socket?.Abort(); active = runner; }
        if (active is not null) { try { await active; } catch { } }
        lifetime.Dispose();
    }
}
