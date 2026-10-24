# Native interoperability probe: N00 and part of N02

This is a local-only feasibility probe, not an Android application, WinUI app,
authentication implementation, production sync engine, or shipping SDK.
Use only synthetic data and a test-owned database. It never contacts Sprøyt,
Authentik or a network server at runtime. Dependency restore may use package
registries while building.

## Run

Prerequisites: the repository's Rust 1.96 toolchain and a working native C
compiler/linker for bundled SQLite, PowerShell 7, and .NET SDK 10.0.301 (or its
allowed patch update). The runner does not install Android or Apple tools.

```powershell
./native/probes/tooling-inventory.ps1
./native/probes/run-probe.ps1 -ArtifactsDirectory C:/Users/haral/.codex/build/sproyt-native-probe
```

Choose a private artifact directory on a disk with enough space. On this host,
builds stay on C: because S: has little free space. CI passes its runner temp
directory instead. The runner restores its temporary environment changes and
does not recursively delete anything. Each run gets a fresh fixture directory;
the database and a small JSON receipt are retained for inspection.

The runner checks formatting, lints and Rust tests, builds the real native
library and managed console, and executes separate managed processes:

1. **seed** saves a draft, atomically admits it to a durable local outbox, and
   marks dispatch in progress. It then waits with the library/database open.
2. **busy** proves another process cannot open that same owned database.
3. The parent retains a SHA-256 of the actual persisted wire payload, then
   forcibly terminates seed after its committed dispatch marker.
4. **resume** reopens SQLite, recovers uncertain delivery, exposes the original
   request bytes, rejects changed-payload reuse, and simulates replay and ACK.
   It also installs a synthetic incoming edit.
5. **verify** starts again, checks persisted ACK, account binding and the
   persisted generation fence after invalidation.
6. **worker** calls the ordinary native library on a dedicated C# worker
   thread. Deterministic gates exercise two-slot queue saturation, cancellation
   before claim and after commit, logout fencing, and draining shutdown with
   exactly one native close on that worker.
7. A separate, explicitly enabled diagnostic build pauses a child after outbox
   insertion, after draft removal before commit, and after commit before the
   API response. The parent receives and hashes the canonical payload before
   killing/reaping each child. **fault_verify** checks the recovered state
   before retry, then verifies one complete admission and the same bytes.

Rust and C# read the same fictional scenario in
`scenarios/persistence-v1.json`. The Rust test independently compares payload
before and after close/reopen. The managed runner compares the recovered
payload with the independently parent-retained pre-kill hash and with replay.
The schema-2 receipt records both library hashes, the pre-termination payload
hash and each recovered crash site. A receipt is emitted only after all phases
pass. The default native library is separately checked to reject diagnostic
commands; the `fault-injection` feature is disabled by default.

## Deliberate boundaries

- The standalone Cargo workspace has its own lockfile and bundled
  `rusqlite`/SQLite dependency. Production `Cargo.toml`, `Cargo.lock`, server
  code and the existing WASM-compatible core are unchanged. Reusing this as
  shipping code would require a dependency/SQLite-version decision; it must
  not simply be added to the server workspace and assumed compatible.
- Reuse of `sproyt-client-core` covers pure delivery decisions. Its current
  events do not enter `Uncertain` after a process restart; this probe's storage
  adapter explicitly owns that recovery transition.
- SQLite has exclusive ownership while open. The process registry serializes
  all calls, including close. Native calls remain synchronous. The C# lifecycle
  probe keeps them on one dedicated worker; this is not a WinUI dispatcher,
  a production scheduler or a multi-process architecture decision.
- Schema handling creates v1, reopens it and rejects unsupported versions.
  There is no older-schema migration, full sync cursor, retention/replay-age
  policy, or account reactivation flow here.
- The local store allows at most eight drafts, eight outbox records and eight
  incoming records, with 1 KiB UTF-8 body limits. Terminal outbox records still
  count toward this probe limit. Media bytes, secrets and real identities do
  not belong in these fixtures.
- Dispatch and acknowledgement are simulated locally. There is no server
  deduplication or exactly-once delivery proof. Recovery after a forced process
  exit does not establish power-loss or storage-controller durability.
- Rust tests cover read-only failure, injected errors between transaction
  operations and after commit, and a real commit failure from a deferred
  foreign-key constraint. The last test proves the pre-commit site is reached
  before allowing SQLite itself to reject commit. These are not physically
  full disk, `SQLITE_FULL`, or commit-time I/O fault tests.
- Invalidation removes logical rows and persists an inactive generation; it
  is not secure file erasure. Key storage, encrypted caches and native privacy
  UI have not been implemented.

## Worker lifecycle contract

`managed/ProbeWorker.cs` admits at most two pending delegates plus the running
operation; saturation immediately faults the returned task. It never waits for
queue capacity. A canceled pending delegate is never executed, but still holds
its queue slot until drained. Execution is claimed under the same gate as
logout, before the delegate enters P/Invoke. Once claimed, cancellation cannot
hide the actual result or exception. The tests gate both sides of that boundary.

