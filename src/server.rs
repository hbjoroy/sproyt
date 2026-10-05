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
    enrollment::EnrollmentService,
    integration::IntegrationService,
    notification::NotificationService,
    operations::{OperationalState, healthz, metrics, record_metrics},
    process::{HeartGateway, ProcessService, SharedProcessGateway},
    web::account::{
        add_saved_emoji, disable_channel_notifications, enable_channel_notifications,
        export_my_data, notification_settings, record_client_event, remove_saved_emoji,
        save_notification_preferences, saved_emojis, subscribe_push, unsubscribe_push,
    },
    web::agents::{
        approve_agent_message, create_agent, grant_agent, revoke_agent, revoke_agent_grant,
    },
    web::assets::{
        app_bundle, client_core_wasm, client_store, client_store_legacy, offline_page,
        pwa_manifest, service_worker, wave_logo_192, wave_logo_512,
    },
    web::auth::{
        auth_callback, auth_login, auth_logout, auth_refresh, auth_session, share_identity,
    },
    web::browser::index,
    web::enrollment::{create_enrollment_invitation, create_global_enrollment_invitation},
    web::integrations::{
        create_grafana_integration, receive_grafana_alerts, receive_report,
        rotate_integration_credential,
    },
    web::mcp::mcp_handler,
    web::media::{download_media, download_media_attachment, download_media_preview, upload_media},
    web::processes::{
        bind_process_application, configure_application, configure_application_processor,
        configure_process_binding, configure_process_role, configure_task_route, correlate_process,
        get_process, inspect_process, list_process_applications, set_heart_feature, start_process,
    },
    web::socket::ws_handler,
    web::stream::{command_handler, events_handler},
    web::system::{add_security_headers, app_readyz, versionz},
};

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) auth: AuthService,
    pub(super) chat: ChatEngine,
    pub(super) operations: OperationalState,
    pub(super) processes: ProcessService,
    pub(super) agents: AgentService,
    pub(super) chat_agents: Option<crate::chatbot::CircleChatAgents>,
    pub(super) integrations: IntegrationService,
    pub(super) notifications: NotificationService,
    pub(super) imagegen: Option<crate::imagegen::ImageGeneration>,
    pub(super) process_pilot: Option<crate::process_pilot::ProcessPilot>,
    pub(super) work_items: Option<crate::work_items::WorkItems>,
    pub(super) enrollment: Option<EnrollmentService>,
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
    let postgres_pool =
        db::connect_postgres_pool(config.database(), config.db_max_connections()).await?;
    let repositories = db::connect_repositories(config.database(), postgres_pool.as_ref()).await?;
    let notifications =
        NotificationService::connect(config.database(), postgres_pool.as_ref()).await?;
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
    let imagegen =
        crate::imagegen::ImageGeneration::from_env(config.database(), postgres_pool.as_ref())
            .await?;
    if let Some(service) = &imagegen {
        service.start_worker(operations.subscribe_shutdown());
    }
    let chat = ChatEngine::start(repositories.chat);
    let chat_agents =
        crate::chatbot::CircleChatAgents::from_env(config.database(), postgres_pool.as_ref())
            .await?;
    chat_agents.start_worker(chat.clone(), operations.subscribe_shutdown());
    let process_pilot =
        crate::process_pilot::ProcessPilot::from_env(config.database(), postgres_pool.as_ref())
            .await?;
    if let Some(pilot) = &process_pilot {
        pilot.start_worker(chat.clone(), operations.subscribe_shutdown());
    }
    let work_items =
        crate::work_items::WorkItems::from_env(config.database(), postgres_pool.as_ref()).await?;
    work_items.start_worker(chat.clone(), operations.subscribe_shutdown());
    let state = AppState {
        process_pilot,
        work_items: Some(work_items),
        imagegen,
        auth,
        chat,
        operations: operations.clone(),
        processes: ProcessService::start(repositories.process, process_gateway_from_env()?),
        agents: AgentService::new(repositories.agent),
        chat_agents: Some(chat_agents),
        integrations: IntegrationService::new(repositories.integration),
        notifications,
        enrollment: EnrollmentService::from_env()?,
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
        .route(
            "/assets/client-core/{revision}/client-core.wasm",
            get(client_core_wasm),
        )
        .route("/service-worker.js", get(service_worker))
        .route("/offline", get(offline_page))
        .route("/assets/sproyt-wave-icon-192.png", get(wave_logo_192))
        .route("/assets/sproyt-wave-icon-512.png", get(wave_logo_512))
        .route("/metrics", get(metrics))
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/session", get(auth_session))
        .route("/auth/share-identity", get(share_identity))
        .route("/share-target", get(index))
        .route("/auth/refresh", post(auth_refresh))
        .route("/auth/logout", get(auth_logout))
        .route("/api/v1/me/export", get(export_my_data))
        .route(
            "/api/v1/me/emojis",
            get(saved_emojis)
                .post(add_saved_emoji)
                .delete(remove_saved_emoji),
        )
        .route("/api/v1/client-events", post(record_client_event))
        .route(
            "/api/v1/me/notifications",
            get(notification_settings).put(save_notification_preferences),
        )
        .route(
            "/api/v1/me/push-subscriptions",
            post(subscribe_push).delete(unsubscribe_push),
        )
        .route(
            "/api/v1/channels/{id}/notifications",
            axum::routing::put(enable_channel_notifications).delete(disable_channel_notifications),
        )
        .route(
            "/api/v1/circles/{id}/enrollment-invitations",
            post(create_enrollment_invitation),
        )
        .route(
            "/api/v1/enrollment-invitations",
            post(create_global_enrollment_invitation),
        )
        .route("/api/v1/channels/{id}/media", post(upload_media))
        .route(
            "/api/v1/imagegen",
            get(crate::web::imagegen::list).post(crate::web::imagegen::enqueue),
        )
        .route(
            "/api/v1/imagegen/{id}/preview",
            get(crate::web::imagegen::preview),
        )
        .route(
            "/api/v1/imagegen/{id}/review",
            post(crate::web::imagegen::review),
        )
        .route("/api/v1/media/{id}", get(download_media))
        .route(
            "/api/v1/media/{id}/download",
            get(download_media_attachment),
        )
        .route("/api/v1/media/{id}/preview", get(download_media_preview))
        .route("/ws", get(ws_handler))
        .route(
            "/api/v1/commands",
            post(command_handler).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/v1/events", get(events_handler))
        .route("/api/v1/processes", post(start_process))
        .route(
            "/api/v1/circles/{id}/work-applications",
            post(configure_application),
        )
        .route(
            "/api/v1/work-applications/{id}/processors",
            post(configure_application_processor),
        )
        .route(
            "/api/v1/work-applications/{id}/process-roles",
            post(configure_process_role),
        )
        .route(
            "/api/v1/channels/{id}/task-routes",
            post(configure_task_route),
        )
        .route(
            "/api/v1/channels/{id}/process-applications",
            get(list_process_applications).post(bind_process_application),
        )
        .route(
            "/api/v1/channels/{id}/work-items/draft/{message_id}",
            get(crate::web::work_items::draft),
        )
        .route(
            "/api/v1/channels/{id}/work-items/applications",
            get(crate::web::work_items::applications),
        )
        .route(
            "/api/v1/channels/{id}/work-items",
            post(crate::web::work_items::register),
        )
        .route(
            "/api/v1/work-item-tasks/{id}",
            get(crate::web::work_items::task),
        )
        .route(
            "/api/v1/channels/{id}/work-items/source/{message_id}",
            get(crate::web::work_items::source_items),
        )
        .route(
            "/api/v1/work-items/{id}/supplements",
            post(crate::web::work_items::supplement),
        )
        .route(
            "/api/v1/work-item-tasks/{id}/decide",
            post(crate::web::work_items::decide),
        )
        .route(
            "/api/v1/work-item-tasks/{id}/github",
            post(crate::web::work_items::export_github),
        )
        .route(
            "/api/v1/work-items/{id}/status-change",
            post(crate::web::work_items::start_status),
        )
        .route(
            "/api/v1/work-item-tasks/{id}/status",
            post(crate::web::work_items::change_status),
        )
        .route(
            "/api/v1/work-items/{id}/status",
            get(crate::web::work_items::public_status),
        )
        .route(
            "/api/v1/channels/{id}/process-bindings",
            post(configure_process_binding),
        )
        .route(
            "/api/v1/channels/{id}/process-pilot",
            get(crate::web::process_pilot::configuration)
                .post(crate::web::process_pilot::configure),
        )
        .route(
            "/api/v1/channels/{id}/process-pilot/start",
            post(crate::web::process_pilot::start),
        )
        .route(
            "/api/v1/process-pilot/tasks/{id}",
            get(crate::web::process_pilot::task),
        )
        .route(
            "/api/v1/process-pilot/tasks/{id}/complete",
            post(crate::web::process_pilot::complete),
        )
        .route("/api/v1/processes/{id}", get(get_process))
        .route("/api/v1/processes/{id}/inspect", post(inspect_process))
        .route("/api/v1/processes/{id}/messages", post(correlate_process))
        .route(
            "/api/v1/circles/{id}/features/heart-event-planning",
            post(set_heart_feature),
        )
        .route("/api/v1/agents", post(create_agent))
        .route(
            "/api/v1/channels/{id}/chat-agents",
            get(crate::web::chatbot::list_channel),
        )
        .route(
            "/api/v1/channels/{id}/chat-agents/{agent_id}",
            axum::routing::patch(crate::web::chatbot::update_channel),
        )
        .route(
            "/api/v1/circles/{id}/chat-agents",
            get(crate::web::chatbot::list).post(crate::web::chatbot::create),
        )
        .route(
            "/api/v1/circles/{id}/chat-agents/{agent_id}",
            axum::routing::patch(crate::web::chatbot::update),
        )
        .route("/api/v1/agents/{id}/grants", post(grant_agent))
        .route("/api/v1/agents/{id}/revoke", post(revoke_agent))
        .route(
            "/api/v1/channels/{id}/integrations/grafana",
            post(create_grafana_integration),
        )
        .route(
            "/api/v1/integrations/{id}/rotate",
            post(rotate_integration_credential),
        )
        .route(
            "/api/v1/integrations/grafana/alerts",
            post(receive_grafana_alerts).layer(DefaultBodyLimit::max(256 * 1024)),
        )
        .route(
            "/api/v1/integrations/grafana/reports",
            post(receive_report).layer(DefaultBodyLimit::max(256 * 1024)),
        )
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
    if !process_outbox_enabled(
        std::env::var("SPROYT_PROCESS_OUTBOX_ENABLED")
            .ok()
            .as_deref(),
    ) {
        return Ok(None);
    }
    let Some(url) = std::env::var("SPROYT_HEART_URL").ok() else {
        return Ok(None);
    };
    let gateway = HeartGateway::new(url, Duration::from_secs(5), 2)?;
    Ok(Some(std::sync::Arc::new(gateway)))
}

fn process_outbox_enabled(value: Option<&str>) -> bool {
    value != Some("false")
}

#[cfg(test)]
#[test]
fn canary_can_disable_generic_process_worker_without_disabling_pilot() {
    assert!(!process_outbox_enabled(Some("false")));
    assert!(process_outbox_enabled(None));
    assert!(process_outbox_enabled(Some("true")));
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
