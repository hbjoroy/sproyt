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
#[cfg(test)]
use web::media::{MediaPreparationError, detected_media_type, prepare_uploaded_media};

use std::time::Duration;

use axum::{
    Json,
    extract::{Query, Request, State, ws::WebSocketUpgrade},
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{COOKIE, LOCATION, SET_COOKIE},
    },
    middleware,
    response::{Html, IntoResponse},
};
#[cfg(test)]
use axum::{Router, routing::post};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use tracing::warn;

#[cfg(test)]
use crate::{
    agent::{AgentScope, CreateAgent, GrantAgent},
    auth::AuthService,
    chat::ChatEngine,
    domain::{ChannelId, UserId},
    notification::NotificationService,
    operations::OperationalState,
    process::{HeartGateway, SetCircleFeature, SharedProcessGateway},
    web::mcp::{MCP_PROTOCOL_VERSION, McpRequest, mcp_handler},
};
use crate::{
    auth::AuthError,
    operations::{healthz, metrics, record_metrics},
    web::http::{WsQuery, auth_error_response},
};
#[cfg(test)]
use axum::http::header::{ACCEPT, AUTHORIZATION, ORIGIN};

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

#[cfg(test)]
#[path = "main_tests/mcp.rs"]
mod mcp_tests;

#[cfg(test)]
#[path = "main_tests/protocol_capacity.rs"]
mod protocol_capacity_tests;
