using System.Text.Json;
using Sproyt.WindowsProbe;

internal static class Program
{
    private static readonly NativeMediaScope A = new("a", "1:family-a"), B = new("b", "2:family-b");
    private static Task<NativeMediaFile> Fetch(NativeMediaLibrary media, NativeMediaScope scope) =>
        media.OriginalAsync("11111111-1111-1111-1111-111111111111", scope, null, CancellationToken.None);

    private static async Task<int> Main(string[] args)
    {
        if (args.Length != 1 || !Path.IsPathFullyQualified(args[0])) throw new ArgumentException("Supply an absolute evidence directory.");
        string run = Path.Combine(args[0], "media-lifecycle-" + Guid.NewGuid().ToString("N"));
        var failures = new List<string>();
        Func<string, Task>[] checks = [StaleFirstAdmission, ReentrantOriginalDuringRetirement, RotationAndSameAccountRelogin, AdmissionBeforeChangeNotification, ClosedWhileOriginalOpen,
            StalePublishPreservesDestination, PublishSerializesWithFamilyChange, SameFamilyCanOverwriteAfterRotation];
        foreach (var check in checks)
        {
            try { await check(Path.Combine(run, check.Method.Name)); }
            catch (Exception e) { failures.Add(check.Method.Name + ": " + e.Message); }
        }
        string receipt = Path.Combine(run, "receipt.json");
        File.WriteAllText(receipt, JsonSerializer.Serialize(new
        {
            passed = failures.Count == 0, scope = "native_media_lifecycle_contracts", source = "production library, synthetic authority, physical temp files",
            checks = checks.Select(c => c.Method.Name), failures
        }));
        Console.WriteLine(receipt);
        foreach (string failure in failures) Console.Error.WriteLine(failure);
        return failures.Count == 0 ? 0 : 1;
    }

    private static async Task StaleFirstAdmission(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        Task switched = Task.CompletedTask;
        owner.AfterNextScopeCheck(() =>
        {
            // Schedule a change immediately after the check. If the caller
            // still owns the auth gate the switch cannot complete until exit.
            switched = Task.Run(() => owner.Switch(B));
            if (!owner.AuthGateHeld) switched.GetAwaiter().GetResult();
        });
        try { using var old = await Fetch(media, A); }
        catch (OperationCanceledException) { }
        await switched.WaitAsync(TimeSpan.FromSeconds(5));
        using var fresh = await Fetch(media, B);
        if (File.ReadAllText(fresh.Path) != B.UserId) throw new IOException("The new family did not receive its own original.");
    }

    private static async Task ReentrantOriginalDuringRetirement(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var retired = await Fetch(media, A);
        NativeMediaFile? fresh = null;
        void Adopt() => fresh = Fetch(media, B).GetAwaiter().GetResult();
        media.Invalidated += Adopt;
        try
        {
            owner.Switch(B);
            if (File.Exists(retired.Path)) throw new IOException("The retired original survived invalidation.");
            if (fresh is null || !File.Exists(fresh.Path)) throw new IOException("Retiring A removed B's open original.");
            if (File.ReadAllText(fresh.Path) != B.UserId) throw new IOException("Incorrect bytes after scope change.");
        }
        finally { media.Invalidated -= Adopt; fresh?.Dispose(); }
        if (fresh is not null && File.Exists(fresh.Path)) throw new IOException("The released original remains on disk.");
    }

    private static async Task RotationAndSameAccountRelogin(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var file = await Fetch(media, A);
        int invalidations = 0;
        media.Invalidated += () => invalidations++;
        owner.Switch(A); // Updated credentials inside the same family.
        if (invalidations != 0 || !File.Exists(file.Path)) throw new IOException("A same-family notification retired an active original.");
        var newLogin = A with { FamilyStamp = "3:family-a-new" };
        owner.Switch(newLogin);
        if (invalidations != 1 || File.Exists(file.Path)) throw new IOException("A new login to the same account retained the previous file.");
        using var fresh = await Fetch(media, newLogin);
        try { using var stale = await Fetch(media, A); }
        catch (OperationCanceledException) { return; }
        throw new IOException("The previous login could read after a new family was admitted.");
    }

