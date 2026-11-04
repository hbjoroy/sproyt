using System.Text;
using System.Text.Json;

internal sealed partial class NativeChatClient : IPersonalCommands
{
    public Task<IReadOnlyList<NativePerson>> ListUsersAsync(NativePersonalScope scope, CancellationToken cancellationToken) =>
        PersonalAsync(scope, "list_users", null, null, "users_listed", payload =>
            PersonalList(payload, "users", ParsePerson, p => p.Id), cancellationToken);

    public Task<NativeDirectResult> OpenDirectAsync(NativePersonalScope scope, string userId, string requestId, CancellationToken cancellationToken)
    {
        MutationId(requestId);
        RequireInput(userId != scope.UserId && PersonalId(userId), requestId);
        return PersonalAsync(scope, "open_direct_channel", new { user_id = userId }, requestId, "direct_channel_opened", payload =>
        {
            var channel = payload.GetProperty("channel");
            NativeClientWire.Require(Text(channel, "kind") == "private");
            return new NativeDirectResult(Id(channel, "id"));
        }, cancellationToken);
    }

    public Task<IReadOnlyList<NativeChannel>> RefreshPersonalChannelsAsync(NativePersonalScope scope, CancellationToken cancellationToken) =>
        PersonalAsync(scope, "list_my_channels", null, null, "channels_listed", _ =>
        {
            // Apply merged this authorized reply in receive order. Use the
            // current directory, including any newer read markers or revocations.
            lock (gate) return (IReadOnlyList<NativeChannel>)Array.AsReadOnly(channels.ToArray());
        }, cancellationToken);

    public Task<IReadOnlyList<NativeMention>> ListMentionsAsync(NativePersonalScope scope, CancellationToken cancellationToken) =>
        PersonalAsync(scope, "list_mentions", null, null, "mentions_listed", payload =>
            PersonalList(payload, "mentions", value =>
            {
                var source = value.GetProperty("message");
                var message = ParseMessage(source);
                NativeClientWire.Require(PersonalId(message.Id) && PersonalId(message.ChannelId) && PersonalId(message.AuthorId) &&
                    (message.ParentMessageId is null || PersonalId(message.ParentMessageId)) && message.Sequence > 0);
                NativeClientWire.Require(message.Author is not null && message.Body is not null);
                if (message.DeletedAt is not null) message = message with { Body = "" };
                return new NativeMention(message, Text(value, "channel_name"), value.GetProperty("read").GetBoolean());
            }, mention => mention.Message.Id), cancellationToken);

    public Task<string> MarkMentionReadAsync(NativePersonalScope scope, string messageId, string requestId, CancellationToken cancellationToken)
    {
        MutationId(requestId);
        RequireInput(PersonalId(messageId), requestId);
        return PersonalAsync(scope, "mark_mention_read", new { message_id = messageId }, requestId, "mention_read", payload =>
        {
            NativeClientWire.Require(Id(payload, "message_id") == messageId);
            return messageId;
        }, cancellationToken);
    }

    public Task<IReadOnlyList<NativeAssignedTask>> ListTasksAsync(NativePersonalScope scope, CancellationToken cancellationToken) =>
        PersonalAsync(scope, "list_tasks", null, null, "tasks_listed", payload =>
            PersonalList(payload, "tasks", row => ParseAssignedTask(row, scope.UserId), task => task.Id), cancellationToken);

    public Task<NativeAssignedTask> SetTaskDoneAsync(NativePersonalScope scope, string taskId, bool done, string requestId, CancellationToken cancellationToken)
    {
        MutationId(requestId);
        RequireInput(PersonalId(taskId), requestId);
        return PersonalAsync(scope, "set_task_done", new { task_id = taskId, done }, requestId, "task_updated", payload =>
        {
            var result = ParseAssignedTask(payload.GetProperty("task"), scope.UserId);
            NativeClientWire.Require(result.Id == taskId && result.Status == (done ? "done" : "open"));
            return result;
        }, cancellationToken);
    }

    public Task<NativePerson> UpdateProfileAsync(NativePersonalScope scope, string displayName, string requestId, CancellationToken cancellationToken)
    {
        MutationId(requestId);
        string name = displayName.Trim();
        RequireInput(name.Length != 0 && Encoding.UTF8.GetByteCount(name) <= 120, requestId);
        return PersonalAsync(scope, "update_profile", new { display_name = name }, requestId, "profile_updated", payload =>
        {
            var person = ParsePerson(payload.GetProperty("profile"));
            NativeClientWire.Require(person.Id == scope.UserId && person.DisplayName == name);
            return person;
        }, cancellationToken);
    }

