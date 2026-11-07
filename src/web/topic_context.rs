use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{ChannelId, ChannelSequence, MessageId, MessageLimit},
    server::AppState,
    web::{
        http::{WsQuery, chat_error_response},
        personal_space::authenticated_user,
    },
};

#[derive(Deserialize)]
pub(crate) struct ContextQuery {
    participant: Option<String>,
    message_id: Uuid,
    sequence: u64,
}

pub(crate) async fn topic_context(
    State(state): State<AppState>,
    Path(channel_id): Path<Uuid>,
    Query(query): Query<ContextQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let actor = authenticated_user(
        &state,
        WsQuery {
            participant: query.participant,
        },
        &headers,
    )
    .await?
    .id;

    let sequence = query.sequence;
    if sequence == 0 || sequence >= i64::MAX as u64 {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }
    let channel = ChannelId::from_uuid(channel_id);
    let target = MessageId::from_uuid(query.message_id);
    let mut messages = state
        .chat
        .load_messages(
            actor.clone(),
            channel.clone(),
            MessageLimit::new(20),
            None,
            Some(ChannelSequence::new(sequence + 1)),
        )
        .await
        .map_err(chat_error_response)?;
    if !messages.iter().any(|message| {
        message.id == target
            && u64::from(message.sequence) == sequence
            && message.parent_message_id.is_none()
            && message.deleted_at.is_none()
    }) {
        return Err(StatusCode::NOT_FOUND.into_response());
    }

    let newer = state
        .chat
        .load_messages(
            actor,
            channel,
            MessageLimit::new(20),
            Some(ChannelSequence::new(sequence)),
            None,
        )
        .await
        .map_err(chat_error_response)?;
    messages.extend(newer);
    messages.retain(|message| message.parent_message_id.is_none() && message.deleted_at.is_none());
    Ok((
        [(CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({ "messages": messages })),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChannelKind, ChannelSlug, DisplayName, MessageBody};

    #[tokio::test]
    async fn context_requires_membership_and_an_exact_root_message() {
        let auth = crate::auth::AuthService::development();
        let alice = auth
            .authenticate_development(Some("topic-context-alice".into()))
            .unwrap()
            .user;
        let bob = auth
            .authenticate_development(Some("topic-context-bob".into()))
            .unwrap()
            .user;
        let state = crate::web::native::test_state(auth, None).await;
        state.chat.ensure_user(alice.clone()).await.unwrap();
        state.chat.ensure_user(bob.clone()).await.unwrap();
        let circle = state
            .chat
            .create_circle(
                alice.id.clone(),
                ChannelSlug::new("topic-context-circle").unwrap(),
                DisplayName::new("Circle").unwrap(),
            )
            .await
            .unwrap();
        let channel = state
            .chat
            .create_channel(
                alice.id.clone(),
                ChannelSlug::new("topic-context-private").unwrap(),
                DisplayName::new("Private").unwrap(),
                ChannelKind::Private,
                Some(circle.id),
            )
            .await
            .unwrap();
        let mut roots = Vec::new();
        for body in ["Before", "Target", "After"] {
            roots.push(
                state
                    .chat
                    .send_message(
                        channel.id.clone(),
                        alice.id.clone(),
                        MessageBody::new(body).unwrap(),
                    )
                    .await
                    .unwrap(),
            );
        }
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/v1/channels/{}/topics/context",
            listener.local_addr().unwrap(),
            channel.id
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let request = |participant: &str, owner: String, message: MessageId, sequence: u64| {
            client
                .get(&url)
                .header("x-sproyt-account", owner)
                .query(&[
                    ("participant", participant.to_owned()),
                    ("message_id", message.as_uuid().to_string()),
                    ("sequence", sequence.to_string()),
                ])
                .send()
        };
        let response = request(
            "topic-context-alice",
            alice.id.to_string(),
            roots[1].id,
            roots[1].sequence.into(),
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        let value: serde_json::Value = response.json().await.unwrap();
        let bodies: Vec<_> = value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["body"].as_str().unwrap())
            .collect();
        assert_eq!(bodies, ["Before", "Target", "After"]);
        assert_eq!(
            request(
                "topic-context-alice",
                alice.id.to_string(),
                roots[0].id,
                roots[1].sequence.into(),
            )
            .await
            .unwrap()
            .status(),
            StatusCode::NOT_FOUND
        );
        for participant in ["topic-context-alice", "topic-context-bob"] {
            assert_eq!(
                request(
                    participant,
                    bob.id.to_string(),
                    roots[1].id,
                    roots[1].sequence.into(),
                )
                .await
                .unwrap()
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        server.abort();
    }
}
