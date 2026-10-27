use crate::{
    chat::ChatError,
    server::AppState,
    web::{
        browser::is_safe_invitation_token,
        http::{auth_error_response, chat_error_response},
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

#[derive(Debug, Default, Deserialize)]
pub(crate) struct LoginQuery {
    pub(crate) invite: Option<String>,
    pub(crate) enrollment: Option<String>,
    #[serde(flatten)]
    pub(crate) view: SafeViewQuery,
}

/// The only view fields carried through session renewal or OIDC. Unknown query
/// fields are discarded; every returned value is formatted from a fixed route.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct SafeViewQuery {
    pub(crate) channel: Option<String>,
    pub(crate) message: Option<String>,
    pub(crate) thread: Option<String>,
    pub(crate) sequence: Option<String>,
    pub(crate) view: Option<String>,
    pub(crate) inbox: Option<String>,
    pub(crate) section: Option<String>,
    pub(crate) circle: Option<String>,
    pub(crate) task: Option<String>,
    pub(crate) feature: Option<String>,
    pub(crate) agent: Option<String>,
    pub(crate) panel: Option<String>,
}

impl SafeViewQuery {
    pub(crate) fn return_to(&self) -> Option<String> {
        let resource = |value: Option<&str>| uuid::Uuid::parse_str(value?).ok();
        match self.view.as_deref() {
            None => safe_chat_return_to(
                self.channel.as_deref(),
                self.message.as_deref(),
                self.thread.as_deref(),
                self.sequence.as_deref(),
            ),
            Some("circles") => Some("/?view=circles".to_owned()),
            Some("direct") => Some("/?view=direct".to_owned()),
            Some("inbox") => match self.inbox.as_deref()? {
                "unread" => Some("/?view=inbox&inbox=unread".to_owned()),
                "mentions" => Some("/?view=inbox&inbox=mentions".to_owned()),
                "tasks" => Some("/?view=inbox&inbox=tasks".to_owned()),
                _ => None,
            },
            Some("settings") => {
                let section = self.section.as_deref().unwrap_or("overview");
                let path = match section {
                    "overview" | "profile" | "notifications" | "welcome" | "appearance"
                    | "help" => {
                        format!("/?view=settings&section={section}")
                    }
                    "circle" | "memory" => format!(
                        "/?view=settings&section={section}&circle={}",
                        resource(self.circle.as_deref())?
                    ),
                    "chat-agents" => {
                        let panel = self.panel.as_deref();
                        if panel.is_some_and(|panel| {
                            !matches!(
                                panel,
                                "overview"
                                    | "behaviour"
                                    | "skills"
                                    | "memory"
                                    | "vision"
                                    | "images"
                                    | "weather"
                                    | "sea"
                                    | "ferry"
                                    | "handoff"
                                    | "channels"
                            )
                        }) {
                            return None;
                        }
                        let mut path = format!(
                            "/?view=settings&section=chat-agents&circle={}",
                            resource(self.circle.as_deref())?
                        );
                        match self.agent.as_deref() {
                            None if panel.is_none() || panel == Some("channels") => {}
                            Some("new") if panel.is_none() || panel == Some("behaviour") => {
                                path.push_str("&agent=new")
                            }
                            Some("new") | None => return None,
                            Some(agent) => {
                                path.push_str(&format!("&agent={}", resource(Some(agent))?))
                            }
                        }
                        if let Some(panel) = panel {
                            path.push_str(&format!("&panel={panel}"));
                        }
                        if let Some(channel) = self.channel.as_deref() {
                            path.push_str(&format!("&channel={}", resource(Some(channel))?));
                        }
                        path
                    }
                    "community" => {
                        let task = self.task.as_deref()?;
                        let base = format!("/?view=settings&section=community&task={task}");
                        match task {
                            "people" | "create-circle" | "circles" | "global-channels"
                            | "global-invite" | "accept-invitation" => base,
                            "create-channel" => match self.circle.as_deref() {
                                Some(circle) => {
                                    format!("{base}&circle={}", resource(Some(circle))?)
                                }
                                None => base,
                            },
                            "channel" => {
                                format!("{base}&channel={}", resource(self.channel.as_deref())?)
                            }
                            "channels" | "invite" | "rename-circle" | "circle-members" => {
                                format!("{base}&circle={}", resource(self.circle.as_deref())?)
                            }
                            _ => return None,
                        }
                    }
                    "advanced" => {
                        let feature = self.feature.as_deref()?;
                        if !matches!(feature, "agent" | "heart") {
                            return None;
                        }
                        let base = format!("/?view=settings&section=advanced&feature={feature}");
                        match self.channel.as_deref() {
                            Some(channel) => format!("{base}&channel={}", resource(Some(channel))?),
                            None => base,
                        }
                    }
                    _ => return None,
                };
                Some(path)
            }
            _ => None,
        }
    }

    pub(crate) fn login_location(&self) -> Option<String> {
        if self.view.is_none() {
            return safe_chat_login_location(
                self.channel.as_deref(),
                self.message.as_deref(),
                self.thread.as_deref(),
                self.sequence.as_deref(),
            );
        }
        self.return_to()
            .map(|path| path.replacen("/", "/auth/login", 1))
    }
}

pub(crate) fn safe_chat_return_to(
    channel: Option<&str>,
    message: Option<&str>,
    thread: Option<&str>,
    sequence: Option<&str>,
) -> Option<String> {
    let channel = uuid::Uuid::parse_str(channel?).ok()?;
    let mut path = format!("/?channel={channel}");
    let message = message.and_then(|value| uuid::Uuid::parse_str(value).ok());
    if let Some(message) = message {
        path.push_str(&format!("&message={message}"));
    }
    if let Some(thread) = thread.and_then(|value| uuid::Uuid::parse_str(value).ok()) {
        path.push_str(&format!("&thread={thread}"));
    }
    if message.is_some()
        && let Some(sequence) = sequence.and_then(|value| value.parse::<u64>().ok())
        && sequence > 0
        && sequence <= 9_007_199_254_740_991
    {
        path.push_str(&format!("&sequence={sequence}"));
    }
    Some(path)
}

pub(crate) fn safe_chat_login_location(
    channel: Option<&str>,
    message: Option<&str>,
    thread: Option<&str>,
    sequence: Option<&str>,
) -> Option<String> {
    safe_chat_return_to(channel, message, thread, sequence)
        .map(|path| path.replacen("/", "/auth/login", 1))
}

pub(crate) async fn auth_login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> axum::response::Response {
    // Enrollment tokens are capabilities.  Do not carry one in `return_to`,
    // where it would reappear in browser history, referrers, or logs.  Instead
    // `AuthService` seals it into the bounded OIDC transaction cookie.
    if let Some(token) = query
        .enrollment
        .filter(|token| is_safe_invitation_token(token))
    {
        return match state.auth.login_enrollment(token) {
            Ok(login) => redirect_with_cookies(&login.authorization_url, &[login.set_cookie]),
            Err(error) => auth_error_response(error),
        };
    }
    let return_to = query
        .invite
        .filter(|token| is_safe_invitation_token(token))
        .map(|token| format!("/?invite={token}"))
        .or_else(|| query.view.return_to());
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
    if let Some(native) = &state.native_sessions {
        match native.owns_state(&query.state).await {
            Ok(true) => {
                return crate::web::native::callback(&state, query.code, query.state, &headers)
                    .await;
            }
            Ok(false) => {}
            Err(error) => return error.into_response(),
        }
    }
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    match state.auth.callback(query.code, query.state, cookie).await {
        Ok(login) => complete_login(&state, login).await,
        Err(error) => auth_error_response(error),
    }
}

async fn complete_login(
    state: &AppState,
    login: crate::auth::LoginComplete,
) -> axum::response::Response {
    let mut cookies = vec![login.set_cookie, login.clear_transaction_cookie];
    if let Some(refresh_cookie) = login.set_refresh_cookie {
        cookies.push(refresh_cookie);
    }
    if let Err(error) = state.chat.ensure_user(login.principal.user.clone()).await {
        return response_with_cookies(
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                error.public_message(),
            )
                .into_response(),
            &cookies,
        );
    }
    if let Some(token) = login.enrollment_token {
        let Some(email) = login.principal.email else {
            return response_with_cookies(
                (
                    axum::http::StatusCode::FORBIDDEN,
                    "Invitasjonen krev ei gyldig e-postadresse frå innlogginga.",
                )
                    .into_response(),
                &cookies,
            );
        };
        return match state
            .chat
            .accept_enrollment_invitation(login.principal.user.id, email, token)
            .await
        {
            Ok(_) => redirect_with_cookies("/", &cookies),
            Err(ChatError::Repository(crate::domain::RepositoryError::NotFound)) => {
                response_with_cookies(
                    (
                        axum::http::StatusCode::FORBIDDEN,
                        "Invitasjonen er ugyldig, utløpt eller allereie brukt.",
                    )
                        .into_response(),
                    &cookies,
                )
            }
            Err(error) => response_with_cookies(chat_error_response(error), &cookies),
        };
    }
    redirect_with_cookies(&login.return_to, &cookies)
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

/// Authoritative identity for local share receipts. A network/server failure
/// must never be interpreted by the worker as an anonymous session.
pub(crate) async fn share_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    share_identity_response(state.auth.authenticate_request(None, cookie).await)
}

