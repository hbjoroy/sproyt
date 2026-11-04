using Sproyt.WindowsProbe;
using System.Text.Json;

static class Program
{
    private static readonly NativePersonalScope A = new("self", "family:1", 1);
    private static NativePeopleController People(Commands commands, NativePersonalScope? scope = null) { var people = new NativePeopleController(commands); people.Bind(scope ?? A); return people; }
    private static NativeProfileController Profile(Commands commands) { var profile = new NativeProfileController(commands); profile.Bind(A); return profile; }
    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
    private static TaskCompletionSource<T> Deferred<T>() => new(TaskCreationOptions.RunContinuationsAsynchronously);
    private static async Task Main(string[] args)
    {
        if (args.Length > 1 || args.Length == 1 && !Path.IsPathFullyQualified(args[0]))
            throw new ArgumentException("Pass one absolute evidence directory, or omit it for stdout-only use.");
        await Directory(); Console.WriteLine("PASS directory identity/search/read failures");
        await Direct(); Console.WriteLine("PASS direct ACK and canonical summary admission");
        await UncertainDirect(); Console.WriteLine("PASS uncertain direct readback without replay");
        await PeopleScopes(); Console.WriteLine("PASS people full-stamp/visit and stale read fences");
        await ProfileDrafts(); Console.WriteLine("PASS own profile by ID and independent dirty drafts");
        await ProfileWrites(); Console.WriteLine("PASS profile ACK preserves newer name/status edits and clear");
        await ProfileReadback(); Console.WriteLine("PASS uncertain profile readback without replay");
        await ProfileScopes(); Console.WriteLine("PASS profile family retention and stale callbacks");
        await Rejections(); Console.WriteLine("PASS correlated definite rejection and mismatched errors");
        Console.WriteLine("PASS all 9 people/profile contract groups");
        if (args.Length == 1)
        {
            string directory = Path.Combine(Path.GetFullPath(args[0]), "people-" + Guid.NewGuid().ToString("N"));
            System.IO.Directory.CreateDirectory(directory);
            string receipt = Path.Combine(directory, "receipt.json");
            await File.WriteAllTextAsync(receipt, JsonSerializer.Serialize(new
            {
                passed = true, scope = "synthetic_people_profile_controller_contracts",
                groups = new[] { "directory identity/search/read failures", "direct ACK and canonical summary admission",
                    "uncertain direct readback without replay", "people full-stamp/visit and stale read fences",
                    "own profile by ID and independent dirty drafts", "profile ACK preserves newer name/status edits and clear",
                    "uncertain profile readback without replay", "profile family retention and stale callbacks",
                    "correlated definite rejection and mismatched errors" },
                actualWinUi = false, actualTransport = false, productionAuthentication = false, storageAcceptance = false
            }));
            Console.WriteLine("Receipt: " + receipt);
        }
    }
    private static async Task Directory()
    {
        var command = new Commands(); var people = People(command);
        await people.LoadAsync(A);
        Check(people.Capture().People.Select(p => p.Id).SequenceEqual(["other"]), "Directory included self or reordered identity.");
        people.Query(A, "ANNA"); Check(people.Capture().People.Count == 1, "Name search is not case insensitive.");
        people.Query(A, "missing"); Check(people.Capture().People.Count == 0, "Search did not filter its own view.");
        people.Query(A, "");
        command.Users = (_, _) => throw new IOException(); await people.LoadAsync(A);
        Check(people.Capture() is { Loaded: true, ReadError: not null } && people.Capture().People.Count == 1, "Failed refresh destroyed previous read projection.");
        Check(command.DirectWrites + command.NameWrites + command.StatusWrites == 0, "Directory opening wrote data.");
    }
    private static async Task Direct()
    {
        var command = new Commands(); var people = People(command); await people.LoadAsync(A);
        Check(await people.OpenDirectAsync(A, "self") is null && command.DirectWrites == 0, "Self DM was admitted.");
        Check(await people.OpenDirectAsync(A, "other") == "dm", "ACK plus direct summary did not return channel.");
        Check(command.DirectWrites == 1 && command.ChannelReads == 1 && Guid.TryParse(command.RequestIds.Single(), out _), "DM command lost request identity.");
        command.Channels = (_, _) => Task.FromResult<IReadOnlyList<NativeChannel>>([new("dm", "Group", false, "other")]);
        Check(await people.OpenDirectAsync(A, "other") is null && people.Capture().HasUncertainDirect, "A non-direct summary authorized navigation.");
        command.Channels = (_, _) => Task.FromResult<IReadOnlyList<NativeChannel>>([new("dm", "Anna", true, "other")]);
        Check(await people.RefreshUncertainDirectAsync(A) == "dm" && command.DirectWrites == 2, "Canonical readback replayed a mutation.");
    }
    private static async Task UncertainDirect()
    {
        var command = new Commands { Open = (_, _, id, _) => throw NativeCommandException.Uncertain(id) };
        var people = People(command); await people.LoadAsync(A); await people.OpenDirectAsync(A, "other");
        Check(people.Capture().HasUncertainDirect && command.DirectWrites == 1, "Lost ACK was reported as definitive failure.");
        command.Channels = (_, _) => Task.FromResult<IReadOnlyList<NativeChannel>>([new("one", "A", true, "other"), new("two", "B", true, "other")]);
        Check(await people.RefreshUncertainDirectAsync(A) is null, "Ambiguous readback chose arbitrary channel.");
        command.Channels = (_, _) => Task.FromResult<IReadOnlyList<NativeChannel>>([new("found", "A", true, "other")]);
        Check(await people.RefreshUncertainDirectAsync(A) == "found" && command.DirectWrites == 1, "Explicit lost-ACK recovery did not use read-only canonical state.");
        await people.OpenDirectAsync(A, "other");
        command.Channels = (_, _) => Task.FromResult<IReadOnlyList<NativeChannel>>([]);
        Check(await people.RefreshUncertainDirectAsync(A) is null && !people.Capture().HasUncertainDirect && command.DirectWrites == 2,
            "Zero-match readback kept every future DM permanently held or issued a write.");
        command.Open = null; command.Channels = null;
        Check(await people.OpenDirectAsync(A, "other") == "dm" && command.DirectWrites == 3 && command.RequestIds.Distinct().Count() == 3,
            "Explicit fresh open after zero-match readback was blocked or reused prior intent.");
        command.Open = (_, _, id, _) => throw NativeCommandException.Uncertain(id);
        await people.OpenDirectAsync(A, "other");
        people.DismissUncertainDirect(A with { Visit = 2 }); Check(people.Capture().HasUncertainDirect, "Old visit dismissed current uncertainty.");
        people.DismissUncertainDirect(A); Check(!people.Capture().HasUncertainDirect && command.DirectWrites == 4, "Explicit dismissal replayed an open.");
    }
    private static async Task PeopleScopes()
    {
        var deferred = Deferred<IReadOnlyList<NativePerson>>(); var command = new Commands { Users = (_, _) => deferred.Task };
        var people = People(command); people.Query(A, "Anna"); var old = people.LoadAsync(A);
        var next = A with { SessionStamp = "family:2", Visit = 2 }; people.Bind(next);
        deferred.SetResult([command.Other, command.Self]); await old;
        Check(people.Capture() is { Loaded: false, Query: "Anna" } && people.Capture().People.Count == 0, "Old full-stamp read crossed visit.");
        people.Query(A, "stale"); Check(people.Capture().Query == "Anna", "Retained old search callback changed new visit.");
        people.Bind(null); Check(people.Capture().Scope is null && people.Capture().Query.Length == 0, "Closed projection exposed old query.");
        people.Bind(next with { Visit = 3 }); Check(people.Capture().Query == "Anna", "Same-family reopen lost query.");
        people.Query(next, "Old visit edit"); Check(people.Capture().Query == "Anna", "Same-stamp old visit edited reopened search.");
        people.Bind(next with { SessionStamp = "family:3", Visit = 3 });
        people.Query(next with { Visit = 3 }, "Old token edit"); Check(people.Capture().Query == "Anna", "Old full stamp edited same-generation search.");
        people.Bind(next with { UserId = "different" }); Check(people.Capture().Query.Length == 0, "Query leaked across accounts.");
    }
    private static async Task ProfileDrafts()
    {
        var command = new Commands(); var profile = Profile(command); await profile.LoadAsync(A);
        Check(profile.Capture().Profile?.Id == "self" && profile.Capture().NameDraft == "Original", "Profile selected first directory row instead of own ID.");
        profile.EditName(A, "My draft"); profile.EditStatusText(A, "Private draft"); profile.EditStatusEmoji(A, "🌿");
        command.Self = command.Self with { DisplayName = "External", StatusText = "External status", StatusEmoji = "☀" };
        await profile.LoadAsync(A);
        Check(profile.Capture() is { NameDraft: "My draft", StatusTextDraft: "Private draft", StatusEmojiDraft: "🌿" } && profile.Capture().Profile?.DisplayName == "External", "Reload overwrote dirty fields or failed canonical update.");
        command.Users = (_, _) => throw new IOException(); await profile.LoadAsync(A);
        Check(profile.Capture().Error is not null && profile.Capture().NameDraft == "My draft", "Read failure discarded draft.");
    }
    private static async Task ProfileWrites()
    {
        var command = new Commands(); var profile = Profile(command); await profile.LoadAsync(A);
        var receipt = Deferred<NativePerson>(); command.Name = (_, _, _, _) => receipt.Task;
        profile.EditName(A, "Submitted"); var saving = profile.SaveNameAsync(A);
        profile.EditName(A, "Newer edit"); profile.EditStatusText(A, "Status draft");
        receipt.SetResult(command.Self with { DisplayName = "Submitted" }); await saving;
        Check(profile.Capture().NameDraft == "Newer edit" && profile.Capture().StatusTextDraft == "Status draft" && profile.Capture().Profile?.DisplayName == "Submitted", "ACK lost a newer or independent field edit.");
        command.Self = command.Self with { DisplayName = "Submitted" };
        profile.EditStatusEmoji(A, "🌿"); await profile.SaveStatusAsync(A);
        Check(command.Self.StatusText == "Status draft" && command.Self.StatusEmoji == "🌿" && profile.Capture().NameDraft == "Newer edit", "Status save modified independent name draft.");
        await profile.ClearStatusAsync(A);
        Check(command.Self.StatusText == "" && command.Self.StatusEmoji == "" && profile.Capture().StatusTextDraft == "" && command.StatusWrites == 2, "Explicit status clear did not acknowledge empty fields.");
        var echoes = new Commands(); var echoProfile = Profile(echoes); await echoProfile.LoadAsync(A);
        echoProfile.EditName(A, "Echo name"); await echoProfile.SaveNameAsync(A);
        string? nameNotice = echoProfile.Capture().Notice;
        echoProfile.EditName(A, "Echo name");
        Check(nameNotice is not null && echoProfile.Capture().Notice == nameNotice && echoProfile.Capture().Profile?.DisplayName == "Echo name",
            "An identical native name echo erased the acknowledged notice.");
        echoes.Self = echoes.Self with { DisplayName = "External clean name" }; await echoProfile.LoadAsync(A);
        Check(echoProfile.Capture().NameDraft == "External clean name", "An identical name echo made a clean field dirty.");
        echoProfile.EditStatusText(A, "Echo status"); echoProfile.EditStatusEmoji(A, "🌿"); await echoProfile.SaveStatusAsync(A);
        string? statusNotice = echoProfile.Capture().Notice;
        echoProfile.EditStatusText(A, "Echo status"); echoProfile.EditStatusEmoji(A, "🌿");
        Check(statusNotice is not null && echoProfile.Capture().Notice == statusNotice && echoProfile.Capture().Profile?.StatusText == "Echo status",
            "Identical native status echoes erased the acknowledged notice.");
        echoes.Self = echoes.Self with { StatusText = "External clean status", StatusEmoji = "☀" }; await echoProfile.LoadAsync(A);
        Check(echoProfile.Capture().StatusTextDraft == "External clean status" && echoProfile.Capture().StatusEmojiDraft == "☀",
            "Identical status echoes made clean fields dirty.");
    }
    private static async Task ProfileReadback()
    {
        var command = new Commands { Name = (_, _, id, _) => throw NativeCommandException.Uncertain(id) };
        var profile = Profile(command); await profile.LoadAsync(A); profile.EditName(A, "Wanted"); await profile.SaveNameAsync(A);
        Check(profile.Capture() is { HasUncertainWrite: true, CanSave: false, NameDraft: "Wanted" }, "Uncertain write was silently retryable or lost draft.");
        await profile.ReadBackAsync(A);
        Check(profile.Capture() is { HasUncertainWrite: true, CanSave: true, NameDraft: "Wanted" } && command.NameWrites == 1, "Mismatched readback replayed or discarded uncertain draft.");
        command.Self = command.Self with { DisplayName = "Wanted" }; await profile.ReadBackAsync(A);
        Check(!profile.Capture().HasUncertainWrite && command.NameWrites == 1 && profile.Capture().Notice is not null, "Matching postcondition did not confirm without replay.");
        command.Status = (_, text, emoji, id, _) =>
        { command.Self = command.Self with { StatusText = text, StatusEmoji = emoji }; throw NativeCommandException.Uncertain(id); };
        profile.EditStatusText(A, "Committed status"); profile.EditStatusEmoji(A, "🌿"); await profile.SaveStatusAsync(A);
        Check(profile.Capture().HasUncertainWrite, "Lost status acknowledgement was silently accepted.");
        await profile.ReadBackAsync(A);
        Check(!profile.Capture().HasUncertainWrite && command.StatusWrites == 1, "Status readback replayed a write.");
        command.Status = (_, text, emoji, _, _) => Task.FromResult(command.Self = command.Self with
            { StatusText = text, StatusEmoji = emoji, StatusExpiresAt = DateTimeOffset.UtcNow.AddHours(1) });
        profile.EditStatusText(A, "Expiry check"); await profile.SaveStatusAsync(A); await profile.ReadBackAsync(A);
        Check(profile.Capture().HasUncertainWrite && command.StatusWrites == 2, "Expiring status falsely confirmed a no-expiry write.");
        command.Self = command.Self with { StatusExpiresAt = null }; await profile.ReadBackAsync(A);
        Check(!profile.Capture().HasUncertainWrite && command.StatusWrites == 2, "Correct expiry postcondition did not confirm read-only.");
    }
    private static async Task ProfileScopes()
    {
        var command = new Commands(); var profile = Profile(command); await profile.LoadAsync(A); profile.EditName(A, "Private");
        var receipt = Deferred<NativePerson>(); command.Name = (_, _, _, _) => receipt.Task; var old = profile.SaveNameAsync(A);
        profile.Bind(null); Check(profile.Capture().Profile is null && profile.Capture().NameDraft == "", "Null scope exposed profile fields.");
        var next = A with { SessionStamp = "family:2", Visit = 3 }; profile.Bind(next); receipt.SetResult(command.Self with { DisplayName = "Late" }); await old;
        Check(profile.Capture().Profile is null && profile.Capture().NameDraft == "Private", "Old write applied after reopen.");
        profile.EditName(A, "Stale edit"); Check(profile.Capture().NameDraft == "Private", "Old control edited a new visit.");
        command.Name = null; await profile.LoadAsync(next); Check(profile.Capture().NameDraft == "Private", "Same-family refresh erased dirty name.");
        profile.Bind(next with { SessionStamp = "different-family:1" }); Check(profile.Capture().NameDraft == "", "Draft crossed login families.");
    }
    private static async Task Rejections()
    {
        var invalid = new Commands(); var local = Profile(invalid); await local.LoadAsync(A);
        foreach (string name in new[] { "   ", new string('x', 121), string.Concat(Enumerable.Repeat("🌿", 31)), "broken\ud800" })
        {
            local.EditName(A, name); await local.SaveNameAsync(A);
            Check(invalid.NameWrites == 0 && local.Capture() is { HasUncertainWrite: false, Error: not null } && local.Capture().NameDraft == name,
                "Invalid name dispatched a command, became uncertain, or discarded its private draft.");
        }
        local.EditName(A, string.Concat(Enumerable.Repeat("🌿", 30))); await local.SaveNameAsync(A);
        Check(invalid.NameWrites == 1 && local.Capture().Error is null, "Exact UTF-8 name byte boundary was rejected.");
        foreach (var value in new[] { (new string('x', 101), "🌿"), ("Status\ntext", "🌿"), ("Status", new string('x', 17)),
            ("Status", "x\u0001y"), ("broken\ud800", "🌿") })
        {
            local.EditStatusText(A, value.Item1); local.EditStatusEmoji(A, value.Item2); await local.SaveStatusAsync(A);
            Check(invalid.StatusWrites == 0 && local.Capture() is { HasUncertainWrite: false, Error: not null } &&
                local.Capture().StatusTextDraft == value.Item1 && local.Capture().StatusEmojiDraft == value.Item2,
                "Invalid status dispatched a command, became uncertain, or discarded a field.");
        }
        local.EditStatusText(A, string.Concat(Enumerable.Repeat("🌿", 100))); local.EditStatusEmoji(A, string.Concat(Enumerable.Repeat("🌿", 16)));
        await local.SaveStatusAsync(A);
        Check(invalid.StatusWrites == 1 && local.Capture().Error is null, "Unicode scalar boundaries were counted as UTF-16 units.");
        var command = new Commands { Name = (_, _, id, _) => throw NativeCommandException.FromServer(id, "validation_error", "Rejected") };
        var profile = Profile(command); await profile.LoadAsync(A); profile.EditName(A, "Rejected edit"); await profile.SaveNameAsync(A);
        Check(!profile.Capture().HasUncertainWrite && profile.Capture().Error is not null && profile.Capture().NameDraft == "Rejected edit", "Correlated rejection lost draft or became uncertain.");
        command.Name = (_, _, _, _) => throw NativeCommandException.FromServer("wrong-id", "validation_error", "Rejected"); await profile.SaveNameAsync(A);
        Check(profile.Capture().HasUncertainWrite, "Mismatched rejection falsely proved no commit.");
        await profile.ReadBackAsync(A);
        command.Name = (_, name, _, _) => Task.FromResult(command.Other with { DisplayName = name });
        await profile.SaveNameAsync(A);
        Check(profile.Capture().HasUncertainWrite && profile.Capture().Profile?.Id == "self", "Wrong-account receipt entered canonical own profile.");
    }
}
