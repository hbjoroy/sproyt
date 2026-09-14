use std::{collections::HashMap, time::Instant};

use axum::{
    Json,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    agent::{AgentScope, CreateAgent, GrantAgent},
    domain::{ChannelId, MessageBody, RepositoryError, UserId},
    integration::{AlertState, DeliveryResult, GRAFANA_PROVIDER, IncomingAlert, IncomingReport},
    operations::IntegrationOutcome,
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};

const MAX_ALERTS: usize = 50;
const MAX_LINKS: usize = 8;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GrafanaWebhook {
    #[serde(default)]
    alerts: Vec<GrafanaAlert>,
    #[serde(default)]
    truncated_alerts: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GrafanaAlert {
    status: String,
    #[serde(default)]
    labels: HashMap<String, String>,
    #[serde(default)]
    annotations: HashMap<String, String>,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    fingerprint: String,
    generator_url: Option<String>,
    dashboard_url: Option<String>,
    panel_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReportWebhook {
    version: String,
    report_id: String,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    title: String,
    summary: String,
    #[serde(default)]
    links: Vec<ReportLink>,
}

#[derive(Debug, Deserialize)]
struct ReportLink {
    label: String,
    url: String,
}

#[derive(Debug, Serialize)]
struct DeliverySummary {
    accepted: usize,
    duplicate: usize,
    ignored: usize,
}

pub(crate) async fn create_grafana_integration(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(id) {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    let actor = principal.user.id;
    let created = match state
        .agents
        .create(CreateAgent {
            actor: actor.clone(),
            owner_id: actor.clone(),
            display_name: "Grafana".to_owned(),
            provider: GRAFANA_PROVIDER.to_owned(),
            service_identity: Uuid::now_v7().to_string(),
            purpose: format!("Grafana-varsling til kanal {channel_id}"),
            rate_limit_per_minute: 120,
            expires_at: None,
        })
        .await
    {
        Ok(value) => value,
        Err(error) => return repository_response(error),
    };
    if let Err(error) = state
        .agents
        .grant(GrantAgent {
            actor: actor.clone(),
            agent_id: created.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel_id),
            scope: AgentScope::SendMessages,
            expires_at: None,
        })
        .await
    {
        let _ = state
            .agents
            .revoke_agent(actor, created.agent_id.clone())
            .await;
        return repository_response(error);
    }
    let mut response = (StatusCode::CREATED, Json(created)).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
}

pub(crate) async fn rotate_integration_credential(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let agent_id = match UserId::new(id) {
        Ok(value) => value,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };
    match state
        .agents
        .rotate_credential(principal.user.id, agent_id)
        .await
    {
        Ok(created) => {
            let mut response = Json(created).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn receive_grafana_alerts(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<GrafanaWebhook>, JsonRejection>,
) -> axum::response::Response {
    let started = Instant::now();
    let principal = match authenticate_integration(&state, &headers).await {
        Ok(value) => value,
        Err(response) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return response;
        }
    };
    let Json(payload) = match payload {
        Ok(value) => value,
        Err(error) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return (error.status(), "ugyldig JSON").into_response();
        }
    };
    if payload.alerts.is_empty() || payload.alerts.len() > MAX_ALERTS {
        record_outcome(&state, IntegrationOutcome::Rejected, started);
        return (
            StatusCode::BAD_REQUEST,
            "tal på alarmar må vere mellom 1 og 50",
        )
            .into_response();
    }
    let rendered = match payload
        .alerts
        .iter()
        .enumerate()
        .map(|(index, alert)| {
            validate_alert(alert).and_then(|state_value| {
                render_alert(
                    alert,
                    state_value,
                    (index == 0).then_some(payload.truncated_alerts),
                )
                .map(|body| (alert, state_value, body))
            })
        })
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(value) => value,
        Err(message) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    };
    let mut summary = DeliverySummary {
        accepted: 0,
        duplicate: 0,
        ignored: 0,
    };
    for (alert, alert_state, body) in rendered {
        match state
            .integrations
            .deliver_alert(IncomingAlert {
                agent_id: principal.agent_id.clone(),
                credential_id: principal.credential_id,
                fingerprint: alert.fingerprint.clone(),
                starts_at: alert.starts_at,
                state: alert_state,
                body,
            })
            .await
        {
            Ok(DeliveryResult::Accepted(message)) => {
                summary.accepted += 1;
                let _ = state.chat.announce_persisted_message(message.id).await;
            }
            Ok(DeliveryResult::Duplicate(_)) => summary.duplicate += 1,
            Ok(DeliveryResult::IgnoredResolved) => summary.ignored += 1,
            Err(error) => {
                record_outcome(&state, IntegrationOutcome::Error, started);
                return integration_repository_response(error);
            }
        }
    }
    let outcome = if summary.accepted > 0 {
        IntegrationOutcome::Accepted
    } else if summary.duplicate > 0 {
        IntegrationOutcome::Duplicate
    } else {
        IntegrationOutcome::Ignored
    };
    record_outcome(&state, outcome, started);
    Json(summary).into_response()
}

pub(crate) async fn receive_report(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Result<Json<ReportWebhook>, JsonRejection>,
) -> axum::response::Response {
    let started = Instant::now();
    let principal = match authenticate_integration(&state, &headers).await {
        Ok(value) => value,
        Err(response) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return response;
        }
    };
    let Json(payload) = match payload {
        Ok(value) => value,
        Err(error) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return (error.status(), "ugyldig JSON").into_response();
        }
    };
    let body = match render_report(&payload) {
        Ok(value) => value,
        Err(message) => {
            record_outcome(&state, IntegrationOutcome::Rejected, started);
            return (StatusCode::BAD_REQUEST, message).into_response();
        }
    };
    match state
        .integrations
        .deliver_report(IncomingReport {
            agent_id: principal.agent_id,
            credential_id: principal.credential_id,
            report_id: payload.report_id,
            body,
        })
        .await
    {
        Ok(DeliveryResult::Accepted(message)) => {
            let _ = state.chat.announce_persisted_message(message.id).await;
            record_outcome(&state, IntegrationOutcome::Accepted, started);
            Json(DeliverySummary {
                accepted: 1,
                duplicate: 0,
                ignored: 0,
            })
            .into_response()
        }
        Ok(DeliveryResult::Duplicate(_)) => {
            record_outcome(&state, IntegrationOutcome::Duplicate, started);
            Json(DeliverySummary {
                accepted: 0,
                duplicate: 1,
                ignored: 0,
            })
            .into_response()
        }
        Ok(DeliveryResult::IgnoredResolved) => {
            unreachable!("reports cannot be ignored as resolved")
        }
        Err(error) => {
            record_outcome(&state, IntegrationOutcome::Error, started);
            integration_repository_response(error)
        }
    }
}

