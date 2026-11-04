namespace Sproyt.WindowsProbe;

// Owned by the account's delivery pump. A deadline limits transmission on a
// healthy socket even if every error, render or channel event wakes that pump.
// IDs and payloads remain in the durable outbox. Reopening the account permits
// an initial attempt under the existing replay-consent and transport checks.
internal sealed class LiveDeliveryBackoff(TimeProvider clock)
{
    private sealed record Failure(long Timestamp, int DelaySeconds);
    private readonly Dictionary<string, Failure> _failures = [];

    internal TimeSpan RecordFailure(string requestId)
    {
        int delaySeconds = _failures.TryGetValue(requestId, out var previous)
            ? Math.Min(previous.DelaySeconds * 2, 60) : 2;
        _failures[requestId] = new(clock.GetTimestamp(), delaySeconds);
        return TimeSpan.FromSeconds(delaySeconds);
    }

    internal TimeSpan Remaining(string requestId, bool manualRetry)
    {
        if (!_failures.TryGetValue(requestId, out var failure)) return TimeSpan.Zero;
        var elapsed = clock.GetElapsedTime(failure.Timestamp);
        var normalDelay = TimeSpan.FromSeconds(failure.DelaySeconds);
        if (elapsed >= normalDelay || (manualRetry && elapsed >= TimeSpan.FromSeconds(1))) return TimeSpan.Zero;
        // A manual click inside the minimum interval is consumed. The next
        // automatic wake still uses the original deadline.
        return normalDelay - elapsed;
    }

    internal void Forget(string requestId) => _failures.Remove(requestId);

    internal void Reconcile(IReadOnlySet<string> unresolved)
    {
        foreach (string id in _failures.Keys.Where(id => !unresolved.Contains(id)).ToArray())
            _failures.Remove(id);
    }
}
