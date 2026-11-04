# Native delivery contracts

This headless runner uses the same delivery coordinator and command error type
as WinUI. It exercises durable rejection, uncertain transport, exact replay,
storage failures, cancellation and account changes during persistence.
Controlled monotonic time also checks capped transport/storage backoff,
manual retry throttling and the single scoped wake-up timer. Window activation
does not grant a manual retry.

```powershell
dotnet run --project native/probes/delivery-contracts/DeliveryContracts.csproj --artifacts-path S:/Build/native-parity-delivery
```

Storage and transport are deterministic fault fixtures. Passing this runner
does not establish real Canary sending, Windows input or UI acceptance.
