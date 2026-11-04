using System.Net.WebSockets;
using System.Text;
using System.Text.Json;

internal sealed record NativeDirectActivity(DateTimeOffset? LastMessageAt, double Score, DateTimeOffset CalculatedAt);
internal sealed record NativeChannel(string Id, string Name, string? CircleName = null, string? CircleId = null, string Kind = "private", bool IsDirect = false,
    string? Slug = null, string Description = "", string? Role = null, ulong LastReadSequence = 0, ulong LatestSequence = 0,
    string? DirectUserId = null, NativeDirectActivity? DirectActivity = null);
internal sealed record NativeMessage(string Id, string ChannelId, string AuthorId, string Author, string Body, DateTimeOffset SentAt, ulong Sequence,
    string? ParentMessageId = null, DateTimeOffset? EditedAt = null, DateTimeOffset? DeletedAt = null);
internal sealed record NativeHistorySnapshot(string? ChannelId, long ViewGeneration, bool LoadingOlder, bool HasOlder, bool AtCapacity, string? Error, ulong? OldestSequence, int LoadedCount);
internal sealed record NativeThreadSummary(string RootMessageId, uint ReplyCount, uint UnreadCount, ulong LatestSequence);
internal sealed record NativeMessageReaction(string MessageId, string Emoji, uint Count, bool ReactedByMe, IReadOnlyList<string> UserIds);
internal sealed record NativeReactionChange(string MessageId, string ChannelId, string UserId, string Emoji, bool Added, uint Count);
internal sealed record NativeThreadSnapshot(string ChannelId, string RootMessageId, long ViewGeneration, long Generation,
    NativeMessage? Root, IReadOnlyList<NativeMessage> Replies, bool Loading = false, string? Error = null, bool AtCapacity = false, ulong LastReadSequence = 0);
internal sealed record NativeConversationSnapshot(NativeThreadSnapshot? Thread, IReadOnlyList<NativeThreadSummary> ThreadSummaries, IReadOnlyList<NativeMessageReaction> Reactions,
    bool ReactionsLoaded = false, string? ReactionError = null, bool ThreadSummariesLoaded = false, string? ThreadSummaryError = null);
internal sealed record NativeChatSnapshot(bool Connected, bool Loading, string? Error, IReadOnlyList<NativeChannel> Channels, string? SelectedChannel, IReadOnlyList<NativeMessage> Messages, string? UserId = null, NativeHistorySnapshot? History = null, NativeConversationSnapshot? Conversation = null);

