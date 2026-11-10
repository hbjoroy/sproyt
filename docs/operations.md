# Production operations

## Initial service objectives

These are beta objectives measured over a rolling 28-day window. Revisit them
after the first month of representative traffic.

| Signal | Objective | Evidence |
| --- | --- | --- |
| HTTP/WebSocket availability | 99.9% of non-maintenance minutes | external probe plus `sproyt_ready` |
| Accepted-message durability | 100%; an acknowledged message survives restart | PostgreSQL conformance and restore drill |
| Send latency | 99% of accepted sends acknowledged within 750 ms | request traces or a protocol load test |
| Reconnect recovery | 99% of clients catch up within 5 s | reconnect scenario in the load test |

Page when readiness is unavailable for five minutes, the five-minute server
error ratio exceeds 2%, or PostgreSQL free storage is forecast to run out in
24 hours. Create a ticket when the latency or reconnect objective consumes
more than 25% of its monthly error budget in seven days.

The current in-process metrics are deliberately low-cardinality and contain no
circle, channel, user, message, token, or subject values. Production should
scrape `/metrics`; dashboards should combine these metrics with ingress and
PostgreSQL metrics. Traces and logs may contain request IDs and durable object
IDs, but never message bodies, cookies, authorization headers, OIDC tokens, or
client secrets.

Incoming integrations add `sproyt_integration_deliveries_total`, with only the
fixed outcomes `accepted`, `duplicate`, `ignored`, `rejected`, and `error`, plus
`sproyt_integration_duration_microseconds_total`. These metrics never use an
integration, channel, alert, report, or user identifier as a label. See
[`incoming-webhooks.md`](incoming-webhooks.md) for the retry and audit contract.

Authenticated browsers report only a fixed event name for WebSocket connect,
disconnect/error, session-refresh success/failure and upload success/failure.
No close reason, URL, filename, browser identity or application content is
accepted by the endpoint or emitted as a metric. Use these counters to detect
fleet-wide regressions; use a request ID supplied to a user for individual HTTP
failures. A refresh-failure ratio above 10% for 15 minutes or more than ten
WebSocket transport errors in 15 minutes creates an operator ticket.

For clusters running Prometheus Operator and Grafana's dashboard sidecar,
enable `monitoring.serviceMonitor.enabled`,
`monitoring.prometheusRule.enabled`, and
`monitoring.grafanaDashboard.enabled`. The chart then installs a `/metrics`
scrape target, page alerts for five minutes of failed readiness and a five-minute
HTTP 5xx ratio above 2%, browser session/WebSocket regression tickets, and the
Sproyt overview dashboard with connection, refresh and upload outcomes. Add the Prometheus
selector labels required by the cluster through each `additionalLabels` map.

## Account data export

`GET /api/v1/me/export` streams the existing portable JSON format directly from
one database snapshot. It includes the complete accessible channel history;
the interactive history limit does not apply. It does not create a stored
archive, download token or temporary file. Account/data-export owns the
contract; operations owns capacity and timeout reviews.

| Resource | Bound |
| --- | --- |
| Local generation/response buffering | One export per application process; no waiting queue |
| PostgreSQL snapshot generation | Two across all replicas sharing the database, at most one per account, enforced by transaction advisory locks |
| PostgreSQL connections | One per active snapshot; exports return 503 when the configured pool has fewer than two connections |
| PostgreSQL duration | 120 s absolute request deadline, 10 s per statement, 120 s idle-transaction backstop |
| SQLite snapshot and buffering deadline | At most 4 s after user upsert, below the existing 5 s busy timeout |
| Message reads | Keyset pages of at most 16 messages; SQL rejects oversized bodies/sender names before returning their contents |
| Application output buffering | One queued chunk, at most 1 MiB per serialized record; no whole-export JSON buffer |
| Temporary archive storage | None |

