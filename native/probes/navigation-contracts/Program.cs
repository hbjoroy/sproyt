using System.Text.Json;
using Sproyt.WindowsProbe;

internal static class Program
{
    private const string UserA = "bc6a105a-4399-48ed-94b6-08bc8f360a41", UserB = "3d3c1958-4ff0-4560-9f49-dcb33748ec79";
    private const string Stamp = "7:family-a:1";
    private static readonly DateTimeOffset Now = DateTimeOffset.Parse("2026-11-04T08:00:00Z");
    private static readonly NativeChannel A = new("channel-a", "Prat", "Like namn", "circle-a");
    private static readonly NativeChannel B = new("channel-b", "Idear", "Like namn", "circle-b");
    private static readonly NativeChannel C = new("channel-c", "Felles", Kind: "public");

    private static int Main(string[] args)
    {
        try
        {
            Check(args.Length == 1 && Path.IsPathFullyQualified(args[0]), "Supply an absolute, test-owned artifact directory.");
            string run = Path.Combine(args[0], "navigation-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(run);
            SearchAndCanonicalDirectory();
            DirectRankingAndStableInteraction();
            HistoryAnchorsAndPreciseTargets();
            SameTargetRetryFencesOldCompletion();
            PendingRotationRestartsCurrentWithoutLosingHistory();
            StaleResumeCannotPreemptCurrentOperation();
            ScopeRotationAndLateResults();
            AccessLossAndBoundedHistory();
            PreferencesAreAccountBound(run);
            File.WriteAllText(Path.Combine(run, "receipt.json"), JsonSerializer.Serialize(new
            {
                passed = true, scope = "native_navigation_headless_contract", source = "linked production navigation controller",
                checks = new[] { "search keeps canonical channels", "equal circle names retain distinct IDs", "server metadata DM ranking",
                    "stable targets during interaction", "back forward and precise targets", "message/sequence/pixel anchor restoration",
                    "account and family fences", "normal token rotation preserves navigation", "late A-B-A result rejection",
                    "access loss prunes history and anchors", "bounded history", "account-owned optional preferences", "storage failure remains navigable" },
                actualWinUi = false, actualTransport = false, actualDraftStorage = false
            }, new JsonSerializerOptions { WriteIndented = true }));
            Console.WriteLine("PASS native navigation: search, scopes, DM ranking, bounded history/anchors, access loss, late results and account-owned preferences.");
            Console.WriteLine("Receipt: " + Path.Combine(run, "receipt.json"));
            return 0;
        }
        catch (Exception exception) { Console.Error.WriteLine(exception); return 1; }
    }

    private static NativeNavigationController Controller(params NativeChannel[] channels)
    {
        var controller = new NativeNavigationController(clock: new FixedClock());
        controller.BindScope(UserA, Stamp);
        Check(controller.UpdateChannels(UserA, Stamp, channels.Length == 0 ? [A, B, C] : channels), "Directory admission failed.");
        return controller;
    }

    private static NativeNavigationVisit Select(NativeNavigationController controller, string id, NativeScrollAnchor? outgoing = null) =>
        controller.Select(new(id), outgoing) ?? throw new InvalidOperationException("Expected selectable target: " + id);

    private static void SearchAndCanonicalDirectory()
    {
        var direct = new NativeChannel("dm", "Ola", IsDirect: true);
        NativeChannel[] canonical = [C, A, B, direct];
        var controller = Controller(canonical);
        canonical[0] = new("mutated-input", "Not canonical");
        var state = controller.Capture();
        Equal(state.Groups.Select(group => group.Id), ["shared", "circle:circle-a", "circle:circle-b"], "Equal names collapsed distinct circles or Felles order changed.");
        controller.SetQuery("IDE");
        var filtered = controller.Capture();
        Check(filtered.Groups.Single().Channels.Single().Id == B.Id && filtered.CircleMatches == 1, "Case-insensitive channel search failed.");
        Equal(filtered.Channels.Select(channel => channel.Id), [C.Id, A.Id, B.Id, direct.Id], "Search or caller mutation changed canonical data.");
        controller.SetQuery("Like namn");
        Check(controller.Capture().Groups.Count == 2, "Circle title search merged same-name groups.");
        controller.SetDirectoryScope(NativeNavigationDirectoryScope.Direct);
        controller.SetQuery("ola");
        Check(controller.Capture().Groups.Single().Id == "direct" && controller.Capture().DirectMatches == 1, "DM scope/search failed.");
        controller.SetQuery("");
        Select(controller, B.Id);
        Check(controller.Capture().DirectoryScope == NativeNavigationDirectoryScope.Circles, "Selection failed to reveal its directory scope.");
        int notifications = 0;
        controller.Changed += () => notifications++;
        controller.UpdateChannels(UserA, Stamp, [C, A, B with { Name = "Nytt namn" }, direct]);
        Check(controller.Capture().Current?.Target.ChannelId == B.Id, "Rename lost selected resource ID.");
        int afterRename = notifications;
        controller.UpdateChannels(UserA, Stamp, [C, A, B with { Name = "Nytt namn" }, direct]);
        controller.Capture();
        Check(notifications == afterRename, "Unchanged directory/capture created a render notification loop.");
    }

    private static void DirectRankingAndStableInteraction()
    {
        NativeChannel Direct(string id, double days, double score, double calculatedDays = 0) =>
            new(id, id, IsDirect: true, DirectActivity: new(Now.AddDays(-days), score, Now.AddDays(-calculatedDays)));
        var active = Direct("dm-active", .1, 0);
        var oldActive = Direct("dm-active-old", 1, 999);
        var strong = Direct("dm-strong", 20, 8);
        var decayed = Direct("dm-decayed", 10, 8, 60); // score 2 after two half-lives
        var weaker = Direct("dm-weaker", 5, 3);
        var empty = new NativeChannel("dm-empty", "Empty", IsDirect: true);
        var controller = Controller(active, oldActive, strong, decayed, weaker, empty);
        controller.SetDirectoryScope(NativeNavigationDirectoryScope.Direct);
        string[] Ids() => controller.Capture().Groups.Single().Channels.Select(channel => channel.Id).ToArray();
        Equal(Ids(), [active.Id, oldActive.Id, strong.Id, weaker.Id, decayed.Id, empty.Id], "DM ranking did not match activity window/decayed server score.");
        Select(controller, empty.Id);
        Equal(Ids(), [active.Id, oldActive.Id, strong.Id, weaker.Id, decayed.Id, empty.Id], "Local visits incorrectly influenced DM activity.");
        controller.SetInteraction(true);
        var recentlyActive = empty with { DirectActivity = new(Now, 100, Now) };
        var newDirect = Direct("dm-new", 0, 0);
        controller.UpdateChannels(UserA, Stamp, [active, oldActive, strong, decayed, weaker, recentlyActive, newDirect]);
        Equal(Ids(), [active.Id, oldActive.Id, strong.Id, weaker.Id, decayed.Id, empty.Id, newDirect.Id], "Existing targets moved under pointer/keyboard interaction.");
        controller.SetInteraction(false);
        Check(Ids()[0] == empty.Id && Ids()[1] == newDirect.Id, "Ranking did not settle after interaction ended.");
    }

    private static void HistoryAnchorsAndPreciseTargets()
    {
        var controller = Controller();
        var a = Select(controller, A.Id);
        controller.Complete(a);
        var aAnchor = new NativeScrollAnchor("message-a", 41, -12.5, false, 500);
        var b = Select(controller, B.Id, aAnchor);
        var bAnchor = new NativeScrollAnchor("message-b", 90, 8, false, 900);
        var back = controller.Back(bAnchor)!;
        Check(back.Target.ChannelId == A.Id && back.Anchor == aAnchor, "Back failed to restore stable message/sequence/pixel anchor.");
        Check(!controller.Complete(a) && !controller.SaveAnchor(b, new(AbsoluteOffset: 1)), "Old selection callback changed a revisited channel.");
        var forward = controller.Forward()!;
        Check(forward.Target.ChannelId == B.Id && forward.Anchor == bAnchor, "Forward lost its viewport anchor.");
        var precise1 = controller.Select(new(B.Id, "message-1", 100))!;
        Check(!precise1.ChannelChanged && !precise1.Anchor.AtEnd && precise1.Anchor.MessageId == "message-1", "Same-channel precise navigation reselected transport or defaulted to tail.");
        var precise2 = controller.Select(new(B.Id, "message-2", 101), new("message-1", 100, 5, false, 1000))!;
        Check(controller.Back()!.Target == precise1.Target, "Same-channel message targets were merged into one history entry.");
        Check(!controller.Fail(precise2, "late failure"), "A stale precise-target failure overwrote a newer view.");
        Select(controller, C.Id);
        Check(!controller.Capture().CanForward, "A new branch retained stale forward history.");
        Check(controller.Select(new("not-allowed")) is null, "Unknown channel was selectable.");
        var current = controller.Capture().Current!;
        Check(!controller.SaveAnchor(current, new(PixelOffset: double.NaN)) && !controller.SaveAnchor(current, new(AbsoluteOffset: -1)), "Invalid scroll geometry entered history.");
        Check(controller.Fail(current, "Safe failure") && controller.Capture().Error == "Safe failure", "Current navigation failure was not visible.");
        controller.UpdateChannels(UserA, Stamp, [A, B, C]);
        Check(controller.Capture().Error == "Safe failure", "Unrelated directory refresh erased a navigation failure.");
        controller.Complete(current);
    }

    private static void ScopeRotationAndLateResults()
    {
        var controller = Controller();
        var a = Select(controller, A.Id);
        var b = Select(controller, B.Id, new("m", 12, 1, false, 200));
        controller.SetQuery("Prat");
        controller.BindScope(UserA, "7:family-a:2");
        Check(controller.Capture().Current?.Target == b.Target && controller.Capture().CanBack && controller.Capture().Query == "Prat", "Normal token rotation cleared navigation.");
        Check(!controller.IsCurrent(b) && !controller.SaveAnchor(b, new(AbsoluteOffset: 999)) &&
            !controller.UpdateChannels(UserA, Stamp, []), "An old full-stamp callback survived token rotation.");
        var rotatedBack = controller.Back()!;
        Check(rotatedBack.SessionStamp == "7:family-a:2" && rotatedBack.Anchor.MessageId == "m", "Rotated history used an old callback stamp or lost its anchor.");
        controller.BindScope(UserB, "8:family-b:1");
        Check(controller.Capture().Channels.Count == 0 && controller.Capture().Current is null && controller.Capture().Query == "", "Account switch exposed previous directory/history/search.");
        controller.UpdateChannels(UserB, "8:family-b:1", [A]);
        controller.BindScope(UserA, Stamp);
        controller.UpdateChannels(UserA, Stamp, [A, B]);
        var newA = Select(controller, A.Id);
        Check(newA.Generation > a.Generation && !controller.IsCurrent(a) && !controller.Complete(a), "A-B-A accepted an old visit with the same account/stamp.");
        controller.BindScope(UserA, "9:new-family:1");
        Check(controller.Capture().Current is null && controller.Capture().Channels.Count == 0, "New login family reused previous navigation.");
        controller.BindScope(null, null);
        controller.SetQuery("private");
        Check(controller.Capture().UserId is null && controller.Capture().Query == "", "Signed-out search retained account input.");
    }

    private static void SameTargetRetryFencesOldCompletion()
    {
        var controller = Controller();
        controller.Complete(Select(controller, A.Id));
        var failed = Select(controller, B.Id);
        controller.Fail(failed, "Try again");
        var retry = Select(controller, B.Id);
        Check(retry.Generation > failed.Generation && controller.Capture().Pending && controller.Capture().Error is null && !retry.ChannelChanged,
            "Retry of the same resource reused its failed async generation.");
        Check(!controller.Complete(failed) && !controller.Fail(failed, "old failure") && !controller.SaveAnchor(failed, new(AbsoluteOffset: 999)),
            "An old same-target completion/failure/anchor changed its retry.");
        var replacement = Select(controller, B.Id);
        Check(replacement.Generation > retry.Generation && !controller.IsCurrent(retry), "Replacing pending same-target navigation retained its old callback fence.");
        Check(controller.Complete(replacement), "Current same-target retry failed to complete.");
        Check(Select(controller, B.Id).Generation == replacement.Generation, "Observing an already completed selection created a new operation.");
        Check(controller.Back()?.Target.ChannelId == A.Id && !controller.Capture().CanBack,
            "Retry added duplicate entries to navigation history.");
    }

    private static void PendingRotationRestartsCurrentWithoutLosingHistory()
    {
        var controller = Controller();
        controller.Complete(Select(controller, A.Id));
        var pending = Select(controller, B.Id, new("message-a", 45, -8, false, 120));
        var wanted = new NativeScrollAnchor("message-b", 90, 4, false, 500);
        controller.SaveAnchor(pending, wanted);
        controller.SetQuery("Search survives reconnect");
        controller.SetDirectoryScope(NativeNavigationDirectoryScope.Direct);
        controller.BindScope(UserA, "7:family-a:2");
        var afterRotation = controller.Capture().Current!;
        Check(afterRotation.Target == pending.Target && !controller.IsCurrent(pending), "Token rotation changed desired target or retained old callback authority.");
        var resumed = controller.RestartCurrent()!;
        Check(resumed.Generation > afterRotation.Generation && resumed.SessionStamp == "7:family-a:2" && resumed.Anchor == wanted &&
            controller.Capture().Pending && controller.Capture().Error is null, "Resume did not take a fresh pending visit/anchor.");
        Check(controller.Capture().Query == "Search survives reconnect" && controller.Capture().DirectoryScope == NativeNavigationDirectoryScope.Direct,
            "Automatic resume rewrote the user's independent search/directory tab.");
        Check(!controller.Complete(pending) && !controller.Fail(pending, "old failure") && !controller.SaveAnchor(pending, new(AbsoluteOffset: 999)) &&
            !controller.Complete(afterRotation), "Old callbacks changed resumed navigation.");
        controller.Complete(resumed);
        var back = controller.Back()!;
        Check(back.Target.ChannelId == A.Id && back.Anchor.MessageId == "message-a" && !controller.Capture().CanBack,
            "Automatic resume duplicated history or lost the outgoing anchor.");
        controller.BindScope(UserB, "8:family-b:1");
        Check(controller.RestartCurrent() is null, "New account resumed a previous owner's target.");
    }

    private static void StaleResumeCannotPreemptCurrentOperation()
    {
        var controller = Controller();
        controller.Complete(Select(controller, A.Id));
        var resumeB = Select(controller, B.Id);
        var currentC = Select(controller, C.Id);
        using var operationC = new CancellationTokenSource();
        CancellationTokenSource? registered = null;
        Check(controller.AdmitCurrent(currentC, () => registered = operationC), "Current operation registration was rejected.");
        bool staleCallbackRan = false;
        Check(!controller.AdmitCurrent(resumeB, () =>
        {
            staleCallbackRan = true;
            registered?.Cancel();
            registered = null;
        }), "Stale resume was admitted after a newer operation registered.");
        Check(!staleCallbackRan && ReferenceEquals(registered, operationC) && !operationC.IsCancellationRequested &&
            !controller.Complete(resumeB) && controller.Complete(currentC),
            "A delayed resume preempted the current operation or completed stale state.");
    }

    private static void AccessLossAndBoundedHistory()
    {
        var controller = Controller();
        Select(controller, A.Id);
        var b = Select(controller, B.Id);
        controller.SaveAnchor(b, new("private-message", 55, 1, false, 777));
        Select(controller, C.Id);
        var oldB = controller.Back()!;
        controller.UpdateChannels(UserA, Stamp, [A, C]);
        var lost = controller.Capture();
        Check(lost.Current is null && lost.CanBack && lost.CanForward && !controller.IsCurrent(oldB), "Access loss retained the current resource or discarded valid history.");
        Check(controller.Back()!.Target.ChannelId == A.Id && controller.Forward()!.Target.ChannelId == C.Id, "Back/forward did not skip revoked channel entries.");
        controller.UpdateChannels(UserA, Stamp, [A, B, C]);
        Check(Select(controller, B.Id).Anchor.AtEnd, "Regranted access resurrected a stale private anchor.");
        for (int index = 0; index < 80; index++) controller.Select(new(A.Id, "message-" + index, (ulong)index));
        int backs = 0;
        while (controller.Back() is not null) backs++;
        Check(backs == 63, "History was unbounded or evicted the wrong entries.");
        controller.UpdateChannels(UserA, Stamp, [A, A]);
        Check(controller.Capture().Channels.Count == 0 && controller.Capture().Current is null && controller.Capture().Error is not null, "Duplicate directory identity did not fail closed.");
    }

    private static void PreferencesAreAccountBound(string run)
    {
        string root = Path.Combine(run, "preferences");
        var store = new NativeNavigationPreferenceStore(root);
        var controller = new NativeNavigationController(store, new FixedClock());
        controller.BindScope(UserA, Stamp);
        controller.SetDirectoryScope(NativeNavigationDirectoryScope.Direct);
        controller.BindScope(UserB, "8:family-b:1");
        Check(controller.Capture().DirectoryScope == NativeNavigationDirectoryScope.Circles, "Account B inherited account A preferences.");
        controller.BindScope(UserA, "9:another-family:1");
        Check(controller.Capture().DirectoryScope == NativeNavigationDirectoryScope.Direct, "Account-owned presentation preference was not restored.");
        controller.SetQuery("private search text");
        string content = File.ReadAllText(Directory.GetFiles(root).Single());
        Check(!content.Contains("private search text") && !content.Contains(A.Name), "Search/conversation data entered presentation preferences.");
        File.WriteAllText(Directory.GetFiles(root).Single(), "{\"Version\":1,\"UserId\":\"" + UserB + "\",\"DirectoryScope\":\"direct\"}");
        Check(store.Read(UserA).DirectoryScope == NativeNavigationDirectoryScope.Circles, "A mismatched stored owner was accepted.");
        File.WriteAllText(Directory.GetFiles(root).Single(), "not JSON");
        controller.BindScope(UserA, "10:corrupt-preference:1");
        Check(controller.Capture().PreferenceError is not null, "Preference corruption was hidden or became fatal.");
        string blocked = Path.Combine(run, "not-a-directory");
        File.WriteAllText(blocked, "owned test sentinel");
        var blockedController = new NativeNavigationController(new NativeNavigationPreferenceStore(blocked), new FixedClock());
        blockedController.BindScope(UserA, Stamp);
        blockedController.UpdateChannels(UserA, Stamp, [new("dm", "DM", IsDirect: true)]);
        Check(Select(blockedController, "dm") is not null && blockedController.Capture().PreferenceError is not null,
            "Failed optional preference storage blocked navigation.");
    }

    private static void Equal(IEnumerable<string> actual, string[] expected, string message) => Check(actual.SequenceEqual(expected), message);
    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
    private sealed class FixedClock : TimeProvider { public override DateTimeOffset GetUtcNow() => Now; }
}
