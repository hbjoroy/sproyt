use crate::{
    server::AppState,
    web::{
        browser::{InviteQuery, is_safe_invitation_token},
        http::auth_error_response,
    },
};
use axum::{
    Json,
    extract::{Query, State},
    http::{
        HeaderMap, HeaderValue,
        header::{COOKIE, LOCATION, SET_COOKIE},
    },
    response::IntoResponse,
};
use serde::Deserialize;

pub(crate) async fn auth_login(
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
pub(crate) struct OidcCallbackQuery {
    code: String,
    state: String,
}

pub(crate) async fn auth_callback(
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

pub(crate) async fn auth_logout(State(state): State<AppState>) -> axum::response::Response {
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

pub(crate) async fn auth_refresh(
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

pub(crate) async fn auth_session(
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

pub(crate) fn redirect_with_cookies(
    location: &str,
    cookies: &[String],
) -> axum::response::Response {
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
