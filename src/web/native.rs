//! Bounded native HTTP edge. No provider or browser authentication cookies are
//! returned to the desktop app, and native bearer APIs reject cookie authority.
use crate::{
    native_sessions::{CancelRequest, ExchangeRequest, LoginRequest, NativeError, RefreshRequest},
    server::AppState,
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Form, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/native/info", get(info))
        .route("/api/v1/native/login", post(login))
        .route("/api/v1/native/exchange", post(exchange))
        .route("/api/v1/native/session", get(session))
        .route("/api/v1/native/refresh", post(refresh))
        .route("/api/v1/native/revoke", post(revoke))
        .route("/api/v1/native/cancel-login", post(cancel))
        .route("/auth/native/browser/{id}", get(browser))
        .route("/auth/native/confirm/{id}", post(confirm))
        .layer(DefaultBodyLimit::max(8192))
}
impl IntoResponse for NativeError {
    fn into_response(self) -> Response {
        (
            self.status(),
            Json(serde_json::json!({"error":"native_rejected"})),
        )
            .into_response()
    }
}
fn service(state: &AppState) -> Result<&crate::native_sessions::NativeSessions, StatusCode> {
    state.native_sessions.as_ref().ok_or(StatusCode::NOT_FOUND)
}
fn no_cookies(headers: &HeaderMap) -> Result<(), NativeError> {
    if headers.contains_key(header::COOKIE) {
        Err(NativeError::Unauthorized)
    } else {
        Ok(())
    }
}
fn bearer(headers: &HeaderMap) -> Result<&str, NativeError> {
    no_cookies(headers)?;
    if headers.get_all(header::AUTHORIZATION).iter().count() != 1 {
        return Err(NativeError::Unauthorized);
    }
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| !v.is_empty() && !v.chars().any(char::is_whitespace))
        .ok_or(NativeError::Unauthorized)
}
fn redirect(url: &str, cookie: Option<&str>) -> Result<Response, NativeError> {
    let mut response = StatusCode::FOUND.into_response();
    response.headers_mut().insert(
        header::LOCATION,
        HeaderValue::from_str(url).map_err(|_| NativeError::Unavailable)?,
    );
    if let Some(cookie) = cookie {
        response.headers_mut().insert(
            header::SET_COOKIE,
            HeaderValue::from_str(cookie).map_err(|_| NativeError::Unavailable)?,
        );
    }
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    Ok(response)
}
async fn info(State(state): State<AppState>) -> Response {
    match service(&state) {
        Ok(s) => Json(s.info()).into_response(),
        Err(r) => r.into_response(),
    }
}
async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    if let Err(error) = no_cookies(&headers) {
        return error.into_response();
    }
    let Ok(Json(input)) = input else {
        return NativeError::Invalid.into_response();
    };
    match s.login(&state.auth, input).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}
async fn exchange(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<ExchangeRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    if let Err(error) = no_cookies(&headers) {
        return error.into_response();
    }
    let Ok(Json(input)) = input else {
        return NativeError::Invalid.into_response();
    };
    match s.exchange(input).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}
async fn session(State(state): State<AppState>, headers: HeaderMap) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    let token = match bearer(&headers) {
        Ok(token) => token,
        Err(e) => return e.into_response(),
    };
    match s.session(&state.auth, token).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}
