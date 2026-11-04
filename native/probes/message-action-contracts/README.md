# Native message action contracts

This runner links the production controller and uses controlled command results.
It covers edit/cancel/conflict, byte-preserved media markers, delete confirmation,
Unicode reactions, account/visit fences and uncertain outcomes. An uncertain
mutation permits authoritative readback; it never resends automatically.

Run from this directory with the pinned SDK and separate artifacts:

```powershell
dotnet run --project MessageActionContracts.csproj --artifacts-path S:/Build/native-message-actions/build -- S:/Build/native-message-actions/receipt.json
```

Transport and real WinUI are verified separately; this is not live Canary or
physical input acceptance.
