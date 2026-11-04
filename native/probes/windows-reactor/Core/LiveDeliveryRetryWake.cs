namespace Sproyt.WindowsProbe;

// Holds one wake for the currently admitted scope. The pump still checks its
// own monotonic backoff and transport admission after receiving the signal.
internal sealed class LiveDeliveryRetryWake(Action signal, TimeProvider? clock = null) : IDisposable
{
    private sealed class Deadline(string scope, long start, TimeSpan delay)
    {
        internal readonly string Scope = scope;
        internal readonly long Start = start;
        internal readonly TimeSpan Delay = delay;
        internal ITimer? Timer;
    }

    private readonly TimeProvider _clock = clock ?? TimeProvider.System;
    private readonly object _gate = new();
    private Deadline? _active;
    private bool _disposed;

    internal void Schedule(string scope, TimeSpan retryAfter)
    {
        ArgumentException.ThrowIfNullOrEmpty(scope);
        ArgumentOutOfRangeException.ThrowIfLessThan(retryAfter, TimeSpan.Zero);
        lock (_gate)
        {
            if (_disposed) return;
            if (_active is { } pending && pending.Scope == scope && Remaining(pending) <= retryAfter) return;

            var next = new Deadline(scope, _clock.GetTimestamp(), retryAfter);
            // Create the timer disabled. Even an immediately due callback sees
            // both its identity and timer handle installed before it can run.
            next.Timer = _clock.CreateTimer(_ => Wake(next), null, Timeout.InfiniteTimeSpan, Timeout.InfiniteTimeSpan);
            var old = _active;
            _active = next;
            try { next.Timer.Change(TimerDelay(retryAfter), Timeout.InfiniteTimeSpan); }
            catch { _active = old; next.Timer.Dispose(); throw; }
            old?.Timer!.Dispose();
        }
    }

    private TimeSpan Remaining(Deadline deadline) => deadline.Delay - _clock.GetElapsedTime(deadline.Start);

    // The system timer has millisecond resolution. Round up so a fractional
    // remaining interval does not repeatedly arm an immediate wake.
    private static TimeSpan TimerDelay(TimeSpan delay) => TimeSpan.FromMilliseconds(Math.Ceiling(delay.TotalMilliseconds));

    private void Wake(Deadline deadline)
    {
        lock (_gate)
        {
            // Dispose does not withdraw a callback already queued by the OS.
            if (!ReferenceEquals(_active, deadline)) return;
            var remaining = Remaining(deadline);
            if (remaining > TimeSpan.Zero)
            {
                deadline.Timer!.Change(TimerDelay(remaining), Timeout.InfiniteTimeSpan);
                return;
            }
            _active = null;
            deadline.Timer!.Dispose();
            // Signal only releases the pump semaphore. Keeping it inside the
            // gate ensures no old scope signals after Cancel/Schedule return.
            signal();
        }
    }

    internal void Cancel()
    {
        lock (_gate) CancelLocked();
    }

    private void CancelLocked()
    {
        var old = _active;
        _active = null;
        old?.Timer!.Dispose();
    }

    public void Dispose()
    {
        lock (_gate)
        {
            _disposed = true;
            CancelLocked();
        }
    }
}
