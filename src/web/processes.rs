use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, header::COOKIE},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    domain::ChannelId,
    process::{
        BindProcessApplication, ConfigureApplication, ConfigureApplicationProcessor,
        ConfigureProcessBinding, ConfigureProcessRole, ConfigureTaskRoute, EnqueueCorrelation,
        EnqueueInspection, EnqueueProcessStart, ProcessLinkId, SetCircleFeature,
    },
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};

#[derive(Deserialize)]
pub(crate) struct StartProcessRequest {
    channel_id: String,
    request_id: String,
    namespace: String,
    definition_name: String,
    definition_version: Option<String>,
    #[serde(default)]
    metadata: serde_json::Value,
}

pub(crate) async fn start_process(
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
pub(crate) struct CorrelateProcessRequest {
    request_id: String,
    #[serde(default)]
    payload: serde_json::Value,
}

pub(crate) async fn correlate_process(
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

pub(crate) async fn get_process(
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
pub(crate) struct InspectProcessRequest {
    request_id: String,
}

pub(crate) async fn inspect_process(
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
pub(crate) struct SetFeatureRequest {
    enabled: bool,
}

pub(crate) async fn set_heart_feature(
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

#[derive(Deserialize)]
pub(crate) struct ProcessBindingRequest {
    process_key: String,
    namespace: String,
    definition_name: String,
    definition_version: String,
    enabled: bool,
}

pub(crate) async fn configure_process_binding(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ProcessBindingRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .configure_process_binding(ConfigureProcessBinding {
            channel_id,
            actor: principal.user.id,
            process_key: body.process_key,
            namespace: body.namespace,
            definition_name: body.definition_name,
            definition_version: body.definition_version,
            enabled: body.enabled,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConfigureApplicationRequest {
    key: String,
    name: String,
    enabled: bool,
}

pub(crate) async fn configure_application(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ConfigureApplicationRequest>,
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
        .configure_application(ConfigureApplication {
            actor: principal.user.id,
            circle_id,
            key: body.key,
            name: body.name,
            enabled: body.enabled,
        })
        .await
    {
        Ok(application) => Json(application).into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct BindApplicationRequest {
    process_key: String,
    application_id: uuid::Uuid,
    enabled: bool,
}

pub(crate) async fn bind_process_application(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<BindApplicationRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .bind_process_application(BindProcessApplication {
            actor: principal.user.id,
            channel_id,
            process_key: body.process_key,
            application_id: body.application_id,
            enabled: body.enabled,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct ListProcessApplicationsQuery {
    process_key: String,
    #[serde(flatten)]
    auth: WsQuery,
}

pub(crate) async fn list_process_applications(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ListProcessApplicationsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query.auth, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let channel_id = match ChannelId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .list_process_applications(principal.user.id, channel_id, query.process_key)
        .await
    {
        Ok(applications) => Json(applications).into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConfigureProcessorRequest {
    user_id: String,
    can_review: bool,
    can_export: bool,
    can_start_development: bool,
}

pub(crate) async fn configure_application_processor(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ConfigureProcessorRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let application_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid application id",
            )
                .into_response();
        }
    };
    let user_id = match crate::domain::UserId::new(body.user_id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .configure_application_processor(ConfigureApplicationProcessor {
            actor: principal.user.id,
            application_id,
            user_id,
            can_review: body.can_review,
            can_export: body.can_export,
            can_start_development: body.can_start_development,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConfigureTaskRouteRequest {
    process_key: String,
    task_channel_id: String,
    task_key: String,
    process_role: String,
    enabled: bool,
}

pub(crate) async fn configure_task_route(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ConfigureTaskRouteRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let source_channel_id = match ChannelId::new(id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    let task_channel_id = match ChannelId::new(body.task_channel_id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .configure_task_route(ConfigureTaskRoute {
            actor: principal.user.id,
            source_channel_id,
            process_key: body.process_key,
            task_channel_id,
            task_key: body.task_key,
            process_role: body.process_role,
            enabled: body.enabled,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}

#[derive(Deserialize)]
pub(crate) struct ConfigureProcessRoleRequest {
    user_id: String,
    process_role: String,
    enabled: bool,
}

pub(crate) async fn configure_process_role(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(body): Json<ConfigureProcessRoleRequest>,
) -> axum::response::Response {
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    let application_id = match uuid::Uuid::parse_str(&id) {
        Ok(id) => id,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid application id",
            )
                .into_response();
        }
    };
    let user_id = match crate::domain::UserId::new(body.user_id) {
        Ok(id) => id,
        Err(error) => {
            return (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response();
        }
    };
    match state
        .processes
        .configure_process_role(ConfigureProcessRole {
            actor: principal.user.id,
            application_id,
            user_id,
            process_role: body.process_role,
            enabled: body.enabled,
        })
        .await
    {
        Ok(()) => axum::http::StatusCode::NO_CONTENT.into_response(),
        Err(error) => repository_response(error),
    }
}
