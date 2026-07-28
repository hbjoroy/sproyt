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
    operations::{OperationalState, healthz, metrics, record_metrics},
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

async fn service_worker() -> axum::response::Response {
    (
        [
            (CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (CACHE_CONTROL, "no-cache"),
            (HeaderName::from_static("service-worker-allowed"), "/"),
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
    tokio::task::spawn_blocking(move || {
        use image::GenericImageView;

        let image =
            image::load_from_memory(&content).map_err(|_| MediaPreparationError::InvalidImage)?;
        let dimensions = image.dimensions();
        if dimensions.0 <= MEDIA_PREVIEW_LONG_EDGE && dimensions.1 <= MEDIA_PREVIEW_LONG_EDGE {
            return Ok((content, Some(dimensions), None));
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
            content,
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
    let html = INDEX_HTML
        .replace("{{NONCE}}", &nonce)
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
        "default-src 'self'; script-src 'nonce-{nonce}' https://cdn.jsdelivr.net; style-src 'self' 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; font-src 'self' data:; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'"
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
        &[logout.clear_cookie, logout.clear_refresh_cookie],
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

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="nn">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <meta name="theme-color" content="#27604a">
    <meta name="application-name" content="Sprøyt">
    <meta name="apple-mobile-web-app-capable" content="yes">
    <meta name="apple-mobile-web-app-status-bar-style" content="default">
    <link rel="manifest" href="/manifest.webmanifest">
    <link rel="icon" href="/assets/sproyt-wave.svg" type="image/svg+xml">
    <link rel="apple-touch-icon" href="/assets/sproyt-wave-192.png">
    <title>Sprøyt</title>
    <style nonce="{{NONCE}}">
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
        width: min(1120px, 100%);
        display: grid;
        grid-template-columns: 280px minmax(0, 1fr);
        grid-template-rows: auto minmax(0, 1fr) auto;
        height: min(760px, calc(100dvh - 48px));
        min-height: 0;
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

      .sidebar {
        grid-row: 1 / -1;
        display: grid;
        grid-template-rows: auto auto 1fr auto;
        gap: 18px;
        padding: 20px 16px;
        border-right: 1px solid #e4e5de;
        background: #f4f6f2;
      }

      .brand { display: flex; align-items: center; gap: 10px; }
      .brand-mark { display: block; width: 42px; height: 42px; border-radius: 12px; }
      .identity { display: grid; gap: 4px; font-size: .9rem; }
      .identity a { color: #245b45; }
      #status-editor { padding: 6px 0; }
      #status-editor label span { display: flex; gap: 6px; }
      #status-emoji { width: 3.2rem; text-align: center; }
      #status-text { min-width: 0; }
      .emoji-picker { position: relative; align-self: center; }
      .emoji-picker summary { cursor: pointer; font-size: 1.35rem; list-style: none; }
      .emoji-picker div { position: absolute; bottom: 2.5rem; left: 0; z-index: 5; display: grid; grid-template-columns: repeat(4, auto); gap: 4px; padding: 8px; border: 1px solid #cdd3ca; border-radius: 8px; background: #fff; box-shadow: 0 8px 24px #0002; }
      #media-previews { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 6px; }
      #media-previews:empty { display: none; }
      #media-previews span { padding: 5px 8px; border-radius: 999px; background: #e7eee8; font-size: .8rem; }
      #upload-status { grid-column: 1 / -1; margin: 0; color: #506057; font-size: .85rem; }
      #upload-status:empty { display: none; }
      #upload-status[data-kind="error"] { color: #9f2929; font-weight: 650; }
      .composer-input { position: relative; min-width: 0; }
      .mention-suggestions { position: absolute; right: 0; bottom: calc(100% + 6px); left: 0; z-index: 10; max-height: min(42vh, 280px); overflow-y: auto; padding: 5px; border: 1px solid #cbd1c8; border-radius: 8px; background: #fff; box-shadow: 0 10px 28px #0003; }
      .mention-suggestions[hidden] { display: none; }
      .mention-suggestions button { display: flex; align-items: center; justify-content: space-between; gap: 8px; width: 100%; border: 0; background: transparent; color: #18201d; text-align: left; }
      .mention-suggestions button:hover, .mention-suggestions button[aria-selected="true"] { background: #dfe8e1; color: #183d2e; }
      .mention-suggestions small { color: #647269; }
      .message-media { margin: 10px 0 0; }
      .message-media img, .message-media video { display: block; max-width: min(100%, 720px); max-height: 70vh; border-radius: 10px; background: #111; }
      .message-media img { cursor: zoom-in; }
      .message-media figcaption { margin-top: 4px; color: #647269; font-size: .78rem; }
      .media-lightbox { width: 100vw; max-width: none; height: 100dvh; max-height: none; margin: 0; padding: 48px 20px 20px; border: 0; background: #090c0acc; color: white; }
      .media-lightbox::backdrop { background: #090c0ae6; }
      .media-lightbox img { display: block; width: auto; max-width: calc(100vw - 40px); height: auto; max-height: calc(100dvh - 100px); margin: auto; object-fit: contain; }
      .media-lightbox p { margin: 8px auto 0; text-align: center; }
      .media-lightbox button { position: fixed; top: 10px; right: 14px; min-width: 42px; min-height: 42px; border-color: #fff8; background: #111b; color: white; font-size: 1.5rem; }
      .message-reactions { display: flex; flex-wrap: wrap; align-items: center; gap: 5px; margin-top: 8px; }
      .reaction-badge { min-height: 28px; padding: 3px 8px; border-radius: 999px; background: #eef2ed; color: #183d2e; font-size: .84rem; }
      .reaction-badge[aria-pressed="true"] { border-color: #245b45; background: #d7e8dc; font-weight: 700; }
      .reaction-picker { position: relative; }
      .reaction-picker summary { cursor: pointer; list-style: none; padding: 3px 7px; border: 1px solid #cbd1c8; border-radius: 999px; font-size: .84rem; }
      .reaction-picker div { position: absolute; bottom: calc(100% + 5px); left: 0; z-index: 8; display: grid; grid-template-columns: repeat(4, auto); gap: 3px; padding: 6px; border: 1px solid #cbd1c8; border-radius: 8px; background: #fff; box-shadow: 0 8px 24px #0002; }
      .reaction-picker button { min-height: 34px; padding: 4px 7px; }
      .mobile-navigation-toggle { display: none; }
      .navigation-heading { margin: 0 8px 6px; color: #647269; font-size: .75rem; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; }
      .channel-list { display: grid; gap: 4px; align-content: start; }
      .task-editor { display: grid; gap: 8px; margin-top: 12px; }
      .channel-group { display: flex; justify-content: space-between; align-items: center; gap: 8px; margin: 12px 8px 2px; color: #647269; font-size: .78rem; font-weight: 700; }
      .channel-button { display: flex; justify-content: space-between; width: 100%; border: 0; background: transparent; color: inherit; text-align: left; }
      .channel-button:hover, .channel-button[aria-current="page"] { background: #dfe8e1; color: #183d2e; }
      .channel-button.has-unread, .channel-group.has-unread { color: #183d2e; font-weight: 800; }
      .unread { min-width: 1.6em; padding: 2px 6px; border-radius: 999px; background: #245b45; color: white; font-size: .75rem; text-align: center; }
      .channel-group .unread { min-width: 0; padding: 1px 6px; font-size: .7rem; }
      .conversation-header { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
      .conversation-header p { margin: 4px 0 0; color: #647269; }
      .peer-status[hidden] { display: none; }
      .profile-status { margin-left: 5px; cursor: help; }
      .empty-state { margin: auto; max-width: 460px; padding: 28px; text-align: center; }
      .empty-state h2 { margin-top: 0; }
      .onboarding { display: grid; gap: 10px; }
      .onboarding-actions { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
      .onboarding-notice { margin: 0; color: #506057; font-size: .85rem; line-height: 1.4; }
      .agent-access { border-top: 1px solid var(--border); padding-top: 12px; }
      .agent-access[open] { display: grid; gap: 10px; }
      .agent-access summary { cursor: pointer; font-weight: 700; }
      .agent-access p { margin: 0; }
      #agent-credential { min-height: 92px; font-family: ui-monospace, monospace; font-size: .75rem; }
      .advanced-tools[hidden] { display: none; }

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

      :focus-visible {
        outline: 3px solid #d17a22;
        outline-offset: 2px;
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

      .process-tools {
        display: grid;
        grid-template-columns: repeat(4, minmax(0, 1fr));
        gap: 8px;
        align-items: end;
        padding-top: 10px;
        border-top: 1px solid #e4e5de;
      }

      .process-view {
        display: grid;
        gap: 8px;
        padding: 10px;
        border: 1px solid #dfe3dc;
        border-radius: 8px;
      }

      .process-event {
        display: grid;
        gap: 4px;
        padding: 8px;
        border-left: 3px solid #245b45;
        background: #f4f6f3;
      }

      .process-event pre {
        margin: 0;
        overflow-x: auto;
        white-space: pre-wrap;
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
        min-height: 0;
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
        grid-template-columns: auto auto 1fr auto;
        gap: 12px;
        border-top: 1px solid #e4e5de;
      }
      form.send textarea { min-height: 52px; max-height: 160px; }

      @media (max-width: 640px) {
        body {
          padding: 12px;
        }

        main {
          height: calc(100dvh - 24px);
          min-height: 0;
          grid-template-columns: 1fr;
          grid-template-rows: auto auto minmax(0, 1fr) auto;
        }

        .sidebar { grid-row: auto; grid-template-columns: 1fr auto; grid-template-rows: auto; gap: 8px; padding: 8px 10px; border-right: 0; border-bottom: 1px solid #e4e5de; }
        .sidebar .identity { display: none; grid-column: 1 / -1; }
        .sidebar.mobile-open .identity { display: grid; }
        .mobile-navigation-toggle { display: inline-flex; align-items: center; align-self: center; }
        .sidebar nav, .sidebar .onboarding, .sidebar .agent-access { display: none; grid-column: 1 / -1; }
        .sidebar.mobile-open nav, .sidebar.mobile-open .onboarding, .sidebar.mobile-open .agent-access { display: grid; }

        .conversation-header { min-height: 48px; padding: 8px 10px; }
        .conversation-header h2 { margin: 0; font-size: 1.15rem; }
        .conversation-header p { font-size: .78rem; }
        .conversation-header .view-controls { display: none; }
        .conversation-header .status[data-routine="true"] { display: none; }

        form.send { grid-template-columns: auto auto minmax(0, 1fr) auto; gap: 6px; padding: 8px; align-items: end; }
        form.send textarea { min-height: 40px; max-height: 112px; padding: 7px 9px; resize: none; }
        form.send button { min-height: 40px; padding: 7px 10px; }
        form.send .emoji-picker summary { display: grid; place-items: center; min-width: 38px; min-height: 40px; }

        .connect,
        .circle-tools,
        .process-tools {
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

        .mention-suggestions { background: #19211c; border-color: #344038; }
        .mention-suggestions button { color: #eef3ee; }
        .reaction-badge { background: #26332b; color: #eef3ee; }
        .reaction-badge[aria-pressed="true"] { background: #315d48; }
        .reaction-picker div { background: #19211c; border-color: #344038; }

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

        .process-tools {
          border-color: #344038;
        }

        .process-view,
        .process-event {
          background: #19211c;
          border-color: #344038;
        }
      }
    </style>
  </head>
  <body>
    <main>
      <aside class="sidebar" id="sidebar-panel">
        <div class="brand"><img class="brand-mark" src="/assets/sproyt-wave.svg" alt=""><h1>Sprøyt</h1></div>
        <button class="mobile-navigation-toggle" id="mobile-navigation-toggle" type="button" aria-expanded="false" aria-controls="mobile-navigation mobile-onboarding">Samtalar og vennekretsar</button>
        <div class="identity">
          <span>Innlogga som <strong>{{DISPLAY_NAME}}</strong></span>
          <details id="status-editor">
            <summary id="current-status">Set status</summary>
            <label>Status <span><input id="status-emoji" aria-label="Status-emoji" maxlength="32" placeholder="🙂"><input id="status-text" maxlength="100" placeholder="Kva skjer?"></span></label>
            <div class="onboarding-actions" id="status-emoji-options" aria-label="Vel status-emoji"><button type="button" data-emoji="🙂">🙂</button><button type="button" data-emoji="💻">💻</button><button type="button" data-emoji="🏠">🏠</button><button type="button" data-emoji="🚶">🚶</button><button type="button" data-emoji="🍽️">🍽️</button><button type="button" data-emoji="🌴">🌴</button></div>
            <div class="onboarding-actions"><button id="save-status" type="button">Lagre</button><button id="clear-status" type="button">Fjern</button></div>
          </details>
          <a href="/auth/logout">Logg ut</a>
        </div>
        <nav aria-label="Samtalar" id="mobile-navigation">
          <p class="navigation-heading">Samtalar</p>
          <div class="onboarding-actions" role="group" aria-label="Personleg innboks">
            <button id="show-unread" type="button">Ulest</button>
            <button id="show-mentions" type="button">Omtalar</button>
            <button id="show-tasks" type="button">Oppgåver</button>
          </div>
          <div class="channel-list" id="channel-list"><span class="status">Lastar …</span></div>
          <p class="navigation-heading">Direktemelding</p>
          <label>Brukar<select id="direct-user"><option value="">Vel brukar</option></select></label>
          <button id="open-direct" type="button" disabled>Start samtale</button>
        </nav>
        <section class="onboarding" id="mobile-onboarding" aria-label="Ny vennekrets">
          <p class="navigation-heading">Vennekrets</p>
          <label>Vennekrets<select id="circle-select"><option value="">Ingen</option></select></label>
          <label>Namn<input id="circle-name" placeholder="Turvenner"></label>
          <input id="circle-slug" type="hidden">
          <div class="onboarding-actions"><button id="create-circle" type="button" disabled>Lag ny</button><button id="create-invitation" type="button" disabled>Inviter</button></div>
          <label>Invitasjonslenkje<input id="invitation-token" placeholder="Lim inn lenkja du fekk"></label>
          <div class="onboarding-actions"><button id="accept-invitation" type="button" disabled>Bli med</button><button id="copy-invitation" type="button" hidden>Kopier lenkje</button></div>
          <p class="onboarding-notice" id="onboarding-notice" role="status" aria-live="polite">Lag ein ny vennekrets, eller lim inn ei invitasjonslenkje.</p>
          <p class="navigation-heading">Kanalar</p>
          <label>Kanalnamn<input id="circle-channel" placeholder="Turprat"></label>
          <label>Tilgang<select id="channel-kind"><option value="local">Open i kretsen</option><option value="private">Privat – berre inviterte</option></select></label>
          <button id="create-circle-channel" type="button" disabled>Lag kanal</button>
          <label>Tilgjengelege kanalar<select id="joinable-channel"><option value="">Ingen</option></select></label>
          <button id="join-circle-channel" type="button" disabled>Bli med i kanal</button>
          <label>Inviter brukar<select id="channel-member"><option value="">Vel brukar</option></select></label>
          <button id="add-channel-member" type="button" disabled>Legg til i vald kanal</button>
          <button id="delete-circle" type="button" hidden disabled>Slett krets</button>
          <button id="export-data" type="button" hidden disabled>Eksporter mine data</button>
        </section>
        <details class="agent-access" {{AGENT_HIDDEN}}>
          <summary>Agenttilgang</summary>
          <p>Lag ein kortliva MCP-agent for den opne samtalen. Tilgangen varer i 30 minutt og kan lesast og skrive meldingar.</p>
          <button id="create-agent-access" type="button" disabled>Lag kortliva tilgang</button>
          <label for="agent-credential">Credential (blir berre vist no)</label>
          <textarea id="agent-credential" readonly hidden></textarea>
          <div class="onboarding-actions">
            <button id="copy-agent-credential" type="button" hidden>Kopier credential</button>
            <button id="revoke-agent-access" type="button" hidden>Trekk tilbake</button>
          </div>
          <p class="onboarding-notice" id="agent-access-notice" role="status" aria-live="polite">Vel ei samtale for å lage tilgang.</p>
        </details>
      </aside>
      <header class="conversation-header">
        <div><h2 id="conversation-title">Samtalar</h2><p class="peer-status" id="conversation-peer-status" hidden></p><p class="status" id="status" role="status" aria-live="polite">Koplar til …</p></div>
        <div class="view-controls" aria-label="Meldingsvising">
          <button id="view-mode" type="button" aria-pressed="true">Les</button>
          <button id="raw-mode" type="button" aria-pressed="false">Kjelde</button>
        </div>
        <form id="connect-form" hidden><input id="channel" value="general"><button id="connect" type="submit">Kople til</button></form>
        <div class="advanced-tools" {{ADVANCED_HIDDEN}}>
          <button id="enable-heart" type="button" disabled>Slå på event-planlegging</button>
          <label>Tittel<input id="process-title" placeholder="Middag på laurdag"></label>
          <button id="start-process" type="button" disabled>Start planlegging</button>
          <label>Prosess-ID<input id="process-id" autocomplete="off"></label>
          <button id="refresh-process" type="button" disabled>Oppdater status</button>
          <button id="inspect-process" type="button" disabled>Hent Heart-status</button>
          <button id="process-yes" type="button" disabled>Svar ja</button>
          <button id="process-no" type="button" disabled>Svar nei</button>
        </div>
        <section class="process-view" id="process-view" aria-live="polite" hidden></section>
      </header>
      <section class="messages" id="messages" aria-live="polite"><div class="empty-state"><h2>Vel ei samtale</h2><p>Samtalane dine kjem fram her når tilkoplinga er klar.</p></div></section>
      <form class="send" id="send-form">
        <details class="emoji-picker"><summary aria-label="Legg til emoji">😊</summary><div id="message-emoji-options"><button type="button" data-emoji="😀">😀</button><button type="button" data-emoji="😂">😂</button><button type="button" data-emoji="❤️">❤️</button><button type="button" data-emoji="👍">👍</button><button type="button" data-emoji="🎉">🎉</button><button type="button" data-emoji="🤔">🤔</button><button type="button" data-emoji="🙏">🙏</button><button type="button" data-emoji="🔥">🔥</button></div></details>
        <button id="attach-media" type="button" aria-label="Last opp bilete eller video">📎</button><input id="media-input" type="file" accept="image/*,video/*,.heic,.heif,.mov" multiple hidden><div id="media-previews"></div><p id="upload-status" role="status" aria-live="assertive"></p>
        <div class="composer-input"><div id="mention-suggestions" class="mention-suggestions" role="listbox" aria-label="Vel brukar å omtale" hidden></div><textarea id="body" name="body" aria-label="Melding" placeholder="Skriv ei melding …" autocomplete="off" aria-autocomplete="list" aria-controls="mention-suggestions" aria-expanded="false" disabled></textarea></div>
        <button id="send" type="submit" disabled>Send</button>
      </form>
    </main>
    <dialog class="media-lightbox" id="media-lightbox" aria-labelledby="media-lightbox-caption"><button id="media-lightbox-close" type="button" aria-label="Lukk fullskjermbiletet">×</button><img id="media-lightbox-image" alt=""><p id="media-lightbox-caption"></p></dialog>

    <script type="module" nonce="{{NONCE}}">
      if ("serviceWorker" in navigator) {
        window.addEventListener("load", () => navigator.serviceWorker.register("/service-worker.js", { scope: "/" }).catch(() => {}));
      }
      const connectForm = document.querySelector("#connect-form");
      const sendForm = document.querySelector("#send-form");
      const channelInput = document.querySelector("#channel");
      const bodyInput = document.querySelector("#body");
      const mentionSuggestions = document.querySelector("#mention-suggestions");
      const sendButton = document.querySelector("#send");
      const statusText = document.querySelector("#status-text");
      const statusEmoji = document.querySelector("#status-emoji");
      const currentStatus = document.querySelector("#current-status");
      const mediaInput = document.querySelector("#media-input");
      const mediaPreviews = document.querySelector("#media-previews");
      const uploadStatus = document.querySelector("#upload-status");
      const mediaLightbox = document.querySelector("#media-lightbox");
      const mediaLightboxImage = document.querySelector("#media-lightbox-image");
      const mediaLightboxCaption = document.querySelector("#media-lightbox-caption");
      const viewModeButton = document.querySelector("#view-mode");
      const rawModeButton = document.querySelector("#raw-mode");
      const statusEl = document.querySelector("#status");
      const messagesEl = document.querySelector("#messages");
      const channelList = document.querySelector("#channel-list");
      const directUser = document.querySelector("#direct-user");
      const openDirect = document.querySelector("#open-direct");
      const conversationTitle = document.querySelector("#conversation-title");
      const conversationPeerStatus = document.querySelector("#conversation-peer-status");
      const circleSelect = document.querySelector("#circle-select");
      const circleName = document.querySelector("#circle-name");
      const circleSlug = document.querySelector("#circle-slug");
      const circleChannel = document.querySelector("#circle-channel");
      const channelKind = document.querySelector("#channel-kind");
      const joinableChannel = document.querySelector("#joinable-channel");
      const joinCircleChannel = document.querySelector("#join-circle-channel");
      const channelMember = document.querySelector("#channel-member");
      const addChannelMember = document.querySelector("#add-channel-member");
      const invitationToken = document.querySelector("#invitation-token");
      const copyInvitation = document.querySelector("#copy-invitation");
      const createAgentAccessButton = document.querySelector("#create-agent-access");
      const copyAgentCredentialButton = document.querySelector("#copy-agent-credential");
      const revokeAgentAccessButton = document.querySelector("#revoke-agent-access");
      const agentCredential = document.querySelector("#agent-credential");
      const agentAccessNotice = document.querySelector("#agent-access-notice");
      const onboardingNotice = document.querySelector("#onboarding-notice");
      const circleButtons = ["#create-circle", "#create-circle-channel", "#create-invitation", "#accept-invitation", "#delete-circle"].map((id) => document.querySelector(id));
      const exportButton = document.querySelector("#export-data");
      const processTitle = document.querySelector("#process-title");
      const processId = document.querySelector("#process-id");
      const processView = document.querySelector("#process-view");
      const processButtons = ["#enable-heart", "#start-process", "#refresh-process", "#inspect-process", "#process-yes", "#process-no"].map((id) => document.querySelector(id));
      const sidebar = document.querySelector("#sidebar-panel");
      const mobileNavigationToggle = document.querySelector("#mobile-navigation-toggle");

      let socket = null;
      let heartbeatTimer = null;
      let reconnectTimer = null;
      let reconnectAttempt = 0;
      let stableConnectionTimer = null;
      let sessionRefreshTimer = null;
      let sessionRefreshDueAt = 0;
      let sessionRefreshPromise = null;
      let sessionRefreshRejected = false;
      let authenticationRecoveryPromise = null;
      let connectionRecoveryPromise = null;
      let lastBackgroundRecoveryAt = 0;
      let renderMode = "view";
      let requestNumber = 0;
      const browserSessionId = `browser-${crypto.randomUUID()}`;
      let activeChannelId = null;
      let subscribedChannelId = null;
      let reconnectScrollOffset = null;
      const activeConversationKey = "sproyt.active-channel.v1";
      let restoredChannelId = null;
      try { restoredChannelId = window.localStorage.getItem(activeConversationKey); } catch (_) {}
      let currentParticipantId = null;
      let requestedChannelSlug = "general";
      const timeline = [];
      const seenMessageIds = new Set();
      const catchUpTargets = new Map();
      const pendingCommands = new Map();
      const pendingMessages = new Map();
      const historyRequestIds = new Set();
      const historyPageSize = 50;
      let historyHasMore = false;
      let historyLoading = false;
      let mermaidPromise = null;
      let knownChannels = [];
      let knownUsers = [];
      const knownCircleUsers = new Map();
      let knownMentions = [];
      let knownTasks = [];
      const knownCircles = new Map();
      let temporaryAgentId = null;
      let pendingMedia = [];
      const messageReactions = new Map();
      const reactionEmojis = [...document.querySelectorAll("#message-emoji-options [data-emoji]")].map((button) => button.dataset.emoji);
      let mentionMatches = [];
      let selectedMentionIndex = 0;
      let activeMention = null;

      function scheduleSessionRefresh(seconds) {
        if (sessionRefreshTimer !== null) window.clearTimeout(sessionRefreshTimer);
        const delay = Math.max(1, Number(seconds) || 1) * 1000;
        sessionRefreshDueAt = Date.now() + delay;
        sessionRefreshTimer = window.setTimeout(() => refreshSession().catch(() => scheduleSessionRefresh(30)), delay);
      }

      function reconnectAfterSessionRefresh() {
        if (!socket || socket.readyState === WebSocket.CLOSED || socket.readyState === WebSocket.CLOSING) {
          connect(true);
          return;
        }
        if (socket.readyState === WebSocket.OPEN) connect(true, socket);
      }

      async function performSessionRefresh() {
        let response;
        try {
          response = await fetch("/auth/refresh", {
            method: "POST",
            credentials: "same-origin",
            headers: { "accept": "application/json" }
          });
        } catch (_) {
          sessionRefreshRejected = false;
          scheduleSessionRefresh(30);
          return false;
        }
        if (response.status === 401) {
          // The active WebSocket revalidates the session and redirects on a
          // real authentication expiry. A refresh token is optional at some
          // OIDC providers, so a failed proactive refresh must not create an
          // Authentik callback/reload loop while the session is still valid.
          sessionRefreshRejected = true;
          scheduleSessionRefresh(30);
          return false;
        }
        if (!response.ok) {
          sessionRefreshRejected = false;
          scheduleSessionRefresh(30);
          return false;
        }
        sessionRefreshRejected = false;
        const result = await response.json();
        const verification = await fetch("/auth/session", {
          credentials: "same-origin",
          cache: "no-store",
          headers: { "accept": "application/json" }
        });
        if (!verification.ok) {
          sessionRefreshRejected = verification.status === 401;
          scheduleSessionRefresh(30);
          return false;
        }
        scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
        reconnectAfterSessionRefresh();
        return true;
      }

      async function refreshSession(waitForLock = false) {
        if (sessionRefreshPromise) return sessionRefreshPromise;
        sessionRefreshPromise = (async () => {
          if (navigator.locks) {
            const options = waitForLock ? {} : { ifAvailable: true };
            return navigator.locks.request("sproyt-session-refresh", options, async (lock) => {
              if (lock) return performSessionRefresh();
              scheduleSessionRefresh(30);
              return false;
            });
          }
          return performSessionRefresh();
        })();
        try {
          return await sessionRefreshPromise;
        } finally {
          sessionRefreshPromise = null;
        }
      }

      async function scheduleInitialSessionRefresh() {
        const response = await fetch("/auth/session", {
          credentials: "same-origin",
          cache: "no-store",
          headers: { "accept": "application/json" }
        });
        if (!response.ok) {
          if (response.status === 401 && await refreshSession(true)) return;
          scheduleSessionRefresh(30);
          return;
        }
        const result = await response.json();
        scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
      }

      scheduleInitialSessionRefresh().catch(() => scheduleSessionRefresh(30));

      async function recoverAuthentication() {
        if (authenticationRecoveryPromise) return authenticationRecoveryPromise;
        authenticationRecoveryPromise = (async () => {
          setConnectionStatus("Fornyar økta …");
          const refreshed = await refreshSession(true);
          if (refreshed) {
            if (!socket || socket.readyState === WebSocket.CLOSED || socket.readyState === WebSocket.CLOSING) connect(true);
            return;
          }
          if (sessionRefreshRejected) {
            setConnectionStatus("Økta må stadfestast på nytt …");
            window.location.assign("/auth/login");
            return;
          }
          scheduleReconnect(1006, "ventar på nett for å fornye økta");
        })();
        try {
          return await authenticationRecoveryPromise;
        } finally {
          authenticationRecoveryPromise = null;
        }
      }

      async function recoverConnection(replaceOpenSocket = false) {
        if (connectionRecoveryPromise) return connectionRecoveryPromise;
        connectionRecoveryPromise = (async () => {
          let response;
          try {
            response = await fetch("/auth/session", {
              credentials: "same-origin",
              cache: "no-store",
              headers: { "accept": "application/json" }
            });
          } catch (_) {
            scheduleReconnect(1006, "ventar på nett");
            return;
          }
          if (response.status === 401) {
            await recoverAuthentication();
            return;
          }
          if (!response.ok) {
            scheduleReconnect(response.status, "kunne ikkje kontrollere økta");
            return;
          }
          const result = await response.json();
          scheduleSessionRefresh(Number(result.refresh_after_seconds) || 300);
          if (!socket || socket.readyState === WebSocket.CLOSED || socket.readyState === WebSocket.CLOSING) {
            connect(true);
          } else if (replaceOpenSocket && socket.readyState === WebSocket.OPEN) {
            connect(true, socket);
          }
        })();
        try {
          return await connectionRecoveryPromise;
        } finally {
          connectionRecoveryPromise = null;
        }
      }

      function resumeAfterBackground() {
        if (document.visibilityState === "hidden") return;
        const now = Date.now();
        if (now - lastBackgroundRecoveryAt < 5_000) return;
        lastBackgroundRecoveryAt = now;
        recoverConnection(true).catch(() => scheduleReconnect(1006, "kunne ikkje gjenopprette sambandet"));
      }

      window.addEventListener("pageshow", resumeAfterBackground);
      window.addEventListener("focus", resumeAfterBackground);
      window.addEventListener("online", resumeAfterBackground);

      function renderMediaPreviews() {
        mediaPreviews.replaceChildren(...pendingMedia.filter((media) => media.channel_id === activeChannelId).map((media) => {
          const item = document.createElement("span");
          item.textContent = `${media.content_type.startsWith("video/") ? "🎬" : "🖼️"} ${media.original_filename}`;
          return item;
        }));
      }

      function setUploadStatus(message, kind = "progress") {
        uploadStatus.textContent = message;
        uploadStatus.dataset.kind = kind;
      }

      async function uploadFailureMessage(response, filename) {
        let detail = "";
        try { detail = (await response.text()).trim(); } catch (_) {}
        const trace = response.headers.get("cf-ray") || response.headers.get("x-request-id");
        const reason = detail && !detail.startsWith("<") ? `: ${detail}` : "";
        const reference = trace ? ` Referanse: ${trace}.` : "";
        return `Opplasting av ${filename} feila (HTTP ${response.status})${reason}.${reference}`;
      }

      function postMedia(url, form, filename) {
        return new Promise((resolve, reject) => {
          const request = new XMLHttpRequest();
          request.open("POST", url);
          request.withCredentials = true;
          request.setRequestHeader("accept", "application/json");
          request.upload.addEventListener("progress", (event) => {
            const progress = event.lengthComputable ? ` ${Math.min(100, Math.round(event.loaded * 100 / event.total))} %` : "";
            setUploadStatus(`Lastar opp ${filename}${progress} …`);
          });
          request.upload.addEventListener("load", () => {
            setUploadStatus(`Opplasting av ${filename} er ferdig. Behandlar fila …`);
          });
          request.addEventListener("load", () => resolve({
            status: request.status,
            ok: request.status >= 200 && request.status < 300,
            headers: { get: (name) => request.getResponseHeader(name) },
            text: async () => request.responseText,
            json: async () => JSON.parse(request.responseText)
          }));
          request.addEventListener("error", () => reject(new Error("Nettverkssambandet vart brote")));
          request.addEventListener("abort", () => reject(new Error("Opplastinga vart avbroten")));
          request.send(form);
        });
      }

      async function uploadMediaFiles(files) {
        if (!activeChannelId) return;
        for (const file of files) {
          if (!file.size || file.size > 35 * 1024 * 1024) {
            setUploadStatus(`${file.name || "Fila"} må vere mellom 1 byte og 35 MiB.`, "error");
            continue;
          }
          const form = new FormData();
          form.append("file", file, file.name || "clipboard-image.png");
          const filename = file.name || "bilete";
          setUploadStatus(`Gjer klar ${filename} (${(file.size / 1024 / 1024).toFixed(1)} MiB) …`);
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const url = `/api/v1/channels/${activeChannelId}/media${authQuery}`;
          let response;
          try {
            response = await postMedia(url, form, filename);
            if (response.status === 401 && await refreshSession(true)) {
              response = await postMedia(url, form, filename);
            }
          } catch (error) {
            const online = navigator.onLine ? "Nettlesaren fekk ikkje noko HTTP-svar frå tenesta" : "Eininga er fråkopla nettet";
            setUploadStatus(`Opplasting av ${file.name || "fila"} feila: ${online}. ${error?.message || "Ukjend nettverksfeil"}.`, "error");
            continue;
          }
          if (response.status === 401) {
            setUploadStatus("Opplasting feila (HTTP 401): Økta kunne ikkje fornyast. Logg inn på nytt.", "error");
            continue;
          }
          if (!response.ok) { setUploadStatus(await uploadFailureMessage(response, file.name || "fila"), "error"); continue; }
          const result = await response.json();
          pendingMedia.push(result.media);
          renderMediaPreviews();
          setUploadStatus(`${file.name || "Fila"} er behandla og klar til å sendast.`, "success");
        }
        setConnected(socket?.readyState === WebSocket.OPEN, "Tilkopla");
      }

      document.querySelector("#attach-media").addEventListener("click", () => mediaInput.click());
      mediaInput.addEventListener("change", () => { uploadMediaFiles([...mediaInput.files]); mediaInput.value = ""; });
      bodyInput.addEventListener("paste", (event) => {
        const files = [...event.clipboardData.files].filter((file) => file.type.startsWith("image/") || file.type.startsWith("video/"));
        if (files.length) { event.preventDefault(); uploadMediaFiles(files); }
      });
      document.querySelector("#media-lightbox-close").addEventListener("click", () => mediaLightbox.close());
      mediaLightbox.addEventListener("click", (event) => {
        if (event.target === mediaLightbox) mediaLightbox.close();
      });
      mediaLightbox.addEventListener("close", () => {
        mediaLightboxImage.removeAttribute("src");
      });

      function mentionHandle(user) {
        return user.display_name.toLocaleLowerCase().replace(/[^\p{L}\p{N}_-]/gu, "");
      }

      function closeMentionSuggestions() {
        activeMention = null;
        mentionMatches = [];
        selectedMentionIndex = 0;
        mentionSuggestions.hidden = true;
        bodyInput.setAttribute("aria-expanded", "false");
        bodyInput.removeAttribute("aria-activedescendant");
      }

      function mentionCandidates() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        return channel?.circle_id ? (knownCircleUsers.get(channel.circle_id) || []) : knownUsers;
      }

      function selectMention(index) {
        const user = mentionMatches[index];
        if (!user || !activeMention) return;
        const replacement = `@${mentionHandle(user)} `;
        bodyInput.setRangeText(replacement, activeMention.start, activeMention.end, "end");
        closeMentionSuggestions();
        bodyInput.focus();
      }

      function renderMentionSuggestions() {
        mentionSuggestions.replaceChildren(...mentionMatches.map((user, index) => {
          const button = document.createElement("button");
          button.type = "button";
          button.id = `mention-option-${user.id}`;
          button.setAttribute("role", "option");
          button.setAttribute("aria-selected", String(index === selectedMentionIndex));
          const name = document.createElement("span");
          name.textContent = user.display_name;
          const handle = document.createElement("small");
          handle.textContent = `@${mentionHandle(user)}`;
          button.append(name, handle);
          button.addEventListener("pointerdown", (event) => event.preventDefault());
          button.addEventListener("click", () => selectMention(index));
          return button;
        }));
        mentionSuggestions.hidden = mentionMatches.length === 0;
        bodyInput.setAttribute("aria-expanded", String(mentionMatches.length > 0));
        if (mentionMatches.length > 0) {
          bodyInput.setAttribute("aria-activedescendant", `mention-option-${mentionMatches[selectedMentionIndex].id}`);
        } else {
          bodyInput.removeAttribute("aria-activedescendant");
        }
      }

      function updateMentionSuggestions() {
        const caret = bodyInput.selectionStart;
        if (caret === null || bodyInput.selectionEnd !== caret) {
          closeMentionSuggestions();
          return;
        }
        const match = bodyInput.value.slice(0, caret).match(/(?:^|\s)@([\p{L}\p{N}_-]*)$/u);
        if (!match) {
          closeMentionSuggestions();
          return;
        }
        const query = match[1].toLocaleLowerCase();
        activeMention = { start: caret - query.length - 1, end: caret };
        mentionMatches = mentionCandidates()
          .filter((user) => mentionHandle(user).startsWith(query))
          .sort((left, right) => left.display_name.localeCompare(right.display_name));
        selectedMentionIndex = Math.min(selectedMentionIndex, Math.max(0, mentionMatches.length - 1));
        renderMentionSuggestions();
      }

      bodyInput.addEventListener("input", updateMentionSuggestions);
      bodyInput.addEventListener("click", updateMentionSuggestions);
      bodyInput.addEventListener("keydown", (event) => {
        if (mentionSuggestions.hidden || mentionMatches.length === 0) return;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          const direction = event.key === "ArrowDown" ? 1 : -1;
          selectedMentionIndex = (selectedMentionIndex + direction + mentionMatches.length) % mentionMatches.length;
          renderMentionSuggestions();
        } else if (event.key === "Enter" || event.key === "Tab") {
          event.preventDefault();
          selectMention(selectedMentionIndex);
        } else if (event.key === "Escape") {
          event.preventDefault();
          closeMentionSuggestions();
        }
      });
      bodyInput.addEventListener("blur", () => window.setTimeout(closeMentionSuggestions, 100));

      connectForm.addEventListener("submit", (event) => {
        event.preventDefault();
        connect();
      });

      sendForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const draft = bodyInput.value.trim();
        const channelMedia = pendingMedia.filter((media) => media.channel_id === activeChannelId);
        const mediaTokens = channelMedia.map((media) => `[[media:${media.id}|${media.content_type}|${encodeURIComponent(media.original_filename)}]]`).join("\n");
        const body = [draft, mediaTokens].filter(Boolean).join("\n");
        if (!socket || socket.readyState !== WebSocket.OPEN || !activeChannelId || body.length === 0) {
          return;
        }
        if (subscribedChannelId !== activeChannelId) return;
        const requestId = sendCommand("send_message", { channel_id: activeChannelId, body });
        if (!requestId) return;
        pendingMessages.set(requestId, { body, draft, mediaIds: channelMedia.map((media) => media.id), channelId: activeChannelId });
        bodyInput.readOnly = true;
        setConnected(true, "Sender meldinga …");
      });

      function insertEmoji(input, emoji) {
        const start = input.selectionStart ?? input.value.length;
        const end = input.selectionEnd ?? start;
        input.setRangeText(emoji, start, end, "end");
        input.focus();
      }

      document.querySelectorAll("#message-emoji-options [data-emoji]").forEach((button) => {
        button.addEventListener("click", () => insertEmoji(bodyInput, button.dataset.emoji));
      });
      document.querySelectorAll("#status-emoji-options [data-emoji]").forEach((button) => {
        button.addEventListener("click", () => { statusEmoji.value = button.dataset.emoji; statusText.focus(); });
      });
      document.querySelector("#save-status").addEventListener("click", () => {
        sendCommand("set_status", { text: statusText.value, emoji: statusEmoji.value, expires_at: null });
      });
      document.querySelector("#clear-status").addEventListener("click", () => {
        statusText.value = "";
        statusEmoji.value = "";
        sendCommand("set_status", { text: "", emoji: "", expires_at: null });
      });

      directUser.addEventListener("change", () => {
        openDirect.disabled = !directUser.value;
      });
      openDirect.addEventListener("click", () => {
        if (directUser.value) sendCommand("open_direct_channel", { user_id: directUser.value });
      });
      document.querySelector("#show-unread").addEventListener("click", () => showInbox("unread"));
      document.querySelector("#show-mentions").addEventListener("click", () => showInbox("mentions"));
      document.querySelector("#show-tasks").addEventListener("click", () => showInbox("tasks"));

      viewModeButton.addEventListener("click", () => setRenderMode("view"));
      rawModeButton.addEventListener("click", () => setRenderMode("raw"));
      mobileNavigationToggle.addEventListener("click", () => {
        const open = sidebar.classList.toggle("mobile-open");
        mobileNavigationToggle.setAttribute("aria-expanded", String(open));
      });
      document.addEventListener("keydown", (event) => {
        if (event.key === "Escape" && sidebar.classList.contains("mobile-open")) {
          sidebar.classList.remove("mobile-open");
          mobileNavigationToggle.setAttribute("aria-expanded", "false");
          mobileNavigationToggle.focus();
        }
      });
      circleButtons[0].addEventListener("click", () => sendCommand("create_circle", {
        name: circleName.value.trim(), slug: slugify(circleSlug.value || circleName.value)
      }));
      circleName.addEventListener("input", updateOnboardingButtons);
      invitationToken.addEventListener("input", updateOnboardingButtons);
      circleSelect.addEventListener("change", updateOnboardingButtons);
      circleSelect.addEventListener("change", () => {
        if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
      });
      circleChannel.addEventListener("input", updateOnboardingButtons);
      circleButtons[1].addEventListener("click", () => {
        if (!circleSelect.value) return;
        const slug = slugify(circleChannel.value);
        sendCommand("create_channel", { slug, name: circleChannel.value.trim(), kind: channelKind.value, circle_id: circleSelect.value });
      });
      joinableChannel.addEventListener("change", updateOnboardingButtons);
      joinCircleChannel.addEventListener("click", () => {
        if (joinableChannel.value) sendCommand("join_channel", { channel: { type: "id", value: joinableChannel.value } });
      });
      channelMember.addEventListener("change", updateOnboardingButtons);
      addChannelMember.addEventListener("click", () => {
        if (activeChannelId && channelMember.value) sendCommand("add_channel_member", { channel_id: activeChannelId, user_id: channelMember.value });
      });
      circleButtons[2].addEventListener("click", () => {
        if (circleSelect.value) sendCommand("create_circle_invitation", { circle_id: circleSelect.value });
      });
      circleButtons[3].addEventListener("click", () => {
        const token = invitationValueToToken(invitationToken.value);
        if (token) {
          onboardingNotice.textContent = "Kontrollerer invitasjonen …";
          sendCommand("accept_circle_invitation", { token });
        }
      });
      copyInvitation.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(invitationToken.value);
          onboardingNotice.textContent = "Invitasjonslenkja er kopiert. Send henne til venen du vil invitere.";
        } catch (_) {
          invitationToken.focus();
          invitationToken.select();
          onboardingNotice.textContent = "Kopier den markerte lenkja og send henne til venen din.";
        }
      });
      createAgentAccessButton.addEventListener("click", createTemporaryAgentAccess);
      copyAgentCredentialButton.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(agentCredential.value);
          agentAccessNotice.textContent = "Credential er kopiert. Handsam han som eit passord.";
        } catch (_) {
          agentCredential.hidden = false;
          agentCredential.focus();
          agentCredential.select();
          agentAccessNotice.textContent = "Kopier den markerte credentialen manuelt.";
        }
      });
      revokeAgentAccessButton.addEventListener("click", revokeTemporaryAgentAccess);
      circleButtons[4].addEventListener("click", () => {
        if (!circleSelect.value) return;
        const selected = circleSelect.options[circleSelect.selectedIndex]?.textContent || "denne vennekretsen";
        if (window.confirm(`Slett ${selected} og all chat- og prosesshistorikk permanent?`)) {
          sendCommand("delete_circle", { circle_id: circleSelect.value });
        }
      });
      messagesEl.addEventListener("scroll", () => {
        if (messagesEl.scrollTop <= 80) loadOlderHistory();
      }, { passive: true });
      exportButton.addEventListener("click", async () => {
        try {
          const response = await fetch("/api/v1/me/export", {
            credentials: "same-origin"
          });
          if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
          const blob = await response.blob();
          const url = URL.createObjectURL(blob);
          const link = document.createElement("a");
          link.href = url;
          link.download = "sproyt-export.json";
          link.click();
          URL.revokeObjectURL(url);
          pushSystem("Dataeksporten er laga.");
        } catch (error) {
          pushSystem(`Kunne ikkje eksportere data: ${error.message}`);
        }
      });
      processButtons[0].addEventListener("click", () => setHeartFeature(true));
      processButtons[1].addEventListener("click", startEventPlanning);
      processButtons[2].addEventListener("click", refreshProcess);
      processButtons[3].addEventListener("click", inspectProcess);
      processButtons[4].addEventListener("click", () => answerProcess("yes"));
      processButtons[5].addEventListener("click", () => answerProcess("no"));

      function slugify(value) {
        return value.trim().toLowerCase().replace(/[^a-z0-9_-]+/g, "-");
      }

      function invitationValueToToken(value) {
        const candidate = value.trim();
        if (!candidate) return "";
        try {
          const url = new URL(candidate, window.location.origin);
          return url.searchParams.get("invite") || candidate;
        } catch (_) {
          return candidate;
        }
      }

      function connect(silent = false, previousSocket = null) {
        if (reconnectTimer !== null) {
          window.clearTimeout(reconnectTimer);
          reconnectTimer = null;
        }
        if (!previousSocket && heartbeatTimer !== null) {
          window.clearInterval(heartbeatTimer);
          heartbeatTimer = null;
        }
        if (!previousSocket && stableConnectionTimer !== null) {
          window.clearTimeout(stableConnectionTimer);
          stableConnectionTimer = null;
        }
        if (!previousSocket && socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
          return;
        }

        catchUpTargets.clear();
        if (activeChannelId && messagesEl.childElementCount > 0) {
          reconnectScrollOffset = Math.max(0, messagesEl.scrollHeight - messagesEl.scrollTop - messagesEl.clientHeight);
        }
        if (!activeChannelId) {
          requestedChannelSlug = (channelInput.value.trim() || "")
            .toLowerCase()
            .replace(/[^a-z0-9_-]+/g, "-");
        }
        const protocol = window.location.protocol === "https:" ? "wss" : "ws";
        const websocketUrl = new URL(`${protocol}://${window.location.host}/ws`);
        const developmentParticipant = new URLSearchParams(window.location.search).get("participant");
        if (developmentParticipant) websocketUrl.searchParams.set("participant", developmentParticipant);
        const nextSocket = new WebSocket(websocketUrl);
        if (!previousSocket) {
          socket = nextSocket;
          subscribedChannelId = null;
        }
        if (!silent) setConnected(false, "Koplar til ...");

        nextSocket.addEventListener("open", () => {
          if (previousSocket && socket !== previousSocket) {
            nextSocket.close(4000, "superseded session refresh");
            return;
          }
          if (!previousSocket && socket !== nextSocket) return;
          socket = nextSocket;
          if (heartbeatTimer !== null) window.clearInterval(heartbeatTimer);
          setConnected(true, "Tilkopla");
          stableConnectionTimer = window.setTimeout(() => {
            if (socket === nextSocket && nextSocket.readyState === WebSocket.OPEN) {
              reconnectAttempt = 0;
            }
          }, 10_000);
          sendCommand("hello");
          sendCommand("list_users");
          sendCommand("list_my_channels");
          sendCommand("list_my_circles");
          if (activeChannelId) sendCommand("subscribe_channel", { channel_id: activeChannelId });
          heartbeatTimer = window.setInterval(() => {
            sendCommand("ping");
            sendCommand("list_users");
            sendCommand("list_my_channels");
            if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          }, 20_000);
          if (previousSocket) {
            window.setTimeout(() => {
              if (socket === nextSocket && nextSocket.readyState === WebSocket.OPEN) {
                previousSocket.close(4000, "session refreshed");
              }
            }, 500);
          }
        });

        nextSocket.addEventListener("message", (event) => {
          if (socket !== nextSocket) return;
          renderServerEvent(JSON.parse(event.data));
        });

        nextSocket.addEventListener("close", (event) => {
          if (previousSocket && socket === previousSocket) {
            scheduleSessionRefresh(30);
            return;
          }
          if (socket !== nextSocket) return;
          subscribedChannelId = null;
          for (const requestId of pendingMessages.keys()) {
            failPendingMessage(requestId, "sambandet vart brote; kontroller samtalen før du prøver igjen");
          }
          if (heartbeatTimer !== null) {
            window.clearInterval(heartbeatTimer);
            heartbeatTimer = null;
          }
          if (stableConnectionTimer !== null) {
            window.clearTimeout(stableConnectionTimer);
            stableConnectionTimer = null;
          }
          if (event.code === 1008) {
            recoverAuthentication().catch(() => scheduleReconnect(event.code, event.reason));
            return;
          }
          scheduleReconnect(event.code, event.reason);
        });

        nextSocket.addEventListener("error", () => {
          if (previousSocket && socket === previousSocket) return;
          if (socket === nextSocket) setConnected(false, "Mista sambandet");
        });
      }

      function scheduleReconnect(closeCode = 1006, closeReason = "") {
        reconnectAttempt += 1;
        const delay = Math.min(15_000, 500 * (2 ** Math.min(reconnectAttempt - 1, 5)));
        const detail = closeReason ? `kode ${closeCode}: ${closeReason}` : `kode ${closeCode}`;
        setConnected(false, `Fråkopla (${detail}) – prøver igjen om ${Math.ceil(delay / 1000)} sekund`);
        reconnectTimer = window.setTimeout(() => {
          reconnectTimer = null;
          recoverConnection().catch(() => scheduleReconnect(closeCode, closeReason));
        }, delay);
      }

      function sendCommand(type, payload) {
        if (!socket || socket.readyState !== WebSocket.OPEN) return null;
        requestNumber += 1;
        const command = {
          protocol: "sproyt.chat.v1",
          request_id: `${browserSessionId}-${requestNumber}`,
          type
        };
        if (payload !== undefined) {
          command.payload = payload;
        }
        socket.send(JSON.stringify(command));
        pendingCommands.set(command.request_id, type);
        return command.request_id;
      }

      function finishPendingMessage(requestId, message) {
        const pending = requestId ? pendingMessages.get(requestId) : undefined;
        if (!pending) return;
        if (message?.channel_id !== pending.channelId || message?.body !== pending.body) {
          console.warn("Sendekvitteringa samsvarar ikkje med kommandoen", {
            requestId,
            requestedChannelId: pending.channelId,
            acceptedChannelId: message?.channel_id,
            acceptedMessageId: message?.id
          });
          failPendingMessage(requestId, "tenaren svarte med ei eldre meldingskvittering; utkastet er bevart");
          return;
        }
        pendingMessages.delete(requestId);
        bodyInput.readOnly = false;
        if (bodyInput.value.trim() === pending.draft) bodyInput.value = "";
        pendingMedia = pendingMedia.filter((media) => !pending.mediaIds.includes(media.id));
        renderMediaPreviews();
        if (message?.channel_id === activeChannelId) bodyInput.focus();
        setConnected(socket?.readyState === WebSocket.OPEN, "Tilkopla");
      }

      function failPendingMessage(requestId, message) {
        const pending = requestId ? pendingMessages.get(requestId) : undefined;
        if (!pending) return;
        pendingMessages.delete(requestId);
        bodyInput.readOnly = false;
        if (bodyInput.value.trim().length === 0) bodyInput.value = pending.body;
        setConnected(socket?.readyState === WebSocket.OPEN, `Meldinga vart ikkje sendt: ${message}`);
        bodyInput.focus();
      }

      function setConnected(connected, status) {
        setConnectionStatus(status);
        const writableChannel = connected
          && activeChannelId !== null
          && subscribedChannelId === activeChannelId;
        bodyInput.disabled = !writableChannel;
        sendButton.disabled = !writableChannel || pendingMessages.size > 0;
        circleButtons.forEach((button) => { button.disabled = !connected; });
        exportButton.disabled = !connected;
        processButtons.forEach((button) => { button.disabled = !connected; });
        updateOnboardingButtons();
      }

      function setConnectionStatus(status) {
        statusEl.textContent = status;
        statusEl.dataset.routine = String(status === "Tilkopla");
      }

      function updateOnboardingButtons() {
        const connected = socket?.readyState === WebSocket.OPEN;
        circleButtons[0].disabled = !connected || circleName.value.trim().length < 2;
        circleButtons[2].disabled = !connected || !circleSelect.value;
        circleButtons[3].disabled = !connected || !invitationValueToToken(invitationToken.value);
        circleButtons[1].disabled = !connected || !circleSelect.value || circleChannel.value.trim().length < 2;
        joinCircleChannel.disabled = !connected || !joinableChannel.value;
        const active = knownChannels.find((channel) => channel.id === activeChannelId);
        addChannelMember.disabled = !connected || !channelMember.value || !active || !["owner", "moderator"].includes(active.role);
      }

      async function processApi(path, method = "GET", body = undefined) {
        const response = await fetch(path, {
          method,
          credentials: "same-origin",
          headers: body === undefined ? {} : { "content-type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body)
        });
        const text = await response.text();
        if (!response.ok) throw new Error(text || `HTTP ${response.status}`);
        return text ? JSON.parse(text) : null;
      }

      async function setHeartFeature(enabled) {
        if (!circleSelect.value) {
          pushSystem("Vel ein vennekrets før event-planlegging blir slått på.");
          return;
        }
        try {
          await processApi(`/api/v1/circles/${circleSelect.value}/features/heart-event-planning`, "POST", { enabled });
          pushSystem(enabled ? "Event-planlegging er slått på for kretsen." : "Event-planlegging er slått av.");
        } catch (error) {
          pushSystem(`Kunne ikkje endre event-planlegging: ${error.message}`);
        }
      }

      async function startEventPlanning() {
        if (!activeChannelId || !circleSelect.value) {
          pushSystem("Vel ein kretskanal før du startar planlegging.");
          return;
        }
        try {
          const result = await processApi("/api/v1/processes", "POST", {
            channel_id: activeChannelId,
            request_id: crypto.randomUUID(),
            namespace: "sproyt",
            definition_name: "event-planning",
            definition_version: "1",
            metadata: { title: processTitle.value.trim() || "Event-planlegging" }
          });
          processId.value = result.process_link_id;
          await refreshProcess();
        } catch (error) {
          pushSystem(`Kunne ikkje starte planlegging: ${error.message}`);
        }
      }

      async function refreshProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          renderProcess(await processApi(`/api/v1/processes/${id}`));
        } catch (error) {
          pushSystem(`Kunne ikkje hente prosess: ${error.message}`);
        }
      }

      async function inspectProcess() {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processApi(`/api/v1/processes/${id}/inspect`, "POST", { request_id: crypto.randomUUID() });
          pushSystem("Heart-status er lagd i den varige køen. Oppdater status om litt.");
        } catch (error) {
          pushSystem(`Kunne ikkje hente Heart-status: ${error.message}`);
        }
      }

      async function answerProcess(answer) {
        const id = processId.value.trim();
        if (!id) return;
        try {
          await processApi(`/api/v1/processes/${id}/messages`, "POST", {
            request_id: crypto.randomUUID(), payload: { answer }
          });
          pushSystem(`Svaret «${answer}» er lagd i den varige køen.`);
        } catch (error) {
          pushSystem(`Kunne ikkje svare på prosessen: ${error.message}`);
        }
      }

      function renderProcess(view) {
        processView.replaceChildren();
        processView.hidden = false;
        const heading = document.createElement("strong");
        heading.textContent = `${view.process.definition_name}: ${view.process.status}`;
        processView.append(heading);
        for (const event of view.events) {
          const article = document.createElement("article");
          article.className = "process-event";
          const meta = document.createElement("span");
          meta.className = "meta";
          meta.textContent = `${event.event_type} · ${event.actor_id}`;
          const payload = document.createElement("pre");
          payload.textContent = JSON.stringify(event.payload, null, 2);
          article.append(meta, payload);
          processView.append(article);
        }
      }

      function setRenderMode(mode) {
        renderMode = mode;
        viewModeButton.setAttribute("aria-pressed", String(mode === "view"));
        rawModeButton.setAttribute("aria-pressed", String(mode === "raw"));
        renderTimeline();
      }

      function renderKnownUsers() {
        directUser.replaceChildren(new Option("Vel brukar", ""));
        channelMember.replaceChildren(new Option("Vel brukar", ""));
        knownUsers.filter((user) => user.id !== currentParticipantId).forEach((user) => {
          const handle = mentionHandle(user);
          const status = [user.status_emoji, user.status_text].filter(Boolean).join(" ");
          const label = `${user.display_name} (@${handle})${status ? ` · ${status}` : ""}`;
          directUser.add(new Option(label, user.id));
          channelMember.add(new Option(`${user.display_name}${status ? ` · ${status}` : ""}`, user.id));
        });
        const own = knownUsers.find((user) => user.id === currentParticipantId);
        if (own) {
          statusEmoji.value = own.status_emoji || "";
          statusText.value = own.status_text || "";
          currentStatus.textContent = own.status_text || own.status_emoji
            ? `${own.status_emoji || ""} ${own.status_text || ""}`.trim()
            : "Set status";
        }
        openDirect.disabled = !directUser.value;
      }

      function activeProfile(userId) {
        return knownUsers.find((user) => user.id === userId);
      }

      function profileStatus(profile) {
        if (!profile || (!profile.status_emoji && !profile.status_text)) return null;
        return {
          symbol: profile.status_emoji || "●",
          text: profile.status_text || "",
          label: [profile.status_emoji, profile.status_text].filter(Boolean).join(" ")
        };
      }

      function appendProfileStatus(target, userId) {
        const status = profileStatus(activeProfile(userId));
        if (!status) return;
        const indicator = document.createElement("span");
        indicator.className = "profile-status";
        indicator.textContent = status.symbol;
        indicator.title = status.label;
        indicator.setAttribute("aria-label", `Status: ${status.label}`);
        target.append(indicator);
      }

      function renderConversationIdentity() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        conversationPeerStatus.hidden = true;
        conversationPeerStatus.replaceChildren();
        if (!channel) return;
        conversationTitle.textContent = channel.name;
        const peer = channel.direct_user_id ? activeProfile(channel.direct_user_id) : null;
        const status = profileStatus(peer);
        if (!peer || !status) return;
        conversationPeerStatus.hidden = false;
        const symbol = document.createElement("span");
        symbol.textContent = status.symbol;
        symbol.title = status.label;
        symbol.setAttribute("aria-label", `Status: ${status.label}`);
        conversationPeerStatus.append(symbol);
        if (status.text) conversationPeerStatus.append(document.createTextNode(` ${status.text}`));
      }

      function renderServerEvent(event) {
        if (event.protocol !== "sproyt.chat.v1") {
          pushSystem("Serveren svarte med ein ukjend protokoll.");
          return;
        }
        const payload = event.payload || {};
        const requestedCommand = event.request_id ? pendingCommands.get(event.request_id) : undefined;
        if (event.request_id) pendingCommands.delete(event.request_id);

        if (event.type === "hello") {
          currentParticipantId = payload.participant_id;
          return;
        }

        if (event.type === "users_listed") {
          knownUsers = payload.users;
          renderKnownUsers();
          renderConversationIdentity();
          renderTimeline({ preserveScroll: true });
          updateMentionSuggestions();
          return;
        }

        if (event.type === "circle_users_listed") {
          knownCircleUsers.set(payload.circle_id, payload.users);
          updateMentionSuggestions();
          return;
        }

        if (event.type === "status_updated") {
          knownUsers = [payload.profile, ...knownUsers.filter((user) => user.id !== payload.profile.id)];
          for (const [circleId, users] of knownCircleUsers) {
            if (users.some((user) => user.id === payload.profile.id)) {
              knownCircleUsers.set(circleId, [payload.profile, ...users.filter((user) => user.id !== payload.profile.id)]);
            }
          }
          renderKnownUsers();
          renderConversationIdentity();
          renderTimeline({ preserveScroll: true });
          document.querySelector("#status-editor").open = false;
          return;
        }

        if (event.type === "mentions_listed") {
          knownMentions = payload.mentions;
          renderMentionInbox();
          return;
        }

        if (event.type === "mention_read") {
          const mention = knownMentions.find((item) => item.message.id === payload.message_id);
          if (mention) mention.read = true;
          renderMentionInbox();
          return;
        }

        if (event.type === "tasks_listed") {
          knownTasks = payload.tasks;
          renderTaskInbox();
          return;
        }

        if (event.type === "task_created") {
          knownTasks = [payload.task, ...knownTasks.filter((task) => task.id !== payload.task.id)];
          showInbox("tasks");
          return;
        }

        if (event.type === "task_updated") {
          knownTasks = knownTasks.map((task) => task.id === payload.task.id ? payload.task : task);
          renderTaskInbox();
          return;
        }

        if (event.type === "circles_listed") {
          knownCircles.clear();
          circleSelect.replaceChildren(new Option("Ingen", ""));
          payload.circles.forEach(([circle, role]) => {
            knownCircles.set(circle.id, { ...circle, role });
            circleSelect.add(new Option(`${circle.name} (${role})`, circle.id));
          });
          if (!circleSelect.value && payload.circles.length > 0) {
            circleSelect.value = payload.circles[0][0].id;
          }
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          updateOnboardingButtons();
          renderChannels();
          return;
        }
        if (event.type === "circle_created") {
          circleSelect.add(new Option(`${payload.circle.name} (owner)`, payload.circle.id));
          circleSelect.value = payload.circle.id;
          pushSystem(`Vennekretsen ${payload.circle.name} er oppretta.`);
          onboardingNotice.textContent = `${payload.circle.name} er klar. No kan du invitere vener.`;
          circleName.value = "";
          sendCommand("create_channel", {
            slug: "prat", name: "Prat", kind: "private", circle_id: payload.circle.id
          });
          return;
        }
        if (event.type === "circle_deleted") {
          activeChannelId = null;
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          pushSystem("Vennekretsen og den tilhøyrande historikken er sletta.");
          return;
        }
        if (event.type === "circle_invitation_created") {
          invitationToken.value = `${window.location.origin}/?invite=${encodeURIComponent(payload.invitation.token)}`;
          copyInvitation.hidden = false;
          onboardingNotice.textContent = "Invitasjonslenkja er klar. Kopier og send henne til venen din.";
          updateOnboardingButtons();
          return;
        }
        if (event.type === "circle_invitation_accepted") {
          onboardingNotice.textContent = "Du er med i vennekretsen. Samtalane blir lasta inn no.";
          invitationToken.value = "";
          copyInvitation.hidden = true;
          const cleanUrl = new URL(window.location.href);
          cleanUrl.searchParams.delete("invite");
          window.history.replaceState({}, "", cleanUrl);
          sendCommand("list_my_circles");
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "channels_listed") {
          knownChannels = payload.channels;
          renderChannels();
          renderConversationIdentity();
          updateAgentAccessControls();
          const requested = knownChannels.find((channel) => channel.slug === requestedChannelSlug);
          const current = knownChannels.find((channel) => channel.id === activeChannelId);
          const restored = knownChannels.find((channel) => channel.id === restoredChannelId);
          // Reconnects (including silent OIDC refresh) must keep the active
          // conversation. The requested slug is only a startup fallback.
          const next = current || restored || requested || knownChannels[0];
          if (next && next.id !== activeChannelId) selectChannel(next);
          return;
        }

        if (event.type === "channel_created") {
          knownChannels.push({ ...payload.channel, role: "owner", latest_sequence: 0, last_read_sequence: 0 });
          renderChannels();
          selectChannel(payload.channel);
          circleChannel.value = "";
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "joinable_channels_listed") {
          joinableChannel.replaceChildren(new Option(payload.channels.length ? "Vel kanal" : "Ingen", ""));
          payload.channels.forEach((channel) => joinableChannel.add(new Option(`# ${channel.name}`, channel.id)));
          updateOnboardingButtons();
          return;
        }

        if (event.type === "membership_joined") {
          sendCommand("list_my_channels");
          if (circleSelect.value) sendCommand("list_joinable_channels", { circle_id: circleSelect.value });
          return;
        }

        if (event.type === "channel_member_added") {
          onboardingNotice.textContent = "Brukaren er lagd til i kanalen.";
          return;
        }

        if (event.type === "direct_channel_opened") {
          let channel = knownChannels.find((item) => item.id === payload.channel.id);
          if (!channel) {
            channel = { ...payload.channel, latest_sequence: 0, last_read_sequence: 0, role: "member" };
            knownChannels.push(channel);
          }
          renderChannels();
          selectChannel(channel);
          sendCommand("list_my_channels");
          return;
        }

        if (event.type === "subscription_started") {
          if (payload.channel_id !== activeChannelId) {
            sendCommand("unsubscribe_channel", { channel_id: payload.channel_id });
            return;
          }
          subscribedChannelId = payload.channel_id;
          setConnectionStatus("Tilkopla");
          renderConversationIdentity();
          payload.history.forEach(appendTimelineMessage);
          historyHasMore = payload.history.length === historyPageSize;
          historyLoading = false;
          acknowledgeLatest(payload.channel_id, payload.history);
          bodyInput.disabled = false;
          sendButton.disabled = false;
          renderChannels();
          const scrollOffset = reconnectScrollOffset;
          reconnectScrollOffset = null;
          renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 });
          if (scrollOffset !== null && scrollOffset >= 80) restoreConversationScrollOffset(scrollOffset);
          updateAgentAccessControls();
          return;
        }

        if (event.type === "subscription_ended") {
          if (payload.channel_id === subscribedChannelId) {
            subscribedChannelId = null;
            setConnected(socket?.readyState === WebSocket.OPEN, "Koplar til samtalen …");
          }
          return;
        }

        if (event.type === "channel_reactions_listed") {
          if (payload.channel_id === activeChannelId) {
            replaceChannelReactions(payload.reactions);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "message_reaction_changed") {
          if (payload.change.channel_id === activeChannelId) {
            applyReactionChange(payload.change);
            renderTimeline({ preserveScroll: true });
          }
          return;
        }

        if (event.type === "chat") {
          const chatEvent = payload.event;
          if (chatEvent.type === "message_accepted") {
            updateLatestSequence(chatEvent.message.channel_id, chatEvent.message.sequence);
            if (chatEvent.message.channel_id === activeChannelId) {
              appendTimelineMessage(chatEvent.message);
              acknowledgeLatest(activeChannelId, [chatEvent.message]);
              renderTimeline();
            } else {
              renderChannels();
            }
          } else if (chatEvent.type === "message_reaction_changed") {
            if (chatEvent.change.channel_id === activeChannelId) {
              applyReactionChange(chatEvent.change);
              renderTimeline({ preserveScroll: true });
            }
          } else if (chatEvent.type === "participant_joined") {
            if (chatEvent.participant_id !== currentParticipantId) pushSystem("Ein ven kom inn i samtalen.");
          } else if (chatEvent.type === "participant_left") {
            if (chatEvent.participant_id !== currentParticipantId) pushSystem("Ein ven gjekk ut av samtalen.");
          }
          return;
        }

        if (event.type === "message_accepted") {
          updateLatestSequence(payload.message.channel_id, payload.message.sequence);
          if (payload.message.channel_id === activeChannelId) {
            appendTimelineMessage(payload.message);
            acknowledgeLatest(activeChannelId, [payload.message]);
            renderTimeline();
          } else {
            renderChannels();
          }
          finishPendingMessage(event.request_id, payload.message);
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
          const olderHistory = historyRequestIds.delete(event.request_id);
          if (olderHistory) {
            historyLoading = false;
            if (payload.channel_id !== activeChannelId) return;
            historyHasMore = payload.messages.length === historyPageSize;
            prependTimelineMessages(payload.messages);
            renderTimeline({ preserveScroll: true });
            return;
          }
          payload.messages.forEach(appendTimelineMessage);
          acknowledgeLatest(payload.channel_id, payload.messages);
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

        if (event.type === "read_marker_updated") {
          const channel = knownChannels.find((item) => item.id === payload.membership.channel_id);
          if (channel) channel.last_read_sequence = payload.membership.last_read_sequence;
          renderChannels();
          return;
        }

        if (event.type === "error") {
          if (historyRequestIds.delete(event.request_id)) historyLoading = false;
          if (requestedCommand === "send_message") {
            failPendingMessage(event.request_id, payload.message || payload.code || "ukjend feil");
            pushSystem(payload.message || payload.code);
            return;
          }
          if (requestedCommand === "accept_circle_invitation") {
            onboardingNotice.textContent = payload.code === "not_found"
              ? "Invitasjonen finst ikkje eller er ikkje gyldig lenger. Be venen din lage ei ny lenkje."
              : "Du kunne ikkje bli med med denne invitasjonen. Kontroller lenkja eller be om ei ny.";
            return;
          }
          if (requestedCommand === "create_circle") {
            onboardingNotice.textContent = "Vennekretsen kunne ikkje opprettast. Prøv eit anna namn.";
            return;
          }
          pushSystem(payload.message || payload.code);
        }
      }

      function renderChannels() {
        channelList.replaceChildren();
        if (knownChannels.length === 0) {
          const empty = document.createElement("p");
          empty.className = "status";
          empty.textContent = "Ingen samtalar enno";
          channelList.append(empty);
          return;
        }
        const groupedChannels = new Map();
        for (const channel of knownChannels) {
          const groupId = channel.circle_id || "direct";
          if (!groupedChannels.has(groupId)) groupedChannels.set(groupId, []);
          groupedChannels.get(groupId).push(channel);
        }
        for (const [groupId, channels] of groupedChannels) {
          const heading = document.createElement("p");
          heading.className = "channel-group";
          const headingLabel = document.createElement("span");
          headingLabel.textContent = groupId === "direct" ? "Andre samtalar" : (knownCircles.get(groupId)?.name || "Vennekrets");
          heading.append(headingLabel);
          const groupUnreadCount = channels.reduce(
            (total, channel) => total + Math.max(0, channel.latest_sequence - channel.last_read_sequence),
            0
          );
          if (groupUnreadCount > 0) {
            heading.classList.add("has-unread");
            const unread = document.createElement("span");
            unread.className = "unread";
            unread.textContent = approximateUnreadCount(groupUnreadCount);
            unread.setAttribute("aria-label", `${groupUnreadCount} uleste meldingar i gruppa`);
            heading.append(unread);
          }
          channelList.append(heading);
          for (const channel of channels) {
          const button = document.createElement("button");
          button.type = "button";
          button.className = "channel-button";
          button.setAttribute("aria-current", channel.id === activeChannelId ? "page" : "false");
          const name = document.createElement("span");
          name.textContent = `# ${channel.name}`;
          button.append(name);
          const unreadCount = Math.max(0, channel.latest_sequence - channel.last_read_sequence);
          if (unreadCount > 0 && channel.id !== activeChannelId) {
            button.classList.add("has-unread");
            const unread = document.createElement("span");
            unread.className = "unread";
            unread.textContent = approximateUnreadCount(unreadCount);
            unread.setAttribute("aria-label", `${unreadCount} uleste meldingar`);
            button.append(unread);
          }
          button.addEventListener("click", () => selectChannel(channel));
          channelList.append(button);
          }
        }
      }

      function approximateUnreadCount(count) {
        if (count < 25) return String(count);
        if (count < 50) return "25+";
        if (count < 100) return "50+";
        return "100+";
      }

      function showInbox(kind) {
        if (subscribedChannelId) {
          sendCommand("unsubscribe_channel", { channel_id: subscribedChannelId });
        }
        subscribedChannelId = null;
        activeChannelId = null;
        reconnectScrollOffset = null;
        timeline.length = 0;
        messageReactions.clear();
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        bodyInput.disabled = true;
        sendButton.disabled = true;
        messagesEl.replaceChildren();
        renderConversationIdentity();
        if (kind === "unread") {
          conversationTitle.textContent = "Uleste meldingar";
          const unread = knownChannels.filter((channel) => channel.latest_sequence > channel.last_read_sequence);
          if (unread.length === 0) {
            messagesEl.innerHTML = '<div class="empty-state"><h2>Alt er lese</h2><p>Du har ingen uleste meldingar akkurat no.</p></div>';
          } else {
            for (const channel of unread) {
              const button = document.createElement("button");
              button.type = "button";
              button.className = "channel-button";
              button.textContent = `${channel.name} · ${channel.latest_sequence - channel.last_read_sequence} uleste`;
              button.addEventListener("click", () => selectChannel(channel));
              messagesEl.append(button);
            }
          }
        } else if (kind === "mentions") {
          conversationTitle.textContent = "Omtalar";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar omtalar …</h2></div>';
          sendCommand("list_mentions");
        } else {
          conversationTitle.textContent = "Oppgåver";
          messagesEl.innerHTML = '<div class="empty-state"><h2>Lastar oppgåver …</h2></div>';
          sendCommand("list_tasks");
        }
        renderChannels();
      }

      function renderMentionInbox() {
        if (conversationTitle.textContent !== "Omtalar") return;
        messagesEl.replaceChildren();
        if (knownMentions.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen omtalar</h2><p>Når nokon skriv @namnet-ditt, kjem meldinga hit.</p></div>';
          return;
        }
        for (const mention of knownMentions) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = `${mention.message.sender_display_name} i ${mention.channel_name}`;
          const body = document.createElement("p");
          body.textContent = mention.message.body;
          const actions = document.createElement("div");
          actions.className = "onboarding-actions";
          const open = document.createElement("button");
          open.type = "button";
          open.textContent = "Opne samtalen";
          open.addEventListener("click", () => {
            const channel = knownChannels.find((item) => item.id === mention.message.channel_id);
            if (channel) selectChannel(channel);
          });
          actions.append(open);
          if (!mention.read) {
            const read = document.createElement("button");
            read.type = "button";
            read.textContent = "Marker lesen";
            read.addEventListener("click", () => sendCommand("mark_mention_read", { message_id: mention.message.id }));
            actions.append(read);
          }
          const task = document.createElement("button");
          task.type = "button";
          task.textContent = "Lag oppgåve";
          task.addEventListener("click", () => createTaskFromMention(mention, card));
          actions.append(task);
          card.append(heading, body, actions);
          if (!mention.read) card.dataset.unread = "true";
          messagesEl.append(card);
        }
      }

      function createTaskFromMention(mention, card) {
        if (card.querySelector(".task-editor")) return;
        const editor = document.createElement("form");
        editor.className = "task-editor";
        const title = document.createElement("input");
        title.required = true;
        title.maxLength = 240;
        title.setAttribute("aria-label", "Oppgåvetittel");
        title.value = mention.message.body.replace(/@\S+/g, "").trim();
        const process = document.createElement("input");
        process.setAttribute("aria-label", "Heart-prosess-ID");
        process.placeholder = "Heart-prosess-ID (valfritt)";
        const save = document.createElement("button");
        save.type = "submit";
        save.textContent = "Lagre oppgåve";
        editor.append(title, process, save);
        editor.addEventListener("submit", (event) => {
          event.preventDefault();
          if (!title.value.trim()) return;
          sendCommand("create_task", {
            source_message_id: mention.message.id,
            assignee_id: currentParticipantId,
            title: title.value.trim(),
            process_link_id: process.value.trim() || null
          });
        });
        card.append(editor);
        title.focus();
      }

      function renderTaskInbox() {
        if (conversationTitle.textContent !== "Oppgåver") return;
        messagesEl.replaceChildren();
        if (knownTasks.length === 0) {
          messagesEl.innerHTML = '<div class="empty-state"><h2>Ingen oppgåver</h2><p>Du kan gjere ei @omtale om til ei oppgåve.</p></div>';
          return;
        }
        for (const task of knownTasks) {
          const card = document.createElement("article");
          card.className = "message";
          const heading = document.createElement("strong");
          heading.textContent = task.title;
          const details = document.createElement("p");
          details.textContent = `${task.channel_name}${task.process_link_id ? ` · Heart ${task.process_link_id}` : ""}`;
          const toggle = document.createElement("button");
          toggle.type = "button";
          toggle.textContent = task.status === "done" ? "Opne igjen" : "Ferdig";
          toggle.addEventListener("click", () => sendCommand("set_task_done", {
            task_id: task.id, done: task.status !== "done"
          }));
          card.append(heading, details, toggle);
          if (task.status === "done") card.dataset.done = "true";
          messagesEl.append(card);
        }
      }

      function selectChannel(channel) {
        if (!channel) return;
        if (channel.id === activeChannelId && channel.id === subscribedChannelId) return;
        sidebar.classList.remove("mobile-open");
        mobileNavigationToggle.setAttribute("aria-expanded", "false");
        const previousChannelId = subscribedChannelId;
        if (previousChannelId) sendCommand("unsubscribe_channel", { channel_id: previousChannelId });
        subscribedChannelId = null;
        timeline.length = 0;
        seenMessageIds.clear();
        historyRequestIds.clear();
        historyHasMore = false;
        historyLoading = false;
        messagesEl.replaceChildren();
        activeChannelId = channel.id;
        restoredChannelId = channel.id;
        try { window.localStorage.setItem(activeConversationKey, channel.id); } catch (_) {}
        reconnectScrollOffset = null;
        closeMentionSuggestions();
        if (channel.circle_id) sendCommand("list_circle_users", { circle_id: channel.circle_id });
        sendCommand("list_channel_reactions", { channel_id: channel.id });
        renderMediaPreviews();
        requestedChannelSlug = channel.slug;
        renderConversationIdentity();
        renderChannels();
        updateAgentAccessControls();
        bodyInput.disabled = true;
        sendButton.disabled = true;
        setConnectionStatus("Koplar til samtalen …");
        if (!sendCommand("subscribe_channel", { channel_id: channel.id })) {
          setConnectionStatus("Vent på samband – trykk på samtalen for å prøve igjen");
        }
      }

      function updateAgentAccessControls() {
        const channel = knownChannels.find((item) => item.id === activeChannelId);
        const canDelegate = channel?.role === "owner" || channel?.role === "moderator";
        createAgentAccessButton.disabled = !canDelegate || temporaryAgentId !== null;
        if (!activeChannelId && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Vel ei samtale for å lage tilgang.";
        } else if (!canDelegate && temporaryAgentId === null) {
          agentAccessNotice.textContent = "Berre eigarar og moderatorar kan gi agenttilgang til denne samtalen.";
        } else if (temporaryAgentId === null) {
          agentAccessNotice.textContent = "Klar til å lage kortliva agenttilgang for denne samtalen.";
        }
      }

      async function agentApi(path, body) {
        const response = await fetch(path, {
          method: "POST",
          credentials: "same-origin",
          headers: { "accept": "application/json", "content-type": "application/json" },
          body: body === undefined ? undefined : JSON.stringify(body)
        });
        if (!response.ok) throw new Error(await response.text() || `HTTP ${response.status}`);
        if (response.status === 204) return null;
        return response.json();
      }

      async function createTemporaryAgentAccess() {
        if (!activeChannelId || temporaryAgentId !== null) return;
        createAgentAccessButton.disabled = true;
        agentAccessNotice.textContent = "Lagar kortliva agenttilgang …";
        const expiresAt = new Date(Date.now() + 30 * 60_000).toISOString();
        let created = null;
        try {
          created = await agentApi("/api/v1/agents", {
            display_name: "Kortliva MCP-agent",
            provider: "sproyt-owner-ui",
            service_identity: crypto.randomUUID(),
            purpose: `Kortliva MCP-tilgang til kanal ${activeChannelId}`,
            rate_limit_per_minute: 30,
            expires_at: expiresAt
          });
          for (const scope of ["read_history", "send_messages"]) {
            await agentApi(`/api/v1/agents/${created.agent_id}/grants`, {
              circle_id: null,
              channel_id: activeChannelId,
              scope,
              expires_at: expiresAt
            });
          }
          temporaryAgentId = created.agent_id;
          agentCredential.value = created.credential;
          agentCredential.hidden = false;
          copyAgentCredentialButton.hidden = false;
          revokeAgentAccessButton.hidden = false;
          agentAccessNotice.textContent = `Tilgangen ${created.agent_id} er klar i 30 minutt. Kopier credentialen no, og trekk han tilbake når testen er ferdig.`;
        } catch (error) {
          if (created?.agent_id) {
            await agentApi(`/api/v1/agents/${created.agent_id}/revoke`).catch(() => {});
          }
          agentAccessNotice.textContent = `Kunne ikkje lage agenttilgang: ${error.message}`;
          updateAgentAccessControls();
        }
      }

      async function revokeTemporaryAgentAccess() {
        if (!temporaryAgentId) return;
        revokeAgentAccessButton.disabled = true;
        try {
          await agentApi(`/api/v1/agents/${temporaryAgentId}/revoke`);
          temporaryAgentId = null;
          agentCredential.value = "";
          agentCredential.hidden = true;
          copyAgentCredentialButton.hidden = true;
          revokeAgentAccessButton.hidden = true;
          revokeAgentAccessButton.disabled = false;
          updateAgentAccessControls();
          agentAccessNotice.textContent = "Agenttilgangen er trekt tilbake.";
        } catch (error) {
          revokeAgentAccessButton.disabled = false;
          agentAccessNotice.textContent = `Kunne ikkje trekkje tilbake agenttilgangen: ${error.message}`;
        }
      }

      function updateLatestSequence(channelId, sequence) {
        const channel = knownChannels.find((item) => item.id === channelId);
        if (channel) channel.latest_sequence = Math.max(channel.latest_sequence || 0, sequence);
      }

      function acknowledgeLatest(channelId, messages) {
        if (channelId !== activeChannelId || messages.length === 0 || document.visibilityState === "hidden") return;
        const sequence = messages.at(-1).sequence;
        updateLatestSequence(channelId, sequence);
        sendCommand("mark_read", { channel_id: channelId, sequence });
      }

      document.addEventListener("visibilitychange", () => {
        if (document.visibilityState !== "visible") return;
        resumeAfterBackground();
        if (!activeChannelId) return;
        const visibleMessages = timeline
          .filter((item) => item.type === "message" && item.message.channel_id === activeChannelId)
          .map((item) => item.message);
        acknowledgeLatest(activeChannelId, visibleMessages);
      });

      function pushSystem(text) {
        timeline.push({ type: "system", text });
        renderTimeline();
      }

      function loadOlderHistory() {
        if (!activeChannelId || !historyHasMore || historyLoading || subscribedChannelId !== activeChannelId) return;
        const oldest = timeline.find((item) => item.type === "message")?.message;
        if (!oldest) return;
        historyLoading = true;
        const requestId = sendCommand("load_recent_messages", {
          channel_id: activeChannelId,
          before: oldest.sequence,
          limit: historyPageSize
        });
        if (requestId) historyRequestIds.add(requestId);
        else historyLoading = false;
      }

      function renderTimeline({ preserveScroll = false, forceBottom = false } = {}) {
        const previousHeight = messagesEl.scrollHeight;
        const previousTop = messagesEl.scrollTop;
        const wasNearBottom = previousHeight - previousTop - messagesEl.clientHeight < 80;
        messagesEl.replaceChildren();
        for (const item of timeline) {
          if (item.type === "message") {
            appendMessage(item.message);
          } else {
            appendSystem(item.text);
          }
        }
        renderMermaidDiagrams();
        if (preserveScroll) {
          messagesEl.scrollTop = messagesEl.scrollHeight - previousHeight + previousTop;
        } else if (forceBottom) {
          settleConversationAtBottom();
        } else if (wasNearBottom) {
          messagesEl.scrollTop = messagesEl.scrollHeight;
        }
      }

      function settleConversationAtBottom() {
        const scroll = () => { messagesEl.scrollTop = messagesEl.scrollHeight; };
        scroll();
        requestAnimationFrame(() => {
          scroll();
          sendForm.scrollIntoView({ block: "nearest" });
          requestAnimationFrame(scroll);
        });
        window.setTimeout(scroll, 150);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", scroll, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", scroll, { once: true });
        });
      }

      function restoreConversationScrollOffset(offset) {
        const restore = () => {
          messagesEl.scrollTop = Math.max(0, messagesEl.scrollHeight - messagesEl.clientHeight - offset);
        };
        restore();
        requestAnimationFrame(restore);
        messagesEl.querySelectorAll("img").forEach((image) => {
          if (!image.complete) image.addEventListener("load", restore, { once: true });
        });
        messagesEl.querySelectorAll("video").forEach((video) => {
          if (video.readyState < 1) video.addEventListener("loadedmetadata", restore, { once: true });
        });
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

      function prependTimelineMessages(messages) {
        const older = [];
        for (const message of messages) {
          if (seenMessageIds.has(message.id)) continue;
          seenMessageIds.add(message.id);
          older.push({ type: "message", message });
        }
        timeline.unshift(...older);
      }

      function replaceChannelReactions(reactions) {
        messageReactions.clear();
        for (const reaction of reactions) {
          if (!messageReactions.has(reaction.message_id)) messageReactions.set(reaction.message_id, new Map());
          messageReactions.get(reaction.message_id).set(reaction.emoji, {
            count: reaction.count,
            reactedByMe: reaction.reacted_by_me
          });
        }
      }

      function applyReactionChange(change) {
        if (!messageReactions.has(change.message_id)) messageReactions.set(change.message_id, new Map());
        const reactions = messageReactions.get(change.message_id);
        const current = reactions.get(change.emoji) || { count: 0, reactedByMe: false };
        current.count = change.count;
        if (change.user_id === currentParticipantId) current.reactedByMe = change.added;
        if (current.count === 0) reactions.delete(change.emoji);
        else reactions.set(change.emoji, current);
        if (reactions.size === 0) messageReactions.delete(change.message_id);
      }

      function reactionButton(messageId, emoji, reaction) {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "reaction-badge";
        button.setAttribute("aria-pressed", String(reaction.reactedByMe));
        button.setAttribute("aria-label", `${emoji}: ${reaction.count} reaksjonar`);
        button.textContent = `${emoji} ${reaction.count}`;
        button.addEventListener("click", () => sendCommand("toggle_message_reaction", {
          message_id: messageId, emoji
        }));
        return button;
      }

      function renderMessageReactions(message) {
        const bar = document.createElement("div");
        bar.className = "message-reactions";
        const reactions = messageReactions.get(message.id) || new Map();
        for (const emoji of reactionEmojis) {
          const reaction = reactions.get(emoji);
          if (reaction?.count > 0) bar.append(reactionButton(message.id, emoji, reaction));
        }
        const picker = document.createElement("details");
        picker.className = "reaction-picker";
        const summary = document.createElement("summary");
        summary.setAttribute("aria-label", "Legg til reaksjon");
        summary.textContent = "😊 +";
        const choices = document.createElement("div");
        for (const emoji of reactionEmojis) {
          const button = document.createElement("button");
          button.type = "button";
          button.textContent = emoji;
          button.setAttribute("aria-label", `Reager med ${emoji}`);
          button.addEventListener("click", () => {
            sendCommand("toggle_message_reaction", { message_id: message.id, emoji });
            picker.open = false;
          });
          choices.append(button);
        }
        picker.append(summary, choices);
        bar.append(picker);
        return bar;
      }

      function appendMessage(message) {
        const wrapper = document.createElement("article");
        wrapper.className = "message";

        const meta = document.createElement("div");
        meta.className = "meta";
        const sender = message.sender_id === currentParticipantId
          ? "Du"
          : (message.sender_display_name || "Ein ven");
        const senderLabel = document.createElement("span");
        senderLabel.textContent = sender;
        appendProfileStatus(senderLabel, message.sender_id);
        meta.append(senderLabel);
        const sentAt = new Date(message.sent_at);
        if (!Number.isNaN(sentAt.valueOf())) {
          const timestamp = document.createElement("time");
          timestamp.dateTime = sentAt.toISOString();
          timestamp.title = sentAt.toLocaleString([], { dateStyle: "full", timeStyle: "short" });
          timestamp.textContent = ` · ${formatMessageTimestamp(sentAt)}`;
          meta.append(timestamp);
        }

        const body = document.createElement("div");
        if (renderMode === "raw") {
          const pre = document.createElement("pre");
          pre.className = "raw-body";
          pre.textContent = message.body;
          body.append(pre);
        } else {
          body.className = "rendered";
          renderMessageBody(message.body, body);
        }

        wrapper.append(meta, body, renderMessageReactions(message));
        messagesEl.append(wrapper);
      }

      function formatMessageTimestamp(sentAt, now = new Date()) {
        const sameDay = sentAt.getFullYear() === now.getFullYear()
          && sentAt.getMonth() === now.getMonth()
          && sentAt.getDate() === now.getDate();
        if (sameDay) return sentAt.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
        const options = { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" };
        if (sentAt.getFullYear() !== now.getFullYear()) options.year = "numeric";
        return sentAt.toLocaleString([], options);
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

      connect();
      const invitationFromUrl = new URL(window.location.href).searchParams.get("invite");
      if (invitationFromUrl) {
        invitationToken.value = window.location.href;
        onboardingNotice.textContent = "Du er invitert til ein vennekrets. Trykk «Bli med» for å godta.";
        updateOnboardingButtons();
      }

      function renderMessageBody(source, target) {
        const token = /\[\[media:([0-9a-f-]{36})\|([^|\]]+)\|([^\]]*)\]\]/gi;
        const attachments = [];
        const text = source.replace(token, (_, id, contentType, encodedName) => {
          let name = "media";
          try { name = decodeURIComponent(encodedName || "media"); } catch (_) {}
          attachments.push({ id, contentType, name });
          return "";
        }).trim();
        if (text) renderMarkdown(text, target);
        attachments.forEach((media) => {
          const figure = document.createElement("figure");
          figure.className = "message-media";
          const element = media.contentType.startsWith("video/") ? document.createElement("video") : document.createElement("img");
          const participant = new URL(window.location.href).searchParams.get("participant");
          const authQuery = participant ? `?participant=${encodeURIComponent(participant)}` : "";
          const originalUrl = `/api/v1/media/${media.id}${authQuery}`;
          element.src = media.contentType.startsWith("image/")
            ? `/api/v1/media/${media.id}/preview${authQuery}`
            : originalUrl;
          if (element instanceof HTMLVideoElement) { element.controls = true; element.preload = "metadata"; }
          else {
            element.alt = media.name;
            element.loading = "lazy";
            element.tabIndex = 0;
            element.setAttribute("role", "button");
            element.setAttribute("aria-label", `Vis ${media.name} i full storleik`);
            const open = () => openMediaLightbox(originalUrl, media.name);
            element.addEventListener("click", open);
            element.addEventListener("keydown", (event) => {
              if (event.key === "Enter" || event.key === " ") { event.preventDefault(); open(); }
            });
          }
          const caption = document.createElement("figcaption");
          caption.textContent = media.name;
          figure.append(element, caption);
          target.append(figure);
        });
      }

      function openMediaLightbox(url, name) {
        mediaLightboxImage.src = url;
        mediaLightboxImage.alt = name;
        mediaLightboxCaption.textContent = name;
        mediaLightbox.showModal();
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
        if (diagrams.length === 0) return;
        if (mermaidPromise === null) {
          mermaidPromise = import("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs")
            .then(({ default: mermaid }) => {
              mermaid.initialize({
                startOnLoad: false,
                securityLevel: "strict",
                theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
              });
              return mermaid;
            });
        }
        let mermaid;
        try {
          mermaid = await mermaidPromise;
        } catch (_) {
          diagrams.forEach((diagram) => { diagram.textContent = "Diagrammet kunne ikkje lastast."; });
          return;
        }
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
        assert!(INDEX_HTML.contains("id=\"upload-status\""));
        assert!(INDEX_HTML.contains("request.upload.addEventListener(\"progress\""));
        assert!(INDEX_HTML.contains("Behandlar fila"));
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
        let nonce = policy
            .split("script-src 'nonce-")
            .nth(1)
            .unwrap()
            .split('\'')
            .next()
            .unwrap()
            .to_owned();
        let body = first.text().await.unwrap();
        assert!(body.contains(&format!("<script type=\"module\" nonce=\"{nonce}\">")));
        assert!(body.contains(&format!("<style nonce=\"{nonce}\">")));
        assert!(
            body.contains("https://cdn.jsdelivr.net/npm/mermaid@11.16.0/dist/mermaid.esm.min.mjs")
        );
        assert!(!body.contains("import mermaid from"));
        assert!(body.contains("mermaidPromise = import("));
        assert!(!body.contains("npm/mermaid@11/dist/"));
        assert!(!body.contains("{{NONCE}}"));
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
        assert!(body.contains("let subscribedChannelId = null"));
        assert!(body.contains("subscribedChannelId === activeChannelId"));
        assert!(
            body.contains("channel.id === activeChannelId && channel.id === subscribedChannelId")
        );
        assert!(body.contains("payload.channel_id !== activeChannelId"));
        assert!(body.contains("const pendingMessages = new Map()"));
        assert!(body.contains("id=\"channel-kind\""));
        assert!(body.contains("id=\"joinable-channel\""));
        assert!(body.contains("id=\"add-channel-member\""));
        assert!(body.contains("sendCommand(\"list_joinable_channels\""));
        assert!(body.contains("sendCommand(\"add_channel_member\""));
        assert!(body.contains("const browserSessionId = `browser-${crypto.randomUUID()}`"));
        assert!(body.contains("request_id: `${browserSessionId}-${requestNumber}`"));
        assert!(!body.contains("request_id: `browser-${requestNumber}`"));
        assert!(body.contains("if (event.type === \"message_accepted\")"));
        assert!(body.contains("finishPendingMessage(event.request_id, payload.message)"));
        assert!(body.contains("message?.channel_id !== pending.channelId"));
        assert!(body.contains("message?.body !== pending.body"));
        assert!(body.contains("failPendingMessage(event.request_id"));
        assert!(!body.contains(
            "sendCommand(\"send_message\", { channel_id: activeChannelId, body });\n        bodyInput.value = \"\";"
        ));
        assert!(body.contains("class=\"advanced-tools\" hidden"));
        assert!(body.contains("<details class=\"agent-access\" hidden>"));
        assert!(body.contains("<summary>Agenttilgang</summary>"));
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
        assert!(body.contains("stableConnectionTimer = window.setTimeout"));
        assert!(body.contains("event.code === 1008"));
        assert!(body.contains("recoverAuthentication().catch"));
        assert!(body.contains("async function recoverConnection(replaceOpenSocket = false)"));
        assert!(body.contains("response.status === 401"));
        assert!(body.contains("connect(true, socket)"));
        assert!(body.contains("recoverConnection().catch(() => scheduleReconnect"));
        assert!(body.contains("fetch(\"/auth/session\""));
        assert!(body.contains("scheduleInitialSessionRefresh()"));
        assert!(body.contains("sessionRefreshDueAt = Date.now() + delay"));
        assert!(body.contains("window.addEventListener(\"pageshow\", resumeAfterBackground)"));
        assert!(body.contains("window.addEventListener(\"online\", resumeAfterBackground)"));
        assert!(body.contains("reconnectAfterSessionRefresh()"));
        assert!(body.contains("connect(true)"));
        assert!(body.contains("connect(true, socket)"));
        assert!(body.contains("const next = current || restored || requested || knownChannels[0]"));
        assert!(body.contains("window.localStorage.setItem(activeConversationKey, channel.id)"));
        assert!(body.contains("let reconnectScrollOffset = null"));
        assert!(body.contains("restoreConversationScrollOffset(scrollOffset)"));
        assert!(body.contains("previousSocket.close(4000, \"session refreshed\")"));
        assert!(!body.contains("sessionRefreshReconnect"));
        assert!(!body.contains("if (response.status === 401) {\n          window.location.assign"));
        assert!(body.contains("Fråkopla (${detail})"));
        assert!(body.contains("function acknowledgeLatest(channelId, messages)"));
        assert!(body.contains("function loadOlderHistory()"));
        assert!(body.contains("before: oldest.sequence"));
        assert!(body.contains("renderTimeline({ preserveScroll: true })"));
        assert!(body.contains(
            "renderTimeline({ forceBottom: scrollOffset === null || scrollOffset < 80 })"
        ));
        assert!(body.contains("function settleConversationAtBottom()"));
        assert!(body.contains("sendForm.scrollIntoView({ block: \"nearest\" })"));
        assert!(body.contains("function formatMessageTimestamp(sentAt, now = new Date())"));
        assert!(body.contains("dateStyle: \"full\", timeStyle: \"short\""));
        assert!(body.contains("appendProfileStatus(senderLabel, message.sender_id)"));
        assert!(body.contains("channel.direct_user_id"));
        assert!(body.contains("function approximateUnreadCount(count)"));
        assert!(body.contains("if (count < 50) return \"25+\""));
        assert!(body.contains("if (count < 100) return \"50+\""));
        assert!(body.contains("groupUnreadCount = channels.reduce"));
        assert!(body.contains("heading.classList.add(\"has-unread\")"));
        assert!(body.contains("button.classList.add(\"has-unread\")"));
        assert!(body.contains("document.addEventListener(\"visibilitychange\""));
        assert!(body.contains(":focus-visible"));
        assert!(body.contains("id=\"mobile-navigation-toggle\""));
        assert!(body.contains("aria-controls=\"mobile-navigation mobile-onboarding\""));
        assert!(body.contains(".sidebar.mobile-open nav, .sidebar.mobile-open .onboarding"));
        assert!(body.contains(".sidebar.mobile-open .identity { display: grid; }"));
        assert!(body.contains("grid-template-rows: auto auto minmax(0, 1fr) auto;"));
        assert!(body.contains("form.send { grid-template-columns: auto auto minmax(0, 1fr) auto"));
        assert!(body.contains(".conversation-header .status[data-routine=\"true\"]"));
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
}
