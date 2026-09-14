# Incoming Grafana webhooks

Sprøyt can turn Grafana alerts and operational reports into ordinary channel
messages. An integration credential is bound to exactly one channel and only
has permission to send messages there. The request URL and payload cannot
select another destination.

## Create and manage a channel integration

An authenticated channel administrator creates an integration with:

```http
POST /api/v1/channels/{channel-id}/integrations/grafana
```

The response contains an agent ID and a credential. The credential is returned
once, with `Cache-Control: no-store`; store it as a secret. Sprøyt stores only
its SHA-256 hash. Rotate it with:

```http
POST /api/v1/integrations/{agent-id}/rotate
```

Rotation revokes all older credentials atomically. Revoke the whole
integration with the existing endpoint:

```http
POST /api/v1/agents/{agent-id}/revoke
```

If somebody adds another active grant or a broader grant to this agent, the
incoming endpoint fails closed instead of guessing a destination.

## Grafana alert contact point

Configure a Grafana Webhook contact point with:

- URL: `https://sproyt.bjoroy.me/api/v1/integrations/grafana/alerts`
- HTTP method: `POST`
- Authorization header scheme: `Bearer`
- Credentials: the one-time Sprøyt credential
- Payload: Grafana's default webhook payload

Do not put the credential in the URL. Sprøyt accepts Grafana's grouped payload,
including groups that contain both firing and resolved alerts. It renders each
alert with a controlled Markdown template containing status, severity,
service, namespace, summary, timestamps, and safe dashboard, panel, generator,
or runbook links when present.

Grafana labels and annotations are untrusted input. Sprøyt does not fetch any
URL, strips control characters, prevents mentions and media directives, and
does not log the raw payload or credential.

For example, this excerpt from a normal Grafana payload is accepted:

```json
{
  "status": "firing",
  "alerts": [{
    "status": "firing",
    "labels": {"severity": "critical", "service": "api", "namespace": "prod"},
    "annotations": {
      "summary": "API @alle is unavailable",
      "runbook_url": "https://grafana.example/runbook/api"
    },
    "startsAt": "2026-09-14T10:00:00Z",
    "fingerprint": "7d8f23",
    "dashboardURL": "https://grafana.example/d/api"
  }],
  "truncatedAlerts": 0
}
```

The visible message uses Sprøyt's own heading/list Markdown template. The
mention-like text above is rendered as `＠alle`, not as a notification.

## Delivery and retry contract

- The maximum request body is 256 KiB and a group can contain at most 50
  alerts. The entire payload is validated before any alert is written.
- A delivery is durable before Sprøyt returns success. Storage failure returns
  `503`, invalid input returns `400`, invalid credentials return `401`, removed
  grants return `403`, and rate limiting returns `429` with `Retry-After`.
- An occurrence is identified by integration, Grafana `fingerprint`, and
  `startsAt`. Repeated firing or resolved deliveries are idempotent across
  replicas and restarts.
- A new `startsAt` is a new occurrence. A resolved event received before its
  firing event cannot later be reopened by delayed delivery.
- `truncatedAlerts` is shown in the generated message when Grafana reports it.

Sprøyt does not promise unlimited sender retries or lossless delivery through
an extended outage. Configure and verify retry behaviour at the sender, and
retain an independent alert route for failures that affect the shared cluster
or database.

## Versioned operational reports

Reports use a separate endpoint and idempotency namespace:

```http
POST /api/v1/integrations/grafana/reports
Authorization: Bearer {credential}
Content-Type: application/json
```

Example version 1 payload:

```json
{
  "version": "1",
  "report_id": "platform-week-2026-37",
  "period_start": "2026-09-07T00:00:00Z",
  "period_end": "2026-09-14T00:00:00Z",
  "title": "Plattformrapport",
  "summary": "Alle sentrale tenester er stabile.",
  "links": [
    {"label": "Dashboard", "url": "https://grafana.example/d/platform"}
  ]
}
```

`report_id` is the idempotency key. Retrying the same report does not create a
second message or a second push notification.

## Operations

The `/metrics` endpoint exposes fixed-cardinality delivery outcomes and total
processing time. Audit events contain only the integration identity, outcome,
status, and persisted message ID—not alert labels, annotations, report text,
payloads, or credentials.

This path is independent of Grafana availability but deliberately shares the
Sprøyt PostgreSQL cluster with chat. A database outage therefore returns `503`
and relies on Grafana retry rather than acknowledging an undurable alert.

The MVP uses Sprøyt's opaque, hashed agent credential. Authentication is kept
outside the delivery/idempotency core so a later Authentik OAuth2
client-credentials adapter can resolve a machine identity to the same bounded
agent principal without changing channel binding or message persistence.
