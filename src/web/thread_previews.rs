use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    domain::{ChannelId, MessageId, THREAD_ROOT_PREVIEW_BATCH_MAX},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviewRequest {
    root_ids: Vec<MessageId>,
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(request): Json<PreviewRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    if headers
        .get("x-sproyt-account")
        .and_then(|v| v.to_str().ok())
        != Some(principal.user.id.to_string().as_str())
    {
        return (
            StatusCode::CONFLICT,
            "Kontoen vart endra. Last trådane på nytt.",
        )
            .into_response();
    }
    if request.root_ids.len() > THREAD_ROOT_PREVIEW_BATCH_MAX {
        return (StatusCode::BAD_REQUEST, "For mange trådar.").into_response();
    }
    let channel = match ChannelId::new(id) {
        Ok(channel) => channel,
        Err(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    match state
        .chat
        .thread_root_previews(principal.user.id, channel, request.root_ids)
        .await
    {
        Ok(roots) => (
            [(CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "roots": roots })),
        )
            .into_response(),
        Err(error) => chat_error_response(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChannelKind, ChannelSlug, DisplayName, MessageBody};

    #[tokio::test]
    async fn thread_preview_http_enforces_account_membership_and_limit() {
        let auth = crate::auth::AuthService::development();
        let alice = auth
            .authenticate_development(Some("thread-http-alice".into()))
            .unwrap()
            .user;
        let bob = auth
            .authenticate_development(Some("thread-http-bob".into()))
            .unwrap()
            .user;
        let state = crate::web::native::test_state(auth, None).await;
        state.chat.ensure_user(alice.clone()).await.unwrap();
        state.chat.ensure_user(bob.clone()).await.unwrap();
        let circle = state
            .chat
            .create_circle(
                alice.id.clone(),
                ChannelSlug::new("thread-http-circle").unwrap(),
                DisplayName::new("Circle").unwrap(),
            )
            .await
            .unwrap();
        let channel = state
            .chat
            .create_channel(
                alice.id.clone(),
                ChannelSlug::new("thread-http-private").unwrap(),
                DisplayName::new("Private").unwrap(),
                ChannelKind::Private,
                Some(circle.id),
            )
            .await
            .unwrap();
        let root = state
            .chat
            .send_message(
                channel.id.clone(),
                alice.id.clone(),
                MessageBody::new("Root").unwrap(),
            )
            .await
            .unwrap();
        state
            .chat
            .send_thread_reply_idempotent(
                channel.id.clone(),
                alice.id.clone(),
                root.id,
                MessageBody::new("Reply").unwrap(),
                "thread-http-reply".into(),
            )
            .await
            .unwrap();
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/api/v1/channels/{}/thread-root-previews",
            listener.local_addr().unwrap(),
            channel.id
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let body = serde_json::json!({"root_ids": [root.id]});
        let response = client
            .post(format!("{url}?participant=thread-http-alice"))
            .header("x-sproyt-account", alice.id.to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "no-store");
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(value["roots"][0]["preview"], "Root");
        let wrong_account = client
            .post(format!("{url}?participant=thread-http-alice"))
            .header("x-sproyt-account", bob.id.to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(wrong_account.status(), StatusCode::CONFLICT);
        let nonmember = client
            .post(format!("{url}?participant=thread-http-bob"))
            .header("x-sproyt-account", bob.id.to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(nonmember.status(), StatusCode::FORBIDDEN);
        let too_many = client
            .post(format!("{url}?participant=thread-http-alice"))
            .header("x-sproyt-account", alice.id.to_string())
            .json(&serde_json::json!({"root_ids": vec![root.id; 31]}))
            .send()
            .await
            .unwrap();
        assert_eq!(too_many.status(), StatusCode::BAD_REQUEST);
        server.abort();
    }
}
