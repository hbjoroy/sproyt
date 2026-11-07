use super::start_test_server_with_state;
use crate::{
    db::SqliteChatRepository,
    domain::{ChannelSlug, DisplayName},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::StatusCode;
use std::{sync::Arc, time::Duration};

#[tokio::test]
async fn circle_process_http_requires_current_account_and_valid_scope() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let (address, server, state) =
        start_test_server_with_state(repository, Duration::from_secs(60)).await;
    let owner = state
        .auth
        .authenticate_request(Some("process-http-owner".into()), None)
        .await
        .unwrap();
    let other = state
        .auth
        .authenticate_request(Some("process-http-other".into()), None)
        .await
        .unwrap();
    state.chat.ensure_user(owner.user.clone()).await.unwrap();
    state.chat.ensure_user(other.user.clone()).await.unwrap();
    let circle = state
        .chat
        .create_circle(
            owner.user.id.clone(),
            ChannelSlug::new("process-http-circle").unwrap(),
            DisplayName::new("Process HTTP circle").unwrap(),
        )
        .await
        .unwrap();
    let client = reqwest::Client::new();
    let base = format!("http://{address}/api/v1/circles/{}/processes", circle.id);
    let account = owner.user.id.to_string();
    let own = format!("{base}?participant=process-http-owner");
    for suffix in ["", "&state=open&limit=1", "&state=all&limit=50"] {
        let response = client
            .get(format!("{own}{suffix}"))
            .header("x-sproyt-account", &account)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{suffix}");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap(),
            serde_json::json!({"processes": [], "next_cursor": null})
        );
    }
    for header in [None, Some(other.user.id.to_string()), Some(String::new())] {
        let mut request = client.get(&own);
        if let Some(header) = header {
            request = request.header("x-sproyt-account", header);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        client
            .get(&own)
            .header("x-sproyt-account", &account)
            .header("x-sproyt-account", &account)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN,
        "duplicate account fences must be rejected"
    );
    for header in [&account, &other.user.id.to_string()] {
        assert_eq!(
            client
                .get(format!("{base}?participant=process-http-other"))
                .header("x-sproyt-account", header)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN,
            "an account header cannot authenticate or grant circle access"
        );
    }
    let wrong_circle_cursor = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "circle": uuid::Uuid::new_v4().to_string(), "all": false, "at": 0, "key": ""
        }))
        .unwrap(),
    );
    let wrong_state_cursor = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&serde_json::json!({
            "circle": circle.id.to_string(), "all": true, "at": 0, "key": ""
        }))
        .unwrap(),
    );
    for suffix in [
        "&state=unknown".to_owned(),
        "&limit=0".to_owned(),
        "&limit=51".to_owned(),
        "&limit=-1".to_owned(),
        "&after=not-a-cursor".to_owned(),
        "&extra=ignored".to_owned(),
        format!("&after={wrong_circle_cursor}"),
        format!("&after={wrong_state_cursor}"),
    ] {
        assert_eq!(
            client
                .get(format!("{own}{suffix}"))
                .header("x-sproyt-account", &account)
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST,
            "{suffix}"
        );
    }
    server.abort();
}
