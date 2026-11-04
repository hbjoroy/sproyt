using System.Text.Json;

if (args.Length != 1 || !Path.IsPathFullyQualified(args[0])) throw new ArgumentException("Supply an absolute test-owned evidence directory.");
var checks = new List<string>();
void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
string Request() => Guid.NewGuid().ToString("N");
async Task<NativeCommandException> Failure(Func<Task> action)
{
    try { await action(); }
    catch (NativeCommandException e) { return e; }
    throw new InvalidOperationException("Expected a rejected or uncertain result.");
}
await using var server = await FixtureServer.OpenAsync();
var owner = new NativeAuthSession(server.SocketUri);
await using var client = new NativeChatClient(owner);
await client.StartAsync();
IPersonalCommands commands = client;
var scope = new NativePersonalScope(FixtureServer.User, owner.Snapshot.SessionStamp!, 42);
var ct = CancellationToken.None;
var people = await commands.ListUsersAsync(scope, ct);
Check(people.Count == 2 && people[1].Handle == "peer" && !JsonSerializer.Serialize(people).Contains("PRIVATE"), "Directory exposed a private identity or lost public information.");
checks.Add("public directory projection");
string dmRequest = Request();
var direct = await commands.OpenDirectAsync(scope, FixtureServer.Peer, dmRequest, ct);
Check(direct.ChannelId == FixtureServer.Direct && client.Snapshot.Channels.Count == 1, "Raw DM ACK was added to authoritative directory.");
Check(server.Received.Single(f => f.Id == dmRequest).Payload.GetProperty("user_id").GetString() == FixtureServer.Peer, "DM requested wrong peer.");
var directory = await commands.RefreshPersonalChannelsAsync(scope, ct);
Check(directory.Single(c => c.Id == direct.ChannelId).DirectUserId == FixtureServer.Peer && client.Snapshot.Channels.Count == 2 && client.Snapshot.SelectedChannel != direct.ChannelId,
    "Authorized refresh lost DM metadata or navigated without the caller.");
checks.Add("verified DM ACK and separate authoritative directory refresh");
var mention = (await commands.ListMentionsAsync(scope, ct)).Single();
Check(!mention.Read && mention.Message.ParentMessageId == FixtureServer.Root && mention.Message.Sequence == 3, "Mention lost its precise thread source.");
server.SendDecoy = true;
Check(await commands.MarkMentionReadAsync(scope, mention.Message.Id, Request(), ct) == mention.Message.Id && (await commands.ListMentionsAsync(scope, ct)).Single().Read, "Unrelated response acknowledged a mention.");
Check(client.Snapshot.Channels.Count == 2, "Unsolicited directory replaced the authorized channel summary.");
checks.Add("precise mention source and correlated read");
var row = (await commands.ListTasksAsync(scope, ct)).Single();
Check(row.Id == FixtureServer.TaskId && row.AssigneeId == scope.UserId && row.SourceMessageId == mention.Message.Id, "Wrong task source or assignee.");
foreach (bool done in new[] { true, false })
{
    var result = await commands.SetTaskDoneAsync(scope, row.Id, done, Request(), ct);
    Check(result.Status == (done ? "done" : "open") && result.CompletedAt.HasValue == done, "Task completion did not use the canonical response.");
}
checks.Add("assigned tasks complete and reopen");
Check((await commands.UpdateProfileAsync(scope, " Profile ", Request(), ct)).DisplayName == "Profile", "Profile input not canonicalized.");
string emoji = string.Concat(Enumerable.Repeat("🦊", 16));
Check((await commands.SetStatusAsync(scope, " reading ", emoji, Request(), ct)).StatusEmoji == emoji, "Status did not allow 16 Unicode scalars.");
Check((await commands.SetStatusAsync(scope, "", "", Request(), ct)).StatusText == "", "Clear status not acknowledged.");
Check(server.Received.Where(f => f.Type == "set_status").All(f => f.Payload.GetProperty("expires_at").ValueKind == JsonValueKind.Null), "Unsupported expiry changed.");
checks.Add("own name, Unicode status and explicit clear");
int count = server.Received.Count;
foreach (var invalid in new Func<Task>[] { () => commands.OpenDirectAsync(scope, scope.UserId, Request(), ct),
    () => commands.UpdateProfileAsync(scope, new string('å', 61), Request(), ct),
    () => commands.SetStatusAsync(scope, "OK", emoji + "🦊", Request(), ct),
    () => commands.SetStatusAsync(scope, "an\nembedded newline", "", Request(), ct) })
    Check((await Failure(invalid)).IsDefinitiveRejection, "Local invalid input was uncertain.");
