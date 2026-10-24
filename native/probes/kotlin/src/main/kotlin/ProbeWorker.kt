package org.sproyt.probe

import java.util.ArrayDeque
import java.util.concurrent.CancellationException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

internal class ProbeQueueFullException : IllegalStateException("The native probe worker queue is full.")
internal class ProbeWorkerInvalidatedException : IllegalStateException("Logout invalidated pending work and closed admission.")
internal class ProbeWorkerClosedException : IllegalStateException("The native probe worker is closing.")

// The future stays private: unrestricted Future.cancel could hide a native
// commit after the worker has already claimed execution.
internal class Ticket<T> internal constructor(
    private val completion: CompletableFuture<T>,
    private val cancelPending: () -> Boolean = { false },
) {
    val isDone: Boolean get() = completion.isDone

    fun cancel(): Boolean = cancelPending()

    fun await(): T {
        try {
            return completion.get(10, TimeUnit.SECONDS)
        } catch (exception: ExecutionException) {
            throw exception.cause ?: exception
        } catch (exception: InterruptedException) {
            Thread.currentThread().interrupt()
            throw exception
        }
    }
}

// A bounded console lifecycle probe, not a production command bus. Admission
// never waits for queue space or for synchronous native work to complete.
internal class ProbeWorker(private val closeNative: () -> Unit = {}) {
    private val gate = ReentrantLock()
    private val changed = gate.newCondition()
    private val pending = ArrayDeque<WorkItem>()
    private val closed = CompletableFuture<Unit>()
    private var logoutBarrier: WorkItem? = null
    private var logoutStarted = false
    private var closing = false
    private val worker = Thread(::runWorker, "Sproyt JNI lifecycle probe").apply {
        isDaemon = true
        start()
    }

    fun <T> queue(operation: () -> T): Ticket<T> = gate.withLock {
        if (closing) return@withLock failed(ProbeWorkerClosedException())
        if (logoutStarted) return@withLock failed(ProbeWorkerInvalidatedException())
        if (pending.size == 2) return@withLock failed(ProbeQueueFullException())
        val work = Work(operation)
        pending.addLast(work)
        changed.signal()
        work.ticket
    }

    // Terminal admission fence: reject pending delegates, allow the one claimed
    // operation to settle, then run one bounded control barrier on the worker.
    fun <T> beginLogout(invalidateNative: () -> T): Ticket<T> = gate.withLock {
        if (closing) return@withLock failed(ProbeWorkerClosedException())
        if (logoutStarted) return@withLock failed(ProbeWorkerInvalidatedException())
        logoutStarted = true
        while (pending.isNotEmpty()) pending.removeFirst().reject(ProbeWorkerInvalidatedException())
        // Once admission is fenced, invalidation is mandatory. Exposing the
        // ordinary pending cancellation path could otherwise skip this barrier.
        val barrier = Work(invalidateNative, cancellable = false)
        logoutBarrier = barrier
        changed.signal()
        barrier.ticket
    }

    // Normal close drains admitted work. The caller only gets a ticket; native
    // cleanup runs exactly once in the dedicated worker's finally block.
    fun closeAsync(): Ticket<Unit> = gate.withLock {
        closing = true
        changed.signal()
        Ticket(closed)
    }

    private fun runWorker() {
        var failure: Throwable? = null
        try {
            while (true) {
                val claimed = gate.withLock {
                    while (pending.isEmpty() && logoutBarrier == null && !closing) changed.await()
                    val work = logoutBarrier?.also { logoutBarrier = null } ?: pending.pollFirst()
                    // Claim and logout share the gate. A dequeued-but-unclaimed
                    // item cannot escape the fence. Cancellation races this CAS.
                    work?.let { Pair(it, it.tryStart()) }
                } ?: break
                claimed.first.execute(claimed.second)
            }
        } catch (exception: Throwable) {
            failure = exception
            gate.withLock {
                closing = true
                while (pending.isNotEmpty()) pending.removeFirst().reject(exception)
                logoutBarrier?.reject(exception)
                logoutBarrier = null
            }
        } finally {
            try {
                closeNative()
            } catch (exception: Throwable) {
                val previous = failure
                if (previous == null) failure = exception else previous.addSuppressed(exception)
            }
            val finalFailure = failure
            if (finalFailure == null) closed.complete(Unit) else closed.completeExceptionally(finalFailure)
        }
    }

    private interface WorkItem {
        fun tryStart(): Boolean
        fun execute(started: Boolean)
        fun reject(exception: Throwable)
    }

    private class Work<T>(private val operation: () -> T, private val cancellable: Boolean = true) : WorkItem {
        private val state = AtomicInteger(PENDING)
        private val result = CompletableFuture<T>()
        val ticket = Ticket(result) { cancelPending() }

        private fun cancelPending(): Boolean {
            if (!cancellable) return false
            if (!state.compareAndSet(PENDING, CANCELED)) return false
            result.completeExceptionally(CancellationException("Canceled before native execution was claimed."))
            return true
        }

        override fun tryStart(): Boolean = state.compareAndSet(PENDING, RUNNING)

        override fun execute(started: Boolean) {
            if (!started) return
            try {
                result.complete(operation())
            } catch (exception: Throwable) {
                result.completeExceptionally(exception)
            } finally {
                state.set(FINISHED)
            }
        }

        override fun reject(exception: Throwable) {
            if (state.compareAndSet(PENDING, FINISHED)) result.completeExceptionally(exception)
        }

        private companion object {
            const val PENDING = 0
            const val RUNNING = 1
            const val FINISHED = 2
            const val CANCELED = 3
        }
    }

    private fun <T> failed(exception: Throwable): Ticket<T> = Ticket(CompletableFuture<T>().also {
        it.completeExceptionally(exception)
    })
}
