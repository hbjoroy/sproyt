mod agent;
mod auth;
mod chat;
mod config;
mod db;
mod domain;
mod operations;
mod process;
mod protocol;
mod ws;

use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Path, Query, State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{ACCEPT, AUTHORIZATION, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
    },
    middleware,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use serde::Deserialize;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    agent::{AgentPrincipal, AgentScope, AgentService, CreateAgent, GrantAgent},
    auth::AuthService,
    chat::{ChatEngine, ChatError},
    config::{AppConfig, AuthMode, LogFormat},
    domain::{ChannelId, ChannelSequence, MessageBody, MessageLimit, UserId},
    operations::{OperationalState, healthz, metrics, record_metrics},
    process::{
        EnqueueCorrelation, EnqueueProcessStart, HeartGateway, ProcessLinkId, ProcessService,
        SetCircleFeature, SharedProcessGateway,
    },
};

#[derive(Clone)]
struct AppState {
    auth: AuthService,
    chat: ChatEngine,
    operations: OperationalState,
    processes: ProcessService,
    agents: AgentService,
    websocket_idle_timeout: Duration,
}

impl axum::extract::FromRef<AppState> for OperationalState {
    fn from_ref(state: &AppState) -> Self {
        state.operations.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        let database = AppConfig::database_from_env()?;
        init_tracing(AppConfig::log_format_from_env()?)?;
        db::migrate(&database).await?;
        info!(database = %database.kind(), "database migrations applied");
        return Ok(());
    }
    let config = AppConfig::from_env()?;
    init_tracing(config.log_format())?;
    let address = config.bind_address();
    let operations = OperationalState::default();
    let repositories = db::connect_repositories(config.database()).await?;
    let auth = match config.auth_mode() {
        AuthMode::Development => AuthService::development(),
        AuthMode::Oidc => {
            AuthService::oidc(
                config
                    .oidc()
                    .expect("OIDC config is present when OIDC mode is selected"),
            )
            .await?
        }
    };
    let state = AppState {
        auth,
        chat: ChatEngine::start(repositories.chat),
        operations: operations.clone(),
        processes: ProcessService::start(repositories.process, process_gateway_from_env()?),
        agents: AgentService::new(repositories.agent),
        websocket_idle_timeout: config.websocket_idle_timeout(),
    };
    let app = build_router(state, operations.clone());

    let listener = tokio::net::TcpListener::bind(address).await?;
    operations.set_ready(true);
    info!(
        %address,
        environment = %config.environment(),
        database = %config.database().kind(),
        "Sproyt is ready"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(operations))
        .await?;

    Ok(())
}

fn build_router(state: AppState, operations: OperationalState) -> Router {
    let request_id_header = HeaderName::from_static("x-request-id");
    Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/readyz", get(app_readyz))
        .route("/metrics", get(metrics))
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/logout", get(auth_logout))
        .route("/ws", get(ws_handler))
        .route("/api/v1/processes", post(start_process))
        .route("/api/v1/processes/{id}/messages", post(correlate_process))
        .route(
            "/api/v1/circles/{id}/features/heart-event-planning",
            post(set_heart_feature),
        )
        .route("/api/v1/agents", post(create_agent))
        .route("/api/v1/agents/{id}/grants", post(grant_agent))
        .route("/api/v1/agent-grants/{id}/revoke", post(revoke_agent_grant))
        .route(
            "/api/v1/messages/{id}/approve-agent",
            post(approve_agent_message),
        )
        .route("/mcp", post(mcp_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            operations.clone(),
            record_metrics,
        ))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
}

fn process_gateway_from_env() -> Result<Option<SharedProcessGateway>, crate::process::ProcessError>
{
    let Some(url) = std::env::var("SPROYT_HEART_URL").ok() else {
        return Ok(None);
    };
    let gateway = HeartGateway::new(url, Duration::from_secs(5), 2)?;
    Ok(Some(std::sync::Arc::new(gateway)))
}

