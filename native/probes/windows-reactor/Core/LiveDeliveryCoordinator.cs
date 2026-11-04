namespace Sproyt.WindowsProbe;

internal enum LiveDeliveryOutcome { Acknowledged, Rejected, Uncertain, Deferred, BackingOff }

internal sealed record LiveDeliveryResult(LiveDeliveryOutcome Outcome, string? Message = null, TimeSpan? RetryAfter = null);

internal sealed class LiveDeliveryPersistenceException(TimeSpan retryAfter, Exception cause)
    : IOException("The local delivery transition could not be confirmed.", cause)
{
    internal TimeSpan RetryAfter { get; } = retryAfter;
}

// One application pump calls this coordinator. It never owns credentials or
// payloads; admission and durable storage remain with their existing owners.
internal sealed class LiveDeliveryCoordinator(TimeProvider? clock = null)
{
    private readonly LiveDeliveryBackoff _wire = new(clock ?? TimeProvider.System);
    private readonly LiveDeliveryBackoff _storage = new(clock ?? TimeProvider.System);
    // Keep the server's terminal receipt while the local write is outstanding.
    // The same account pump can finish journaling without another transmission.
    private readonly Dictionary<string, LiveDeliveryResult> _receipts = [];

    internal void Reconcile(IReadOnlyList<QueuedMessage> persisted)
    {
        var unresolved = persisted.Where(item => item.State is not ("rejected" or "acknowledged")).Select(item => item.RequestId).ToHashSet();
        _wire.Reconcile(unresolved);
        _storage.Reconcile(persisted.Where(item => item.State != "rejected").Select(item => item.RequestId).ToHashSet());
        foreach (string id in _receipts.Keys.Where(id => !unresolved.Contains(id)).ToArray())
            _receipts.Remove(id);
    }

    internal async Task<LiveDeliveryResult> DispatchAsync(QueuedMessage item,
        Func<string, string, Task> persist, Func<Task> transmit, Func<bool> admitted,
        CancellationToken cancellationToken, bool manualRetry = false)
    {
        if (item.State == "rejected") return new(LiveDeliveryOutcome.Rejected);
        if (cancellationToken.IsCancellationRequested) return new(LiveDeliveryOutcome.Deferred);
        string id = item.RequestId;
        _receipts.TryGetValue(id, out var receipt);
        bool needsTransport = receipt is null && item.State != "acknowledged";
        if (needsTransport && !admitted()) return new(LiveDeliveryOutcome.Deferred);

        var storageDelay = _storage.Remaining(id, manualRetry);
        if (storageDelay > TimeSpan.Zero) return new(LiveDeliveryOutcome.BackingOff, RetryAfter: storageDelay);

        async Task Write(string action)
        {
            try { await persist(id, action); }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { throw; }
            catch (Exception failure) { throw new LiveDeliveryPersistenceException(_storage.RecordFailure(id), failure); }
        }

        if (needsTransport)
        {
            var remaining = _wire.Remaining(id, manualRetry);
            if (remaining > TimeSpan.Zero) return new(LiveDeliveryOutcome.BackingOff, RetryAfter: remaining);
            if (item.State == "dispatching") await Write("uncertain");
            await Write("dispatch");
            // Account/session may change while the durable write is pending.
            // The transport performs its own atomic admission as well.
            if (cancellationToken.IsCancellationRequested || !admitted())
            {
                await Write("uncertain");
                _storage.Forget(id);
                return new(LiveDeliveryOutcome.Deferred);
            }
            try
            {
                await transmit();
                receipt = new(LiveDeliveryOutcome.Acknowledged);
            }
            catch (NativeCommandException failure) when
                (failure.IsDefinitiveRejection && failure.RequestId == item.RequestId)
            {
                receipt = new(LiveDeliveryOutcome.Rejected, failure.Message);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                return new(LiveDeliveryOutcome.Deferred);
            }
            catch
            {
                // Set the deadline before awaiting storage. A lost local write
                // result cannot let a queued Changed signal retrigger the wire.
                var retryAfter = _wire.RecordFailure(id);
                await Write("uncertain");
                _storage.Forget(id);
                return new(LiveDeliveryOutcome.Uncertain, RetryAfter: retryAfter);
            }
            _receipts[id] = receipt;
            _wire.Forget(id);
        }
        if (receipt?.Outcome == LiveDeliveryOutcome.Rejected) await Write("reject");
        else
        {
            if (item.State != "acknowledged") await Write("acknowledge");
            await Write("forget");
        }
        _receipts.Remove(id);
        _storage.Forget(id);
        return receipt ?? new(LiveDeliveryOutcome.Acknowledged);
    }
}
