mod agent;
mod auth;
mod chat;
mod config;
mod db;
mod domain;
mod notification;
mod operations;
mod process;
mod protocol;
mod ws;

use std::{io::Cursor, time::Duration};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, Path, Query, Request, State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{
            ACCEPT, AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE, COOKIE, LOCATION, ORIGIN,
            SET_COOKIE,
        },
    },
    middleware,
    response::{Html, IntoResponse},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    agent::{AgentPrincipal, AgentScope, AgentService, CreateAgent, GrantAgent},
    auth::{AuthError, AuthService},
    chat::{ChatEngine, ChatError},
    config::{AppConfig, AuthMode, LogFormat},
    domain::{
        ChannelId, ChannelSequence, MediaId, MediaUpload, MediaVariant, MessageBody, MessageLimit,
        UserId,
    },
    notification::{NotificationPreferences, NotificationService, PushSubscriptionInput},
    operations::{ClientEvent, OperationalState, healthz, metrics, record_metrics},
    process::{
        EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, HeartGateway, ProcessLinkId,
        ProcessService, SetCircleFeature, SharedProcessGateway,
    },
};

const BUILD_REVISION: &str = match option_env!("SPROYT_BUILD_REVISION") {
    Some(revision) => revision,
    None => "unknown",
};
const PWA_MANIFEST: &str = include_str!("../assets/manifest.webmanifest");
const CLIENT_STORE: &str = include_str!("../assets/client-store.js");
const SERVICE_WORKER: &str = include_str!("../assets/service-worker.js");
const OFFLINE_HTML: &str = include_str!("../assets/offline.html");
const WAVE_LOGO_SVG: &str = include_str!("../assets/sproyt-wave.svg");
const WAVE_LOGO_192: &[u8] = include_bytes!("../assets/sproyt-wave-192.png");
const WAVE_LOGO_512: &[u8] = include_bytes!("../assets/sproyt-wave-512.png");

#[derive(Serialize)]
struct VersionInfo {
    service: &'static str,
    version: &'static str,
    revision: &'static str,
}

#[derive(Clone)]
struct AppState {
    auth: AuthService,
    chat: ChatEngine,
    operations: OperationalState,
    processes: ProcessService,
    agents: AgentService,
    notifications: NotificationService,
    websocket_idle_timeout: Duration,
    advanced_ui_enabled: bool,
    agent_ui_enabled: bool,
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
    let notifications = NotificationService::connect(config.database()).await?;
    notifications.start_worker(operations.subscribe_shutdown());
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
        notifications,
        websocket_idle_timeout: config.websocket_idle_timeout(),
        advanced_ui_enabled: std::env::var("SPROYT_UI_ADVANCED_ENABLED").as_deref() == Ok("true"),
        agent_ui_enabled: std::env::var("SPROYT_UI_AGENT_ENABLED").as_deref() == Ok("true"),
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
        .route("/versionz", get(versionz))
        .route("/manifest.webmanifest", get(pwa_manifest))
        .route("/assets/client-store.js", get(client_store_legacy))
        .route(
            "/assets/client-store/{revision}/client-store.js",
            get(client_store),
        )
        .route("/service-worker.js", get(service_worker))
        .route("/offline", get(offline_page))
        .route("/assets/sproyt-wave.svg", get(wave_logo_svg))
        .route("/assets/sproyt-wave-192.png", get(wave_logo_192))
        .route("/assets/sproyt-wave-512.png", get(wave_logo_512))
        .route("/metrics", get(metrics))
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/session", get(auth_session))
        .route("/auth/refresh", post(auth_refresh))
        .route("/auth/logout", get(auth_logout))
        .route("/api/v1/me/export", get(export_my_data))
        .route("/api/v1/client-events", post(record_client_event))
        .route(
            "/api/v1/me/notifications",
            get(notification_settings).put(save_notification_preferences),
        )
        .route(
            "/api/v1/me/push-subscriptions",
            post(subscribe_push).delete(unsubscribe_push),
        )
        .route("/api/v1/channels/{id}/media", post(upload_media))
        .route("/api/v1/media/{id}", get(download_media))
        .route("/api/v1/media/{id}/preview", get(download_media_preview))
        .route("/ws", get(ws_handler))
        .route("/api/v1/processes", post(start_process))
        .route("/api/v1/processes/{id}", get(get_process))
        .route("/api/v1/processes/{id}/inspect", post(inspect_process))
        .route("/api/v1/processes/{id}/messages", post(correlate_process))
        .route(
            "/api/v1/circles/{id}/features/heart-event-planning",
            post(set_heart_feature),
        )
        .route("/api/v1/agents", post(create_agent))
        .route("/api/v1/agents/{id}/grants", post(grant_agent))
        .route("/api/v1/agents/{id}/revoke", post(revoke_agent))
        .route("/api/v1/agent-grants/{id}/revoke", post(revoke_agent_grant))
        .route(
            "/api/v1/messages/{id}/approve-agent",
            post(approve_agent_message),
        )
        .route("/mcp", post(mcp_handler))
        .with_state(state.clone())
        // Leave room for multipart headers around the 35 MiB media payload.
        .layer(DefaultBodyLimit::max(36 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(
            operations.clone(),
            record_metrics,
        ))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid))
        .layer(middleware::from_fn(add_security_headers))
}

async fn versionz() -> Json<VersionInfo> {
    Json(VersionInfo {
        service: "sproyt",
        version: env!("CARGO_PKG_VERSION"),
        revision: BUILD_REVISION,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ClientEventInput {
    WebsocketConnected,
    WebsocketDisconnected,
    WebsocketError,
    SessionRefreshSucceeded,
    SessionRefreshFailed,
    UploadSucceeded,
    UploadFailed,
}

#[derive(Deserialize)]
struct ClientEventReport {
    event: ClientEventInput,
}

async fn record_client_event(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(report): Json<ClientEventReport>,
) -> axum::response::Response {
    if let Err(error) = authenticate_http(&state, query, &headers).await {
        return auth_error_response(error);
    }
    let event = match report.event {
        ClientEventInput::WebsocketConnected => ClientEvent::WebSocketConnected,
        ClientEventInput::WebsocketDisconnected => ClientEvent::WebSocketDisconnected,
        ClientEventInput::WebsocketError => ClientEvent::WebSocketError,
        ClientEventInput::SessionRefreshSucceeded => ClientEvent::SessionRefreshSucceeded,
        ClientEventInput::SessionRefreshFailed => ClientEvent::SessionRefreshFailed,
        ClientEventInput::UploadSucceeded => ClientEvent::UploadSucceeded,
        ClientEventInput::UploadFailed => ClientEvent::UploadFailed,
    };
    state.operations.record_client_event(event);
    axum::http::StatusCode::NO_CONTENT.into_response()
}

async fn pwa_manifest() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "application/manifest+json"),
            (CACHE_CONTROL, "public, max-age=3600"),
        ],
        PWA_MANIFEST,
    )
        .into_response()
}

fn client_store_fingerprint(build_revision: &str, client_store: &[u8]) -> String {
    let revision_is_safe = (7..=64).contains(&build_revision.len())
        && build_revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'));
    if revision_is_safe {
        return build_revision.to_owned();
    }
    format!("{:x}", Sha256::digest(client_store))
}

async fn client_store_legacy() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
        ],
        CLIENT_STORE,
    )
        .into_response()
}

async fn client_store(Path(fingerprint): Path<String>) -> axum::response::Response {
    if fingerprint != client_store_fingerprint(BUILD_REVISION, CLIENT_STORE.as_bytes()) {
        return axum::http::StatusCode::NOT_FOUND.into_response();
    }
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        CLIENT_STORE,
    )
        .into_response()
}

async fn service_worker() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
            (HeaderName::from_static("service-worker-allowed"), "/"),
            (
                HeaderName::from_static("content-security-policy"),
                "default-src 'none'; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
            ),
        ],
        SERVICE_WORKER,
    )
        .into_response()
}

async fn offline_page() -> axum::response::Response {
    let mut response = Html(OFFLINE_HTML).into_response();
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static("default-src 'self'; img-src 'self'; style-src 'unsafe-inline'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    );
    response
}

async fn wave_logo_svg() -> axum::response::Response {
    static_asset("image/svg+xml", WAVE_LOGO_SVG.as_bytes())
}

async fn wave_logo_192() -> axum::response::Response {
    static_asset("image/png", WAVE_LOGO_192)
}

async fn wave_logo_512() -> axum::response::Response {
    static_asset("image/png", WAVE_LOGO_512)
}

fn static_asset(content_type: &'static str, content: &'static [u8]) -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, content_type),
            (CACHE_CONTROL, "public, max-age=604800, immutable"),
        ],
        content,
    )
        .into_response()
}

