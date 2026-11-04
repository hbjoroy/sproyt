using System.Text.Json;
using Sproyt.WindowsProbe;

if (args.Length != 1 || !Path.IsPathFullyQualified(args[0])) throw new ArgumentException("Supply an absolute evidence directory.");
var checks = new List<string>();
void Check(bool value, string message) { if (!value) throw new InvalidOperationException(message); }

var app = new LiveApplication();
var first = new NativePersonalScope("A", "12:family:2", 1);
app.BindForProbe("B", "A", first.SessionStamp);
app.CoverConversation(first);
Check(!app.CoveredForProbe, "A mismatched workspace owner covered the conversation.");
app.BindForProbe("A", "A", first.SessionStamp);
app.CoverConversation(first with { UserId = "B", Visit = 300 });
app.CoverConversation(first with { SessionStamp = "12:family:1", Visit = 301 });
Check(!app.CoveredForProbe, "A foreign account or stale full stamp established a cover.");
app.CoverConversation(first);
Check(app.CoveredForProbe, "The current exact account/stamp was not admitted after stale callers.");
checks.Add("cover requires current workspace, account and full stamp");

string rotatedStamp = "12:family:3";
app.BindForProbe("A", "A", rotatedStamp);
Check(app.CoveredForProbe, "Normal token rotation exposed the conversation before the UI could rebind.");
var rotated = new NativePersonalScope("A", rotatedStamp, 2);
app.CoverConversation(rotated);
app.UncoverConversation(first);
Check(app.CoveredForProbe, "A stale surface retired the rotated cover.");
app.UncoverConversation(rotated with { UserId = "B" });
Check(app.CoveredForProbe, "The wrong account cleared the current visit.");
app.UncoverConversation(rotated with { });
Check(!app.CoveredForProbe, "An equivalent copy of the active immutable scope could not close it.");
checks.Add("same-family rotation stays covered; stale removal ignored; equivalent current scope may clear");

var newer = rotated with { Visit = 3 };
app.CoverConversation(newer);
app.CoverConversation(rotated);
app.UncoverConversation(rotated);
Check(app.CoveredForProbe, "An older cover replaced and then cleared a newer one.");
app.UncoverConversation(newer);
app.CoverConversation(newer);
app.CoverConversation(rotated);
Check(!app.CoveredForProbe, "A callback remasked a closed visit using its full stamp.");
checks.Add("watermark prevents older or equal visits remasking after close");

var live = newer with { Visit = 4 };
app.CoverConversation(live);
app.BindForProbe("A", "A", "13:family:1");
Check(!app.CoveredForProbe, "A new session lifecycle reused a compatible-looking family prefix.");
app.BindForProbe("B", "B", live.SessionStamp);
Check(!app.CoveredForProbe, "The old account cover affected another account.");
app.BindForProbe(null, null, null);
Check(!app.CoveredForProbe, "Signed-out auth kept a cover.");
checks.Add("same-account relogin, account switch and sign-out invalidate old cover");

string run = Path.Combine(args[0], "personal-cover-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(run);
string receipt = Path.Combine(run, "receipt.json");
File.WriteAllText(receipt, JsonSerializer.Serialize(new { passed = true, scope = "native_personal_application_cover", linked_production_partial = true, synthetic_authority = true, checks }));
Console.WriteLine(receipt);

internal sealed class NativeMessage;
internal sealed class NativeChannel;
