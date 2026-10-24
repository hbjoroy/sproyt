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
3. The runner forcibly terminates seed after its committed dispatch marker.
4. **resume** reopens SQLite, recovers uncertain delivery, exposes the original
   request bytes, rejects changed-payload reuse, and simulates replay and ACK.
   It also installs a synthetic incoming edit.
5. **verify** starts again, checks persisted ACK, account binding and the
   persisted generation fence after invalidation.

Rust and C# read the same fictional scenario in
`scenarios/persistence-v1.json`. The Rust test independently compares payload
before and after close/reopen. The managed runner compares the recovered
payload with replay; it does not store a separate pre-kill copy of those bytes.

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
  all calls, including close. This is a proof restriction, not a production
  scheduling, cancellation, multi-process or UI-thread design. All calls are
  synchronous and must stay off UI threads.
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
- The write-failure test uses SQLite `query_only=ON` (read-only failure), not a
  physically full disk, `SQLITE_FULL` or a commit-time I/O fault.
- Invalidation removes logical rows and persists an inactive generation; it
  is not secure file erasure. Key storage, encrypted caches and native privacy
  UI have not been implemented.

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

## Acceptance status

See [the recorded results](../../docs/native-client-probe-results.md) and
[the implementation plan](../../docs/native-client-implementation-plan.md).
Hosted Windows/Linux checks run in `native-probe.yml`; their result must be
recorded separately from the local Windows ARM64 run. Kotlin, Swift, WinUI,
Android/iOS packaging and physical devices remain outside this probe.
