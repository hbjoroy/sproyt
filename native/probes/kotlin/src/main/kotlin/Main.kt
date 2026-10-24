package org.sproyt.probe

import java.nio.file.Files
import java.nio.file.Path
import java.security.MessageDigest
import java.util.concurrent.CountDownLatch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

internal object NativeBridge {
    init {
        val library = Path.of(checkNotNull(System.getenv("SPROYT_NATIVE_PROBE_LIBRARY")))
        check(library.isAbsolute && Files.isRegularFile(library)) { "Supply the exact absolute native probe library." }
        System.load(library.toString())
        check(abiVersion() == 1 && responseCapacity() == 256 * 1024) { "Unexpected native ABI contract." }
    }

    @JvmStatic external fun abiVersion(): Int
    @JvmStatic external fun responseCapacity(): Int
    @JvmStatic external fun call(input: ByteArray, output: ByteArray): Int
}

private fun rawCall(input: ByteArray): JsonObject {
    val output = ByteArray(NativeBridge.responseCapacity())
    // A native bridge exception or -4 may follow commit. Never automatically
    // repeat a mutation to get its output; reopen/recover the same request ID.
    val length = NativeBridge.call(input, output)
    check(length in 1..output.size) { "Native ABI failure $length; outcome may be unknown." }
    return Json.parseToJsonElement(output.copyOf(length).decodeToString(throwOnInvalidSequence = true)).jsonObject
}

private fun call(request: JsonObject, expectedError: String? = null): JsonObject {
    val reply = rawCall(request.toString().encodeToByteArray())
    if (expectedError != null) {
        check(!reply.getValue("ok").jsonPrimitive.boolean && reply["error"]?.jsonPrimitive?.content == expectedError) {
            "Expected $expectedError: $reply"
        }
        return buildJsonObject {}
    }
    check(reply.getValue("ok").jsonPrimitive.boolean) { reply.toString() }
    return reply.getValue("result").jsonObject
}