async fn app_readyz(State(state): State<AppState>) -> axum::response::Response {
    if !state.operations.is_ready() {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "not ready\n").into_response();
    }
    match tokio::time::timeout(Duration::from_secs(2), state.chat.health_check()).await {
        Ok(Ok(())) => (axum::http::StatusCode::OK, "ready\n").into_response(),
        Ok(Err(error)) => {
            warn!(%error, "readiness database probe failed");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dependency unavailable\n",
            )
                .into_response()
        }
        Err(_) => {
            warn!("readiness database probe timed out");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dependency timeout\n",
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
struct StartProcessRequest {
    channel_id: String,
    request_id: String,
    namespace: String,
    definition_name: String,
    definition_version: Option<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

async fn start_process(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<StartProcessRequest>,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    let principal = match state.auth.authenticate_request(query.participant, cookie) {
        Ok(principal) => principal,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    };
    let channel_id = match ChannelId::new(body.channel_id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .enqueue_start(EnqueueProcessStart {
            channel_id,
            actor: principal.user.id,
            request_id: body.request_id,
            namespace: body.namespace,
            definition_name: body.definition_name,
            definition_version: body.definition_version,
            metadata: body.metadata,
        })
        .await
    {
        Ok(link) => (
            axum::http::StatusCode::ACCEPTED,
            Json(serde_json::json!({"process_link_id": link.id.as_uuid(), "status": link.status})),
        )
            .into_response(),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

#[derive(Deserialize)]
struct CorrelateProcessRequest {
    request_id: String,
    #[serde(default)]
    payload: serde_json::Value,
}

async fn correlate_process(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<CorrelateProcessRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(principal) => principal,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    };
    let process_link_id = match ProcessLinkId::parse(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid process link id",
            )
                .into_response();
        }
    };
    match state
        .processes
        .enqueue_correlation(EnqueueCorrelation {
            process_link_id,
            actor: principal.user.id,
            request_id: body.request_id,
            payload: body.payload,
        })
        .await
    {
        Ok(outbox_id) => (
            axum::http::StatusCode::ACCEPTED,
            Json(serde_json::json!({"outbox_id": outbox_id.as_uuid()})),
        )
            .into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
struct SetFeatureRequest {
    enabled: bool,
}

async fn set_heart_feature(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<SetFeatureRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(principal) => principal,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    };
    let circle_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => crate::domain::CircleId::from_uuid(id),
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid circle id").into_response();
        }
    };
    match state
        .processes
        .set_circle_feature(SetCircleFeature {
            circle_id,
            actor: principal.user.id,
            feature: "heart.event-planning".to_owned(),
            enabled: body.enabled,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

fn authenticate_http(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<crate::auth::AuthenticatedPrincipal, crate::auth::AuthError> {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    state.auth.authenticate_request(query.participant, cookie)
}

fn repository_response(error: crate::domain::RepositoryError) -> axum::response::Response {
    let status = match error {
        crate::domain::RepositoryError::PermissionDenied => axum::http::StatusCode::FORBIDDEN,
        crate::domain::RepositoryError::NotFound => axum::http::StatusCode::NOT_FOUND,
        crate::domain::RepositoryError::Conflict => axum::http::StatusCode::CONFLICT,
        crate::domain::RepositoryError::Storage(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.to_string()).into_response()
}

#[derive(Deserialize)]
struct CreateAgentRequest {
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

async fn create_agent(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<CreateAgentRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
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
        Ok(created) => (axum::http::StatusCode::CREATED, Json(created)).into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
struct GrantAgentRequest {
    circle_id: Option<String>,
    channel_id: Option<String>,
    scope: AgentScope,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn grant_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<GrantAgentRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
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

async fn revoke_agent_grant(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
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

#[derive(Deserialize)]
struct McpRequest {
    #[serde(default = "mcp_jsonrpc")]
    jsonrpc: String,
    #[serde(default)]
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

fn mcp_jsonrpc() -> String {
    "2.0".to_owned()
}
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

fn is_supported_mcp_version(version: &str) -> bool {
    matches!(version, "2025-03-26" | "2025-06-18" | MCP_PROTOCOL_VERSION)
}

async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpRequest>,
) -> axum::response::Response {
    if let Some(origin) = headers.get(ORIGIN).and_then(|value| value.to_str().ok()) {
        let allowed = std::env::var("SPROYT_MCP_ALLOWED_ORIGINS").unwrap_or_default();
        if !allowed
            .split(',')
            .map(str::trim)
            .any(|candidate| !candidate.is_empty() && candidate == origin)
        {
            return axum::http::StatusCode::FORBIDDEN.into_response();
        }
    }
    let accepts = headers
        .get(ACCEPT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !(accepts.contains("application/json") && accepts.contains("text/event-stream")) {
        return axum::http::StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if request.jsonrpc != "2.0" {
        return mcp_json_response(
            axum::http::StatusCode::BAD_REQUEST,
            serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":-32600,"message":"invalid JSON-RPC version"}}),
        );
    }
    if request.method != "initialize" {
        let version = headers
            .get("mcp-protocol-version")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("2025-03-26");
        if !is_supported_mcp_version(version) {
            return axum::http::StatusCode::BAD_REQUEST.into_response();
        }
    }
    let credential = match headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        Some(value) => value,
        None => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    let principal = match state.agents.authenticate(credential).await {
        Ok(value) => value,
        Err(_) => return axum::http::StatusCode::UNAUTHORIZED.into_response(),
    };
    let notification = request.id.is_null() || request.method.starts_with("notifications/");
    let response = match request.method.as_str() {
        "initialize" => {
            let protocol_version = request
                .params
                .get("protocolVersion")
                .and_then(|version| version.as_str())
                .filter(|version| is_supported_mcp_version(version))
                .unwrap_or(MCP_PROTOCOL_VERSION);
            Ok(
                serde_json::json!({"protocolVersion":protocol_version,"capabilities":{"tools":{}},"serverInfo":{"name":"sproyt","version":env!("CARGO_PKG_VERSION")}}),
            )
        }
        "notifications/initialized" => Ok(serde_json::Value::Null),
        "tools/list" => Ok(serde_json::json!({"tools": mcp_tools()})),
        "tools/call" => mcp_call(&state, &principal, request.params).await,
        _ => Err((-32601, "method not found".to_owned())),
    };
    if notification {
        return axum::http::StatusCode::ACCEPTED.into_response();
    }
    mcp_json_response(
        axum::http::StatusCode::OK,
        match response {
            Ok(result) => serde_json::json!({"jsonrpc":"2.0","id":request.id,"result":result}),
            Err((code, message)) => {
                serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code,"message":message}})
            }
        },
    )
}

fn mcp_json_response(
    status: axum::http::StatusCode,
    value: serde_json::Value,
) -> axum::response::Response {
    (status, Json(value)).into_response()
}

fn mcp_tools() -> serde_json::Value {
    serde_json::json!([
      {"name":"list_channels","description":"List channels granted to this agent","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
      {"name":"read_messages","description":"Read channel history","inputSchema":{"type":"object","required":["channel_id"],"properties":{"channel_id":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200},"after_sequence":{"type":"integer","minimum":0}},"additionalProperties":false}},
      {"name":"send_message","description":"Send an agent-authored message","inputSchema":{"type":"object","required":["channel_id","body","request_id"],"properties":{"channel_id":{"type":"string"},"body":{"type":"string"},"request_id":{"type":"string"},"provenance":{"type":"string","enum":["generated","delegated"],"default":"generated"}},"additionalProperties":false}},
      {"name":"mark_read","description":"Advance the agent read marker","inputSchema":{"type":"object","required":["channel_id","sequence"],"properties":{"channel_id":{"type":"string"},"sequence":{"type":"integer","minimum":0}},"additionalProperties":false}}
    ])
}

async fn mcp_call(
    state: &AppState,
    principal: &AgentPrincipal,
    params: serde_json::Value,
) -> Result<serde_json::Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or((-32602, "missing tool name".to_owned()))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let result = match name {
        "list_channels" => {
            let channels = state
                .chat
                .list_channels(principal.agent_id.clone())
                .await
                .map_err(mcp_chat_error)?;
            let mut granted = Vec::with_capacity(channels.len());
            for channel in channels {
                if state
                    .agents
                    .has_any_scope(
                        principal,
                        channel.circle_id.clone(),
                        Some(channel.id.clone()),
                    )
                    .await
                    .map_err(mcp_repository_error)?
                {
                    granted.push(channel);
                }
            }
            serde_json::to_value(granted).map_err(|e| (-32603, e.to_string()))?
        }
        "read_messages" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::ReadHistory,
                )
                .await
                .map_err(mcp_repository_error)?;
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .min(200) as u16;
            let after = args
                .get("after_sequence")
                .and_then(|v| v.as_u64())
                .map(ChannelSequence::new);
            serde_json::to_value(
                state
                    .chat
                    .load_messages(
                        principal.agent_id.clone(),
                        channel,
                        MessageLimit::new(limit),
                        after,
                    )
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "send_message" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::SendMessages,
                )
                .await
                .map_err(mcp_repository_error)?;
            let body = MessageBody::new(
                args.get("body")
                    .and_then(|v| v.as_str())
                    .ok_or((-32602, "missing body".to_owned()))?,
            )
            .map_err(|e| (-32602, e.to_string()))?;
            let request_id = args
                .get("request_id")
                .and_then(|v| v.as_str())
                .ok_or((-32602, "missing request_id".to_owned()))?
                .to_owned();
            let delegated =
                args.get("provenance").and_then(|value| value.as_str()) == Some("delegated");
            let agent_id = principal.agent_id.clone();
            let message = state
                .chat
                .send_message_idempotent(channel, agent_id.clone(), body, request_id)
                .await
                .map_err(mcp_chat_error)?;
            if delegated {
                state
                    .agents
                    .mark_delegated(agent_id, message.id)
                    .await
                    .map_err(mcp_repository_error)?;
            }
            let provenance = state
                .agents
                .message_provenance(message.id)
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"message":message,"provenance":provenance})
        }
        "mark_read" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::ReadHistory,
                )
                .await
                .map_err(mcp_repository_error)?;
            let sequence = args
                .get("sequence")
                .and_then(|v| v.as_u64())
                .ok_or((-32602, "missing sequence".to_owned()))?;
            serde_json::to_value(
                state
                    .chat
                    .mark_read(
                        principal.agent_id.clone(),
                        channel,
                        ChannelSequence::new(sequence),
                    )
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        _ => return Err((-32602, "unknown tool".to_owned())),
    };
    Ok(
        serde_json::json!({"content":[{"type":"text","text":serde_json::to_string(&result).map_err(|e|(-32603,e.to_string()))?}],"structuredContent":result}),
    )
}

fn mcp_channel(args: &serde_json::Value) -> Result<ChannelId, (i64, String)> {
    ChannelId::new(
        args.get("channel_id")
            .and_then(|v| v.as_str())
            .ok_or((-32602, "missing channel_id".to_owned()))?,
    )
    .map_err(|e| (-32602, e.to_string()))
}
fn mcp_repository_error(error: crate::domain::RepositoryError) -> (i64, String) {
    (-32003, error.to_string())
}
fn mcp_chat_error(error: ChatError) -> (i64, String) {
    (-32004, error.to_string())
}

async fn approve_agent_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
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

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn init_tracing(log_format: LogFormat) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sproyt=info"));
    match log_format {
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init()?,
        LogFormat::Pretty => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()?,
    }
    Ok(())
}

async fn shutdown_signal(operations: OperationalState) {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            warn!(%error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => warn!(%error, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    operations.begin_shutdown();
    info!(grace_period_seconds = 30, "shutdown requested");
}

async fn ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    let principal = match state.auth.authenticate_request(query.participant, cookie) {
        Ok(principal) => principal,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    };
    let shutdown = state.operations.subscribe_shutdown();
    upgrade
        .on_upgrade(move |socket| {
            ws::handle_socket(
                state.chat,
                principal,
                socket,
                shutdown,
                state.websocket_idle_timeout,
            )
        })
        .into_response()
}

async fn auth_login(State(state): State<AppState>) -> axum::response::Response {
    match state.auth.login() {
        Ok(login) => redirect_with_cookies(&login.authorization_url, &[login.set_cookie]),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn auth_callback(
    State(state): State<AppState>,
    Query(query): Query<OidcCallbackQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    match state.auth.callback(query.code, query.state, cookie).await {
        Ok(login) => {
            if let Err(error) = state.chat.ensure_user(login.principal.user).await {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    error.to_string(),
                )
                    .into_response();
            }
            redirect_with_cookies("/", &[login.set_cookie, login.clear_transaction_cookie])
        }
        Err(error) => (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response(),
    }
}

async fn auth_logout(State(state): State<AppState>) -> axum::response::Response {
    let logout = state.auth.logout();
    redirect_with_cookies(&logout.redirect_url, &[logout.clear_cookie])
}

fn redirect_with_cookies(location: &str, cookies: &[String]) -> axum::response::Response {
    let mut response = axum::http::StatusCode::SEE_OTHER.into_response();
    let headers = response.headers_mut();
    match HeaderValue::from_str(location) {
        Ok(location) => {
            headers.insert(LOCATION, location);
        }
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
    for cookie in cookies {
        if let Ok(cookie) = HeaderValue::from_str(cookie) {
            headers.append(SET_COOKIE, cookie);
        }
    }
    response
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    participant: Option<String>,
}

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="nn">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Sproyt - Hello Chat</title>
    <style>
      :root {
        color-scheme: light dark;
        font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        background: #f7f7f4;
        color: #18201d;
      }

      * {
        box-sizing: border-box;
      }

      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
      }

      main {
        width: min(860px, 100%);
        display: grid;
        grid-template-rows: auto 1fr auto;
        min-height: min(760px, calc(100vh - 48px));
        border: 1px solid #d7d8d0;
        border-radius: 8px;
        background: #ffffff;
        box-shadow: 0 18px 50px rgb(24 32 29 / 12%);
        overflow: hidden;
      }

      header,
      form,
      .messages {
        padding: 18px;
      }

      header {
        display: grid;
        gap: 12px;
        border-bottom: 1px solid #e4e5de;
      }

      h1 {
        margin: 0;
        font-size: 1.6rem;
        line-height: 1.1;
      }

      label {
        display: grid;
        gap: 4px;
        color: #506057;
        font-size: 0.9rem;
      }

      input,
      textarea,
      button {
        min-height: 40px;
        border: 1px solid #cbd1c8;
        border-radius: 6px;
        font: inherit;
      }

      input,
      textarea {
        width: 100%;
        padding: 8px 10px;
        background: #ffffff;
        color: #18201d;
      }

      textarea {
        min-height: 84px;
        resize: vertical;
      }

      button {
        padding: 8px 14px;
        background: #245b45;
        color: #ffffff;
        cursor: pointer;
      }

      button:disabled {
        cursor: default;
        opacity: 0.55;
      }

      .connect {
        display: grid;
        grid-template-columns: 1fr 1fr auto;
        gap: 12px;
        align-items: end;
      }

      .circle-tools {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr)) auto;
        gap: 8px;
        align-items: end;
      }

      .status {
        color: #506057;
        min-height: 1.2em;
      }

      .view-controls {
        display: flex;
        gap: 8px;
      }

      .view-controls button {
        min-height: 34px;
        background: #eef2ed;
        color: #253128;
      }

      .view-controls button[aria-pressed="true"] {
        background: #245b45;
        color: #ffffff;
      }

      .messages {
        align-content: start;
        display: grid;
        gap: 10px;
        overflow-y: auto;
        background: #fbfbf8;
      }

      .message {
        display: grid;
        gap: 4px;
        padding: 12px;
        border: 1px solid #dfe3dc;
        border-radius: 8px;
        background: #ffffff;
      }

      .meta {
        color: #506057;
        font-size: 0.85rem;
      }

      .rendered {
        display: grid;
        gap: 10px;
        line-height: 1.45;
      }

      .rendered h1,
      .rendered h2,
      .rendered h3 {
        margin: 0;
        line-height: 1.2;
      }

      .rendered h1 {
        font-size: 1.35rem;
      }

      .rendered h2 {
        font-size: 1.2rem;
      }

      .rendered h3 {
        font-size: 1.05rem;
      }

      .rendered p,
      .rendered ul,
      .rendered ol,
      .rendered blockquote {
        margin: 0;
      }

      .rendered ul,
      .rendered ol {
        padding-left: 22px;
      }

      .rendered blockquote {
        padding-left: 12px;
        border-left: 3px solid #b9c6bd;
        color: #506057;
      }

      .rendered pre,
      .raw-body {
        overflow-x: auto;
        margin: 0;
        padding: 12px;
        border: 1px solid #d6ddd5;
        border-radius: 6px;
        background: #f4f6f3;
        color: #18201d;
      }

      .rendered code,
      .raw-body {
        font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
      }

      .rendered p code,
      .rendered li code {
        padding: 1px 5px;
        border-radius: 4px;
        background: #eef2ed;
      }

      .mermaid-shell {
        overflow-x: auto;
        padding: 12px;
        border: 1px solid #d6ddd5;
        border-radius: 6px;
        background: #ffffff;
      }

      .system {
        color: #506057;
        font-size: 0.9rem;
      }

      form.send {
        display: grid;
        grid-template-columns: 1fr auto;
        gap: 12px;
        border-top: 1px solid #e4e5de;
      }

      @media (max-width: 640px) {
        body {
          padding: 12px;
        }

        main {
          min-height: calc(100vh - 24px);
        }

        .connect,
        .circle-tools,
        form.send {
          grid-template-columns: 1fr;
        }
      }

      @media (prefers-color-scheme: dark) {
        :root {
          background: #111613;
          color: #eef3ee;
        }

        main,
        input,
        textarea,
        .message {
          background: #19211c;
          border-color: #344038;
          color: #eef3ee;
        }

        header,
        form.send {
          border-color: #344038;
        }

        .messages {
          background: #121814;
        }

        label,
        .meta,
        .status,
        .system,
        .rendered blockquote {
          color: #b6c1b9;
        }

        .view-controls button,
        .rendered pre,
        .raw-body,
        .rendered p code,
        .rendered li code {
          background: #111713;
          border-color: #344038;
          color: #eef3ee;
        }

        .mermaid-shell {
          background: #eef3ee;
          border-color: #344038;
        }
      }
    </style>
  </head>
  <body>
    <main>
      <header>
        <h1>Hello Chat</h1>
        <form class="connect" id="connect-form">
          <label>
            Kanal
            <input id="channel" name="channel" value="general" autocomplete="off">
          </label>
          <label>
            Namn
            <input id="participant" name="participant" value="alice" autocomplete="off">
          </label>
          <button id="connect" type="submit">Kople til</button>
        </form>
        <div class="status" id="status">Ikkje tilkopla</div>
        <div class="circle-tools" aria-label="Vennekretsar">
          <label>Vennekrets<select id="circle-select"><option value="">Ingen</option></select></label>
          <label>Namn<input id="circle-name" placeholder="Turvenner"></label>
          <label>Slug<input id="circle-slug" placeholder="turvenner"></label>
          <button id="create-circle" type="button" disabled>Lag krets</button>
          <label>Ny kanal<input id="circle-channel" placeholder="planlegging"></label>
          <button id="create-circle-channel" type="button" disabled>Lag kanal</button>
          <button id="create-invitation" type="button" disabled>Lag invitasjon</button>
          <label>Invitasjonstoken<input id="invitation-token" placeholder="Lim inn token"></label>
          <button id="accept-invitation" type="button" disabled>Godta</button>
        </div>
        <div class="view-controls" aria-label="Meldingsvising">
          <button id="view-mode" type="button" aria-pressed="true">View</button>
          <button id="raw-mode" type="button" aria-pressed="false">Raw</button>
        </div>
      </header>
      <section class="messages" id="messages" aria-live="polite"></section>
      <form class="send" id="send-form">
        <textarea id="body" name="body" placeholder="Skriv Markdown, kode eller Mermaid" autocomplete="off" disabled></textarea>
        <button id="send" type="submit" disabled>Send</button>
      </form>
    </main>

    <script type="module">
      import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";

      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
      });

      const connectForm = document.querySelector("#connect-form");
      const sendForm = document.querySelector("#send-form");
      const channelInput = document.querySelector("#channel");
      const participantInput = document.querySelector("#participant");
      const bodyInput = document.querySelector("#body");
      const sendButton = document.querySelector("#send");
      const viewModeButton = document.querySelector("#view-mode");
      const rawModeButton = document.querySelector("#raw-mode");
      const statusEl = document.querySelector("#status");
      const messagesEl = document.querySelector("#messages");
      const circleSelect = document.querySelector("#circle-select");
      const circleName = document.querySelector("#circle-name");
      const circleSlug = document.querySelector("#circle-slug");
      const circleChannel = document.querySelector("#circle-channel");
      const invitationToken = document.querySelector("#invitation-token");
      const circleButtons = ["#create-circle", "#create-circle-channel", "#create-invitation", "#accept-invitation"].map((id) => document.querySelector(id));

      let socket = null;
      let heartbeatTimer = null;
      let renderMode = "view";
      let requestNumber = 0;
      let activeChannelId = null;
      let requestedChannelSlug = "general";
      const timeline = [];
      const seenMessageIds = new Set();
      const catchUpTargets = new Map();

      connectForm.addEventListener("submit", (event) => {
        event.preventDefault();
        connect();
      });

      sendForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const body = bodyInput.value.trim();
        if (!socket || socket.readyState !== WebSocket.OPEN || !activeChannelId || body.length === 0) {
          return;
        }
        sendCommand("send_message", { channel_id: activeChannelId, body });
        bodyInput.value = "";
        bodyInput.focus();
      });

      viewModeButton.addEventListener("click", () => setRenderMode("view"));
      rawModeButton.addEventListener("click", () => setRenderMode("raw"));
      circleButtons[0].addEventListener("click", () => sendCommand("create_circle", {
        name: circleName.value.trim(), slug: slugify(circleSlug.value || circleName.value)
      }));
      circleButtons[1].addEventListener("click", () => {
        if (!circleSelect.value) return;
        const slug = slugify(circleChannel.value);
        sendCommand("create_channel", { slug, name: circleChannel.value.trim(), kind: "private", circle_id: circleSelect.value });
      });
      circleButtons[2].addEventListener("click", () => {
        if (circleSelect.value) sendCommand("create_circle_invitation", { circle_id: circleSelect.value });
      });
      circleButtons[3].addEventListener("click", () => {
        if (invitationToken.value.trim()) sendCommand("accept_circle_invitation", { token: invitationToken.value.trim() });
      });

      function slugify(value) {
        return value.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
      }

      function connect() {
        if (heartbeatTimer !== null) {
          window.clearInterval(heartbeatTimer);
          heartbeatTimer = null;
        }
        if (socket) {
          socket.close();
        }

        timeline.length = 0;
        seenMessageIds.clear();
        catchUpTargets.clear();
        activeChannelId = null;
        messagesEl.replaceChildren();
        requestedChannelSlug = (channelInput.value.trim() || "general")
          .toLowerCase()
          .replace(/[^a-z0-9_-]+/g, "-");
        const participant = encodeURIComponent(participantInput.value.trim() || "guest");
        const protocol = window.location.protocol === "https:" ? "wss" : "ws";
        socket = new WebSocket(`${protocol}://${window.location.host}/ws?participant=${participant}`);
        setConnected(false, "Koplar til ...");

        socket.addEventListener("open", () => {
          setConnected(true, `Tilkopla ${requestedChannelSlug} som ${decodeURIComponent(participant)}`);
          sendCommand("hello");
          sendCommand("list_my_channels");
          sendCommand("list_my_circles");
          heartbeatTimer = window.setInterval(() => sendCommand("ping"), 20_000);
        });

        socket.addEventListener("message", (event) => {
          renderServerEvent(JSON.parse(event.data));
        });

        socket.addEventListener("close", () => {
          if (heartbeatTimer !== null) {
            window.clearInterval(heartbeatTimer);
            heartbeatTimer = null;
          }
          setConnected(false, "Fråkopla");
        });

        socket.addEventListener("error", () => {
          setConnected(false, "WebSocket-feil");
        });
      }

      function sendCommand(type, payload) {
        requestNumber += 1;
        const command = {
          protocol: "sproyt.chat.v1",
          request_id: `browser-${requestNumber}`,
          type
        };
        if (payload !== undefined) {
          command.payload = payload;
        }
        socket.send(JSON.stringify(command));
      }

      function setConnected(connected, status) {
        statusEl.textContent = status;
        bodyInput.disabled = !connected;
        sendButton.disabled = !connected;
        circleButtons.forEach((button) => { button.disabled = !connected; });
      }

      function setRenderMode(mode) {
        renderMode = mode;
        viewModeButton.setAttribute("aria-pressed", String(mode === "view"));
        rawModeButton.setAttribute("aria-pressed", String(mode === "raw"));
        renderTimeline();
      }

      function renderServerEvent(event) {
        if (event.protocol !== "sproyt.chat.v1") {
          pushSystem("Serveren svarte med ein ukjend protokoll.");
          return;
        }
        const payload = event.payload || {};

        if (event.type === "circles_listed") {
          circleSelect.replaceChildren(new Option("Ingen", ""));
          payload.circles.forEach(([circle, role]) => circleSelect.add(new Option(`${circle.name} (${role})`, circle.id)));
          return;
        }
        if (event.type === "circle_created") {
          circleSelect.add(new Option(`${payload.circle.name} (owner)`, payload.circle.id));
          circleSelect.value = payload.circle.id;
          pushSystem(`Vennekretsen ${payload.circle.name} er oppretta.`);
          return;
        }
        if (event.type === "circle_invitation_created") {
          invitationToken.value = payload.invitation.token;
          pushSystem("Invitasjonstoken er laga og lagt i tokenfeltet.");
          return;
        }
        if (event.type === "circle_invitation_accepted") {
          pushSystem("Invitasjonen er godteken.");
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "channels_listed") {
          const existing = payload.channels.find((channel) => channel.slug === requestedChannelSlug);
          if (existing) {
            activeChannelId = existing.id;
            const unread = Math.max(0, existing.latest_sequence - existing.last_read_sequence);
            if (unread > 0) pushSystem(`${unread} uleste meldingar i ${existing.name}.`);
            sendCommand("subscribe_channel", { channel_id: activeChannelId });
          } else {
            sendCommand("create_channel", {
              slug: requestedChannelSlug,
              name: requestedChannelSlug,
              kind: "private"
            });
          }
          return;
        }

        if (event.type === "channel_created") {
          activeChannelId = payload.channel.id;
          sendCommand("subscribe_channel", { channel_id: activeChannelId });
          return;
        }

        if (event.type === "subscription_started") {
          activeChannelId = payload.channel_id;
          payload.history.forEach(appendTimelineMessage);
          renderTimeline();
          return;
        }

        if (event.type === "chat") {
          const chatEvent = payload.event;
          if (chatEvent.type === "message_accepted") {
            appendTimelineMessage(chatEvent.message);
            renderTimeline();
          } else if (chatEvent.type === "participant_joined") {
            pushSystem(`${chatEvent.participant_id} kom inn i ${chatEvent.channel_id}`);
          } else if (chatEvent.type === "participant_left") {
            pushSystem(`${chatEvent.participant_id} gjekk ut av ${chatEvent.channel_id}`);
          }
          return;
        }

        if (event.type === "lagged") {
          pushSystem(`Klienten låg etter og hoppa over ${payload.skipped} event; lastar inn att.`);
          catchUpTargets.set(payload.channel_id, payload.latest_known_sequence);
          sendCommand("load_recent_messages", {
            channel_id: payload.channel_id,
            after: payload.last_seen_sequence,
            limit: 200
          });
          return;
        }

        if (event.type === "messages_loaded") {
          payload.messages.forEach(appendTimelineMessage);
          renderTimeline();
          const target = catchUpTargets.get(payload.channel_id);
          const last = payload.messages.at(-1);
          if (target !== undefined && last && last.sequence < target) {
            sendCommand("load_recent_messages", {
              channel_id: payload.channel_id,
              after: last.sequence,
              limit: 200
            });
          } else if (target !== undefined) {
            catchUpTargets.delete(payload.channel_id);
          }
          return;
        }

        if (event.type === "error") {
          pushSystem(payload.message || payload.code);
        }
      }

      function pushSystem(text) {
        timeline.push({ type: "system", text });
        renderTimeline();
      }

      function renderTimeline() {
        messagesEl.replaceChildren();
        for (const item of timeline) {
          if (item.type === "message") {
            appendMessage(item.message);
          } else {
            appendSystem(item.text);
          }
        }
        renderMermaidDiagrams();
        messagesEl.scrollTop = messagesEl.scrollHeight;
      }

      function renderMessage(message) {
        appendTimelineMessage(message);
        renderTimeline();
      }

      function appendTimelineMessage(message) {
        if (seenMessageIds.has(message.id)) return;
        seenMessageIds.add(message.id);
        timeline.push({ type: "message", message });
      }

      function appendMessage(message) {
        const wrapper = document.createElement("article");
        wrapper.className = "message";

        const meta = document.createElement("div");
        meta.className = "meta";
        meta.textContent = `${message.sender_id} #${message.sequence}`;

        const body = document.createElement("div");
        if (renderMode === "raw") {
          const pre = document.createElement("pre");
          pre.className = "raw-body";
          pre.textContent = message.body;
          body.append(pre);
        } else {
          body.className = "rendered";
          renderMarkdown(message.body, body);
        }

        wrapper.append(meta, body);
        messagesEl.append(wrapper);
      }

      function renderSystem(text) {
        pushSystem(text);
      }

      function appendSystem(text) {
        const line = document.createElement("div");
        line.className = "system";
        line.textContent = text;
        messagesEl.append(line);
      }

      function renderMarkdown(source, target) {
        const lines = source.replace(/\r\n/g, "\n").split("\n");
        let paragraph = [];
        let list = null;
        let inFence = false;
        let fenceLanguage = "";
        let fenceLines = [];

        const flushParagraph = () => {
          if (paragraph.length === 0) {
            return;
          }
          const p = document.createElement("p");
          appendInline(p, paragraph.join(" "));
          target.append(p);
          paragraph = [];
        };

        const flushList = () => {
          if (!list) {
            return;
          }
          target.append(list.element);
          list = null;
        };

        const flushFence = () => {
          const code = fenceLines.join("\n");
          if (fenceLanguage.toLowerCase() === "mermaid") {
            const shell = document.createElement("div");
            shell.className = "mermaid-shell";
            const diagram = document.createElement("div");
            diagram.className = "mermaid";
            diagram.textContent = code;
            shell.append(diagram);
            target.append(shell);
          } else {
            const pre = document.createElement("pre");
            const codeEl = document.createElement("code");
            if (fenceLanguage) {
              codeEl.dataset.language = fenceLanguage;
            }
            codeEl.textContent = code;
            pre.append(codeEl);
            target.append(pre);
          }
          inFence = false;
          fenceLanguage = "";
          fenceLines = [];
        };

        for (const line of lines) {
          const fence = line.match(/^```([A-Za-z0-9_-]+)?\s*$/);
          if (fence) {
            if (inFence) {
              flushFence();
            } else {
              flushParagraph();
              flushList();
              inFence = true;
              fenceLanguage = fence[1] || "";
              fenceLines = [];
            }
            continue;
          }

          if (inFence) {
            fenceLines.push(line);
            continue;
          }

          if (/^\s*$/.test(line)) {
            flushParagraph();
            flushList();
            continue;
          }

          const heading = line.match(/^(#{1,3})\s+(.+)$/);
          if (heading) {
            flushParagraph();
            flushList();
            const level = String(heading[1].length);
            const h = document.createElement(`h${level}`);
            appendInline(h, heading[2]);
            target.append(h);
            continue;
          }

          const quote = line.match(/^>\s?(.+)$/);
          if (quote) {
            flushParagraph();
            flushList();
            const blockquote = document.createElement("blockquote");
            appendInline(blockquote, quote[1]);
            target.append(blockquote);
            continue;
          }

          const unordered = line.match(/^\s*[-*]\s+(.+)$/);
          const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
          if (unordered || ordered) {
            flushParagraph();
            const kind = ordered ? "ol" : "ul";
            if (!list || list.kind !== kind) {
              flushList();
              list = { kind, element: document.createElement(kind) };
            }
            const li = document.createElement("li");
            appendInline(li, (unordered || ordered)[1]);
            list.element.append(li);
            continue;
          }

          flushList();
          paragraph.push(line.trim());
        }

        if (inFence) {
          flushFence();
        }
        flushParagraph();
        flushList();
      }

      function appendInline(parent, text) {
        const parts = text.split(/(`[^`]+`)/g);
        for (const part of parts) {
          if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
            const code = document.createElement("code");
            code.textContent = part.slice(1, -1);
            parent.append(code);
          } else if (part.length > 0) {
            parent.append(document.createTextNode(part));
          }
        }
      }

      async function renderMermaidDiagrams() {
        if (renderMode !== "view") {
          return;
        }
        const diagrams = [...messagesEl.querySelectorAll(".mermaid")];
        for (const diagram of diagrams) {
          if (diagram.dataset.rendered) {
            continue;
          }
          diagram.dataset.rendered = "true";
          try {
            await mermaid.run({ nodes: [diagram] });
          } catch (error) {
            diagram.textContent = `Mermaid-feil: ${error.message || error}`;
          }
        }
      }
    </script>
  </body>
