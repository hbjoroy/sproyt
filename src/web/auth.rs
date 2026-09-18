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
                Some(circle.id),
                "invitee@example.test".to_owned(),
                Utc::now() + Duration::hours(1),
            )
            .await
            .unwrap();
        chat.activate_enrollment_invitation(issued.token.clone(), uuid::Uuid::new_v4())
            .await
            .unwrap();
        let state = AppState {
            imagegen: None,
            auth: AuthService::development(),
            chat: chat.clone(),
            operations: OperationalState::default(),
            processes: ProcessService::start(repository.clone(), None),
            agents: AgentService::new(repository.clone()),
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