// A transport projection of server events. Drafts and durable outbox work belong
// to the application; this client never manufactures an accepted chat message.
internal sealed partial class NativeChatClient : IAsyncDisposable
{
    private const string Protocol = "sproyt.chat.v1";
    private readonly object gate = new();
    private readonly NativeAuthSession owner;
    private readonly CancellationTokenSource lifetime = new();
    private readonly SemaphoreSlim sendGate = new(1, 1);
    private readonly SemaphoreSlim selectionGate = new(1, 1);
    private readonly SemaphoreSlim readGate = new(1, 1);
    private readonly SemaphoreSlim reactionGate = new(1, 1);
    private readonly SemaphoreSlim summaryGate = new(1, 1);
    internal const int MaximumThreadReplies = 2000;
    private NativeThreadSnapshot? thread;
    private long threadGeneration, reactionRevision, summaryRevision;
    private NativeThreadSummary[] threadSummaries = [];
    private NativeMessageReaction[] reactions = [];
    private bool reactionsLoaded, summariesLoaded;
    private string? reactionError, summaryError;
    private readonly SemaphoreSlim maintenance = new(0, 1);
    private readonly Dictionary<string, Pending> pending = new(StringComparer.Ordinal);
    private readonly Dictionary<string, NativeMessage> messages = new(StringComparer.Ordinal);
    private readonly Dictionary<string, NativeMessage> messageUpdates = new(StringComparer.Ordinal);
    internal const int HistoryPageSize = 50;
    internal const int MaximumHistoryMessages = 2000;
    private long viewGeneration;
    private bool loadingOlder, hasOlder, atCapacity;
    private ulong? oldestSequence;
    private string? historyError;
    private readonly Dictionary<string, string> circles = new(StringComparer.Ordinal);
    private NativeChannel[] channels = [];
    private string? selected;
    private bool connected;
    private bool loading;
    private bool disposed;
    private bool reloadChannels;
    private bool reloadHistory;
    private bool reloadSummaries;
    private bool reloadReactions;
    private string? error;
    private string? connectedUser;
    private string? connectedStamp;
    private string? projectionUser;
    private ulong connectedGeneration;
    private ClientWebSocket? socket;
    private Task? runner;
    private readonly TaskCompletionSource firstConnection = new(TaskCreationOptions.RunContinuationsAsynchronously);
    internal event Action? Changed;
    private sealed class FrameCapacityException : IOException { internal FrameCapacityException() : base("Svaret er større enn minnegrensa på 20 MiB i denne Windows-versjonen.") { } }
    private sealed class Pending(string wire, SendFence? fence, string type)
    {
        internal string Wire { get; } = wire;
        internal SendFence? Fence { get; } = fence;
        internal string Type { get; } = type;
        internal int Waiters { get; set; }
        internal TaskCompletionSource<JsonElement> Completion { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    }
    internal NativeChatClient(NativeAuthSession owner) { this.owner = owner; owner.Changed += OwnerChanged; }
    internal NativeChatSnapshot Snapshot
    {
        get
        {
            lock (gate) return new(connected, loading, error, Array.AsReadOnly(channels.ToArray()), selected,
                Array.AsReadOnly(messages.Values.Where(message => message.ChannelId == selected).OrderBy(message => message.Sequence).ThenBy(message => message.Id, StringComparer.Ordinal).ToArray()), projectionUser,
                new(selected, viewGeneration, loadingOlder, hasOlder, atCapacity, historyError, oldestSequence, messages.Count),
                new(thread, Array.AsReadOnly(threadSummaries.ToArray()), Array.AsReadOnly(reactions.ToArray()), reactionsLoaded, reactionError, summariesLoaded, summaryError));
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
            { channels = []; ResetHistory(); circles.Clear(); selected = null; connected = false; loading = false; error = null; projectionUser = fenced.UserId; }
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
                    bool requested = await wake; bool updateChannels; bool updateHistory; bool updateSummaries; bool updateReactions; string? historyChannel; long historyGeneration; SendFence repairScope;
                    lock (gate) { updateChannels = reloadChannels; updateHistory = reloadHistory; updateSummaries = reloadSummaries; updateReactions = reloadReactions; reloadChannels = false; reloadHistory = false; reloadSummaries = false; reloadReactions = false; historyChannel = selected; historyGeneration = viewGeneration; repairScope = new(connectedUser!, connectedStamp!); }
                    if (updateChannels)
                    {
                        await CommandAsync("list_my_circles", null, null, lifetime.Token);
                        await CommandAsync("list_my_channels", null, null, lifetime.Token);
                    }
                    if (updateHistory && historyChannel is not null)
                        await SelectChannelCoreAsync(historyChannel, historyGeneration, repairScope, lifetime.Token);
                    if (updateSummaries && !updateHistory && historyChannel is not null)
                    {
                        // A captured background refresh cannot switch views or
                        // disconnect a newer selection when its fence expires.
                        try { await ListThreadSummariesAsync(historyChannel, historyGeneration, repairScope.UserId, repairScope.SessionStamp, lifetime.Token); }
                        catch (OperationCanceledException) when (lifetime.IsCancellationRequested) { throw; }
                        catch { }
                    }
                    if (updateReactions && !updateHistory && historyChannel is not null)
                    {
                        try { await ListChannelReactionsAsync(historyChannel, historyGeneration, repairScope.UserId, repairScope.SessionStamp, lifetime.Token); }
                        catch (OperationCanceledException) when (lifetime.IsCancellationRequested) { throw; }
                        catch { }
                    }
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
                    // A completed old page can still be awaiting its merge
                    // continuation. Invalidate it before any next socket opens.
                    viewGeneration++; loadingOlder = false;
                    ResetConversation();
                    foreach (var request in pending) request.Value.Completion.TrySetException(NativeCommandException.Uncertain(request.Key));
                    pending.Clear();
                }
                Notify();
            }
            try { await Task.Delay(TimeSpan.FromSeconds(backoff), lifetime.Token); } catch (OperationCanceledException) { break; }
            backoff = Math.Min(15, backoff * 2);
        }
    }
    internal Task SelectChannelAsync(string channelId, CancellationToken cancellationToken = default,
        string? expectedUserId = null, string? expectedSessionStamp = null)
    {
        NativeClientWire.Require((expectedUserId is null) == (expectedSessionStamp is null));
        var auth = owner.Snapshot;
        return SelectChannelCoreAsync(channelId, null,
            new(expectedUserId ?? auth.UserId ?? "", expectedSessionStamp ?? auth.SessionStamp ?? ""), cancellationToken);
    }
    internal Task RepairHistoryAsync(string channelId, long expectedViewGeneration, CancellationToken cancellationToken = default)
    {
        var auth = owner.Snapshot;
        return SelectChannelCoreAsync(channelId, expectedViewGeneration, new(auth.UserId ?? "", auth.SessionStamp ?? ""), cancellationToken);
    }
    private async Task SelectChannelCoreAsync(string channelId, long? expectedViewGeneration, SendFence scope, CancellationToken cancellationToken)
    {
        NativeClientWire.Require(Guid.TryParseExact(channelId, "D", out _));
        await selectionGate.WaitAsync(cancellationToken);
        long? operatedGeneration = null;
        try
        {
            string? previous = null; long generation = 0; SendFence? fence = null;
            bool admitted = owner.AdmitTransport(scope.UserId, scope.SessionStamp, () =>
            {
                lock (gate)
                {
                    // Repair is a captured background intent, not a fresh user
                    // selection. Recheck after the semaphore and directory awaits.
                    if (expectedViewGeneration is long expected && !SameView(channelId, expected)) return false;
                    NativeClientWire.Require(!disposed && connected && connectedUser == scope.UserId && connectedStamp == scope.SessionStamp && channels.Any(channel => channel.Id == channelId));
                    previous = selected; selected = channelId; ResetHistory(); generation = viewGeneration; operatedGeneration = generation;
                    fence = new(scope.UserId, scope.SessionStamp, channelId, generation); loading = true; error = null;
                    return true;
                }
            });
            if (!admitted) return;
            Notify();
            if (previous is not null && previous != channelId) await CommandAsync("unsubscribe_channel", new { channel_id = previous }, null, cancellationToken, fence);
            await CommandAsync("subscribe_channel", new { channel_id = channelId }, null, cancellationToken, fence);
            await LoadHistoryPageAsync(channelId, generation, null, fence!, cancellationToken);
        }
        catch (OperationCanceledException) { throw; }
        catch (Exception failure)
        {
            lock (gate) if (operatedGeneration is long generation && SameView(channelId, generation)) error = SafeError(failure.Message);
            throw new InvalidOperationException(SafeError(failure.Message));
        }
        finally { lock (gate) if (viewGeneration == operatedGeneration) loading = false; selectionGate.Release(); if (operatedGeneration.HasValue) Notify(); }
    }
    private void ResetHistory()
    {
        messages.Clear(); messageUpdates.Clear(); viewGeneration++; loadingOlder = false; hasOlder = false; atCapacity = false; oldestSequence = null; historyError = null;
        ResetConversation();
    }
    private void ResetConversation()
    {
        thread = null; threadGeneration++; threadSummaries = []; reactions = [];
        reactionsLoaded = summariesLoaded = false; reactionError = summaryError = null;
        reactionRevision++; summaryRevision++;
        reloadSummaries = reloadReactions = false;
    }
    private bool SameView(string channel, long generation) => !disposed && connected && selected == channel && viewGeneration == generation;
    internal async Task LoadOlderAsync(string channelId, long generation, string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation); ulong? before = null; bool admitted = false;
        owner.AdmitTransport(fence.UserId, fence.SessionStamp, () =>
        {
            lock (gate)
            {
                if (!SameView(channelId, generation) || loading || loadingOlder || !hasOlder || atCapacity) return false;
                before = oldestSequence; loadingOlder = true; historyError = null; admitted = true; return true;
            }
        });
        if (!admitted) return;
        Notify();
        try { await LoadHistoryPageAsync(channelId, generation, before, fence, cancellationToken); }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { throw; }
        catch (Exception failure)
        {
            lock (gate) if (SameView(channelId, generation))
            {
                if (AccessDenied(failure)) ClearDeniedView(channelId, generation);
                else historyError = "Eldre meldingar kunne ikkje hentast. Prøv igjen. " + SafeError(failure.Message);
            }
        }
        finally { lock (gate) if (viewGeneration == generation) loadingOlder = false; Notify(); }
    }
    private static bool AccessDenied(Exception failure) => failure is NativeCommandException rejected && rejected.Code is "permission_denied" or "not_found";
    private bool ClearDeniedView(string channelId, long generation)
    {
        lock (gate)
        {
            if (!SameView(channelId, generation)) return false;
            ResetHistory(); selected = null; channels = channels.Where(c => c.Id != channelId).ToArray();
            error = "Du har ikkje lenger tilgang til denne samtalen."; return true;
        }
    }
    private async Task LoadHistoryPageAsync(string channelId, long generation, ulong? before, SendFence fence, CancellationToken cancellationToken)
    {
        var response = await CommandAsync("load_recent_messages", new { channel_id = channelId, limit = HistoryPageSize, after = (ulong?)null, before }, null, cancellationToken, fence);
        NativeClientWire.Require(response.GetProperty("type").GetString() == "messages_loaded" && response.GetProperty("payload").GetProperty("channel_id").GetString() == channelId);
        var page = DecodeMessageEvents(response);
        NativeClientWire.Require(page.Count <= HistoryPageSize && page.All(m => m.ChannelId == channelId && m.Sequence > 0 && (!before.HasValue || m.Sequence < before.Value)));
        NativeClientWire.Require(page.Select(m => m.Id).Distinct(StringComparer.Ordinal).Count() == page.Count && page.Zip(page.Skip(1), (a, b) => a.Sequence < b.Sequence).All(v => v));
        owner.AdmitTransport(fence.UserId, fence.SessionStamp, () =>
        {
            lock (gate)
            {
                if (!SameView(channelId, generation)) return false;
                int additions = page.Count(m => m.ParentMessageId is null && !messages.ContainsKey(m.Id));
                if (messages.Count + additions > MaximumHistoryMessages) { atCapacity = true; historyError = "Minnegrensa for denne samtalen er nådd. Ny opning hentar dei nyaste meldingane."; return false; }
                foreach (var message in page) Merge(message);
                // Cursor uses all raw rows, including replies, so a reply-only
                // page still makes progress without leaking replies into roots.
                if (page.Count > 0) oldestSequence = !oldestSequence.HasValue ? page[0].Sequence : Math.Min(oldestSequence.Value, page[0].Sequence);
                hasOlder = page.Count == HistoryPageSize && oldestSequence > 1;
                atCapacity = messages.Count >= MaximumHistoryMessages;
                historyError = null; error = null; return true;
            }
        });
        Notify();
    }
    internal async Task MarkVisibleReadAsync(string channelId, long generation, IReadOnlyList<string> visibleMessageIds, string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        // Empty/offscreen reports never turn latest_sequence into read state.
        if (visibleMessageIds.Count == 0) return;
        var ids = visibleMessageIds.Take(MaximumHistoryMessages).ToArray();
        await readGate.WaitAsync(cancellationToken);
        try
        {
            var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
            ulong sequence = owner.AdmitTransport(fence.UserId, fence.SessionStamp, () =>
            {
                lock (gate)
                {
                    if (!SameView(channelId, generation) || loading) return 0UL;
                    ulong visible = ids.Where(messages.ContainsKey).Select(id => messages[id]).Where(m => m.ChannelId == channelId && m.ParentMessageId is null).Select(m => m.Sequence).DefaultIfEmpty().Max();
                    return visible > (channels.FirstOrDefault(c => c.Id == channelId)?.LastReadSequence ?? 0) ? visible : 0;
                }
            });
            if (sequence == 0) return;
            var response = await CommandAsync("mark_read", new { channel_id = channelId, sequence }, null, cancellationToken, fence);
            NativeClientWire.Require(response.GetProperty("type").GetString() == "read_marker_updated");
            var membership = response.GetProperty("payload").GetProperty("membership");
            NativeClientWire.Require(membership.GetProperty("channel_id").GetString() == channelId && membership.GetProperty("user_id").GetString() == expectedUserId);
            // Apply receives correlated ACKs too, but validating here ensures a
            // wrong-resource response cannot look like successful read admission.
        }
        catch (NativeCommandException failure) when (AccessDenied(failure))
        { if (ClearDeniedView(channelId, generation)) Notify(); throw; }
        finally { readGate.Release(); }
    }
    internal async Task<NativeMessage> SendAsync(string channelId, string body, string requestId, string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default, string? parentMessageId = null)
    {
        NativeClientWire.Require(Guid.TryParseExact(channelId, "D", out _) && !string.IsNullOrWhiteSpace(body) && Encoding.UTF8.GetByteCount(body) <= 1024 && RequestId(requestId));
        NativeClientWire.Require(Guid.TryParseExact(expectedUserId, "D", out _) && !string.IsNullOrEmpty(expectedSessionStamp));
        NativeClientWire.Require(parentMessageId is null || Guid.TryParseExact(parentMessageId, "D", out _));
        var response = await CommandAsync("send_message", new { channel_id = channelId, parent_message_id = parentMessageId, body }, requestId, cancellationToken, new SendFence(expectedUserId, expectedSessionStamp));
        try
        {
            NativeClientWire.Require(response.GetProperty("type").GetString() == "message_accepted");
            var message = ParseMessage(response.GetProperty("payload").GetProperty("message"));
            NativeClientWire.Require(message.ChannelId == channelId && message.AuthorId == expectedUserId && message.ParentMessageId == parentMessageId && message.Body == body.Trim());
            owner.AdmitTransport(expectedUserId, expectedSessionStamp, () => { lock (gate) { Merge(message, live: true); return true; } });
            Notify();
            return message;
        }
        catch (Exception exception) when (exception is InvalidOperationException or KeyNotFoundException or FormatException)
        { throw NativeCommandException.Uncertain(requestId); }
    }
    private T InView<T>(SendFence fence, Func<T> action) => owner.AdmitTransport(fence.UserId, fence.SessionStamp, () =>
    {
        lock (gate)
        {
            NativeClientWire.Require(fence.Channel is not null && fence.ViewGeneration is long view && SameView(fence.Channel, view));
            if (fence.ThreadGeneration is long visit) NativeClientWire.Require(thread?.Generation == visit);
            return action();
        }
    });
    internal void CloseThread(string channelId, long generation, string expectedUserId, string expectedSessionStamp)
    {
        InView(new(expectedUserId, expectedSessionStamp, channelId, generation), () => { thread = null; threadGeneration++; return true; });
        Notify();
    }
    internal async Task<NativeThreadSnapshot> LoadThreadAsync(string channelId, string rootMessageId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        NativeClientWire.Require(Guid.TryParseExact(rootMessageId, "D", out _));
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        long visit = InView(fence, () =>
        {
            long next = ++threadGeneration;
            thread = thread is { } known && known.ChannelId == channelId && known.RootMessageId == rootMessageId
                ? known with { Generation = next, Loading = true, Error = null, AtCapacity = false }
                : new(channelId, rootMessageId, generation, next, null, [], Loading: true);
            return next;
        });
        Notify();
        try
        {
            var loaded = await ReadThreadAsync(channelId, rootMessageId, fence with { ThreadGeneration = visit }, cancellationToken);
            return InView(fence, () =>
            {
                NativeClientWire.Require(thread?.Generation == visit && thread.RootMessageId == rootMessageId);
                if (loaded.Count - 1 > MaximumThreadReplies)
                    thread = thread! with { Loading = false, AtCapacity = true, Error = "Tråden er større enn grensa i denne Windows-versjonen." };
                else
                {
                    var existing = thread!;
                    var merged = loaded.ToDictionary(m => m.Id);
                    if (messages.TryGetValue(rootMessageId, out var knownRoot)) merged[rootMessageId] = Newer(merged[rootMessageId], knownRoot);
                    if (existing.Root is { } root) merged[root.Id] = Newer(merged.GetValueOrDefault(root.Id), root);
                    foreach (var reply in existing.Replies) merged[reply.Id] = Newer(merged.GetValueOrDefault(reply.Id), reply);
                    var replies = merged.Values.Where(m => m.ParentMessageId == rootMessageId).OrderBy(m => m.Sequence).ToArray();
                    thread = replies.Length > MaximumThreadReplies || existing.AtCapacity
                        ? existing with { Root = merged[rootMessageId], Replies = [], Loading = false, AtCapacity = true, Error = "Tråden er større enn grensa i denne Windows-versjonen." }
                        : existing with { Root = merged[rootMessageId], Replies = Array.AsReadOnly(replies), Loading = false, Error = null, AtCapacity = false };
                    Merge(merged[rootMessageId], allowInsert: false);
                }
                return thread;
            });
        }
        catch (FrameCapacityException)
        {
            return InView(fence, () =>
            {
                NativeClientWire.Require(thread?.Generation == visit);
                thread = thread! with { Loading = false, AtCapacity = true, Root = null, Replies = [], Error = "Tråden er større enn svargrensa på 20 MiB i denne Windows-versjonen." };
                return thread;
            });
        }
        catch (Exception failure)
        {
            lock (gate) if (SameView(channelId, generation) && thread?.Generation == visit)
            {
                if (failure is NativeCommandException { Code: "permission_denied" }) ClearDeniedView(channelId, generation);
                else thread = thread with { Loading = false, Error = "Tråden kunne ikkje hentast. Prøv igjen." };
            }
            throw;
        }
        finally { Notify(); }
    }
    private async Task<IReadOnlyList<NativeMessage>> ReadThreadAsync(string channelId, string rootMessageId, SendFence fence, CancellationToken cancellationToken)
    {
        var response = await CommandAsync("load_thread", new { root_message_id = rootMessageId }, null, cancellationToken, fence);
        NativeClientWire.Require(response.GetProperty("type").GetString() == "thread_loaded" && response.GetProperty("payload").GetProperty("root_message_id").GetString() == rootMessageId);
        var raw = response.GetProperty("payload").GetProperty("messages");
        NativeClientWire.Require(raw.GetArrayLength() <= 100000);
        var loaded = raw.EnumerateArray().Select(ParseMessage).ToArray();
        NativeClientWire.Require(loaded.Length > 0 && loaded.All(m => m.ChannelId == channelId && m.Sequence > 0 &&
            (m.Id == rootMessageId ? m.ParentMessageId is null : m.ParentMessageId == rootMessageId)) && loaded.Count(m => m.Id == rootMessageId) == 1 &&
            loaded.Select(m => m.Id).Distinct(StringComparer.Ordinal).Count() == loaded.Length && loaded.Zip(loaded.Skip(1), (a,b) => a.Sequence < b.Sequence).All(v => v));
        InView(fence, () => true);
        return Array.AsReadOnly(loaded);
    }
    internal async Task<NativeMessage?> ReadMessageAsync(string channelId, string rootMessageId, string messageId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        try
        {
            var loaded = await ReadThreadAsync(channelId, rootMessageId, fence, cancellationToken);
            var found = loaded.FirstOrDefault(m => m.Id == messageId);
            return InView(fence, () =>
            {
                if (found is null) return null;
                var admitted = LatestKnown(found); Merge(admitted, allowInsert: false); return admitted;
            });
        }
        catch (NativeCommandException failure) when (failure.Code == "permission_denied") { if (ClearDeniedView(channelId, generation)) Notify(); throw; }
    }
    internal async Task<IReadOnlyList<NativeThreadSummary>> ListThreadSummariesAsync(string channelId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        await summaryGate.WaitAsync(cancellationToken);
        try
        {
            for (int attempt = 0; attempt < 4; attempt++)
            {
                long revision = InView(fence, () => summaryRevision);
                var response = await CommandAsync("list_thread_summaries", new { channel_id = channelId }, null, cancellationToken, fence);
                NativeClientWire.Require(response.GetProperty("type").GetString() == "thread_summaries_listed" && response.GetProperty("payload").GetProperty("channel_id").GetString() == channelId);
                var raw = response.GetProperty("payload").GetProperty("summaries"); NativeClientWire.Require(raw.GetArrayLength() <= 10000);
                var summaries = raw.EnumerateArray().Select(ParseThreadSummary).ToArray();
                NativeClientWire.Require(summaries.Select(s => s.RootMessageId).Distinct(StringComparer.Ordinal).Count() == summaries.Length);
                bool applied = InView(fence, () => { if (revision != summaryRevision) return false; threadSummaries = summaries; summariesLoaded = true; summaryError = null; return true; });
                if (applied) { Notify(); return Array.AsReadOnly(summaries); }
            }
            throw NativeCommandException.Uncertain("thread-summary-read");
        }
        catch (NativeCommandException failure) when (AccessDenied(failure)) { if (ClearDeniedView(channelId, generation)) Notify(); throw; }
        catch { lock (gate) if (SameView(channelId, generation)) summaryError = "Trådoversikta kunne ikkje hentast. Prøv igjen."; Notify(); throw; }
        finally { summaryGate.Release(); }
    }
    internal async Task MarkThreadVisibleReadAsync(string channelId, string rootMessageId, long generation, long threadVisit,
        IReadOnlyList<string> visibleMessageIds, string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        if (visibleMessageIds.Count == 0) return;
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation, threadVisit);
        await readGate.WaitAsync(cancellationToken);
        try
        {
            ulong sequence = InView(fence, () =>
            {
                if (thread is not { Loading: false, Error: null, AtCapacity: false } current || current.RootMessageId != rootMessageId || current.Generation != threadVisit) return 0UL;
                ulong measured = current.Replies.Where(m => visibleMessageIds.Contains(m.Id)).Select(m => m.Sequence).DefaultIfEmpty().Max();
                return measured > current.LastReadSequence ? measured : 0UL;
            });
            if (sequence == 0) return;
            long beforeReadRevision = InView(fence, () => summaryRevision);
            var response = await CommandAsync("mark_thread_read", new { root_message_id = rootMessageId, sequence }, null, cancellationToken, fence);
            NativeClientWire.Require(response.GetProperty("type").GetString() == "thread_read_updated");
            var summary = ParseThreadSummary(response.GetProperty("payload").GetProperty("summary"));
            NativeClientWire.Require(summary.RootMessageId == rootMessageId && summary.LatestSequence >= sequence);
            InView(fence, () =>
            {
                NativeClientWire.Require(thread?.Generation == threadVisit && thread.RootMessageId == rootMessageId);
                thread = thread! with { LastReadSequence = Math.Max(thread.LastReadSequence, sequence) };
                if (beforeReadRevision == summaryRevision)
                    threadSummaries = threadSummaries.Where(s => s.RootMessageId != rootMessageId).Append(summary).ToArray();
                else { reloadSummaries = true; WakeMaintenance(); }
                summaryRevision++;
                return true;
            });
            Notify();
        }
        catch (NativeCommandException failure) when (failure.Code == "permission_denied")
        {
            bool cleared; lock (gate) cleared = thread?.Generation == threadVisit && ClearDeniedView(channelId, generation);
            if (cleared) Notify(); throw;
        }
        finally { readGate.Release(); }
    }
    internal async Task<IReadOnlyList<NativeMessageReaction>> ListChannelReactionsAsync(string channelId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        await reactionGate.WaitAsync(cancellationToken);
        try
        {
            for (int attempt = 0; attempt < 4; attempt++)
            {
                long revision = InView(fence, () => reactionRevision);
                var response = await CommandAsync("list_channel_reactions", new { channel_id = channelId }, null, cancellationToken, fence);
                NativeClientWire.Require(response.GetProperty("type").GetString() == "channel_reactions_listed" && response.GetProperty("payload").GetProperty("channel_id").GetString() == channelId);
                var raw = response.GetProperty("payload").GetProperty("reactions"); NativeClientWire.Require(raw.GetArrayLength() <= 10000);
                var listed = raw.EnumerateArray().Select(ParseReaction).ToArray();
                NativeClientWire.Require(listed.Select(r => (r.MessageId,r.Emoji)).Distinct().Count() == listed.Length && listed.All(r => r.ReactedByMe == r.UserIds.Contains(expectedUserId)));
                bool applied = InView(fence, () => { if (revision != reactionRevision) return false; reactions = listed; reactionsLoaded = true; reactionError = null; return true; });
                if (applied) { Notify(); return Array.AsReadOnly(listed); }
            }
            throw NativeCommandException.Uncertain("reaction-read");
        }
        catch (NativeCommandException failure) when (AccessDenied(failure)) { if (ClearDeniedView(channelId, generation)) Notify(); throw; }
        catch { lock (gate) if (SameView(channelId, generation)) reactionError = "Reaksjonane kunne ikkje hentast. Prøv igjen."; Notify(); throw; }
        finally { reactionGate.Release(); }
    }
    internal async Task<NativeMessage> EditMessageAsync(string channelId, string messageId, string body, string requestId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default) =>
        await MutateMessageAsync("edit_message", channelId, messageId, body, requestId, generation, expectedUserId, expectedSessionStamp, cancellationToken);
    internal async Task<NativeMessage> DeleteMessageAsync(string channelId, string messageId, string requestId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default) =>
        await MutateMessageAsync("delete_message", channelId, messageId, null, requestId, generation, expectedUserId, expectedSessionStamp, cancellationToken);
    private async Task<NativeMessage> MutateMessageAsync(string type, string channelId, string messageId, string? body, string requestId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken)
    {
        NativeClientWire.Require(Guid.TryParseExact(messageId, "D", out _) && RequestId(requestId) &&
            (body is null || !string.IsNullOrWhiteSpace(body) && Encoding.UTF8.GetByteCount(body) <= 64 * 1024));
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        object payload = type == "edit_message" ? new { message_id = messageId, body } : new { message_id = messageId };
        var response = await CommandAsync(type, payload, requestId, cancellationToken, fence);
        try
        {
            NativeClientWire.Require(response.GetProperty("type").GetString() == (type == "edit_message" ? "message_edited" : "message_deleted"));
            var message = ParseMessage(response.GetProperty("payload").GetProperty("message"));
            NativeClientWire.Require(message.Id == messageId && message.ChannelId == channelId && message.AuthorId == expectedUserId &&
                (type == "delete_message" ? message.DeletedAt is not null : message.DeletedAt is null && message.EditedAt is not null && message.Body == body!.Trim()));
            var admitted = InView(fence, () => { var current = LatestKnown(message); Merge(current, live: true, allowInsert: false); return current; }); Notify(); return admitted;
        }
        catch (Exception failure) when (failure is InvalidOperationException or KeyNotFoundException or FormatException) { throw NativeCommandException.Uncertain(requestId); }
    }
    internal async Task<NativeReactionChange> ToggleReactionAsync(string channelId, string messageId, string emoji, string requestId, long generation,
        string expectedUserId, string expectedSessionStamp, CancellationToken cancellationToken = default)
    {
        NativeClientWire.Require(Guid.TryParseExact(messageId, "D", out _) && RequestId(requestId) && !string.IsNullOrWhiteSpace(emoji) && Encoding.UTF8.GetByteCount(emoji) <= 32);
        var fence = new SendFence(expectedUserId, expectedSessionStamp, channelId, generation);
        long beforeRevision = InView(fence, () => reactionRevision);
        var response = await CommandAsync("toggle_message_reaction", new { message_id = messageId, emoji }, requestId, cancellationToken, fence);
        try
        {
            NativeClientWire.Require(response.GetProperty("type").GetString() == "message_reaction_changed");
            var change = ParseReactionChange(response.GetProperty("payload").GetProperty("change"));
            NativeClientWire.Require(change.MessageId == messageId && change.ChannelId == channelId && change.UserId == expectedUserId && change.Emoji == emoji);
            InView(fence, () =>
            {
                if (beforeRevision == reactionRevision) ApplyReaction(change);
                else { reloadReactions = true; WakeMaintenance(); }
                return true;
            }); Notify(); return change;
        }
        catch (Exception failure) when (failure is InvalidOperationException or KeyNotFoundException or FormatException) { throw NativeCommandException.Uncertain(requestId); }
    }
    private static NativeThreadSummary ParseThreadSummary(JsonElement value)
    {
        var result = new NativeThreadSummary(value.GetProperty("root_message_id").GetString()!, value.GetProperty("reply_count").GetUInt32(), value.GetProperty("unread_count").GetUInt32(), value.GetProperty("latest_sequence").GetUInt64());
        NativeClientWire.Require(Guid.TryParseExact(result.RootMessageId, "D", out _) && result.UnreadCount <= result.ReplyCount && result.LatestSequence > 0); return result;
    }
    private static NativeMessageReaction ParseReaction(JsonElement value)
    {
        var users = value.GetProperty("user_ids").EnumerateArray().Select(v => v.GetString()!).ToArray();
        var result = new NativeMessageReaction(value.GetProperty("message_id").GetString()!, value.GetProperty("emoji").GetString()!, value.GetProperty("count").GetUInt32(), value.GetProperty("reacted_by_me").GetBoolean(), Array.AsReadOnly(users));
        NativeClientWire.Require(Guid.TryParseExact(result.MessageId, "D", out _) && users.Length <= 10000 && users.Distinct(StringComparer.Ordinal).Count() == users.Length && users.All(u => Guid.TryParseExact(u, "D", out _)) && result.Count == users.Length && !string.IsNullOrEmpty(result.Emoji) && Encoding.UTF8.GetByteCount(result.Emoji) <= 32); return result;
    }
    private static NativeReactionChange ParseReactionChange(JsonElement value)
    {
        var change = new NativeReactionChange(value.GetProperty("message_id").GetString()!, value.GetProperty("channel_id").GetString()!, value.GetProperty("user_id").GetString()!, value.GetProperty("emoji").GetString()!, value.GetProperty("added").GetBoolean(), value.GetProperty("count").GetUInt32());
        NativeClientWire.Require(Guid.TryParseExact(change.MessageId, "D", out _) && Guid.TryParseExact(change.ChannelId, "D", out _) && Guid.TryParseExact(change.UserId, "D", out _) && !string.IsNullOrWhiteSpace(change.Emoji) && Encoding.UTF8.GetByteCount(change.Emoji) <= 32);
        return change;
    }
    private void ApplyReaction(NativeReactionChange change)
    {
        if (change.ChannelId != selected) return;
        var prior = reactions.FirstOrDefault(r => r.MessageId == change.MessageId && r.Emoji == change.Emoji);
        if (prior is null && reactions.Length >= 10000)
        { reactionsLoaded = false; reactionError = "Reaksjonsgrensa i denne Windows-versjonen er nådd."; return; }
        var users = prior?.UserIds.ToHashSet(StringComparer.Ordinal) ?? [];
        if (change.Added) users.Add(change.UserId); else users.Remove(change.UserId);
        if (users.Count > 10000)
        { reactionsLoaded = false; reactionError = "Reaksjonsgrensa i denne Windows-versjonen er nådd."; return; }
        reactions = reactions.Where(r => r.MessageId != change.MessageId || r.Emoji != change.Emoji).ToArray();
        if (change.Count > 0) reactions = reactions.Append(new NativeMessageReaction(change.MessageId, change.Emoji, change.Count, users.Contains(connectedUser!), Array.AsReadOnly(users.Order(StringComparer.Ordinal).ToArray()))).ToArray();
        reactionRevision++;
        if (reactionsLoaded && users.Count != change.Count) { reloadReactions = true; WakeMaintenance(); }
    }
    private static NativeMessage Newer(NativeMessage? current, NativeMessage incoming) => current is not null &&
        (current.DeletedAt is not null && incoming.DeletedAt is null || (current.DeletedAt ?? current.EditedAt ?? current.SentAt) > (incoming.DeletedAt ?? incoming.EditedAt ?? incoming.SentAt)) ? current : incoming.DeletedAt is null ? incoming : incoming with { Body = "" };
    private sealed record SendFence(string UserId, string SessionStamp, string? Channel = null, long? ViewGeneration = null, long? ThreadGeneration = null, ClientWebSocket? PersonalSocket = null);
    private void WakeMaintenance() { try { maintenance.Release(); } catch (SemaphoreFullException) { } }
    private async Task<JsonElement> CommandAsync(string type, object? payload, string? requestId, CancellationToken cancellationToken, SendFence? fence = null)
    {
        requestId ??= Guid.NewGuid().ToString("N");
        string wire = JsonSerializer.Serialize(new { protocol = Protocol, request_id = requestId, type, payload });
        (Pending Request, ClientWebSocket Socket, bool Transmit) Capture()
        {
            lock (gate)
            {
                NativeClientWire.Require(!disposed && connected && socket is { State: WebSocketState.Open });
                if (fence?.ViewGeneration is long view) NativeClientWire.Require(SameView(fence.Channel!, view));
                if (fence?.ThreadGeneration is long visit) NativeClientWire.Require(thread?.Generation == visit);
                if (fence is not null) AdmitSocket(fence.UserId, fence.SessionStamp, connectedUser, connectedStamp, socket, fence.PersonalSocket ?? socket, () => true);
                bool transmit = false; Pending request;
                if (pending.TryGetValue(requestId, out var existing)) { NativeClientWire.Require(existing.Wire == wire); request = existing; }
                else { NativeClientWire.Require(pending.Count < 64); request = new Pending(wire, fence, type); pending.Add(requestId, request); transmit = true; }
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
                            if (fence?.ViewGeneration is long view) NativeClientWire.Require(SameView(fence.Channel!, view));
                            if (fence?.ThreadGeneration is long visit) NativeClientWire.Require(thread?.Generation == visit);
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
            // The account can change after the receiver completes this task but
            // before its continuation runs. Never classify an old account's error
            // as a definitive result for the newly active account.
            return fence is null ? NativeClientWire.CommandResponse(response, requestId)
                : owner.AdmitTransport(fence.UserId, fence.SessionStamp, () => NativeClientWire.CommandResponse(response, requestId));
        }
        catch (FrameCapacityException) { throw; }
        catch (NativeCommandException failure) { SetCommandError(fence, failure.Message); throw; }
        catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested || lifetime.IsCancellationRequested) { throw; }
        catch (Exception exception) when (exception is InvalidOperationException or WebSocketException or OperationCanceledException or System.IO.IOException)
        { var failure = NativeCommandException.Uncertain(requestId); SetCommandError(fence, failure.Message); throw failure; }
        finally { lock (gate) if (--request.Waiters == 0 && pending.TryGetValue(requestId, out var current) && ReferenceEquals(current, request)) pending.Remove(requestId); }
    }
    private async Task ReceiveAsync(ClientWebSocket active, CancellationToken cancellationToken)
    {
        var buffer = new byte[8192];
        while (active.State == WebSocketState.Open && !cancellationToken.IsCancellationRequested)
        {
            using var bytes = new MemoryStream(); WebSocketReceiveResult result; bool oversized = false;
            do
            {
                result = await active.ReceiveAsync(buffer, cancellationToken);
                if (result.MessageType == WebSocketMessageType.Close) return;
                NativeClientWire.Require(result.MessageType == WebSocketMessageType.Text);
                if (!oversized && bytes.Length + result.Count > 20 * 1024 * 1024) { oversized = true; bytes.SetLength(0); }
                if (!oversized) bytes.Write(buffer, 0, result.Count);
            } while (!result.EndOfMessage);
            if (oversized)
            {
                // Consume the entire frame with bounded memory, preserving the
                // connection. Reconnecting would automatically reopen the same
                // oversized thread. Without a complete envelope we cannot
                // safely correlate it, so reads fail closed and mutations stay
                // uncertain rather than being replayed by this transport.
                lock (gate) foreach (var (id, request) in pending)
                    request.Completion.TrySetException(request.Type is "load_thread" or "list_thread_summaries" or "list_channel_reactions" or "list_users" or "list_mentions" or "list_tasks" or "list_my_channels"
                        ? new FrameCapacityException() : NativeCommandException.Uncertain(id));
                continue;
            }
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
            Pending? correlated = null;
            if (envelope.TryGetProperty("request_id", out var correlation) && correlation.ValueKind == JsonValueKind.String)
                pending.TryGetValue(correlation.GetString()!, out correlated);
            // Personal replies are consumed by the typed caller. A foreign
            // success type must never apply chat data or set a global error.
            if (correlated?.Fence?.PersonalSocket is not null && (correlated.Type != "list_my_channels" || type != "channels_listed"))
                return true;
            if (type == "channels_listed" && correlated?.Type != "list_my_channels")
                return true;
            if (type == "hello") NativeClientWire.Require(payload.GetProperty("participant_id").GetString() == connectedUser);
            else if (type == "circles_listed")
            {
                circles.Clear(); foreach (var tuple in payload.GetProperty("circles").EnumerateArray()) { var circle = tuple[0]; circles[circle.GetProperty("id").GetString()!] = circle.GetProperty("name").GetString()!; }
                channels = channels.Select(channel => channel with { CircleName = channel.CircleId is not null && circles.TryGetValue(channel.CircleId, out var name) ? name : null }).ToArray();
            }
            else if (type == "channels_listed")
            {
                IReadOnlyList<NativeChannel> listed;
                try
                {
                    listed = DecodeChannels(payload, circles);
                    if (correlated?.Fence?.PersonalSocket is not null)
                        NativeClientWire.Require(listed.All(c => Guid.TryParseExact(c.Id, "D", out _) && !string.IsNullOrWhiteSpace(c.Name) &&
                            c.Kind is "public" or "local" or "private" && (c.DirectUserId is null || Guid.TryParseExact(c.DirectUserId, "D", out _))) &&
                            listed.Select(c => c.Id).Distinct(StringComparer.Ordinal).Count() == listed.Count);
                }
                catch (Exception e) when (correlated?.Fence?.PersonalSocket is not null && e is InvalidOperationException or FormatException or KeyNotFoundException)
                {
                    correlated.Completion.TrySetException(NativeCommandException.Uncertain(correlation.GetString()!));
                    return true;
                }
                var prior = channels.ToDictionary(c => c.Id);
                channels = listed.Select(c => prior.TryGetValue(c.Id, out var known) ? c with { LastReadSequence = Math.Max(c.LastReadSequence, known.LastReadSequence), LatestSequence = Math.Max(c.LatestSequence, known.LatestSequence) } : c).ToArray();
                if (selected is not null && !channels.Any(channel => channel.Id == selected)) { selected = null; ResetHistory(); }
            }
            else if (type is "messages_loaded" or "subscription_started")
            {
                // Only a correlated, account/view-fenced continuation may
                // apply history. Late A→B→A pages must not alter the new view.
            }
            // Mutation ACKs are validated by the requesting continuation before
            // merging. Broadcast events remain authoritative for subscribed data.
            else if (type == "chat") foreach (var message in DecodeMessageEvents(envelope))
            {
                Merge(message, live: true, allowInsert: payload.GetProperty("event").GetProperty("type").GetString() == "message_accepted");
            }
            if (type == "chat" && payload.GetProperty("event").GetProperty("type").GetString() == "message_reaction_changed")
                ApplyReaction(ParseReactionChange(payload.GetProperty("event").GetProperty("change")));
            if (type == "chat" && payload.GetProperty("event").GetProperty("type").GetString() == "read_marker_updated")
            {
                var read = payload.GetProperty("event");
                if (read.GetProperty("user_id").GetString() == connectedUser) ConfirmRead(read.GetProperty("channel_id").GetString()!, read.GetProperty("sequence").GetUInt64());
            }
            if (type == "read_marker_updated" && envelope.TryGetProperty("request_id", out var readId) && readId.ValueKind == JsonValueKind.String && readId.GetString() is string readRequestId && pending.TryGetValue(readRequestId, out var readRequest))
            {
                using var request = JsonDocument.Parse(readRequest.Wire);
                var sent = request.RootElement;
                var membership = payload.GetProperty("membership");
                if (sent.GetProperty("type").GetString() == "mark_read" && membership.GetProperty("user_id").GetString() == connectedUser && membership.GetProperty("channel_id").GetString() == sent.GetProperty("payload").GetProperty("channel_id").GetString())
                    ConfirmRead(membership.GetProperty("channel_id").GetString()!, membership.GetProperty("last_read_sequence").GetUInt64());
            }
            else if (type == "error" && envelope.TryGetProperty("request_id", out var failureId) && failureId.ValueKind == JsonValueKind.String && failureId.GetString() is string failureRequestId && pending.TryGetValue(failureRequestId, out var failedRequest))
            {
                if ((failedRequest.Fence?.ViewGeneration is not long view || SameView(failedRequest.Fence.Channel!, view)) &&
                    (failedRequest.Fence?.ThreadGeneration is not long threadVisit || thread?.Generation == threadVisit))
                    error = SafeError(payload.GetProperty("message").GetString() ?? "Tenesta avviste førespurnaden.");
            }
            if (type is "circles_changed" or "circle_deleted" or "circle_left" or "membership_left") { reloadChannels = true; wake = true; }
            if (type == "lagged" && payload.GetProperty("channel_id").GetString() == selected) { reloadHistory = true; wake = true; }
            if (type == "chat" && payload.GetProperty("event").GetProperty("type").GetString() is "channel_created" or "participant_joined" or "participant_left") { reloadChannels = true; wake = true; }
        }
        if (wake) { try { maintenance.Release(); } catch (SemaphoreFullException) { } }
        Notify(); return true;
    }
    private void ConfirmRead(string channelId, ulong sequence) => channels = channels.Select(c => c.Id == channelId ? c with { LastReadSequence = Math.Max(c.LastReadSequence, sequence) } : c).ToArray();
    private NativeMessage LatestKnown(NativeMessage incoming)
    {
        var current = Newer(messages.GetValueOrDefault(incoming.Id), incoming);
        current = Newer(messageUpdates.GetValueOrDefault(incoming.Id), current);
        if (thread?.Root is { } root && root.Id == incoming.Id) current = Newer(root, current);
        if (thread?.Replies.FirstOrDefault(m => m.Id == incoming.Id) is { } reply) current = Newer(reply, current);
        return current;
    }
    private void Merge(NativeMessage message, bool live = false, bool allowInsert = true)
    {
        message = LatestKnown(message);
        channels = channels.Select(c => c.Id == message.ChannelId ? c with { LatestSequence = Math.Max(c.LatestSequence, message.Sequence) } : c).ToArray();
        if (message.ChannelId == selected && message.DeletedAt is not null)
        {
            reactions = reactions.Where(r => r.MessageId != message.Id).ToArray(); reactionRevision++;
        }
        if (thread is { } currentThread && currentThread.ChannelId == message.ChannelId)
        {
            if (message.Id == currentThread.RootMessageId && message.ParentMessageId is null)
                thread = currentThread with { Root = Newer(currentThread.Root, message) };
            else if (message.ParentMessageId == currentThread.RootMessageId)
            {
                var known = currentThread.Replies.FirstOrDefault(m => m.Id == message.Id);
                if (known is not null || currentThread.Replies.Count < MaximumThreadReplies)
                    thread = currentThread with { Replies = Array.AsReadOnly(currentThread.Replies.Where(m => m.Id != message.Id).Append(Newer(known, message)).OrderBy(m => m.Sequence).ToArray()) };
                else thread = currentThread with { AtCapacity = true, Error = "Tråden er større enn grensa i denne Windows-versjonen." };
            }
        }
        if (live && message.ChannelId == selected && message.ParentMessageId is not null)
        { summaryRevision++; reloadSummaries = true; WakeMaintenance(); }
        if (message.ChannelId != selected || message.ParentMessageId is not null) return;
        if (!allowInsert && !messages.ContainsKey(message.Id))
        {
            // Cold thread roots and mutation readback must not create a hole
            // in the contiguous root-history window. Retain newer observations
            // separately so an in-flight old history page cannot resurrect them.
            if (messageUpdates.ContainsKey(message.Id) || messageUpdates.Count < MaximumHistoryMessages) messageUpdates[message.Id] = message;
            return;
        }
        if (messages.TryGetValue(message.Id, out var current))
        {
            if (current.DeletedAt is not null && message.DeletedAt is null) return;
            if ((current.DeletedAt ?? current.EditedAt ?? current.SentAt) > (message.DeletedAt ?? message.EditedAt ?? message.SentAt)) return;
        }
        else if (messages.Count >= MaximumHistoryMessages) { atCapacity = true; historyError = "Minnegrensa for samtalen er nådd. Opne samtalen på nytt for nyaste meldingar."; return; }
        messages[message.Id] = Newer(messages.GetValueOrDefault(message.Id), message);
        messageUpdates.Remove(message.Id);
    }
    private static string? OptionalString(JsonElement value, string property) => value.TryGetProperty(property, out var item) && item.ValueKind != JsonValueKind.Null ? item.GetString() : null;
    private static DateTimeOffset? OptionalDate(JsonElement value, string property) => value.TryGetProperty(property, out var item) && item.ValueKind != JsonValueKind.Null ? item.GetDateTimeOffset() : null;
    private static ulong OptionalSequence(JsonElement value, string property) => value.TryGetProperty(property, out var item) ? item.GetUInt64() : 0;
    internal static IReadOnlyList<NativeChannel> DecodeChannels(JsonElement payload, IReadOnlyDictionary<string, string> circles)
    {
        var listed = payload.GetProperty("channels"); NativeClientWire.Require(listed.GetArrayLength() <= 2000);
        return Array.AsReadOnly(listed.EnumerateArray().Select(channel =>
        {
            string? circle = OptionalString(channel, "circle_id");
            string? directUser = OptionalString(channel, "direct_user_id");
            bool direct = channel.TryGetProperty("is_direct", out var directValue) && directValue.GetBoolean();
            NativeDirectActivity? activity = null;
            if (channel.TryGetProperty("direct_activity", out var ranking) && ranking.ValueKind != JsonValueKind.Null)
            {
                double score = ranking.GetProperty("score").GetDouble(); NativeClientWire.Require(double.IsFinite(score));
                activity = new(OptionalDate(ranking, "last_message_at"), score, ranking.GetProperty("calculated_at").GetDateTimeOffset());
            }
            return new NativeChannel(channel.GetProperty("id").GetString()!, channel.GetProperty("name").GetString()!,
                circle is not null && circles.TryGetValue(circle, out var name) ? name : null, circle, channel.GetProperty("kind").GetString()!, direct || directUser is not null,
                OptionalString(channel, "slug"), OptionalString(channel, "description") ?? "", OptionalString(channel, "role"),
                OptionalSequence(channel, "last_read_sequence"), OptionalSequence(channel, "latest_sequence"), directUser, activity);
        }).ToArray());
    }
    private static NativeMessage ParseMessage(JsonElement value) => new(value.GetProperty("id").GetString()!, value.GetProperty("channel_id").GetString()!, value.GetProperty("sender_id").GetString()!, value.GetProperty("sender_display_name").GetString()!, value.GetProperty("body").GetString()!, value.GetProperty("sent_at").GetDateTimeOffset(), value.GetProperty("sequence").GetUInt64(),
        OptionalString(value, "parent_message_id"), OptionalDate(value, "edited_at"), OptionalDate(value, "deleted_at"));
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
    private static string SafeError(string value) => NativeClientWire.SafeMessage(value);
    private void SetCommandError(SendFence? fence, string value)
    {
        if (fence?.PersonalSocket is not null) return;
        lock (gate)
        {
            if (fence?.ViewGeneration is long generation && !SameView(fence.Channel!, generation)) return;
            if (fence?.ThreadGeneration is long visit && thread?.Generation != visit) return;
            error = value;
        }
        Notify();
    }
    public async ValueTask DisposeAsync()
    {
        Task? active;
        lock (gate) { if (disposed) return; disposed = true; owner.Changed -= OwnerChanged; lifetime.Cancel(); firstConnection.TrySetCanceled(lifetime.Token); socket?.Abort(); active = runner; }
        if (active is not null) { try { await active; } catch { } }
        lifetime.Dispose();
    }
}
