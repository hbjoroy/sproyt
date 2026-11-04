using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Sproyt.WindowsProbe;

internal static partial class Program
{
    internal const string User = "00000000-0000-0000-0000-000000000001", Channel = "00000000-0000-0000-0000-000000000002",
        Parent = "00000000-0000-0000-0000-000000000003", Media = "00000000-0000-0000-0000-000000000004";
    private static readonly NativeUploadTarget Target = new(User, "1:family-a", Channel, Parent);
    private static int _checks;
    private static async Task<int> Main(string[] args)
    {
        try
        {
            Require(args.Length == 1 && Path.IsPathFullyQualified(args[0]), "Supply a test-owned absolute artifact directory.");
            Directory.CreateDirectory(args[0]);
            await Check("lost create/chunk/complete receipts keep exact identity and attach once", async () =>
            {
                var local = new Local(); var source = local.Source(NativeUploadHttpClient.ChunkBytes * 2 + 17);
                var wire = new Wire(local) { LoseCreate = true, LoseChunk = true, LoseComplete = true };
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope);
                string id = await queue.EnqueueAsync(Target, source); await Until(() => local.Attached.ContainsKey(id) && queue.Capture().Jobs.Count == 0);
                Require(wire.Creates == 2 && wire.Completes == 2 && wire.UniqueChunks == 3 && wire.ChunkCalls == 4 && local.Attached.Count == 1,
                    "Uncertain retry duplicated a chunk/attachment or changed upload identity.");
                Require(wire.FirstDispatchWasJournaled && wire.MaximumChunk <= NativeUploadHttpClient.ChunkBytes && local.MaximumRead <= NativeUploadHttpClient.ChunkBytes,
                    "Dispatch preceded journaling or used an unbounded client buffer.");
                Require(local.Attached[id].Target == Target, "Completion moved to another channel/thread.");
            });
            await Check("post-commit journal failure stops network advancement", async () =>
            {
                var local = new Local { FailSave = jobs => jobs.Any(job => job.UploadId is not null && job.UploadedBytes == 0) };
                var source = local.Source(29); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await Until(() => queue.Capture().StorageError is not null); await Task.Delay(50);
                Require(wire.Creates == 1 && wire.ChunkCalls == 0, "Storage failure did not stop dispatch.");
                await queue.RetryAsync(id); await Until(() => local.Attached.ContainsKey(id) && queue.Capture().Jobs.Count == 0);
                Require(wire.Creates == 1 && wire.Statuses == 1, "Storage retry forgot its received create receipt.");
            });
            await Check("lost attachment commit recovers without upload or draft duplication", async () =>
            {
                var local = new Local { LoseAttachAfterCommit = true }; var source = local.Source(31); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await Until(() => queue.Capture().StorageError is not null);
                Require(local.Attached.Count == 1 && wire.Completes == 1, "Injected lost attachment result was not after commit.");
                await queue.CancelAsync(id);
                Require(queue.Capture().Jobs.Single() is { State: NativeUploadState.Ready, HandoffStarted: true }, "Cancel discarded an unknown committed attachment receipt.");
                await queue.RetryAsync(id); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.Attached.Count == 1 && local.AttachCalls == 2 && wire.Completes == 1 && wire.Creates == 1, "Attachment retry repeated upload or merge.");
            });
            await Check("cancel cannot interrupt an already admitted atomic draft handoff", async () =>
            {
                var local = new Local { HoldAttach = true }; var source = local.Source(17); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await local.AttachEntered.Task.WaitAsync(TimeSpan.FromSeconds(5)); await queue.CancelAsync(id);
                Require(queue.Capture().Jobs.Single() is { State: NativeUploadState.Ready, HandoffStarted: true }, "Cancel crossed the durable handoff admission boundary.");
                local.ReleaseAttach.TrySetResult(); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.Attached.Count == 1, "An admitted handoff was lost.");
            });
            await Check("ambiguous enqueue retains its source independently of logout UI projection", async () =>
            {
                var local = new Local { FailSaveBefore = true }; var source = local.Source(11); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope);
                await Throws<IOException>(() => queue.EnqueueAsync(Target, source)); string id = queue.Capture().Jobs.Single().Id;
                queue.BindScope(null);
                Require(queue.Capture().Jobs.Count == 0 && queue.RetainsSource(source.SourceId) && wire.Creates == 0, "Logout hid a retained source from staging cleanup or dispatched before commit.");
                queue.BindScope(Target.Scope); await queue.RetryAsync(id); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.Attached.Count == 1, "Retained enqueue could not resume after explicit storage retry.");
            });
            await Check("late authoritative channel admission wakes a same-family restored queue", async () =>
            {
                var local = new Local(); var source = local.Source(13); var wire = new Wire(local); bool admitted = false;
                local.Saved = [new(Guid.NewGuid().ToString("D"), Target, source, Guid.NewGuid().ToString("D"))];
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, _ => admitted, new ImmediateClock());
                queue.BindScope(Target.Scope); await Task.Delay(50); Require(wire.Creates == 0, "Missing authoritative channel admission dispatched an upload.");
                admitted = true; queue.BindScope(Target.Scope); queue.RecheckAdmission(); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.Attached.Count == 1, "Same-family restored queue stayed asleep after admission became ready.");
            });
            await Check("cancel claim beats handoff atomically and survives caller cancellation", async () =>
            {
                var local = new Local(); var source = local.Source(17); var wire = new Wire(local);
                NativeUploadQueue? queue = null; Task? cancelling = null; int readyAdmissions = 0; using var caller = new CancellationTokenSource();
                queue = await NativeUploadQueue.OpenAsync(local, wire, target =>
                {
                    if (local.Saved.SingleOrDefault() is { State: NativeUploadState.Ready, HandoffStarted: false } ready && ++readyAdmissions == 2)
                    { cancelling = queue!.CancelAsync(ready.Id, caller.Token); caller.Cancel(); }
                    return target == Target;
                }, new ImmediateClock());
                await using (queue)
                {
                    queue.BindScope(Target.Scope); await queue.EnqueueAsync(Target, source);
                    await Until(() => queue.Capture().Jobs.SingleOrDefault()?.State == NativeUploadState.Cancelled);
                    await cancelling!.WaitAsync(TimeSpan.FromSeconds(5));
                    Require(local.AttachCalls == 0 && local.Saved.Single().State == NativeUploadState.Cancelled, "A cancellation claim was abandoned or crossed the handoff boundary.");
                }
            });
            await Check("a revoked first channel cannot starve another admitted upload", async () =>
            {
                var local = new Local(); var source = local.Source(17); var wire = new Wire(local);
                var revoked = Target with { ChannelId = "00000000-0000-0000-0000-000000000088" };
                local.Saved = [new(Guid.NewGuid().ToString("D"), revoked, source, Guid.NewGuid().ToString("D")), new(Guid.NewGuid().ToString("D"), Target, source, Guid.NewGuid().ToString("D"))];
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, target => target == Target, new ImmediateClock()); queue.BindScope(Target.Scope);
                await Until(() => local.Attached.Count == 1);
                Require(queue.Capture().Jobs.All(job => job.Target == revoked) && wire.Creates == 1, "A revoked first job blocked or dispatched ahead of an authorized target.");
            });
            await Check("offline state cannot remove an uncertain attachment handoff", async () =>
            {
                var local = new Local { LoseAttachAfterCommit = true }; var source = local.Source(17); var wire = new Wire(local); bool admitted = true;
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, _ => admitted, new ImmediateClock()); queue.BindScope(Target.Scope);
                string id = await queue.EnqueueAsync(Target, source); await Until(() => queue.Capture().StorageError is not null); admitted = false;
                await Throws<InvalidOperationException>(() => queue.RemoveAsync(id));
                Require(queue.Capture().Jobs.Single().HandoffStarted, "Temporary offline state retired an unknown draft receipt.");
                admitted = true; await queue.RetryAsync(id); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.Attached.Count == 1, "Offline recovery duplicated the handoff.");
            });
            await Check("enqueue rechecks account admission after the journal gate", async () =>
            {
                var local = new Local { HoldSave = true }; var first = local.Source(17); var second = local.Source(19); var wire = new Wire(local); bool admitted = true;
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, _ => admitted, new ImmediateClock()); queue.BindScope(Target.Scope);
                var enqueueFirst = queue.EnqueueAsync(Target, first); await local.SaveEntered.Task.WaitAsync(TimeSpan.FromSeconds(5));
                var enqueueSecond = queue.EnqueueAsync(Target, second); admitted = false; local.ReleaseSave.TrySetResult();
                await enqueueFirst; await Throws<OperationCanceledException>(() => enqueueSecond);
                Require(queue.Capture().Jobs.Count == 1 && !queue.RetainsSource(second.SourceId) && wire.Creates == 0, "A queued old-account admission persisted or dispatched after the gate wait.");
            });
            await Check("stale scope registration cannot override a newer account binding", async () =>
            {
                var local = new Local(); var source = local.Source(17); var wire = new Wire(local);
                local.Saved = [new(Guid.NewGuid().ToString("D"), Target, source, Guid.NewGuid().ToString("D"))];
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, _ => false, new ImmediateClock());
                object registrationA = new(), registrationB = new(), current = registrationA;
                queue.BindScope(Target.Scope, () => ReferenceEquals(current, registrationA));
                current = registrationB; var scopeB = new NativeMediaScope("00000000-0000-0000-0000-000000000099", "2:family-b");
                queue.BindScope(scopeB, () => ReferenceEquals(current, registrationB));
                queue.BindScope(Target.Scope, () => ReferenceEquals(current, registrationA));
                Require(queue.Capture().Jobs.Count == 0 && queue.Capture().ActiveFamilyStamp == scopeB.FamilyStamp, "A late old-account registration exposed old filenames or replaced the new scope.");
            });
            await Check("failed final queue removal retains its storage-retry handle", async () =>
            {
                var local = new Local { FailSave = jobs => jobs.Count == 0 }; var source = local.Source(23); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await Until(() => queue.Capture().StorageError is not null);
                Require(queue.Capture().Jobs.Single().State == NativeUploadState.Attached, "Removal lost the only retry handle.");
                await queue.RetryAsync(id); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(local.AttachCalls == 1 && wire.Completes == 1, "Final removal repeated completed work.");
            });
            await Check("draft capacity rejection keeps ready media without an automatic retry loop", async () =>
            {
                var local = new Local { AllowAttach = false }; var source = local.Source(19); var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await Until(() => queue.Capture().Jobs.SingleOrDefault() is { State: NativeUploadState.Ready, Error: not null });
                await Task.Delay(50); Require(local.AttachCalls == 1 && wire.Completes == 1, "Ready-capacity error retried itself.");
                local.AllowAttach = true; await queue.RetryAsync(id); await Until(() => queue.Capture().Jobs.Count == 0);
                Require(wire.Completes == 1 && local.Attached.Count == 1, "Ready retry uploaded again.");
            });
            await Check("cancel interrupts blocked transfer and rejects its late receipt", async () =>
            {
                var local = new Local(); var source = local.Source(71); var wire = new Wire(local) { HoldChunk = true };
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); string id = await queue.EnqueueAsync(Target, source);
                await wire.ChunkEntered.Task.WaitAsync(TimeSpan.FromSeconds(5)); await queue.CancelAsync(id); wire.ReleaseChunk.TrySetResult();
                await Until(() => !queue.Capture().Busy);
                Require(queue.Capture().Jobs.Single().State == NativeUploadState.Cancelled && local.Attached.Count == 0 && wire.Aborts == 1, "Cancelled upload attached or missed its scoped abort.");
                await queue.RemoveAsync(id); Require(queue.Capture().Jobs.Count == 0, "Cancelled slot was not removable.");
            });
            await Check("held same-account jobs remain visible and removable while other users see no metadata", async () =>
            {
                var local = new Local(); var source = local.Source(61); var wire = new Wire(local) { HoldChunk = true }; NativeMediaScope? current = Target.Scope;
                await using var queue = await NativeUploadQueue.OpenAsync(local, wire, target => target.Scope == current, new ImmediateClock()); queue.BindScope(current);
                for (int index = 0; index < NativeUploadQueue.MaximumJobs; index++) await queue.EnqueueAsync(Target, source);
                await Throws<InvalidOperationException>(() => queue.EnqueueAsync(Target, source));
                current = new(User, "2:family-b"); queue.BindScope(current); await Until(() => !queue.Capture().Busy);
                Require(queue.Capture().Jobs.Count == 8 && queue.Capture().Jobs.All(job => job.Target.FamilyStamp != queue.Capture().ActiveFamilyStamp), "Held jobs consumed invisible capacity.");
                await Throws<OperationCanceledException>(() => queue.RetryAsync(queue.Capture().Jobs[0].Id));
                await queue.RemoveAsync(queue.Capture().Jobs[0].Id); Require(queue.Capture().Jobs.Count == 7, "Held upload could not be removed.");
                current = new("00000000-0000-0000-0000-000000000099", "1:other-family"); queue.BindScope(current);
                Require(queue.Capture().Jobs.Count == 0 && local.Attached.Count == 0, "Another user saw filenames or received an attachment.");
            });
            await Check("restart resumes the exact persisted server cursor", async () =>
            {
                var local = new Local(); var source = local.Source(NativeUploadHttpClient.ChunkBytes + 13); var wire = new Wire(local);
                var job = new NativeUploadJob(Guid.NewGuid().ToString("D"), Target, source, Guid.NewGuid().ToString("D")); local.Saved = [job];
                var created = await wire.CreateAsync(job, default); job = job with { UploadId = created.Id };
                byte[] first = local.Bytes[source.SourceId][..NativeUploadHttpClient.ChunkBytes];
                var status = await wire.PutChunkAsync(job, first, first.Length, Convert.ToHexStringLower(SHA256.HashData(first)), default);
                local.Saved = [job with { NextChunk = status.NextChunk, UploadedBytes = status.ReceivedBytes, State = NativeUploadState.Uploading }];
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); await Until(() => local.Attached.Count == 1 && queue.Capture().Jobs.Count == 0);
                Require(wire.Creates == 1 && wire.Statuses == 1 && wire.UniqueChunks == 2, "Restart changed identity or resent acknowledged prefix.");
            });
            await Check("changed source bytes cannot become an attached marker", async () =>
            {
                var local = new Local(); var source = local.Source(37) with { Sha256 = new string('0', 64) }; var wire = new Wire(local);
                await using var queue = await Open(local, wire); queue.BindScope(Target.Scope); await queue.EnqueueAsync(Target, source);
                await Until(() => queue.Capture().Jobs.SingleOrDefault()?.State == NativeUploadState.Failed);
                Require(local.Attached.Count == 0, "A different source fingerprint entered the draft.");
            });
            await Check("HTTP paths, chunks, hashes and response projections are bounded", HttpContracts);
            bool realOwner = OperatingSystem.IsWindows();
            if (realOwner) await Check("production session allowlist account/bearer rotation and logout fences", () => AuthContracts(args[0]));
            await Check("encoded marker names round-trip and stay within composer capacity", () =>
            {
                const string name = "blå%20 | [foto].png"; var attachment = new NativeUploadedAttachment(Media, name, "image/png", 1, new string('a', 64));
                var parsed = NativeMessageContent.Parse(attachment.Marker).Single(part => part.Kind == "media");
                Require(parsed.Text == name && parsed.MediaId == Media, "Filename was decoded twice or split into another marker field.");
                string large = (attachment with { FileName = string.Concat(Enumerable.Repeat("🦊", 255)) }).Marker;
                Require(Encoding.UTF8.GetByteCount(large) <= 1024 && NativeMessageContent.Parse(large).Count(part => part.Kind == "media") == 1, "Encoded Unicode label cannot fit an empty composer.");
                return Task.CompletedTask;
            });
            File.WriteAllText(Path.Combine(args[0], "receipt.json"), JsonSerializer.Serialize(new { passed = true, checks = _checks, productionQueue = true,
                productionHttpAdapter = true, realWindowsSessionOwner = realOwner, syntheticHttpServer = true, actualCanary = false, actualDraftStore = false,
                maximumJobs = NativeUploadQueue.MaximumJobs, chunkBytes = NativeUploadHttpClient.ChunkBytes, responseBytes = NativeUploadHttpClient.MaximumReplyBytes,
                uploadDoesNotSendChat = true }, new JsonSerializerOptions { WriteIndented = true }));
            Console.WriteLine($"PASS {_checks} native upload contracts."); return 0;
        }
        catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
    private static Task<NativeUploadQueue> Open(Local local, Wire wire) => NativeUploadQueue.OpenAsync(local, wire, target => target == Target, new ImmediateClock());
    private static async Task Check(string name, Func<Task> test) { await test(); _checks++; Console.WriteLine("PASS " + name); }
    internal static void Require(bool value, string message) { if (!value) throw new InvalidOperationException(message); }
    private static async Task Until(Func<bool> condition)
    { for (int attempt = 0; attempt < 500; attempt++) { if (condition()) return; await Task.Delay(10); } throw new InvalidOperationException("Upload contract did not settle."); }
    private static async Task Throws<T>(Func<Task> action) where T : Exception
    { try { await action(); } catch (T) { return; } throw new InvalidOperationException("Expected " + typeof(T).Name); }
}