async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<RefreshRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    let token = match bearer(&headers) {
        Ok(token) => token,
        Err(e) => return e.into_response(),
    };
    let Ok(Json(input)) = input else {
        return NativeError::Invalid.into_response();
    };
    match s.refresh(&state.auth, token, input).await {
        Ok(value) => Json(value).into_response(),
        Err(e) => e.into_response(),
    }
}
async fn revoke(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    if !body.is_empty() {
        return NativeError::Invalid.into_response();
    }
    let token = match bearer(&headers) {
        Ok(token) => token,
        Err(e) => return e.into_response(),
    };
    match s.revoke(token).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Result<Json<CancelRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    if let Err(error) = no_cookies(&headers) {
        return error.into_response();
    }
    let Ok(Json(input)) = input else {
        return NativeError::Invalid.into_response();
    };
    match s.cancel_login(input).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    }
}
async fn browser(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    match s
        .browser(&id)
        .await
        .and_then(|(url, cookie)| redirect(&url, Some(&cookie)))
    {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}
fn escaped(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
pub(crate) async fn callback(
    state: &AppState,
    code: String,
    oidc_state: String,
    headers: &HeaderMap,
) -> Response {
    let s = match service(state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    match s.callback(&state.auth, code, oidc_state, cookie).await {
        Err(e) => e.into_response(),
        Ok(value) => {
            if state
                .chat
                .ensure_user(value.principal.user.clone())
                .await
                .is_err()
            {
                return NativeError::Unavailable.into_response();
            }
            let html = format!(
                "<!doctype html><html lang=nn><meta charset=utf-8><meta name=viewport content='width=device-width'><title>Sprøyt for Windows</title><style>html{{color-scheme:light dark}}body{{font:17px/1.5 system-ui,sans-serif;max-width:30rem;margin:10vh auto;padding:24px;background:#faf9f5;color:#191b18}}h1{{font-size:28px;line-height:1.2}}button{{min-height:48px;padding:12px 20px;font:inherit;font-weight:600;border:1px solid #191b18;border-radius:2px;background:#d9ed70;color:#191b18}}@media(prefers-color-scheme:dark){{body{{background:#191b18;color:#ebece1}}}}</style><p>Sprøyt · Windows Canary</p><h1>Logg inn i Windows-appen?</h1><p>Konto: <strong>{}</strong><br>{}</p><form method=post action='/auth/native/confirm/{}'><input type=hidden name=nonce value='{}'><button type=submit>Fortset med denne kontoen</button></form><p>Lukk fana dersom kontoen er feil.</p></html>",
                escaped(&value.principal.user.display_name.to_string()),
                escaped(value.principal.email.as_deref().unwrap_or("Sprøyt-konto")),
                escaped(&value.transaction_id),
                escaped(&value.nonce)
            );
            let mut response = Html(html).into_response();
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
                .headers_mut()
                .insert("referrer-policy", HeaderValue::from_static("origin"));
            response.headers_mut().insert("content-security-policy",HeaderValue::from_str(&format!("default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'self' http://127.0.0.1:{}",value.callback_port)).expect("stored numeric callback port is a header-safe CSP source"));
            response
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Confirmation {
    nonce: String,
}
async fn confirm(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    input: Result<Form<Confirmation>, axum::extract::rejection::FormRejection>,
) -> Response {
    let s = match service(&state) {
        Ok(s) => s,
        Err(r) => return r.into_response(),
    };
    if headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) != Some(s.origin()) {
        return NativeError::Unauthorized.into_response();
    }
    let Ok(Form(input)) = input else {
        return NativeError::Invalid.into_response();
    };
    let cookie = headers.get(header::COOKIE).and_then(|v| v.to_str().ok());
    match s
        .confirm(&id, &input.nonce, cookie)
        .await
        .and_then(|url| redirect(&url, None))
    {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
pub(crate) async fn test_state(
    auth: crate::auth::AuthService,
    native: Option<crate::native_sessions::NativeSessions>,
) -> AppState {
    use std::sync::Arc;
    let repository = Arc::new(
        crate::db::SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    AppState {
        media_store: None,
        auth,
        native_sessions: native,
        chat: crate::chat::ChatEngine::start(repository.clone()),
        operations: crate::operations::OperationalState::default(),
        processes: crate::process::ProcessService::start(repository.clone(), None),
        agents: crate::agent::AgentService::new(repository.clone()),
        chat_agents: None,
        integrations: crate::integration::IntegrationService::new(repository),
        notifications: crate::notification::NotificationService::test(),
        imagegen: None,
        process_pilot: None,
        work_items: None,
        enrollment: None,
        websocket_idle_timeout: std::time::Duration::from_secs(60),
        advanced_ui_enabled: false,
        agent_ui_enabled: false,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_authority_rejects_cookie_mix_and_duplicate_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer token".parse().unwrap());
        assert!(bearer(&headers).is_ok());
        headers.insert(header::COOKIE, "sproyt_session=browser".parse().unwrap());
        assert!(bearer(&headers).is_err());
        headers.remove(header::COOKIE);
        headers.append(header::AUTHORIZATION, "Bearer second".parse().unwrap());
        assert!(bearer(&headers).is_err());
        assert!(escaped("<Alice>&\"'") == "&lt;Alice&gt;&amp;&quot;&#39;");
    }
    #[tokio::test]
    async fn native_endpoints_are_disabled_by_default() {
        let state = test_state(crate::auth::AuthService::development(), None).await;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, routes().with_state(state))
                .await
                .unwrap()
        });
        let client = reqwest::Client::new();
        assert!(
            client
                .get(format!("{origin}/api/v1/native/info"))
                .send()
                .await
                .unwrap()
                .status()
                == StatusCode::NOT_FOUND
        );
        for path in ["login", "exchange", "refresh", "cancel-login"] {
            assert!(
                client
                    .post(format!("{origin}/api/v1/native/{path}"))
                    .json(&serde_json::json!({}))
                    .send()
                    .await
                    .unwrap()
                    .status()
                    == StatusCode::NOT_FOUND
            );
        }
        server.abort();
    }
}
