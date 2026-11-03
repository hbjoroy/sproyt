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
            var store = new Store(); var dispatcher = new LiveDeliveryCoordinator();
            var server = new Dictionary<string, string>(); var attempts = new List<QueuedMessage>();
            Task Send()
            {
                attempts.Add(store.Item);
                server.TryAdd(store.Item.RequestId, store.Item.Body);
                if (attempts.Count == 1) throw new IOException("Server committed, response lost.");
                return Task.CompletedTask;
            }
            Require((await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default)).Outcome == LiveDeliveryOutcome.Uncertain);
            Require((await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default)).Outcome == LiveDeliveryOutcome.Acknowledged);
            Require(server.Count == 1 && attempts.Count == 2 && store.Forgotten &&
                attempts.All(item => item.RequestId == "request-1" && item.Body == "Same immutable payload"));
        });
        await Check("forget failure never retransmits an acknowledged message", async () =>
        {
            var store = new Store { FailOnce = "forget" }; var dispatcher = new LiveDeliveryCoordinator(); int sends = 0;
            Task Send() { sends++; return Task.CompletedTask; }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "acknowledged");
            await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Forgotten);
        });
        await Check("failed reject persistence is retried without another send", async () =>
        {
            var store = new Store { FailOnce = "reject" }; var dispatcher = new LiveDeliveryCoordinator(); int sends = 0;
            Task Send() { sends++; throw NativeCommandException.FromServer(store.Item.RequestId, "validation_error", "Ugyldig melding."); }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "dispatching");
            await dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default);
            Require(sends == 1 && store.Item.State == "rejected");
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
            var store = new Store { FailAfterOnce = "acknowledge" }; var dispatcher = new LiveDeliveryCoordinator(); int sends = 0;
            Task Send() { sends++; return Task.CompletedTask; }
            await ThrowsIo(() => dispatcher.DispatchAsync(store.Item, store.Persist, Send, () => true, default));
            Require(store.Item.State == "acknowledged");
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
    private static async Task ThrowsIo(Func<Task> action)
    {
        try { await action(); }
        catch (IOException) { return; }
        throw new InvalidOperationException("Expected injected storage failure.");
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
