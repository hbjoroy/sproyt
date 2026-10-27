//! Transient, channel-authorised agent place lookup. Provider results are not stored.
use axum::{
    Json,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

use crate::{
    chatbot::MapsLookupError,
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, repository_response},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageBatch {
    message_ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LookupRequest {
    #[serde(default)]
    allow_shared_location: bool,
    preview_binding: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RouteRequest {
    token: String,
    #[serde(default)]
    allow_shared_location: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeoapifyConsent {
    preview_binding: String,
    #[serde(default)]
    allow_shared_location: bool,
    origin_text: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeoapifyOrigin {
    roll_id: String,
    index: usize,
}

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

async fn account(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<crate::auth::AuthenticatedPrincipal, Response> {
    let principal = authenticate_http(state, query, headers)
        .await
        .map_err(auth_error_response)?;
    if headers.get_all("x-sproyt-account").iter().count() != 1
        || headers
            .get("x-sproyt-account")
            .and_then(|value| value.to_str().ok())
            != Some(principal.user.id.to_string().as_str())
    {
        return Err((StatusCode::CONFLICT, "Kontoen vart endra. Prøv igjen.").into_response());
    }
    Ok(principal)
}

pub(crate) async fn available(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<MessageBatch>,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    let valid = !input.message_ids.is_empty()
        && input.message_ids.len() <= 30
        && uuid::Uuid::parse_str(&channel).is_ok()
        && input
            .message_ids
            .iter()
            .all(|id| uuid::Uuid::parse_str(id).is_ok())
        && input
            .message_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            == input.message_ids.len();
    if !valid {
        return no_store((StatusCode::BAD_REQUEST, "Ugyldig meldingsutval.").into_response());
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let message_ids = match service
        .place_lookup_messages(&principal.user.id, &channel, &input.message_ids)
        .await
    {
        Ok(message_ids) => message_ids,
        Err(error) => return no_store(repository_response(error)),
    };
    let geoapify_message_ids = match service
        .geoapify_roll_messages(&principal.user.id, &channel, &message_ids)
        .await
    {
        Ok(ids) => ids,
        Err(error) => return no_store(repository_response(error)),
    };
    let google_message_ids = match service
        .google_place_lookup_messages(&principal.user.id, &channel, &message_ids)
        .await
    {
        Ok(ids) => ids,
        Err(error) => return no_store(repository_response(error)),
    };
    no_store(Json(serde_json::json!({"message_ids":message_ids,"geoapify_message_ids":geoapify_message_ids,"google_message_ids":google_message_ids})).into_response())
}

pub(crate) async fn lookup(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<LookupRequest>,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&message).is_err() {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller melding.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .run_place_lookup(
            &principal.user.id,
            &channel,
            &message,
            input.allow_shared_location,
            &input.preview_binding,
        )
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(MapsLookupError::ConsentRequired) => (
            StatusCode::FORBIDDEN,
            "Du må tillate at den delte posisjonen blir brukt til Google-søket.",
        )
            .into_response(),
        Err(MapsLookupError::NotFound) => (
            StatusCode::NOT_FOUND,
            "Stadssøket er ikkje lenger tilgjengeleg.",
        )
            .into_response(),
        Err(MapsLookupError::Busy) => (
            StatusCode::TOO_MANY_REQUESTS,
            "For mange søk akkurat no. Prøv igjen om litt.",
        )
            .into_response(),
        Err(
            MapsLookupError::Timeout | MapsLookupError::Provider | MapsLookupError::Unavailable,
        ) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Stadssøket er ikkje tilgjengeleg akkurat no. Prøv igjen.",
        )
            .into_response(),
    };
    no_store(response)
}

/// Compute a walking route only to a place selected from this viewer's search.
pub(crate) async fn route(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<RouteRequest>,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&message).is_err() {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller melding.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .run_place_route(
            &principal.user.id,
            &channel,
            &message,
            &input.token,
            input.allow_shared_location,
        )
        .await
    {
        Ok(result) => Json(result).into_response(),
        Err(MapsLookupError::ConsentRequired) => (
            StatusCode::FORBIDDEN,
            "Du må tillate at den delte posisjonen blir brukt til Google-søket.",
        )
            .into_response(),
        Err(MapsLookupError::NotFound) => (
            StatusCode::NOT_FOUND,
            "Staden er ikkje lenger tilgjengeleg. Søk på nytt.",
        )
            .into_response(),
        Err(MapsLookupError::Busy) => (
            StatusCode::TOO_MANY_REQUESTS,
            "For mange søk akkurat no. Prøv igjen om litt.",
        )
            .into_response(),
        Err(
            MapsLookupError::Timeout | MapsLookupError::Provider | MapsLookupError::Unavailable,
        ) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Gangruta er ikkje tilgjengeleg akkurat no. Prøv igjen.",
        )
            .into_response(),
    };
    no_store(response)
}

/// The proposed query is user-authored and is shown before Google receives it.
pub(crate) async fn preview(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&message).is_err() {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller melding.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .place_lookup_preview(&principal.user.id, &channel, &message)
        .await
    {
        Ok(Some(preview)) => Json(preview).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            "Stadssøket er ikkje lenger tilgjengeleg.",
        )
            .into_response(),
        Err(error) => repository_response(error),
    };
    no_store(response)
}

/// Only the person who requested the search can authorize this provider.
pub(crate) async fn geoapify_preview(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&message).is_err() {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller melding.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .geoapify_roll_preview(&principal.user.id, &channel, &message)
        .await
    {
        Ok(preview) => Json(preview).into_response(),
        Err(error) => repository_response(error),
    };
    no_store(response)
}

pub(crate) async fn geoapify_consent(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<GeoapifyConsent>,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&message).is_err() {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller melding.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .consent_geoapify_roll(
            &principal.user.id,
            &channel,
            &message,
            &input.preview_binding,
            input.allow_shared_location,
            input.origin_text.as_deref(),
        )
        .await
    {
        Ok(view) => Json(view).into_response(),
        Err(error) => repository_response(error),
    };
    no_store(response)
}

pub(crate) async fn geoapify_origin(
    State(state): State<AppState>,
    Path((channel, message)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<GeoapifyOrigin>,
) -> Response {
    let principal = match account(&state, query, &headers).await {
        Ok(value) => value,
        Err(error) => return no_store(error),
    };
    if uuid::Uuid::parse_str(&channel).is_err()
        || uuid::Uuid::parse_str(&message).is_err()
        || uuid::Uuid::parse_str(&input.roll_id).is_err()
    {
        return no_store(
            (StatusCode::BAD_REQUEST, "Ugyldig samtale eller oppslag.").into_response(),
        );
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    let response = match service
        .choose_geoapify_roll_origin(
            &principal.user.id,
            &channel,
            &message,
            &input.roll_id,
            input.index,
        )
        .await
    {
        Ok(view) => Json(view).into_response(),
        Err(error) => repository_response(error),
    };
    no_store(response)
}

pub(crate) fn body_limit() -> DefaultBodyLimit {
    DefaultBodyLimit::max(8 * 1024)
}

#[cfg(test)]
#[path = "agent_place_lookup_tests.rs"]
mod tests;