    public Task<NativePerson> SetStatusAsync(NativePersonalScope scope, string text, string emoji, string requestId, CancellationToken cancellationToken)
    {
        MutationId(requestId);
        text = text.Trim(); emoji = emoji.Trim();
        RequireInput(text.EnumerateRunes().Count() <= 100 && emoji.EnumerateRunes().Count() <= 16 &&
            !text.Any(char.IsControl) && !emoji.Any(char.IsControl), requestId);
        return PersonalAsync(scope, "set_status", new { text, emoji, expires_at = (DateTimeOffset?)null }, requestId, "status_updated", payload =>
        {
            var person = ParsePerson(payload.GetProperty("profile"));
            NativeClientWire.Require(person.Id == scope.UserId && person.StatusText == text && person.StatusEmoji == emoji && person.StatusExpiresAt is null);
            return person;
        }, cancellationToken);
    }

    private async Task<T> PersonalAsync<T>(NativePersonalScope scope, string command, object? arguments, string? id,
        string expected, Func<JsonElement, T> parse, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        string requestId = id ?? Guid.NewGuid().ToString("N");
        var active = owner.AdmitTransport(scope.UserId, scope.SessionStamp, () =>
        {
            lock (gate)
            {
                NativeClientWire.Require(connected && socket is not null);
                return socket!;
            }
        });
        var response = await CommandAsync(command, arguments, requestId, cancellationToken, new(scope.UserId, scope.SessionStamp, PersonalSocket: active));
        // Decode potentially large lists outside the auth gate, then admit the
        // typed result again using the exact account/session that issued it.
        T result;
        try
        {
            NativeClientWire.Require(Text(response, "type") == expected);
            result = parse(response.GetProperty("payload"));
        }
        catch (Exception e) when (e is InvalidOperationException or FormatException or KeyNotFoundException or OverflowException)
        { throw NativeCommandException.Uncertain(requestId); }
        cancellationToken.ThrowIfCancellationRequested();
        return owner.AdmitTransport(scope.UserId, scope.SessionStamp, () =>
        {
            lock (gate) return AdmitSocket(scope.UserId, scope.SessionStamp, connectedUser, connectedStamp, socket, active, () => result);
        });
    }

    private static IReadOnlyList<T> PersonalList<T>(JsonElement payload, string key, Func<JsonElement, T> parse, Func<T, string> identity)
    {
        var rows = payload.GetProperty(key).EnumerateArray().Select(parse).ToArray();
        NativeClientWire.Require(rows.Select(identity).Distinct(StringComparer.Ordinal).Count() == rows.Length);
        return Array.AsReadOnly(rows);
    }

    private static NativePerson ParsePerson(JsonElement row)
    {
        string kind = Text(row, "kind");
        NativeClientWire.Require(kind == "human");
        return new(Id(row, "id"), kind, OptionalString(row, "handle"), Text(row, "display_name"),
            Text(row, "status_text"), Text(row, "status_emoji"), OptionalDate(row, "status_expires_at"));
    }

    private static NativeAssignedTask ParseAssignedTask(JsonElement row, string user)
    {
        string assignee = Id(row, "assignee_id"), status = Text(row, "status");
        NativeClientWire.Require(assignee == user && status is "open" or "done");
        return new(Id(row, "id"), Id(row, "source_message_id"), Id(row, "channel_id"), Text(row, "channel_name"),
            assignee, Text(row, "title"), status, OptionalDate(row, "completed_at"));
    }

    private static bool PersonalId(string value) => Guid.TryParseExact(value, "D", out var id) && id.ToString("D") == value;
    private static string Text(JsonElement row, string key) => row.GetProperty(key).GetString() ?? throw NativeClientWire.Failure();
    private static string Id(JsonElement row, string key)
    {
        string value = Text(row, key);
        NativeClientWire.Require(PersonalId(value));
        return value;
    }
    private static void MutationId(string value) => RequireInput(RequestId(value), value);
    private static void RequireInput(bool valid, string requestId)
    {
        if (!valid) throw NativeCommandException.FromServer(requestId, "validation_error", "Kontroller verdiane og prøv igjen.");
    }
}
