# Native command and projection contracts

This standalone fixture compiles the production `NativeChatClient`, wire helper
and command exception against an explicitly synthetic session owner. A loopback
Kestrel websocket exercises the real command correlation, send path, receive
loop, reconnect and model decoder. It does not mint credentials or contact
Canary, and it does not replace the real session/DPAPI probes.

Run from this directory with the pinned .NET SDK and a unique output directory:

```powershell
dotnet run --project ClientContracts.csproj --artifacts-path S:/Build/native-parity-contracts/client-contracts -- S:/Build/native-parity-contracts/client-contracts-receipt.json
```

The fixture checks known domain rejection versus uncertain server errors,
request/protocol correlation, malformed errors, lost-ACK replay with identical
wire bytes and ID, one frame for duplicate waiters, and account invalidation
after a response arrives but before its continuation applies the result. It
also verifies channel role/read/ranking metadata and thread/edit/delete fields.

Only `NativeCommandException.IsDefinitiveRejection` permits the application to
mark a durable send rejected. `Code` preserves a valid server error code. Unknown
codes, storage errors and engine failures remain uncertain. Cancellation or an
authentication failure is never evidence of a definitive domain rejection.
