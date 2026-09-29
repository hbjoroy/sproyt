use crate::{
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct Lookup {
    participant: Option<String>,
    message_id: Uuid,
}
#[derive(Deserialize)]
pub(crate) struct Enable {
    enabled: bool,
}
#[derive(Deserialize)]
pub(crate) struct Start {
    request_id: Uuid,
}
#[derive(Deserialize)]
pub(crate) struct Complete {
    request_id: Uuid,
    message_id: Uuid,
}
fn private(value: impl serde::Serialize) -> Response {
    ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}
fn disabled() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        "Process pilot is not enabled.",
    )
        .into_response()
}

pub(crate) async fn configuration(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.process_pilot else {
        return private(json!({"configured":false,"can_configure":false,"can_start":false}));
    };
    match service
        .configuration(&principal.user.id, &id.to_string())
        .await
    {
        Ok(v) => private(v),
        Err(e) => repository_response(e),
    }
}
pub(crate) async fn configure(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Enable>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.process_pilot else {
        return disabled();
    };
    match service
        .configure(&principal.user.id, &id.to_string(), body.enabled)
        .await
    {
        Ok(v) => private(v),
        Err(e) => repository_response(e),
    }
}
pub(crate) async fn start(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Start>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.process_pilot else {
        return disabled();
    };
    match service
        .start(&principal.user.id, &id.to_string(), body.request_id)
        .await
    {
        Ok(v) => private(v),
        Err(e) => repository_response(e),
    }
}
pub(crate) async fn task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<Lookup>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(
        &state,
        WsQuery {
            participant: query.participant,
        },
        &headers,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.process_pilot else {
        return disabled();
    };
    match service
        .task(
            &principal.user.id,
            &id.to_string(),
            &query.message_id.to_string(),
        )
        .await
    {
        Ok(v) => private(v),
        Err(e) => repository_response(e),
    }
}
pub(crate) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Complete>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.process_pilot else {
        return disabled();
    };
    match service
        .complete(
            &principal.user.id,
            &id.to_string(),
            &body.message_id.to_string(),
            body.request_id,
        )
        .await
    {
        Ok(v) => private(v),
        Err(e) => repository_response(e),
    }
}
