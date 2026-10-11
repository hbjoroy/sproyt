use crate::{
    media_store::{CHUNK_BYTES, NewUpload, UploadError},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};
use axum::{
    Json,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

static CHUNK_TRANSFERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
static FINALIZATIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

pub(crate) async fn configuration(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    if let Err(error) = authenticate_http(&state, query, &headers).await {
        return auth_error_response(error);
    }
    private_json(
        serde_json::json!({"enabled":state.media_store.is_some(),"allowed_channel_ids":state.media_store.as_ref().and_then(|s|s.allowed_channels()),"max_file_bytes":state.media_store.as_ref().map(|s|s.max_file_bytes()).unwrap_or(35*1024*1024),"chunk_bytes":CHUNK_BYTES}),
    )
}

fn private_json(value: impl serde::Serialize) -> Response {
    ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}
fn unavailable() -> Response {
    (StatusCode::NOT_FOUND, "Chunk uploads are not enabled.").into_response()
}
fn busy() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::RETRY_AFTER, "2")],
        "Opplasting er opptatt. Prøv igjen om litt.",
    )
        .into_response()
}
fn failure(error: UploadError) -> Response {
    let status = match &error {
        UploadError::NotFound => StatusCode::NOT_FOUND,
        UploadError::Conflict => StatusCode::CONFLICT,
        UploadError::Quota => StatusCode::INSUFFICIENT_STORAGE,
        UploadError::Invalid => StatusCode::BAD_REQUEST,
        UploadError::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        UploadError::Storage(_) => {
            tracing::warn!(%error,"media upload operation failed");
            StatusCode::SERVICE_UNAVAILABLE
        }
    };
    (
        status,
        [(header::CACHE_CONTROL, "private, no-store")],
        error.to_string(),
    )
        .into_response()
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<NewUpload>,
) -> Response {
    if !crate::web::stream::same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(error) => return auth_error_response(error),
    };
    let Some(store) = &state.media_store else {
        return unavailable();
    };
    match store.create(&principal.user.id, channel, &input).await {
        Ok(status) => (StatusCode::CREATED, private_json(status)).into_response(),
        Err(error) => failure(error),
    }
}

pub(crate) async fn status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(error) => return auth_error_response(error),
    };
    let Some(store) = &state.media_store else {
        return unavailable();
    };
    match store.status(&principal.user.id, id).await {
        Ok(status) => private_json(status),
        Err(error) => failure(error),
    }
}

pub(crate) async fn put_chunk(
    State(state): State<AppState>,
    Path((id, index)): Path<(Uuid, u32)>,
    Query(query): Query<WsQuery>,
    request: Request,
) -> Response {
    let headers = request.headers();
    if !crate::web::stream::same_origin(headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = match authenticate_http(&state, query, headers).await {
        Ok(p) => p,
        Err(error) => return auth_error_response(error),
    };
    let Some(store) = &state.media_store else {
        return unavailable();
    };
    let hash = match headers
        .get("x-chunk-sha256")
        .and_then(|value| value.to_str().ok())
    {
        Some(value)
            if value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) =>
        {
            value.to_string()
        }
        _ => return failure(UploadError::Invalid),
    };
    if let Err(error) = store.status(&principal.user.id, id).await {
        return failure(error);
    }
    let _permit = match CHUNK_TRANSFERS.try_acquire() {
        Ok(p) => p,
        Err(_) => return busy(),
    };
    // Explicit bound after authentication/admission. No eager Bytes extractor.
    let bytes = match tokio::time::timeout(
        std::time::Duration::from_secs(60),
        axum::body::to_bytes(request.into_body(), CHUNK_BYTES),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    match store
        .put_chunk(&principal.user.id, id, index, &bytes, &hash)
        .await
    {
        Ok(status) => private_json(status),
        Err(error) => failure(error),
    }
}

pub(crate) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    if !crate::web::stream::same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(error) => return auth_error_response(error),
    };
    let Some(store) = &state.media_store else {
        return unavailable();
    };
    let _permit = match FINALIZATIONS.try_acquire() {
        Ok(p) => p,
        Err(_) => return busy(),
    };
    let id = match store.complete(&principal.user.id, id).await {
        Ok(id) => id,
        Err(error) => return failure(error),
    };
    match state.chat.media_metadata(principal.user.id, id).await {
        Ok(media) => {
            private_json(serde_json::json!({"media":media,"url":format!("/api/v1/media/{id}")}))
        }
        Err(error) => chat_error_response(error),
    }
}

pub(crate) async fn abort(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    if !crate::web::stream::same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(error) => return auth_error_response(error),
    };
    let Some(store) = &state.media_store else {
        return unavailable();
    };
    match store.abort(&principal.user.id, id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => failure(error),
    }
}
