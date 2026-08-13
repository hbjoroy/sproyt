use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    WsQuery,
    agent::{AgentScope, CreateAgent, GrantAgent},
    auth_error_response, authenticate_http,
    domain::{ChannelId, UserId},
    repository_response,
    server::AppState,
};

#[derive(Deserialize)]
pub(crate) struct CreateAgentRequest {
    display_name: String,
    provider: String,
    service_identity: String,
    purpose: String,
    #[serde(default = "default_agent_rate_limit")]
    rate_limit_per_minute: u16,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

const fn default_agent_rate_limit() -> u16 {
    60
}

pub(crate) async fn create_agent(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<CreateAgentRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    match state
        .agents
        .create(CreateAgent {
            actor: principal.user.id.clone(),
            owner_id: principal.user.id,
            display_name: body.display_name,
            provider: body.provider,
            service_identity: body.service_identity,
            purpose: body.purpose,
            rate_limit_per_minute: body.rate_limit_per_minute,
            expires_at: body.expires_at,
        })
        .await
    {
        Ok(created) => {
            let mut response = (axum::http::StatusCode::CREATED, Json(created)).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct GrantAgentRequest {
    circle_id: Option<String>,
    channel_id: Option<String>,
    scope: AgentScope,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) async fn grant_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<GrantAgentRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let agent_id = match UserId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let circle_id = match body
        .circle_id
        .map(|id| uuid::Uuid::parse_str(&id).map(crate::domain::CircleId::from_uuid))
        .transpose()
    {
        Ok(id) => id,
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid circle id").into_response();
        }
    };
    let channel_id = match body.channel_id.map(ChannelId::new).transpose() {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .agents
        .grant(GrantAgent {
            actor: principal.user.id,
            agent_id,
            circle_id,
            channel_id,
            scope: body.scope,
            expires_at: body.expires_at,
        })
        .await
    {
        Ok(id) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({"grant_id":id})),
        )
            .into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn revoke_agent_grant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let grant_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => return (axum::http::StatusCode::BAD_REQUEST, "invalid grant id").into_response(),
    };
    match state.agents.revoke(principal.user.id, grant_id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn revoke_agent(
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
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state.agents.revoke_agent(principal.user.id, agent_id).await {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

pub(crate) async fn approve_agent_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let message_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => crate::domain::MessageId::from_uuid(id),
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid message id").into_response();
        }
    };
    match state
        .agents
        .approve_message(principal.user.id, message_id)
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}
