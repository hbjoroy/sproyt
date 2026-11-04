using System.Text.Json;
using Sproyt.WindowsProbe;

if (args.Length != 1 || !Path.IsPathFullyQualified(args[0])) throw new ArgumentException("Supply an absolute evidence directory.");
var controller = new NativePersonalSurfaceController();
var checks = new List<string>();
void Check(bool value, string message) { if (!value) throw new InvalidOperationException(message); }
NativePersonalScope Open(NativePersonalSurfaceKind kind) => controller.Open(kind) ?? throw new InvalidOperationException("Failed to open a bound surface.");

Check(!controller.Bind(null, null) && controller.Open(NativePersonalSurfaceKind.People) is null, "Signed-out surface opened.");
Check(controller.Bind("A", "1:family-a:1") && controller.Scope is null, "Binding an account opened a surface.");
var first = Open(NativePersonalSurfaceKind.People);
Check(!controller.Bind("A", "1:family-a:1") && controller.Current(first), "Repeated binding invalidated the visit.");
var inbox = Open(NativePersonalSurfaceKind.Mentions);
Check(inbox.Visit > first.Visit && !controller.Current(first) && controller.Current(inbox), "Changing surfaces reused a visit.");
checks.Add("signed-out admission, unchanged binding and fresh per-surface visits");

Check(!controller.Bind("A", "1:family-a:2") && controller.Kind == NativePersonalSurfaceKind.Mentions, "Token rotation closed the user's surface.");
var rotated = controller.Scope!;
Check(rotated.SessionStamp == "1:family-a:2" && rotated.Visit > inbox.Visit && !controller.Current(inbox), "Rotation admitted the old stamp/visit.");
Check(controller.Current(rotated) && !controller.Current(rotated with { UserId = "B" }), "A foreign user matched the current visit.");
checks.Add("same-family rotation preserves the surface and invalidates old callbacks");

controller.Close();
Check(controller.Kind is null && !controller.Current(rotated), "Closing left an active visit.");
var profile = Open(NativePersonalSurfaceKind.Profile);
Check(profile.Visit > rotated.Visit && !controller.Current(rotated), "Close/reopen resurrected a callback.");
Check(controller.Bind("A", "1:family-b:1") && controller.Scope is null && controller.Kind is null, "Relogin to the same account retained the old visit.");
var relogin = Open(NativePersonalSurfaceKind.Tasks);
Check(controller.Bind("B", "1:family-b:1") && !controller.Current(relogin), "A foreign account reused a family with the same spelling.");
var secondUser = Open(NativePersonalSurfaceKind.Unread);
Check(controller.Bind("A", "1:family-a:1") && !controller.Current(secondUser), "A to B to A reopened B.");
var returned = Open(NativePersonalSurfaceKind.People);
Check(returned.Visit > secondUser.Visit && !controller.Current(first), "A restored stamp reused the original A visit.");
checks.Add("close, same-account relogin and A to B to A discard the previous visit");

Check(controller.Bind(null, null) && !controller.Current(returned), "Logout retained old callbacks.");
foreach (string? badStamp in new string?[] { null, "", "no-separator", ":1", "family:" })
    Check(!controller.Bind("A", badStamp) && controller.Open(NativePersonalSurfaceKind.Profile) is null, "Incomplete auth admitted a personal surface.");
Check(!controller.Bind(null, "1:family-a:1") && controller.Scope is null, "Missing account admitted a surface.");
Check(controller.Bind("A", "1:family-a:1") && Open(NativePersonalSurfaceKind.People).Visit > returned.Visit, "Logout reset the monotonic visit counter.");
checks.Add("missing or incomplete credentials close surfaces without resetting the visit sequence");

var replacement = new NativePersonalSurfaceController();
replacement.Bind("A", "1:family-a:1");
var retired = controller.Scope!;
var mounted = replacement.Open(NativePersonalSurfaceKind.People)!;
Check(mounted.Visit > retired.Visit && mounted.UserId == retired.UserId && mounted.SessionStamp == retired.SessionStamp,
    "Recreating the component reused a visit under the same account and stamp.");
Check(!replacement.Current(retired with { }) && !controller.Current(mounted) && replacement.Current(mounted with { }),
    "A stale callback from the retired controller matched the replacement's scope.");
checks.Add("replacement controller under identical auth rejects old value-equal scope snapshots");

string run = Path.Combine(args[0], "personal-surface-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(run);
string receipt = Path.Combine(run, "receipt.json");
File.WriteAllText(receipt, JsonSerializer.Serialize(new { passed = true, scope = "native_personal_surface_owner", checks }));
Console.WriteLine(receipt);

// The command interface's two unrelated DTO dependencies are never instantiated.
internal sealed class NativeMessage;
internal sealed class NativeChannel;
