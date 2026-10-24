using System.Text.Json;

internal static class WorkerProbe
{
    private static readonly TimeSpan Deadline = TimeSpan.FromSeconds(10);

    public static void Run(Func<object, string?, JsonElement> call, string database, string account, string channel)
    {
        Check(!File.Exists(database), "Worker probe refuses to reuse an existing database.");
        int callerThread = Environment.CurrentManagedThreadId;
        int nativeThread = 0;
        int closeCount = 0;
        ulong handle = 0;

        JsonElement Native(object request, string? expectedError = null)
        {
            int current = Environment.CurrentManagedThreadId;
            Check(current != callerThread, "Native work ran on the submitting thread.");
            if (nativeThread == 0) nativeThread = current;
            Check(current == nativeThread, "Native handle crossed worker threads.");
            return call(request, expectedError);
        }

        var worker = new ProbeWorker(() =>
        {
            Check(Environment.CurrentManagedThreadId == nativeThread, "Native close ran outside its worker.");
            Interlocked.Increment(ref closeCount);
            if (handle != 0) Native(new { Op = "close", Handle = handle });
        });
        try
        {
            handle = Await(worker.Queue(() => Native(new { Op = "open", Database = database, Account = account }).GetProperty("handle").GetUInt64()));
            using (var canceledBeforeAdmission = new CancellationTokenSource())
            {
                canceledBeforeAdmission.Cancel();
                ExpectCanceled(worker.Queue(() => Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Never admitted" }), canceledBeforeAdmission.Token));
            }

            // The running delegate is held, so both queue slots and every
            // admission/cancellation assertion below are deterministic.
            using (var gate = new ExecutionGate())
            using (var cancellation = new CancellationTokenSource())
            {
                var active = worker.Queue(() => gate.Hold(() => Native(new { Op = "snapshot", Handle = handle })));
                gate.WaitUntilEntered();
                bool canceledDelegateRan = false;
                var canceledWrite = worker.Queue(() =>
                {
                    canceledDelegateRan = true;
                    return Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Never executed" });
                }, cancellation.Token);
                var queuedSnapshot = worker.Queue(() => Native(new { Op = "snapshot", Handle = handle }));
                Expect<ProbeQueueFullException>(worker.Queue(() => Native(new { Op = "snapshot", Handle = handle })));
                cancellation.Cancel();
                ExpectCanceled(canceledWrite);
                gate.Release();
                Await(active);
                Check(Await(queuedSnapshot).GetProperty("drafts").GetArrayLength() == 0, "A canceled write persisted.");
                Check(!canceledDelegateRan, "A canceled pending delegate executed.");
            }

            // Claiming execution is the boundary, even before P/Invoke entry.
            using (var gate = new ExecutionGate())
            using (var cancellation = new CancellationTokenSource())
            {
                var claimed = worker.Queue(() => gate.Hold(() => Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Claimed work survives cancellation" })), cancellation.Token);
                gate.WaitUntilEntered();
                cancellation.Cancel();
                Check(!claimed.IsCompleted, "Cancellation hid an already-claimed operation.");
                gate.Release();
                Await(claimed);
                var snapshot = Await(worker.Queue(() => Native(new { Op = "snapshot", Handle = handle })));
                Check(snapshot.GetProperty("drafts")[0].GetProperty("body").GetString() == "Claimed work survives cancellation", "Claimed native write was skipped.");
            }

            // Cancellation after a native commit must preserve the actual
            // completion. Hold the result after the synchronous native call.
            using (var gate = new ExecutionGate())
            using (var cancellation = new CancellationTokenSource())
            {
                var committed = worker.Queue(() =>
                {
                    var result = Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Committed result survives cancellation" });
                    return gate.Hold(() => result);
                }, cancellation.Token);
                gate.WaitUntilEntered();
                cancellation.Cancel();
                Check(!committed.IsCompleted, "Cancellation completed an already-running native call.");
                gate.Release();
                Await(committed);
                Check(committed.Status == TaskStatus.RanToCompletion, "A committed result was hidden by cancellation.");
                var snapshot = Await(worker.Queue(() => Native(new { Op = "snapshot", Handle = handle })));
                Check(snapshot.GetProperty("drafts")[0].GetProperty("body").GetString() == "Committed result survives cancellation", "Committed write was lost.");
            }

            using (var gate = new ExecutionGate())
            {
                var active = worker.Queue(() => gate.Hold(() => Native(new { Op = "snapshot", Handle = handle })));
                gate.WaitUntilEntered();
                bool oldWriteRan = false;
                var oldWrite = worker.Queue(() =>
                {
                    oldWriteRan = true;
                    return Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Queued before logout" });
                });
                var logout = worker.BeginLogout(() =>
                {
                    Native(new { Op = "invalidate", Handle = handle, Generation = 1 });
                    Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Late direct callback" }, "stale_generation");
                    return Native(new { Op = "snapshot", Handle = handle });
                });
                Expect<ProbeWorkerInvalidatedException>(oldWrite);
                Expect<ProbeWorkerInvalidatedException>(worker.Queue(() => Native(new { Op = "snapshot", Handle = handle })));
                Expect<ProbeWorkerInvalidatedException>(worker.BeginLogout(() => 0));
                Check(!logout.IsCompleted, "Logout passed an active native operation.");
                gate.Release();
                Await(active);
                var invalidated = Await(logout);
                Check(!oldWriteRan, "A queued old-generation write executed after the logout fence.");
                Check(!invalidated.GetProperty("active").GetBoolean() && invalidated.GetProperty("generation").GetInt32() == 2, "Logout did not invalidate the native generation.");
                Check(invalidated.GetProperty("drafts").GetArrayLength() == 0, "Logout left drafts visible.");
            }
        }
        finally
        {
            Await(worker.CloseAsync());
            Await(worker.DisposeAsync().AsTask());
        }
        Check(closeCount == 1, "Repeated disposal closed the native handle more than once.");

        VerifyCloseWhileActive(call, database + ".drain", account, channel, callerThread);
        Console.WriteLine("PASS worker: bounded admission, cancellation boundary, logout fence and worker-thread drain/close");
    }

    private static void VerifyCloseWhileActive(Func<object, string?, JsonElement> call, string database, string account, string channel, int callerThread)
    {
        Check(!File.Exists(database), "Drain probe refuses to reuse an existing database.");
        ulong handle = 0;
        int workerThread = 0;
        int closeCount = 0;
        bool queuedWriteRan = false;
        JsonElement Native(object request)
        {
            Check(Environment.CurrentManagedThreadId == workerThread && workerThread != callerThread, "Drain native call left its worker.");
            return call(request, null);
        }
        var worker = new ProbeWorker(() =>
        {
            Check(queuedWriteRan, "Native handle closed before admitted work drained.");
            Native(new { Op = "close", Handle = handle });
            Interlocked.Increment(ref closeCount);
        });
        try
        {
            handle = Await(worker.Queue(() =>
            {
                workerThread = Environment.CurrentManagedThreadId;
                return Native(new { Op = "open", Database = database, Account = account }).GetProperty("handle").GetUInt64();
            }));
            using (var gate = new ExecutionGate())
            {
                var active = worker.Queue(() => gate.Hold(() => Native(new { Op = "snapshot", Handle = handle })));
                gate.WaitUntilEntered();
                var pending = worker.Queue(() =>
                {
                    var result = Native(new { Op = "save_draft", Handle = handle, Generation = 1, Channel = channel, Body = "Drain before close" });
                    queuedWriteRan = true;
                    return result;
                });
                var closing = worker.CloseAsync();
                Check(!closing.IsCompleted && closeCount == 0, "Close disposed the native handle during active work.");
                Expect<ObjectDisposedException>(worker.Queue(() => Native(new { Op = "snapshot", Handle = handle })));
                gate.Release();
                Await(active);
                Await(pending);
                Await(closing);
                Await(worker.CloseAsync());
            }
        }
        finally { Await(worker.DisposeAsync().AsTask()); }
        Check(closeCount == 1 && queuedWriteRan, "Drain/close did not complete exactly once.");
    }

    private static T Await<T>(Task<T> task) => task.WaitAsync(Deadline).GetAwaiter().GetResult();
    private static void Await(Task task) => task.WaitAsync(Deadline).GetAwaiter().GetResult();

    private static void Expect<T>(Task task) where T : Exception
    {
        try { Await(task); }
        catch (T) { return; }
        throw new InvalidOperationException($"Expected {typeof(T).Name}.");
    }

    private static void ExpectCanceled(Task task)
    {
        Expect<OperationCanceledException>(task);
        Check(task.IsCanceled, "Cancellation did not settle as a canceled task.");
    }

    private static void Check(bool condition, string message)
    {
        if (!condition) throw new InvalidOperationException(message);
    }

    private sealed class ExecutionGate : IDisposable
    {
        private readonly ManualResetEventSlim _entered = new();
        private readonly ManualResetEventSlim _release = new();
        public T Hold<T>(Func<T> operation)
        {
            _entered.Set();
            Check(_release.Wait(Deadline), "Timed out waiting to release the active worker call.");
            return operation();
        }
        public void WaitUntilEntered() => Check(_entered.Wait(Deadline), "Timed out waiting for the worker to enter the gated operation.");
        public void Release() => _release.Set();
        public void Dispose()
        {
            // Always release on a failed assertion. Events are kept alive until
            // the worker settles, so cleanup cannot race a waiting native call.
            _release.Set();
        }
    }
}
