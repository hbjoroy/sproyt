use crate::{
    auth::AuthError,
    server::AppState,
    web::{
        assets::{APP_BUNDLE, BUILD_REVISION, INDEX_HTML, app_bundle_fingerprint},
        http::auth_error_response,
    },
};
use axum::{
    extract::{Query, State},
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{COOKIE, LOCATION},
    },
    response::{Html, IntoResponse},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct InviteQuery {
    pub(crate) invite: Option<String>,
}

pub(crate) async fn index(
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
                return crate::web::auth::redirect_with_cookies(
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
    let app_revision = app_bundle_fingerprint(BUILD_REVISION, APP_BUNDLE.as_bytes());
    let app_url = format!("/assets/app/{app_revision}/app.js");
    let html = INDEX_HTML
        .replace("{{NONCE}}", &nonce)
        .replace("{{APP_URL}}", &app_url)
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

pub(crate) fn is_safe_invitation_token(token: &str) -> bool {
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
