use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{
    notification::{NotificationPreferences, PushSubscriptionInput},
    operations::ClientEvent,
    server::AppState,
    web::http::{
        WsQuery, auth_error_response, authenticate_http, chat_error_response, repository_response,
    },
};

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
pub(crate) struct ClientEventReport {
    event: ClientEventInput,
}

pub(crate) async fn record_client_event(
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

pub(crate) async fn export_my_data(
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

pub(crate) async fn notification_settings(
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

pub(crate) async fn save_notification_preferences(
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

pub(crate) async fn subscribe_push(
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
pub(crate) struct PushUnsubscribe {
    endpoint: String,
}

pub(crate) async fn unsubscribe_push(
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