async fn add_security_headers(
    request: Request,
    next: middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    // A WebSocket 101 is not a document response. Extra document policy
    // headers are unnecessary there and some proxies treat decorated upgrade
    // responses inconsistently.
    if response.status() == axum::http::StatusCode::SWITCHING_PROTOCOLS {
        return response;
    }
    let headers = response.headers_mut();
    let defaults = [
        (
            "content-security-policy",
            "default-src 'none'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'",
        ),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("cross-origin-opener-policy", "same-origin"),
        ("cache-control", "no-store"),
    ];
    for (name, value) in defaults {
        if !headers.contains_key(name) {
            headers.insert(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
    }
    response
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
    // Readiness protects traffic that needs this replica's durable store. OIDC
    // discovery is verified at startup and refreshed during authentication;
    // putting an internet request here makes kubelet probes remove otherwise
    // healthy replicas whenever the identity provider is merely slow.
    let database = async {
        state
            .chat
            .health_check()
            .await
            .map_err(|error| error.kind())
    };
    match tokio::time::timeout(Duration::from_secs(2), database).await {
        Ok(Ok(())) => (axum::http::StatusCode::OK, "ready\n").into_response(),
        Ok(Err(error_kind)) => {
            warn!(error_kind, "readiness dependency probe failed");
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "dependency unavailable\n",
            )
                .into_response()
        }
        Err(_) => {
            warn!("readiness dependency probe timed out");
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
    let principal = match state
        .auth
        .authenticate_request(query.participant, cookie)
        .await
    {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
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
        Err(error) => repository_response(error),
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
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
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

async fn get_process(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
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
        .get_process(principal.user.id, process_link_id)
        .await
    {
        Ok(view) => Json(view).into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
struct InspectProcessRequest {
    request_id: String,
}

async fn inspect_process(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<InspectProcessRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
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
        .enqueue_inspection(EnqueueInspection {
            process_link_id,
            actor: principal.user.id,
            request_id: body.request_id,
        })
        .await
    {
        Ok(outbox_id) => (
            axum::http::StatusCode::ACCEPTED,
            Json(serde_json::json!({"outbox_id":outbox_id.as_uuid()})),
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
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
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

async fn authenticate_http(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<crate::auth::AuthenticatedPrincipal, crate::auth::AuthError> {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    state
        .auth
        .authenticate_request(query.participant, cookie)
        .await
}

fn repository_response(error: crate::domain::RepositoryError) -> axum::response::Response {
    let status = match error {
        crate::domain::RepositoryError::PermissionDenied => axum::http::StatusCode::FORBIDDEN,
        crate::domain::RepositoryError::NotFound => axum::http::StatusCode::NOT_FOUND,
        crate::domain::RepositoryError::Conflict => axum::http::StatusCode::CONFLICT,
        crate::domain::RepositoryError::Storage(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.public_message()).into_response()
}

fn auth_error_response(error: AuthError) -> axum::response::Response {
    let status = match error {
        AuthError::InvalidIdentity(_) | AuthError::Unsupported(_) => {
            axum::http::StatusCode::BAD_REQUEST
        }
        AuthError::Unauthorized => axum::http::StatusCode::UNAUTHORIZED,
        AuthError::External(_) => axum::http::StatusCode::SERVICE_UNAVAILABLE,
        AuthError::InvalidSessionKey => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.public_message()).into_response()
}

async fn export_my_data(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    if let Err(error) = state.chat.ensure_user(principal.user.clone()).await {
        return chat_error_response(error);
    }
    let export = match state.chat.export_user_data(principal.user.id.clone()).await {
        Ok(export) => export,
        Err(error) => return chat_error_response(error),
    };
    let body = match serde_json::to_vec_pretty(&export) {
        Ok(body) => body,
        Err(_) => return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let filename = format!(
        "attachment; filename=\"sproyt-export-{}.json\"",
        principal.user.id
    );
    let Ok(disposition) = HeaderValue::from_str(&filename) else {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let mut response = body.into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(axum::http::header::CONTENT_DISPOSITION, disposition);
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn notification_settings(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    match state.notifications.settings(principal.user.id).await {
        Ok(settings) => Json(settings).into_response(),
        Err(error) => repository_response(error),
    }
}

async fn save_notification_preferences(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(preferences): Json<NotificationPreferences>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    match state
        .notifications
        .save_preferences(principal.user.id, preferences)
        .await
    {
        Ok(preferences) => Json(preferences).into_response(),
        Err(error) => repository_response(error),
    }
}

async fn subscribe_push(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(subscription): Json<PushSubscriptionInput>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(300).collect());
    match state
        .notifications
        .subscribe(principal.user.id, subscription, user_agent)
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
struct PushUnsubscribe {
    endpoint: String,
}

async fn unsubscribe_push(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(subscription): Json<PushUnsubscribe>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    match state
        .notifications
        .unsubscribe(principal.user.id, subscription.endpoint)
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

const MAX_MEDIA_BYTES: usize = 35 * 1024 * 1024;
const MEDIA_PREVIEW_LONG_EDGE: u32 = 720;

async fn upload_media(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(channel) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let field = match multipart.next_field().await {
        Ok(Some(field)) => field,
        Ok(None) => {
            return (axum::http::StatusCode::BAD_REQUEST, "missing media file").into_response();
        }
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid multipart upload",
            )
                .into_response();
        }
    };
    let filename = field
        .file_name()
        .unwrap_or("upload")
        .chars()
        .filter(|c| !c.is_control())
        .take(255)
        .collect::<String>();
    let declared_type = field
        .content_type()
        .unwrap_or("application/octet-stream")
        .to_owned();
    let content = match field.bytes().await {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_MEDIA_BYTES => bytes.to_vec(),
        Ok(_) => {
            return (
                axum::http::StatusCode::PAYLOAD_TOO_LARGE,
                "media must contain 1 to 35 MiB",
            )
                .into_response();
        }
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "could not read media").into_response();
        }
    };
    let content_type = match detected_media_type(&content, &declared_type) {
        Some(value) => value,
        None => {
            return (
                axum::http::StatusCode::UNSUPPORTED_MEDIA_TYPE,
                "only supported images and videos can be uploaded",
            )
                .into_response();
        }
    };
    let (content, dimensions, preview) = match prepare_uploaded_media(content, &content_type).await
    {
        Ok(value) => value,
        Err(MediaPreparationError::InvalidImage) => {
            return (
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "image is incomplete or invalid",
            )
                .into_response();
        }
        Err(MediaPreparationError::Worker) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "could not prepare image",
            )
                .into_response();
        }
    };
    match state
        .chat
        .store_media(MediaUpload {
            actor: principal.user.id,
            channel_id,
            filename: if filename.is_empty() {
                "upload".into()
            } else {
                filename
            },
            content_type,
            content,
            dimensions,
            preview,
        })
        .await
    {
        Ok(media) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({"media": media, "url": format!("/api/v1/media/{}", media.id)})),
        )
            .into_response(),
        Err(error) => chat_error_response(error),
    }
}

#[derive(Debug)]
enum MediaPreparationError {
    InvalidImage,
    Worker,
}

async fn prepare_uploaded_media(
    content: Vec<u8>,
    content_type: &str,
) -> Result<(Vec<u8>, Option<(u32, u32)>, Option<MediaVariant>), MediaPreparationError> {
    if !matches!(
        content_type,
        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
    ) {
        return Ok((content, None, None));
    }
    if !has_complete_image_container(&content, content_type) {
        return Err(MediaPreparationError::InvalidImage);
    }
    let is_jpeg = content_type == "image/jpeg";
    tokio::task::spawn_blocking(move || {
        use image::GenericImageView;

        let image =
            image::load_from_memory(&content).map_err(|_| MediaPreparationError::InvalidImage)?;
        let orientation = exif_orientation(&content, is_jpeg);
        let image = apply_exif_orientation(image, orientation);
        let normalized_content = if is_jpeg && orientation != 1 {
            let mut output = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 92)
                .encode_image(&image)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            output
        } else {
            content
        };
        let dimensions = image.dimensions();
        if dimensions.0 <= MEDIA_PREVIEW_LONG_EDGE && dimensions.1 <= MEDIA_PREVIEW_LONG_EDGE {
            return Ok((normalized_content, Some(dimensions), None));
        }
        let preview_image = image.resize(
            MEDIA_PREVIEW_LONG_EDGE,
            MEDIA_PREVIEW_LONG_EDGE,
            image::imageops::FilterType::Lanczos3,
        );
        let preview_dimensions = preview_image.dimensions();
        let (preview_type, preview_content) = if preview_image.color().has_alpha() {
            let mut output = Cursor::new(Vec::new());
            preview_image
                .write_to(&mut output, image::ImageFormat::Png)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            ("image/png", output.into_inner())
        } else {
            let mut output = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 82)
                .encode_image(&preview_image)
                .map_err(|_| MediaPreparationError::InvalidImage)?;
            ("image/jpeg", output)
        };
        Ok((
            normalized_content,
            Some(dimensions),
            Some(MediaVariant {
                content_type: preview_type.to_owned(),
                width: preview_dimensions.0,
                height: preview_dimensions.1,
                content: preview_content,
            }),
        ))
    })
    .await
    .map_err(|_| MediaPreparationError::Worker)?
}

fn exif_orientation(content: &[u8], is_jpeg: bool) -> u32 {
    if !is_jpeg {
        return 1;
    }
    let mut cursor = Cursor::new(content);
    exif::Reader::new()
        .read_from_container(&mut cursor)
        .ok()
        .and_then(|metadata| {
            metadata
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|field| field.value.get_uint(0))
        })
        .unwrap_or(1)
}

fn apply_exif_orientation(image: image::DynamicImage, orientation: u32) -> image::DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

fn has_complete_image_container(content: &[u8], content_type: &str) -> bool {
    match content_type {
        // Android camera apps commonly create motion photos by appending video or
        // metadata after the complete JPEG stream. The EOI marker still proves
        // that the image stream is complete; requiring it to be the final two
        // bytes rejects those otherwise valid uploads.
        "image/jpeg" => {
            content.starts_with(&[0xff, 0xd8])
                && content.windows(2).any(|window| window == [0xff, 0xd9])
        }
        "image/png" => content.ends_with(b"\0\0\0\0IEND\xaeB`\x82"),
        "image/gif" => content.last() == Some(&0x3b),
        "image/webp" => content
            .get(4..8)
            .and_then(|size| size.try_into().ok())
            .map(u32::from_le_bytes)
            .is_some_and(|size| usize::try_from(size).ok() == content.len().checked_sub(8)),
        _ => false,
    }
}

async fn download_media(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let media_id = match MediaId::new(id) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let (media, content) = match state.chat.load_media(principal.user.id, media_id).await {
        Ok(value) => value,
        Err(error) => return chat_error_response(error),
    };
    let mut response = content.into_response();
    if let Ok(value) = HeaderValue::from_str(&media.content_type) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=3600"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn download_media_preview(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let media_id = match MediaId::new(id) {
        Ok(value) => value,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .chat
        .load_media_preview(principal.user.id.clone(), media_id)
        .await
    {
        Ok(Some(preview)) => media_content_response(preview.content_type, preview.content),
        Ok(None) => match state.chat.load_media(principal.user.id, media_id).await {
            Ok((media, content)) => media_content_response(media.content_type, content),
            Err(error) => chat_error_response(error),
        },
        Err(error) => chat_error_response(error),
    }
}

fn media_content_response(content_type: String, content: Vec<u8>) -> axum::response::Response {
    let mut response = content.into_response();
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        response
            .headers_mut()
            .insert(axum::http::header::CONTENT_TYPE, value);
    }
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("private, max-age=86400, immutable"),
    );
    response.headers_mut().insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    response
}

fn detected_media_type(content: &[u8], declared: &str) -> Option<String> {
    let detected = if content.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if content.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if content.starts_with(b"GIF87a") || content.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if content.starts_with(b"RIFF") && content.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else if content.get(4..8) == Some(b"ftyp") {
        match content.get(8..12) {
            Some(b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1") => Some("image/heic"),
            Some(b"avif" | b"avis") => Some("image/avif"),
            Some(b"qt  ") => Some("video/quicktime"),
            _ => Some("video/mp4"),
        }
    } else if content.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        Some(if declared == "video/webm" {
            "video/webm"
        } else {
            "video/x-matroska"
        })
    } else {
        None
    };
    detected.map(str::to_owned)
}

fn chat_error_response(error: ChatError) -> axum::response::Response {
    let status = match &error {
        ChatError::Repository(crate::domain::RepositoryError::PermissionDenied) => {
            axum::http::StatusCode::FORBIDDEN
        }
        ChatError::Repository(crate::domain::RepositoryError::NotFound) => {
            axum::http::StatusCode::NOT_FOUND
        }
        ChatError::Repository(crate::domain::RepositoryError::Conflict) => {
            axum::http::StatusCode::CONFLICT
        }
        ChatError::Repository(crate::domain::RepositoryError::Storage(_)) => {
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        }
        ChatError::Validation(_) => axum::http::StatusCode::BAD_REQUEST,
        ChatError::EngineStopped => axum::http::StatusCode::SERVICE_UNAVAILABLE,
    };
    (status, error.public_message()).into_response()
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
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
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
        Ok(created) => {
            let mut response = (axum::http::StatusCode::CREATED, Json(created)).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
            response
        }
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
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
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
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
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

async fn revoke_agent(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
    };
    let agent_id = match UserId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state.agents.revoke_agent(principal.user.id, agent_id).await {
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
        Err(crate::domain::RepositoryError::Conflict) => {
            return axum::http::StatusCode::TOO_MANY_REQUESTS.into_response();
        }
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
      {"name":"mark_read","description":"Advance the agent read marker","inputSchema":{"type":"object","required":["channel_id","sequence"],"properties":{"channel_id":{"type":"string"},"sequence":{"type":"integer","minimum":0}},"additionalProperties":false}},
      {"name":"start_process","description":"Start an enabled Heart process from a channel","inputSchema":{"type":"object","required":["channel_id","request_id","namespace","definition_name"],"properties":{"channel_id":{"type":"string"},"request_id":{"type":"string"},"namespace":{"type":"string"},"definition_name":{"type":"string"},"definition_version":{"type":"string"},"metadata":{"type":"object"}},"additionalProperties":false}},
      {"name":"get_process","description":"Read a process linked to an authorized channel","inputSchema":{"type":"object","required":["process_link_id"],"properties":{"process_link_id":{"type":"string"}},"additionalProperties":false}},
      {"name":"inspect_process","description":"Request a fresh Heart process inspection","inputSchema":{"type":"object","required":["process_link_id","request_id"],"properties":{"process_link_id":{"type":"string"},"request_id":{"type":"string"}},"additionalProperties":false}},
      {"name":"complete_process_work","description":"Correlate an authorized response to a Heart receive node","inputSchema":{"type":"object","required":["process_link_id","request_id","payload"],"properties":{"process_link_id":{"type":"string"},"request_id":{"type":"string"},"payload":{}},"additionalProperties":false}}
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
                        None,
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
        "start_process" => {
            let channel = mcp_channel(&args)?;
            state
                .agents
                .require_scope(
                    principal,
                    None,
                    Some(channel.clone()),
                    AgentScope::StartProcesses,
                )
                .await
                .map_err(mcp_repository_error)?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let namespace = mcp_required_string(&args, "namespace")?;
            let definition_name = mcp_required_string(&args, "definition_name")?;
            let definition_version = args
                .get("definition_version")
                .and_then(|value| value.as_str())
                .map(str::to_owned);
            let metadata = args
                .get("metadata")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            serde_json::to_value(
                state
                    .processes
                    .enqueue_start(EnqueueProcessStart {
                        channel_id: channel,
                        actor: principal.agent_id.clone(),
                        request_id,
                        namespace,
                        definition_name,
                        definition_version,
                        metadata,
                    })
                    .await
                    .map_err(mcp_repository_error)?,
            )
            .map_err(|_| (-32603, "failed to encode tool result".to_owned()))?
        }
        "get_process" => {
            let (_, view) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            serde_json::to_value(view)
                .map_err(|_| (-32603, "failed to encode tool result".to_owned()))?
        }
        "inspect_process" => {
            let (process_link_id, _) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let outbox_id = state
                .processes
                .enqueue_inspection(EnqueueInspection {
                    process_link_id,
                    actor: principal.agent_id.clone(),
                    request_id,
                })
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"outbox_id":outbox_id.as_uuid()})
        }
        "complete_process_work" => {
            let (process_link_id, _) =
                mcp_process_with_scope(state, principal, &args, AgentScope::CompleteProcessWork)
                    .await?;
            let request_id = mcp_required_string(&args, "request_id")?;
            let payload = args
                .get("payload")
                .cloned()
                .ok_or((-32602, "missing payload".to_owned()))?;
            let outbox_id = state
                .processes
                .enqueue_correlation(EnqueueCorrelation {
                    process_link_id,
                    actor: principal.agent_id.clone(),
                    request_id,
                    payload,
                })
                .await
                .map_err(mcp_repository_error)?;
            serde_json::json!({"outbox_id":outbox_id.as_uuid()})
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

fn mcp_required_string(
    args: &serde_json::Value,
    name: &'static str,
) -> Result<String, (i64, String)> {
    args.get(name)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .ok_or((-32602, format!("missing {name}")))
}

async fn mcp_process_with_scope(
    state: &AppState,
    principal: &AgentPrincipal,
    args: &serde_json::Value,
    scope: AgentScope,
) -> Result<(ProcessLinkId, crate::process::ProcessView), (i64, String)> {
    let process_link_id = args
        .get("process_link_id")
        .and_then(|value| value.as_str())
        .ok_or((-32602, "missing process_link_id".to_owned()))
        .and_then(|value| {
            ProcessLinkId::parse(value).map_err(|_| (-32602, "invalid process_link_id".to_owned()))
        })?;
    let view = state
        .processes
        .get_process(principal.agent_id.clone(), process_link_id)
        .await
        .map_err(mcp_repository_error)?;
    state
        .agents
        .require_scope(
            principal,
            None,
            Some(view.process.channel_id.clone()),
            scope,
        )
        .await
        .map_err(mcp_repository_error)?;
    Ok((process_link_id, view))
}
fn mcp_repository_error(error: crate::domain::RepositoryError) -> (i64, String) {
    (-32003, error.public_message().to_owned())
}
fn mcp_chat_error(error: ChatError) -> (i64, String) {
    (-32004, error.public_message())
}

async fn approve_agent_message(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return auth_error_response(error),
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

#[derive(Debug, Default, Deserialize)]
struct InviteQuery {
    invite: Option<String>,
}

async fn index(
    State(state): State<AppState>,
    Query(query): Query<InviteQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    let principal = match state.auth.authenticate_request(None, cookie).await {
        Ok(principal) => principal,
        Err(AuthError::Unauthorized) => {
            // A suspended browser may have lost its short access session while
            // retaining the long-lived refresh credential. Renew locally and
            // retry this URL before involving the identity provider.
            if let Ok(renewal) = state.auth.renew_session(cookie).await {
                let return_to = query
                    .invite
                    .as_deref()
                    .filter(|token| is_safe_invitation_token(token))
                    .map_or_else(|| "/".to_owned(), |token| format!("/?invite={token}"));
                return redirect_with_cookies(
                    &return_to,
                    &[renewal.set_cookie, renewal.set_refresh_cookie],
                );
            }
            let login_location = query
                .invite
                .filter(|token| is_safe_invitation_token(token))
                .map_or_else(
                    || "/auth/login".to_owned(),
                    |token| format!("/auth/login?invite={token}"),
                );
            return (
                axum::http::StatusCode::SEE_OTHER,
                [(LOCATION, login_location)],
            )
                .into_response();
        }
        Err(error) => return auth_error_response(error),
    };
    let mut random = [0_u8; 18];
    if getrandom::fill(&mut random).is_err() {
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let nonce = URL_SAFE_NO_PAD.encode(random);
    let client_store_revision = client_store_fingerprint(BUILD_REVISION, CLIENT_STORE.as_bytes());
    let client_store_url = format!("/assets/client-store/{client_store_revision}/client-store.js");
    let html = INDEX_HTML
        .replace("{{NONCE}}", &nonce)
        .replace("{{CLIENT_STORE_URL}}", &client_store_url)
        .replace(
            "{{DISPLAY_NAME}}",
            &escape_html(&principal.user.display_name.to_string()),
        )
        .replace(
            "{{ADVANCED_HIDDEN}}",
            if state.advanced_ui_enabled {
                ""
            } else {
                "hidden"
            },
        )
        .replace(
            "{{AGENT_HIDDEN}}",
            if state.agent_ui_enabled { "" } else { "hidden" },
        );
    let policy = format!(
        "default-src 'self'; script-src 'self' 'nonce-{nonce}' https://cdn.jsdelivr.net; worker-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
    );
    let mut response = Html(html).into_response();
    let headers = response.headers_mut();
    let security_headers = [
        ("content-security-policy", policy.as_str()),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        (
            "permissions-policy",
            "camera=(), microphone=(), geolocation=()",
        ),
        ("cross-origin-opener-policy", "same-origin"),
        ("cache-control", "no-store"),
    ];
    for (name, value) in security_headers {
        let Ok(value) = HeaderValue::from_str(value) else {
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        headers.insert(HeaderName::from_static(name), value);
    }
    response
}

fn is_safe_invitation_token(token: &str) -> bool {
    (16..=512).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
    let cookie = headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let requested_name = query.participant;
    let principal = match state
        .auth
        .authenticate_request(requested_name.clone(), cookie.as_deref())
        .await
    {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let shutdown = state.operations.subscribe_shutdown();
    upgrade
        .on_upgrade(move |socket| {
            ws::handle_socket(
                state.chat,
                ws::SocketAuthentication::new(state.auth, principal, requested_name, cookie),
                socket,
                shutdown,
                state.websocket_idle_timeout,
            )
        })
        .into_response()
}

async fn auth_login(
    State(state): State<AppState>,
    Query(query): Query<InviteQuery>,
) -> axum::response::Response {
    let return_to = query
        .invite
        .filter(|token| is_safe_invitation_token(token))
        .map(|token| format!("/?invite={token}"));
    match state.auth.login(return_to) {
        Ok(login) => redirect_with_cookies(&login.authorization_url, &[login.set_cookie]),
        Err(error) => auth_error_response(error),
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
                    error.public_message(),
                )
                    .into_response();
            }
            let mut cookies = vec![login.set_cookie, login.clear_transaction_cookie];
            if let Some(refresh_cookie) = login.set_refresh_cookie {
                cookies.push(refresh_cookie);
            }
            redirect_with_cookies(&login.return_to, &cookies)
        }
        Err(error) => auth_error_response(error),
    }
}

async fn auth_logout(State(state): State<AppState>) -> axum::response::Response {
    let logout = state.auth.logout();
    redirect_with_cookies(
        &logout.redirect_url,
        &[
            logout.clear_cookie,
            logout.clear_refresh_cookie,
            logout.clear_legacy_refresh_cookie,
        ],
    )
}

async fn auth_refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    match state.auth.renew_session(cookie).await {
        Ok(renewal) => {
            let mut response = Json(serde_json::json!({
                "refresh_after_seconds": renewal.refresh_after_seconds
            }))
            .into_response();
            match HeaderValue::from_str(&renewal.set_cookie) {
                Ok(cookie) => {
                    response.headers_mut().append(SET_COOKIE, cookie);
                    if let Ok(refresh_cookie) = HeaderValue::from_str(&renewal.set_refresh_cookie) {
                        response.headers_mut().append(SET_COOKIE, refresh_cookie);
                    }
                    response.headers_mut().insert(
                        axum::http::header::CACHE_CONTROL,
                        HeaderValue::from_static("no-store"),
                    );
                    response
                }
                Err(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }
        Err(crate::auth::AuthError::Unsupported(_)) => {
            auth_error_response(crate::auth::AuthError::Unauthorized)
        }
        Err(error) => auth_error_response(error),
    }
}

async fn auth_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    match state.auth.session_refresh_after(cookie) {
        Ok(refresh_after_seconds) => {
            let mut response = Json(serde_json::json!({
                "refresh_after_seconds": refresh_after_seconds
            }))
            .into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(error) => auth_error_response(error),
    }
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

const INDEX_HTML: &str = include_str!("../assets/index.html");

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
}

#[cfg(test)]
mod protocol_capacity_tests {
    use super::*;
    use crate::{
        agent::{AgentRepository, AgentService},
        db::{PostgresChatRepository, SqliteChatRepository},
        process::{ProcessRepository, ProcessService, StartedProcess},
    };
    use futures_util::{SinkExt, StreamExt};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Instant,
    };
    use tokio_tungstenite::{
        MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message as ClientMessage,
    };

    type TestSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

    #[test]
    fn media_signatures_override_untrusted_declared_types() {
        assert_eq!(
            detected_media_type(b"\x89PNG\r\n\x1a\nrest", "text/html").as_deref(),
            Some("image/png")
        );
        assert_eq!(detected_media_type(b"not media", "image/png"), None);
        assert_eq!(
            detected_media_type(b"\0\0\0\x18ftypheicrest", "application/octet-stream").as_deref(),
            Some("image/heic")
        );
        assert_eq!(
            detected_media_type(b"\0\0\0\x14ftypqt  rest", "video/quicktime").as_deref(),
            Some("video/quicktime")
        );
    }

    #[test]
    fn browser_exposes_paste_upload_and_safe_media_rendering() {
        assert!(INDEX_HTML.contains("bodyInput.addEventListener(\"paste\""));
        assert!(INDEX_HTML.contains("accept=\"image/*,video/*,.heic,.heif,.mov\""));
        assert!(INDEX_HTML.contains("/api/v1/channels/${activeChannelId}/media"));
        assert!(INDEX_HTML.contains("response.status === 401 && await refreshSession(true)"));
        assert!(INDEX_HTML.contains("element.loading = \"lazy\""));
        assert!(INDEX_HTML.contains("element.controls = true"));
        assert!(INDEX_HTML.contains("/api/v1/media/${media.id}/preview"));
        assert!(INDEX_HTML.contains("function openMediaLightbox(url, name)"));
        assert!(INDEX_HTML.contains("mediaLightbox.showModal()"));
        assert!(INDEX_HTML.contains("max-height: min(48dvh, 420px)"));
        assert!(INDEX_HTML.contains("max-width: calc(100vw - 24px)"));
        assert!(INDEX_HTML.contains("id=\"upload-status\""));
        assert!(INDEX_HTML.contains("request.upload.addEventListener(\"progress\""));
        assert!(INDEX_HTML.contains("Behandlar fila"));
        assert!(INDEX_HTML.contains("className = \"media-preview-remove\""));
        assert!(
            INDEX_HTML.contains(
                "remove.setAttribute(\"aria-label\", `Fjern ${media.original_filename}`)"
            )
        );
        assert!(INDEX_HTML.contains(
            "pendingMedia = pendingMedia.filter((candidate) => candidate.id !== media.id)"
        ));
        assert!(INDEX_HTML.contains("if (pendingMessages.size > 0) return"));
        assert!(INDEX_HTML.contains("bodyInput.focus({ preventScroll: true })"));
    }

    #[test]
    fn browser_uses_a_compact_composer_with_safe_keyboard_semantics() {
        assert!(INDEX_HTML.contains("--composer-rest-height: 44px"));
        assert!(INDEX_HTML.contains("--composer-max-height: 126px"));
        assert!(INDEX_HTML.contains("height: 44px; min-width: 44px; min-height: 44px"));
        assert!(INDEX_HTML.contains("resize: none; overflow-y: hidden"));
        assert!(INDEX_HTML.contains("function resizeComposer()"));
        assert!(INDEX_HTML.contains("bodyInput.value.length === 0\n          ? minimum"));
        assert!(
            INDEX_HTML.contains("bodyInput.value.length > 0 && bodyInput.scrollHeight > maximum")
        );
        assert!(INDEX_HTML.contains("form.send.is-expanded #media-previews:not(:empty)"));
        assert!(INDEX_HTML.contains("form.send.is-expanded #upload-status:not(:empty)"));
        assert!(INDEX_HTML.contains("min-width: 44px; min-height: 44px"));
        assert!(INDEX_HTML.contains("composer-icon\" id=\"attach-media\""));
        assert!(INDEX_HTML.contains("id=\"composer-tools\" aria-label=\"Meldingsverktøy\" hidden"));
        assert!(INDEX_HTML.contains("composerTools.hidden = !composerHasFocus"));
        assert!(INDEX_HTML.contains("sendForm.addEventListener(\"focusin\""));
        assert!(INDEX_HTML.contains(
            "sendForm.addEventListener(\"focusout\", closeComposerToolsAfterFocusLeaves)"
        ));
        assert!(INDEX_HTML.contains("document.addEventListener(\"pointerdown\""));
        assert!(INDEX_HTML.contains("if (sendForm.contains(event.target)) return"));
        assert!(INDEX_HTML.contains("if (sendForm.contains(document.activeElement)) return"));
        assert!(INDEX_HTML.contains("messageEmojiPicker.open = false"));
        assert!(INDEX_HTML.contains("aria-label=\"Send melding\" title=\"Send melding\""));
        assert!(!INDEX_HTML.contains(">Send</button>"));
        assert!(INDEX_HTML.contains("compositionstart"));
        assert!(INDEX_HTML.contains("compositionend"));
        assert!(INDEX_HTML.contains("event.keyCode !== 229"));
        assert!(INDEX_HTML.contains("!event.isComposing"));
        assert!(INDEX_HTML.contains("usesDesktopComposerKeys.matches"));
        assert!(INDEX_HTML.contains("sendForm.requestSubmit()"));
        assert!(INDEX_HTML.contains("@media (prefers-reduced-motion: no-preference)"));
        assert!(INDEX_HTML.contains("attachMediaButton.disabled = !writableChannel"));
        assert!(INDEX_HTML.contains("syncComposerState();"));
    }

    #[test]
    fn browser_keeps_compact_status_controls_and_saves_the_complete_draft() {
        assert!(INDEX_HTML.contains("class=\"status-fields\""));
        assert!(INDEX_HTML.contains("class=\"status-emoji-options\""));
        assert!(INDEX_HTML.contains("class=\"secondary-button\" id=\"clear-status\""));
        assert!(INDEX_HTML.contains(">Nullstill</button>"));
        assert!(INDEX_HTML.contains(">Slå på varsling</button>"));
        assert!(INDEX_HTML.contains("class=\"logout-link\""));
        assert!(INDEX_HTML.contains("class=\"logout-icon\" aria-hidden=\"true\""));
        assert!(INDEX_HTML.contains(".inbox-icon { display: grid; width: 26px; height: 26px"));
        assert!(INDEX_HTML.contains("letter-spacing: .01em"));
        assert!(
            INDEX_HTML.contains("const statusDraft = { emoji: \"\", text: \"\", dirty: false }")
        );
        assert!(INDEX_HTML.contains("statusDraft.emoji = statusEmoji.value"));
        assert!(INDEX_HTML.contains("statusDraft.text = statusText.value"));
        assert!(INDEX_HTML.contains(
            "sendCommand(\"set_status\", { text: statusDraft.text, emoji: statusDraft.emoji, expires_at: null })"
        ));
        assert!(INDEX_HTML.contains("if (!statusDraft.dirty)"));
        assert!(INDEX_HTML.contains(
            "if (payload.profile.id === currentParticipantId) statusDraft.dirty = false"
        ));
    }

    #[test]
    fn browser_keeps_desktop_sidebar_controls_compact_and_reachable() {
        assert!(INDEX_HTML.contains("id=\"desktop-sidebar-toggle\""));
        assert!(INDEX_HTML.contains("sproyt.desktop-sidebar-collapsed.v1"));
        assert!(
            INDEX_HTML.contains("main.desktop-sidebar-collapsed { grid-template-columns: 56px")
        );
        assert!(
            INDEX_HTML.contains("main.desktop-sidebar-expanded { grid-template-columns: 280px")
        );
        assert!(INDEX_HTML.contains("id=\"desktop-advanced-entry\""));
        assert!(INDEX_HTML.contains(
            ".advanced-tools button:not([disabled]), .advanced-tools input:not([disabled])"
        ));
        assert!(INDEX_HTML.contains("processTitle.tabIndex = -1"));
        assert!(INDEX_HTML.contains("[data-tooltip]:hover::after, .sidebar.desktop-collapsed [data-tooltip]:focus-visible::after"));
        assert!(INDEX_HTML.contains("data-tooltip=\"Kollaps menyen\""));
        assert!(INDEX_HTML.contains("data-tooltip=\"Set status\""));
        assert!(INDEX_HTML.contains("data-tooltip=\"Varsel\""));
        assert!(INDEX_HTML.contains("data-tooltip=\"Ulest\""));
        assert!(INDEX_HTML.contains("data-tooltip=\"Omtalar\""));
        assert!(INDEX_HTML.contains("data-tooltip=\"Oppgåver\""));
        assert!(INDEX_HTML.contains("button.dataset.tooltip = buttonLabel"));
        assert!(INDEX_HTML.contains("currentStatus.dataset.tooltip = statusLabel"));
        assert!(INDEX_HTML.contains("notificationSummary.dataset.tooltip = notificationLabel"));
    }

    #[test]
    fn browser_is_an_installable_pwa_with_bounded_offline_caching() {
        let manifest: serde_json::Value = serde_json::from_str(PWA_MANIFEST).unwrap();
        assert_eq!(manifest["name"], "Sprøyt");
        assert_eq!(manifest["display"], "standalone");
        assert_eq!(manifest["start_url"], "/");
        assert!(
            manifest["icons"]
                .as_array()
                .is_some_and(|icons| icons.len() >= 3)
        );
        assert!(INDEX_HTML.contains("rel=\"manifest\" href=\"/manifest.webmanifest\""));
        assert!(INDEX_HTML.contains("navigator.serviceWorker.register"));
        assert!(INDEX_HTML.contains("/assets/sproyt-wave.svg"));
        assert!(INDEX_HTML.contains("viewport-fit=cover"));
        assert!(INDEX_HTML.contains("--app-height: 100dvh"));
        assert!(INDEX_HTML.contains("env(safe-area-inset-bottom)"));
        assert!(INDEX_HTML.contains("const height = viewport?.height || window.innerHeight"));
        assert!(INDEX_HTML.contains("--app-offset-top: 0px"));
        assert!(INDEX_HTML.contains("height: var(--app-height)"));
        assert!(!INDEX_HTML.contains("width: min(1120px, 100%)"));
        assert!(!INDEX_HTML.contains("height: min(760px, calc(100dvh - 48px));"));
        assert!(INDEX_HTML.contains("overflow-y: auto;\n        overscroll-behavior: contain;\n        scrollbar-gutter: stable;"));
        assert!(SERVICE_WORKER.contains("request.mode === \"navigate\""));
        assert!(SERVICE_WORKER.contains("url.pathname.startsWith(\"/api/\")"));
        assert!(SERVICE_WORKER.contains("url.pathname.startsWith(\"/auth/\")"));
        assert_eq!(&WAVE_LOGO_192[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&WAVE_LOGO_512[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[tokio::test]
    async fn image_upload_creates_a_bounded_preview_and_rejects_truncation() {
        use image::GenericImageView;

        let source = image::DynamicImage::new_rgb8(1_440, 900);
        let mut encoded = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90)
            .encode_image(&source)
            .unwrap();
        let (_, dimensions, preview) = prepare_uploaded_media(encoded, "image/jpeg").await.unwrap();
        assert_eq!(dimensions, Some((1_440, 900)));
        let preview = preview.unwrap();
        assert_eq!(preview.content_type, "image/jpeg");
        let decoded = image::load_from_memory(&preview.content).unwrap();
        assert_eq!(decoded.dimensions(), (720, 450));

        let portrait_pixels = image::DynamicImage::new_rgb8(1_440, 900);
        let mut iphone_jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut iphone_jpeg, 90)
            .encode_image(&portrait_pixels)
            .unwrap();
        let exif_orientation_6 = [
            0xff, 0xe1, 0x00, 0x22, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 0x2a, 0, 8, 0, 0, 0,
            1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ];
        iphone_jpeg.splice(2..2, exif_orientation_6);
        let (normalized, dimensions, preview) = prepare_uploaded_media(iphone_jpeg, "image/jpeg")
            .await
            .unwrap();
        assert_eq!(dimensions, Some((900, 1_440)));
        let decoded = image::load_from_memory(&preview.unwrap().content).unwrap();
        assert_eq!(decoded.dimensions(), (450, 720));
        let normalized = image::load_from_memory(&normalized).unwrap();
        assert_eq!(normalized.dimensions(), (900, 1_440));

        let small_portrait_pixels = image::DynamicImage::new_rgb8(640, 480);
        let mut small_samsung_jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut small_samsung_jpeg, 90)
            .encode_image(&small_portrait_pixels)
            .unwrap();
        small_samsung_jpeg.splice(2..2, exif_orientation_6);
        let (normalized, dimensions, preview) =
            prepare_uploaded_media(small_samsung_jpeg, "image/jpeg")
                .await
                .unwrap();
        assert_eq!(dimensions, Some((480, 640)));
        assert!(preview.is_none());
        let normalized = image::load_from_memory(&normalized).unwrap();
        assert_eq!(normalized.dimensions(), (480, 640));

        let source = image::DynamicImage::new_rgb8(32, 24);
        let mut motion_photo = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut motion_photo, 90)
            .encode_image(&source)
            .unwrap();
        motion_photo.extend_from_slice(b"appended Android motion-photo payload");
        let (_, dimensions, preview) = prepare_uploaded_media(motion_photo, "image/jpeg")
            .await
            .unwrap();
        assert_eq!(dimensions, Some((32, 24)));
        assert!(preview.is_none());

        let truncated = prepare_uploaded_media(vec![0xff, 0xd8, 0xff], "image/jpeg").await;
        assert!(matches!(
            truncated,
            Err(MediaPreparationError::InvalidImage)
        ));
    }

    #[test]
    fn browser_exposes_circle_scoped_mention_autocomplete() {
        assert!(INDEX_HTML.contains("id=\"mention-suggestions\""));
        assert!(INDEX_HTML.contains("aria-autocomplete=\"list\""));
        assert!(INDEX_HTML.contains("sendCommand(\"list_circle_users\""));
        assert!(INDEX_HTML.contains("knownCircleUsers.get(channel.circle_id)"));
        assert!(INDEX_HTML.contains("mentionHandle(user).startsWith(query)"));
        assert!(INDEX_HTML.contains("event.key === \"ArrowDown\""));
        assert!(INDEX_HTML.contains("event.key === \"Enter\""));
        assert!(INDEX_HTML.contains("selectMention(selectedMentionIndex)"));
    }

    #[test]
    fn browser_exposes_durable_reaction_badges() {
        assert!(INDEX_HTML.contains("className = \"reaction-badge\""));
        assert!(INDEX_HTML.contains("`${emoji} ${reaction.count}`"));
        assert!(INDEX_HTML.contains("aria-pressed"));
        assert!(INDEX_HTML.contains("sendCommand(\"toggle_message_reaction\""));
        assert!(INDEX_HTML.contains("sendCommand(\"list_channel_reactions\""));
        assert!(INDEX_HTML.contains("event.type === \"message_reaction_changed\""));
        assert!(INDEX_HTML.contains("chatEvent.type === \"message_reaction_changed\""));
        assert!(INDEX_HTML.contains("className = \"message-reaction-details\""));
        assert!(INDEX_HTML.contains("reactionHeading.textContent = \"Reaksjonar\""));
        assert!(INDEX_HTML.contains("reaction.user_ids || []"));
        assert!(INDEX_HTML.contains("activeProfile(userId)?.display_name"));
        assert!(INDEX_HTML.contains("id=\"reaction-emoji-catalog\""));
        assert!(INDEX_HTML.contains("Søk eller lim inn Unicode-emoji"));
        assert!(INDEX_HTML.contains("submitCustomReaction"));
    }

    #[test]
    fn browser_patches_only_affected_reaction_card_with_timeline_fallback() {
        assert!(INDEX_HTML.contains("function patchMessageReactions(messageId)"));
        assert!(INDEX_HTML.contains("|| [...threadReplies.values()].flat().find"));
        assert!(INDEX_HTML.contains("for (const container of [messagesEl, threadMessages])"));
        assert!(
            INDEX_HTML
                .contains("const nextReactions = renderMessageReactions(message, (open) => {")
        );
        assert!(INDEX_HTML.contains("card.classList.toggle(\"reaction-picker-requested\", open)"));
        assert!(INDEX_HTML.contains("const thread = reactions.querySelector(\".thread-link\");"));
        assert!(INDEX_HTML.contains("const menu = card.querySelector(\".message-menu\");"));
        assert!(INDEX_HTML.contains("if (thread) nextReactions.append(thread);"));
        assert!(
            INDEX_HTML.contains(
                "if (menu) placeMessageMenu(card, nextReactions, menu, thread, messageId);"
            )
        );
        assert!(INDEX_HTML.contains("reactions.replaceWith(nextReactions);"));
        assert!(!INDEX_HTML.contains("reactions.replaceWith(renderMessageReactions(message));"));
        assert!(INDEX_HTML.contains("if (!card || !reactions) continue;"));

        let patch = INDEX_HTML
            .split("function patchMessageReactions(messageId) {")
            .nth(1)
            .and_then(|value| value.split("\n      function appendMessage").next())
            .expect("keyed reaction patch helper");
        let capture = patch
            .find("const interaction = captureMessageInteraction(container);")
            .expect("capture interaction before patch");
        let replace = patch
            .find("reactions.replaceWith(nextReactions);")
            .expect("replace reaction footer");
        let restore = patch
            .find("restoreMessageInteraction(container, interaction);")
            .expect("restore interaction after patch");
        assert!(capture < replace && replace < restore);

        for reaction_event in [
            "if (event.type === \"message_reaction_changed\") {\n          if (payload.change.channel_id === activeChannelId) {\n            applyReactionChange(payload.change);\n            if (!patchMessageReactions(payload.change.message_id)) {\n              renderTimeline({ preserveScroll: true });\n            }",
            "} else if (chatEvent.type === \"message_reaction_changed\") {\n            if (chatEvent.change.channel_id === activeChannelId) {\n              applyReactionChange(chatEvent.change);\n              if (!patchMessageReactions(chatEvent.change.message_id)) {\n                renderTimeline({ preserveScroll: true });\n              }",
        ] {
            assert!(INDEX_HTML.contains(reaction_event));
        }
    }

    #[test]
    fn browser_keepalive_does_not_rebuild_interactive_message_views() {
        let heartbeat = INDEX_HTML
            .split("connectionSupervisor.state.heartbeatTimer = window.setInterval(() => {")
            .nth(1)
            .and_then(|value| value.split("}, 20_000);").next())
            .expect("heartbeat block");
        assert!(heartbeat.contains("sendCommand(\"ping\")"));
        assert!(!heartbeat.contains("list_users"));
        assert!(!heartbeat.contains("list_my_channels"));
        assert!(!heartbeat.contains("list_mentions"));
        assert!(!heartbeat.contains("list_tasks"));
        assert!(INDEX_HTML.contains("function refreshVisibleProfileStatuses(userId = null)"));
        assert!(INDEX_HTML.contains("senderLabel.dataset.profileUserId = message.sender_id"));
        assert!(INDEX_HTML.contains("refreshVisibleProfileStatuses(payload.profile.id)"));
        assert!(INDEX_HTML.contains("const interaction = captureMessageInteraction(messagesEl)"));
        assert!(INDEX_HTML.contains("restoreMessageInteraction(messagesEl, interaction)"));
        assert!(INDEX_HTML.contains(".reaction-picker[open]"));
        assert!(INDEX_HTML.contains("focus({ preventScroll: true })"));
    }

    #[test]
    fn browser_rotates_sockets_only_for_real_session_changes() {
        assert!(INDEX_HTML.contains("event.data?.type === \"session_rotated\""));
        assert!(INDEX_HTML.contains("type: \"session_rotated\""));
        assert!(INDEX_HTML.contains("if (connectionSupervisor.state.socketHandoff) return"));
        assert!(
            INDEX_HTML.contains("const handoff = { previousSocket, nextSocket, timeoutId: null }")
        );
        assert!(INDEX_HTML.contains("}, 10_000);"));
        assert!(INDEX_HTML.contains("session handoff timed out"));
        assert!(
            INDEX_HTML
                .contains("if (handoff.timeoutId !== null) window.clearTimeout(handoff.timeoutId)")
        );
        let handoff_open = INDEX_HTML
            .split("if (previousSocket) {")
            .nth(1)
            .and_then(|value| {
                value
                    .split("reportClientEvent(\"websocket_connected\")")
                    .next()
            })
            .expect("socket handoff open block");
        assert!(!handoff_open.contains("subscribedChannelId = null"));
        assert!(handoff_open.contains("setConnectionStatus(\"Gjenopprettar samtalen …\")"));
        assert!(INDEX_HTML.contains("finishSocketHandoff(connectionSupervisor.state.socket)"));
        assert!(
            INDEX_HTML
                .contains("connectionSupervisor.state.socketHandoff?.nextSocket === nextSocket")
        );
        assert!(INDEX_HTML.contains("connectionSupervisor.state.socket = fallbackSocket"));
        assert!(!INDEX_HTML.contains("}, 500);"));
        assert!(INDEX_HTML.contains("connectionSupervisor.recover(false)"));
        assert!(INDEX_HTML.contains(
            ".catch(() => connectionSupervisor.scheduleReconnect(1006, \"kunne ikkje gjenopprette sambandet\"))"
        ));
        assert!(!INDEX_HTML.contains(
            "recoverConnection(true).catch(() => scheduleReconnect(1006, \"kunne ikkje gjenopprette sambandet\"))"
        ));
    }

    #[test]
    fn browser_routes_session_connection_and_events_through_supervisors() {
        assert!(INDEX_HTML.contains("const sessionSupervisor = (() => {"));
        assert!(INDEX_HTML.contains("const connectionSupervisor = (() => {"));
        assert!(
            INDEX_HTML
                .contains("import { createApplicationStore, createServerEventMailbox } from \"{{CLIENT_STORE_URL}}\";")
        );
        assert!(INDEX_HTML.contains("const applicationStore = createApplicationStore();"));
        assert!(CLIENT_STORE.contains("export function createApplicationStore()"));
        assert!(CLIENT_STORE.contains("updateSession(patch)"));
        assert!(CLIENT_STORE.contains("updateConnection(patch)"));
        assert!(CLIENT_STORE.contains("reduceServerEvent(event)"));
        assert!(
            CLIENT_STORE.contains("export function createServerEventMailbox({ reduce, deliver })")
        );
        assert!(CLIENT_STORE.contains("deliver(reduce(queue.shift()));"));
        let mailbox = CLIENT_STORE
            .split("export function createServerEventMailbox({ reduce, deliver }) {")
            .nth(1)
            .expect("serialized mailbox factory");
        let queued = mailbox.find("queue.push(event);").expect("enqueue event");
        let reduce_then_deliver = mailbox
            .find("deliver(reduce(queue.shift()));")
            .expect("reduce before delivery");
        assert!(queued < reduce_then_deliver);
        assert!(INDEX_HTML.contains("const serverEventMailbox = createServerEventMailbox({"));
        assert!(INDEX_HTML.contains("reduce: applicationStore.reduceServerEvent,"));
        assert!(INDEX_HTML.contains("deliver: renderServerEvent"));
        assert!(!INDEX_HTML.contains("const applicationStore = (() => {"));
        assert!(!INDEX_HTML.contains("const serverEventMailbox = (() => {"));
        assert!(!INDEX_HTML.contains("let sessionRefreshTimer"));
        assert!(!INDEX_HTML.contains("let sessionRefreshPromise"));
        assert!(!INDEX_HTML.contains("let authenticationRecoveryPromise"));
        assert!(!INDEX_HTML.contains("let connectionRecoveryPromise"));
        assert!(!INDEX_HTML.contains("let reconnectTimer"));
        assert!(!INDEX_HTML.contains("let reconnectAttempt"));
        assert!(!INDEX_HTML.contains("let heartbeatTimer"));
        assert!(!INDEX_HTML.contains("let stableConnectionTimer"));
        assert!(!INDEX_HTML.contains("let socket = null"));
        assert!(!INDEX_HTML.contains("let socketHandoff = null"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.recoveryPromise"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.reconnectTimer"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.reconnectAttempt"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.heartbeatTimer"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.stableConnectionTimer"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.socket"));
        assert!(INDEX_HTML.contains("connectionSupervisor.state.socketHandoff"));
        assert!(INDEX_HTML.contains("sessionSupervisor.start();"));
        assert!(INDEX_HTML.contains("connectionSupervisor.start();"));
        assert!(INDEX_HTML.contains("sessionSupervisor.schedule(seconds)"));
        assert!(INDEX_HTML.contains("connectionSupervisor.replaceAfterSessionRefresh()"));
        assert!(INDEX_HTML.contains("serverEventMailbox.enqueue(JSON.parse(event.data))"));
        assert!(!INDEX_HTML.contains("renderServerEvent(JSON.parse(event.data))"));
    }

    #[test]
    fn client_store_fingerprint_uses_safe_revisions_or_asset_hashes() {
        assert_eq!(
            client_store_fingerprint("a1b2c3d", b"first asset"),
            "a1b2c3d"
        );
        assert_eq!(
            client_store_fingerprint(&"a".repeat(64), b"first asset"),
            "a".repeat(64)
        );

        let unknown = client_store_fingerprint("unknown", b"first asset");
        assert_eq!(unknown.len(), 64);
        assert_ne!(unknown, "unknown");
        assert_ne!(
            unknown,
            client_store_fingerprint("unknown", b"second asset")
        );
        assert_ne!(
            client_store_fingerprint("ABCDEF0", b"first asset"),
            "ABCDEF0"
        );
    }

    #[test]
    fn browser_exposes_author_owned_message_editing() {
        assert!(INDEX_HTML.contains("sendCommand(\"edit_message\""));
        assert!(INDEX_HTML.contains("message.sender_id === currentParticipantId"));
        assert!(INDEX_HTML.contains("chatEvent.type === \"message_edited\""));
        assert!(INDEX_HTML.contains("event.type === \"message_edited\""));
        assert!(INDEX_HTML.contains("· redigert"));
        assert!(INDEX_HTML.contains("className = \"message-editor\""));
        assert!(
            INDEX_HTML.contains("const mediaTokens = message.body.match(mediaTokenPattern) || []")
        );
    }

    #[test]
    fn browser_exposes_author_owned_soft_deletion() {
        assert!(INDEX_HTML.contains("sendCommand(\"delete_message\""));
        assert!(INDEX_HTML.contains("chatEvent.type === \"message_deleted\""));
        assert!(INDEX_HTML.contains("event.type === \"message_deleted\""));
        assert!(INDEX_HTML.contains("Meldinga er sletta."));
        assert!(INDEX_HTML.contains("window.confirm(\"Vil du slette meldinga?"));
        assert!(INDEX_HTML.contains(
            "if (!message.deleted_at) {\n          footer = renderMessageReactions(message, (open) => {"
        ));
        assert!(INDEX_HTML.contains("if (!message.deleted_at) {\n          const menu = document.createElement(\"details\");"));
    }

    #[test]
    fn browser_exposes_compact_durable_message_threads() {
        assert!(INDEX_HTML.contains("id=\"thread-panel\""));
        assert!(INDEX_HTML.contains("grid-template-rows: auto minmax(0, 1fr) auto"));
        assert!(INDEX_HTML.contains("height: min(760px, calc(var(--app-height) - 24px))"));
        assert!(INDEX_HTML.contains(".thread-messages { display: grid; min-height: 0;"));
        assert!(INDEX_HTML.contains(".thread-form { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-self: end;"));
        assert!(INDEX_HTML.contains("parent_message_id: activeThreadRootId"));
        assert!(INDEX_HTML.contains("function openThread(messageId)"));
        assert!(INDEX_HTML.contains("id=\"thread-emoji-picker\""));
        assert!(INDEX_HTML.contains("#thread-emoji-options [data-emoji]"));
        assert!(INDEX_HTML.contains("insertEmoji(threadBody, button.dataset.emoji)"));
        assert!(INDEX_HTML.contains("if (event.key === \"Escape\" && threadEmojiPicker.open)"));
        assert!(INDEX_HTML.contains(
            "threadEmojiPicker.querySelector(\"summary\")?.focus({ preventScroll: true })"
        ));
        assert!(
            INDEX_HTML
                .contains("if (threadPanel.open && !threadEmojiPicker.contains(event.target))")
        );
        assert!(INDEX_HTML.contains("function settleThreadAtBottom()"));
        assert!(INDEX_HTML.contains("threadMessages.scrollTop = threadMessages.scrollHeight"));
        assert!(!INDEX_HTML.contains("sendCommand(\"load_thread\", { root_message_id: messageId });\n        threadBody.focus();"));
        assert!(INDEX_HTML.contains("const threadReplies = new Map()"));
        assert!(INDEX_HTML.contains("const threadDraftPrefix = \"sproyt.thread-draft.v1.\""));
        assert!(INDEX_HTML.contains(
            "function persistThreadDraft(rootId = activeThreadRootId, channelId = activeChannelId)"
        ));
        assert!(INDEX_HTML.contains("function restoreThreadDraft(rootId, channelId)"));
        assert!(INDEX_HTML.contains("clearThreadDraft(pending.rootId, pending.channelId)"));
        assert!(!INDEX_HTML.contains(
            "localStorage.setItem(threadDraftKey(channelId, rootId), JSON.stringify(state.media))"
        ));
        assert!(INDEX_HTML.contains("thread.textContent = replyCount === 0 ? \"🧵\""));
        assert!(INDEX_HTML.contains("footer.append(thread);"));
        assert!(INDEX_HTML.contains("message.parent_message_id"));
        assert!(INDEX_HTML.contains(".thread-panel { width: 100vw"));
        assert!(INDEX_HTML.contains("sendCommand(\"load_thread\""));
        assert!(INDEX_HTML.contains("sendCommand(\"list_thread_summaries\""));
        assert!(INDEX_HTML.contains("sendCommand(\"mark_thread_read\""));
        assert!(INDEX_HTML.contains("event.type === \"thread_loaded\""));
        assert!(INDEX_HTML.contains("summary?.unread_count"));
        assert!(INDEX_HTML.contains("pendingThreadToOpen = mention.message.parent_message_id"));
    }

    #[test]
    fn browser_uses_compact_accessible_mobile_conversation_bar() {
        assert!(INDEX_HTML.contains(
            "<div class=\"mobile-app-mark\"><img src=\"/assets/sproyt-wave.svg\" alt=\"\"></div>"
        ));
        assert!(
            INDEX_HTML.contains("class=\"conversation-circle\" id=\"conversation-circle\" hidden")
        );
        assert!(
            INDEX_HTML
                .contains("class=\"conversation-context\" id=\"conversation-context\" hidden")
        );
        assert!(INDEX_HTML.contains(
            "id=\"connection-status-toggle\" type=\"button\" aria-expanded=\"false\" aria-controls=\"status\""
        ));
        assert!(INDEX_HTML.contains("aria-label=\"Opne menyen\""));
        assert!(INDEX_HTML.contains("grid-template-rows: 52px minmax(0, 1fr) auto;"));
        assert!(INDEX_HTML.contains(
            ".composer-area { position: relative; z-index: 4; grid-column: 2; grid-row: 3; }"
        ));
        assert!(INDEX_HTML.contains(".composer-area { grid-column: 1; grid-row: 3; }"));
        assert!(INDEX_HTML.contains(".sidebar.mobile-open { position: absolute; top: 52px;"));
        assert!(INDEX_HTML.contains(".conversation-header { position: sticky; top: 0;"));
        assert!(INDEX_HTML.contains("grid-template-columns: 32px minmax(0, 1fr) 44px 44px 44px;"));
        assert!(INDEX_HTML.contains("width: 44px; min-width: 44px; min-height: 44px;"));
        assert!(INDEX_HTML.contains("connectionStatusToggle.setAttribute(\"aria-label\", `Sambandsstatus: ${connection.status}`)"));
        assert!(INDEX_HTML.contains("conversationCircle.textContent = channel.circle_id"));
        assert!(
            INDEX_HTML.contains("connectionStatusDot.dataset.reconnecting = String(reconnecting)")
        );
        assert!(INDEX_HTML.contains(".connection-status-dot[data-reconnecting=\"true\"]"));
        assert!(INDEX_HTML.contains("(channel.direct_user_id ? \"Direktemelding\" : \"Felles\")"));
        assert!(INDEX_HTML.contains("sidebar.setAttribute(\"aria-label\", \"Sprøyt-meny\")"));
        assert!(INDEX_HTML.contains("firstControl?.focus()"));
        assert!(
            INDEX_HTML
                .contains("event.key === \"Tab\" && sidebar.classList.contains(\"mobile-open\")")
        );
        assert!(INDEX_HTML.contains("messagesEl.inert = open"));
    }

    #[test]
    fn browser_keeps_conversation_dense_with_accessible_message_actions() {
        assert!(INDEX_HTML.contains(
            ".conversation-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 50px; padding: 6px 12px; }"
        ));
        assert!(INDEX_HTML.contains(".messages {\n        align-content: start;\n        display: grid;\n        gap: 8px;\n        padding: 12px;"));
        assert!(INDEX_HTML.contains("padding: 7px 9px;"));
        assert!(INDEX_HTML.contains(".rendered {\n        display: grid;\n        gap: 7px;"));
        assert!(INDEX_HTML.contains(
            ".message-menu > summary,\n        .message-menu button,\n        .thread-link,\n        .reaction-badge,\n        .reaction-picker summary { min-height: 44px; }"
        ));
        assert!(INDEX_HTML.contains("className = \"message-menu\""));
        assert!(
            INDEX_HTML.contains("function placeMessageMenu(card, footer, menu, thread, messageId)")
        );
        assert!(INDEX_HTML.contains("menu.classList.add(\"footer-menu\")"));
        assert!(INDEX_HTML.contains("footer.insertBefore(menu, thread || null)"));
        assert!(
            INDEX_HTML.contains(".message-menu.footer-menu + .thread-link { margin-left: 0; }")
        );
        assert!(INDEX_HTML.contains("Fleire handlingar for meldinga"));
        assert!(INDEX_HTML.contains("Legg til reaksjon"));
        assert!(INDEX_HTML.contains("message.sender_id === currentParticipantId"));
        assert!(INDEX_HTML.contains("reaction-picker-requested"));
    }

    #[test]
    fn browser_exposes_channel_members_and_owner_managed_markdown_description() {
        assert!(INDEX_HTML.contains("id=\"channel-people\""));
        assert!(INDEX_HTML.contains(".conversation-header .channel-people { order: 3; }"));
        assert!(
            INDEX_HTML
                .contains(".channel-people { width: 36px; min-width: 36px; min-height: 36px;")
        );
        assert!(INDEX_HTML.contains(".channel-people { width: 44px; min-width: 44px;"));
        assert!(INDEX_HTML.contains(".channel-details-dialog > header { display: flex; align-items: center; justify-content: space-between;"));
        assert!(INDEX_HTML.contains(".channel-details-dialog > header button { width: 40px; min-width: 40px; min-height: 40px; padding: 0;"));
        assert!(INDEX_HTML.contains(".channel-details-dialog-body { display: grid; gap: 14px; padding: 14px; overflow-y: auto; }"));
        assert!(INDEX_HTML.contains("id=\"channel-member-search\" type=\"search\""));
        assert!(INDEX_HTML.contains("max-height: min(454px, 45dvh)"));
        assert!(INDEX_HTML.contains("overscroll-behavior: contain"));
        assert!(INDEX_HTML.contains("channelMemberSearch.addEventListener(\"input\""));
        assert!(INDEX_HTML.contains(".normalize(\"NFKD\")"));
        assert!(INDEX_HTML.contains("`Viser ${visibleUsers.length} av ${users.length}`"));
        assert!(INDEX_HTML.contains("function requestChannelMembers(channelId)"));
        assert!(
            INDEX_HTML.contains("sendCommand(\"list_channel_users\", { channel_id: channelId })")
        );
        assert!(INDEX_HTML.contains("showChannelMemberLoadError(channelId"));
        assert!(INDEX_HTML.contains("retry.textContent = \"Prøv igjen\""));
        assert!(INDEX_HTML.contains("requestChannelMembers(channel.id)"));
        assert!(INDEX_HTML.contains("event.type === \"channel_users_listed\""));
        assert!(INDEX_HTML.contains("id=\"channel-member-add\" hidden"));
        assert!(INDEX_HTML.contains("<strong>Legg til i kanalen</strong>"));
        assert!(
            INDEX_HTML
                .contains("id=\"invite-channel-member\" type=\"button\" disabled>Inviter</button>")
        );
        assert!(INDEX_HTML.contains("const pendingChannelInvitationRecipients = new Map()"));
        assert!(INDEX_HTML.contains("pendingDirectInvitationMessages.set(directRequestId"));
        assert!(INDEX_HTML.contains(
            "sendCommand(\"send_message\", { channel_id: channel.id, body: directInvitationMessage })"
        ));
        assert!(INDEX_HTML.contains("`[[invite:${payload.invitation.token}]]`"));
        assert!(INDEX_HTML.contains(
            "channelMemberAdd.hidden = ![\"owner\", \"moderator\"].includes(channel.role)"
        ));
        assert!(INDEX_HTML.contains("function refreshChannelMemberOptions(channelId)"));
        assert!(INDEX_HTML.contains(
            "const eligibleUsers = channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers"
        ));
        assert!(INDEX_HTML.contains(
            "if (channel.circle_id) sendCommand(\"list_circle_users\", { circle_id: channel.circle_id })"
        ));
        assert!(INDEX_HTML.contains("!memberIds.has(user.id)"));
        assert!(INDEX_HTML.contains("channelDetailsDialog.dataset.channelId"));
        assert!(!INDEX_HTML.contains("Bli med i kanal"));
        assert!(!INDEX_HTML.contains("Legg til i vald kanal"));
        assert!(INDEX_HTML.contains("channelDescriptionForm.hidden = channel.role !== \"owner\""));
        assert!(INDEX_HTML.contains("sendCommand(\"update_channel_description\""));
        assert!(INDEX_HTML.contains("renderMarkdown(channel.description, conversationContext)"));
        assert!(INDEX_HTML.contains("maxlength=\"2000\""));
    }

    #[test]
    fn browser_uses_one_complete_theme_contract_for_dark_mode_controls() {
        assert!(INDEX_HTML.contains(
            "<meta name=\"theme-color\" content=\"#111613\" media=\"(prefers-color-scheme: dark)\">"
        ));
        assert!(INDEX_HTML.contains("color-scheme: light dark;"));
        assert!(INDEX_HTML.contains("accent-color: var(--accent);"));
        assert!(
            INDEX_HTML.contains("input,\n      textarea,\n      select {\n        width: 100%;")
        );
        assert!(INDEX_HTML.contains(
            "select option,\n      select optgroup {\n        background-color: var(--control);\n        color: var(--ink);"
        ));
        assert!(INDEX_HTML.contains("@media (prefers-color-scheme: dark)"));
        assert!(INDEX_HTML.contains("--control: #111713;"));
        assert!(INDEX_HTML.contains("--ink: #f2f6f2;"));
        assert!(INDEX_HTML.contains(
            ".bottom-navigation-list button[aria-current=\"page\"] { background: var(--surface-hover); color: var(--ink); }"
        ));
        assert!(INDEX_HTML.contains(
            ".channel-button:hover, .channel-button[aria-current=\"page\"] { background: var(--surface-hover); color: var(--ink); }"
        ));
    }

    #[test]
    fn browser_refreshes_unread_summaries_when_a_background_tab_returns() {
        assert!(INDEX_HTML.contains(
            "if (document.visibilityState !== \"visible\") return;\n        resumeAfterBackground();\n        sendCommand(\"list_my_channels\");"
        ));
    }

    #[test]
    fn browser_linkifies_safe_web_urls_without_expanding_messages() {
        assert!(INDEX_HTML.contains("function appendLinkedText(parent, text)"));
        assert!(INDEX_HTML.contains("const urlPattern = /https?:\\/\\/[^\\s<>]+/gi"));
        assert!(INDEX_HTML.contains("link.rel = \"noopener noreferrer\""));
        assert!(INDEX_HTML.contains("link.referrerPolicy = \"no-referrer\""));
        assert!(INDEX_HTML.contains("function readableLinkLabel(href)"));
        assert!(
            INDEX_HTML.contains(".rendered a { overflow-wrap: anywhere; word-break: break-word; }")
        );
        assert!(INDEX_HTML.contains("min-width: 0;\n        max-width: 100%;"));
    }

    async fn start_test_server(
        repository: Arc<SqliteChatRepository>,
        websocket_idle_timeout: Duration,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let (address, server, _) =
            start_test_server_with_state(repository, websocket_idle_timeout).await;
        (address, server)
    }

    async fn start_test_server_with_state(
        repository: Arc<SqliteChatRepository>,
        websocket_idle_timeout: Duration,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>, AppState) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
        let process_repository: Arc<dyn ProcessRepository> = repository.clone();
        let agent_repository: Arc<dyn AgentRepository> = repository;
        let operations = OperationalState::default();
        operations.set_ready(true);
        let state = AppState {
            auth: AuthService::development(),
            chat: ChatEngine::start(chat_repository),
            operations: operations.clone(),
            processes: ProcessService::start(process_repository, None),
            agents: AgentService::new(agent_repository),
            notifications: NotificationService::test(),
            websocket_idle_timeout,
            advanced_ui_enabled: false,
            agent_ui_enabled: false,
        };
        let app = build_router(state.clone(), operations);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server, state)
    }

    async fn start_postgres_test_server(
        repository: Arc<PostgresChatRepository>,
        websocket_idle_timeout: Duration,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
        let process_repository: Arc<dyn ProcessRepository> = repository.clone();
        let agent_repository: Arc<dyn AgentRepository> = repository;
        let operations = OperationalState::default();
        operations.set_ready(true);
        let state = AppState {
            auth: AuthService::development(),
            chat: ChatEngine::start(chat_repository),
            operations: operations.clone(),
            processes: ProcessService::start(process_repository, None),
            agents: AgentService::new(agent_repository),
            notifications: NotificationService::test(),
            websocket_idle_timeout,
            advanced_ui_enabled: false,
            agent_ui_enabled: false,
        };
        let app = build_router(state, operations);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server)
    }

    async fn start_test_server_with_gateway(
        repository: Arc<SqliteChatRepository>,
        gateway: SharedProcessGateway,
    ) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let chat_repository: Arc<dyn crate::domain::ChatRepository> = repository.clone();
        let process_repository: Arc<dyn ProcessRepository> = repository.clone();
        let agent_repository: Arc<dyn AgentRepository> = repository;
        let operations = OperationalState::default();
        operations.set_ready(true);
        let state = AppState {
            auth: AuthService::development(),
            chat: ChatEngine::start(chat_repository),
            operations: operations.clone(),
            processes: ProcessService::start(process_repository, Some(gateway)),
            agents: AgentService::new(agent_repository),
            notifications: NotificationService::test(),
            websocket_idle_timeout: Duration::from_secs(60),
            advanced_ui_enabled: false,
            agent_ui_enabled: false,
        };
        let app = build_router(state, operations);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (address, server)
    }

    #[derive(Clone)]
    struct RecoverableHeartState {
        available: Arc<AtomicBool>,
        starts: Arc<AtomicUsize>,
        instance_id: uuid::Uuid,
    }

    async fn recoverable_heart_start(
        State(state): State<RecoverableHeartState>,
    ) -> impl IntoResponse {
        state.starts.fetch_add(1, Ordering::SeqCst);
        if !state.available.load(Ordering::SeqCst) {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error":"temporarily unavailable"})),
            );
        }
        (
            axum::http::StatusCode::CREATED,
            Json(serde_json::json!({"instance_id":state.instance_id})),
        )
    }

    async fn recoverable_heart_gateway() -> (SharedProcessGateway, RecoverableHeartState) {
        let state = RecoverableHeartState {
            available: Arc::new(AtomicBool::new(false)),
            starts: Arc::new(AtomicUsize::new(0)),
            instance_id: uuid::Uuid::now_v7(),
        };
        let app = Router::new()
            .route("/api/v1/instances", post(recoverable_heart_start))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let gateway =
            HeartGateway::new(format!("http://{address}"), Duration::from_secs(1), 0).unwrap();
        (Arc::new(gateway), state)
    }

    async fn connect(address: std::net::SocketAddr) -> TestSocket {
        connect_as(address, "capacity-user").await
    }

    async fn connect_as(address: std::net::SocketAddr, participant: &str) -> TestSocket {
        let url = format!("ws://{address}/ws?participant={participant}");
        connect_async(url).await.unwrap().0
    }

    #[tokio::test]
    async fn owner_revokes_agent_and_existing_mcp_credential_immediately_fails() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server, state) =
            start_test_server_with_state(repository, Duration::from_secs(60)).await;
        let owner_principal = state
            .auth
            .authenticate_request(Some("agent-owner".to_owned()), None)
            .await
            .unwrap();
        state.chat.ensure_user(owner_principal.user).await.unwrap();

        let client = reqwest::Client::new();
        let created = client
            .post(format!(
                "http://{address}/api/v1/agents?participant=agent-owner"
            ))
            .json(&serde_json::json!({
                "display_name":"Revocable agent",
                "provider":"contract",
                "service_identity":"revocable-agent",
                "purpose":"revocation route contract",
                "rate_limit_per_minute":60,
                "expires_at":null
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(created.status(), axum::http::StatusCode::CREATED);
        assert_eq!(created.headers()["cache-control"], "no-store");
        let created: serde_json::Value = created.json().await.unwrap();
        let agent_id = created["agent_id"].as_str().unwrap();
        let credential = created["credential"].as_str().unwrap();
        let mcp_request = serde_json::json!({
            "jsonrpc":"2.0",
            "id":"initialize",
            "method":"initialize",
            "params":{"protocolVersion":MCP_PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"revocation-test","version":"1"}}
        });
        let before = client
            .post(format!("http://{address}/mcp"))
            .bearer_auth(credential)
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("accept", "application/json, text/event-stream")
            .json(&mcp_request)
            .send()
            .await
            .unwrap();
        assert_eq!(before.status(), axum::http::StatusCode::OK);

        let revoked = client
            .post(format!(
                "http://{address}/api/v1/agents/{agent_id}/revoke?participant=agent-owner"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(revoked.status(), axum::http::StatusCode::NO_CONTENT);
        let after = client
            .post(format!("http://{address}/mcp"))
            .bearer_auth(credential)
            .header("mcp-protocol-version", MCP_PROTOCOL_VERSION)
            .header("accept", "application/json, text/event-stream")
            .json(&mcp_request)
            .send()
            .await
            .unwrap();
        assert_eq!(after.status(), axum::http::StatusCode::UNAUTHORIZED);
        server.abort();
    }

    #[tokio::test]
    async fn browser_entrypoint_uses_per_response_csp_and_security_headers() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;

        let first = reqwest::get(format!("http://{address}/")).await.unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let headers = first.headers().clone();
        assert_eq!(headers["x-content-type-options"], "nosniff");
        assert_eq!(headers["x-frame-options"], "DENY");
        assert_eq!(headers["referrer-policy"], "no-referrer");
        assert_eq!(headers["cross-origin-opener-policy"], "same-origin");
        assert_eq!(headers["cache-control"], "no-store");
        let policy = headers["content-security-policy"].to_str().unwrap();
        assert!(policy.contains("object-src 'none'"));
        assert!(policy.contains("frame-ancestors 'none'"));
        assert!(policy.contains("script-src 'self' 'nonce-"));
        assert!(policy.contains("worker-src 'self'"));
        let nonce = policy
            .split("script-src 'self' 'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap()
            .to_owned();
        let body = first.text().await.unwrap();
        assert!(body.contains(&format!("<script type=\"module\" nonce=\"{nonce}\">")));
        let client_store_fingerprint =
            client_store_fingerprint(BUILD_REVISION, CLIENT_STORE.as_bytes());
        assert!(
            body.find(&format!(
                "import {{ createApplicationStore, createServerEventMailbox }} from \"/assets/client-store/{client_store_fingerprint}/client-store.js\";"
            ))
                .unwrap()
            < body
                    .find("function syncAppViewportHeight() {")
                    .unwrap()
        );
        assert!(body.contains(&format!("<style nonce=\"{nonce}\">")));
        assert!(
            body.contains("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs")
        );
        assert!(!body.contains("import mermaid from"));
        assert!(body.contains("mermaidPromise = import("));
        assert!(!body.contains("npm/mermaid@11/dist/"));
        assert!(!body.contains("{{NONCE}}"));
        assert!(!body.contains("{{CLIENT_STORE_URL}}"));
        assert!(!body.contains("{{DISPLAY_NAME}}"));
        assert!(!body.contains("{{AGENT_HIDDEN}}"));
        assert!(body.contains("Innlogga som <strong>guest</strong>"));
        assert!(!body.contains("id=\"participant\""));
        assert!(
            body.contains(
                "const websocketUrl = new URL(`${protocol}://${window.location.host}/ws`)"
            )
        );
        assert!(body.contains("const nextSocket = new WebSocket(websocketUrl)"));
        assert!(!body.contains("let subscribedChannelId = null"));
        assert!(
            body.contains("connectionSupervisor.state.subscribedChannelId === activeChannelId")
        );
        assert!(
            body.contains("channel.id === activeChannelId && channel.id === connectionSupervisor.state.subscribedChannelId")
        );
        assert!(body.contains("payload.channel_id !== activeChannelId"));
        assert!(body.contains("const pendingMessages = new Map()"));

        let service_worker = reqwest::get(format!("http://{address}/service-worker.js"))
            .await
            .unwrap();
        assert_eq!(service_worker.status(), reqwest::StatusCode::OK);
        assert_eq!(service_worker.headers()["cache-control"], "no-cache");
        let worker_policy = service_worker.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(worker_policy.contains("default-src 'none'"));
        assert!(worker_policy.contains("connect-src 'self'"));

        let client_store_url = format!(
            "http://{address}/assets/client-store/{client_store_fingerprint}/client-store.js"
        );
        let client_store = reqwest::get(&client_store_url).await.unwrap();
        assert_eq!(client_store.status(), reqwest::StatusCode::OK);
        assert_eq!(
            client_store.headers()["content-type"],
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            client_store.headers()["cache-control"],
            "public, max-age=31536000, immutable"
        );
        let client_store_body = client_store.text().await.unwrap();
        assert!(client_store_body.contains("export function createApplicationStore()"));
        assert!(
            client_store_body
                .contains("export function createServerEventMailbox({ reduce, deliver })")
        );
        let legacy_client_store = reqwest::get(format!("http://{address}/assets/client-store.js"))
            .await
            .unwrap();
        assert_eq!(legacy_client_store.status(), reqwest::StatusCode::OK);
        assert_eq!(legacy_client_store.headers()["cache-control"], "no-cache");
        assert_eq!(
            legacy_client_store.headers()["content-type"],
            "text/javascript; charset=utf-8"
        );
        let stale_client_store = reqwest::get(format!(
            "http://{address}/assets/client-store/stale-revision/client-store.js"
        ))
        .await
        .unwrap();
        assert_eq!(stale_client_store.status(), reqwest::StatusCode::NOT_FOUND);
        assert!(!body.contains("id=\"channel-kind\""));
        assert!(!body.contains("id=\"create-circle-channel\""));
        assert!(!body.contains("id=\"create-channel-invitation\""));
        assert!(body.contains("id=\"circle-joinable-list\""));
        assert!(!body.contains("id=\"joinable-channel\""));
        assert!(body.contains("id=\"add-channel-member\""));
        assert!(body.contains("function scopedCircleChannelSlug(circleId, value)"));
        assert!(body.contains("scopedCircleChannelSlug(managedCircleId, name)"));
        assert!(body.contains("scopedCircleChannelSlug(payload.circle.id, \"prat\")"));
        assert!(body.contains(
            "knownCircles.set(payload.circle.id, { ...payload.circle, role: \"owner\" })"
        ));
        assert!(body.contains("const activeCircleKey = \"sproyt.active-circle.v1\""));
        assert!(body.contains("function restoreActiveCircle()"));
        assert!(body.contains(".find((circleId) => circleId && knownCircles.has(circleId))"));
        assert!(body.contains(
            "try { window.localStorage.setItem(activeCircleKey, activeCircleId); } catch (_) {}"
        ));
        assert!(body.contains("const restoredCircle = restoreActiveCircle();"));
        assert!(body.contains("if (restoredCircle) sendCommand(\"list_joinable_channels\", { circle_id: restoredCircle });"));
        assert!(body.contains("clearActiveCircle(deletedCircleId);"));
        assert!(body.contains("clearActiveCircle(departedCircleId);"));
        assert!(body.contains(
            "if (channel.circle_id) {\n          rememberCircleChannel(channel);\n          setActiveCircle(channel.circle_id);\n          circleSelect.value = channel.circle_id;\n        }"
        ));
        assert!(body.contains("Kanalen kunne ikkje opprettast."));
        assert!(body.contains("sendCommand(\"list_joinable_channels\""));
        assert!(body.contains("sendCommand(\"add_channel_member\""));
        assert!(body.contains("const browserSessionId = `browser-${crypto.randomUUID()}`"));
        assert!(body.contains("request_id: `${browserSessionId}-${requestNumber}`"));
        assert!(body.contains(
            "if (type === \"list_my_channels\") latestChannelListRequestId = command.request_id;"
        ));
        assert!(body.contains("if (event.request_id !== latestChannelListRequestId) return;"));
        assert!(body.contains("if (event.request_id !== latestCircleListRequestId) return;"));
        assert!(!body.contains("request_id: `browser-${requestNumber}`"));
        assert!(body.contains("if (event.type === \"message_accepted\")"));
        assert!(body.contains("finishPendingMessage(event.request_id, payload.message)"));
        assert!(body.contains("message?.channel_id !== pending.channelId"));
        assert!(body.contains("message?.body !== pending.body"));
        assert!(body.contains("failPendingMessage(event.request_id"));
        assert!(body.contains(
            "pendingMessages.set(requestId, { body, draft, mediaIds: channelMedia.map((media) => media.id), channelId: activeChannelId });\n        bodyInput.value = \"\";"
        ));
        assert!(body.contains("bodyInput.value = pending.draft"));
        assert!(body.contains("const channelDraftPrefix = \"sproyt.channel-draft.v1.\""));
        assert!(body.contains("function persistActiveDraft()"));
        assert!(body.contains("function restoreActiveDraft()"));
        assert!(body.contains("window.localStorage.setItem(key, bodyInput.value)"));
        assert!(body.contains(
            "if (channel.id === activeChannelId && channel.id === connectionSupervisor.state.subscribedChannelId) return;\n        persistActiveDraft();"
        ));
        assert!(body.contains("activeChannelId = channel.id;\n        restoreActiveDraft();"));
        assert!(body.contains("class=\"advanced-tools\" hidden"));
        assert!(body.contains("<details class=\"agent-access\" hidden>"));
        assert!(body.contains(
            "<summary data-tooltip=\"Agenttilgang\" title=\"Agenttilgang\">Agenttilgang</summary>"
        ));
        assert!(body.contains("id=\"create-agent-access\""));
        assert!(body.contains("function createTemporaryAgentAccess()"));
        assert!(body.contains("[\"read_history\", \"send_messages\"]"));
        assert!(body.contains("function revokeTemporaryAgentAccess()"));
        assert!(body.contains("channel?.role === \"owner\" || channel?.role === \"moderator\""));
        assert!(body.contains(
            "agentAccessNotice.textContent = \"Klar til å lage kortliva agenttilgang for denne samtalen.\""
        ));
        assert!(body.contains(
            "updateAgentAccessControls();\n          agentAccessNotice.textContent = \"Agenttilgangen er trekt tilbake.\";"
        ));
        assert!(body.contains("connect();"));
        assert!(body.contains("function scheduleReconnect(closeCode"));
        assert!(
            body.contains("connectionSupervisor.state.stableConnectionTimer = window.setTimeout")
        );
        assert!(body.contains("event.code === 1008"));
        assert!(body.contains("recoverAuthentication().catch"));
        assert!(body.contains("async function recoverConnection(replaceOpenSocket = false)"));
        assert!(body.contains("response.status === 401"));
        assert!(body.contains("connect(true, currentSocket)"));
        assert!(body.contains("recoverConnection().catch(() => scheduleReconnect"));
        assert!(body.contains("fetch(\"/auth/session\""));
        assert!(body.contains("scheduleInitialSessionRefresh()"));
        assert!(body.contains("sessionSupervisor.state.refreshDueAt = Date.now() + delay"));
        assert!(body.contains("window.addEventListener(\"pageshow\", resumeAfterBackground)"));
        assert!(body.contains("window.addEventListener(\"online\", resumeAfterBackground)"));
        assert!(body.contains("reconnectAfterSessionRefresh()"));
        assert!(body.contains("setConnectionStatus(\"Fornyar økta …\")"));
        assert!(body.contains("let lastUserActivityAt = Date.now()"));
        assert!(body.contains("function noteUserActivity()"));
        assert!(body.contains("window.addEventListener(\"pointerdown\", noteUserActivity"));
        assert!(body.contains("if (await useCurrentSessionIfAnotherTabRenewed())"));
        assert!(body.contains("Date.now() - lastUserActivityAt < 120_000"));
        assert!(body.contains("vi ventar så du ikkje mistar arbeidet ditt"));
        assert!(body.contains("connect(true)"));
        assert!(body.contains("connect(true, currentSocket)"));
        assert!(body.contains(
            "const next = invited || current || restored || requested || knownChannels[0]"
        ));
        assert!(body.contains("[[invite:${payload.invitation.token}]]"));
        assert!(body.contains("function renderInvitationCard(token, target)"));
        assert!(body.contains("const invitationInspectionCache = new Map()"));
        assert!(
            body.contains("if (cached?.status === \"pending\" || cached?.status === \"missing\")")
        );
        assert!(body.contains("pendingInvitationInspections.set(requestId, token)"));
        assert!(body.contains("if (requestedCommand === \"inspect_invitation\")"));
        assert!(body.contains("showInvitationError(inspectedInvitationToken, message)"));
        assert!(body.contains(
            "respondToInvitation(token, \"accept_invitation\", \"Godtek invitasjonen …\")"
        ));
        assert!(!body.contains("sendCommand(\"accept_circle_invitation\", { token })"));
        assert!(body.contains(
            "respondToInvitation(token, \"decline_invitation\", \"Avviser invitasjonen …\")"
        ));
        assert!(
            body.contains("const authoredByMe = invitation.invited_by === currentParticipantId")
        );
        assert!(body.contains("Du må først vere medlem i vennekretsen"));
        assert!(body.contains("window.addEventListener(\"focus\", refreshVisibleInvitationCards)"));
        assert!(body.contains("historyHasMore = false;\n            console.error(\"Kunne ikkje laste eldre meldingar\""));
        assert!(body.contains("window.localStorage.setItem(activeConversationKey, channel.id)"));
        assert!(body.contains("let reconnectScrollOffset = null"));
        assert!(body.contains("restoreConversationScrollOffset(scrollOffset)"));
        assert!(body.contains("previousSocket.close(4000, \"session refreshed\")"));
        assert!(!body.contains("sessionRefreshReconnect"));
        assert!(!body.contains("if (response.status === 401) {\n          window.location.assign"));
        assert!(!body.contains("window.location.reload()"));
        assert!(body.contains("Fråkopla (${detail})"));
        assert!(body.contains("function acknowledgeLatest(channelId, messages)"));
        assert!(body.contains("function loadOlderHistory()"));
        assert!(body.contains("before: oldest.sequence"));
        assert!(body.contains("renderTimeline({ preserveScroll: true })"));
        assert!(body.contains(
            "renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 })"
        ));
        assert!(body.contains("function settleConversationAtBottom()"));
        assert!(!body.contains("sendForm.scrollIntoView"));
        assert!(body.contains("const offsetTop = viewport?.offsetTop || 0"));
        assert!(
            body.contains(
                "window.visualViewport?.addEventListener(\"scroll\", syncAppViewportHeight"
            )
        );
        assert!(body.contains("transform: translateY(var(--app-offset-top))"));
        assert!(body.contains("function formatMessageTimestamp(sentAt, now = new Date())"));
        assert!(body.contains("dateStyle: \"full\", timeStyle: \"short\""));
        assert!(body.contains("appendProfileStatus(senderLabel, message.sender_id)"));
        assert!(body.contains("channel.direct_user_id"));
        assert!(body.contains("function approximateUnreadCount(count)"));
        assert!(body.contains("if (count < 50) return \"25+\""));
        assert!(body.contains("if (count < 100) return \"50+\""));
        assert!(body.contains("button.classList.add(\"has-unread\")"));
        assert!(body.contains("if (unreadCount > 0) {"));
        assert!(body.contains("class=\"inbox-navigation\""));
        assert!(body.contains("id=\"unread-count\""));
        assert!(body.contains("id=\"mention-count\""));
        assert!(body.contains("id=\"task-count\""));
        assert!(body.contains("activeInboxKind = kind"));
        assert!(body.contains("className = \"unread-inbox\""));
        assert!(body.contains("className = \"unread-card\""));
        assert!(body.contains("function openChannelManagement(circleId)"));
        assert!(!body.contains("Samtalar og vennekretsar"));
        assert!(!body.contains("id=\"channel-list\""));
        assert!(body.contains("leave.textContent = `Forlat # ${activeChannel.name}`"));
        assert!(body.contains("sendCommand(\"leave_channel\""));
        assert!(body.contains("event.type === \"membership_left\""));
        assert!(body.contains("activeChannel.name.trim().toLocaleLowerCase() !== \"prat\""));
        assert!(body.contains("id=\"circle-channel-dialog\""));
        assert!(body.contains("function renderManagedJoinableChannels(channels)"));
        assert!(body.contains("+ Finn fleire kanalar"));
        assert!(body.contains("className = \"joinable-channel-description\""));
        assert!(body.contains("renderMarkdown(channel.description, description)"));
        assert!(body.contains("sendCommand(\"leave_circle\""));
        assert!(body.contains("event.type === \"circle_left\""));
        assert!(body.contains("circle.role === \"owner\""));
        assert!(body.contains("document.addEventListener(\"visibilitychange\""));
        assert!(body.contains(":focus-visible"));
        assert!(body.contains("id=\"mobile-navigation-toggle\""));
        assert!(body.contains("aria-controls=\"mobile-navigation\""));
        assert!(body.contains(
            "id=\"view-mode-toggle\" type=\"button\" role=\"switch\" aria-checked=\"false\""
        ));
        assert!(body.contains("class=\"view-mode-switch-icon\" aria-hidden=\"true\"><svg"));
        assert!(body.contains("setRenderMode(renderMode === \"raw\" ? \"view\" : \"raw\")"));
        assert!(
            body.contains("viewModeToggle.setAttribute(\"aria-checked\", String(showsSource))")
        );
        assert!(body.contains(".conversation-header .view-controls { display: none; }"));
        assert!(!body.contains("id=\"view-mode\""));
        assert!(!body.contains("id=\"raw-mode\""));
        assert!(body.contains("class=\"bottom-navigation\" aria-label=\"Område- og kanalveljar\""));
        assert!(body.contains("</form>\n        <nav class=\"bottom-navigation\""));
        assert!(body.contains("id=\"bottom-channel-panel\""));
        assert!(body.contains("id=\"bottom-circle-panel\""));
        assert!(body.contains(".bottom-navigation-panel { position: relative; min-width: 0; }"));
        assert!(body.contains("bottom: calc(100% + 5px);"));
        assert!(body.contains("if (bottomNavigation.contains(event.target)) return;"));
        assert!(
            body.contains(
                "bottomChannelPanel.open = false;\n        bottomCirclePanel.open = false;"
            )
        );
        assert!(body.contains("function pendingMessageToReveal(message, requestId = null)"));
        assert!(body.contains("message.sender_id !== currentParticipantId"));
        assert!(body.contains(
            "renderTimeline({ revealMessageId: revealOwnMessage ? payload.message.id : null })"
        ));
        assert!(body.contains(
            "renderTimeline({ revealMessageId: revealOwnMessage ? chatEvent.message.id : null })"
        ));
        assert!(body.contains("function revealTimelineMessage(messageId)"));
        assert!(body.contains("const cardRect = card.getBoundingClientRect()"));
        assert!(body.contains("const viewportRect = messagesEl.getBoundingClientRect()"));
        assert!(body.contains("if (delta > 0) messagesEl.scrollTop += delta"));
        assert!(body.contains(
            "aria-label=\"Vel kanal\"><span class=\"bottom-navigation-label\"># Kanal</span>"
        ));
        assert!(body.contains(
            "aria-label=\"Vel område\"><span class=\"bottom-navigation-label\">◎ Felles</span>"
        ));
        assert!(body.contains("height: 40px;\n        min-height: 40px"));
        assert!(body.contains("function renderBottomNavigation()"));
        assert!(body.contains("const channelLabel = activeChannel"));
        assert!(body.contains("bottomCircleToggle.querySelector(\".bottom-navigation-label\").textContent = `◎ ${circleLabel}`"));
        assert!(body.contains(".message-menu:not([open]) > div { display: none; }"));
        assert!(body.contains(
            ".message .reaction-picker[open] { visibility: visible; pointer-events: auto; }"
        ));
        assert!(body.contains("card.classList.add(\"reaction-picker-requested\")"));
        assert!(body.contains("const scopedChannels = activeCircleId"));
        assert!(!body.contains("sharedButton.textContent = \"(Felles)\""));
        assert!(!body.contains("directButton.textContent = \"(Direkte)\""));
        assert!(body.contains("class=\"circle-tool-rail\" role=\"toolbar\""));
        assert!(body.contains(".circle-tool-rail { display: grid; justify-items: center;"));
        assert!(body.contains("font-size: 1.22rem; line-height: 1; text-align: center;"));
        assert!(body.contains("content: attr(aria-label)"));
        assert!(body.contains(
            ".circle-tool-button:hover::before, .circle-tool-button:focus-visible::before"
        ));
        assert!(!body.contains("id=\"circle-tool-circles\""));
        let shared_tool = body.find("id=\"circle-tool-shared\"").unwrap();
        let direct_tool = body.find("id=\"circle-tool-direct\"").unwrap();
        let settings_tool = body.find("id=\"circle-tool-settings\"").unwrap();
        assert!(shared_tool < direct_tool && direct_tool < settings_tool);
        assert!(body.contains("aria-label=\"Direkte samtalar\""));
        assert!(body.contains("aria-label=\"Felles\""));
        assert!(body.contains("id=\"circle-tool-settings\""));
        assert!(!body.contains("circleToolMode"));
        assert!(!body.contains("function setCircleToolMode(mode)"));
        assert!(body.contains("function activateRootScope(scope)"));
        assert!(body.contains(
            "circleToolDirect.addEventListener(\"click\", () => activateRootScope(\"direct\"))"
        ));
        assert!(body.contains(
            "circleToolShared.addEventListener(\"click\", () => activateRootScope(\"shared\"))"
        ));
        assert!(body.contains(
            "circleToolShared.setAttribute(\"aria-pressed\", String(!activeCircleId && activeRootScope === \"shared\"))"
        ));
        assert!(body.contains("if (circleAdminDialog.open) return"));
        assert!(body.contains("id=\"circle-admin-dialog\""));
        assert!(body.contains("if (!circleAdminDialog.open) circleAdminDialog.showModal()"));
        assert!(body.contains(
            "const directChannels = knownChannels.filter((channel) => channel.direct_user_id)"
        ));
        assert!(!body.contains("const primaryChannels = knownChannels.filter"));
        assert!(
            body.contains("const circleChannelHistoryKey = \"sproyt.active-channel-by-circle.v1\"")
        );
        assert!(body.contains("function rememberCircleChannel(channel)"));
        assert!(
            body.contains("function preferredCircleChannel(circleId, channels = knownChannels)")
        );
        assert!(body.contains("const remembered = available.find"));
        assert!(body.contains("channel.name.trim().toLocaleLowerCase() === \"prat\""));
        assert!(body.contains("return remembered || primary || available[0] || null"));
        assert!(
            body.contains("const preferredChannel = preferredCircleChannel(circleId, channels)")
        );
        assert!(body.contains("if (preferredChannel) selectChannel(preferredChannel)"));
        assert!(body.contains("rememberCircleChannel(channel)"));
        assert!(body.contains("forgetCircleChannel(departedCircleId)"));
        assert!(
            body.contains("activeRootScope = channel.direct_user_id ? \"direct\" : \"shared\"")
        );
        assert!(body.contains("id=\"direct-message-dialog\""));
        assert!(body.contains("id=\"direct-message-status\" role=\"status\" aria-live=\"polite\""));
        assert!(body.contains("startDirect.textContent = \"+ Ny samtale …\""));
        assert!(body.contains("function openDirectMessageDialog()"));
        assert!(body.contains("directMessageStatus.textContent = \"Hentar fersk personliste …\""));
        assert!(body.contains("if (!sendCommand(\"list_users\"))"));
        assert!(body.contains("if (requestedCommand === \"open_direct_channel\")"));
        assert!(body.contains("Brukaren finst ikkje lenger. Lukk dialogen og prøv på nytt."));
        assert!(body.contains("activeProfile(channel?.direct_user_id)?.display_name"));
        assert!(body.contains("if (knownChannels.length > 0) renderChannels()"));
        assert!(!body.contains("heading.textContent = \"Andre samtalar\""));
        assert!(body.contains("button.setAttribute(\"aria-current\", circleId === activeCircleId ? \"page\" : \"false\")"));
        assert!(body.contains("button.classList.add(\"has-unread\")"));
        assert!(body.contains("function closeBottomNavigation(panel, toggle)"));
        assert!(body.contains("if (event.key === \"Escape\" && bottomChannelPanel.open)"));
        assert!(body.contains("padding-bottom: 6px;"));
        assert!(!body.contains("<summary>Administrer kretsar</summary>"));
        assert!(body.contains("<h2 id=\"circle-admin-title\">Administrer vennekretsar</h2>"));
        assert!(body.contains(".sidebar.mobile-open nav, .sidebar.mobile-open .agent-access"));
        assert!(body.contains(".sidebar.mobile-open .identity { display: grid;"));
        assert!(body.contains(".sidebar.mobile-open { position: absolute; top: 52px;"));
        assert!(body.contains("overflow-y: auto; overscroll-behavior: contain;"));
        assert!(body.contains("grid-template-rows: 52px minmax(0, 1fr) auto;"));
        assert!(body.contains("form.send { grid-template-columns: minmax(0, 1fr) auto"));
        assert!(body.contains(".connection-status-toggle[aria-expanded=\"true\"] + .status"));
        assert!(body.contains("setConnectionStatus(\"Tilkopla\")"));
        assert!(
            body.contains("mobileNavigationToggle.setAttribute(\"aria-expanded\", String(open))")
        );
        assert!(body.contains("event.key === \"Escape\""));
        assert!(body.contains("message.sender_display_name || \"Ein ven\""));
        assert!(body.contains("Invitasjonslenkje"));
        assert!(body.contains("Invitasjonen finst ikkje eller er ikkje gyldig lenger"));

        let second = reqwest::get(format!("http://{address}/")).await.unwrap();
        let second_policy = second.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(!second_policy.contains(&format!("'nonce-{nonce}'")));
        server.abort();
    }

    #[tokio::test]
    async fn authenticated_client_events_are_bounded_and_exported_without_payload_data() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
        let client = reqwest::Client::new();

        let accepted = client
            .post(format!(
                "http://{address}/api/v1/client-events?participant=telemetry-user"
            ))
            .json(&serde_json::json!({"event":"session_refresh_failed"}))
            .send()
            .await
            .unwrap();
        assert_eq!(accepted.status(), axum::http::StatusCode::NO_CONTENT);

        let rejected = client
            .post(format!(
                "http://{address}/api/v1/client-events?participant=telemetry-user"
            ))
            .json(&serde_json::json!({
                "event":"arbitrary_event",
                "message":"private text must never become a metric"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            rejected.status(),
            axum::http::StatusCode::UNPROCESSABLE_ENTITY
        );

        let metrics = client
            .get(format!("http://{address}/metrics"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(metrics.contains("sproyt_client_events_total{event=\"session_refresh_failed\"} 1"));
        assert!(!metrics.contains("private text"));
        assert!(!metrics.contains("telemetry-user"));
        server.abort();
    }

    #[tokio::test]
    async fn websocket_upgrade_is_not_decorated_as_a_document_response() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;

        let url = format!("ws://{address}/ws?participant=upgrade-policy-user");
        let (mut socket, response) = connect_async(url).await.unwrap();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::SWITCHING_PROTOCOLS
        );
        assert!(!response.headers().contains_key("content-security-policy"));
        assert!(
            !response
                .headers()
                .contains_key("cross-origin-opener-policy")
        );

        socket
            .send(ClientMessage::Text(
                serde_json::json!({
                    "protocol":"sproyt.chat.v1",
                    "request_id":"upgrade-hello",
                    "type":"hello"
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();
        let response = tokio::time::timeout(Duration::from_secs(2), socket.next())
            .await
            .expect("hello response timed out")
            .expect("socket closed after upgrade")
            .unwrap();
        assert!(response.into_text().unwrap().contains("\"type\":\"hello\""));
        socket.close(None).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn security_headers_cover_operational_oidc_and_not_found_responses() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();

        for path in ["/healthz", "/versionz", "/auth/login", "/does-not-exist"] {
            let response = client
                .get(format!("http://{address}{path}"))
                .send()
                .await
                .unwrap();
            let headers = response.headers();
            assert_eq!(headers["x-content-type-options"], "nosniff", "{path}");
            assert_eq!(headers["x-frame-options"], "DENY", "{path}");
            assert_eq!(headers["referrer-policy"], "no-referrer", "{path}");
            assert_eq!(
                headers["cross-origin-opener-policy"], "same-origin",
                "{path}"
            );
            assert_eq!(headers["cache-control"], "no-store", "{path}");
            assert!(
                headers["content-security-policy"]
                    .to_str()
                    .unwrap()
                    .contains("default-src 'none'"),
                "{path}"
            );
        }
        let version: serde_json::Value = client
            .get(format!("http://{address}/versionz"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(version["service"], "sproyt");
        assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(version["revision"], BUILD_REVISION);
        server.abort();
    }

    #[test]
    fn invitation_return_path_accepts_only_bounded_url_safe_tokens() {
        assert!(is_safe_invitation_token(
            "WGp_FxqwngrypwMMIvAh1CMLGC0OTkIY-FIwjPElISU"
        ));
        assert!(!is_safe_invitation_token("short"));
        assert!(!is_safe_invitation_token(
            "valid-length-but-has-a-query&next=https://evil.invalid"
        ));
        assert!(!is_safe_invitation_token(&"a".repeat(513)));
    }

    #[tokio::test]
    async fn portable_export_is_private_complete_and_not_cacheable() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server, _) =
            start_test_server_with_state(repository, Duration::from_secs(60)).await;

        let mut owner = connect_as(address, "export-owner").await;
        let channel = command(
            &mut owner,
            "export-channel",
            "create_channel",
            serde_json::json!({"slug":"export-visible","name":"Export visible","kind":"private"}),
        )
        .await;
        let channel_id = channel["payload"]["channel"]["id"].clone();
        command(
            &mut owner,
            "export-message",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"portable visible body"}),
        )
        .await;

        let mut outsider = connect_as(address, "export-outsider").await;
        let hidden = command(
            &mut outsider,
            "hidden-channel",
            "create_channel",
            serde_json::json!({"slug":"export-hidden","name":"Export hidden","kind":"private"}),
        )
        .await;
        command(
            &mut outsider,
            "hidden-message",
            "send_message",
            serde_json::json!({"channel_id":hidden["payload"]["channel"]["id"],"body":"must not leak"}),
        )
        .await;

        let response = reqwest::get(format!(
            "http://{address}/api/v1/me/export?participant=export-owner"
        ))
        .await
        .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(
            response.headers()["content-disposition"]
                .to_str()
                .unwrap()
                .starts_with("attachment; filename=\"sproyt-export-")
        );
        let export: serde_json::Value = response.json().await.unwrap();
        assert_eq!(export["format"], crate::domain::PORTABLE_USER_EXPORT_FORMAT);
        let exported_channel = export["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["channel"]["id"] == channel_id)
            .expect("the user's private channel must be exported alongside general");
        assert_eq!(
            exported_channel["messages"][0]["body"],
            "portable visible body"
        );
        assert!(!export.to_string().contains("must not leak"));
        server.abort();
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

    async fn wait_for_chat_body(socket: &mut TestSocket, expected_body: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let frame = socket
                    .next()
                    .await
                    .expect("server closed before cross-replica chat event")
                    .unwrap();
                if let ClientMessage::Text(text) = frame {
                    let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "chat"
                        && event["payload"]["event"]["type"] == "message_accepted"
                        && event["payload"]["event"]["message"]["body"] == expected_body
                    {
                        return;
                    }
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("cross-replica message {expected_body:?} was not delivered"));
    }

    async fn mcp_tool(
        state: &AppState,
        headers: &HeaderMap,
        id: &str,
        name: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        let response = mcp_handler(
            State(state.clone()),
            headers.clone(),
            Json(McpRequest {
                jsonrpc: "2.0".to_owned(),
                id: serde_json::json!(id),
                method: "tools/call".to_owned(),
                params: serde_json::json!({"name":name,"arguments":arguments}),
            }),
        )
        .await;
        let body = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let response: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(response.get("result").is_some(), "{response}");
        response["result"]["structuredContent"].clone()
    }

    #[tokio::test]
    async fn websocket_and_mcp_adapters_have_identical_chat_outcomes() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server, state) =
            start_test_server_with_state(repository, Duration::from_secs(60)).await;
        let mut browser = connect_as(address, "adapter-owner").await;
        let created = command(
            &mut browser,
            "adapter-create",
            "create_channel",
            serde_json::json!({"slug":"adapter-contract","name":"Adapter contract","kind":"private","circle_id":null}),
        )
        .await;
        let channel_id = created["payload"]["channel"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let owner = state
            .auth
            .authenticate_development(Some("adapter-owner".to_owned()))
            .unwrap()
            .user
            .id;
        let agent = state
            .agents
            .create(CreateAgent {
                actor: owner.clone(),
                owner_id: owner.clone(),
                display_name: "Adapter agent".to_owned(),
                provider: "contract".to_owned(),
                service_identity: "adapter-agent".to_owned(),
                purpose: "Adapter conformance".to_owned(),
                rate_limit_per_minute: 60,
                expires_at: None,
            })
            .await
            .unwrap();
        for scope in [AgentScope::ReadHistory, AgentScope::SendMessages] {
            state
                .agents
                .grant(GrantAgent {
                    actor: owner.clone(),
                    agent_id: agent.agent_id.clone(),
                    circle_id: None,
                    channel_id: Some(ChannelId::new(channel_id.clone()).unwrap()),
                    scope,
                    expires_at: None,
                })
                .await
                .unwrap();
        }
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", agent.credential)).unwrap(),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/json, text/event-stream"),
        );
        headers.insert(
            HeaderName::from_static("mcp-protocol-version"),
            HeaderValue::from_static(MCP_PROTOCOL_VERSION),
        );

        let browser_channels = command(
            &mut browser,
            "browser-list",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        let agent_channels = mcp_tool(
            &state,
            &headers,
            "agent-list",
            "list_channels",
            serde_json::json!({}),
        )
        .await;
        assert_eq!(browser_channels["payload"]["channels"][0]["id"], channel_id);
        assert_eq!(agent_channels[0]["id"], channel_id);

        let browser_message = command(
            &mut browser,
            "browser-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from browser"}),
        )
        .await;
        let browser_replay = command(
            &mut browser,
            "browser-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from browser"}),
        )
        .await;
        assert_eq!(
            browser_message["payload"]["message"]["id"],
            browser_replay["payload"]["message"]["id"]
        );
        let agent_message = mcp_tool(
            &state,
            &headers,
            "agent-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from agent","request_id":"agent-domain-send"}),
        )
        .await;
        let agent_replay = mcp_tool(
            &state,
            &headers,
            "agent-send-replay",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"from agent","request_id":"agent-domain-send"}),
        )
        .await;
        assert_eq!(
            agent_message["message"]["id"],
            agent_replay["message"]["id"]
        );

        let browser_history = command(
            &mut browser,
            "browser-read",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":50,"after":0}),
        )
        .await;
        let agent_history = mcp_tool(
            &state,
            &headers,
            "agent-read",
            "read_messages",
            serde_json::json!({"channel_id":channel_id,"limit":50,"after_sequence":0}),
        )
        .await;
        assert_eq!(browser_history["payload"]["messages"], agent_history);
        assert_eq!(agent_history.as_array().unwrap().len(), 2);

        let browser_read = command(
            &mut browser,
            "browser-mark-read",
            "mark_read",
            serde_json::json!({"channel_id":channel_id,"sequence":2}),
        )
        .await;
        let agent_read = mcp_tool(
            &state,
            &headers,
            "agent-mark-read",
            "mark_read",
            serde_json::json!({"channel_id":channel_id,"sequence":2}),
        )
        .await;
        assert_eq!(
            browser_read["payload"]["membership"]["last_read_sequence"],
            agent_read["last_read_sequence"]
        );
        assert_eq!(agent_read["last_read_sequence"], 2);

        browser.close(None).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn browser_process_pilot_exposes_durable_status_and_idempotent_inspect() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) =
            start_test_server(repository.clone(), Duration::from_secs(60)).await;
        let mut owner = connect_as(address, "process-browser-owner").await;
        let circle = command(
            &mut owner,
            "process-circle",
            "create_circle",
            serde_json::json!({"slug":"process-circle","name":"Process circle"}),
        )
        .await;
        let circle_id = circle["payload"]["circle"]["id"].as_str().unwrap();
        let channel = command(
            &mut owner,
            "process-channel",
            "create_channel",
            serde_json::json!({"slug":"process-channel","name":"Process channel","kind":"private","circle_id":circle_id}),
        )
        .await;
        let channel_id = channel["payload"]["channel"]["id"].as_str().unwrap();
        let client = reqwest::Client::new();
        let base = format!("http://{address}");
        let feature = client
            .post(format!("{base}/api/v1/circles/{circle_id}/features/heart-event-planning?participant=process-browser-owner"))
            .json(&serde_json::json!({"enabled":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(feature.status(), reqwest::StatusCode::NO_CONTENT);
        let started = client
            .post(format!(
                "{base}/api/v1/processes?participant=process-browser-owner"
            ))
            .json(&serde_json::json!({
                "channel_id":channel_id,
                "request_id":"browser-process-start",
                "namespace":"sproyt",
                "definition_name":"event-planning",
                "definition_version":"1",
                "metadata":{"title":"Dinner"}
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(started.status(), reqwest::StatusCode::ACCEPTED);
        let started: serde_json::Value = started.json().await.unwrap();
        let process_id = started["process_link_id"].as_str().unwrap();
        let job = repository
            .lease_next(Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        repository
            .complete_start(
                job,
                StartedProcess {
                    instance_id: uuid::Uuid::now_v7(),
                },
            )
            .await
            .unwrap();

        let view = client
            .get(format!(
                "{base}/api/v1/processes/{process_id}?participant=process-browser-owner"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(view.status(), reqwest::StatusCode::OK);
        let view: serde_json::Value = view.json().await.unwrap();
        assert_eq!(view["process"]["status"], "active");
        assert_eq!(view["events"][0]["event_type"], "process.started");

        let inspect_url = format!(
            "{base}/api/v1/processes/{process_id}/inspect?participant=process-browser-owner"
        );
        let inspect = client
            .post(&inspect_url)
            .json(&serde_json::json!({"request_id":"browser-inspect"}))
            .send()
            .await
            .unwrap();
        assert_eq!(inspect.status(), reqwest::StatusCode::ACCEPTED);
        let inspect: serde_json::Value = inspect.json().await.unwrap();
        let replay = client
            .post(inspect_url)
            .json(&serde_json::json!({"request_id":"browser-inspect"}))
            .send()
            .await
            .unwrap();
        let replay: serde_json::Value = replay.json().await.unwrap();
        assert_eq!(inspect["outbox_id"], replay["outbox_id"]);

        let denied = client
            .get(format!(
                "{base}/api/v1/processes/{process_id}?participant=process-outsider"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);
        owner.close(None).await.unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn heart_unavailable_does_not_interrupt_chat_and_recovers_once() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (gateway, heart) = recoverable_heart_gateway().await;
        let (address, server) = start_test_server_with_gateway(repository, gateway).await;
        let mut owner = connect_as(address, "heart-isolation-owner").await;
        let circle = command(
            &mut owner,
            "isolation-circle",
            "create_circle",
            serde_json::json!({"slug":"heart-isolation","name":"Heart isolation"}),
        )
        .await;
        let circle_id = circle["payload"]["circle"]["id"].as_str().unwrap();
        let channel = command(
            &mut owner,
            "isolation-channel",
            "create_channel",
            serde_json::json!({"slug":"heart-isolation-chat","name":"Heart isolation chat","kind":"private","circle_id":circle_id}),
        )
        .await;
        let channel_id = channel["payload"]["channel"]["id"].as_str().unwrap();
        let client = reqwest::Client::new();
        let base = format!("http://{address}");
        let feature = client
            .post(format!(
                "{base}/api/v1/circles/{circle_id}/features/heart-event-planning?participant=heart-isolation-owner"
            ))
            .json(&serde_json::json!({"enabled":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(feature.status(), reqwest::StatusCode::NO_CONTENT);
        let started = client
            .post(format!(
                "{base}/api/v1/processes?participant=heart-isolation-owner"
            ))
            .json(&serde_json::json!({
                "channel_id":channel_id,
                "request_id":"heart-isolation-start",
                "namespace":"sproyt",
                "definition_name":"event-planning",
                "definition_version":"1",
                "metadata":{"title":"Resilient dinner"}
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(started.status(), reqwest::StatusCode::ACCEPTED);
        let process_id = started.json::<serde_json::Value>().await.unwrap()["process_link_id"]
            .as_str()
            .unwrap()
            .to_owned();

        tokio::time::timeout(Duration::from_secs(3), async {
            while heart.starts.load(Ordering::SeqCst) == 0 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("outbox did not attempt Heart while it was unavailable");

        let chat_body = "ordinary chat remains available without Heart";
        command(
            &mut owner,
            "chat-during-heart-outage",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":chat_body}),
        )
        .await;
        let loaded = command(
            &mut owner,
            "chat-during-heart-outage-read",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":20,"after":0}),
        )
        .await;
        assert!(
            loaded["payload"]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["body"] == chat_body),
            "ordinary chat must persist and read while Heart is unavailable"
        );

        heart.available.store(true, Ordering::SeqCst);
        let recovered_view = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let response = client
                    .get(format!(
                        "{base}/api/v1/processes/{process_id}?participant=heart-isolation-owner"
                    ))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), reqwest::StatusCode::OK);
                let view = response.json::<serde_json::Value>().await.unwrap();
                if view["process"]["status"] == "active" {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("queued process did not recover after Heart returned");
        assert!(heart.starts.load(Ordering::SeqCst) >= 2);
        assert_eq!(
            recovered_view["events"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|event| event["event_type"] == "process.started")
                .count(),
            1,
            "Heart recovery must complete the durable start exactly once"
        );

        owner.close(None).await.unwrap();
        server.abort();
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
        let mismatch = command_response(
            &mut socket,
            "send-1",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"must not replace the accepted body"}),
        )
        .await;
        assert_eq!(mismatch["type"], "error");
        assert_eq!(mismatch["payload"]["code"], "conflict");
        let replay = command(
            &mut socket,
            "send-1",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"capacity-1"}),
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
    async fn postgres_two_replica_realtime_and_restart_catch_up_gate() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        let alice_name = format!("replica-alice-{suffix}");
        let bob_name = format!("replica-bob-{suffix}");
        let first_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
        first_repository.migrate().await.unwrap();
        let second_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
        let (first_address, first_server) =
            start_postgres_test_server(first_repository, Duration::from_secs(60)).await;
        let (second_address, second_server) =
            start_postgres_test_server(second_repository, Duration::from_secs(60)).await;
        let mut alice = connect_as(first_address, &alice_name).await;
        let mut bob = connect_as(second_address, &bob_name).await;

        let alice_channels = command(
            &mut alice,
            "alice-channels",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        let channel_id = alice_channels["payload"]["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|channel| channel["slug"] == "general")
            .expect("global general channel must be available to every authenticated user")["id"]
            .as_str()
            .unwrap()
            .to_owned();
        command(
            &mut alice,
            "alice-subscribe",
            "subscribe_channel",
            serde_json::json!({"channel_id":channel_id}),
        )
        .await;
        command(
            &mut bob,
            "bob-subscribe",
            "subscribe_channel",
            serde_json::json!({"channel_id":channel_id}),
        )
        .await;

        let first_body = format!("cross-replica-a-{suffix}");
        let accepted = command(
            &mut alice,
            "alice-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":first_body}),
        )
        .await;
        let first_sequence = accepted["payload"]["message"]["sequence"].as_u64().unwrap();
        wait_for_chat_body(&mut bob, &first_body).await;

        alice.close(None).await.unwrap();
        first_server.abort();
        let missed_body = format!("restart-catch-up-{suffix}");
        command(
            &mut bob,
            "bob-send",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":missed_body}),
        )
        .await;

        let replacement_repository = Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
        let (replacement_address, replacement_server) =
            start_postgres_test_server(replacement_repository, Duration::from_secs(60)).await;
        let reconnect_started = Instant::now();
        let mut reconnected = connect_as(replacement_address, &alice_name).await;
        let loaded = command(
            &mut reconnected,
            "alice-catch-up",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":20,"after":first_sequence}),
        )
        .await;
        assert!(reconnect_started.elapsed() < Duration::from_secs(5));
        assert!(
            loaded["payload"]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|message| message["body"] == missed_body),
            "a restarted replica must catch up messages accepted while it was unavailable"
        );

        reconnected.close(None).await.unwrap();
        bob.close(None).await.unwrap();
        second_server.abort();
        replacement_server.abort();
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
        let member_hello = command(
            &mut member,
            "member-hello",
            "hello",
            serde_json::Value::Null,
        )
        .await;
        let member_id = member_hello["payload"]["participant_id"].clone();
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
        let not_inherited = command(
            &mut member,
            "channels-after-invite",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        assert!(
            !not_inherited["payload"]["channels"]
                .as_array()
                .unwrap()
                .iter()
                .any(|channel| channel["id"] == channel_id),
            "private channels must not be inherited with circle membership"
        );
        let denied = command_response(
            &mut member,
            "private-self-join",
            "join_channel",
            serde_json::json!({"channel":{"type":"id","value":channel_id}}),
        )
        .await;
        assert_eq!(denied["payload"]["code"], "permission_denied");
        command(
            &mut owner,
            "invite-private-member",
            "add_channel_member",
            serde_json::json!({"channel_id":channel_id,"user_id":member_id}),
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
        let loaded_messages = loaded["payload"]["messages"].as_array().unwrap();
        assert_eq!(loaded_messages.len(), 2);
        assert!(
            loaded_messages
                .iter()
                .all(|message| message["sender_display_name"] == "circle-owner")
        );
        let older_page = command(
            &mut member,
            "load-older-page",
            "load_recent_messages",
            serde_json::json!({"channel_id":channel_id,"limit":50,"before":2}),
        )
        .await;
        let older_messages = older_page["payload"]["messages"].as_array().unwrap();
        assert_eq!(older_messages.len(), 1);
        assert_eq!(older_messages[0]["sequence"].as_u64(), Some(1));
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

        let denied = command_response(
            &mut member,
            "member-delete-circle",
            "delete_circle",
            serde_json::json!({"circle_id":circle_id}),
        )
        .await;
        assert_eq!(denied["payload"]["code"], "permission_denied");
        let deleted = command(
            &mut owner,
            "owner-delete-circle",
            "delete_circle",
            serde_json::json!({"circle_id":circle_id}),
        )
        .await;
        assert_eq!(deleted["type"], "circle_deleted");

        let channels = command(
            &mut member,
            "list-after-delete",
            "list_my_channels",
            serde_json::Value::Null,
        )
        .await;
        assert!(
            channels["payload"]["channels"]
                .as_array()
                .unwrap()
                .iter()
                .all(|summary| summary["id"] != channel_id)
        );
        let circles = command(
            &mut member,
            "list-circles-after-delete",
            "list_my_circles",
            serde_json::Value::Null,
        )
        .await;
        assert!(
            circles["payload"]["circles"]
                .as_array()
                .unwrap()
                .iter()
                .all(|entry| entry[0]["id"] != circle_id)
        );
        server.abort();
    }

    #[tokio::test]
    async fn leaving_circle_disconnects_inaccessible_websocket_channels() {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let (address, server) = start_test_server(repository, Duration::from_secs(60)).await;
        let mut owner = connect_as(address, "leave-circle-owner").await;
        let circle = command(
            &mut owner,
            "leave-circle-create",
            "create_circle",
            serde_json::json!({"slug":"leave-circle","name":"Leave circle"}),
        )
        .await;
        let circle_id = circle["payload"]["circle"]["id"].clone();
        let channel = command(
            &mut owner,
            "leave-circle-channel",
            "create_channel",
            serde_json::json!({"slug":"leave-circle-chat","name":"Leave circle chat","kind":"local","circle_id":circle_id}),
        )
        .await;
        let channel_id = channel["payload"]["channel"]["id"].clone();
        let invitation = command(
            &mut owner,
            "leave-circle-invite",
            "create_circle_invitation",
            serde_json::json!({"circle_id":circle_id}),
        )
        .await;

        let mut member = connect_as(address, "leave-circle-member").await;
        let member_id = command(
            &mut member,
            "leave-circle-member-hello",
            "hello",
            serde_json::Value::Null,
        )
        .await["payload"]["participant_id"]
            .clone();
        command(
            &mut member,
            "leave-circle-accept",
            "accept_circle_invitation",
            serde_json::json!({"token":invitation["payload"]["invitation"]["token"]}),
        )
        .await;
        command(
            &mut member,
            "leave-circle-join-channel",
            "join_channel",
            serde_json::json!({"channel":{"type":"id","value":channel_id}}),
        )
        .await;
        command(
            &mut owner,
            "leave-circle-owner-subscribe",
            "subscribe_channel",
            serde_json::json!({"channel_id":channel_id}),
        )
        .await;
        command(
            &mut member,
            "leave-circle-member-subscribe",
            "subscribe_channel",
            serde_json::json!({"channel_id":channel_id}),
        )
        .await;

        command(
            &mut member,
            "leave-circle",
            "leave_circle",
            serde_json::json!({"circle_id":circle_id}),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let frame = owner.next().await.unwrap().unwrap();
                if let ClientMessage::Text(text) = frame {
                    let event: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if event["type"] == "chat"
                        && event["payload"]["event"]["type"] == "participant_left"
                        && event["payload"]["event"]["participant_id"] == member_id
                    {
                        return;
                    }
                }
            }
        })
        .await
        .expect("owner did not observe the departed member leaving presence");

        command(
            &mut owner,
            "leave-circle-send-after-leave",
            "send_message",
            serde_json::json!({"channel_id":channel_id,"body":"must not reach departed member"}),
        )
        .await;
        assert!(
            tokio::time::timeout(Duration::from_millis(350), member.next())
                .await
                .is_err(),
            "departed member received a websocket event after leaving the circle"
        );
        server.abort();
    }
}
