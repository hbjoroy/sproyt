use crate::{
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
    work_items::FeedbackReview,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

pub(crate) async fn get(
    State(state): State<AppState>,
    Path((channel, message)): Path<(Uuid, Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service
        .feedback_issue(principal.user.id, channel, message)
        .await
    {
        Ok(view) => ([(header::CACHE_CONTROL, "private, no-store")], Json(view)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn review(
    State(state): State<AppState>,
    Path((channel, message)): Path<(Uuid, Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<FeedbackReview>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service
        .review_feedback(principal.user.id, channel, message, body)
        .await
    {
        Ok(view) => ([(header::CACHE_CONTROL, "private, no-store")], Json(view)).into_response(),
        Err(error) => repository_response(error),
    }
}

fn signed_delivery(key: &[u8], body: &[u8], header: Option<&str>) -> bool {
    let Some(signature) = header
        .and_then(|s| s.strip_prefix("sha256="))
        .and_then(|s| hex::decode(s).ok())
    else {
        return false;
    };
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::verify(&key, body, &signature).is_ok()
}

pub(crate) async fn github_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> StatusCode {
    let Some(secret) = std::env::var("SPROYT_FEEDBACK_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        return StatusCode::NOT_FOUND;
    };
    if !signed_delivery(
        secret.as_bytes(),
        &body,
        headers
            .get("X-Hub-Signature-256")
            .and_then(|v| v.to_str().ok()),
    ) {
        return StatusCode::UNAUTHORIZED;
    }
    if headers.get("X-Github-Event").and_then(|v| v.to_str().ok()) != Some("issues") {
        return StatusCode::NO_CONTENT;
    }
    let Ok(event) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    if !matches!(
        event["action"].as_str(),
        Some("closed" | "reopened" | "status_changed")
    ) {
        return StatusCode::NO_CONTENT;
    }
    let (Some(installation), Some(repo), Some(issue)) = (
        event["installation"]["id"].as_i64(),
        event["repository"]["id"].as_i64(),
        event["issue"]["id"].as_i64(),
    ) else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match service.wake_feedback_issue(installation, repo, issue).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn feedback_webhook_verifies_original_bytes() {
        let key = b"local-test-key";
        let tag = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key),
            b"event",
        );
        let header = format!("sha256={}", hex::encode(tag));
        assert!(signed_delivery(key, b"event", Some(&header)));
        assert!(!signed_delivery(key, b"changed", Some(&header)));
        assert!(!signed_delivery(key, b"event", None));
    }
}
