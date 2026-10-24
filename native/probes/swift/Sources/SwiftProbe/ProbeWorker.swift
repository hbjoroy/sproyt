import Foundation

enum WorkerFailure: Error, Equatable {
    case queueFull, closed, invalidated, canceled, deadline
}

// All mutable fields in the Sendable reference types below are protected by
// their condition/lock. No Swift or C callback crosses the native ABI.
final class Completion: @unchecked Sendable {
    private let condition = NSCondition()
    private var result: Result<JSON, Error>?

    var isCompleted: Bool {
        condition.lock(); defer { condition.unlock() }
        return result != nil
    }

    func finish(_ result: Result<JSON, Error>) {
        condition.lock(); defer { condition.unlock() }
        guard self.result == nil else { return }
        self.result = result
        condition.broadcast()
    }

    func wait() throws -> JSON {
        condition.lock(); defer { condition.unlock() }
        let deadline = Date().addingTimeInterval(10)
        while result == nil {
            if !condition.wait(until: deadline), result == nil { throw WorkerFailure.deadline }
        }
        return try result!.get()
    }
}

final class Job: @unchecked Sendable {
    private enum State: Equatable { case pending, running, finished }
    private let lock = NSLock()
    private var state = State.pending
    private let operation: @Sendable () throws -> JSON
    private let cancellable: Bool
    let completion = Completion()

    init(cancellable: Bool = true, _ operation: @escaping @Sendable () throws -> JSON) {
        self.cancellable = cancellable
        self.operation = operation
    }

    // Claim and cancellation share this lock. Cancellation after claim cannot
    // replace either a committed result or the actual operation failure.
    func claim() -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard state == .pending else { return false }
        state = .running
        return true
    }

    func reject(_ error: WorkerFailure) {
        lock.lock(); defer { lock.unlock() }
        guard state == .pending else { return }
        state = .finished
        completion.finish(.failure(error))
    }

    func cancel() { if cancellable { reject(.canceled) } }

    func execute() {
        let result = Result { try operation() }
        lock.lock()
        state = .finished
        lock.unlock()
        completion.finish(result)
    }
}

final class ProbeWorker: @unchecked Sendable {
    private let condition = NSCondition()
    private var pending: [Job] = []
    private var logoutBarrier: Job?
    private var loggingOut = false
    private var closing = false
    private let closeNative: @Sendable () throws -> Void
    private let closed = Completion()

    init(closeNative: @escaping @Sendable () throws -> Void) {
        self.closeNative = closeNative
        // The thread owns this instance through drain and native close.
        let thread = Thread { [self] in run() }
        thread.name = "Sproyt Swift native lifecycle probe"
        thread.start()
    }

    func queue(canceled: Bool = false, _ operation: @escaping @Sendable () throws -> JSON) -> Job {
        let job = Job(operation)
        condition.lock(); defer { condition.unlock() }
        if closing { job.reject(.closed) }
        else if loggingOut { job.reject(.invalidated) }
        else if canceled { job.reject(.canceled) }
        else if pending.count == 2 { job.reject(.queueFull) }
        else { pending.append(job); condition.signal() }
        return job
    }

    func logout(_ operation: @escaping @Sendable () throws -> JSON) -> Job {
        let barrier = Job(cancellable: false, operation)
        condition.lock(); defer { condition.unlock() }
        if closing { barrier.reject(.closed) }
        else if loggingOut { barrier.reject(.invalidated) }
        else {
            loggingOut = true
            for job in pending { job.reject(.invalidated) }
            pending.removeAll()
            logoutBarrier = barrier
            condition.signal()
        }
        return barrier
    }

    @discardableResult func close() -> Completion {
        condition.lock(); defer { condition.unlock() }
        closing = true
        condition.signal()
        return closed
    }

    private func run() {
        while true {
            condition.lock()
            while pending.isEmpty && logoutBarrier == nil && !closing { condition.wait() }
            let next: Job?
            if let barrier = logoutBarrier { next = barrier; logoutBarrier = nil }
            else if !pending.isEmpty { next = pending.removeFirst() }
            else { next = nil }
            // Claim under the admission lock: logout cannot pass a dequeued
            // pending delegate before its execution boundary.
            let claimed = next?.claim() ?? false
            condition.unlock()
            guard let next else { break }
            if claimed { next.execute() }
        }
        do {
            try closeNative()
            closed.finish(.success(try JSON([:])))
        } catch { closed.finish(.failure(error)) }
    }
}

final class ExecutionGate: @unchecked Sendable {
    private let condition = NSCondition()
    private var entered = false
    private var released = false

    func hold(_ operation: () throws -> JSON) throws -> JSON {
        condition.lock()
        entered = true
        condition.broadcast()
        let deadline = Date().addingTimeInterval(10)
        while !released {
            if !condition.wait(until: deadline), !released {
                condition.unlock()
                throw WorkerFailure.deadline
            }
        }
        condition.unlock()
        return try operation()
    }

    func waitUntilEntered() throws {
        condition.lock(); defer { condition.unlock() }
        let deadline = Date().addingTimeInterval(10)
        while !entered {
            if !condition.wait(until: deadline), !entered { throw WorkerFailure.deadline }
        }
    }

    func release() {
        condition.lock(); defer { condition.unlock() }
        released = true
        condition.broadcast()
    }
}

// Handles and observations are guarded as well as confined to one worker.
// This is a probe harness, not a proposed shipping SDK wrapper.
final class NativeSession: @unchecked Sendable {
    private let lock = NSLock()
    private let caller = Thread.current
    private var owner: Thread?
    private var handle: UInt64 = 0
    private var closes = 0
    private var marked = false

    var closeCount: Int { lock.lock(); defer { lock.unlock() }; return closes }
    var delegateRan: Bool { lock.lock(); defer { lock.unlock() }; return marked }
    func markDelegate() { lock.lock(); defer { lock.unlock() }; marked = true }

    func open(_ path: String, account: String) throws -> JSON {
        let result = try native(["op": "open", "database": path, "account": account])
        let opened = try result.uint("handle")
        lock.lock(); handle = opened; lock.unlock()
        return result
    }

    func request(_ op: String, _ fields: [String: Any] = [:], expecting error: String? = nil) throws -> JSON {
        lock.lock(); let current = handle; lock.unlock()
        var request = fields
        request["op"] = op
        request["handle"] = current
        return try native(request, expecting: error)
    }

    func close(requireMarked: Bool = false) throws {
        if requireMarked { try check(delegateRan, "Native close passed pending work") }
        lock.lock(); let current = handle; lock.unlock()
        if current != 0 { _ = try request("close") }
        lock.lock(); closes += 1; handle = 0; lock.unlock()
    }

    private func native(_ request: [String: Any], expecting error: String? = nil) throws -> JSON {
        lock.lock()
        let current = Thread.current
        if owner == nil { owner = current }
        let valid = current != caller && current == owner
        lock.unlock()
        try check(valid, "Native call left its dedicated worker thread")
        return try Native.call(request, expecting: error)
    }
}