private fun request(op: String, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject = buildJsonObject {
    put("op", op)
    fields()
}

private fun hash(value: String): String = MessageDigest.getInstance("SHA-256")
    .digest(value.encodeToByteArray()).joinToString("") { "%02x".format(it.toInt() and 255) }

fun main(args: Array<String>) {
    try {
        check(args.size >= 3) { "Supply phase, test-owned database, shared fixture and optional parent hash." }
        val phase = args[0]
        val database = Path.of(args[1]).toAbsolutePath()
        val fixture = Json.parseToJsonElement(Files.readString(Path.of(args[2]))).jsonObject
        check(fixture.getValue("schema").jsonPrimitive.int == 1)
        val account = fixture.getValue("account").jsonPrimitive.content
        val channel = fixture.getValue("channel").jsonPrimitive.content
        val requestId = fixture.getValue("request_id").jsonPrimitive.content
        val body = fixture.getValue("body").jsonPrimitive.content
        fun open(): Long = call(request("open") {
            put("database", database.toString()); put("account", account)
        }).getValue("handle").jsonPrimitive.long
        fun command(op: String, handle: Long, fields: JsonObjectBuilder.() -> Unit = {}): JsonObject = request(op) {
            put("handle", handle); fields()
        }
        fun snapshot(handle: Long) = call(command("snapshot", handle))
        fun enqueue(handle: Long, text: String = body) = command("enqueue", handle) {
            put("generation", 1); put("request_id", requestId); put("channel", channel); put("body", text)
        }
        fun delivery(handle: Long, action: String) = call(command("delivery", handle) {
            put("generation", 1); put("request_id", requestId); put("action", action)
        })
        fun save(handle: Long) = command("save_draft", handle) {
            put("generation", 1); put("channel", channel); put("body", body)
        }

        when (phase) {
            "guard" -> {
                check(!Files.exists(database))
                call(request("arm_fault") { put("handle", 0); put("generation", 1); put("site", "after_insert"); put("mode", "pause") }, "invalid_request")
                val handle = open()
                check(NativeBridge.call(save(handle).toString().encodeToByteArray(), ByteArray(1)) == -2)
                check(NativeBridge.call(ByteArray(16 * 1024 + 1) { 32 }, ByteArray(NativeBridge.responseCapacity())) == -3)
                check(NativeBridge.call(byteArrayOf(), ByteArray(NativeBridge.responseCapacity())) == -1)
                val invalid = rawCall(byteArrayOf(255.toByte()))
                check(!invalid.getValue("ok").jsonPrimitive.boolean && invalid["error"]?.jsonPrimitive?.content == "invalid_request")
                check(snapshot(handle).getValue("drafts").jsonArray.isEmpty())
                call(command("close", handle))
                println("PASS guard: JNI input bounds, invalid UTF-8, no mutation and diagnostics excluded")
            }
            "worker" -> WorkerProbe.run(::call, database, account, channel)
            "seed" -> {
                check(!Files.exists(database))
                val handle = open()
                call(save(handle))
                call(enqueue(handle))
                delivery(handle, "dispatch")
                val state = snapshot(handle)
                check(state.getValue("drafts").jsonArray.isEmpty())
                val entry = state.getValue("outbox").jsonArray.single().jsonObject
                check(entry.getValue("state").jsonPrimitive.content == "dispatching")
                println("PROBE_HASH " + hash(entry.getValue("payload").jsonPrimitive.content))
                println("PASS seed: Kotlin/JNI journaled the shared Unicode payload")
                System.out.flush()
                CountDownLatch(1).await() // Parent kills this process with its native handle open.
            }
            "busy" -> {
                call(request("open") { put("database", database.toString()); put("account", account) }, "store_busy")
                println("PASS busy: JNI process respects exclusive database ownership")
            }
            "resume" -> {
                check(args.size == 4 && args[3].matches(Regex("[0-9a-f]{64}")))
                val handle = open()
                val recovered = snapshot(handle).getValue("outbox").jsonArray.single().jsonObject
                check(recovered.getValue("request_id").jsonPrimitive.content == requestId)
                check(recovered.getValue("state").jsonPrimitive.content == "uncertain")
                val wire = recovered.getValue("payload").jsonPrimitive.content
                check(hash(wire) == args[3]) { "Recovered bytes differ from the parent-retained pre-kill hash." }
                check(Json.parseToJsonElement(wire).jsonObject.getValue("payload").jsonObject.getValue("body").jsonPrimitive.content == body)
                call(enqueue(handle, "Changed"), "request_conflict")
                repeat(2) { call(enqueue(handle)) }
                check(snapshot(handle).getValue("outbox").jsonArray.size == 1)
                delivery(handle, "dispatch")
                check(snapshot(handle).getValue("outbox").jsonArray.single().jsonObject.getValue("payload").jsonPrimitive.content == wire)
                repeat(2) { delivery(handle, "acknowledge") }
                for ((revision, text) in listOf(1 to "Before", 2 to "After")) {
                    call(command("apply_message", handle) {
                        put("generation", 1); put("message_id", "fixture-message"); put("revision", revision)
                        put("body", text); put("deleted", false)
                    })
                }
                check(snapshot(handle).getValue("messages").jsonArray.single().jsonObject.getValue("body").jsonPrimitive.content == "After")
                call(command("close", handle))
                call(command("snapshot", handle), "invalid_handle")
                println("PASS resume: parent hash, same-ID replay, duplicate ACK, edit and stale handle")
            }
            "verify" -> {
                call(request("open") { put("database", database.toString()); put("account", "fixture-bob") }, "account_mismatch")
                var handle = open()
                check(snapshot(handle).getValue("outbox").jsonArray.single().jsonObject.getValue("state").jsonPrimitive.content == "acknowledged")
                call(command("invalidate", handle) { put("generation", 1) })
                call(save(handle), "stale_generation")
                call(command("close", handle))
                handle = open()
                val state = snapshot(handle)
                check(!state.getValue("active").jsonPrimitive.boolean && state.getValue("generation").jsonPrimitive.int == 2)
                check(state.getValue("drafts").jsonArray.isEmpty() && state.getValue("outbox").jsonArray.isEmpty())
                call(save(handle), "stale_generation")
                call(command("close", handle))
                println("PASS verify: persisted ACK, account binding and logout generation fence")
            }
            else -> error("Unsupported probe phase")
        }
    } catch (exception: Throwable) {
        System.err.println("FAIL Kotlin probe: ${exception.message}")
        kotlin.system.exitProcess(1)
    }
}