There is no export-specific waiting queue and advisory locks are nonblocking;
normal user upsert and database connection acquisition can still wait. Busy
exports return 429 with `Retry-After: 5`; clients
should wait before starting a fresh attempt. PostgreSQL advisory locks apply
to generation and are released when the snapshot commits or rolls back. Local
admission remains held until the final queued payload is consumed or the
deadline discards it. The HTTP server and socket retain their normal transport
buffers. These limits bound server export work, not the number of bytes a user
may legitimately have in their history.
SQLx rollback cleanup is asynchronous: a cancelled query may briefly retain its
connection after local admission is released. The PostgreSQL statement timeout
also bounds that outstanding database work; the advisory locks remain owned by
the transaction until cleanup completes.

Authentication and access are checked when the request starts. Content and
memberships come from the same snapshot. A membership revoked during an
accepted export affects subsequent requests; the accepted snapshot can finish
within its deadline. Already delivered bytes cannot be recalled. There is no
later download that can bypass a fresh authentication or membership check.

The response remains `application/json` with attachment disposition,
`Cache-Control: no-store` and `X-Content-Type-Options: nosniff`. JSON is complete
only after all selected records have been written and the snapshot transaction
has completed successfully. A database error, record-size limit or timeout
after headers interrupts the response body; it must not be accepted as a
successful partial export. The browser's existing download handler reports a
failed body read instead of saving it as a successful export. Retry the entire
request to obtain a new snapshot.
The memory budget applies to the server. The existing hidden browser export
handler collects a download blob in browser memory; this change does not expose
that control or change its client-side storage behavior.

Client disconnection cancels generation. A deadline also cancels work when the
client stops reading, so a slow client cannot retain a database snapshot
indefinitely. No export content, authentication credentials or user identifiers
should be added to resource-measurement logs.

Use PostgreSQL for replicated deployment and representative concurrent-chat
capacity measurements. SQLite is a single-process deployment option; its read
snapshot can delay writers, so its export deadline must be shorter than the
database busy timeout. Do not enable WAL as an export optimization without a
separate dependency and recovery assessment.

Review the limits if complete legitimate exports regularly exceed the time or
record budget. Measure the relevant bottleneck before increasing capacity or
introducing an asynchronous archive service. Raising the interactive history
limit or silently omitting rows is not a fallback.

### Export measurement, 5 November 2026

The #351 candidate was measured with a fresh Windows application process and
an isolated PostgreSQL 17.10 database for each sample. Each synthetic message
contained 2 KiB. The application used its four-connection pool; the database
fixture had two CPUs and a 768 MiB memory limit. The build used locked
dependencies, prebuilt frontend assets and development debug information off.

| Messages | Complete JSON bytes | Download time | App working-set increase / peak | Private-bytes increase | Concurrent chat acknowledgements | Chat p95 / maximum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 4,000 | 9,208,401 | 0.763 s | 1.86 / 26.49 MiB | 1.44 MiB | 12 | 48.45 / 48.45 ms |
| 40,000 | 92,110,406 | 21.297 s | 3.69 / 27.79 MiB | 3.33 MiB | 380 | 17.48 / 46.56 ms |

Both samples verified every message and sequence, valid complete JSON, and
exclusion of messages sent after the snapshot. Each observed export held one
PostgreSQL connection and two advisory locks. The tenfold output increase did
not produce a proportional increase in application memory. These are two
single-host synthetic samples with compressible data, not production capacity
or latency-SLO evidence. The different chat sample counts do not establish a
latency improvement for the larger export.

[`tools/measure-account-export.mjs`](../tools/measure-account-export.mjs)
contains the reproduction procedure. It samples server working set and private
bytes at a requested 25 ms interval, streams the response to a disk sink in a
separate client process, pauses 12 ms per 256 KiB received, and sends chat
messages roughly every 40 ms. It stops memory sampling before parsing the
downloaded JSON in the client. Its synthetic database and output file are
removed afterward. The separate PostgreSQL HTTP regression verifies admission
across three application instances, snapshot membership after revocation,
disconnect cleanup and interrupted-body errors.

