# Native upload contracts

Run from this directory with its pinned SDK and an isolated artifact directory:

```powershell
dotnet run --project UploadContracts.csproj --artifacts-path S:/Build/native-upload-owner/managed -- S:/Build/native-upload-owner/contracts
```

The runner links the production queue, HTTP adapter and session owner. Its server,
spool and atomic draft-merge hooks are controlled fault fixtures. On Windows it
also opens an actual DPAPI session journal with synthetic grants and HTTP, testing
the production allowlist, bearer/account binding, token rotation and logout.
That Windows-only check is explicitly omitted on Linux. No live Canary server,
real credentials, physical picker or production draft store is exercised here.

The application supplies an account-owned `INativeUploadLocalStore`: private
immutable source staging, atomic full-job journal writes and an idempotent
target-bound draft merge by job ID. `SaveAsync` must retain all login families
for this account. `AttachAsync` returns true only after the exact marker and its
receipt are durable. It never sends chat. False retains a Ready item for explicit
retry; a thrown/unknown merge result blocks advancement until reconciliation.

`NativeUploadQueue.OpenAsync` loads without dispatch. Bind its verified media
scope and pass a fresh admission predicate that includes current channel access.
`BindScope(scope, stillCurrent)` can additionally check the host's immutable scope
registration under the queue gate; that callback must be a short reference check,
without authentication locks, I/O or network work. `RecheckAdmission` wakes a
restored queue when authoritative channels or connectivity become ready, without
overriding backoff or changing any target.

Each job captures user, login family, channel and optional thread parent. Normal
token rotation keeps the family and uses a fresh concrete grant for every HTTP
operation. Another login family never silently resumes or retargets old work.
Same-user held jobs stay visible and removable so they cannot consume invisible
slots; another user sees no filename metadata. Old-family removal only retires
the local queue entry. A previously committed draft attachment may remain.

There are eight journalled jobs per account, one active transfer and one 1 MiB
chunk buffer. Source length is bounded at 1 GiB and the server's configured limit;
the host imposes its own private spool budget. Successful JSON replies are bounded
at 16 KiB. Stable request IDs, upload IDs, cursor/hash and source fingerprint are
retained through uncertain create/chunk/complete responses. Transient operations
have four bounded retries (0.5, 1, 2, 4 seconds); definitive HTTP rejection is not
automatically replayed. Source filenames are at most 200 UTF-16 units, family IDs
are ASCII up to 128 characters and stored errors are bounded to 128 runes.

All dispatch waits for the preceding durable journal write. A failed enqueue save
can still retain its source; the host must call `RetainsSource(sourceId)` on that
captured queue before releasing a staged file, independent of the logout-filtered
UI snapshot. A terminal removal failure retains a retry handle too.

Cancellation claims and handoff admission share one gate. Once a cancel claim
wins, its local journal transition finishes even if the caller cancels. Before
calling the host's atomic merge, Ready + `HandoffStarted` is durably journalled.
That already-admitted merge cannot be cancelled: the UI disables Cancel and an
unknown result requires idempotent reconciliation, including after restart.
Temporary offline state cannot retire that receipt. A definite no-capacity result
clears the flag, retains Ready/error and does not create an automatic retry loop.

The marker encodes its filename once with `Uri.EscapeDataString`, preserves whole
runes/escapes and caps the encoded label at 512 bytes. The original filename stays
in metadata. A marker fits an empty 1024-byte pilot composer, but an existing draft
may still need space before the host can merge it. Completed media remains Ready
until that merge succeeds; its source is not discarded as a failed upload.
