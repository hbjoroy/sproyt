//! Account-owned preferences only. This does not admit native credentials to
//! browser routes, channel subscriptions, push registration or media uploads.
use axum::{
    Json,
    extract::{RawQuery, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    auth::AuthenticatedPrincipal,
    notification::{NotificationMode, NotificationPreferences},
    server::AppState,
    web::{
        http::{chat_error_response, repository_response},
        stream::same_origin,
    },
};

const ACCOUNT_HEADER: &str = "x-sproyt-account";

// The native contract rejects extra authority fields rather than silently
// ignoring user_id/participant supplied alongside valid preferences.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreferenceInput {
    mode: NotificationMode,
    direct_messages: bool,
    mentions: bool,
    weekly_weekday: i16,
}

async fn admit(
    state: &AppState,
    headers: &HeaderMap,
    query: Option<String>,
) -> Result<AuthenticatedPrincipal, Response> {
    let sessions = super::service(state).map_err(IntoResponse::into_response)?;
    let token = super::bearer(headers).map_err(IntoResponse::into_response)?;
    // This /me operation has no query contract. In particular participant must
    // never become an alternate identity or development-auth fallback.
    if query.is_some() {
        return Err(StatusCode::UNAUTHORIZED.into_response());
    }
    if !same_origin(headers) {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let principal = sessions
        .authenticate(&state.auth, token)
        .await
        .map_err(IntoResponse::into_response)?;
    // An account-switch fence, not authority: the authenticated native grant
    // always selects the account. Duplicated/missing/mismatched fences fail.
    let mut accounts = headers.get_all(ACCOUNT_HEADER).iter();
    let account = accounts.next().and_then(|value| value.to_str().ok());
    if accounts.next().is_some() || account != Some(principal.user.id.to_string().as_str()) {
        return Err(StatusCode::CONFLICT.into_response());
    }
    Ok(principal)
}

fn preferences_response(account: &str, preferences: NotificationPreferences) -> Response {
    let mut response = Json(preferences).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    // The value is a canonical server-owned UUID, never a reflected request.
    response.headers_mut().insert(
        ACCOUNT_HEADER,
        HeaderValue::from_str(account).expect("canonical account UUID is a header value"),
    );
    response
}

pub(super) async fn read(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let account = principal.user.id.to_string();
    match state.notifications.settings(principal.user.id).await {
        Ok(settings) => preferences_response(&account, settings.preferences),
        Err(error) => repository_response(error),
    }
}

pub(super) async fn save(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    input: Result<Json<PreferenceInput>, JsonRejection>,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    let input = match input {
        Ok(Json(input)) => input,
        Err(rejection) => {
            return if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            }
            .into_response();
        }
    };
    let account = principal.user.id.to_string();
    let preferences = NotificationPreferences {
        mode: input.mode,
        direct_messages: input.direct_messages,
        mentions: input.mentions,
        weekly_weekday: input.weekly_weekday,
    };
    // A native login can reach this route before opening its first chat
    // connection. Materialize only the authenticated/fenced account, as the
    // native command handler does, before writing FK-owned preferences.
    if let Err(error) = state.chat.ensure_user(principal.user.clone()).await {
        return chat_error_response(error);
    }
    match state
        .notifications
        .save_preferences(principal.user.id, preferences)
        .await
    {
        Ok(preferences) => preferences_response(&account, preferences),
        Err(error) => repository_response(error),
    }
}
