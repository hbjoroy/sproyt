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
mod server;
mod web;
mod ws;

use server::AppState;
#[cfg(test)]
use server::build_router;
use web::assets::{BUILD_REVISION, CLIENT_STORE, INDEX_HTML, client_store_fingerprint};

use std::{io::Cursor, time::Duration};

use axum::{
    Json,
    extract::{Multipart, Path, Query, Request, State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{ACCEPT, AUTHORIZATION, COOKIE, LOCATION, ORIGIN, SET_COOKIE},
    },
    middleware,
    response::{Html, IntoResponse},
};
#[cfg(test)]
use axum::{Router, routing::post};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{
    agent::{AgentPrincipal, AgentScope, CreateAgent, GrantAgent},
    auth::AuthError,
    chat::ChatError,
    domain::{
        ChannelId, ChannelSequence, MediaId, MediaUpload, MediaVariant, MessageBody, MessageLimit,
        UserId,
    },
    notification::{NotificationPreferences, PushSubscriptionInput},
    operations::{ClientEvent, healthz, metrics, record_metrics},
    process::{
        EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, ProcessLinkId, SetCircleFeature,
    },
};
#[cfg(test)]
use crate::{
    auth::AuthService,
    chat::ChatEngine,
    notification::NotificationService,
    operations::OperationalState,
    process::{HeartGateway, SharedProcessGateway},
};

#[cfg(test)]
use web::assets::{PWA_MANIFEST, SERVICE_WORKER, WAVE_LOGO_192, WAVE_LOGO_512};

#[derive(Serialize)]
struct VersionInfo {
    service: &'static str,
    version: &'static str,
    revision: &'static str,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    server::run().await
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

#[cfg(test)]
#[path = "main_tests/mcp.rs"]
mod mcp_tests;

#[cfg(test)]
#[path = "main_tests/protocol_capacity.rs"]
mod protocol_capacity_tests;
