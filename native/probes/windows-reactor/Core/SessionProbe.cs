namespace Sproyt.WindowsProbe;

// Real local persistence checks for the controller without starting the UI.
// The runner supplies a unique, test-owned path; no existing file is removed.
public static class SessionProbe
{
    public static async Task<string> RunAsync(string database, string libraryPath)
    {
        string path = Path.GetFullPath(database);
        if (File.Exists(path)) throw new InvalidOperationException("Session probe refuses to reuse an existing database.");
        const string firstChannel = "probe-general";
        const string secondChannel = "probe-team";
        const string firstBody = "Hei frå den lokale Windows-proben 👋";
        const string secondBody = "Eit anna fiktivt rom";
        string identity = $"windows-probe-{Guid.NewGuid():N}";

        await using (var initial = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false))
        {
            await Expect("store_busy", async () =>
            {
                await using var duplicateOwner = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false);
            }).ConfigureAwait(false);
            await initial.SaveDraftAsync(firstChannel, firstBody).ConfigureAwait(false);
            await initial.SaveDraftAsync(secondChannel, secondBody).ConfigureAwait(false);
            var saved = await initial.SnapshotAsync().ConfigureAwait(false);
            Check(saved.Drafts.Count == 2 && saved.Outbox.Count == 0, "Drafts were not saved independently.");
        }

        var session = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false);
        try
        {
            var reopened = await session.SnapshotAsync().ConfigureAwait(false);
            Check(reopened.Drafts.Single(item => item.Channel == firstChannel).Body == firstBody &&
                reopened.Drafts.Single(item => item.Channel == secondChannel).Body == secondBody, "Draft bodies did not survive close/reopen.");
            await Expect("text_too_large", () => session.SaveDraftAsync(firstChannel, new string('ø', 513))).ConfigureAwait(false);
            Check((await session.SnapshotAsync().ConfigureAwait(false)).Drafts.Single(item => item.Channel == firstChannel).Body == firstBody,
                "A rejected UTF-8 draft overwrote saved text.");

            var queued = await session.EnqueueAsync(firstChannel, firstBody, identity).ConfigureAwait(false);
            Check(queued == new QueuedMessage(identity, firstChannel, firstBody, "queued"), "Enqueue did not return the stored queued payload.");
            var afterEnqueue = await session.SnapshotAsync().ConfigureAwait(false);
            Check(afterEnqueue.Drafts.All(item => item.Channel != firstChannel) &&
                afterEnqueue.Drafts.Single(item => item.Channel == secondChannel).Body == secondBody, "Enqueue did not clear only its matching draft.");
            Check(await session.EnqueueAsync(firstChannel, firstBody, identity).ConfigureAwait(false) == queued,
                "An identical request ID did not return its original stored item.");
            await Expect("request_conflict", async () =>
            {
                await session.EnqueueAsync(firstChannel, "Changed payload", identity).ConfigureAwait(false);
            }).ConfigureAwait(false);
            Check((await session.SnapshotAsync().ConfigureAwait(false)).Outbox.Count == 1, "A repeated identity produced a duplicate outbox item.");

            for (int index = 1; index < 8; index++)
                await session.EnqueueAsync(firstChannel, $"Fiktiv kømelding {index}", $"{identity}-{index}").ConfigureAwait(false);
            const string retainedBody = "Ta vare på utkastet når køen er full";
            await session.SaveDraftAsync(secondChannel, retainedBody).ConfigureAwait(false);
            await Expect("capacity", async () =>
            {
                await session.EnqueueAsync(secondChannel, retainedBody, $"{identity}-overflow").ConfigureAwait(false);
            }).ConfigureAwait(false);
            var full = await session.SnapshotAsync().ConfigureAwait(false);
            Check(full.Outbox.Count == 8 && full.Outbox.All(item => item.State == "queued"), "Local requests changed state without delivery.");
            Check(full.Drafts.Single(item => item.Channel == secondChannel).Body == retainedBody, "A full outbox cleared its rejected draft.");

            // Admission closes synchronously, but already-admitted writes drain
            // before native close. A new request after close must be rejected.
            var finalDraft = session.SaveDraftAsync(secondChannel, retainedBody + " – lagra før lukking");
            var closing = session.DisposeAsync().AsTask();
            await finalDraft.ConfigureAwait(false);
            await closing.ConfigureAwait(false);
            try
            {
                await session.SnapshotAsync().ConfigureAwait(false);
                throw new InvalidOperationException("A disposed session admitted a new snapshot.");
            }
            catch (ObjectDisposedException) { }
        }
        finally
        {
            await session.DisposeAsync().ConfigureAwait(false);
        }

        await using (var restored = await NativeSession.OpenAsync(path, libraryPath).ConfigureAwait(false))
        {
            var snapshot = await restored.SnapshotAsync().ConfigureAwait(false);
            Check(snapshot.Outbox.Count == 8 && snapshot.Outbox.Count(item => item.RequestId == identity) == 1 &&
                snapshot.Outbox.Single(item => item.RequestId == identity).Body == firstBody, "Stable queued identities did not survive reopen.");
            Check(snapshot.Drafts.Single(item => item.Channel == secondChannel).Body.EndsWith("lagra før lukking", StringComparison.Ordinal),
                "An admitted draft was lost during close.");
        }
        return $"PASS Windows controller: exclusive owner, draft reopen, UTF-8 limit, atomic enqueue, stable identity, capacity and drain/close; database={path}";
    }

    private static async Task Expect(string code, Func<Task> operation)
    {
        try { await operation().ConfigureAwait(false); }
        catch (NativeProbeException exception) when (exception.Code == code) { return; }
        throw new InvalidOperationException($"Expected native error {code}.");
    }

    private static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }
}
