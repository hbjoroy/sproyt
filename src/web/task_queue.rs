use crate::{
    server::AppState,
    web::{
        http::{WsQuery, repository_response},
        personal_space::authenticated_user,
    },
    work_items::TaskQueueCursor,
};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub(crate) struct TaskQueueQuery {
    participant: Option<String>,
    channel_id: Option<Uuid>,
    cursor: Option<String>,
    limit: Option<usize>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Query(query): Query<TaskQueueQuery>,
    headers: HeaderMap,
) -> Response {
    let user = match authenticated_user(
        &state,
        WsQuery {
            participant: query.participant,
        },
        &headers,
    )
    .await
    {
        Ok(user) => user,
        Err(response) => return response,
    };
    let limit = query.limit.unwrap_or(25);
    if !(1..=100).contains(&limit) {
        return (StatusCode::BAD_REQUEST, "limit must be between 1 and 100").into_response();
    }
    let cursor = match query
        .cursor
        .as_deref()
        .map(str::parse::<TaskQueueCursor>)
        .transpose()
    {
        Ok(cursor) => cursor,
        Err(()) => return (StatusCode::BAD_REQUEST, "invalid cursor").into_response(),
    };
    let Some(service) = state.work_items else {
        return (
            [(header::CACHE_CONTROL, "private, no-store")],
            Json(serde_json::json!({"tasks": [], "next_cursor": null})),
        )
            .into_response();
    };
    match service
        .task_queue(
            user.id,
            query.channel_id,
            cursor,
            limit,
            state.process_pilot.as_ref(),
        )
        .await
    {
        Ok(page) => ([(header::CACHE_CONTROL, "private, no-store")], Json(page)).into_response(),
        Err(error) => repository_response(error),
    }
}
