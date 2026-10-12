# Media operations

The web replicas publish cached aggregates on `/metrics`. Every 30 seconds,
with startup jitter and a three-second timeout, one statement reads queue and
quota metadata through the existing database pool. Scrapes never query SQL,
read originals, decode media or expose user/channel/media IDs. No migration or
extra connection pool is required. Collection stops with the application.
The timeout bounds client waiting, not PostgreSQL statement execution. The
monitor starts only when chunk uploads are enabled; disabling that feature
also disables collection and must not be interpreted as an empty queue.

`sproyt_media_snapshot_timestamp_seconds` records the last successful read.
Failures preserve that snapshot and increment `sproyt_media_snapshot_errors_total`.
Zero timestamp means no successful observation, **not** an empty healthy queue.
Filter each replica's snapshot for freshness before aggregating with `max`,
never `sum`: production and canary share one database.

`sproyt_media_preview_jobs{state="pending|running|ready|failed"}` is a gauge.
Old failures stay visible in the dashboard; alert on observed growth over a
short window rather than `failed > 0`. Deletion can mask growth; this initial
aggregate is not an audit trail of every processing failure.

`sproyt_media_oldest_eligible_seconds` counts time since a pending job became
due, or since a running lease expired. Future retries and live leases are
excluded. Zero means no eligible work in the last valid snapshot.

`sproyt_media_original_charged_bytes` and `sproyt_media_reserved_bytes` use the
same minimum 1 MiB charge as admission control. Reservations include open and
sealed uploads until completion/cleanup; completed uploads are not counted
twice. `sproyt_media_quota_bytes` comes from the persisted shared policy.
Logical quota usage is **not** physical PostgreSQL disk usage: previews, WAL,
indices, older media and other applications consume disk too.

## Initial response

- Stale/missing snapshot: check web readiness, SQL connectivity/pool pressure
  and collector errors before trusting queue/quota values.
- Eligible job older than ten minutes: check the single production worker,
  restart/OOM events, scratch capacity and bounded processor errors. Do not
  start multiple ad-hoc workers or delete originals to clear the queue.
- New failed jobs: inspect processing/codec failures. Originals remain
  downloadable; do not treat an old failed gauge as a new incident.
- Logical quota pressure: distinguish reservations awaiting cleanup from
  retained originals. Plan capacity or an explicitly approved retention policy.
- Physical disk pressure: inspect the PostgreSQL PVC, WAL and database growth.
  Missing PVC metrics means unknown capacity, not sufficient free space.

The existing VictoriaMetrics scrape already admits `sproyt_.*`. Grafana rules,
dashboard and physical PVC scraping are maintained separately in Rocket GitOps.
This slice of #170 does not sign off OIDC/device/load/privacy acceptance.
