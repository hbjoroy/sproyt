use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::{
    domain::{ChannelId, MediaUpload, UserId},
    imagegen::{ImageGeneration, Job},
    server::AppState,
    web::{
        http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
        media::prepare_uploaded_media,
    },
};

fn unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Image generation is not enabled.",
    )
        .into_response()
}
fn internal() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Could not update your image request. Please retry.",
    )
        .into_response()
}
fn private_json(value: serde_json::Value) -> Response {
    ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}

#[derive(Deserialize)]
pub(crate) struct Request {
    channel_id: String,
    request_id: Uuid,
    prompt: String,
}

pub(crate) async fn enqueue(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Request>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = &state.imagegen else {
        return unavailable();
    };
    let channel = match ChannelId::new(body.channel_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid channel").into_response(),
    };
    if let Err(e) = state
        .chat
        .list_channel_users(principal.user.id.clone(), channel.clone())
        .await
    {
        return chat_error_response(e);
    }
    let prompt = body.prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > 2000 {
        return (
            StatusCode::BAD_REQUEST,
            "Use a prompt between 1 and 2000 characters.",
        )
            .into_response();
    }
    match service
        .enqueue(principal.user.id, channel, body.request_id, prompt.into())
        .await
    {
        Ok(job) => private_json(json!({"job":job.view()})),
        Err(e) => {
            tracing::warn!(error=%e,"image request admission failed");
            (
                StatusCode::CONFLICT,
                "Review your current image first, or try again when the queue has room.",
            )
                .into_response()
        }
    }
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = &state.imagegen else {
        return private_json(json!({"enabled":false,"jobs":[]}));
    };
    match service.list(principal.user.id).await {
        Ok(jobs) => private_json(
            json!({"enabled":true,"jobs":jobs.iter().map(Job::view).collect::<Vec<_>>()}),
        ),
        Err(_) => internal(),
    }
}

async fn owned(service: &ImageGeneration, id: &str, owner: UserId) -> Result<Job, Response> {
    match service.get(id).await {
        Ok(Some(job)) if job.owner_id == owner => Ok(job),
        Ok(_) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(_) => Err(internal()),
    }
}

pub(crate) async fn preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = &state.imagegen else {
        return unavailable();
    };
    let job = match owned(service, &id, principal.user.id.clone()).await {
        Ok(j) => j,
        Err(r) => return r,
    };
    if !matches!(job.state.as_str(), "ready" | "accepting" | "accepted") {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(e) = state
        .chat
        .list_channel_users(principal.user.id.clone(), job.channel_id.clone())
        .await
    {
        return chat_error_response(e);
    }
    match job.image.and_then(|s| STANDARD.decode(s).ok()) {
        Some(bytes) => (
            [
                (header::CONTENT_TYPE, "image/png"),
                (header::CACHE_CONTROL, "private, no-store"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            bytes,
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Decision {
    Accept,
    Decline,
    Dismiss,
}

#[derive(Deserialize)]
pub(crate) struct Review {
    decision: Decision,
}

pub(crate) async fn review(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Review>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = &state.imagegen else {
        return unavailable();
    };
    let mut job = match owned(service, &id, principal.user.id.clone()).await {
        Ok(j) => j,
        Err(r) => return r,
    };
    if let Err(e) = state
        .chat
        .list_channel_users(principal.user.id.clone(), job.channel_id.clone())
        .await
    {
        return chat_error_response(e);
    }
    if service.published(&job).await.unwrap_or(true) {
        return (StatusCode::CONFLICT, "This image has already been posted.").into_response();
    }
    match body.decision {
        Decision::Accept if job.state == "accepted" => {
            return private_json(json!({"job":job.view(),"media":job.media}));
        }
        Decision::Accept if job.state == "ready" => {
            job.transition("accepting");
            if !service.save(&mut job).await.unwrap_or(false) {
                return StatusCode::CONFLICT.into_response();
            }
            let bytes = match job.image.as_ref().and_then(|s| STANDARD.decode(s).ok()) {
                Some(b) => b,
                None => return internal(),
            };
            let prepared = match prepare_uploaded_media(bytes, "image/png").await {
                Ok(p) => p,
                Err(_) => return internal(),
            };
            match state
                .chat
                .store_media(MediaUpload {
                    actor: principal.user.id,
                    channel_id: job.channel_id.clone(),
                    filename: format!("heartsync-{}.png", job.id),
                    content_type: "image/png".into(),
                    content: prepared.0,
                    dimensions: prepared.1,
                    preview: prepared.2,
                })
                .await
            {
                Ok(media) => {
                    job.media = Some(media);
                    job.transition("accepted");
                }
                Err(e) => {
                    job.transition("ready");
                    let _ = service.save(&mut job).await;
                    return chat_error_response(e);
                }
            }
        }
        Decision::Decline if matches!(job.state.as_str(), "ready" | "declined") => {
            job.transition("declined");
            job.image = None;
            job.prompt.clear();
        }
        Decision::Dismiss if matches!(job.state.as_str(), "accepted" | "failed" | "dismissed") => {
            job.transition("dismissed");
            job.image = None;
            job.prompt.clear();
        }
        _ => {
            return (
                StatusCode::CONFLICT,
                "This image is not ready for that action.",
            )
                .into_response();
        }
    }
    match service.save(&mut job).await {
        Ok(true) => private_json(json!({"job":job.view(),"media":job.media})),
        Ok(false) => StatusCode::CONFLICT.into_response(),
        Err(_) => internal(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn imagegen_private_job_cannot_be_read_or_reviewed_by_another_owner() {
        let service = ImageGeneration::test("http://unused").await;
        let alice = UserId::named("alice");
        let job = service
            .enqueue(
                alice.clone(),
                ChannelId::generate(),
                Uuid::now_v7(),
                "private prompt".into(),
            )
            .await
            .unwrap();
        assert!(owned(&service, &job.id, alice).await.is_ok());
        assert_eq!(
            owned(&service, &job.id, UserId::named("bob"))
                .await
                .err()
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            owned(&service, "unknown", UserId::named("bob"))
                .await
                .err()
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        assert!(service.list(UserId::named("bob")).await.unwrap().is_empty());
    }
}