fn share_identity_response(
    principal: Result<crate::auth::AuthenticatedPrincipal, crate::auth::AuthError>,
) -> axum::response::Response {
    let mut response = match principal {
        Ok(principal) => {
            Json(serde_json::json!({ "user_id": principal.user.id.to_string() })).into_response()
        }
        Err(error) => auth_error_response(error),
    };
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        HeaderValue::from_static("no-store"),
    );
    response
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

fn response_with_cookies(
    mut response: axum::response::Response,
    cookies: &[String],
) -> axum::response::Response {
    for cookie in cookies {
        if let Ok(cookie) = HeaderValue::from_str(cookie) {
            response.headers_mut().append(SET_COOKIE, cookie);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn share_identity_exposes_only_authenticated_id_and_never_caches_failures() {
        let principal = AuthService::development()
            .authenticate_request(None, None)
            .await
            .unwrap();
        let expected = principal.user.id.to_string();
        let response = share_identity_response(Ok(principal));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({"user_id": expected})
        );
        for (error, status) in [
            (
                crate::auth::AuthError::Unauthorized,
                StatusCode::UNAUTHORIZED,
            ),
            (
                crate::auth::AuthError::External("offline".to_owned()),
                StatusCode::SERVICE_UNAVAILABLE,
            ),
        ] {
            let response = share_identity_response(Err(error));
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
    }

    #[test]
    fn message_return_path_survives_login_without_accepting_external_redirects() {
        let channel = uuid::Uuid::new_v4().to_string();
        let message = uuid::Uuid::new_v4().to_string();
        let thread = uuid::Uuid::new_v4().to_string();
        let path = format!("/?channel={channel}&message={message}&thread={thread}&sequence=42");
        assert_eq!(
            safe_chat_return_to(Some(&channel), Some(&message), Some(&thread), Some("42")),
            Some(path.clone())
        );
        assert_eq!(
            safe_chat_login_location(Some(&channel), Some(&message), Some(&thread), Some("42")),
            Some(path.replacen("/", "/auth/login", 1))
        );
        assert_eq!(
            safe_chat_return_to(Some("https://evil.test"), Some(&message), None, None),
            None
        );
        assert_eq!(
            safe_chat_return_to(Some(&channel), Some("https://evil.test"), None, None),
            Some(format!("/?channel={channel}"))
        );
    }

    fn assert_view_login_roundtrip(path: &str) {
        // Exercise both actual flattened Axum query extractors: root URL,
        // renewal target, login URL, then the sealed OIDC return target.
        let root = Query::<crate::web::browser::InviteQuery>::try_from_uri(&path.parse().unwrap())
            .unwrap()
            .0;
        assert_eq!(root.view.return_to().as_deref(), Some(path));
        let login_path = root.view.login_location().unwrap();
        assert_eq!(login_path, path.replacen("/", "/auth/login", 1));
        let login = Query::<LoginQuery>::try_from_uri(&login_path.parse().unwrap())
            .unwrap()
            .0;
        assert_eq!(login.view.return_to().as_deref(), Some(path));
        assert!(root.invite.is_none());
        assert!(login.invite.is_none());
        assert!(login.enrollment.is_none());
    }

    #[test]
    fn navigation_and_personal_settings_roundtrip_through_cold_login() {
        for path in [
            "/?view=circles",
            "/?view=direct",
            "/?view=inbox&inbox=unread",
            "/?view=inbox&inbox=mentions",
            "/?view=inbox&inbox=tasks",
            "/?view=settings&section=overview",
            "/?view=settings&section=profile",
            "/?view=settings&section=notifications",
            "/?view=settings&section=welcome",
            "/?view=settings&section=appearance",
            "/?view=settings&section=help",
        ] {
            assert_view_login_roundtrip(path);
        }
    }

    #[test]
    fn scoped_settings_roundtrip_only_known_tasks_and_resource_fields() {
        let circle = uuid::Uuid::new_v4();
        let channel = uuid::Uuid::new_v4();
        for section in ["circle", "chat-agents", "memory"] {
            assert_view_login_roundtrip(&format!(
                "/?view=settings&section={section}&circle={circle}"
            ));
        }
        for task in [
            "people",
            "create-circle",
            "circles",
            "global-channels",
            "global-invite",
            "accept-invitation",
            "create-channel",
        ] {
            assert_view_login_roundtrip(&format!("/?view=settings&section=community&task={task}"));
        }
        for task in [
            "create-channel",
            "channels",
            "invite",
            "rename-circle",
            "circle-members",
        ] {
            assert_view_login_roundtrip(&format!(
                "/?view=settings&section=community&task={task}&circle={circle}"
            ));
        }
        assert_view_login_roundtrip(&format!(
            "/?view=settings&section=community&task=channel&channel={channel}"
        ));
        for feature in ["agent", "heart"] {
            assert_view_login_roundtrip(&format!(
                "/?view=settings&section=advanced&feature={feature}"
            ));
            assert_view_login_roundtrip(&format!(
                "/?view=settings&section=advanced&feature={feature}&channel={channel}"
            ));
        }
    }

    #[test]
    fn agent_settings_panels_and_context_survive_cold_login() {
        let circle = uuid::Uuid::new_v4();
        let agent = uuid::Uuid::new_v4();
        let channel = uuid::Uuid::new_v4();
        let base = format!("/?view=settings&section=chat-agents&circle={circle}");
        assert_view_login_roundtrip(&base);
        assert_view_login_roundtrip(&format!("{base}&panel=channels&channel={channel}"));
        assert_view_login_roundtrip(&format!("{base}&agent=new"));
        assert_view_login_roundtrip(&format!("{base}&agent=new&panel=behaviour"));
        assert_view_login_roundtrip(&format!("{base}&agent={agent}"));
        for panel in [
            "overview",
            "behaviour",
            "skills",
            "memory",
            "vision",
            "images",
            "weather",
            "sea",
            "ferry",
            "handoff",
            "channels",
        ] {
            let path = format!("{base}&agent={agent}&panel={panel}&channel={channel}");
            assert_view_login_roundtrip(&path);
            let untrusted =
                format!("{path}&invite=secret&token=secret&return_to=https%3A%2F%2Fevil.test");
            let query = Query::<SafeViewQuery>::try_from_uri(&untrusted.parse().unwrap())
                .unwrap()
                .0;
            assert_eq!(query.return_to(), Some(path));
        }
    }

    #[test]
    fn agent_settings_reject_invalid_ids_panels_and_combinations() {
        let circle = uuid::Uuid::new_v4();
        let agent = uuid::Uuid::new_v4();
        let base = format!("/?view=settings&section=chat-agents&circle={circle}");
        for suffix in [
            "&agent=bad".to_owned(),
            "&agent=".to_owned(),
            "&panel=".to_owned(),
            "&panel=skills".to_owned(),
            "&panel=overview".to_owned(),
            "&agent=new&panel=overview".to_owned(),
            "&agent=new&panel=channels".to_owned(),
            "&panel=channels&channel=bad".to_owned(),
            format!("&agent={agent}&panel=unknown"),
            format!("&agent={agent}&channel=bad"),
        ] {
            let path = format!("{base}{suffix}");
            let query = Query::<SafeViewQuery>::try_from_uri(&path.parse().unwrap())
                .unwrap()
                .0;
            assert_eq!(query.return_to(), None, "accepted {path}");
            assert_eq!(query.login_location(), None, "accepted login {path}");
        }
    }

    #[test]
    fn safe_view_queries_drop_unrecognized_fields_and_normalize_identifiers() {
        let circle = uuid::Uuid::new_v4();
        let compact = circle.simple().to_string().to_uppercase();
        let path = format!(
            "/?view=settings&section=circle&circle={compact}&return_to=https%3A%2F%2Fevil.test&credential=secret&invite=capability&enrollment=another-capability&unknown=value"
        );
        let query = Query::<LoginQuery>::try_from_uri(&path.parse().unwrap())
            .unwrap()
            .0;
        assert_eq!(query.invite.as_deref(), Some("capability"));
        assert_eq!(query.enrollment.as_deref(), Some("another-capability"));
        assert_eq!(
            query.view.return_to(),
            Some(format!("/?view=settings&section=circle&circle={circle}"))
        );
        assert_eq!(
            query.view.login_location(),
            Some(format!(
                "/auth/login?view=settings&section=circle&circle={circle}"
            ))
        );
        let default = Query::<SafeViewQuery>::try_from_uri(
            &"/?view=settings&section=overview&channel=https%3A%2F%2Fevil.test"
                .parse()
                .unwrap(),
        )
        .unwrap()
        .0;
        assert_eq!(
            default.return_to().as_deref(),
            Some("/?view=settings&section=overview")
        );
    }

    #[test]
    fn safe_view_queries_reject_unknown_routes_and_invalid_required_uuids() {
        for path in [
            "/?view=https%3A%2F%2Fevil.test",
            "/?view=inbox&inbox=other",
            "/?view=settings&section=%2F%2Fevil.test",
            "/?view=settings&section=circle",
            "/?view=settings&section=chat-agents&circle=not-a-uuid",
            "/?view=settings&section=memory&circle=https%3A%2F%2Fevil.test",
            "/?view=settings&section=community&task=unknown",
            "/?view=settings&section=community&task=channel&channel=bad",
            "/?view=settings&section=community&task=invite&circle=bad",
            "/?view=settings&section=community&task=create-channel&circle=bad",
            "/?view=settings&section=advanced&feature=external",
            "/?view=settings&section=advanced&feature=agent&channel=bad",
            "/?return_to=%2F%2Fevil.test&credential=secret",
        ] {
            let query = Query::<SafeViewQuery>::try_from_uri(&path.parse().unwrap())
                .unwrap()
                .0;
            assert_eq!(query.return_to(), None, "accepted {path}");
            assert_eq!(query.login_location(), None, "accepted login {path}");
        }
    }

    #[test]
    fn thread_only_links_and_safe_message_sequences_survive_login() {
        let channel = uuid::Uuid::new_v4();
        let thread = uuid::Uuid::new_v4();
        let message = uuid::Uuid::new_v4();
        let path = format!("/?channel={channel}&thread={thread}");
        assert_view_login_roundtrip(&path);
        assert_eq!(
            safe_chat_return_to(
                Some(&channel.to_string()),
                None,
                Some(&thread.to_string()),
                Some("42")
            ),
            Some(path)
        );
        for sequence in [
            "0",
            "-1",
            "invalid",
            "9007199254740992",
            "18446744073709551615",
        ] {
            assert_eq!(
                safe_chat_return_to(
                    Some(&channel.to_string()),
                    Some(&message.to_string()),
                    Some(&thread.to_string()),
                    Some(sequence)
                ),
                Some(format!(
                    "/?channel={channel}&message={message}&thread={thread}"
                ))
            );
        }
        assert_view_login_roundtrip(&format!(
            "/?channel={channel}&message={message}&thread={thread}&sequence=9007199254740991"
        ));
    }
    use crate::{
        agent::AgentService,
        auth::{AuthService, AuthenticatedPrincipal, LoginComplete},
        chat::ChatEngine,
        db::SqliteChatRepository,
        domain::{ChannelSlug, DisplayName, PrincipalKind, User, UserId},
        notification::NotificationService,
        operations::OperationalState,
        process::ProcessService,
    };
    use axum::http::{StatusCode, header};
    use chrono::{Duration, Utc};
    use std::sync::Arc;

    fn completed_login(user: User, email: Option<&str>, token: Option<String>) -> LoginComplete {
        LoginComplete {
            principal: AuthenticatedPrincipal {
                issuer: "https://auth.example".to_owned(),
                subject: "new-user".to_owned(),
                email: email.map(str::to_owned),
                user,
            },
            set_cookie: "sproyt_session=session; Path=/".to_owned(),
            set_refresh_cookie: None,
            clear_transaction_cookie: "sproyt_oidc_tx=; Path=/auth/callback; Max-Age=0".to_owned(),
            return_to: "/?invite=legacy-token".to_owned(),
            enrollment_token: token,
        }
    }

    #[tokio::test]
    async fn enrollment_callback_consumes_the_token_once_and_never_redirects_with_it() {
        verify_enrollment_callback(false).await;
        verify_enrollment_callback(true).await;
    }

    async fn verify_enrollment_callback(existing_account: bool) {
        let repository = Arc::new(
            SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repository.migrate().await.unwrap();
        let chat = ChatEngine::start(repository.clone());
        let owner = User {
            id: UserId::named("enrollment-owner"),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Enrollment owner").unwrap(),
            handle: Some(crate::domain::Handle::new("enrollment-owner").unwrap()),
            external_provider: Some("test".to_owned()),
            external_subject: Some("enrollment-owner".to_owned()),
            created_at: Utc::now(),
        };
        chat.ensure_user(owner.clone()).await.unwrap();
        let circle = chat
            .create_circle(
                owner.id.clone(),
                ChannelSlug::new("enrollment-test").unwrap(),
                DisplayName::new("Enrollment test").unwrap(),
            )
            .await
            .unwrap();
        let issued = chat
            .prepare_enrollment_invitation(
                owner.id.clone(),
                Some(circle.id.clone()),
                "invitee@example.test".to_owned(),
                Utc::now() + Duration::hours(1),
            )
            .await
            .unwrap();
        chat.activate_enrollment_invitation(issued.token.clone(), uuid::Uuid::new_v4())
            .await
            .unwrap();
        let state = AppState {
            media_store: None,
            imagegen: None,
            process_pilot: None,
            work_items: None,
            auth: AuthService::development(),
            native_sessions: None,
            chat: chat.clone(),
            operations: OperationalState::default(),
            processes: ProcessService::start(repository.clone(), None),
            agents: AgentService::new(repository.clone()),
            chat_agents: None,
            integrations: crate::integration::IntegrationService::new(repository),
            notifications: NotificationService::test(),
            enrollment: None,
            websocket_idle_timeout: std::time::Duration::from_secs(60),
            advanced_ui_enabled: false,
            agent_ui_enabled: false,
        };
        let invitee = User {
            id: UserId::named("enrollment-invitee"),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Enrollment invitee").unwrap(),
            handle: Some(crate::domain::Handle::new("enrollment-invitee").unwrap()),
            external_provider: Some("https://auth.example".to_owned()),
            external_subject: Some("new-user".to_owned()),
            created_at: Utc::now(),
        };

        if existing_account {
            chat.ensure_user(invitee.clone()).await.unwrap();
        }
        let wrong_account = complete_login(
            &state,
            completed_login(
                owner.clone(),
                Some("wrong-account@example.test"),
                Some(issued.token.clone()),
            ),
        )
        .await;
        assert_eq!(wrong_account.status(), StatusCode::FORBIDDEN);
        assert!(
            chat.list_circles(invitee.id.clone())
                .await
                .unwrap()
                .is_empty()
        );

        let response = complete_login(
            &state,
            completed_login(
                invitee.clone(),
                Some("invitee@example.test"),
                Some(issued.token.clone()),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(response.headers().get(header::LOCATION).unwrap(), "/");
        let memberships = chat.list_circles(invitee.id.clone()).await.unwrap();
        assert_eq!(memberships.len(), 1);
        assert_eq!(memberships[0].0.id, circle.id);
        assert!(
            !response
                .headers()
                .get(header::LOCATION)
                .unwrap()
                .to_str()
                .unwrap()
                .contains(&issued.token)
        );

        let replay = complete_login(
            &state,
            completed_login(
                invitee.clone(),
                Some("invitee@example.test"),
                Some(issued.token),
            ),
        )
        .await;
        assert_eq!(replay.status(), StatusCode::FORBIDDEN);
        assert!(
            replay
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .any(|cookie| cookie.to_str().unwrap().contains("sproyt_oidc_tx="))
        );

        let missing_email = complete_login(
            &state,
            completed_login(invitee, None, Some("unused-enrollment-token".to_owned())),
        )
        .await;
        assert_eq!(missing_email.status(), StatusCode::FORBIDDEN);
    }
}
