using System.Text.Json;
using Sproyt.WindowsProbe;

await DeliveryContracts.RunAsync(args.FirstOrDefault());

internal static class DeliveryContracts
{
    private static int _passed;

    internal static async Task RunAsync(string? receiptPath)
    {
        await Check("definitive rejection is durable and never replayed", async () =>
        {
            var store = new Store(); var dispatcher = new LiveDeliveryCoordinator(); int sends = 0;
            Task Send() { sends++; throw NativeCommandException.FromServer(store.Item.RequestId, "permission_denied", "Ingen tilgang."); }
            var result = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(result.Outcome == LiveDeliveryOutcome.Rejected && store.Item.State == "rejected");
            // A fresh coordinator models reopening a durably rejected queue.
            await new LiveDeliveryCoordinator().DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Actions.SequenceEqual(["dispatch", "reject"]));
        });
        await Check("server/storage errors retain an uncertain send", async () =>
        {
            var store = new Store();
            var result = await new LiveDeliveryCoordinator().DispatchAsync(store.Item, store.Persist,
                () => throw NativeCommandException.FromServer(store.Item.RequestId, "storage_error", "Feil."), () => true, default);
            Require(result.Outcome == LiveDeliveryOutcome.Uncertain && store.Item.State == "uncertain");
        });
        await Check("an unrelated rejection cannot reject this request", async () =>
        {
            var store = new Store();
            var result = await new LiveDeliveryCoordinator().DispatchAsync(store.Item, store.Persist,
                () => throw NativeCommandException.FromServer("another-request", "permission_denied", "Ingen tilgang."), () => true, default);
            Require(result.Outcome == LiveDeliveryOutcome.Uncertain && store.Item.State == "uncertain");
        });
        await Check("lost ACK replays the exact payload and request ID", async () =>
        {
            var clock = new ManualClock(); var store = new Store(); var dispatcher = new LiveDeliveryCoordinator(clock);
            var server = new Dictionary<string, string>(); var attempts = new List<QueuedMessage>();
            Task Send()
            {
                attempts.Add(store.Item);
                server.TryAdd(store.Item.RequestId, store.Item.Body);
                if (attempts.Count == 1) throw new IOException("Server committed, response lost.");
                return Task.CompletedTask;
            }
            var first = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(first.Outcome == LiveDeliveryOutcome.Uncertain && first.RetryAfter == TimeSpan.FromSeconds(2));
            clock.Advance(first.RetryAfter!.Value);
            Require((await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default)).Outcome == LiveDeliveryOutcome.Acknowledged);
            Require(server.Count == 1 && attempts.Count == 2 && store.Forgotten &&
                attempts.All(item => item.RequestId == "request-1" && item.Body == "Same immutable payload"));
        });
        await Check("repeated live-socket error signals respect each monotonic deadline and the cap", async () =>
        {
            var clock = new ManualClock(); var dispatcher = new LiveDeliveryCoordinator(clock); var store = new Store();
            int sends = 0;
            Task Send()
            {
                sends++;
                string code = sends % 2 == 0 ? "future_server_error" : "storage_error";
                throw NativeCommandException.FromServer(store.Item.RequestId, code, "Uvisst utfall.");
            }
            int attempt = 0;
            foreach (int seconds in new[] { 2, 4, 8, 16, 32, 60, 60 })
            {
                attempt++;
                TimeSpan delay = TimeSpan.FromSeconds(seconds);
                var result = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
                Require(result.Outcome == LiveDeliveryOutcome.Uncertain && result.RetryAfter == delay && sends == attempt);
                int writes = store.Actions.Count;
                // A server reply and a UI error both wake the real pump. More
                // notifications and repeated durable snapshots cannot admit IO.
                for (int signal = 0; signal < 20; signal++)
                {
                    dispatcher.Reconcile([store.Item]);
                    var gated = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
                    Require(gated.Outcome == LiveDeliveryOutcome.BackingOff && gated.RetryAfter == delay);
                }
                clock.Advance(delay - TimeSpan.FromMilliseconds(1));
                var early = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
                Require(early.Outcome == LiveDeliveryOutcome.BackingOff && early.RetryAfter == TimeSpan.FromMilliseconds(1));
                Require(store.Actions.Count == writes && sends == attempt && store.Item.State == "uncertain");
                clock.Advance(TimeSpan.FromMilliseconds(1));
            }
            var success = await dispatcher.DispatchAsync(store.Item, store.Persist, () => Task.CompletedTask, () => true, default);
            Require(success.Outcome == LiveDeliveryOutcome.Acknowledged && success.RetryAfter is null && store.Forgotten);
        });
        await Check("manual retry has a minimum interval and preserves admission and terminal rejection", async () =>
        {
            var clock = new ManualClock(); var dispatcher = new LiveDeliveryCoordinator(clock); var store = new Store();
            bool admitted = true; int sends = 0;
            Task Send()
            {
                sends++;
                string code = sends == 3 ? "permission_denied" : "storage_error";
                throw NativeCommandException.FromServer(store.Item.RequestId, code, "Server result");
            }
            Task<LiveDeliveryResult> Pump(bool manual = false) =>
                dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => admitted, default, manualRetry: manual);
            Require((await Pump()).Outcome == LiveDeliveryOutcome.Uncertain);
            Require((await Pump(true)).Outcome == LiveDeliveryOutcome.BackingOff && sends == 1);
            clock.Advance(TimeSpan.FromSeconds(1));
            admitted = false;
            Require((await Pump(true)).Outcome == LiveDeliveryOutcome.Deferred && sends == 1);
            admitted = true;
            Require((await Pump()).Outcome == LiveDeliveryOutcome.BackingOff && sends == 1);
            Require((await Pump(true)).RetryAfter == TimeSpan.FromSeconds(4) && sends == 2);
            Require((await Pump(true)).Outcome == LiveDeliveryOutcome.BackingOff && sends == 2);
            clock.Advance(TimeSpan.FromSeconds(1));
            Require((await Pump(true)).Outcome == LiveDeliveryOutcome.Rejected && sends == 3 && store.Item.State == "rejected");
            clock.Advance(TimeSpan.FromMinutes(10));
            Require((await Pump(true)).Outcome == LiveDeliveryOutcome.Rejected && sends == 3);
        });
        await Check("a lost uncertain write result cannot bypass the retry deadline", async () =>
        {
            var clock = new ManualClock(); var dispatcher = new LiveDeliveryCoordinator(clock);
            var store = new Store { FailOnce = "uncertain" }; int sends = 0;
            Task Send() { sends++; return sends == 1 ? Task.FromException(new IOException("Lost server ACK")) : Task.CompletedTask; }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "dispatching");
            dispatcher.Reconcile([store.Item]);
            var gated = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(gated.Outcome == LiveDeliveryOutcome.BackingOff && sends == 1);
            clock.Advance(TimeSpan.FromSeconds(2));
            var replay = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(replay.Outcome == LiveDeliveryOutcome.Acknowledged && sends == 2 && store.Forgotten);
        });
        await Check("forget failure never retransmits an acknowledged message", async () =>
        {
            var clock = new ManualClock(); var store = new Store { FailOnce = "forget" }; var dispatcher = new LiveDeliveryCoordinator(clock); int sends = 0;
            Task Send() { sends++; return Task.CompletedTask; }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "acknowledged");
            dispatcher.Reconcile([store.Item]);
            var wait = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(wait.Outcome == LiveDeliveryOutcome.BackingOff && wait.RetryAfter == TimeSpan.FromSeconds(2));
            clock.Advance(wait.RetryAfter!.Value);
            await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Forgotten);
        });
        await Check("failed reject persistence is retried without another send", async () =>
        {
            var clock = new ManualClock(); var store = new Store { FailOnce = "reject" }; var dispatcher = new LiveDeliveryCoordinator(clock); int sends = 0;
            Task Send() { sends++; throw NativeCommandException.FromServer(store.Item.RequestId, "validation_error", "Ugyldig melding."); }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "dispatching");
            for (int signal = 0; signal < 20; signal++)
            {
                dispatcher.Reconcile([store.Item]);
                var wait = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
                Require(wait.Outcome == LiveDeliveryOutcome.BackingOff && wait.RetryAfter == TimeSpan.FromSeconds(2));
            }
            Require(store.Actions.SequenceEqual(["dispatch"]) && sends == 1);
            clock.Advance(TimeSpan.FromSeconds(2));
            await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Item.State == "rejected");
        });
        await Check("a received ACK survives failed local persistence without retransmission", async () =>
        {
            var clock = new ManualClock(); var store = new Store(); var dispatcher = new LiveDeliveryCoordinator(clock);
            int sends = 0;
            Task Send() { sends++; return Task.CompletedTask; }
            foreach (int delay in new[] { 2, 4 })
            {
                store.FailOnce = "acknowledge";
                var failure = await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
                Require(failure.RetryAfter == TimeSpan.FromSeconds(delay) && store.Item.State == "dispatching" && sends == 1);
                dispatcher.Reconcile([store.Item]);
                var wait = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
                Require(wait.Outcome == LiveDeliveryOutcome.BackingOff && wait.RetryAfter == failure.RetryAfter);
                clock.Advance(failure.RetryAfter);
            }
            var settled = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(settled.Outcome == LiveDeliveryOutcome.Acknowledged && sends == 1 && store.Forgotten);
        });
        await Check("account change during dispatch persistence prevents transport", async () =>
        {
            bool admitted = true; int sends = 0;
            var store = new Store { AfterPersist = action => { if (action == "dispatch") admitted = false; } };
            var result = await new LiveDeliveryCoordinator().DispatchAsync(store.Item, store.Persist,
                () => { sends++; return Task.CompletedTask; }, () => admitted, default);
            Require(result.Outcome == LiveDeliveryOutcome.Deferred && sends == 0 && store.Item.State == "uncertain");
        });
        await Check("lost local reject result reconciles from committed state", async () =>
        {
            var store = new Store { FailAfterOnce = "reject" }; var dispatcher = new LiveDeliveryCoordinator(); int sends = 0;
            Task Send() { sends++; throw NativeCommandException.FromServer(store.Item.RequestId, "permission_denied", "Ingen tilgang."); }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "rejected");
            dispatcher.Reconcile([store.Item]);
            var result = await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(result.Outcome == LiveDeliveryOutcome.Rejected && sends == 1);
        });
        await Check("lost local acknowledgement result skips transport on retry", async () =>
        {
            var clock = new ManualClock(); var store = new Store { FailAfterOnce = "acknowledge" }; var dispatcher = new LiveDeliveryCoordinator(clock); int sends = 0;
            Task Send() { sends++; return Task.CompletedTask; }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "acknowledged");
            clock.Advance(TimeSpan.FromSeconds(2));
            await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Forgotten);
        });
        await Check("cancelled delivery leaves durable state for restart", async () =>
        {
            var store = new Store(); using var lifetime = new CancellationTokenSource();
            var result = await new LiveDeliveryCoordinator().DispatchAsync(store.Item, store.Persist,
                () => { lifetime.Cancel(); throw new OperationCanceledException(lifetime.Token); }, () => true, lifetime.Token);
            Require(result.Outcome == LiveDeliveryOutcome.Deferred && store.Item.State == "dispatching" && !store.Forgotten);
        });
        await Check("retry wake retains the earliest deadline in the same scope", () =>
        {
            var clock = new ManualClock(); int wakes = 0;
            using var wake = new LiveDeliveryRetryWake(() => wakes++, clock);
            wake.Schedule("account-a", TimeSpan.FromSeconds(10));
            var original = clock.Timers.Single();
            clock.Advance(TimeSpan.FromSeconds(3));
            wake.Schedule("account-a", TimeSpan.FromSeconds(8));
            wake.Schedule("account-a", TimeSpan.FromSeconds(7));
            Require(clock.Timers.Count == 1 && !original.Disposed && wakes == 0);

            wake.Schedule("account-a", TimeSpan.FromSeconds(2));
            Require(clock.Timers.Count == 2 && clock.Timers.Count(timer => !timer.Disposed) == 1 && original.Disposed);
            original.FireQueuedCallback();
            clock.Advance(TimeSpan.FromMilliseconds(1999));
            var earlier = clock.Timers.Last();
            earlier.FireQueuedCallback(); // An early OS callback must wait for monotonic expiry.
            Require(wakes == 0);
            clock.Advance(TimeSpan.FromMilliseconds(1));
            earlier.FireQueuedCallback();
            Require(wakes == 1 && earlier.Disposed);
            return Task.CompletedTask;
        });
        await Check("replaced, cancelled and disposed scopes cannot signal the pump", () =>
        {
            var clock = new ManualClock(); int wakes = 0;
            using var wake = new LiveDeliveryRetryWake(() => wakes++, clock);
            wake.Schedule("account-a", TimeSpan.FromSeconds(1));
            var oldScope = clock.Timers.Single();
            wake.Schedule("account-b", TimeSpan.FromSeconds(10));
            var newScope = clock.Timers.Last();
            oldScope.FireQueuedCallback();
            clock.Advance(TimeSpan.FromSeconds(1));
            Require(oldScope.Disposed && !newScope.Disposed && wakes == 0);
            wake.Cancel();
            newScope.FireQueuedCallback();
            clock.Advance(TimeSpan.FromMinutes(1));
            Require(newScope.Disposed && wakes == 0);

            wake.Schedule("account-c", TimeSpan.Zero);
            clock.Advance(TimeSpan.Zero);
            Require(wakes == 1);
            wake.Schedule("account-c", TimeSpan.FromSeconds(2));
            var beforeClose = clock.Timers.Last();
            wake.Dispose();
            beforeClose.FireQueuedCallback();
            wake.Schedule("after-close", TimeSpan.Zero);
            wake.Cancel();
            clock.Advance(TimeSpan.FromMinutes(1));
            Require(wakes == 1 && clock.Timers.All(timer => timer.Disposed));
            return Task.CompletedTask;
        });
        string receipt = JsonSerializer.Serialize(new { scope = "native_delivery_coordinator", passed = _passed, actualTransport = false });
        if (receiptPath is not null)
        {
            string path = Path.GetFullPath(receiptPath);
            Directory.CreateDirectory(Path.GetDirectoryName(path)!);
            await File.WriteAllTextAsync(path, receipt);
        }
        Console.WriteLine(receipt);
    }

    private static async Task Check(string name, Func<Task> test)
    {
        try { await test().WaitAsync(TimeSpan.FromSeconds(10)); _passed++; Console.WriteLine("PASS " + name); }
        catch (Exception failure) { throw new InvalidOperationException("FAIL " + name, failure); }
    }
    private static void Require(bool condition) { if (!condition) throw new InvalidOperationException("Contract failed."); }
    private static async Task<LiveDeliveryPersistenceException> ThrowsIo(Func<Task> action)
    {
        try { await action(); }
        catch (LiveDeliveryPersistenceException failure) { return failure; }
        throw new InvalidOperationException("Expected injected storage failure.");
    }

    private sealed class ManualClock : TimeProvider
    {
        private long _ticks;
        internal readonly List<ManualTimer> Timers = [];
        public override long TimestampFrequency => TimeSpan.TicksPerSecond;
        public override long GetTimestamp() => _ticks;
        public override ITimer CreateTimer(TimerCallback callback, object? state, TimeSpan dueTime, TimeSpan period)
        {
            var timer = new ManualTimer(this, callback, state);
            timer.Change(dueTime, period);
            Timers.Add(timer);
            return timer;
        }
        internal void Advance(TimeSpan elapsed)
        {
            _ticks = checked(_ticks + elapsed.Ticks);
            foreach (var timer in Timers.ToArray()) timer.FireIfDue();
        }
    }

    private sealed class ManualTimer(ManualClock clock, TimerCallback callback, object? state) : ITimer
    {
        private long? _due;
        internal bool Disposed { get; private set; }
        public bool Change(TimeSpan dueTime, TimeSpan period)
        {
            Require(period == Timeout.InfiniteTimeSpan);
            if (Disposed) return false;
            _due = dueTime == Timeout.InfiniteTimeSpan ? null : checked(clock.GetTimestamp() + dueTime.Ticks);
            return true;
        }
        internal void FireIfDue()
        {
            if (_due is not long due || due > clock.GetTimestamp()) return;
            _due = null;
            FireQueuedCallback();
        }
        internal void FireQueuedCallback() => callback(state);
        public void Dispose() { Disposed = true; _due = null; }
        public ValueTask DisposeAsync() { Dispose(); return ValueTask.CompletedTask; }
    }

    private sealed class Store
    {
        internal QueuedMessage Item = new("request-1", "channel-1", "Same immutable payload", "queued");
        internal readonly List<string> Actions = [];
        internal string? FailOnce;
        internal string? FailAfterOnce;
        internal Action<string>? AfterPersist;
        internal bool Forgotten;
        internal Task Persist(string id, string action)
        {
            Require(id == Item.RequestId && !Forgotten);
            if (FailOnce == action) { FailOnce = null; throw new IOException("Injected local write failure."); }
            Actions.Add(action);
            string state = (Item.State, action) switch
            {
                ("queued" or "uncertain", "dispatch") => "dispatching",
                ("dispatching" or "uncertain", "uncertain") => "uncertain",
                ("dispatching" or "uncertain", "acknowledge") => "acknowledged",
                ("dispatching" or "uncertain" or "rejected", "reject") => "rejected",
                ("acknowledged", "forget") => "acknowledged",
                _ => throw new InvalidOperationException("Illegal durable transition.")
            };
            Item = Item with { State = state };
            if (action == "forget") Forgotten = true;
            AfterPersist?.Invoke(action);
            if (FailAfterOnce == action) { FailAfterOnce = null; throw new IOException("Committed but local reply lost."); }
            return Task.CompletedTask;
        }
    }
}
