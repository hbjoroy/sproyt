using System.Text.Json;
using Sproyt.WindowsProbe;

const string User = "user-a", Stamp = "1:family-a:1", Channel = "channel-a", Root = "root-a";
const long View = 12, Transport = 31;
var now = DateTimeOffset.Parse("2026-11-04T08:00:00Z");
int checks = 0;

void Require(bool value, string message) { if (!value) throw new InvalidOperationException(message); }
NativeThreadFeatureController Owner()
{
    var owner = new NativeThreadFeatureController(); owner.BindScope(User, Stamp); return owner;
}
NativeMessage Message(string id, ulong sequence, string? parent = Root, string channel = Channel) =>
    new(id, channel, "author", "Forfattar", "Melding " + id, now, sequence, ParentMessageId: parent);
NativeThreadSnapshot Thread(string root = Root, long generation = Transport, bool loading = false) =>
    new(Channel, root, View, generation, Message(root, 10, null),
        [Message(root + "-reply-1", 11, root), Message(root + "-reply-2", 13, root)], loading);
void Check(string name, Action test) { test(); checks++; Console.WriteLine("PASS " + name); }

try
{
    Require(args.Length == 1 && Path.IsPathFullyQualified(args[0]), "Supply a test-owned absolute artifacts directory.");
    Check("root and directory selection require an account", () =>
    {
        var owner = new NativeThreadFeatureController();
        Require(owner.Open(Channel, Root) is null && owner.OpenDirectory(Channel) is null, "Unbound selection was admitted.");
        owner.BindScope(User, Stamp);
        Require(owner.Open("bad\nchannel", Root) is null && owner.Open(Channel, "") is null, "Invalid resource was admitted.");
    });
    Check("same-root A-B-A and late close are fenced", () =>
    {
        var owner = Owner(); var a = owner.Open(Channel, Root)!;
        var b = owner.Open(Channel, "root-b")!; var newA = owner.Open(Channel, Root)!;
        Require(newA.Generation > b.Generation && !owner.ApplyThread(a, Thread()) && !owner.Close(a), "An old A visit entered or closed new A.");
        Require(owner.ApplyThread(newA, Thread()) && owner.Capture().Thread?.RootMessageId == Root, "Current A was not admitted.");
    });
    Check("thread anchors and token rotation preserve only intended scope", () =>
    {
        var owner = Owner(); var a = owner.Open(Channel, Root)!;
        var anchor = new NativeScrollAnchor("root-a-reply-1", 11, -9, false, 120);
        Require(owner.SaveAnchor(a, anchor), "Current anchor was refused.");
        owner.Open(Channel, "root-b"); var reopened = owner.Open(Channel, Root)!;
        Require(owner.Capture().Anchor == anchor, "Switching roots lost the separate anchor.");
        owner.ApplyThread(reopened, Thread()); owner.BindScope(User, "1:family-a:2");
        var rotated = owner.Capture();
        Require(rotated.Visit?.RootMessageId == Root && rotated.Visit.SessionStamp == "1:family-a:2" && rotated.Loading &&
            rotated.Thread is null && rotated.Anchor == anchor && !owner.ApplyThread(reopened, Thread()), "Token rotation retained old data/callbacks or lost the target.");
        owner.BindScope(User, "2:family-b:1");
        Require(owner.Capture().Visit is null && owner.Capture().Anchor.AtEnd, "New login family retained thread state.");
        owner.Open(Channel, Root); Require(owner.Capture().Anchor.AtEnd, "Old-family anchor survived new login.");
        owner.BindScope("user-b", "1:family-c:1"); Require(owner.Capture().Visit is null, "Account switch retained a private thread.");
    });
    Check("directory preserves true summaries and unread order without fabricated previews", () =>
    {
        var owner = Owner(); var visit = owner.OpenDirectory(Channel)!;
        NativeThreadSummary[] summaries = [new("root-b", 2, 0, 90), new(Root, 3, 1, 20), new("empty", 0, 0, 100)];
        Require(owner.ApplyDirectory(visit, summaries, [Message(Root, 10, null)]), "Directory was refused.");
        var snapshot = owner.Capture();
        Require(snapshot.Directory.Select(item => item.Summary.RootMessageId).SequenceEqual([Root, "root-b"]) &&
            snapshot.Directory[0].Root?.Id == Root && snapshot.Directory[1].Root is null && snapshot.Directory[0].Summary.UnreadCount == 1,
            "Directory changed server counts, retained empty threads or invented old-root content.");
        int changes = 0; owner.Changed += () => changes++;
        owner.ApplyDirectory(visit, summaries, [Message(Root, 10, null)]);
        Require(changes == 0, "Unchanged directory restarted the UI notification loop.");
        owner.Open(Channel, Root); Require(!owner.ApplyDirectory(visit, summaries), "Late directory replaced detail.");
    });
    Check("directory bounds and invalid summaries fail atomically", () =>
    {
        var owner = Owner(); var visit = owner.OpenDirectory(Channel)!;
        owner.ApplyDirectory(visit, [new(Root, 1, 0, 11)]);
        owner.ApplyDirectory(visit, [new(Root, 1, 0, 11), new(Root, 2, 0, 12)]);
        Require(owner.Capture().Error is not null && owner.Capture().Directory.Count == 0, "Duplicate roots partially mutated the directory.");
        var tooMany = Enumerable.Range(0, NativeThreadFeatureController.MaximumDirectoryRows + 1).Select(index => new NativeThreadSummary("root-" + index, 1, 0, (ulong)index)).ToArray();
        owner.ApplyDirectory(visit, tooMany);
        Require(owner.Capture().AtCapacity && owner.Capture().Directory.Count == 0, "Large directory was silently truncated.");
        owner.ApplyDirectory(visit, [new(Root, 1, 2, 11)]);
        Require(owner.Capture().Error is not null, "Impossible unread counts were admitted.");
    });
    Check("thread projection is immutable and unchanged observations are quiet", () =>
    {
        var owner = Owner(); var visit = owner.Open(Channel, Root)!; var incoming = Thread();
        owner.ApplyThread(visit, incoming); int changes = 0; owner.Changed += () => changes++;
        owner.ApplyThread(visit, incoming with { Replies = incoming.Replies.Reverse().ToArray() });
        Require(changes == 0, "Unchanged thread restarted the UI notification loop.");
        var changed = incoming with { Replies = incoming.Replies.Select(reply => reply with { Body = "Endra", EditedAt = now.AddMinutes(1) }).ToArray() };
        owner.ApplyThread(visit, changed);
        Require(changes == 1 && owner.Capture().Thread?.Replies.All(reply => reply.Body == "Endra") == true, "Current edit was not projected.");
        var source = changed.Replies.ToArray(); owner.ApplyThread(visit, changed with { Replies = source }); source[0] = Message("other", 100);
        Require(owner.Capture().Thread?.Replies.All(reply => reply.Id != "other") == true, "Mutable caller list changed an admitted projection.");
    });
    Check("malformed and oversized reply sets cannot become readable partial threads", () =>
    {
        var owner = Owner(); var visit = owner.Open(Channel, Root)!;
        owner.ApplyThread(visit, Thread() with { Replies = [Message("wrong-parent", 11, "root-b")] });
        Require(owner.Capture().Error is not null && owner.Capture().Thread is null, "Foreign-root reply was admitted.");
        owner.ApplyThread(visit, Thread() with { Replies = Enumerable.Range(0, NativeThreadFeatureController.MaximumReplies + 1)
            .Select(index => Message("reply-" + index, (ulong)index + 11)).ToArray() });
        Require(owner.Capture().AtCapacity && owner.Capture().Thread is null, "Oversized thread was silently truncated.");
    });
    Check("transport generation independently fences projection and read observations", () =>
    {
        var owner = Owner(); var visit = owner.Open(Channel, Root)!; owner.ApplyThread(visit, Thread());
        Require(!owner.ApplyThread(visit, Thread(generation: Transport + 1)) &&
            owner.VisibleRead(visit, Transport + 1, true, true, false, ["root-a-reply-1"]) is null,
            "Another transport generation entered the current detail.");
    });
    Check("visible-thread reading excludes parent offscreen and pending rows", () =>
    {
        var owner = Owner(); var visit = owner.Open(Channel, Root)!; owner.ApplyThread(visit, Thread());
        var report = owner.VisibleRead(visit, Transport, true, true, false, [Root, "root-a-reply-1", "not-visible", "pending:reply", "root-a-reply-1"]);
        Require(report is { ChannelId: Channel, RootMessageId: Root, ViewGeneration: View, ThreadGeneration: Transport } &&
            report.MessageIds.SequenceEqual(["root-a-reply-1"]), "Read request included unloaded, parent, pending or duplicate rows.");
        Require(owner.Capture().Thread?.LastReadSequence == 0, "Merely reporting visibility optimistically advanced confirmed read state.");
        Require(owner.VisibleRead(visit, Transport, false, true, false, report!.MessageIds) is null &&
            owner.VisibleRead(visit, Transport, true, false, false, report.MessageIds) is null &&
            owner.VisibleRead(visit, Transport, true, true, true, report.MessageIds) is null, "Inactive/hidden/restoring view marked replies read.");
        owner.Fail(visit, "offline"); Require(owner.VisibleRead(visit, Transport, true, true, false, report.MessageIds) is null, "Failed thread marked replies read.");
    });
    Check("deleted thread parent remains a navigable tombstone", () =>
    {
        var owner = Owner(); var visit = owner.Open(Channel, Root)!; var thread = Thread();
        owner.ApplyThread(visit, thread with { Root = thread.Root! with { Body = "", DeletedAt = now } });
        Require(owner.Capture().Thread?.Root?.DeletedAt == now && owner.Capture().Thread?.Replies.Count == 2, "Deleting parent hid an existing reply thread.");
        owner.Close(visit); Require(owner.Capture().Visit is null && owner.Capture().Thread is null, "Close retained visible thread data.");
    });
    Directory.CreateDirectory(args[0]);
    var receipt = new { passed = true, scope = "native_thread_feature_headless", checks,
        productionOwner = true, actualWinUi = false, actualTransport = false, actualDraftStorage = false,
        accountVisitAndTransportFences = true, visibleRepliesOnly = true, noOptimisticRead = true, boundedReplies = NativeThreadFeatureController.MaximumReplies };
    File.WriteAllText(Path.Combine(args[0], "receipt.json"), JsonSerializer.Serialize(receipt, new JsonSerializerOptions { WriteIndented = true }));
    Console.WriteLine($"PASS {checks} native thread feature contracts."); return 0;
}
catch (Exception error) { Console.Error.WriteLine(error); return 1; }