## Backup and restore drill

Use the managed PostgreSQL provider's encrypted point-in-time recovery when
available. Before an actual schema migration, verify a sufficiently recent,
usable recovery point against the project's recovery objectives. Take a logical
backup when the existing recovery point is insufficient:

```sh
pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" --file sproyt.dump
```

Quarterly and when recovery assumptions change, the release owner restores into
an isolated database and namespace. This includes backup tooling and incompatible
data or storage changes. Ordinary UI releases, no-op migration jobs and additive
migrations do not each require a new backup or restore drill. Keep scheduled
backups/PITR running; backup cadence and restore-test cadence are separate decisions.
The weekly CI drill uses synthetic data and follows the
[independent recovery policy](ci-iteration-strategy.md#independent-recovery-decision).

```sh
createdb sproyt_restore
pg_restore --exit-on-error --no-owner --no-acl --dbname sproyt_restore sproyt.dump
DATABASE_URL=postgres://.../sproyt_restore sproyt migrate
```

Start the same immutable image used in production against the restored
database. Verify readiness, circle/channel counts, the maximum sequence for a
sample of channels, and that a test principal can reconnect and read history.
Do not send traffic or OIDC callbacks from the drill namespace to production.
Record image digest, backup timestamp, restore duration, row-count checks, and
operator in the release evidence.

## Deployment, rollback, and incident response

1. Use the [CI delivery policy](ci-iteration-strategy.md#delivery-phases) and
   [release checklist](release-checklist.md) to publish the reviewed revision
   or promote the verified canary digest. Retain the corresponding delivery
   evidence.
2. Back up PostgreSQL before a schema change. When applicable, confirm the
   pre-upgrade migration Job runs once and succeeds.
3. Deploy by immutable image digest. Confirm the GitOps revision in Argo CD,
   the application and worker rollout, and public health and readiness. Check
   that `/versionz` reports the expected application commit.
4. Smoke-test login, create/list channel, send and receive between two users,
   reconnect/catch-up, and logout.
5. Watch readiness, errors, latency, restarts, PostgreSQL connections, and
   storage for at least 15 minutes.
6. Publish the short production announcement using the
   [release checklist](release-checklist.md#announce-each-production-rollout).

The release news publisher uses the existing reserved Sprøyt agent and ordinary
chat persistence. It requires the production pod configuration, checks that the
binary revision is live at the public production URL, and resolves the unique
open Felles channel named **Nyhende i Sprøyt**. A publication receipt is recorded
in the same transaction as the chat message and survives message deletion. It
makes retries with the same revision and summary idempotent, including
concurrent retries. A different summary for an already announced revision is
rejected.

On `SIGTERM`, Sproyt first marks readiness false, stops accepting new HTTP
work through Axum's graceful-shutdown path, and sends WebSocket close code
1001 to every connected client. The Helm chart gives this drain 30 seconds
before Kubernetes may terminate the container. Keep the chart's
`terminationGracePeriodSeconds` at or above the application grace period.

The ARM64 delivery CI job lints and renders Helm and validates the delivery
contract. The selected PostgreSQL and recovery jobs cover application-level
multi-replica behaviour and database recovery. See the
[rollout and rollback policy](ci-iteration-strategy.md#rollout-and-rollback)
for automated coverage, manual evidence and the deferred kind job.

For changes that affect chart, migration, runtime or rollout and rollback
behaviour, the release owner exercises the applicable fresh install, forward
migration, two-replica rolling upgrade and application rollback paths in an
isolated cluster before promotion.
Record candidate and rollback digests, chart revision, migration Job results,
ready replicas and probe results. Confirm the previous image works with the
forward schema. [`tools/kind-smoke.sh`](../tools/kind-smoke.sh) can exercise
install, scale and Helm revision rollback with one preloaded compatible web
image and development authentication in a disposable cluster. Retain results
when the rehearsal is performed.

For application regressions, restore the last verified image digests and
compatible chart and values in GitOps, then verify Argo CD and runtime
readiness. Keep the database on the forward schema. Migrations must remain
expand-and-contract compatible. Disable an incomplete capability with its
feature flag. If integrity may be affected, stop writes by scaling the
Deployment to zero, preserve logs and a fresh database snapshot, then restore
or repair from an isolated copy.

During an incident, name an incident lead, timestamp decisions, and use request
IDs to correlate ingress, application, and database evidence. Treat accidental
logging of message content, cookies, tokens, or secrets as a privacy incident:
restrict log access, rotate affected credentials, preserve an access trail,
and determine the affected retention window.

## Retention and capacity

Chat history is retained until a circle owner deletes it; audit events are
retained for at least 365 days for the beta. Database backups default to 35
days. A circle owner can use the `delete_circle` protocol command (or **Slett
krets** in the browser) to atomically delete the circle and all database rows
that belong to it, including channels, messages, invitations, process state and
agent grants. The durable `circle.deleted` audit event retains the actor, target
and cause but not deleted message content. Backups age out deleted content on
the backup schedule; operators must not restore an old backup over production
without replaying deletion obligations.

An authenticated user can request `GET /api/v1/me/export` or use **Eksporter
mine data** in the browser. The non-cacheable `sproyt.user-export.v1` JSON file
contains their profile, circle memberships, channel metadata/read markers and
complete ordered history for channels they are currently authorized to read.
It also includes their private, stable Sprøyt signup number when they have one.
The number is never included in public profiles or people lists; deleted
accounts leave an anonymous historical slot so numbers are never reused.
It deliberately excludes channels the user has not joined and other users'
profile/identity-provider fields. Export is produced from one database snapshot
so concurrent sends cannot create internally inconsistent cursors. These
defaults still require an explicit product/privacy decision before public
production.

Before each material traffic increase, load-test at two replicas with a
production-sized PostgreSQL tier. Exercise concurrent sends to one hot channel,
many quiet channels, reconnect/catch-up, rolling restart, and a temporarily
unavailable database. Pass only when the objectives above hold, sequences stay
unique and contiguous, and recovery creates no duplicate acknowledged message.

When backend checks are selected, CI runs two fast `sproyt.chat.v1` recovery
baselines. The SQLite `websocket_capacity_reconnect_and_service_restart_gate`
sends 40 durable messages, enforces the 750 ms p99 send objective, restarts the
application service, and requires exact cursor catch-up within five seconds. The PostgreSQL
`postgres_two_replica_realtime_and_restart_catch_up_gate` starts two independent
application processes with separate LISTEN connections, proves live delivery
between them, stops one process, accepts a message on the surviving replica,
and requires the restarted replica to catch up within five seconds. It also
proves that a freshly migrated database exposes the global `general` channel to
every authenticated user. These deterministic gates catch protocol,
cross-replica and recovery regressions early; they do not replace the
production-sized exercise required before a material traffic increase.

## Database and chat capacity observations

`/metrics` adds process-local observations for the first #353 measurement slice.
No queue size, pool limit, timeout or retry policy is changed. Pool snapshot
gauges exist only when the web runtime registers its existing shared PostgreSQL
pool; SQLite has chat phase observations without PostgreSQL series.

| Metric | Meaning |
| --- | --- |
| `sproyt_pool_connections_size`, `_idle`, `_max` | Independently sampled pool gauges, labelled only `pool="shared_postgres"`; they are not pool waiting time. |
| `sproyt_pool_selected_acquire_duration_seconds` | Histogram from polling an explicit acquire until connection, timeout, closed pool, other error or cancellation. Includes any SQLx connection creation/validation during checkout; excludes query and `BEGIN` time. |
| `sproyt_chat_phase_duration_seconds` | Histograms for `admission` (seeking queue space), `queued` (admitted until dequeue), and `execution` (actor handling). |
| `sproyt_chat_phase_in_flight`, `_peak` | Current and process-lifetime peak phase counts, aggregated across local chat engines. Admission includes attempts that obtain space immediately. Queued excludes executing commands. |

Pool timing is deliberately **selected coverage**, not all database calls:

- `send` / `send_idempotent`: the transaction acquire in `append_message` /
  `append_message_idempotent`. After commit the connection is released before
  the existing separate `pg_notify` acquire, which is not measured here.
- `history_access` / `history_read`: the membership check and the selected
  page query in `load_recent_messages`, including the reconnect `after` path.
  They retain separate acquisitions and the same access checks.
- `push_select` / `push_claim`: candidate lookup and conditional lease update
  in `NotificationService::claim`. `push_complete` covers each existing SQL
  statement for delivered, expired or retry outcomes, including the separate
  subscription-status update. No connection spans external delivery.

Other acquire paths, queries/transaction duration, listeners, push startup
backfill, and other application pools remain outside this histogram. Check
that coverage before attributing total database contention to these samples.
Acquire latency is not exclusively time parked waiting for a free connection.
Missing histogram series mean no observed attempt in that process, not zero
latency. All observations reset at process restart; peak gauges are not sums
to compare with a single engine's 1,024-place mailbox limit.

Chat labels use only the 13 fixed command classes. Admission outcomes are
`admitted`, `closed`, `cancelled`; queued outcomes are `dequeued`, `discarded`;
execution outcomes are `completed`, `cancelled`. Completed includes handled
domain rejection and does not claim successful persistence or client delivery.
Dropping a reply receiver after admission does not cancel the accepted command.
Receiver shutdown counts discarded queued commands; aborting actor execution
records cancellation. There are no identifiers, SQL or error messages in labels.

Histograms use seconds with bounds 0.001, 0.005, 0.025, 0.1, 0.5, 0.75, 1, 5
and `+Inf`. Only observed combinations are rendered. The upper bound is 1,395
new time series per process: 385 pool histogram samples, 1,001 chat histogram
samples, six phase gauges and three registered-pool gauges.

For example, compare successful selected acquire p99 with chat admission and
queued p99, rather than treating HTTP header latency as message acknowledgement:

```promql
histogram_quantile(0.99, sum by (le, operation) (
  rate(sproyt_pool_selected_acquire_duration_seconds_bucket{result="acquired"}[5m])))
histogram_quantile(0.99, sum by (le, phase) (
  rate(sproyt_chat_phase_duration_seconds_bucket{result=~"admitted|dequeued|completed"}[5m])))
sum by (operation, result) (
  rate(sproyt_pool_selected_acquire_duration_seconds_count{result!="acquired"}[5m]))
```

These observations support the isolated capacity exercise in the
[next-step plan](architecture-alignment-next-step-plan.md). They do not establish
a production resource budget or justify more replicas, higher pool limits or
channel partitioning. Rollback is a code revert; there is no migration or new
configuration to undo.

### Local verification, 10 November 2026

The measurement slice was exercised on Windows with locked dependencies and
development/test debug information disabled, using its managed S: build cache.
The broad Rust run passed 479 tests with 12 explicit ignores, using one test
thread. Legacy fixtures that use `./target` require that directory even when
Cargo output is redirected; the directory contained test data only. The first
parallel run also encountered a transient SQLite file-removal lock.

An isolated PostgreSQL 17 container ran the eight repository tests and all four
capacity tests. This includes holding the only pool connection to distinguish
wait/success, timeout, cancellation and closed-pool outcomes, and sending plus
idempotent replay/history with a one-connection pool while receiving the
post-commit notifications. Existing shared-budget, two-replica recovery and
bounded-export contracts also passed. An initial all-`postgres_` invocation
mixed agent job fixtures and omitted the marine feature flag; the six affected
agent contracts each passed with a fresh database and the required flag.
Their test setup was corrected locally without changing application policy.

The explicit mailbox diagnostic compared raw Tokio and instrumented channels
with identical commands/capacity, 10,000 commands per sample and three samples:

| Sample | Raw channel | Instrumented | Added time per command |
| --- | ---: | ---: | ---: |
| 1 | 6,767 µs | 11,939 µs | 517 ns |
| 2 | 6,653 µs | 11,646 µs | 499 ns |
| 3 | 6,691 µs | 11,787 µs | 509 ns |

Run it with `cargo test chat::mailbox::tests::mailbox_instrumentation_overhead_sample
--locked -- --ignored --nocapture` (through the managed wrapper on Windows).
This single-task diagnostic has no database, network or producer contention;
it measures mailbox instrumentation overhead only. It is not a production
capacity or latency result. The separate isolated load matrix remains #353
delivery B.

## Heart failure isolation

Heart is an optional internal dependency, not part of the ordinary chat write
path. Process commands are committed to the durable outbox before delivery;
retryable Heart failures leave the command queued with bounded backoff. The
`heart_unavailable_does_not_interrupt_chat_and_recovers_once` regression runs
the real HTTP gateway against a controllable 503 endpoint, proves that a chat
message can still be sent and read during the outage, restores Heart, and
requires exactly one durable `process.started` event. A release still needs the
target-cluster rolling-update and end-to-end pilot observations in the release
checklist.

For the write portion of the target MCP/agent exercise, use a quiet channel
where the authenticated user is owner or moderator. Open that channel, expand
**Agenttilgang**, and choose **Lag kortliva tilgang**. Sproyt creates a
30-minute, channel-bounded agent with only `read_history` and `send_messages`,
shows the credential once, and marks the credential response `Cache-Control:
no-store`. Record the non-secret agent ID shown in the status, choose **Kopier
credential**, then run Node.js 24 or newer and paste the clipboard value only
into the silent prompt:

```sh
export SPROYT_MCP_URL=https://sproyt.bjoroy.me/mcp
export SPROYT_CHANNEL_ID='dedicated-load-channel-id'
read -r -s -p 'Agent credential: ' SPROYT_AGENT_CREDENTIAL
export SPROYT_AGENT_CREDENTIAL
node tools/mcp-load.mjs --confirm-write --messages 40 --concurrency 4
unset SPROYT_AGENT_CREDENTIAL
```

The tool refuses to write without `--confirm-write`, never prints the
credential or message bodies, verifies that the grant includes the channel,
uses stable idempotency keys, replays one request, requires unique contiguous
sequences and enforces the 750 ms p99 objective. It emits
`sproyt.mcp-load-evidence.v1` JSON for the release record. Use an otherwise
quiet channel because unrelated concurrent sends intentionally make the
contiguity check fail. Immediately choose **Trekk tilbake** in the same browser
after the run. This atomically revokes the profile, credential and both grants,
and writes an `agent.revoked` audit event. The 30-minute expiry independently
bounds all three if explicit revocation is interrupted. Never retain the
credential in release evidence.

The previously issued bearer credential must receive HTTP 401 immediately;
do not retain it for a later run.

This MCP exercise proves the auditable agent write path and database latency;
it does not substitute for the authenticated browser WebSocket reconnect and
rolling-restart journey.

## Server measurements (October 2026)

HTTP metrics distinguish operational probes (/healthz, /readyz, /metrics,
/versionz) from application traffic. The existing aggregate request,
in-flight, server-error and duration counters now count application traffic
only. sproyt_http_response_duration_seconds is a histogram in seconds,
labelled by router template, bounded method/status classes and
traffic="application" or traffic="probe". No raw path, query, account or
channel ID is used. At most 512 detailed series are retained per process;
additional combinations share two overflow series, one per traffic class.

Application p95 across replicas over five minutes:

    histogram_quantile(0.95,
      sum by (le) (rate(sproyt_http_response_duration_seconds_bucket{traffic="application"}[5m])))

Use 0.99 for p99 and retain route in the aggregation to inspect one route
template. These measure time to response headers, not a WebSocket/SSE body's
lifetime. Cancelled handlers have status_class="cancelled" and release the
in-flight gauge.

sproyt_connections_active has transport="websocket" or transport="sse" and is
owned by the server's authenticated connection lifetime, including cancellation.
sproyt_connections_opened_total and sproyt_connections_closed_total track
those transitions. Sum gauges across replicas; use counter rates across
restarts. SSE control and channel streams count individually. These are
connections, not unique users or sessions; the browser telemetry counters
remain separate and are never used to calculate these gauges.

### Transport and Heart detail (16 October 2026)

`sproyt_transport_events_total` has exactly two transport values and nine
fixed event values. WS `envelope_sent` means its application envelope send
future completed successfully; `send_failed` and `receive_failed` count actual
socket errors. Protocol ping/pong/close control frames are excluded from those
send counters. These observations are not durable-message acknowledgements.

WS `queue_backpressure` counts a full 256-entry outbound queue encountered by a
subscription producer. It waits for capacity as before; it does not silently
drop the pending envelope. `queue_closed` counts an attempted enqueue into a
closed queue. `broadcast_lagged` counts receiver overruns and
`broadcast_skipped` counts the broadcast events skipped, independently for WS
and SSE. The existing Lagged protocol event/cursor catch-up remains responsible
for recovery; skipped events are not necessarily lost durable chat messages.

SSE `body_frame_yielded` counts frames pulled from the application stream,
including its comment and application heartbeats. Axum's automatic keep-alive
comments are excluded. HTTP body backpressure controls pulling; a yielded frame
is not evidence that the browser received it. SSE's infallible application body
cannot observe downstream socket send errors, so it does not fabricate a
`send_failed` count. Slow-consumer overruns use its actual broadcast-lag metrics.
The existing connection guards count closure and cancellation for both transports.

`resume_requested` counts a valid WS LoadRecentMessages command with an `after`
cursor, or an admitted SSE channel stream carrying `after`/Last-Event-ID. It is
not an inferred reconnect or a distinct-user count. Requests can be ordinary
catch-up/history navigation. Existing client reconnect telemetry remains separate.
All of these counters are process-local, reset on process restart and must be
aggregated with `rate`/`increase`, not by subtracting browser lifecycle events.

`sproyt_heart_attempt_duration_seconds` is a seconds histogram with closed
method labels GET/POST/OTHER and result labels success/timeout/transport/http_4xx/
http_5xx/http_other/invalid_response/cancelled. Its `_count` series provides the
attempt/error counters. V1 gateway retries produce separate observations,
excluding retry backoff; v2 pilot, work-item review, GitHub export and status
adapters use the same instrumentation. JSON-read latency includes bounded body
decoding; completion commands that intentionally ignore the response body measure
to response headers, then retain their authoritative reconciliation behavior.
Cancellation never becomes success. No request URL, account/channel/task ID,
correlation ID, credential or message text becomes a label. Storage is bounded
at 18 transport counters and 24 Heart histogram series per process.

Heart p95 and failed HTTP/transport attempts across replicas:

    histogram_quantile(0.95,
      sum by (le) (rate(sproyt_heart_attempt_duration_seconds_bucket[5m])))
    sum by (result) (rate(sproyt_heart_attempt_duration_seconds_count{result!="success"}[5m]))

Deterministic tests cover cumulative buckets, fixed cardinality, skipped-event
versus lag-incident counts, process reset, cancelled attempts, bounded v2
response decoding and queue backpressure without dropped pending envelopes.
Existing tests in `operations::measurements` cover connection cancellation and
probe separation. This code completes the concrete schema-free measurement slice
of #170; real-provider/device/load/privacy/operational sign-off remains open.
No shared database or Heart service was stopped for this instrumentation.
