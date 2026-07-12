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
        header::{AUTHORIZATION, COOKIE, LOCATION, SET_COOKIE},
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
    agent::{AgentScope, AgentService, CreateAgent, GrantAgent},
    auth::AuthService,
    chat::{ChatEngine, ChatError},
    config::{AppConfig, AuthMode, LogFormat},
    domain::{ChannelId, ChannelSequence, MessageBody, MessageLimit, UserId},
    operations::{OperationalState, healthz, metrics, readyz, record_metrics},
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
}

impl axum::extract::FromRef<AppState> for OperationalState {
    fn from_ref(state: &AppState) -> Self {
        state.operations.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = AppConfig::from_env()?;
    init_tracing(config.log_format())?;
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        db::migrate(config.database()).await?;
        info!(database = %config.database().kind(), "database migrations applied");
        return Ok(());
    }
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
    };
    let request_id_header = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
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
        .route("/mcp", post(mcp_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            operations.clone(),
            record_metrics,
        ))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

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

fn process_gateway_from_env() -> Result<Option<SharedProcessGateway>, crate::process::ProcessError>
{
    let Some(url) = std::env::var("SPROYT_HEART_URL").ok() else {
        return Ok(None);
    };
    let gateway = HeartGateway::new(url, Duration::from_secs(5), 2)?;
    Ok(Some(std::sync::Arc::new(gateway)))
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
    #[serde(default)]
    id: serde_json::Value,
    method: String,
    #[serde(default)]
    params: serde_json::Value,
}

async fn mcp_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpRequest>,
) -> Json<serde_json::Value> {
    let response = match request.method.as_str() {
        "initialize" => Ok(
            serde_json::json!({"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"sproyt","version":env!("CARGO_PKG_VERSION")}}),
        ),
        "notifications/initialized" => Ok(serde_json::Value::Null),
        "tools/list" => Ok(serde_json::json!({"tools": mcp_tools()})),
        "tools/call" => mcp_call(&state, &headers, request.params).await,
        _ => Err((-32601, "method not found".to_owned())),
    };
    Json(match response {
        Ok(result) => serde_json::json!({"jsonrpc":"2.0","id":request.id,"result":result}),
        Err((code, message)) => {
            serde_json::json!({"jsonrpc":"2.0","id":request.id,"error":{"code":code,"message":message}})
        }
    })
}

fn mcp_tools() -> serde_json::Value {
    serde_json::json!([
      {"name":"list_channels","description":"List channels granted to this agent","inputSchema":{"type":"object","properties":{},"additionalProperties":false}},
      {"name":"read_messages","description":"Read channel history","inputSchema":{"type":"object","required":["channel_id"],"properties":{"channel_id":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":200},"after_sequence":{"type":"integer","minimum":0}},"additionalProperties":false}},
      {"name":"send_message","description":"Send an agent-authored message","inputSchema":{"type":"object","required":["channel_id","body","request_id"],"properties":{"channel_id":{"type":"string"},"body":{"type":"string"},"request_id":{"type":"string"}},"additionalProperties":false}},
      {"name":"mark_read","description":"Advance the agent read marker","inputSchema":{"type":"object","required":["channel_id","sequence"],"properties":{"channel_id":{"type":"string"},"sequence":{"type":"integer","minimum":0}},"additionalProperties":false}}
    ])
}

async fn mcp_call(
    state: &AppState,
    headers: &HeaderMap,
    params: serde_json::Value,
) -> Result<serde_json::Value, (i64, String)> {
    let credential = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((-32001, "missing bearer credential".to_owned()))?;
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
            let principal = state
                .agents
                .authenticate(credential)
                .await
                .map_err(mcp_repository_error)?;
            serde_json::to_value(
                state
                    .chat
                    .list_channels(principal.agent_id)
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "read_messages" => {
            let channel = mcp_channel(&args)?;
            let principal = state
                .agents
                .authorize(
                    credential,
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
                    .load_messages(principal.agent_id, channel, MessageLimit::new(limit), after)
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "send_message" => {
            let channel = mcp_channel(&args)?;
            let principal = state
                .agents
                .authorize(
                    credential,
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
            serde_json::to_value(
                state
                    .chat
                    .send_message_idempotent(channel, principal.agent_id, body, request_id)
                    .await
                    .map_err(mcp_chat_error)?,
            )
            .map_err(|e| (-32603, e.to_string()))?
        }
        "mark_read" => {
            let channel = mcp_channel(&args)?;
            let principal = state
                .agents
                .authorize(
                    credential,
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
                    .mark_read(principal.agent_id, channel, ChannelSequence::new(sequence))
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

    operations.set_ready(false);
    info!(grace_period_seconds = 30, "shutdown requested");
    tokio::time::sleep(Duration::from_millis(100)).await;
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
    upgrade
        .on_upgrade(move |socket| ws::handle_socket(state.chat, principal, socket))
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
      let renderMode = "view";
      let requestNumber = 0;
      let activeChannelId = null;
      let requestedChannelSlug = "general";
      const timeline = [];

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
        if (socket) {
          socket.close();
        }

        timeline.length = 0;
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
        });

        socket.addEventListener("message", (event) => {
          renderServerEvent(JSON.parse(event.data));
        });

        socket.addEventListener("close", () => {
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
          payload.history.forEach((message) => timeline.push({ type: "message", message }));
          renderTimeline();
          return;
        }

        if (event.type === "chat") {
          const chatEvent = payload.event;
          if (chatEvent.type === "message_accepted") {
            timeline.push({ type: "message", message: chatEvent.message });
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
          sendCommand("load_recent_messages", {
            channel_id: payload.channel_id,
            after: payload.last_seen_sequence,
            limit: 200
          });
          return;
        }

        if (event.type === "messages_loaded") {
          payload.messages.forEach((message) => timeline.push({ type: "message", message }));
          renderTimeline();
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
        timeline.push({ type: "message", message });
        renderTimeline();
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
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", created.credential)).unwrap(),
        );
        let call = |id| McpRequest {
            id: serde_json::json!(id),
            method: "tools/call".to_owned(),
            params: serde_json::json!({"name":"send_message","arguments":{"channel_id":channel.id.to_string(),"body":"from agent","request_id":"mcp-send-1"}}),
        };
        let first = mcp_handler(State(state.clone()), headers.clone(), Json(call(1)))
            .await
            .0;
        let repeated = mcp_handler(State(state.clone()), headers.clone(), Json(call(2)))
            .await
            .0;
        assert!(first.get("result").is_some(), "{first}");
        assert_eq!(
            first["result"]["structuredContent"]["id"],
            repeated["result"]["structuredContent"]["id"]
        );
        agents.revoke(owner, grant_id).await.unwrap();
        let revoked = mcp_handler(State(state), headers, Json(call(3))).await.0;
        assert!(revoked.get("error").is_some(), "{revoked}");
    }
}
