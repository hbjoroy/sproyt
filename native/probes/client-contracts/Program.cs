using System.Diagnostics;
using System.Text.Json;

internal static class Program
{
    private static void Check(bool condition, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0)
    { if (!condition) throw new InvalidOperationException($"Client contract assertion failed at line {line}."); }
    private static async Task<NativeCommandException> Failure(Task operation)
    {
        try { await operation.WaitAsync(TimeSpan.FromSeconds(8)); }
        catch (NativeCommandException failure) { return failure; }
        throw new InvalidOperationException("Expected a typed command failure.");
    }
    private static async Task Until(Func<bool> condition)
    {
        var timeout = Stopwatch.StartNew();
        while (!condition()) { Check(timeout.Elapsed < TimeSpan.FromSeconds(8)); await Task.Delay(20); }
    }
    private static JsonElement Json(object value) => JsonSerializer.SerializeToElement(value);
    private static NativeCommandException DecodeFailure(object envelope, string id)
    {
        try { NativeClientWire.CommandResponse(Json(envelope), id); }
        catch (NativeCommandException failure) { return failure; }
        throw new InvalidOperationException("Expected response failure.");
    }

    public static async Task<int> Main(string[] args)
    {
        try
        {
            Check(args.Length == 1);
            await Run();
            await File.WriteAllTextAsync(args[0], JsonSerializer.Serialize(new
            {
                passed = true, production_transport_source = true, local_websocket = true, synthetic_session_owner = true,
                live_credentials = false, live_canary = false, domain_rejections = true, uncertain_server_errors = true,
                correlated_errors_only = true, lost_ack_exact_wire_replay = true, duplicate_waiter_single_frame = true,
                account_change_rejects_late_result = true, complete_channel_metadata = true, thread_edit_delete_metadata = true,
                architecture = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),
            }, new JsonSerializerOptions { WriteIndented = true }));
            Console.WriteLine("PASS native client transport contracts");
            return 0;
        }
        catch (Exception exception) { Console.Error.WriteLine(exception); return 1; }
    }

    private static async Task Run()
    {
        foreach (string code in new[] { "validation_error", "permission_denied", "quota", "conflict", "not_found" })
        {
            var failure = DecodeFailure(new { protocol = "sproyt.chat.v1", request_id = "request", type = "error", payload = new { code, message = "Rejected\u0000" } }, "request");
            Check(failure.RequestId == "request" && failure.Code == code && failure.IsDefinitiveRejection && failure.Message == "Rejected");
        }
        foreach (string code in new[] { "storage_error", "engine_stopped", "future_error", "unauthorized", "unsupported_protocol" })
        {
            var failure = DecodeFailure(new { protocol = "sproyt.chat.v1", request_id = "request", type = "error", payload = new { code, message = "Unknown" } }, "request");
            Check(!failure.IsDefinitiveRejection && failure.Code == code);
        }
        foreach (object envelope in new object[]
        {
            new { protocol = "sproyt.chat.v1", request_id = "other", type = "error", payload = new { code = "permission_denied", message = "Wrong request" } },
            new { protocol = "other", request_id = "request", type = "error", payload = new { code = "permission_denied", message = "Wrong protocol" } },
            new { protocol = "sproyt.chat.v1", request_id = "request", type = "error", payload = new { message = "Missing code" } },
            new { protocol = "sproyt.chat.v1", request_id = "request", type = "error", payload = new { code = "permission_denied\u0000", message = "Invalid code" } },
        }) Check(!DecodeFailure(envelope, "request").IsDefinitiveRejection);

        var channel = NativeChatClient.DecodeChannels(Json(new { channels = new[] { new
        {
            id = FixtureServer.Channel, name = "Channel", slug = "a-channel", kind = "private", description = "**Description**",
            circle_id = "circle", role = "observer", last_read_sequence = 12UL, latest_sequence = 99UL,
            direct_user_id = FixtureServer.User, is_direct = false,
            direct_activity = new { last_message_at = "2026-11-02T12:00:00Z", score = 4.25, calculated_at = "2026-11-03T12:00:00Z" },
        } } }), new Dictionary<string, string> { ["circle"] = "Circle" }).Single();
        Check(channel.Slug == "a-channel" && channel.Description == "**Description**" && channel.Role == "observer" && channel.LastReadSequence == 12 && channel.LatestSequence == 99);
        Check(channel.IsDirect && channel.DirectUserId == FixtureServer.User && channel.CircleName == "Circle" && channel.DirectActivity?.Score == 4.25 && channel.DirectActivity.LastMessageAt is not null);
        var legacy = NativeChatClient.DecodeChannels(Json(new { channels = new[] { new { id = FixtureServer.Channel, name = "Legacy", kind = "private" } } }), new Dictionary<string, string>()).Single();
        Check(legacy.Role is null && legacy.LastReadSequence == 0 && !legacy.IsDirect && legacy.DirectActivity is null);
        var message = new
        {
            id = "36c2229f-b714-4712-9f72-14ba290aa498", channel_id = FixtureServer.Channel, sender_id = FixtureServer.User,
            sender_display_name = "Sender", body = "Updated", sent_at = "2026-11-01T12:00:00Z", sequence = 40UL,
            parent_message_id = "cbf993fa-4f75-4d51-ae76-6fbe29d0f565", edited_at = "2026-11-02T12:00:00Z", deleted_at = "2026-11-03T12:00:00Z",
        };
        foreach (string type in new[] { "message_accepted", "message_edited", "message_deleted", "chat", "messages_loaded", "subscription_started" })
        {
            object payload = type switch
            {
                "chat" => new { @event = new { type = "message_deleted", message } },
                "messages_loaded" => new { messages = new[] { message } },
                "subscription_started" => new { history = new[] { message } },
                _ => new { message },
            };
            var decoded = NativeChatClient.DecodeMessageEvents(Json(new { protocol = "sproyt.chat.v1", type, payload })).Single();
            Check(decoded.ParentMessageId == message.parent_message_id && decoded.EditedAt == DateTimeOffset.Parse(message.edited_at) && decoded.DeletedAt == DateTimeOffset.Parse(message.deleted_at));
        }

        await using var fixture = await FixtureServer.StartAsync();
        var owner = new NativeAuthSession(fixture.SocketUri);
        await using var client = new NativeChatClient(owner);
        await client.StartAsync();
        Check(client.Snapshot.Channels.Single().Role == "moderator" && client.Snapshot.Channels.Single().LastReadSequence == 7);
        Task<NativeMessage> Send(string id, string body = "Synthetic message") => client.SendAsync(FixtureServer.Channel, body, id, FixtureServer.User, "synthetic:family:0");
        foreach (string code in new[] { "validation_error", "permission_denied", "quota", "conflict", "not_found", "storage_error", "engine_stopped", "future_error" })
        {
            string id = "request_" + code; fixture.Failures[id] = code;
            var failure = await Failure(Send(id));
            Check(failure.Code == code && failure.RequestId == id && failure.IsDefinitiveRejection == (code is "validation_error" or "permission_denied" or "quota" or "conflict" or "not_found"));
            Check(fixture.Frames[id].Count == 1 && !fixture.Accepted.ContainsKey(id));
        }
        fixture.DropFirst = "lost_ack";
        Check(!(await Failure(Send("lost_ack"))).IsDefinitiveRejection);
        await Until(() => client.Snapshot.Connected && !client.Snapshot.Loading);
        var accepted = await Send("lost_ack").WaitAsync(TimeSpan.FromSeconds(8));
        Check(accepted.ChannelId == FixtureServer.Channel && fixture.Frames["lost_ack"].Count == 2 && fixture.Frames["lost_ack"][0] == fixture.Frames["lost_ack"][1] && fixture.Accepted.Count == 1);

        fixture.Hold = "shared_waiters";
        Task<NativeMessage> first = Send("shared_waiters");
        await fixture.Held.Task.WaitAsync(TimeSpan.FromSeconds(8));
        Task<NativeMessage> second = Send("shared_waiters");
        fixture.Release.SetResult();
        var both = await Task.WhenAll(first, second).WaitAsync(TimeSpan.FromSeconds(8));
        Check(both[0].Id == both[1].Id && fixture.Frames["shared_waiters"].Count == 1);

        // The response has reached the pending task. Change account exactly
        // before the continuation's final owner admission, not merely before IO.
        int admission = 0;
        fixture.Failures["old_account"] = "permission_denied";
        owner.BeforeAdmission = () => { if (++admission == 3) owner.RequireAuthentication(); };
        var stale = await Failure(Send("old_account"));
        Check(!stale.IsDefinitiveRejection && client.Snapshot.Messages.Count == 0 && client.Snapshot.Channels.Count == 0);
        Check(fixture.Frames["old_account"].Count == 1);
    }
}
