package org.sproyt.probe

import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.CancellationException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

internal object WorkerProbe {
    fun run(call: (JsonObject, String?) -> JsonObject, database: Path, account: String, channel: String) {
        check(!Files.exists(database)) { "Worker probe refuses to reuse an existing database." }
        val callerThread = Thread.currentThread()
        var nativeThread: Thread? = null
        val closeCount = AtomicInteger()
        var handle = 0L

        fun native(request: JsonObject, expectedError: String? = null): JsonObject {
            val current = Thread.currentThread()
            check(current !== callerThread && current.isDaemon) { "Native work did not run on a dedicated background worker." }
            if (nativeThread == null) nativeThread = current
            check(current === nativeThread) { "Native handle crossed worker threads." }
            return call(request, expectedError)
        }

        fun request(op: String, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject = buildJsonObject {
            put("op", op)
            put("handle", handle)
            fields()
        }

        fun save(body: String): JsonObject = request("save_draft") {
            put("generation", 1)
            put("channel", channel)
            put("body", body)
        }

        val worker = ProbeWorker {
            check(Thread.currentThread() === nativeThread) { "Native close ran outside its worker." }
            closeCount.incrementAndGet()
            if (handle != 0L) native(request("close"))
        }
        try {
            handle = worker.queue {
                native(buildJsonObject {
                    put("op", "open")
                    put("database", database.toAbsolutePath().toString())
                    put("account", account)
                }).getValue("handle").jsonPrimitive.long
            }.await()

            // Hold the active item so queue saturation and pending cancellation
            // do not depend on scheduler timing or sleep durations.
            ExecutionGate().use { gate ->
                val active = worker.queue { gate.hold { native(request("snapshot")) } }
                gate.waitUntilEntered()
                val canceledDelegateRan = AtomicBoolean()
                val canceledWrite = worker.queue {
                    canceledDelegateRan.set(true)
                    native(save("Never executed"))
                }
                val queuedSnapshot = worker.queue { native(request("snapshot")) }
                expect<ProbeQueueFullException>(worker.queue { native(request("snapshot")) })
                check(canceledWrite.cancel()) { "Pending cancellation did not claim the item." }
                expect<CancellationException>(canceledWrite)
                gate.release()
                active.await()
                check(queuedSnapshot.await().getValue("drafts").jsonArray.isEmpty()) { "A canceled write persisted." }
                check(!canceledDelegateRan.get()) { "A canceled pending delegate executed." }
            }

            // Execution claim is the cancellation boundary, before JNI entry.
            ExecutionGate().use { gate ->
                val claimed = worker.queue { gate.hold { native(save("Claimed work survives cancellation")) } }
                gate.waitUntilEntered()
                check(!claimed.cancel() && !claimed.isDone) { "Cancellation hid an already-claimed native operation." }
                gate.release()
                claimed.await()
                val snapshot = worker.queue { native(request("snapshot")) }.await()
                check(draftBody(snapshot) == "Claimed work survives cancellation") { "Claimed native write was skipped." }
            }

            // Once committed, cancellation must preserve the actual result.
            ExecutionGate().use { gate ->
                val committed = worker.queue {
                    val result = native(save("Committed result survives cancellation"))
                    gate.hold { result }
                }
                gate.waitUntilEntered()
                check(!committed.cancel() && !committed.isDone) { "Cancellation hid a committed native result." }
                gate.release()
                committed.await()
                val snapshot = worker.queue { native(request("snapshot")) }.await()
                check(draftBody(snapshot) == "Committed result survives cancellation") { "Committed native write was lost." }
            }

            ExecutionGate().use { gate ->
                val active = worker.queue { gate.hold { native(request("snapshot")) } }
                gate.waitUntilEntered()
                val oldWriteRan = AtomicBoolean()
                val oldWrite = worker.queue {
                    oldWriteRan.set(true)
                    native(save("Queued before logout"))
                }
                val logout = worker.beginLogout {
                    native(request("invalidate") { put("generation", 1) })
                    native(save("Late direct callback"), "stale_generation")
                    native(request("snapshot"))
                }
                expect<ProbeWorkerInvalidatedException>(oldWrite)
                expect<ProbeWorkerInvalidatedException>(worker.queue { native(request("snapshot")) })
                expect<ProbeWorkerInvalidatedException>(worker.beginLogout { Unit })
                check(!logout.cancel() && !logout.isDone) { "The mandatory logout barrier was canceled or passed an active native operation." }
                gate.release()
                active.await()
                val invalidated = logout.await()
                check(!oldWriteRan.get()) { "A queued old-generation write executed after the logout fence." }
                check(!invalidated.getValue("active").jsonPrimitive.boolean && invalidated.getValue("generation").jsonPrimitive.int == 2) {
                    "Logout did not invalidate the native generation."
                }
                check(invalidated.getValue("drafts").jsonArray.isEmpty()) { "Logout left drafts visible." }
            }
        } finally {
            worker.closeAsync().await()
            worker.closeAsync().await()
        }
        check(closeCount.get() == 1) { "Repeated close closed the native handle more than once." }
        verifyCloseWhileActive(call, Path.of(database.toString() + ".drain"), account, channel, callerThread)
        println("PASS JVM worker: bounded admission, cancellation boundary, logout fence and worker-thread drain/close")
    }

    private fun verifyCloseWhileActive(call: (JsonObject, String?) -> JsonObject, database: Path, account: String, channel: String, callerThread: Thread) {
        check(!Files.exists(database)) { "Drain probe refuses to reuse an existing database." }
        var handle = 0L
        var workerThread: Thread? = null
        val closeCount = AtomicInteger()
        val queuedWriteRan = AtomicBoolean()
        fun native(request: JsonObject): JsonObject {
            check(Thread.currentThread() === workerThread && workerThread !== callerThread) { "Drain native call left its worker." }
            return call(request, null)
        }
        fun request(op: String, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject = buildJsonObject {
            put("op", op)
            put("handle", handle)
            fields()
        }
        val worker = ProbeWorker {
            check(queuedWriteRan.get()) { "Native handle closed before admitted work drained." }
            native(request("close"))
            closeCount.incrementAndGet()
        }
        try {
            handle = worker.queue {
                workerThread = Thread.currentThread()
                native(buildJsonObject {
                    put("op", "open")
                    put("database", database.toAbsolutePath().toString())
                    put("account", account)
                }).getValue("handle").jsonPrimitive.long
            }.await()
            ExecutionGate().use { gate ->
                val active = worker.queue { gate.hold { native(request("snapshot")) } }
                gate.waitUntilEntered()
                val pending = worker.queue {
                    val result = native(request("save_draft") {
                        put("generation", 1)
                        put("channel", channel)
                        put("body", "Drain before close")
                    })
                    queuedWriteRan.set(true)
                    result
                }
                val closing = worker.closeAsync()
                check(!closing.isDone && closeCount.get() == 0) { "Close disposed the native handle during active work." }
                expect<ProbeWorkerClosedException>(worker.queue { native(request("snapshot")) })
                gate.release()
                active.await()
                pending.await()
                closing.await()
                worker.closeAsync().await()
            }
        } finally {
            worker.closeAsync().await()
        }
        check(closeCount.get() == 1 && queuedWriteRan.get()) { "Drain/close did not complete exactly once." }
    }

    private fun draftBody(snapshot: JsonObject): String? = snapshot.getValue("drafts").jsonArray[0]
        .jsonObject.getValue("body").jsonPrimitive.contentOrNull

    private inline fun <reified T : Throwable> expect(ticket: Ticket<*>) {
        try {
            ticket.await()
        } catch (exception: Throwable) {
            if (exception is T) return
            throw exception
        }
        error("Expected ${T::class.simpleName}.")
    }

    private class ExecutionGate : AutoCloseable {
        private val entered = CountDownLatch(1)
        private val released = CountDownLatch(1)
        fun <T> hold(operation: () -> T): T {
            entered.countDown()
            check(released.await(10, TimeUnit.SECONDS)) { "Timed out waiting to release the active worker call." }
            return operation()
        }
        fun waitUntilEntered() {
            check(entered.await(10, TimeUnit.SECONDS)) { "Timed out waiting for the worker to enter the gated operation." }
        }
        fun release() = released.countDown()
        override fun close() = release()
    }
}
