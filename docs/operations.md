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

## Backup and restore drill

Use the managed PostgreSQL provider's encrypted point-in-time recovery when
available. Also take a logical backup before a schema migration:

```sh
pg_dump --format=custom --no-owner --no-acl "$DATABASE_URL" --file sproyt.dump
```

Quarterly, restore into an isolated database and namespace:

```sh
createdb sproyt_restore
pg_restore --exit-on-error --no-owner --no-acl --dbname sproyt_restore sproyt.dump
SPROYT_DATABASE_URL=postgres://.../sproyt_restore sproyt migrate
```

Start the same immutable image used in production against the restored
database. Verify readiness, circle/channel counts, the maximum sequence for a
sample of channels, and that a test principal can reconnect and read history.
Do not send traffic or OIDC callbacks from the drill namespace to production.
Record image digest, backup timestamp, restore duration, row-count checks, and
operator in the release evidence.

## Deployment, rollback, and incident response

1. Run CI and retain its rendered manifest, SBOM, and vulnerability report.
2. Back up PostgreSQL and run the pre-upgrade migration Job once.
3. Deploy by immutable image digest. Confirm migration Job success and both
   probes before admitting traffic.
4. Smoke-test login, create/list channel, send, reconnect/catch-up, and logout.
5. Watch readiness, errors, latency, restarts, PostgreSQL connections, and
   storage for at least 15 minutes.

For application regressions, roll back the Deployment to the previous image.
Migrations must remain expand-and-contract compatible; never roll a database
schema backward as the first response. Disable an incomplete capability with
its feature flag. If integrity may be affected, stop writes by scaling the
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
days. These defaults require an explicit product/privacy decision before public
production because deletion and export workflows are not implemented yet.

Before each material traffic increase, load-test at two replicas with a
production-sized PostgreSQL tier. Exercise concurrent sends to one hot channel,
many quiet channels, reconnect/catch-up, rolling restart, and a temporarily
unavailable database. Pass only when the objectives above hold, sequences stay
unique and contiguous, and recovery creates no duplicate acknowledged message.
