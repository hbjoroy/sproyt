# Upload application admission contracts

This executable links the production `LiveUploadApplication.cs`, upload queue,
HTTP adapter and upload contracts. Only the auth, disk and workspace boundaries
are synthetic. It performs no network, DPAPI, Rust journal or WinUI acceptance;
the HTTP, disk/Workspace and native UI probes establish those separate layers.

Run from this directory with its pinned SDK:

```powershell
dotnet run --project UploadAdmissionContracts.csproj --artifacts-path S:/Build/native-content-admission/build -- S:/Build/native-content-admission/receipt.json
```

The tests call the actual application's upload methods while holding a source
read open. They verify that offline, logout, account replacement and login-family
replacement cancel active staging and an admission waiter, while normal token
rotation and channel navigation preserve the original immutable target. Closing
the application cancels staging before waiting for admission and disposing disk
ownership.

The race test suspends a real staging cancellation callback outside the auth
gate, binds a new family concurrently, and then resumes the earlier callback.
The actual queue's registration predicate prevents the old callback from
overwriting the newer scope. The concurrent bind also proves that cancellation
does not hold the auth projection gate.

The handoff tests call the production attachment callback: definitive composer
capacity/busy rejection returns false for a retryable Ready job; unknown commit
and pending receipt outcomes still throw into the journal-reconciliation path.
The actual old-family removal method also waits for an existing handoff receipt
to settle before retiring its journal record. A failed/unknown settlement keeps
the job, and an account change during settlement blocks late retirement while
hiding the previous account's filename metadata.
No production test hook or alternate auth factory is added.
