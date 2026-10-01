use crate::{
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
    work_items::Registration,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

pub(crate) async fn applications(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
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
    match service.applications(principal.user.id, channel).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn draft(
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
    match service.draft(principal.user.id, channel, message).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn register(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Registration>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.register(principal.user.id, channel, body).await {
        Ok(value) => (
            StatusCode::ACCEPTED,
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(value),
        )
            .into_response(),
        Err(error) => repository_response(error),
    }
}
