import Foundation
import CryptoKit
import CProbe
import Darwin

struct ProbeFailure: Error, CustomStringConvertible, Sendable {
    let description: String
    init(_ message: String) { description = message }
}

func check(_ condition: @autoclosure () throws -> Bool, _ message: String) throws {
    if try !condition() { throw ProbeFailure(message) }
}

// Immutable encoded JSON crosses the probe worker boundary. Foundation's
// mutable [String: Any] trees stay local to the synchronous caller.
struct JSON: Sendable {
    let data: Data
    init(_ object: Any) throws {
        data = try JSONSerialization.data(withJSONObject: object, options: [.sortedKeys])
    }
    init(data: Data) throws {
        _ = try JSONSerialization.jsonObject(with: data)
        self.data = data
    }
    var object: [String: Any] {
        // Every constructor validates JSON, and all probe replies are objects.
        (try? JSONSerialization.jsonObject(with: data) as? [String: Any]) ?? [:]
    }
    func string(_ key: String) throws -> String {
        guard let value = object[key] as? String else { throw ProbeFailure("Missing string: \(key)") }
        return value
    }
    func uint(_ key: String) throws -> UInt64 {
        guard let value = object[key] as? NSNumber else { throw ProbeFailure("Missing integer: \(key)") }
        return value.uint64Value
    }
    func bool(_ key: String) throws -> Bool {
        guard let value = object[key] as? Bool else { throw ProbeFailure("Missing boolean: \(key)") }
        return value
    }
    func array(_ key: String) throws -> [JSON] {
        guard let values = object[key] as? [[String: Any]] else { throw ProbeFailure("Missing array: \(key)") }
        return try values.map { try JSON($0) }
    }
    func first(_ key: String) throws -> JSON {
        guard let value = try array(key).first else { throw ProbeFailure("Empty array: \(key)") }
        return value
    }
}

struct Fixture: Decodable, Sendable {
    let schema: Int
    let account: String
    let channel: String
    let request_id: String
    let body: String
}

enum Native {
    static let capacity = 256 * 1024

    static func validate() throws {
        guard let path = ProcessInfo.processInfo.environment["SPROYT_NATIVE_PROBE_LIBRARY"] else {
            throw ProbeFailure("Set SPROYT_NATIVE_PROBE_LIBRARY to the linked Rust probe library")
        }
        try check((path as NSString).isAbsolutePath && FileManager.default.fileExists(atPath: path), "Native library path must be an existing absolute path")
        // The function itself remains a compile-time C import. This C calling
        // convention annotation is only for inspecting its image address;
        // no dynamic lookup or handwritten invocation replaces the header.
        let importedVersion: @convention(c) () -> UInt32 = sproyt_probe_abi_version
        let address = unsafeBitCast(importedVersion, to: UnsafeRawPointer.self)
        var info = Dl_info()
        try check(dladdr(address, &info) != 0, "Cannot identify the loaded native library")
        guard let imageName = info.dli_fname else { throw ProbeFailure("Loaded native image has no path") }
        let loaded = URL(fileURLWithPath: String(cString: imageName)).resolvingSymlinksInPath().standardizedFileURL.path
        let expected = URL(fileURLWithPath: path).resolvingSymlinksInPath().standardizedFileURL.path
        try check(loaded == expected, "Loaded native library does not match SPROYT_NATIVE_PROBE_LIBRARY: \(loaded)")
        try check(sproyt_probe_abi_version() == 1, "Unexpected ABI version")
        try check(sproyt_probe_response_capacity() == capacity, "Unexpected ABI response capacity")
    }

    // Exactly one native execution. Pointers cannot escape these nested scopes;
    // the output is a separate buffer and only the returned byte count is read.
    static func raw(_ input: Data, outputCapacity: Int = capacity) -> (Int32, Data) {
        var output = [UInt8](repeating: 0, count: outputCapacity)
        let count = input.withUnsafeBytes { source in
            output.withUnsafeMutableBufferPointer { target in
                sproyt_probe_call(source.bindMemory(to: UInt8.self).baseAddress,
                                  input.count, target.baseAddress, target.count)
            }
        }
        guard count > 0 && Int(count) <= output.count else { return (count, Data()) }
        return (count, Data(output.prefix(Int(count))))
    }

    static func call(_ request: [String: Any], expecting error: String? = nil) throws -> JSON {
        let input = try JSON(request).data
        try check(input.count <= 16 * 1024, "Probe request exceeds input budget")
        return try reply(raw(input), expecting: error)
    }

    static func reply(_ response: (Int32, Data), expecting error: String? = nil) throws -> JSON {
        if response.0 == -4 {
            throw ProbeFailure("Native failure after possible execution: outcome unknown; never automatically retry a mutation")
        }
        try check(response.0 > 0, "ABI rejected the call before execution: \(response.0)")
        try check(Int(response.0) == response.1.count, "Invalid reply length after execution: outcome unknown; never automatically retry a mutation")
        let envelope: JSON
        do { envelope = try JSON(data: response.1) }
        catch { throw ProbeFailure("Unreadable native reply after execution: outcome unknown; never automatically retry a mutation") }
        if let error {
            try check(try envelope.bool("ok") == false, "Expected error: \(error)")
            try check(try envelope.string("error") == error, "Unexpected native error")
            return try JSON([:])
        }
        try check(try envelope.bool("ok"), "Native error: \(String(decoding: response.1, as: UTF8.self))")
        guard let result = envelope.object["result"] as? [String: Any] else {
            throw ProbeFailure("Missing native result after execution: outcome unknown; never automatically retry a mutation")
        }
        return try JSON(result)
    }

    static func hash(_ string: String) -> String {
        SHA256.hash(data: Data(string.utf8)).map { String(format: "%02x", $0) }.joined()
    }
}
