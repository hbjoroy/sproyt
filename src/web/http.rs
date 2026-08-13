use axum::{
    http::{HeaderMap, header::COOKIE},
    response::IntoResponse,
};
use serde::Deserialize;

use crate::{auth::AuthError, chat::ChatError, server::AppState};

#[derive(Debug, Deserialize)]
pub(crate) struct WsQuery {
    pub(crate) participant: Option<String>,
}

pub(crate) async fn authenticate_http(
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

pub(crate) fn repository_response(
    error: crate::domain::RepositoryError,
) -> axum::response::Response {
    let status = match error {
        crate::domain::RepositoryError::PermissionDenied => axum::http::StatusCode::FORBIDDEN,
        crate::domain::RepositoryError::NotFound => axum::http::StatusCode::NOT_FOUND,
        crate::domain::RepositoryError::Conflict => axum::http::StatusCode::CONFLICT,
        crate::domain::RepositoryError::Storage(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    };
    (status, error.public_message()).into_response()
}

pub(crate) fn auth_error_response(error: AuthError) -> axum::response::Response {
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

pub(crate) fn chat_error_response(error: ChatError) -> axum::response::Response {
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
