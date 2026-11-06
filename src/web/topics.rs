use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    chat::ChatError,
    domain::{ChannelId, UserId},
    server::AppState,
    topics::{TopicStore, valid_key},
    web::{
        http::{WsQuery, chat_error_response},
        personal_space::authenticated_user,
    },
};

async fn authorize(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<(UserId, TopicStore), Response> {
    let actor = authenticated_user(state, query, headers).await?.id;
    let store = state
        .topics
        .clone()
        .ok_or_else(|| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    Ok((actor, store))
}

fn private_json(value: impl Serialize) -> Response {
    ([(CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let (actor, store) = authorize(&state, query, &headers).await?;
    let result = store
        .summary(&actor, &ChannelId::from_uuid(channel))
        .await
        .map_err(|error| chat_error_response(ChatError::Repository(error)))?;
    Ok(private_json(result))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TopicSetting {
    enabled: bool,
}

pub(crate) async fn set_enabled(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(setting): Json<TopicSetting>,
) -> Result<Response, Response> {
    let (actor, store) = authorize(&state, query, &headers).await?;
    let result = store
        .set_enabled(&actor, &ChannelId::from_uuid(channel), setting.enabled)
        .await
        .map_err(|error| chat_error_response(ChatError::Repository(error)))?;
    Ok(private_json(result))
}

#[derive(Deserialize)]
pub(crate) struct MatchQuery {
    topic: String,
    before: Option<i64>,
}

pub(crate) async fn matches(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    Query(page): Query<MatchQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let (actor, store) = authorize(&state, query, &headers).await?;
    let before = page.before.unwrap_or(i64::MAX);
    if before <= 0 || !valid_key(&page.topic) {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }
    let result = store
        .matches(&actor, &ChannelId::from_uuid(channel), &page.topic, before)
        .await
        .map_err(|error| chat_error_response(ChatError::Repository(error)))?;
    Ok(private_json(result))
}
