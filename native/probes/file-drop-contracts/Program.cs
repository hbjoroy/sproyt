using System.Text.Json;
using Sproyt.WindowsProbe;

var checks = new List<string>();
string sample = Path.GetFullPath(Path.Combine(Path.GetTempPath(), "native-file-drop-fixture.txt"));
void Check(bool value, string message) { if (!value) throw new InvalidOperationException(message); }
Task<IReadOnlyList<NativeFileDropItem>> Items(params NativeFileDropItem[] files) => Task.FromResult<IReadOnlyList<NativeFileDropItem>>(files);

// Copy is the only offered operation. Merely hovering performs no staging.
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    Check(intake.CanOffer(true, true), "A writable file-copy hover was rejected.");
    Check(!intake.CanOffer(false, true) && !intake.CanOffer(true, false), "Text/URL or move-only hover was accepted.");
    Check(fixture.Added.Count == 0, "Hover copied a file.");
    fixture.Target = fixture.Target with { Channel = "second", Thread = "thread-b" };
    Check(await intake.DropAsync(true, true, _ => Items(new NativeFileDropItem(sample))), "Valid file drop was not submitted.");
    Check(fixture.Added.Single().Target == fixture.Target, "Drop reused the hover target instead of the actual current composer.");
    checks.Add("Copy-only hover; fresh target at Drop; no hover IO");
}
// An OS resolver may complete after navigation: preserve the original composer.
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    var captured = fixture.Target;
    var held = new TaskCompletionSource<IReadOnlyList<NativeFileDropItem>>(TaskCreationOptions.RunContinuationsAsynchronously);
    var dropping = intake.DropAsync(true, true, _ => held.Task);
    Check(fixture.Captures == 1, "The target was not synchronously captured before the resolver awaited.");
    fixture.Target = fixture.Target with { Channel = "second", Thread = "thread-b" };
    held.SetResult([new(sample)]);
    Check(await dropping, "Same-family navigation discarded the captured target.");
    Check(fixture.Added.Single().Target == captured && fixture.Captures == 1, "A delayed resolver retargeted a file or recaptured the composer.");
    checks.Add("Delayed file resolution retains captured root/thread composer");
}
foreach (string change in new[] { "account", "logout", "family", "membership", "account-roundtrip" })
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    var held = new TaskCompletionSource<IReadOnlyList<NativeFileDropItem>>(TaskCreationOptions.RunContinuationsAsynchronously);
    var dropping = intake.DropAsync(true, true, _ => held.Task);
    if (change == "account") fixture.User = "B";
    if (change == "logout") fixture.Active = false;
    if (change == "family") fixture.Family = "new-family";
    if (change == "membership") fixture.Access = false;
    if (change == "account-roundtrip") { fixture.User = "B"; fixture.User = "A"; fixture.Family = "new-A-family"; }
    fixture.Errors.Clear();
    held.SetResult([new(sample)]);
    Check(!await dropping && fixture.Added.Count == 0 && fixture.Errors.Count == 0, "Retired " + change + " drop staged a file or leaked its result/error.");
    checks.Add("Delayed result fenced after " + change);
}
// The whole file set is validated before calling the existing queue owner.
foreach (var invalid in new[]
{
    Array.Empty<NativeFileDropItem>(),
    new[] { new NativeFileDropItem(sample), new NativeFileDropItem(Path.GetTempPath(), false) },
    new[] { new NativeFileDropItem("https://example.invalid/private") },
    new[] { new NativeFileDropItem("relative-file.txt") },
    Enumerable.Range(0, 9).Select(_ => new NativeFileDropItem(sample)).ToArray()
})
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    Check(!await intake.DropAsync(true, true, _ => Items(invalid)), "Invalid drop was accepted.");
    Check(fixture.Added.Count == 0 && fixture.Errors.Last().Message is not null, "Invalid drop partially staged files or lost its scoped error.");
}
checks.Add("Mixed folders, URLs, empty/relative paths and over-capacity sets reject before submission");
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    var captured = fixture.Target;
    var held = new TaskCompletionSource<IReadOnlyList<NativeFileDropItem>>(TaskCreationOptions.RunContinuationsAsynchronously);
    var dropping = intake.DropAsync(true, true, _ => held.Task);
    fixture.Target = fixture.Target with { Channel = "other", Thread = "other-thread" };
    held.SetException(new IOException("PRIVATE FILE PATH"));
    Check(!await dropping && fixture.Errors.Last().Target == captured, "A delayed error moved to the newly selected composer.");
    checks.Add("Delayed source failure remains keyed to the captured composer");
}
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    int resolved = 0;
    Check(!await intake.DropAsync(false, true, _ => { resolved++; return Items(new NativeFileDropItem(sample)); }), "Text/URL payload was accepted.");
    Check(!await intake.DropAsync(true, false, _ => { resolved++; return Items(new NativeFileDropItem(sample)); }), "Move-only payload was accepted.");
    Check(resolved == 0 && fixture.Added.Count == 0, "Rejected formats resolved or staged data.");
    Check(!await intake.DropAsync(true, true, _ => throw new UnauthorizedAccessException("SECRET private path")), "Unreadable source was accepted.");
    Check(fixture.Errors.Last().Target == fixture.Target && !fixture.Errors.Last().Message!.Contains("SECRET"), "Read failure lost scope or exposed exception details.");
    checks.Add("Unsupported formats and unreadable sources produce safe scoped errors");
}
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    var held = new TaskCompletionSource<IReadOnlyList<NativeFileDropItem>>(TaskCreationOptions.RunContinuationsAsynchronously);
    var dropping = intake.DropAsync(true, true, _ => held.Task);
    Check(!intake.CanOffer(true, true), "Busy helper offered another transfer.");
    Check(!await intake.DropAsync(true, true, _ => throw new InvalidOperationException("Second resolver ran")), "Concurrent second drop was accepted.");
    intake.Dispose();
    Check(!await dropping.WaitAsync(TimeSpan.FromSeconds(2)), "Unmount did not retire its pending drop promptly.");
    held.TrySetResult([new(sample)]);
    await Task.Yield();
    Check(fixture.Added.Count == 0 && !intake.CanOffer(true, true), "Unmounted helper staged a late source.");
    checks.Add("Single transfer, retained-host unmount cancellation and no late staging");
}
{
    var fixture = new Fixture(); using var intake = fixture.Create();
    Check(!await intake.DropAsync(true, true, _ => throw new IOException("PRIVATE PATH TOKEN")), "Resolver failure was accepted.");
    Check(await intake.DropAsync(true, true, _ => Items(new NativeFileDropItem(sample))), "A failed drop left the host permanently busy.");
    Check(fixture.Added.Count == 1 && fixture.Errors.All(error => error.Message is null || !error.Message.Contains("PRIVATE")), "Retry duplicated data or leaked resolver diagnostics.");
    checks.Add("Resolver failure is retryable without private diagnostics or duplicate submissions");
}
foreach (bool unmount in new[] { false, true })
{
    var fixture = new Fixture();
    var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    var finish = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    using var intake = new NativeFileDropIntake<Target>(() => fixture.Target,
        target => fixture.Active && target.User == fixture.User && target.Family == fixture.Family,
        async (target, paths) => { entered.SetResult(); await finish.Task; },
        (target, message) => fixture.Errors.Add((target, message)));
    var dropping = intake.DropAsync(true, true, _ => Items(new NativeFileDropItem(sample)));
    await entered.Task.WaitAsync(TimeSpan.FromSeconds(2));
    if (unmount) intake.Dispose(); else fixture.Family = "retired-family";
    finish.SetResult();
    Check(!await dropping, "A retired handoff was reported as an accepted drop.");
    checks.Add(unmount ? "Unmount during queue handoff is retired" : "Account-family loss during queue handoff is retired");
}
var result = new { passed = true, scope = "native_file_drop_owner", checks, notTested = new[] { "physical shell drag/drop", "actual WinUI routed-event deferral", "real filesystem spooling and transport" } };
if (args.Length == 1)
{
    string output = Path.GetFullPath(args[0]); Directory.CreateDirectory(Path.GetDirectoryName(output)!);
    File.WriteAllText(output, JsonSerializer.Serialize(result, new JsonSerializerOptions { WriteIndented = true }));
}
Console.WriteLine($"PASS native file-drop owner: {checks.Count} admission, captured-target, rejection and lifetime checks.");

internal sealed record Target(string User, string Family, string Channel, string? Thread = null);
internal sealed class Fixture
{
    internal Target Target = new("A", "family-a", "first");
    internal string User = "A", Family = "family-a";
    internal bool Active = true, Access = true;
    internal int Captures;
    internal readonly List<(Target Target, IReadOnlyList<string> Paths)> Added = [];
    internal readonly List<(Target Target, string? Message)> Errors = [];
    internal NativeFileDropIntake<Target> Create() => new(
        () => { Captures++; return Active ? Target : null; },
        target => Active && Access && target.User == User && target.Family == Family,
        (target, paths) => { Added.Add((target, paths)); return Task.CompletedTask; },
        (target, message) => Errors.Add((target, message)));
}
