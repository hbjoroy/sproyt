use crate::{
    chat::ChatError,
    domain::{ChannelId, MembershipRole, UserId},
    server::AppState,
    web::{
        http::{WsQuery, chat_error_response},
        personal_space::authenticated_user,
    },
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

fn failure(error: crate::domain::RepositoryError) -> Response {
    chat_error_response(ChatError::Repository(error))
}
async fn response(
    state: &AppState,
    actor: UserId,
    channel: ChannelId,
) -> Result<Response, Response> {
    let result = state
        .chat
        .channel_governance(actor, channel)
        .await
        .map_err(failure)?;
    Ok(([(CACHE_CONTROL, "private, no-store")], Json(result)).into_response())
}
pub(crate) async fn get(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let actor = authenticated_user(&state, query, &headers).await?.id;
    response(&state, actor, ChannelId::from_uuid(channel)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HiddenInput {
    hidden: bool,
}
pub(crate) async fn set_hidden(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<HiddenInput>,
) -> Result<Response, Response> {
    let actor = authenticated_user(&state, query, &headers).await?.id;
    let channel = ChannelId::from_uuid(channel);
    state
        .chat
        .set_channel_hidden(actor.clone(), channel.clone(), input.hidden)
        .await
        .map_err(failure)?;
    response(&state, actor, channel).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoleInput {
    role: MembershipRole,
}
pub(crate) async fn set_role(
    State(state): State<AppState>,
    Path((channel, user)): Path<(Uuid, Uuid)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<RoleInput>,
) -> Result<Response, Response> {
    let actor = authenticated_user(&state, query, &headers).await?.id;
    let channel = ChannelId::from_uuid(channel);
    state
        .chat
        .set_channel_member_role(
            actor.clone(),
            channel.clone(),
            UserId::from_uuid(user),
            input.role,
        )
        .await
        .map_err(failure)?;
    response(&state, actor, channel).await
}