    private static async Task AdmissionBeforeChangeNotification(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var old = await Fetch(media, A);
        NativeMediaFile? fresh = null;
        try
        {
            // Another auth observer may already have mounted the new UI while
            // the library's own owner-change notification is still in transit.
            owner.Switch(B, () => fresh = Fetch(media, B).GetAwaiter().GetResult());
            if (File.Exists(old.Path) || fresh is null || File.ReadAllText(fresh.Path) != B.UserId)
                throw new IOException("Admission depended on the order of auth subscribers.");
        }
        finally { fresh?.Dispose(); }
    }
    private static async Task ClosedWhileOriginalOpen(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var file = await Fetch(media, A);
        using (var open = File.Open(file.Path, FileMode.Open, FileAccess.Read, FileShare.Read))
        {
            media.Dispose();
            if (media.Current(A)) throw new IOException("A disposed media owner still admitted the viewer.");
        }
        file.Dispose();
        if (File.Exists(file.Path) || Directory.Exists(Path.GetDirectoryName(file.Path)))
            throw new IOException("A locked original or its private folder survived final release.");
    }

    private static async Task StalePublishPreservesDestination(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var file = await Fetch(media, A);
        string staged = Path.Combine(directory, "staged"), destination = Path.Combine(directory, "chosen");
        File.Copy(file.Path, staged); File.WriteAllText(destination, "existing");
        owner.Switch(A with { FamilyStamp = "3:another-family" });
        try { media.PublishSavedFile(A, staged, destination); }
        catch (OperationCanceledException)
        {
            if (File.ReadAllText(destination) != "existing" || File.ReadAllText(staged) != A.UserId)
                throw new IOException("Rejected publication mutated the user's destination or consumed the staged file.");
            return;
        }
        throw new IOException("An old family published a save after a same-account relogin.");
    }

    private static async Task PublishSerializesWithFamilyChange(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var file = await Fetch(media, A);
        string staged = Path.Combine(directory, "staged"), destination = Path.Combine(directory, "chosen");
        File.Copy(file.Path, staged); File.WriteAllText(destination, "existing");
        string? contentsAtSwitch = null;
        Task switched = Task.CompletedTask;
        owner.AfterNextScopeCheck(() =>
        {
            switched = Task.Run(() => owner.Switch(B, () => contentsAtSwitch = File.ReadAllText(destination)));
            // If the authorization check releases its gate before rename, let
            // B complete here, making a late A overwrite deterministic.
            if (!owner.AuthGateHeld) switched.WaitAsync(TimeSpan.FromSeconds(5)).GetAwaiter().GetResult();
        });
        media.PublishSavedFile(A, staged, destination);
        await switched.WaitAsync(TimeSpan.FromSeconds(5));
        if (contentsAtSwitch != A.UserId || File.Exists(staged))
            throw new IOException("The old family's save was published after the account switched.");
    }

    private static async Task SameFamilyCanOverwriteAfterRotation(string directory)
    {
        var owner = new NativeAuthSession(A);
        using var media = new NativeMediaLibrary(owner, directory);
        using var file = await Fetch(media, A);
        string staged = Path.Combine(directory, "staged"), destination = Path.Combine(directory, "chosen");
        File.Copy(file.Path, staged); File.WriteAllText(destination, "existing");
        owner.Switch(A);
        media.PublishSavedFile(A, staged, destination);
        if (File.ReadAllText(destination) != A.UserId || File.Exists(staged))
            throw new IOException("A current-family save did not replace the chosen file.");

        File.WriteAllText(staged, "later");
        media.Dispose();
        try { media.PublishSavedFile(A, staged, destination); }
        catch (OperationCanceledException)
        {
            if (File.ReadAllText(destination) != A.UserId) throw new IOException("A retired media library overwrote the chosen file.");
            return;
        }
        throw new IOException("A disposed media library still admitted a save.");
    }
}
