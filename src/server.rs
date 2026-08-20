use std::time::Duration;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::HeaderName,
    middleware,
    routing::{get, post},
};
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    agent::AgentService,
    auth::AuthService,
    chat::ChatEngine,
    config::{AppConfig, AuthMode, LogFormat},
    db,
    notification::NotificationService,
    operations::{OperationalState, healthz, metrics, record_metrics},
    process::{HeartGateway, ProcessService, SharedProcessGateway},
    web::account::{
        export_my_data, notification_settings, record_client_event, save_notification_preferences,
        subscribe_push, unsubscribe_push,
    },
    web::agents::{
        approve_agent_message, create_agent, grant_agent, revoke_agent, revoke_agent_grant,
    },
    web::assets::{
        app_bundle, client_store, client_store_legacy, offline_page, pwa_manifest, service_worker,
        wave_logo_192, wave_logo_512, wave_logo_svg,
    },
    web::auth::{auth_callback, auth_login, auth_logout, auth_refresh, auth_session},
    web::browser::index,
    web::mcp::mcp_handler,
    web::media::{download_media, download_media_preview, upload_media},
    web::processes::{
        correlate_process, get_process, inspect_process, set_heart_feature, start_process,
    },
    web::socket::ws_handler,
    web::system::{add_security_headers, app_readyz, versionz},
};

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) auth: AuthService,
    pub(super) chat: ChatEngine,
    pub(super) operations: OperationalState,
    pub(super) processes: ProcessService,
    pub(super) agents: AgentService,
    pub(super) notifications: NotificationService,
    pub(super) websocket_idle_timeout: Duration,
    pub(super) advanced_ui_enabled: bool,
    pub(super) agent_ui_enabled: bool,
}

impl axum::extract::FromRef<AppState> for OperationalState {
    fn from_ref(state: &AppState) -> Self {
        state.operations.clone()
    }
}

pub(super) async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

pub(super) fn build_router(state: AppState, operations: OperationalState) -> Router {
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
        .route("/assets/app/{revision}/app.js", get(app_bundle))
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

fn process_gateway_from_env() -> Result<Option<SharedProcessGateway>, crate::process::ProcessError>
{
    let Some(url) = std::env::var("SPROYT_HEART_URL").ok() else {
        return Ok(None);
    };
    let gateway = HeartGateway::new(url, Duration::from_secs(5), 2)?;
    Ok(Some(std::sync::Arc::new(gateway)))
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
