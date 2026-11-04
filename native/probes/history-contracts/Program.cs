using System.Diagnostics;
using System.Text.Json;
using Sproyt.WindowsProbe;

internal static class Program
{
    private const string Stamp = "synthetic:family:0";
    private static void Check(bool value, [System.Runtime.CompilerServices.CallerLineNumber] int line = 0)
    { if (!value) throw new InvalidOperationException("History contract assertion failed at line " + line); }
    private static async Task Until(Func<bool> condition)
    {
        var time = Stopwatch.StartNew();
        while (!condition()) { Check(time.Elapsed < TimeSpan.FromSeconds(8)); await Task.Delay(10); }
    }
    private static Task Older(NativeChatClient client) => client.LoadOlderAsync(HistoryServer.A, client.Snapshot.History!.ViewGeneration, HistoryServer.User, Stamp);
    private static Task Read(NativeChatClient client, params string[] ids) => client.MarkVisibleReadAsync(HistoryServer.A, client.Snapshot.History!.ViewGeneration, ids, HistoryServer.User, Stamp);
    public static async Task<int> Main(string[] args)
    {
        try
        {
            Check(args.Length == 1);
            await PagingReadingAndPolicy(); await DelayedViewsAndMutations(); await CapacityAndAccess(); await LargeBodiesAndRevokedPage();
            await QueuedRepairCannotOverrideSelection(); await DeniedReadingClearsOnlyCurrentView(); await DisconnectFencesCompletedOldPage();
            await QueuedSelectionRetainsOriginalAccount();
            Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(args[0]))!);
            await File.WriteAllTextAsync(args[0], JsonSerializer.Serialize(new {
                passed = true, production_transport_source = true, local_websocket = true, synthetic_session_owner = true,
                live_canary = false, actual_credentials = false, oldest_cursor_includes_replies = true, roots_only = true,
                paging_retry_same_cursor = true, malformed_page_atomic = true, stable_ids_sequences = true,
                no_optimistic_read = true, measured_ids_only = true, inactive_offscreen_restore_read_suppressed = true,
                late_page_view_fence = true, account_revocation_fence = true, permission_loss_clears_projection = true, large_legal_messages = true,
                queued_repair_preserves_user_selection = true, read_denial_clears_current_only = true, disconnect_fences_completed_page = true,
                queued_selection_original_account = true,
                stale_history_cannot_resurrect = true, bounded_history_no_silent_eviction = true,
                maximum_roots = NativeChatClient.MaximumHistoryMessages, page_size = NativeChatClient.HistoryPageSize,
                architecture = System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture.ToString(),
            }, new JsonSerializerOptions { WriteIndented = true }));
            Console.WriteLine("PASS native history and visible-read contracts"); return 0;
        }
        catch (Exception failure) { Console.Error.WriteLine(failure); return 1; }
    }
    private static async Task PagingReadingAndPolicy()
    {
        await using var server = await HistoryServer.StartAsync();
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        var initial = client.Snapshot;
        Check(initial.Messages.Count == 50 && initial.Messages[0].Sequence == 106 && initial.History!.HasOlder);
        Check(initial.History!.OldestSequence == 106 && initial.Channels.Single(c => c.Id == HistoryServer.A).LastReadSequence == 0 && server.Reads.IsEmpty);
        Check(initial.Messages.All(m => m.ParentMessageId is null));
        string[] measured = [HistoryServer.Id(110), HistoryServer.Id(120), HistoryServer.Id(999), "pending:local"];
        foreach (var (active, visible, restoring) in new[] { (false, true, false), (true, false, false), (true, true, true) })
            Check(ConversationHistoryPolicy.VisibleRead(initial, active, visible, restoring, measured) is null);
        var report = ConversationHistoryPolicy.VisibleRead(initial, true, true, false, measured)!;
        Check(report.MessageIds.Count == 2 && report.ViewGeneration == initial.History.ViewGeneration);
        await Read(client); await Read(client, HistoryServer.Id(999), "pending:local"); Check(server.Reads.IsEmpty);
        server.HoldNextRead = true; var reading = Read(client, report.MessageIds.ToArray());
        await server.ReadHeld.Task.WaitAsync(TimeSpan.FromSeconds(8));
        Check(client.Snapshot.Channels.Single(c => c.Id == HistoryServer.A).LastReadSequence == 0);
        server.ReadRelease.SetResult(); await reading;
        Check(server.Reads.Single() == 120 && client.Snapshot.Channels.Single(c => c.Id == HistoryServer.A).LastReadSequence == 120);
        Check(client.Snapshot.History!.ViewGeneration == initial.History.ViewGeneration);
        await Read(client, HistoryServer.Id(110)); Check(server.Reads.Count == 1);
        await server.ReadEvent(150, "other-user"); await Task.Delay(30);
        Check(client.Snapshot.Channels.Single(c => c.Id == HistoryServer.A).LastReadSequence == 120);
        await server.ReadEvent(130); await Until(() => client.Snapshot.Channels.Single(c => c.Id == HistoryServer.A).LastReadSequence == 130);
        Check(client.Snapshot.History!.ViewGeneration == initial.History.ViewGeneration);

        server.FailNext = true; await Older(client);
        Check(client.Snapshot.History!.Error is not null && !client.Snapshot.History.LoadingOlder && client.Snapshot.History.OldestSequence == 106 && client.Snapshot.Messages.Count == 50);
        await Older(client);
        Check(server.Pages.ToArray()[^2].Before == 106 && server.Pages.ToArray()[^1].Before == 106);
        Check(client.Snapshot.Messages.Count == 50 && client.Snapshot.History!.OldestSequence == 56 && client.Snapshot.History.HasOlder);
        server.MalformedNext = true; await Older(client);
        Check(client.Snapshot.History!.OldestSequence == 56 && client.Snapshot.Messages.Count == 50 && client.Snapshot.History.Error is not null);
        await Older(client);
        Check(client.Snapshot.History!.OldestSequence == 6 && client.Snapshot.Messages.Count == 100);
        await Older(client);
        var complete = client.Snapshot;
        Check(complete.Messages.Count == 105 && complete.History!.OldestSequence == 1 && !complete.History.HasOlder);
        Check(complete.Messages.Select(m => m.Id).Distinct().Count() == complete.Messages.Count && complete.Messages.Zip(complete.Messages.Skip(1), (a, b) => a.Sequence < b.Sequence).All(v => v));
        int pages = server.Pages.Count; await Older(client); Check(server.Pages.Count == pages);
        Check(complete.Messages.Any(m => m.Id == initial.Messages[0].Id && m.Sequence == initial.Messages[0].Sequence));
    }
    private static async Task DelayedViewsAndMutations()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        server.HoldNextPage = true; var page = Older(client); await Until(() => server.Held.Task.IsCompleted);
        int requests = server.Pages.Count; await Older(client); Check(server.Pages.Count == requests && client.Snapshot.History!.LoadingOlder);
        await server.EditEvent(60, false); await server.EditEvent(61, true);
        await Until(() => client.Snapshot.Messages.Any(m => m.Id == HistoryServer.Id(61) && m.DeletedAt is not null));
        server.Release.TrySetResult(); await page;
        Check(client.Snapshot.Messages.Single(m => m.Id == HistoryServer.Id(60)).Body == "Edited body");
        Check(client.Snapshot.Messages.Single(m => m.Id == HistoryServer.Id(61)).DeletedAt is not null);

        await client.SelectChannelAsync(HistoryServer.A); long old = client.Snapshot.History!.ViewGeneration;
        server.HoldNextPage = true; page = Older(client); await Until(() => server.Held.Task.IsCompleted);
        await client.SelectChannelAsync(HistoryServer.B); await client.SelectChannelAsync(HistoryServer.A);
        Check(client.Snapshot.History!.ViewGeneration != old);
        server.Release.TrySetResult(); await page;
        Check(client.Snapshot.Messages.Count == 50 && client.Snapshot.Messages.All(m => m.Sequence >= 106));
        int reads = server.Reads.Count;
        await client.MarkVisibleReadAsync(HistoryServer.A, old, [HistoryServer.Id(110)], HistoryServer.User, Stamp);
        Check(server.Reads.Count == reads);

        Task? switched = null; int viewAdmissions = 0;
        owner.BeforeAdmission = () => { if (++viewAdmissions == 3) switched = client.SelectChannelAsync(HistoryServer.B); };
        try { await Read(client, HistoryServer.Id(110)); } catch (NativeCommandException) { } catch (InvalidOperationException) { }
        owner.BeforeAdmission = null; await switched!;
        Check(server.Reads.Count == reads && client.Snapshot.SelectedChannel == HistoryServer.B);
        await client.SelectChannelAsync(HistoryServer.A);

        // Revoke exactly before the captured socket write, after finding the
        // visible IDs. An old viewport must not send under a changed account.
        int admissions = 0; owner.BeforeAdmission = () => { if (++admissions == 3) owner.RequireAuthentication(); };
        try { await Read(client, HistoryServer.Id(110)); } catch (NativeCommandException) { } catch (InvalidOperationException) { }
        await Until(() => client.Snapshot.Messages.Count == 0 && !client.Snapshot.Connected);
        Check(server.Reads.Count == reads && client.Snapshot.History!.ChannelId is null);
    }
    private static async Task LargeBodiesAndRevokedPage()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false; server.Total = 3;
        server.Body = _ => "x" + new string('\u0001', 65535);
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        Check(client.Snapshot.Messages.Count == 3 && client.Snapshot.Messages.All(m => System.Text.Encoding.UTF8.GetByteCount(m.Body) == 65536));
        server.Total = 155; server.Body = n => $"Message {n}"; await client.SelectChannelAsync(HistoryServer.A);
        server.HoldNextPage = true; var page = Older(client); await Until(() => server.Held.Task.IsCompleted);
        owner.RequireAuthentication(); server.Release.TrySetResult(); await page;
        Check(client.Snapshot.Messages.Count == 0 && client.Snapshot.Channels.Count == 0 && client.Snapshot.History!.ChannelId is null);
    }
    private static async Task QueuedRepairCannotOverrideSelection()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        // Hold the selection semaphore with a real current refresh, queue the
        // user's next intent first, then the captured background repair behind it.
        server.HoldNextPage = true; var heldSelection = client.SelectChannelAsync(HistoryServer.A);
        await Until(() => server.Held.Task.IsCompleted);
        long captured = client.Snapshot.History!.ViewGeneration;
        var userSelection = client.SelectChannelAsync(HistoryServer.B);
        var staleRepair = client.RepairHistoryAsync(HistoryServer.A, captured);
        server.Release.TrySetResult(); await Task.WhenAll(heldSelection, userSelection, staleRepair);
        Check(client.Snapshot.SelectedChannel == HistoryServer.B && !client.Snapshot.Loading);
        Check(server.Pages.Last().Channel == HistoryServer.B);
        // The same channel coming back later is still a different visit.
        await client.SelectChannelAsync(HistoryServer.A);
        long revisit = client.Snapshot.History!.ViewGeneration; int count = server.Pages.Count;
        await client.RepairHistoryAsync(HistoryServer.A, captured);
        Check(server.Pages.Count == count && client.Snapshot.History!.ViewGeneration == revisit);
    }
    private static async Task DeniedReadingClearsOnlyCurrentView()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        server.HoldNextRead = server.DenyNextRead = true;
        var oldRead = Read(client, HistoryServer.Id(110));
        await server.ReadHeld.Task.WaitAsync(TimeSpan.FromSeconds(8));
        await client.SelectChannelAsync(HistoryServer.B); long current = client.Snapshot.History!.ViewGeneration;
        server.ReadRelease.TrySetResult();
        try { await oldRead; } catch (NativeCommandException) { }
        Check(client.Snapshot.SelectedChannel == HistoryServer.B && client.Snapshot.Messages.Count > 0 && client.Snapshot.History!.ViewGeneration == current && client.Snapshot.Error is null);
        server.DenyNextRead = true;
        try { await client.MarkVisibleReadAsync(HistoryServer.B, current, [HistoryServer.Id(110, HistoryServer.B)], HistoryServer.User, Stamp); }
        catch (NativeCommandException) { }
        Check(client.Snapshot.SelectedChannel is null && client.Snapshot.Messages.Count == 0 && client.Snapshot.Channels.All(c => c.Id != HistoryServer.B));
    }
    private static async Task DisconnectFencesCompletedOldPage()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        using var releaseMerge = new ManualResetEventSlim();
        var mergeReached = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int admissions = 0;
        owner.BeforeAdmission = () => {
            if (Interlocked.Increment(ref admissions) == 5)
            { mergeReached.SetResult(); Check(releaseMerge.Wait(TimeSpan.FromSeconds(10))); }
        };
        long oldGeneration = client.Snapshot.History!.ViewGeneration;
        var oldPage = Older(client);
        try
        {
            await mergeReached.Task.WaitAsync(TimeSpan.FromSeconds(8));
            // The old response is already fully received. Pause before merging,
            // disconnect, then hold the next hello after connected becomes true.
            server.HoldNextHello = true; server.AbortSocket();
            await server.HelloHeld.Task.WaitAsync(TimeSpan.FromSeconds(8));
            Check(client.Snapshot.Connected && client.Snapshot.Loading);
            Check(client.Snapshot.History!.ViewGeneration != oldGeneration);
            releaseMerge.Set(); await oldPage;
            Check(client.Snapshot.Messages.Count == 50 && client.Snapshot.Messages.All(m => m.Sequence >= 106));
        }
        finally { releaseMerge.Set(); server.HelloRelease.TrySetResult(); owner.BeforeAdmission = null; }
        await Until(() => client.Snapshot.Connected && !client.Snapshot.Loading);
    }
    private static async Task QueuedSelectionRetainsOriginalAccount()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        using var release = new ManualResetEventSlim();
        var reached = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        int admissions = 0;
        owner.BeforeAdmission = () => {
            if (Interlocked.Increment(ref admissions) == 8)
            { reached.SetResult(); Check(release.Wait(TimeSpan.FromSeconds(10))); }
        };
        var first = client.SelectChannelAsync(HistoryServer.A, expectedUserId: HistoryServer.User, expectedSessionStamp: Stamp);
        Task? queued = null;
        const string other = "b28041bc-1b5b-40f6-aaee-2f9163c5f9dd";
        try
        {
            await reached.Task.WaitAsync(TimeSpan.FromSeconds(8));
            queued = client.SelectChannelAsync(HistoryServer.B, expectedUserId: HistoryServer.User, expectedSessionStamp: Stamp);
            server.SessionUser = other; owner.SwitchAccount(other, "synthetic:new-family:1");
            // New account B has connected and received the shared directory,
            // but its bootstrap selection is queued behind the old UI intent.
            await Until(() => client.Snapshot.Connected && client.Snapshot.UserId == other && client.Snapshot.Channels.Count > 0);
            release.Set();
            try { await first; } catch (InvalidOperationException) { }
            try { await queued; } catch (InvalidOperationException) { }
            await Until(() => client.Snapshot.Connected && !client.Snapshot.Loading && client.Snapshot.SelectedChannel == HistoryServer.A);
            Check(server.Pages.All(page => page.Channel != HistoryServer.B));
            Check(client.Snapshot.UserId == other && client.Snapshot.Error is null);
        }
        finally { release.Set(); owner.BeforeAdmission = null; }
    }
    private static async Task CapacityAndAccess()
    {
        await using var server = await HistoryServer.StartAsync(); server.IsReply = _ => false; server.Total = 2050;
        var owner = new NativeAuthSession(server.SocketUri);
        await using var client = new NativeChatClient(owner); await client.StartAsync();
        string initialId = client.Snapshot.Messages[0].Id;
        for (int i = 1; i < 40; i++) await Older(client);
        Check(client.Snapshot.Messages.Count == 2000 && client.Snapshot.History!.AtCapacity && client.Snapshot.Messages.Any(m => m.Id == initialId));
        int count = server.Pages.Count; await Older(client); Check(server.Pages.Count == count);
        await client.SelectChannelAsync(HistoryServer.A); Check(!client.Snapshot.History!.AtCapacity && client.Snapshot.Messages.Count == 50);
        server.DenyNext = true; await Older(client);
        Check(client.Snapshot.Messages.Count == 0 && client.Snapshot.SelectedChannel is null && client.Snapshot.Channels.All(c => c.Id != HistoryServer.A));
        Check(owner.Snapshot.Phase == NativeAuthPhase.Active);
        server.Total = 0; await client.SelectChannelAsync(HistoryServer.B);
        Check(client.Snapshot.Messages.Count == 0 && !client.Snapshot.History!.HasOlder && client.Snapshot.History.OldestSequence is null);
    }
}