`BeginLogout` immediately closes admission and rejects queued delegates; its
single control-barrier slot invalidates the native generation after already
running work finishes. Logout does not cancel a native transaction halfway
through. Normal `CloseAsync` closes admission, drains admitted work and closes
the native handle exactly once on its worker. Repeated disposal shares that
completion. A failed logout must be observed by the caller, not reported as
successful. These are local lifecycle semantics, not remote session revocation.

Diagnostic `arm_fault` requests target the next decoded `enqueue` attempt on
that handle. Validation or an idempotent duplicate fast path consumes the
one-shot hook even when the chosen site is not reached. A post-commit error is
an **unknown acknowledgement**, never evidence of rollback. Retrying uses the
same request ID and payload. Diagnostic pauses intentionally never return; the
runner has a bounded marker deadline and kills/reaps its own child on failure.

## Small C ABI

See `client-core/include/sproyt_probe.h`. ABI version is 1. The managed runner
loads the exact caller-supplied library path, using the platform's native
architecture. No unmanaged data structures or Rust enums cross the boundary.

- Input: one UTF-8 JSON request, at most 16 KiB.
- Output: a caller-owned **256 KiB buffer allocated before execution**. Return
  is the exact JSON byte count without a NUL terminator. Do not repeat a
  mutation to find its response size.
- JSON response: `ok:true,result:...` or `ok:false,error:<bounded code>`.
- ABI errors: -1 invalid/null arguments, -2 insufficient output capacity,
  -3 oversized input, -4 internal panic/response invariant failure. The first
  three execute no command; -4 has an unknown outcome and must not be blindly
  replayed as a fresh request. The boundary cannot catch every abort, including
  allocator failure.
- Foreign callers must provide valid, nonoverlapping, stable input and output
  regions of the stated lengths. The library cannot validate arbitrary dangling
  pointers. Handles are monotonically allocated and never reused during that
  library-load lifetime; stale handles and double close are rejected. Do not
  unload/reload the library while handles or calls are outstanding.

Operations: `open(database,account)`, `close(handle)`, `snapshot(handle)`,
`save_draft(handle,generation,channel,body)`,
`enqueue(handle,generation,request_id,channel,body)`,
`delivery(handle,generation,request_id,action)`,
`apply_message(handle,generation,message_id,revision,body,deleted)`, and
`invalidate(handle,generation)`. The managed code is the complete executable
example. A fixture account string is a local partition, not authentication.

## Kotlin and Swift desktop bindings

`run-language-probe.ps1` adds two desktop-language probes against the same
Rust store and shared Unicode scenario. Kotlin/JVM uses a feature-gated JNI
adapter that delegates once to the existing C ABI; copied input/output buffers
avoid retaining JNI critical arrays during SQLite work. Swift imports the
authoritative C header through a module map and verifies the actual loaded
library path. Neither language implements persistence or a second outbox.

```powershell
# Requires a JDK and Maven; CI supplies Temurin 21 on Linux.
./native/probes/run-language-probe.ps1 -Language kotlin -ArtifactsDirectory /tmp/sproyt-native-languages
# Requires a Mac with Swift 6 or later; CI uses macos-15.
./native/probes/run-language-probe.ps1 -Language swift -ArtifactsDirectory /tmp/sproyt-native-languages
```

These commands check tool availability and never install a local SDK. They
build outside the source tree, retain fresh test-owned fixtures, and run ABI
guards, separate seed/busy/recovery/verification processes and a language-owned
bounded worker. The parent retains the pre-kill payload hash. Each worker
exercises saturation, pending cancellation, claimed work before/after mutation,
logout admission fencing and drain/close exactly once. A post-execution bridge
or reply failure is an unknown outcome; no wrapper automatically retries it.

Language receipts have their own schema 1 and identify the language, actual
compiler/runtime, platform, source revision/dirty status, ordinary library hash,
and twelve outcomes. JNI and fault-injection are independent optional features;
ordinary language probes reject diagnostic commands. Existing C# transaction
crash tests remain in place rather than being duplicated in each wrapper.

This is desktop binding feasibility. Kotlin on HotSpot is not Android/ART,
and Swift on macOS is not an iOS package. Compose/SwiftUI, OS credential stores,
physical devices, generated-binding selection, packaging, schema migration and
native authentication still require their own gates. See
[the language probe contract](../../docs/native-client-language-probes.md).

## Acceptance status

See [the recorded results](../../docs/native-client-probe-results.md) and
[the implementation plan](../../docs/native-client-implementation-plan.md).
Hosted Windows/Linux checks run in `native-probe.yml`; their result must be
recorded separately from the local Windows ARM64 run. Kotlin, Swift, WinUI,
Android/iOS packaging and physical devices remain outside this probe.