fn record_outcome(state: &AppState, outcome: IntegrationOutcome, started: Instant) {
    state.operations.record_integration(
        outcome,
        u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX),
    );
}

async fn authenticate_integration(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<crate::agent::AgentPrincipal, axum::response::Response> {
    let credential = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (StatusCode::UNAUTHORIZED, "Bearer-nøkkel manglar").into_response())?;
    match state.agents.authenticate(credential).await {
        Ok(principal) if principal.provider == GRAFANA_PROVIDER => Ok(principal),
        Ok(_) | Err(RepositoryError::PermissionDenied) => {
            Err((StatusCode::UNAUTHORIZED, "ugyldig nøkkel").into_response())
        }
        Err(RepositoryError::Conflict) => {
            let mut response =
                (StatusCode::TOO_MANY_REQUESTS, "for mange førespurnader").into_response();
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                HeaderValue::from_static("60"),
            );
            Err(response)
        }
        Err(RepositoryError::Storage(_)) => {
            Err((StatusCode::SERVICE_UNAVAILABLE, "mellombels utilgjengeleg").into_response())
        }
        Err(RepositoryError::NotFound) => {
            Err((StatusCode::UNAUTHORIZED, "ugyldig nøkkel").into_response())
        }
    }
}

fn integration_repository_response(error: RepositoryError) -> axum::response::Response {
    match error {
        RepositoryError::PermissionDenied => {
            (StatusCode::FORBIDDEN, "integrasjonen har ikkje tilgang").into_response()
        }
        RepositoryError::Storage(_) => {
            (StatusCode::SERVICE_UNAVAILABLE, "mellombels utilgjengeleg").into_response()
        }
        RepositoryError::Conflict => (StatusCode::CONFLICT, "konflikt").into_response(),
        RepositoryError::NotFound => {
            (StatusCode::NOT_FOUND, "ressursen finst ikkje").into_response()
        }
    }
}

fn validate_alert(alert: &GrafanaAlert) -> Result<AlertState, &'static str> {
    if alert.fingerprint.is_empty() || alert.fingerprint.len() > 128 {
        return Err("ugyldig fingerprint");
    }
    bounded_map(&alert.labels, 40, 80, 500)?;
    bounded_map(&alert.annotations, 40, 80, 4_000)?;
    match alert.status.as_str() {
        "firing" => Ok(AlertState::Firing),
        "resolved" if alert.ends_at.is_some() => Ok(AlertState::Resolved),
        "resolved" => Err("resolved alarm manglar endsAt"),
        _ => Err("status må vere firing eller resolved"),
    }
}

fn bounded_map(
    map: &HashMap<String, String>,
    count: usize,
    key: usize,
    value: usize,
) -> Result<(), &'static str> {
    if map.len() > count || map.iter().any(|(k, v)| k.len() > key || v.len() > value) {
        Err("for store labels eller annotations")
    } else {
        Ok(())
    }
}

