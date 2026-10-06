//! Authenticated self-service only; no administrator-selected user parameter.
use crate::{
    chatbot::memory::repository::{ActionInput, ChoiceInput, Mutation},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

pub(crate) async fn read(
    State(state): State<AppState>,
    Path((circle, agent)): Path<(uuid::Uuid, uuid::Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return private(auth_error_response(error)),
    };
    let Some(service) = &state.chat_agents else {
        return private(axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    private(
        match service
            .read_memory(&principal.user.id, &circle.to_string(), &agent.to_string())
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => repository_response(error),
        },
    )
}

pub(crate) async fn choice(
    State(state): State<AppState>,
    Path((circle, agent)): Path<(uuid::Uuid, uuid::Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<ChoiceInput>,
) -> Response {
    mutate(
        state,
        circle,
        agent,
        query,
        headers,
        Mutation::Choice(input),
    )
    .await
}

pub(crate) async fn action(
    State(state): State<AppState>,
    Path((circle, agent)): Path<(uuid::Uuid, uuid::Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<ActionInput>,
) -> Response {
    mutate(
        state,
        circle,
        agent,
        query,
        headers,
        Mutation::Action(input),
    )
    .await
}

async fn mutate(
    state: AppState,
    circle: uuid::Uuid,
    agent: uuid::Uuid,
    query: WsQuery,
    headers: HeaderMap,
    input: Mutation,
) -> Response {
    if !crate::web::stream::same_origin(&headers) {
        return private(axum::http::StatusCode::FORBIDDEN.into_response());
    }
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return private(auth_error_response(error)),
    };
    let Some(service) = &state.chat_agents else {
        return private(axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    private(
        match service
            .mutate_memory(
                &principal.user.id,
                &circle.to_string(),
                &agent.to_string(),
                input,
            )
            .await
        {
            Ok(view) => Json(view).into_response(),
            Err(error) => repository_response(error),
        },
    )
}
