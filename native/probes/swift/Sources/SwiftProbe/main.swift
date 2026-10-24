import Foundation
import Darwin

func run() throws {
    let args = Array(CommandLine.arguments.dropFirst())
    try check(args.count >= 3 && ["seed", "busy", "resume", "verify", "worker", "guard"].contains(args[0]), "Supply phase database fixturePath [prekillhash]")
    let phase = args[0]
    try check(args.count == (phase == "resume" ? 4 : 3), "Unexpected probe arguments")
    let database = URL(fileURLWithPath: args[1]).standardizedFileURL.path
    let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: URL(fileURLWithPath: args[2])))
    try check(fixture.schema == 1, "Unsupported fixture schema")
    try Native.validate()

    func open() throws -> UInt64 {
        try Native.call(["op": "open", "database": database, "account": fixture.account]).uint("handle")
    }
    func snapshot(_ handle: UInt64) throws -> JSON { try Native.call(["op": "snapshot", "handle": handle]) }
    func close(_ handle: UInt64) throws { _ = try Native.call(["op": "close", "handle": handle]) }
    func write(_ op: String, _ handle: UInt64, body: String? = nil, expecting error: String? = nil) throws {
        var request: [String: Any] = ["op": op, "handle": handle, "generation": 1, "channel": fixture.channel, "body": body ?? fixture.body]
        if op == "enqueue" { request["request_id"] = fixture.request_id }
        _ = try Native.call(request, expecting: error)
    }
    func delivery(_ handle: UInt64, _ action: String) throws {
        _ = try Native.call(["op": "delivery", "handle": handle, "generation": 1, "request_id": fixture.request_id, "action": action])
    }

    switch phase {
    case "worker":
        try WorkerProbe.run(database: database, fixture: fixture)
    case "seed":
        try check(!FileManager.default.fileExists(atPath: database), "Seed refuses to overwrite an existing database")
        let handle = try open()
        try write("save_draft", handle)
        try write("enqueue", handle)
        try delivery(handle, "dispatch")
        let state = try snapshot(handle)
        try check(try state.array("drafts").isEmpty && state.array("outbox").count == 1, "Admission was not atomic")
        let outbox = try state.first("outbox")
        try check(try outbox.string("state") == "dispatching", "No durable dispatch")
        print("PROBE_HASH " + Native.hash(try outbox.string("payload")))
        print("PASS seed: Swift called Rust, journaled Unicode payload and atomically cleared draft")
        fflush(nil)
        // Parent kills the process with the Rust handle open; no close or Drop.
        // This proves process-termination recovery, not power-loss durability.
        while true { Thread.sleep(forTimeInterval: 60) }
    case "busy":
        _ = try Native.call(["op": "open", "database": database, "account": fixture.account], expecting: "store_busy")
        print("PASS busy: second process cannot take ownership of the open database")
    case "resume":
        let retainedHash = args[3]
        try check(retainedHash.count == 64 && retainedHash.allSatisfy { $0.isHexDigit }, "Resume requires the parent-retained hash")
        let handle = try open()
        let recovered = try snapshot(handle)
        try check(try recovered.array("outbox").count == 1, "Recovery did not retain one request")
        let item = try recovered.first("outbox")
        try check(try item.string("request_id") == fixture.request_id && item.string("state") == "uncertain", "Interrupted dispatch identity/state changed")
        let wire = try item.string("payload")
        // Check the independently retained pre-kill bytes before any retry.
        try check(Native.hash(wire) == retainedHash, "Recovered payload differs from pre-kill bytes")
        let payload = try JSON(data: Data(wire.utf8))
        let inner = try JSON(payload.object["payload"] ?? [:])
        try check(try inner.string("body") == fixture.body, "Unicode payload changed")
        try write("enqueue", handle, body: "Changed", expecting: "request_conflict")
        try write("enqueue", handle)
        try write("enqueue", handle)
        try check(try snapshot(handle).array("outbox").count == 1, "Same-ID retry created duplicates")
        try delivery(handle, "dispatch")
        try check(try snapshot(handle).first("outbox").string("payload") == wire, "Replay payload bytes changed")
        try delivery(handle, "acknowledge")
        try delivery(handle, "acknowledge")
        for (revision, body) in [(1, "Before"), (2, "After")] {
            _ = try Native.call(["op": "apply_message", "handle": handle, "generation": 1, "message_id": "fixture-message", "revision": revision, "body": body, "deleted": false])
        }
        let state = try snapshot(handle)
        try check(try state.array("outbox").count == 1 && state.first("messages").string("body") == "After", "Duplicate ACK or incoming edit failed")
        try close(handle)
        _ = try Native.call(["op": "snapshot", "handle": handle], expecting: "invalid_handle")
        print("PASS resume: original bytes replayed once; duplicate ACK, incoming edit and stale handle verified")
    case "verify":
        _ = try Native.call(["op": "open", "database": database, "account": "fixture-bob"], expecting: "account_mismatch")
        var handle = try open()
        try check(try snapshot(handle).first("outbox").string("state") == "acknowledged", "ACK did not persist")
        _ = try Native.call(["op": "invalidate", "handle": handle, "generation": 1])
        try write("save_draft", handle, body: "Late callback", expecting: "stale_generation")
        try close(handle)
        handle = try open()
        let state = try snapshot(handle)
        try check(try !state.bool("active") && state.uint("generation") == 2, "Logout generation fence did not persist")
        try check(try state.array("outbox").isEmpty, "Invalidated outbox remained visible")
        try write("save_draft", handle, body: "Late callback after reopen", expecting: "stale_generation")
        try close(handle)
        print("PASS verify: persisted ACK, account boundary and logout fence across reopen")
    case "guard":
        try check(!FileManager.default.fileExists(atPath: database), "Guard requires a fresh database")
        _ = try Native.call(["op": "arm_fault", "handle": 0, "generation": 1, "site": "after_insert", "mode": "pause"], expecting: "invalid_request")
        let handle = try open()
        let before = try snapshot(handle)
        _ = try Native.reply(Native.raw(Data([0xFF])), expecting: "invalid_request")
        _ = try Native.reply(Native.raw(Data("{".utf8)), expecting: "invalid_request")
        let mutation = try JSON(["op": "save_draft", "handle": handle, "generation": 1, "channel": fixture.channel, "body": "Must not persist"]).data
        try check(Native.raw(mutation, outputCapacity: Native.capacity - 1).0 == -2, "Undersized output accepted")
        var oversized = mutation
        oversized.append(Data(repeating: 0x20, count: 16 * 1024 + 1 - oversized.count))
        try check(Native.raw(oversized).0 == -3, "Oversized input accepted")
        try check(Native.raw(Data()).0 == -1, "Empty input accepted")
        let after = try snapshot(handle)
        try check(before.data == after.data, "Rejected ABI input mutated storage")
        try close(handle)
        print("PASS guard: ordinary library rejects diagnostic commands; invalid/bounded buffers do not mutate storage")
    default: throw ProbeFailure("Unsupported phase")
    }
}

do { try run() }
catch {
    FileHandle.standardError.write(Data("Probe failed: \(error)\n".utf8))
    exit(1)
}
