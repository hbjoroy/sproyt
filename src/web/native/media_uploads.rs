//! Native upload admission is explicit and account-fenced. Browser upload
//! routes retain cookie authority and their existing same-origin checks.
use axum::{
    extract::{Path, RawQuery, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::web::{media_uploads as shared, stream::same_origin};
use crate::{auth::AuthenticatedPrincipal, media_store::NewUpload, server::AppState};

const ACCOUNT: &str = "x-sproyt-account";

async fn admit(
    state: &AppState,
    headers: &HeaderMap,
    query: Option<String>,
) -> Result<AuthenticatedPrincipal, Response> {
    let sessions = super::service(state).map_err(IntoResponse::into_response)?;
    let token = super::bearer(headers).map_err(IntoResponse::into_response)?;
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
    let mut accounts = headers.get_all(ACCOUNT).iter();
    let account = accounts.next().and_then(|value| value.to_str().ok());
    if accounts.next().is_some() || account != Some(principal.user.id.to_string().as_str()) {
        return Err(StatusCode::CONFLICT.into_response());
    }
    Ok(principal)
}

fn account_response(principal: &AuthenticatedPrincipal, mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response.headers_mut().insert(
        ACCOUNT,
        HeaderValue::from_str(&principal.user.id.to_string()).expect("canonical UUID header"),
    );
    response
}

pub(super) async fn configuration(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    account_response(&principal, shared::configuration_for(&state))
}

pub(super) async fn create(
    State(state): State<AppState>,
    Path(channel): Path<Uuid>,
    RawQuery(query): RawQuery,
    request: Request,
) -> Response {
    let principal = match admit(&state, request.headers(), query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    let json_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| {
            value.trim().eq_ignore_ascii_case("application/json")
                || value
                    .trim()
                    .to_ascii_lowercase()
                    .starts_with("application/")
                    && value.trim().to_ascii_lowercase().ends_with("+json")
        });
    if !json_type {
        return account_response(&principal, StatusCode::BAD_REQUEST.into_response());
    }
    let bytes = match tokio::time::timeout(
        std::time::Duration::from_secs(15),
        axum::body::to_bytes(request.into_body(), 8192),
    )
    .await
    {
        Ok(Ok(value)) => value,
        Ok(Err(_)) => {
            return account_response(&principal, StatusCode::PAYLOAD_TOO_LARGE.into_response());
        }
        Err(_) => return account_response(&principal, StatusCode::REQUEST_TIMEOUT.into_response()),
    };
    let input: NewUpload = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return account_response(&principal, StatusCode::BAD_REQUEST.into_response()),
    };
    account_response(
        &principal,
        shared::create_for(&state, &principal.user.id, channel, input).await,
    )
}

pub(super) async fn status(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    account_response(
        &principal,
        shared::status_for(&state, &principal.user.id, id).await,
    )
}

pub(super) async fn put_chunk(
    State(state): State<AppState>,
    Path((id, index)): Path<(Uuid, u32)>,
    RawQuery(query): RawQuery,
    request: Request,
) -> Response {
    let principal = match admit(&state, request.headers(), query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    account_response(
        &principal,
        shared::put_chunk_for(&state, &principal.user.id, id, index, request).await,
    )
}

pub(super) async fn complete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    account_response(
        &principal,
        shared::complete_for(&state, &principal.user.id, id).await,
    )
}

pub(super) async fn abort(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let principal = match admit(&state, &headers, query).await {
        Ok(value) => value,
        Err(response) => return response,
    };
    account_response(
        &principal,
        shared::abort_for(&state, &principal.user.id, id).await,
    )
}
