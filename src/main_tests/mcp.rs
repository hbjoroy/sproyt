use super::*;
use crate::{
    agent::{AgentRepository, AgentService},
    db::SqliteChatRepository,
    domain::{ChannelKind, ChannelSlug, DisplayName, PrincipalKind, User},
    process::{ProcessRepository, ProcessService},
};
use chrono::Utc;
use std::sync::Arc;

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(response.into_body(), 1_048_576)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn mcp_uses_agent_scope_idempotency_and_immediate_revocation() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
    let process_repository: Arc<dyn ProcessRepository> = repository.clone();
    let agent_repository: Arc<dyn AgentRepository> = repository;
    let chat = ChatEngine::start(chat_repository);
    let agents = AgentService::new(agent_repository);
    let owner = UserId::named("mcp-owner");
    chat.ensure_user(User {
        id: owner.clone(),
        kind: PrincipalKind::Human,
        display_name: DisplayName::new("MCP owner").unwrap(),
        external_provider: Some("test".to_owned()),
        external_subject: Some("mcp-owner".to_owned()),
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let channel = chat
        .create_channel(
            owner.clone(),
            ChannelSlug::new("mcp-test").unwrap(),
            DisplayName::new("MCP test").unwrap(),
            ChannelKind::Private,
            None,
        )
        .await
        .unwrap();
    let created = agents
        .create(CreateAgent {
            actor: owner.clone(),
            owner_id: owner.clone(),
            display_name: "MCP agent".to_owned(),
            provider: "test".to_owned(),
            service_identity: "mcp-agent".to_owned(),
            purpose: "MCP conformance".to_owned(),
            rate_limit_per_minute: 7,
            expires_at: None,
        })
        .await
        .unwrap();
    let grant_id = agents
        .grant(GrantAgent {
            actor: owner.clone(),
            agent_id: created.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel.id.clone()),
            scope: AgentScope::SendMessages,
            expires_at: None,
        })
        .await
        .unwrap();
    let state = AppState {
        auth: AuthService::development(),
        chat,
        operations: OperationalState::default(),
        processes: ProcessService::start(process_repository, None),
        agents: agents.clone(),
        notifications: NotificationService::test(),
        websocket_idle_timeout: Duration::from_secs(60),
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", created.credential)).unwrap(),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(MCP_PROTOCOL_VERSION),
    );
    let initialized = response_json(
            mcp_handler(
                State(state.clone()),
                headers.clone(),
                Json(McpRequest {
                    jsonrpc: "2.0".to_owned(),
                    id: serde_json::json!("initialize"),
                    method: "initialize".to_owned(),
                    params: serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}),
                }),
            )
            .await,
        )
        .await;
    assert_eq!(
        initialized["result"]["protocolVersion"],
        serde_json::json!("2025-06-18")
    );
    let notification = mcp_handler(
        State(state.clone()),
        headers.clone(),
        Json(McpRequest {
            jsonrpc: "2.0".to_owned(),
            id: serde_json::Value::Null,
            method: "notifications/initialized".to_owned(),
            params: serde_json::json!({}),
        }),
    )
    .await;
    assert_eq!(notification.status(), axum::http::StatusCode::ACCEPTED);
    let call = |id| McpRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(id),
        method: "tools/call".to_owned(),
        params: serde_json::json!({"name":"send_message","arguments":{"channel_id":channel.id.to_string(),"body":"from agent","request_id":"mcp-send-1","provenance":"delegated"}}),
    };
    let first =
        response_json(mcp_handler(State(state.clone()), headers.clone(), Json(call(1))).await)
            .await;
    let repeated =
        response_json(mcp_handler(State(state.clone()), headers.clone(), Json(call(2))).await)
            .await;
    assert!(first.get("result").is_some(), "{first}");
    assert_eq!(
        first["result"]["structuredContent"]["message"]["id"],
        repeated["result"]["structuredContent"]["message"]["id"]
    );
    assert_eq!(
        first["result"]["structuredContent"]["provenance"]["provenance"],
        "delegated"
    );
    let message_id = crate::domain::MessageId::from_uuid(
        uuid::Uuid::parse_str(
            first["result"]["structuredContent"]["message"]["id"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
    );
    agents
        .approve_message(owner.clone(), message_id)
        .await
        .unwrap();
    assert_eq!(
        agents
            .message_provenance(message_id)
            .await
            .unwrap()
            .provenance,
        crate::agent::ActivityProvenance::HumanApproved
    );
    let list_call = |id| McpRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(id),
        method: "tools/call".to_owned(),
        params: serde_json::json!({"name":"list_channels","arguments":{}}),
    };
    let listed = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(list_call("list-before-revoke")),
        )
        .await,
    )
    .await;
    assert_eq!(
        listed["result"]["structuredContent"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    agents.revoke(owner, grant_id).await.unwrap();
    let revoked =
        response_json(mcp_handler(State(state.clone()), headers.clone(), Json(call(3))).await)
            .await;
    assert!(revoked.get("error").is_some(), "{revoked}");
    let listed = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(list_call("list-after-revoke")),
        )
        .await,
    )
    .await;
    assert!(
        listed["result"]["structuredContent"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let rate_limited = mcp_handler(
        State(state),
        headers,
        Json(list_call("rate-limit-exceeded")),
    )
    .await;
    assert_eq!(
        rate_limited.status(),
        axum::http::StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn mcp_process_tools_enforce_separate_scopes_and_idempotency() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let chat = ChatEngine::start(repository.clone());
    let agents = AgentService::new(repository.clone());
    let owner = UserId::named("mcp-process-owner");
    chat.ensure_user(User {
        id: owner.clone(),
        kind: PrincipalKind::Human,
        display_name: DisplayName::new("MCP process owner").unwrap(),
        external_provider: Some("test".to_owned()),
        external_subject: Some("mcp-process-owner".to_owned()),
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let circle = chat
        .create_circle(
            owner.clone(),
            ChannelSlug::new("mcp-process-circle").unwrap(),
            DisplayName::new("MCP process circle").unwrap(),
        )
        .await
        .unwrap();
    let channel = chat
        .create_channel(
            owner.clone(),
            ChannelSlug::new("mcp-process-channel").unwrap(),
            DisplayName::new("MCP process channel").unwrap(),
            ChannelKind::Private,
            Some(circle.id.clone()),
        )
        .await
        .unwrap();
    repository
        .set_circle_feature(SetCircleFeature {
            circle_id: circle.id,
            actor: owner.clone(),
            feature: "heart.event-planning".to_owned(),
            enabled: true,
        })
        .await
        .unwrap();
    let created = agents
        .create(CreateAgent {
            actor: owner.clone(),
            owner_id: owner.clone(),
            display_name: "MCP process agent".to_owned(),
            provider: "test".to_owned(),
            service_identity: "mcp-process-agent".to_owned(),
            purpose: "MCP process conformance".to_owned(),
            rate_limit_per_minute: 60,
            expires_at: None,
        })
        .await
        .unwrap();
    let complete_grant = agents
        .grant(GrantAgent {
            actor: owner.clone(),
            agent_id: created.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel.id.clone()),
            scope: AgentScope::CompleteProcessWork,
            expires_at: None,
        })
        .await
        .unwrap();
    let state = AppState {
        auth: AuthService::development(),
        chat,
        operations: OperationalState::default(),
        processes: ProcessService::start(repository.clone(), None),
        agents: agents.clone(),
        notifications: NotificationService::test(),
        websocket_idle_timeout: Duration::from_secs(60),
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", created.credential)).unwrap(),
    );
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static(MCP_PROTOCOL_VERSION),
    );
    let tool_call = |id: &str, name: &str, arguments: serde_json::Value| McpRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(id),
        method: "tools/call".to_owned(),
        params: serde_json::json!({"name":name,"arguments":arguments}),
    };
    let start_args = serde_json::json!({
        "channel_id": channel.id,
        "request_id":"mcp-process-start",
        "namespace":"friends",
        "definition_name":"event-planning",
        "metadata":{"title":"Dinner"}
    });
    let denied = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call(
                "start-denied",
                "start_process",
                start_args.clone(),
            )),
        )
        .await,
    )
    .await;
    assert!(denied.get("error").is_some(), "{denied}");
    let start_grant = agents
        .grant(GrantAgent {
            actor: owner.clone(),
            agent_id: created.agent_id.clone(),
            circle_id: None,
            channel_id: Some(channel.id.clone()),
            scope: AgentScope::StartProcesses,
            expires_at: None,
        })
        .await
        .unwrap();
    let started = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call("start-1", "start_process", start_args.clone())),
        )
        .await,
    )
    .await;
    let replay = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call("start-2", "start_process", start_args)),
        )
        .await,
    )
    .await;
    let process_id = started["result"]["structuredContent"]["id"]
        .as_str()
        .unwrap();
    assert_eq!(
        started["result"]["structuredContent"]["id"],
        replay["result"]["structuredContent"]["id"]
    );
    let job = repository
        .lease_next(Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    repository
        .complete_start(
            job,
            crate::process::StartedProcess {
                instance_id: uuid::Uuid::now_v7(),
            },
        )
        .await
        .unwrap();
    let process_args = serde_json::json!({"process_link_id":process_id});
    let view = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call("get", "get_process", process_args.clone())),
        )
        .await,
    )
    .await;
    assert_eq!(
        view["result"]["structuredContent"]["process"]["status"],
        "active"
    );
    let response_args = serde_json::json!({
        "process_link_id":process_id,
        "request_id":"mcp-process-response",
        "payload":{"answer":"yes"}
    });
    let response = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call(
                "response-1",
                "complete_process_work",
                response_args.clone(),
            )),
        )
        .await,
    )
    .await;
    let response_replay = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call(
                "response-2",
                "complete_process_work",
                response_args,
            )),
        )
        .await,
    )
    .await;
    assert_eq!(
        response["result"]["structuredContent"]["outbox_id"],
        response_replay["result"]["structuredContent"]["outbox_id"]
    );
    let inspect_args = serde_json::json!({
        "process_link_id":process_id,
        "request_id":"mcp-process-inspect"
    });
    let inspection = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call(
                "inspect-1",
                "inspect_process",
                inspect_args.clone(),
            )),
        )
        .await,
    )
    .await;
    let inspection_replay = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call("inspect-2", "inspect_process", inspect_args)),
        )
        .await,
    )
    .await;
    assert_eq!(
        inspection["result"]["structuredContent"]["outbox_id"],
        inspection_replay["result"]["structuredContent"]["outbox_id"]
    );
    agents.revoke(owner.clone(), complete_grant).await.unwrap();
    let revoked_complete = response_json(
        mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(tool_call("get-revoked", "get_process", process_args)),
        )
        .await,
    )
    .await;
    assert!(
        revoked_complete.get("error").is_some(),
        "{revoked_complete}"
    );
    agents.revoke(owner, start_grant).await.unwrap();
    let revoked_start = response_json(
        mcp_handler(
            State(state),
            headers,
            Json(tool_call(
                "start-revoked",
                "start_process",
                serde_json::json!({
                    "channel_id":channel.id,
                    "request_id":"mcp-process-start-after-revoke",
                    "namespace":"friends",
                    "definition_name":"event-planning"
                }),
            )),
        )
        .await,
    )
    .await;
    assert!(revoked_start.get("error").is_some(), "{revoked_start}");
}

