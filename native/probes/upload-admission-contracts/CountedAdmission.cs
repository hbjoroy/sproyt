using Sproyt.WindowsProbe;

internal static partial class Probe
{
    private static async Task CheckCountedAdmissionAsync()
    {
        var app = new LiveApplication();
        await app.OpenForProbeAsync();
        try
        {
            var root = app.TargetForProbe();
            var thread = root with { ParentMessageId = "55555555-5555-4555-8555-555555555555" };
            int accepted = await app.AddUploadFilesCountedAsync(thread, ["unsupported.docx"]);
            Check(accepted == 0 && app.JobsForProbe().Jobs.Count == 0 && app.IssueForProbe().Target == thread,
                "A rejected file was counted or its thread target was lost.");
            app.BeforeStageForProbe(path =>
            {
                if (path == "missing.txt") throw new FileNotFoundException();
            });
            accepted = await app.AddUploadFilesCountedAsync(thread, ["missing.txt"]);
            Check(accepted == 0 && app.IssueForProbe().Target == thread, "A spool failure acknowledged a file.");
            accepted = await app.AddUploadFilesCountedAsync(thread, ["first.txt", "missing.txt", "skipped.txt"]);
            Check(accepted == 1 && app.JobsForProbe().Jobs.Single().Target == thread && app.IssueForProbe().Target == thread,
                "A partial batch did not retain exactly its first admission and the captured failure target.");
            await app.AddUploadFilesAsync(root, ["another.txt"]);
            Check(app.IssueForProbe().Target == thread, "A successful root picker cleared the thread's error.");
            await app.AddUploadFilesAsync(thread, ["replacement.txt"]);
            Check(app.IssueForProbe().Message is null, "A success in the original thread did not clear its error.");

            // The family retires while staging the second file. The first
            // admission was already durable; further files must not be counted.
            app.BeforeStageForProbe(path =>
            {
                if (path == "switch.txt") app.ScopeForProbe(NativeAuthPhase.Active, User, "2:another-family:1");
            });
            accepted = await app.AddUploadFilesCountedAsync(root, ["before-switch.txt", "switch.txt", "skipped.txt"]);
            Check(accepted == 1 && await app.AddUploadFilesCountedAsync(root, ["stale.txt"]) == 0,
                "Family change lost a confirmed admission or accepted a stale file.");
        }
        finally { await app.CloseForProbeAsync(); }

        app = new LiveApplication();
        await app.OpenForProbeAsync();
        try
        {
            var target = app.TargetForProbe();
            app.FailJournalForProbe(true);
            int accepted = await app.AddUploadFilesCountedAsync(target, ["unconfirmed.txt"]);
            var state = app.JobsForProbe();
            Check(accepted == 0 && state.StorageError is not null && state.Jobs.Count == 1 && app.IssueForProbe().Target == target,
                "An in-memory queue entry was counted as a successful journal write.");
            app.ScopeForProbe(NativeAuthPhase.Active, User, "2:new-login:1");
            Check(app.IssueForProbe() == (null, null), "A scoped file error crossed into a new family.");
        }
        finally { app.FailJournalForProbe(false); await app.CloseForProbeAsync(); }
    }
}
