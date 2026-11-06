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
    domain::{
        ChannelId, ChannelSequence, MessageId, MessageLimit, PersonalNote, PrincipalKind,
        RepositoryError, User, UserId,
    },
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};

pub(crate) async fn authenticated_user(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<User, Response> {
    let principal = authenticate_http(state, query, headers)
        .await
        .map_err(auth_error_response)?;
    if principal.user.kind != PrincipalKind::Human {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    if headers.get_all("x-sproyt-account").iter().count() != 1
        || headers
            .get("x-sproyt-account")
            .and_then(|value| value.to_str().ok())
            != Some(principal.user.id.to_string().as_str())
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Kontoen vart endra. Opne området på nytt.",
        )
            .into_response());
    }
    Ok(principal.user)
}

async fn owner(state: &AppState, query: WsQuery, headers: &HeaderMap) -> Result<UserId, Response> {
    let user = authenticated_user(state, query, headers).await?;
    let owner = user.id.clone();
    state
        .chat
        .ensure_user(user)
        .await
        .map_err(chat_error_response)?;
    Ok(owner)
}

fn private_json(value: impl Serialize) -> Response {
    ([(CACHE_CONTROL, "private, no-store")], Json(value)).into_response()
}

pub(crate) async fn open(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let owner = owner(&state, query, &headers).await?;
    let channel = state
        .chat
        .open_personal_space(owner)
        .await
        .map_err(chat_error_response)?;
    Ok(private_json(channel))
}

pub(crate) async fn note(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let owner = owner(&state, query, &headers).await?;
    let note = state
        .chat
        .personal_note(owner)
        .await
        .map_err(chat_error_response)?;
    Ok(private_json(note))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoteInput {
    markdown: String,
    revision: i64,
}

pub(crate) async fn save_note(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<NoteInput>,
) -> Result<Response, Response> {
    let owner = owner(&state, query, &headers).await?;
    if input.markdown.len() > 64 * 1024 || input.markdown.contains('\0') || input.revision < 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Notatet må vere gyldig tekst på høgst 64 KiB.",
        )
            .into_response());
    }
    let note = state
        .chat
        .save_personal_note(
            owner,
            PersonalNote {
                markdown: input.markdown,
                revision: input.revision,
            },
        )
        .await
        .map_err(|error| match error {
            ChatError::Repository(RepositoryError::Conflict) => {
                (StatusCode::CONFLICT, "Notatet har ei nyare lagra utgåve.").into_response()
            }
            error => chat_error_response(error),
        })?;
    Ok(private_json(note))
}

pub(crate) async fn save_link(
    State(state): State<AppState>,
    Path(message_id): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let owner = owner(&state, query, &headers).await?;
    let created = state
        .chat
        .save_personal_link(owner, MessageId::from_uuid(message_id))
        .await
        .map_err(chat_error_response)?;
    Ok(private_json(serde_json::json!({ "created": created })))
}

pub(crate) async fn source_message(
    State(state): State<AppState>,
    Path((channel_id, message_id, sequence)): Path<(Uuid, Uuid, i64)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let actor = authenticated_user(&state, query, &headers).await?.id;
    let unavailable = || {
        (
            StatusCode::NOT_FOUND,
            [(CACHE_CONTROL, "private, no-store")],
        )
            .into_response()
    };
    let sequence = u64::try_from(sequence)
        .ok()
        .filter(|sequence| *sequence > 0)
        .ok_or_else(unavailable)?;
    // Use the same membership gate as channel history without changing the read marker.
    let messages = state
        .chat
        .load_messages(
            actor,
            ChannelId::from_uuid(channel_id),
            MessageLimit::new(1),
            Some(ChannelSequence::new(sequence - 1)),
            None,
        )
        .await
        .map_err(|error| match error {
            ChatError::Repository(
                RepositoryError::PermissionDenied | RepositoryError::NotFound,
            ) => unavailable(),
            error => chat_error_response(error),
        })?;
    let source = messages
        .into_iter()
        .find(|message| {
            message.id == MessageId::from_uuid(message_id)
                && u64::from(message.sequence) == sequence
                && message.deleted_at.is_none()
        })
        .ok_or_else(unavailable)?;
    Ok(private_json(source))
}