Check(server.Received.Count == count, "Invalid input crossed the transport.");
checks.Add("pre-send own DM and byte/scalar validation");
server.Override = type => type == "open_direct_channel" ? ("channels_listed", new { channels = Array.Empty<object>() }) : null;
string wrong = Request();
var uncertain = await Failure(() => commands.OpenDirectAsync(scope, FixtureServer.Peer, wrong, ct));
Check(!uncertain.IsDefinitiveRejection && uncertain.RequestId == wrong && client.Snapshot.Channels.Count == 2 && server.Received.Count(f => f.Id == wrong) == 1,
    "Wrong response type affected directory or was accepted/replayed.");
server.Override = type => type == "list_my_channels" ? ("channels_listed", new { channels = new[] { new { missing = true } } }) : null;
Check(!(await Failure(() => commands.RefreshPersonalChannelsAsync(scope, ct))).IsDefinitiveRejection && client.Snapshot.Connected && client.Snapshot.Channels.Count == 2,
    "Malformed channel reply replaced state or took down the shared socket.");
server.Override = type => type == "list_my_channels" ? ("channels_listed", new { channels = new[] { new { id = "bad", name = "Foreign", kind = "private" } } }) : null;
Check(!(await Failure(() => commands.RefreshPersonalChannelsAsync(scope, ct))).IsDefinitiveRejection && client.Snapshot.Channels.Count == 2,
    "Invalid resource identity was admitted as an authorized channel.");
server.Override = type => type switch
{
    "mark_mention_read" => ("mention_read", new { message_id = FixtureServer.Root }),
    "list_tasks" => ("tasks_listed", new { tasks = new[] { server.TaskRow(FixtureServer.Peer) } }),
    "set_task_done" => ("task_updated", new { task = server.TaskRow(scope.UserId, FixtureServer.Root) }),
    "update_profile" => ("profile_updated", new { profile = server.Person(FixtureServer.Peer) }),
    "list_users" => ("users_listed", new { users = new[] { server.Person(scope.UserId), server.Person(scope.UserId) } }),
    _ => null
};
foreach (var invalid in new Func<Task>[] { () => commands.MarkMentionReadAsync(scope, mention.Message.Id, Request(), ct),
    () => commands.ListTasksAsync(scope, ct), () => commands.SetTaskDoneAsync(scope, row.Id, false, Request(), ct),
    () => commands.UpdateProfileAsync(scope, "Profile", Request(), ct), () => commands.ListUsersAsync(scope, ct) })
    Check(!(await Failure(invalid)).IsDefinitiveRejection, "Foreign or duplicate payload was treated as confirmed.");
checks.Add("typed ACK, resource, ownership and malformed-directory isolation");
server.Override = type => type == "set_status" ? ("error", new { code = "permission_denied", message = "Rejected" }) : null;
string denied = Request();
var rejected = await Failure(() => commands.SetStatusAsync(scope, "never", "", denied, ct));
Check(rejected.IsDefinitiveRejection && rejected.RequestId == denied && client.Snapshot.Error is null, "Personal rejection leaked into the chat error or lost correlation.");
server.Override = null;
checks.Add("personal rejection stays on its surface");
server.Override = type => type == "list_users" ? ("users_listed", new { users = Array.Empty<object>(), padding = new string('x', 20 * 1024 * 1024) }) : null;
bool bounded = false;
try { await commands.ListUsersAsync(scope, ct); }
catch (IOException e) { bounded = e.Message.Contains("20 MiB", StringComparison.Ordinal); }
server.Override = null;
Check(bounded && client.Snapshot.Connected && (await commands.ListUsersAsync(scope, ct)).Count == 2,
    "Oversize list did not report capacity or prevented a subsequent explicit refresh.");
checks.Add("oversize list reported without truncation or reconnect");
server.Hold = "set_status";
string held = Request();
var pending = commands.SetStatusAsync(scope, "pending", "", held, ct);
await server.Held.Task.WaitAsync(TimeSpan.FromSeconds(3));
owner.RequireAuthentication(); server.Release.TrySetResult();
try { await pending; throw new Exception("An old-family write was confirmed to the new session."); }
catch (Exception e) when (e is NativeCommandException or InvalidOperationException or OperationCanceledException) { }
Check(server.Received.Count(f => f.Id == held) == 1, "Unconfirmed status was automatically replayed.");
checks.Add("logout while awaiting ACK; no mutation replay");
string directoryPath = Path.Combine(args[0], "personal-wire-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(directoryPath);
string receipt = Path.Combine(directoryPath, "receipt.json");
File.WriteAllText(receipt, JsonSerializer.Serialize(new { passed = true, scope = "native_personal_wire", actual_loopback_websocket = true,
    synthetic_authority = true, checks, production_authentication = false }));
Console.WriteLine(receipt);
