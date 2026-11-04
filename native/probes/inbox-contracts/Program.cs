using System.Text.Json;
using Sproyt.WindowsProbe;

var checks = new List<string>();
void Require(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
async Task Check(string name, Func<Task> test) { await test(); checks.Add(name); Console.WriteLine("PASS " + name); }
TaskCompletionSource<T> Held<T>() => new(TaskCreationOptions.RunContinuationsAsynchronously);
var scope = Data.Scope;
await Check("authoritative unread counts, immutable snapshots and quiet binding", () =>
{
    using var owner = Data.Owner(); int changed = 0; owner.Changed += () => changed++;
    owner.Bind(scope, [Data.Channel, Data.OtherChannel]); Require(changed == 0, "Unchanged bindings caused a render loop.");
    var snapshot = owner.Capture(); Require(snapshot.Ready && snapshot.Unread.Select(row => row.Count).SequenceEqual([5UL, 2UL]), "Unread counts were fabricated or reordered.");
    owner.Bind(scope, [Data.Channel with { LatestSequence = 2, LastReadSequence = 3 }]);
    Require(owner.Capture().Unread.Count == 0 && snapshot.Unread.Count == 2, "Read sequence underflow or mutable snapshot.");
    return Task.CompletedTask;
});
await Check("mention load has no read side effect and preserves exact reply source", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands();
    await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    var row = owner.Capture().Mentions.Single();
    Require(!row.Mention.Read && commands.MentionWrites == 0 && row.Destination is { Exact: true, Target.ThreadId: "thread-root", Target.Sequence: 7 }, "Loading marked a mention read or lost its exact source.");
});
await Check("tasks retain assignment with explicit channel fallback and unavailable source", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    Require(owner.Capture().Tasks.Single().Destination is { Exact: false, Target.MessageId: null, Target.ThreadId: null }, "Unknown task source pretended to be an exact root.");
    owner.Bind(scope, [Data.Channel], [Data.Message]);
    Require(owner.Capture().Tasks.Single().Destination is { Exact: true, Target.ThreadId: "thread-root" }, "Independent known source was ignored.");
    owner.Bind(scope, [], []);
    Require(owner.Capture().Tasks.Single() is { Destination: null, Task.Title: "Ei oppgåve" }, "Access loss removed the assigned task or retained source access.");
    await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(owner.Capture().Tasks.Single().Task.Status == "done", "Assigned task could not be completed after leaving its source channel.");
});
await Check("loaded mentions provide independent exact task source hints", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands();
    await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    Require(owner.TaskDestination(scope, Data.Task.Id) is { Exact: true, Target.MessageId: "message-a", Target.ThreadId: "thread-root" }, "Mention-backed task source was guessed or lost.");
});
await Check("mark-read waits for matching receipt and duplicate click stays single", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    var held = Held<string>(); commands.OnMark = _ => held.Task;
    var pending = owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    Require(owner.Capture().Mentions.Single() is { Mention.Read: false, Action.Busy: true }, "Read changed before acknowledgement.");
    await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    Require(commands.MentionWrites == 1, "A duplicate click transmitted twice.");
    held.SetResult(Data.Message.Id); await pending;
    Require(owner.Capture().Mentions.Single() is { Mention.Read: true, Action.Busy: false, Action.NeedsReadBack: false }, "Matching read receipt was not applied.");
});
await Check("definitive rejection retains confirmed state and scoped message", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    commands.OnDone = (_, _) => throw NativeCommandException.FromServer("request", "permission_denied", "Avvist av tenesta.");
    await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(owner.Capture().Tasks.Single() is { Task.Status: "open", Action.NeedsReadBack: false, Action.Error: "Avvist av tenesta." }, "Definitive refusal was treated as success or uncertainty.");
});
await Check("uncertain mention has explicit readback with no automatic replay", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    commands.OnMark = _ => throw new IOException("PRIVATE SOURCE TOKEN"); await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    for (int i = 0; i < 5; i++) owner.Capture();
    await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    Require(commands.MentionWrites == 1 && owner.Capture().Mentions.Single().Action.NeedsReadBack, "Uncertain read was replayed automatically.");
    commands.Mentions = [Data.Mention with { Read = true }]; await owner.ReadBackMentionAsync(scope, Data.Message.Id, commands);
    Require(owner.Capture().Mentions.Single() is { Mention.Read: true, Action.NeedsReadBack: false } && commands.MentionWrites == 1, "Readback performed a mutation.");
    Require(owner.Capture().Mentions.Single().Action.Error?.Contains("PRIVATE") != true, "Private exception text escaped.");
});
await Check("uncertain task reads actual server state before another explicit change", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    commands.OnDone = (_, _) => throw new IOException("PRIVATE"); await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(owner.Capture().Tasks.Single() is { Task.Status: "open", Action.NeedsReadBack: true }, "Task toggled optimistically.");
    await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands); Require(commands.TaskWrites == 1, "Uncertain task was repeated.");
    await owner.ReadBackTaskAsync(scope, Data.Task.Id, commands);
    Require(owner.Capture().Tasks.Single() is { Task.Status: "open", Action.NeedsReadBack: false, Action.Error: not null }, "Unchanged readback incorrectly reported requested completion.");
    commands.OnDone = null; await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(owner.Capture().Tasks.Single().Task.Status == "done" && commands.Requests.Distinct().Count() == 2, "Explicit retry did not use a new action ID.");
    await owner.SetTaskDoneAsync(scope, Data.Task.Id, false, commands);
    Require(owner.Capture().Tasks.Single().Task.Status == "open" && commands.TaskWrites == 3, "Reopen was not acknowledged.");
});
await Check("missing uncertain target stays visible without invented result", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    commands.OnDone = (_, _) => throw new IOException(); await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    commands.Tasks = []; await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands); await owner.ReadBackTaskAsync(scope, Data.Task.Id, commands);
    Require(owner.Capture().Tasks.Single() is { Retained: true, Action.NeedsReadBack: true, Action.Busy: false, Task.Status: "open" }, "Missing readback target was silently declared done or discarded.");
});
await Check("wrong task/read receipts remain uncertain", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    commands.OnMark = _ => Task.FromResult("other-message"); await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    Require(owner.Capture().Mentions.Single().Action.NeedsReadBack, "Wrong mention ID was acknowledged.");
    await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    commands.OnDone = (_, _) => Task.FromResult(Data.Task with { AssigneeId = "other-user", Status = "done" }); await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(owner.Capture().Tasks.Single() is { Task.Status: "open", Action.NeedsReadBack: true }, "Foreign task receipt entered this account.");
});
await Check("late list cannot roll back a newer confirmed task/read acknowledgement", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    var tasks = Held<IReadOnlyList<NativeAssignedTask>>(); commands.OnTasks = () => tasks.Task;
    var loading = owner.LoadAsync(scope, NativeInboxTab.Tasks, commands); await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    tasks.SetResult([Data.Task]); await loading;
    Require(owner.Capture().Tasks.Single().Task.Status == "done" && !owner.Capture().Loading, "Older list rolled back a confirmed completion.");
    commands.OnTasks = null; await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    var mentions = Held<IReadOnlyList<NativeMention>>(); commands.OnMentions = () => mentions.Task;
    loading = owner.LoadAsync(scope, NativeInboxTab.Mentions, commands); await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    mentions.SetResult([Data.Mention]); await loading;
    Require(owner.Capture().Mentions.Single().Mention.Read, "Older list rolled back confirmed mention read.");
});
await Check("out-of-order refresh success/error cannot replace the newer list", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); var first = Held<IReadOnlyList<NativeAssignedTask>>();
    commands.OnTasks = () => first.Task; var old = owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    commands.OnTasks = null; commands.Tasks = [Data.Task with { Status = "done" }]; await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    first.SetException(new IOException("PRIVATE")); await old;
    Require(owner.Capture() is { Ready: true, Loading: false, Error: null } && owner.Capture().Tasks.Single().Task.Status == "done", "A late refresh failure replaced current data.");
});
await Check("full stamp and surface A-B-A visits fence delayed results", async () =>
{
    foreach (var next in new[] { scope with { UserId = "user-b", SessionStamp = "2:family-b:1", Visit = 2 }, scope with { SessionStamp = "1:family-a:2" }, scope with { Visit = 3 } })
    {
        using var owner = Data.Owner(); var commands = new Commands(); var held = Held<IReadOnlyList<NativeMention>>(); commands.OnMentions = () => held.Task;
        var loading = owner.LoadAsync(scope, NativeInboxTab.Mentions, commands); owner.Bind(null); owner.Bind(next, [Data.Channel]);
        held.SetResult([Data.Mention]); await loading;
        Require(owner.Capture().Mentions.Count == 0 && owner.Capture().Scope == next, "A retired account/stamp/visit exposed delayed mentions.");
        await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands); Require(commands.MentionWrites == 0, "Old row callback sent a mutation.");
    }
});
await Check("closing from a synchronous Changed callback prevents dispatch", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    owner.Changed += () => { if (owner.Capture().Tasks.Any(row => row.Action.Busy)) owner.Bind(null); };
    await owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands);
    Require(commands.TaskWrites == 0 && !owner.Capture().Open, "A reentrant close dispatched a retired visit mutation.");
});
await Check("oversized, duplicate and foreign lists fail atomically", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands
    { Tasks = Enumerable.Range(0, 2001).Select(index => Data.Task with { Id = "task-" + index }).ToArray() };
    await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    Require(owner.Capture() is { AtCapacity: true, Ready: false } && owner.Capture().Tasks.Count == 0, "Oversized task list was silently truncated.");
    commands.Tasks = [Data.Task with { AssigneeId = "other-user" }]; await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    Require(owner.Capture() is { Ready: false, Error: not null } && owner.Capture().Tasks.Count == 0, "Foreign tasks entered the projection.");
    commands.Mentions = [Data.Mention, Data.Mention]; await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    Require(!owner.Capture().Ready && owner.Capture().Mentions.Count == 0, "Duplicate mentions partially entered the projection.");
    commands.Mentions = Enumerable.Range(0, 201).Select(index => Data.Mention with { Message = Data.Message with { Id = "message-" + index } }).ToArray();
    await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands); Require(owner.Capture().AtCapacity, "Unpaged mention boundary was not enforced.");
});
await Check("unchanged directory bind preserves read failure until explicit retry", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands { OnChannels = () => throw new IOException("PRIVATE") };
    await owner.LoadAsync(scope, NativeInboxTab.Unread, commands); owner.Bind(scope, [Data.Channel, Data.OtherChannel]);
    Require(owner.Capture().Error is not null && !owner.Capture().Error!.Contains("PRIVATE"), "Render binding cleared a network error or leaked its diagnostics.");
    commands.OnChannels = null; await owner.LoadAsync(scope, NativeInboxTab.Unread, commands);
    Require(owner.Capture() is { Ready: true, Error: null }, "Directory retry failed to recover.");
});
await Check("ambiguous source hints fall back to channel without guessing a thread", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    owner.Bind(scope, [Data.Channel], [Data.Message, Data.Message with { ParentMessageId = "other-root" }]);
    Require(owner.TaskDestination(scope, Data.Task.Id) is { Exact: false, Target.ThreadId: null }, "Ambiguous independent source invented a precise target.");
});
await Check("per-item concurrent work keeps separate pending states", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands { Tasks = [Data.Task, Data.Task with { Id = "task-b", SourceMessageId = "source-b" }] };
    await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands);
    var a = Held<NativeAssignedTask>(); var b = Held<NativeAssignedTask>(); commands.OnDone = (id, _) => id == Data.Task.Id ? a.Task : b.Task;
    var pendingA = owner.SetTaskDoneAsync(scope, Data.Task.Id, true, commands); var pendingB = owner.SetTaskDoneAsync(scope, "task-b", true, commands);
    a.SetResult(Data.Task with { Status = "done" }); await pendingA;
    Require(owner.Capture().Tasks.Single(row => row.Task.Id == "task-b").Action.Busy, "Another item's receipt unlocked a pending task.");
    b.SetException(new IOException()); await pendingB;
    Require(owner.Capture().Tasks.Single(row => row.Task.Id == Data.Task.Id).Task.Status == "done" && owner.Capture().Tasks.Single(row => row.Task.Id == "task-b").Action.NeedsReadBack, "Concurrent outcomes were mixed.");
});
await Check("membership loss purges mention metadata and fences late lists and read receipts", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    var list = Held<IReadOnlyList<NativeMention>>(); var mark = Held<string>();
    commands.OnMentions = () => list.Task; commands.OnMark = _ => mark.Task;
    var loading = owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    var writing = owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    owner.Bind(scope, [Data.OtherChannel]);
    Require(owner.Capture().Mentions.Count == 0 && owner.MentionDestination(scope, Data.Message.Id) is null, "Revoked channel left a mention body or metadata projection.");
    owner.Bind(scope, [Data.Channel, Data.OtherChannel]);
    list.SetResult([Data.Mention]); mark.SetResult(Data.Message.Id); await Task.WhenAll(loading, writing);
    Require(owner.Capture().Mentions.Count == 0, "Rejoining allowed a retired mention list/receipt to resurrect content.");
});
await Check("revoked channels remove orphan uncertain mentions and reject stale member-list content", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands(); await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    commands.OnMark = _ => throw new IOException(); await owner.MarkMentionReadAsync(scope, Data.Message.Id, commands);
    commands.Mentions = []; await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    Require(owner.Capture().Mentions.Single().Retained, "Uncertain item was not retained before access loss.");
    owner.Bind(scope, [Data.OtherChannel]);
    Require(owner.Capture().Mentions.Count == 0, "Held uncertain mention retained revoked sender/channel metadata.");
    int reads = commands.MentionReads; await owner.ReadBackMentionAsync(scope, Data.Message.Id, commands);
    Require(commands.MentionReads == reads, "Revoked mention callback dispatched a new readback.");
    commands.Mentions = [Data.Mention]; await owner.LoadAsync(scope, NativeInboxTab.Mentions, commands);
    Require(owner.Capture().Mentions.Count == 0, "Late/stale server membership content overrode the authoritative directory.");
});
await Check("bounded concurrent actions show a row error instead of silently dispatching more work", async () =>
{
    using var owner = Data.Owner(); var commands = new Commands
    { Tasks = Enumerable.Range(0, 33).Select(index => Data.Task with { Id = "task-" + index, SourceMessageId = "source-" + index }).ToArray() };
    await owner.LoadAsync(scope, NativeInboxTab.Tasks, commands); var held = Held<NativeAssignedTask>(); commands.OnDone = (_, _) => held.Task;
    var pending = commands.Tasks.Select(task => owner.SetTaskDoneAsync(scope, task.Id, true, commands)).ToArray();
    Require(commands.TaskWrites == NativeInboxController.MaximumPendingActions && owner.Capture().Tasks.Single(row => row.Task.Id == "task-32").Action.Error is not null,
        "Pending limit silently dropped input or sent excess mutations.");
    held.SetException(new IOException()); await Task.WhenAll(pending);
});
var receipt = new { passed = true, scope = "native_inbox_controller", checks,
    notTested = new[] { "actual WinUI controls", "live authenticated Canary", "two-user read synchronization" } };
if (args.Length == 1) { string path = Path.GetFullPath(args[0]); Directory.CreateDirectory(Path.GetDirectoryName(path)!); File.WriteAllText(path, JsonSerializer.Serialize(receipt, new JsonSerializerOptions { WriteIndented = true })); }
Console.WriteLine($"PASS native inbox controller: {checks.Count} ownership, receipt, readback, source and capacity checks.");
