namespace Sproyt.WindowsProbe;

internal enum LiveDeliveryOutcome { Acknowledged, Rejected, Uncertain, Deferred }

internal sealed record LiveDeliveryResult(LiveDeliveryOutcome Outcome, string? Message = null);

// One application pump calls this coordinator. It never owns credentials or
// payloads; admission and durable storage remain with their existing owners.
internal sealed class LiveDeliveryCoordinator
{
    // A failed local reject write must be retried before any further network
    // attempt in this process. A successful durable reject no longer needs it.
    private readonly Dictionary<string, string> _unpersistedRejections = [];

    internal void Reconcile(IReadOnlyList<QueuedMessage> persisted)
    {
        var unresolved = persisted.Where(item => item.State != "rejected").Select(item => item.RequestId).ToHashSet();
        foreach (string id in _unpersistedRejections.Keys.Where(id => !unresolved.Contains(id)).ToArray())
            _unpersistedRejections.Remove(id);
    }

    internal async Task<LiveDeliveryResult> DispatchAsync(QueuedMessage item,
        Func<string, string, Task> persist, Func<Task> transmit, Func<bool> admitted,
        CancellationToken cancellationToken)
    {
        if (item.State == "rejected") return new(LiveDeliveryOutcome.Rejected);
        if (_unpersistedRejections.TryGetValue(item.RequestId, out var rejection))
            return await RejectAsync(item.RequestId, rejection, persist);
        if (cancellationToken.IsCancellationRequested || !admitted()) return new(LiveDeliveryOutcome.Deferred);

        if (item.State != "acknowledged")
        {
            if (item.State == "dispatching") await persist(item.RequestId, "uncertain");
            await persist(item.RequestId, "dispatch");
            // Account/session may change while the durable write is pending.
            // The transport performs its own atomic admission as well.
            if (cancellationToken.IsCancellationRequested || !admitted())
            {
                await persist(item.RequestId, "uncertain");
                return new(LiveDeliveryOutcome.Deferred);
            }
            try { await transmit(); }
            catch (NativeCommandException failure) when
                (failure.IsDefinitiveRejection && failure.RequestId == item.RequestId)
            {
                _unpersistedRejections[item.RequestId] = failure.Message;
                return await RejectAsync(item.RequestId, failure.Message, persist);
            }
            catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
            {
                return new(LiveDeliveryOutcome.Deferred);
            }
            catch
            {
                await persist(item.RequestId, "uncertain");
                return new(LiveDeliveryOutcome.Uncertain);
            }
            await persist(item.RequestId, "acknowledge");
        }
        await persist(item.RequestId, "forget");
        return new(LiveDeliveryOutcome.Acknowledged);
    }

    private async Task<LiveDeliveryResult> RejectAsync(string requestId, string message,
        Func<string, string, Task> persist)
    {
        await persist(requestId, "reject");
        _unpersistedRejections.Remove(requestId);
        return new(LiveDeliveryOutcome.Rejected, message);
    }
}
