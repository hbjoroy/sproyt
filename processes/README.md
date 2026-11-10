# Heart process definitions

Deploy `event-planning.yaml` to the same Heart namespace configured for
Sproyt. The application supplies its durable `process_link_id` in start
metadata and uses that value for receive-node correlation. This avoids using a
display name, channel slug, or Heart-generated identifier as an authorization
boundary. Both database adapters persist this server-generated value in the
START outbox payload in the same transaction as the process link. A caller's
`process_link_id` is overwritten. Omitted/null metadata becomes an object;
other non-object metadata is rejected as a conflict before persisting a new
link or command. Request replay retains the original link and payload.

The pilot must be enabled for the circle through the `heart.event-planning`
feature flag. Removing `SPROYT_HEART_URL` is the deployment-wide kill switch;
ordinary chat remains available and pending outbox records remain durable.

## Real event-planning regression probe

`tools/heart-event-planning-probe.mjs` runs the unchanged chart definition
against the pinned deployed Heart image and disposable PostgreSQL using `wslc`.
Pass a freshly built native Sprøyt binary with `--binary`; optionally pass the
Rust test executable with `--contract-binary` to run the PostgreSQL shared
repository contract in a separate fixture database. Run Cargo through the
workspace's `DevCache.ps1` routine on Windows and use the binary paths printed
by that routine. Read the relevant `wslc` subcommand help first.

```powershell
node tools/heart-event-planning-probe.mjs --confirm-isolated --binary $sproytBinary --contract-binary $contractBinary
```

The probe creates labelled containers/network, loopback-only published ports,
synthetic credentials and a synthetic user/circle/channel. It checks title-only,
spoofed-ID and omitted metadata, API replay, actual receive-node correlation,
completion with decision `yes`, and durable retry/chat during a Heart pause.
It writes evidence under `.local` and removes its owned fixtures. It does not
run Kubernetes controllers or alter historical outbox/Heart instances.
The [2026-11-10 evidence](../docs/evidence/heart-event-planning-2026-11-10.json)
records the successful rerun after the missing correlation metadata was fixed.