</html>
"##;

#[cfg(test)]
mod mcp_tests {
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
                rate_limit_per_minute: 60,
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
            websocket_idle_timeout: Duration::from_secs(60),
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
            mcp_handler(State(state), headers, Json(list_call("list-after-revoke"))).await,
        )
        .await;
        assert!(
            listed["result"]["structuredContent"]
                .as_array()
                .unwrap()
                .is_empty()
        );
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
            websocket_idle_timeout: Duration::from_secs(60),
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
}

#[cfg(test)]
mod protocol_capacity_tests {
    use super::*;
    use crate::{
        agent::{AgentRepository, AgentService},
        db::SqliteChatRepository,
        process::{ProcessRepository, ProcessService},
    };
    use futures_util::{SinkExt, StreamExt};
    use std::{sync::Arc, time::Instant};
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as ClientMessage,
    };

    type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    async fn start_test_server(
        repository: Arc<SqliteChatRepository>,
        websocket_idle_timeout: Duration,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
        let process_repository: Arc<dyn ProcessRepository> = repository.clone();
        let agent_repository: Arc<dyn AgentRepository> = repository;
        let operations = OperationalState::default();
        operations.set_ready(true);
        let app = build_router(
            AppState {
                auth: AuthService::development(),
                chat: ChatEngine::start(chat_repository),
                operations: operations.clone(),
                processes: ProcessService::start(process_repository, None),
                agents: AgentService::new(agent_repository),
                websocket_idle_timeout,
            },
            operations,
        );
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server)
    }

    async fn connect(address: std::net::SocketAddr) -> TestSocket {
        connect_as(address, "capacity-user").await
    }

    async fn connect_as(address: std::net::SocketAddr, participant: &str) -> TestSocket {
        let url = format!("ws://{address}/ws?participant={participant}");
        connect_async(url).await.unwrap().0
    }

    async fn command(
        socket: &mut TestSocket,
        request_id: &str,
        command_type: &str,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        let response = command_response(socket, request_id, command_type, payload).await;
        assert_ne!(response["type"], "error", "{response}");
        response
    }

    async fn command_response(
        socket: &mut TestSocket,
        request_id: &str,
        command_type: &str,
        payload: serde_json::Value,
    ) -> serde_json::Value {
        let mut envelope = serde_json::json!({
            "protocol": crate::protocol::PROTOCOL_ID,
            "request_id": request_id,
            "type": command_type,
        });
        if !payload.is_null() {
            envelope["payload"] = payload;
        }
        socket
            .send(ClientMessage::Text(envelope.to_string().into()))
            .await
            .unwrap();
        loop {
            let frame = tokio::time::timeout(Duration::from_secs(5), socket.next())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "protocol response exceeded five seconds for {command_type}/{request_id}"
                    )
                })
                .expect("server closed before protocol response")
                .unwrap();
            if let ClientMessage::Text(text) = frame {
                let response: serde_json::Value = serde_json::from_str(&text).unwrap();
                if response.get("request_id").and_then(|id| id.as_str()) == Some(request_id) {
                    return response;
                }
            }
        }
    }

    #[tokio::test]
    async fn websocket_capacity_reconnect_and_service_restart_gate() {
        const MESSAGE_COUNT: usize = 40;
        const CURSOR: u64 = 20;
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, first_server) =
            start_test_server(repository.clone(), Duration::from_secs(60)).await;
        let mut socket = connect(address).await;
        let created = command(
            &mut socket,
            "create",
            "create_channel",
            serde_json::json!({"slug":"capacity-gate","name":"Capacity gate","kind":"private","circle_id":null}),
        )
        .await;
        let channel_id = created["payload"]["channel"]["id"]
            .as_str()
            .unwrap()
            .to_owned();

        let mut latencies = Vec::with_capacity(MESSAGE_COUNT);
        let mut first_message_id = None;
        for index in 1..=MESSAGE_COUNT {
            let started = Instant::now();
            let accepted = command(
                &mut socket,
                &format!("send-{index}"),
                "send_message",
                serde_json::json!({"channel_id":channel_id,"body":format!("capacity-{index}")}),
            )
            .await;
            latencies.push(started.elapsed());
            assert_eq!(
                accepted["payload"]["message"]["sequence"].as_u64(),
                Some(index as u64)
            );
            if index == 1 {
                first_message_id = accepted["payload"]["message"]["id"]
                    .as_str()
                    .map(str::to_owned);
            }
        }
        let replay = command(
            &mut socket,
            "send-1",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"must not replace the accepted body"}),
        )
        .await;
        assert_eq!(
            replay["payload"]["message"]["id"].as_str(),
            first_message_id.as_deref()
        );
        assert_eq!(replay["payload"]["message"]["sequence"].as_u64(), Some(1));
        assert_eq!(
            replay["payload"]["message"]["body"].as_str(),
            Some("capacity-1")
        );
        latencies.sort_unstable();
        let p99_index = ((MESSAGE_COUNT * 99).div_ceil(100)).saturating_sub(1);
        assert!(
            latencies[p99_index] < Duration::from_millis(750),
            "p99 send latency was {:?}",
            latencies[p99_index]
        );

        socket.close(None).await.unwrap();
        first_server.abort();
        let _ = first_server.await;

        let (restart_address, restarted_server) =
            start_test_server(repository, Duration::from_secs(60)).await;
        let reconnect_started = Instant::now();
        let mut reconnected = connect(restart_address).await;
        let loaded = command(
            &mut reconnected,
            "catch-up",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":MESSAGE_COUNT,"after":CURSOR}),
        )
        .await;
        assert!(reconnect_started.elapsed() < Duration::from_secs(5));
        let messages = loaded["payload"]["messages"].as_array().unwrap();
        assert_eq!(messages.len(), MESSAGE_COUNT - CURSOR as usize);
        assert_eq!(
            messages.first().unwrap()["sequence"].as_u64(),
            Some(CURSOR + 1)
        );
        assert_eq!(
            messages.last().unwrap()["sequence"].as_u64(),
            Some(MESSAGE_COUNT as u64)
        );

        reconnected.close(None).await.unwrap();
        restarted_server.abort();
    }

    #[tokio::test]
    async fn idle_websocket_is_closed_with_a_stable_reason() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_millis(100)).await;
        let mut socket = connect(address).await;

        let frame = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("idle close frame exceeded test deadline")
            .expect("server closed without a WebSocket close frame")
            .unwrap();
        match frame {
            ClientMessage::Close(Some(frame)) => {
                assert_eq!(frame.reason, "idle timeout");
            }
            other => panic!("expected idle close frame, received {other:?}"),
        }
        server.abort();
    }

    #[tokio::test]
    async fn websocket_reports_authorization_and_unknown_command_errors() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
        let mut owner = connect_as(address, "wire-owner").await;
        let created = command(
            &mut owner,
            "create-private",
            "create_channel",
            serde_json::json!({"slug":"wire-private","name":"Wire private","kind":"private","circle_id":null}),
        )
        .await;
        let channel_id = created["payload"]["channel"]["id"].clone();

        let mut outsider = connect_as(address, "wire-outsider").await;
        let denied = command_response(
            &mut outsider,
            "unauthorized-load",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
        )
        .await;
        assert_eq!(denied["type"], "error");
        assert_eq!(denied["payload"]["code"], "permission_denied");

        outsider
            .send(ClientMessage::Text(
                serde_json::json!({"protocol":"sproyt.chat.v1","request_id":"unknown","type":"future_command"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let unknown = tokio::time::timeout(Duration::from_secs(2), outsider.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let ClientMessage::Text(unknown) = unknown else {
            panic!("expected structured unknown-command error")
        };
        let unknown: serde_json::Value = serde_json::from_str(&unknown).unwrap();
        assert_eq!(unknown["type"], "error");
        assert_eq!(unknown["payload"]["code"], "invalid_envelope");

        outsider
            .send(ClientMessage::Text(
                serde_json::json!({"protocol":"sproyt.chat.v1","request_id":"future-field","type":"ping","future_extension":{"version":2}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        loop {
            let frame = outsider.next().await.unwrap().unwrap();
            if let ClientMessage::Text(text) = frame {
                let response: serde_json::Value = serde_json::from_str(&text).unwrap();
                if response["request_id"] == "future-field" {
                    assert_eq!(response["type"], "pong");
                    break;
                }
            }
        }
        server.abort();
    }

    #[tokio::test]
    async fn two_users_complete_private_circle_slice_with_unread_reconnect() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
        let mut owner = connect_as(address, "circle-owner").await;
        let circle = command(
            &mut owner,
            "circle-create",
            "create_circle",
            serde_json::json!({"slug":"friends","name":"Friends"}),
        )
        .await;
        let circle_id = circle["payload"]["circle"]["id"].clone();
        let invitation = command(
            &mut owner,
            "circle-invite",
            "create_circle_invitation",
            serde_json::json!({"circle_id":circle_id}),
        )
        .await;
        let token = invitation["payload"]["invitation"]["token"].clone();
        let channel = command(
            &mut owner,
            "circle-channel",
            "create_channel",
            serde_json::json!({"slug":"friends-chat","name":"Friends chat","kind":"private","circle_id":circle_id}),
        )
        .await;
        let channel_id = channel["payload"]["channel"]["id"].clone();

        let mut member = connect_as(address, "circle-member").await;
        let denied = command_response(
            &mut member,
            "join-before-invite",
            "join_channel",
            serde_json::json!({"channel":{"type":"id","value":channel_id}}),
        )
        .await;
        assert_eq!(denied["payload"]["code"], "permission_denied");
        command(
            &mut member,
            "accept-invite",
            "accept_circle_invitation",
            serde_json::json!({"token":token}),
        )
        .await;
        command(
            &mut member,
            "join-after-invite",
            "join_channel",
            serde_json::json!({"channel":{"type":"id","value":channel_id}}),
        )
        .await;

        for sequence in 1..=2 {
            command(
                &mut owner,
                &format!("circle-send-{sequence}"),
                "send_message",
                serde_json::json!({"channel_id":channel_id,"body":format!("friend-message-{sequence}")}),
            )
            .await;
        }
        member.close(None).await.unwrap();
        let mut member = connect_as(address, "circle-member").await;
        let listed = command(
            &mut member,
            "list-unread",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        let summary = listed["payload"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|summary| summary["id"] == channel_id)
            .unwrap();
        assert_eq!(summary["last_read_sequence"].as_u64(), Some(0));
        assert_eq!(summary["latest_sequence"].as_u64(), Some(2));
        let loaded = command(
            &mut member,
            "load-unread",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
        )
        .await;
        assert_eq!(loaded["payload"]["messages"].as_array().unwrap().len(), 2);
        command(
            &mut member,
            "mark-all-read",
            "mark_read",
            serde_json::json!({"channel_id":channel_id,"sequence":2}),
        )
        .await;
        member.close(None).await.unwrap();
        let mut member = connect_as(address, "circle-member").await;
        let listed = command(
            &mut member,
            "list-read",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        let summary = listed["payload"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|summary| summary["id"] == channel_id)
            .unwrap();
        assert_eq!(summary["last_read_sequence"].as_u64(), Some(2));
        assert_eq!(summary["latest_sequence"].as_u64(), Some(2));
        server.abort();
    }
}
