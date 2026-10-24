import Foundation

enum WorkerProbe {
    static func expect(_ failure: WorkerFailure, _ job: Job) throws {
        try check(job.completion.isCompleted, "Admission/cancellation did not settle immediately")
        do { _ = try job.completion.wait() }
        catch let error as WorkerFailure where error == failure { return }
        throw ProbeFailure("Expected worker rejection: \(failure)")
    }

    static func gated(_ body: (ExecutionGate) throws -> Void) rethrows {
        let gate = ExecutionGate()
        defer { gate.release() }
        try body(gate)
    }

    static func run(database: String, fixture: Fixture) throws {
        try check(!FileManager.default.fileExists(atPath: database), "Worker requires a fresh database")
        let session = NativeSession()
        let worker = ProbeWorker { try session.close() }
        defer { _ = try? worker.close().wait() }
        _ = try worker.queue { try session.open(database, account: fixture.account) }.completion.wait()

        try expect(.canceled, worker.queue(canceled: true) {
            session.markDelegate()
            return try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Never admitted"])
        })

        try gated { gate in
            let active = worker.queue { try gate.hold { try session.request("snapshot") } }
            try gate.waitUntilEntered()
            let pending = worker.queue {
                session.markDelegate()
                return try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Never executed"])
            }
            let snapshot = worker.queue { try session.request("snapshot") }
            try expect(.queueFull, worker.queue { try session.request("snapshot") })
            pending.cancel()
            try expect(.canceled, pending)
            gate.release()
            _ = try active.completion.wait()
            try check(try snapshot.completion.wait().array("drafts").isEmpty, "Pending cancellation mutated storage")
            try check(!session.delegateRan, "Canceled pending delegate executed")
        }

        try gated { gate in
            let claimed = worker.queue {
                try gate.hold {
                    try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Claimed work survives cancellation"])
                }
            }
            try gate.waitUntilEntered()
            claimed.cancel()
            try check(!claimed.completion.isCompleted, "Cancellation hid an already-claimed operation")
            gate.release()
            _ = try claimed.completion.wait()
            let snapshot = try worker.queue { try session.request("snapshot") }.completion.wait()
            try check(try snapshot.first("drafts").string("body") == "Claimed work survives cancellation", "Claimed native call was skipped")
        }

        try gated { gate in
            let committed = worker.queue {
                let result = try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Committed result survives cancellation"])
                return try gate.hold { result }
            }
            try gate.waitUntilEntered()
            committed.cancel()
            try check(!committed.completion.isCompleted, "Cancellation hid a native commit")
            gate.release()
            _ = try committed.completion.wait()
            let snapshot = try worker.queue { try session.request("snapshot") }.completion.wait()
            try check(try snapshot.first("drafts").string("body") == "Committed result survives cancellation", "Committed write was lost")
        }

        // An already-claimed failing call must surface its real native error
        // as well; cancellation cannot replace it with a canceled completion.
        try gated { gate in
            let failing = worker.queue {
                try gate.hold {
                    try session.request("save_draft", ["generation": 99, "channel": fixture.channel, "body": "Stale write"])
                }
            }
            try gate.waitUntilEntered()
            failing.cancel()
            try check(!failing.completion.isCompleted, "Cancellation hid a claimed failure")
            gate.release()
            var sawNativeFailure = false
            do {
                _ = try failing.completion.wait()
            } catch let error as ProbeFailure {
                try check(error.description.contains("stale_generation"), "Actual native error was replaced")
                sawNativeFailure = true
            }
            try check(sawNativeFailure, "Expected a native error from the claimed operation")
        }

        try gated { gate in
            let active = worker.queue { try gate.hold { try session.request("snapshot") } }
            try gate.waitUntilEntered()
            let pending = worker.queue {
                session.markDelegate()
                return try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Queued before logout"])
            }
            let logout = worker.logout {
                _ = try session.request("invalidate", ["generation": 1])
                _ = try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Late callback"], expecting: "stale_generation")
                return try session.request("snapshot")
            }
            try expect(.invalidated, pending)
            try expect(.invalidated, worker.queue { try session.request("snapshot") })
            try expect(.invalidated, worker.logout { try JSON([:]) })
            try check(!logout.completion.isCompleted, "Logout passed active work")
            gate.release()
            _ = try active.completion.wait()
            let invalidated = try logout.completion.wait()
            try check(!session.delegateRan, "Old-generation pending delegate ran")
            try check(try !invalidated.bool("active") && invalidated.uint("generation") == 2, "Logout did not invalidate generation")
            try check(try invalidated.array("drafts").isEmpty, "Logout left drafts visible")
        }

        _ = try worker.close().wait()
        _ = try worker.close().wait()
        try check(session.closeCount == 1, "Native handle closed more than once")
        try drain(database: database + ".drain", fixture: fixture)
        print("PASS worker: bounded admission, cancellation boundary, logout fence and worker-thread drain/close")
    }

    private static func drain(database: String, fixture: Fixture) throws {
        try check(!FileManager.default.fileExists(atPath: database), "Drain requires a fresh database")
        let session = NativeSession()
        let worker = ProbeWorker { try session.close(requireMarked: true) }
        defer { _ = try? worker.close().wait() }
        _ = try worker.queue { try session.open(database, account: fixture.account) }.completion.wait()
        try gated { gate in
            let active = worker.queue { try gate.hold { try session.request("snapshot") } }
            try gate.waitUntilEntered()
            let pending = worker.queue {
                let result = try session.request("save_draft", ["generation": 1, "channel": fixture.channel, "body": "Drain before close"])
                session.markDelegate()
                return result
            }
            let closing = worker.close()
            try check(!closing.isCompleted && session.closeCount == 0, "Close passed active work")
            try expect(.closed, worker.queue { try session.request("snapshot") })
            gate.release()
            _ = try active.completion.wait()
            _ = try pending.completion.wait()
            _ = try closing.wait()
            _ = try worker.close().wait()
        }
        try check(session.closeCount == 1 && session.delegateRan, "Close did not drain exactly once")
    }
}
