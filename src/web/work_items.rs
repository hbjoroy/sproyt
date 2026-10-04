use crate::{
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
    work_items::{
        Decision, ExportCommand, Registration, StatusDecision, StatusStart, SupplementCommand,
    },
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

#[derive(serde::Deserialize)]
pub(crate) struct TaskLookup {
    participant: Option<String>,
    message_id: Uuid,
}

pub(crate) async fn source_items(
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
        .source_items(principal.user.id, channel, message)
        .await
    {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn supplement(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<SupplementCommand>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.add_supplement(principal.user.id, id, body).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn start_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<StatusStart>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.start_status(principal.user.id, id, body).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn change_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<StatusDecision>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.change_status(principal.user.id, id, body).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn public_status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<TaskLookup>,
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
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service
        .public_status(principal.user.id, id, query.message_id)
        .await
    {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<TaskLookup>,
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
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.task(principal.user.id, id, query.message_id).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn decide(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<Decision>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.decide(principal.user.id, id, body).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn export_github(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ExportCommand>,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(p) => p,
        Err(e) => return auth_error_response(e),
    };
    let Some(service) = state.work_items else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.export_github(principal.user.id, id, body).await {
        Ok(value) => ([(header::CACHE_CONTROL, "private, no-store")], Json(value)).into_response(),
        Err(error) => repository_response(error),
    }
}

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