#[tokio::test]
async fn mcp_rejects_incompatible_transport_requests_before_dispatch() {
    let repository = Arc::new(
        SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let state = AppState {
        auth: AuthService::development(),
        chat: ChatEngine::start(repository.clone()),
        operations: OperationalState::default(),
        processes: ProcessService::start(repository.clone(), None),
        agents: AgentService::new(repository),
        notifications: NotificationService::test(),
        websocket_idle_timeout: Duration::from_secs(60),
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    };
    let request = || McpRequest {
        jsonrpc: "2.0".to_owned(),
        id: serde_json::json!(1),
        method: "tools/list".to_owned(),
        params: serde_json::json!({}),
    };

    let response = mcp_handler(State(state.clone()), HeaderMap::new(), Json(request())).await;
    assert_eq!(response.status(), axum::http::StatusCode::NOT_ACCEPTABLE);

    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        ORIGIN,
        HeaderValue::from_static("https://untrusted.invalid"),
    );
    let response = mcp_handler(State(state.clone()), headers, Json(request())).await;
    assert_eq!(response.status(), axum::http::StatusCode::FORBIDDEN);

    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(
        HeaderName::from_static("mcp-protocol-version"),
        HeaderValue::from_static("2099-01-01"),
    );
    let response = mcp_handler(State(state), headers, Json(request())).await;
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}
