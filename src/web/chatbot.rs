use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
};

use crate::{
    chatbot::AgentInput,
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(circle): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    if uuid::Uuid::parse_str(&circle).is_err() {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid circle id").into_response();
    }
    let Some(service) = &state.chat_agents else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.list(&principal.user.id, &circle).await {
        Ok(agents) => {
            Json(serde_json::json!({"agents":agents,"worker_available":service.available()}))
                .into_response()
        }
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn create(
    State(state): State<AppState>,
    Path(circle): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<AgentInput>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    if uuid::Uuid::parse_str(&circle).is_err() {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid circle id").into_response();
    }
    let Some(service) = &state.chat_agents else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service.create(&principal.user.id, &circle, input).await {
        Ok(agent) => (axum::http::StatusCode::CREATED, Json(agent)).into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn update(
    State(state): State<AppState>,
    Path((circle, id)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<AgentInput>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    if uuid::Uuid::parse_str(&circle).is_err() || uuid::Uuid::parse_str(&id).is_err() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "invalid agent or circle id",
        )
            .into_response();
    }
    let Some(service) = &state.chat_agents else {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    match service
        .update(&principal.user.id, &circle, &id, input)
        .await
    {
        Ok(agent) => Json(agent).into_response(),
        Err(error) => repository_response(error),
    }
}
