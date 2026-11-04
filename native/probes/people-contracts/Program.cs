using Sproyt.WindowsProbe;

static class Program
{
    private static readonly NativePersonalScope A = new("self", "family:1", 1);
    private static NativePeopleController People(Commands commands, NativePersonalScope? scope = null) { var people = new NativePeopleController(commands); people.Bind(scope ?? A); return people; }
    private static NativeProfileController Profile(Commands commands) { var profile = new NativeProfileController(commands); profile.Bind(A); return profile; }
    private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
    private static TaskCompletionSource<T> Deferred<T>() => new(TaskCreationOptions.RunContinuationsAsynchronously);
    private static async Task Main()
    {
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
