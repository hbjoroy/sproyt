internal sealed class ProbeQueueFullException : InvalidOperationException
{
    public ProbeQueueFullException() : base("The native probe worker queue is full.") { }
}

internal sealed class ProbeWorkerInvalidatedException : InvalidOperationException
{
    public ProbeWorkerInvalidatedException() : base("Logout invalidated this worker's pending work and closed admission.") { }
}

// A deliberately small lifecycle probe, not a production command bus. Native
// calls remain synchronous, but admission never waits for queue capacity or I/O.
internal sealed class ProbeWorker : IAsyncDisposable
{
    private const int Capacity = 2;
    private readonly object _gate = new();
    private readonly Queue<IWork> _pending = new();
    private readonly Action? _closeNative;
    private readonly TaskCompletionSource _closed = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly Thread _thread;
    private IWork? _logoutBarrier;
    private bool _logoutStarted;
    private bool _closing;

    public ProbeWorker(Action? closeNative = null)
    {
        _closeNative = closeNative;
        _thread = new Thread(Run) { IsBackground = true, Name = "Sproyt native lifecycle probe" };
        _thread.Start();
    }

    public Task<T> Queue<T>(Func<T> operation, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(operation);
        lock (_gate)
        {
            if (_closing) return Task.FromException<T>(new ObjectDisposedException(nameof(ProbeWorker)));
            if (_logoutStarted) return Task.FromException<T>(new ProbeWorkerInvalidatedException());
            if (cancellationToken.IsCancellationRequested) return Task.FromCanceled<T>(cancellationToken);
            if (_pending.Count == Capacity) return Task.FromException<T>(new ProbeQueueFullException());
            var work = new Work<T>(operation, cancellationToken);
            _pending.Enqueue(work);
            Monitor.Pulse(_gate);
            return work.Task;
        }
    }

    // Logout is a terminal admission fence. Already-running work may commit;
    // pending work is rejected before its delegate can run. The single control
    // slot is bounded and executes after the running call, on the same thread.
    public Task<T> BeginLogout<T>(Func<T> invalidateNative)
    {
        ArgumentNullException.ThrowIfNull(invalidateNative);
        lock (_gate)
        {
            if (_closing) return Task.FromException<T>(new ObjectDisposedException(nameof(ProbeWorker)));
            if (_logoutStarted) return Task.FromException<T>(new ProbeWorkerInvalidatedException());
            _logoutStarted = true;
            while (_pending.TryDequeue(out var pending)) pending.Reject(new ProbeWorkerInvalidatedException());
            var barrier = new Work<T>(invalidateNative, CancellationToken.None);
            _logoutBarrier = barrier;
            Monitor.Pulse(_gate);
            return barrier.Task;
        }
    }

    // Normal shutdown drains admitted work. This returns immediately; disposing
    // the native handle occurs only in Run's finally block, never on the caller.
    public Task CloseAsync()
    {
        lock (_gate)
        {
            _closing = true;
            Monitor.Pulse(_gate);
            return _closed.Task;
        }
    }

    public ValueTask DisposeAsync() => new(CloseAsync());

    private void Run()
    {
        Exception? failure = null;
        try
        {
            while (true)
            {
                IWork work;
                bool started;
                lock (_gate)
                {
                    while (_pending.Count == 0 && _logoutBarrier is null && !_closing) Monitor.Wait(_gate);
                    if (_logoutBarrier is not null)
                    {
                        work = _logoutBarrier;
                        _logoutBarrier = null;
                    }
                    else if (_pending.TryDequeue(out var next)) work = next;
                    else break;
                    // Claim under the admission gate: a dequeued item cannot
                    // escape a concurrent logout fence before it starts.
                    started = work.TryStart();
                }
                work.Execute(started);
            }
        }
        catch (Exception exception)
        {
            failure = exception;
            lock (_gate)
            {
                _closing = true;
                while (_pending.TryDequeue(out var pending)) pending.Reject(exception);
                _logoutBarrier?.Reject(exception);
                _logoutBarrier = null;
            }
        }
        finally
        {
            try { _closeNative?.Invoke(); }
            catch (Exception exception) { failure = failure is null ? exception : new AggregateException(failure, exception); }
            if (failure is null) _closed.TrySetResult();
            else _closed.TrySetException(failure);
        }
    }

    private interface IWork
    {
        bool TryStart();
        void Execute(bool started);
        void Reject(Exception exception);
    }

    private sealed class Work<T> : IWork
    {
        private const int Pending = 0, Running = 1, Finished = 2, Canceled = 3;
        private readonly Func<T> _operation;
        private readonly CancellationToken _token;
        private readonly TaskCompletionSource<T> _result = new(TaskCreationOptions.RunContinuationsAsynchronously);
        private readonly CancellationTokenRegistration _registration;
        private int _state;

        public Work(Func<T> operation, CancellationToken token)
        {
            _operation = operation;
            _token = token;
            _registration = token.Register(CancelPending);
        }

        public Task<T> Task => _result.Task;

        private void CancelPending()
        {
            if (Interlocked.CompareExchange(ref _state, Canceled, Pending) == Pending)
                _result.TrySetCanceled(_token);
        }

        public bool TryStart() => Interlocked.CompareExchange(ref _state, Running, Pending) == Pending;

        public void Execute(bool started)
        {
            // This CAS is the execution/cancellation boundary. Once Running,
            // cancellation cannot hide a committed native result or exception.
            _registration.Dispose();
            if (!started) return;
            try { _result.TrySetResult(_operation()); }
            catch (Exception exception) { _result.TrySetException(exception); }
            finally { Volatile.Write(ref _state, Finished); }
        }

        public void Reject(Exception exception)
        {
            if (Interlocked.CompareExchange(ref _state, Finished, Pending) == Pending)
                _result.TrySetException(exception);
            _registration.Dispose();
        }
    }
}
