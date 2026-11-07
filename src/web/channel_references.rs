use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    domain::{CHANNEL_REFERENCE_BATCH_MAX, ChannelId, ChannelSlug},
    server::AppState,
    web::{
        http::{WsQuery, chat_error_response},
        personal_space::authenticated_user,
    },
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReferenceRequest {
    slugs: Vec<String>,
}

pub(crate) async fn resolve(
    State(state): State<AppState>,
    Path(source): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(request): Json<ReferenceRequest>,
) -> Result<Response, Response> {
    let actor = authenticated_user(&state, query, &headers).await?.id;
    if request.slugs.len() > CHANNEL_REFERENCE_BATCH_MAX {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }
    let slugs = request
        .slugs
        .into_iter()
        .map(ChannelSlug::new)
        .collect::<Result<_, _>>()
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
    let channels = state
        .chat
        .resolve_channel_references(actor, ChannelId::from_uuid(source), slugs)
        .await
        .map_err(chat_error_response)?;
    Ok((
        [(CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({ "channels": channels })),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChannelKind, DisplayName};

    #[tokio::test]
    async fn channel_reference_http_checks_account_membership_and_batch() {
        let auth = crate::auth::AuthService::development();
        let alice = auth
            .authenticate_development(Some("reference-alice".into()))
            .unwrap()
            .user;
        let bob = auth
            .authenticate_development(Some("reference-bob".into()))
            .unwrap()
            .user;
        let state = crate::web::native::test_state(auth, None).await;
        state.chat.ensure_user(alice.clone()).await.unwrap();
        state.chat.ensure_user(bob.clone()).await.unwrap();
        let source = state
            .chat
            .create_channel(
                alice.id.clone(),
                ChannelSlug::new("reference-source").unwrap(),
                DisplayName::new("Source").unwrap(),
                ChannelKind::Private,
                None,
            )
            .await
            .unwrap();
        let target = state
            .chat
            .create_channel(
                alice.id.clone(),
                ChannelSlug::new("reference-target").unwrap(),
                DisplayName::new("Target").unwrap(),
                ChannelKind::Public,
                None,
            )
            .await
            .unwrap();
        let app = crate::server::build_router(state.clone(), state.operations.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "http://{}/api/v1/channels/{}/channel-references",
            listener.local_addr().unwrap(),
            source.id
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let body = serde_json::json!({"slugs": ["REFERENCE-TARGET"]});
        let alice_url = format!("{endpoint}?participant=reference-alice");
        let response = client
            .post(&alice_url)
            .header("x-sproyt-account", alice.id.to_string())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CACHE_CONTROL], "private, no-store");
        let value: serde_json::Value = response.json().await.unwrap();
        assert_eq!(value["channels"][0]["id"], target.id.to_string());
        assert_eq!(value["channels"][0]["slug"], "reference-target");
        assert_eq!(value["channels"][0]["is_member"], true);

        for url in [
            alice_url.clone(),
            format!("{endpoint}?participant=reference-bob"),
        ] {
            let forbidden = client
                .post(url)
                .header("x-sproyt-account", bob.id.to_string())
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        }
        let too_many = client
            .post(alice_url)
            .header("x-sproyt-account", alice.id.to_string())
            .json(&serde_json::json!({"slugs": vec!["x"; CHANNEL_REFERENCE_BATCH_MAX+1]}))
            .send()
            .await
            .unwrap();
        assert_eq!(too_many.status(), StatusCode::BAD_REQUEST);
        server.abort();
    }
}