fn render_alert(
    alert: &GrafanaAlert,
    state: AlertState,
    truncated: Option<u32>,
) -> Result<MessageBody, &'static str> {
    let icon = if state == AlertState::Firing {
        "🔴"
    } else {
        "🟢"
    };
    let severity = label(alert, &["severity", "priority"]);
    let service = label(alert, &["service", "app", "job"]);
    let namespace = label(alert, &["namespace"]);
    let summary = annotation(alert, &["summary", "description"])
        .or_else(|| label(alert, &["alertname"]))
        .unwrap_or("Grafana-alarm");
    let mut output = format!("## {icon} {}", escape(summary, 1_000));
    for (name, value) in [
        ("Status", Some(state.as_str())),
        ("Alvor", severity),
        ("Teneste", service),
        ("Namespace", namespace),
    ] {
        if let Some(value) = value {
            output.push_str(&format!("\n- {name}: {}", escape(value, 500)));
        }
    }
    output.push_str(&format!("\n- Start: {}", alert.starts_at.to_rfc3339()));
    if let Some(end) = alert.ends_at {
        output.push_str(&format!("\n- Slutt: {}", end.to_rfc3339()));
    }
    for (name, value) in [
        ("Dashboard", &alert.dashboard_url),
        ("Panel", &alert.panel_url),
        ("Kjelde", &alert.generator_url),
        ("Runbook", &alert.annotations.get("runbook_url").cloned()),
    ] {
        if let Some(url) = value.as_deref().and_then(safe_url) {
            output.push_str(&format!("\n- {name}: {url}"));
        }
    }
    if let Some(count) = truncated.filter(|count| *count > 0) {
        output.push_str(&format!(
            "\n\n⚠️ Grafana utelét {count} alarmar frå gruppa."
        ));
    }
    MessageBody::new(output).map_err(|_| "formatert alarm er for stor")
}

fn render_report(report: &ReportWebhook) -> Result<MessageBody, &'static str> {
    if report.version != "1"
        || report.report_id.is_empty()
        || report.report_id.len() > 128
        || report.title.is_empty()
        || report.title.len() > 500
        || report.summary.len() > 8_000
        || report.period_end < report.period_start
        || report.links.len() > MAX_LINKS
    {
        return Err("ugyldig rapport");
    }
    let mut output = format!(
        "## 📊 {}\n{}\n\n{} – {}",
        escape(&report.title, 500),
        escape(&report.summary, 8_000),
        report.period_start.to_rfc3339(),
        report.period_end.to_rfc3339()
    );
    for link in &report.links {
        if link.label.is_empty() || link.label.len() > 120 {
            return Err("ugyldig rapportlenkje");
        }
        let Some(url) = safe_url(&link.url) else {
            return Err("ugyldig rapportlenkje");
        };
        output.push_str(&format!("\n- {}: {url}", escape(&link.label, 120)));
    }
    MessageBody::new(output).map_err(|_| "formatert rapport er for stor")
}

fn label<'a>(alert: &'a GrafanaAlert, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| alert.labels.get(*key).map(String::as_str))
}
fn annotation<'a>(alert: &'a GrafanaAlert, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| alert.annotations.get(*key).map(String::as_str))
}

fn safe_url(value: &str) -> Option<String> {
    if value.len() > 2_048 || value.chars().any(char::is_control) {
        return None;
    }
    let url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url.to_string())
}

fn escape(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for character in value.chars().take(max_chars) {
        output.push(match character {
            '@' => '＠',
            '\\' => '＼',
            '*' => '＊',
            '_' => '＿',
            '[' => '［',
            ']' => '］',
            '`' => '｀',
            '#' => '＃',
            '<' => '‹',
            '>' => '›',
            '|' => '｜',
            '\n' | '\r' | '\t' => ' ',
            value if value.is_control() => ' ',
            value => value,
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_text_cannot_create_mentions_or_media() {
        let escaped = escape("@alle **fare** [[media:id|x]]", 100);
        assert!(!escaped.contains('@'));
        assert!(!escaped.contains("[[media:"));
        assert!(escaped.contains("＠alle"));
    }

    #[test]
    fn unsafe_links_are_rejected() {
        assert!(safe_url("https://grafana.example/d/one").is_some());
        assert!(safe_url("https://user:secret@example.test/").is_none());
        assert!(safe_url("javascript:alert(1)").is_none());
    }

    #[test]
    fn grafana_alert_uses_the_supported_markdown_template() {
        let alert = GrafanaAlert {
            status: "firing".to_owned(),
            labels: HashMap::from([
                ("severity".to_owned(), "critical".to_owned()),
                ("service".to_owned(), "api".to_owned()),
            ]),
            annotations: HashMap::from([
                ("summary".to_owned(), "API @alle er nede".to_owned()),
                (
                    "runbook_url".to_owned(),
                    "https://grafana.example/runbook/api".to_owned(),
                ),
            ]),
            starts_at: "2026-09-14T10:00:00Z".parse().unwrap(),
            ends_at: None,
            fingerprint: "abc".to_owned(),
            generator_url: None,
            dashboard_url: Some("https://grafana.example/d/api".to_owned()),
            panel_url: None,
        };
        let body = render_alert(&alert, AlertState::Firing, Some(2)).unwrap();
        assert!(body.as_str().starts_with("## 🔴 API ＠alle er nede"));
        assert!(body.as_str().contains("\n- Alvor: critical"));
        assert!(
            body.as_str()
                .contains("Dashboard: https://grafana.example/d/api")
        );
        assert!(body.as_str().contains("utelét 2 alarmar"));
        assert!(!body.as_str().contains('@'));
    }
}
