use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{
    chatbot::location::{LocationInput, enabled},
    server::AppState,
    web::{
        http::{WsQuery, auth_error_response, authenticate_http, repository_response},
        stream::same_origin,
    },
};

fn no_store(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

async fn principal(
    state: &AppState,
    query: WsQuery,
    headers: &HeaderMap,
) -> Result<crate::auth::AuthenticatedPrincipal, Response> {
    authenticate_http(state, query, headers)
        .await
        .map_err(|error| no_store(auth_error_response(error)))
}

pub(crate) async fn list(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    let principal = match principal(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if uuid::Uuid::parse_str(&channel).is_err() {
        return no_store((StatusCode::BAD_REQUEST, "invalid channel id").into_response());
    }
    if !enabled() {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    no_store(
        match service.list_locations(&principal.user.id, &channel).await {
            Ok(view) => Json(view).into_response(),
            Err(error) => repository_response(error),
        },
    )
}

pub(crate) async fn put(
    State(state): State<AppState>,
    Path((channel, agent)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(input): Json<LocationInput>,
) -> Response {
    if !same_origin(&headers) {
        return no_store(StatusCode::FORBIDDEN.into_response());
    }
    let principal = match principal(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&agent).is_err() {
        return no_store((StatusCode::BAD_REQUEST, "invalid channel or agent id").into_response());
    }
    if !enabled() {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    }
    let Some(service) = &state.chat_agents else {
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    no_store(
        match service
            .put_location(&principal.user.id, &channel, &agent, input)
            .await
        {
            Ok(location) => Json(location).into_response(),
            Err(error) => repository_response(error),
        },
    )
}

pub(crate) async fn delete(
    State(state): State<AppState>,
    Path((channel, agent)): Path<(String, String)>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Response {
    if !same_origin(&headers) {
        return no_store(StatusCode::FORBIDDEN.into_response());
    }
    let principal = match principal(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(response) => return response,
    };
    if uuid::Uuid::parse_str(&channel).is_err() || uuid::Uuid::parse_str(&agent).is_err() {
        return no_store((StatusCode::BAD_REQUEST, "invalid channel or agent id").into_response());
    }
    let Some(service) = &state.chat_agents else {
        // Revocation remains available while new sharing is disabled, but it
        // still needs the configured store to identify the caller's own row.
        return no_store(StatusCode::SERVICE_UNAVAILABLE.into_response());
    };
    no_store(
        match service
            .delete_location(&principal.user.id, &channel, &agent)
            .await
        {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => repository_response(error),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chatbot::location::{AgentLocationView, AgentLocationsView, SharedLocation};
    use chrono::{TimeZone, Utc};

    #[test]
    fn location_api_response_is_private_and_has_the_stable_shape() {
        let response = no_store(
            Json(AgentLocationsView {
                agents: vec![AgentLocationView {
                    id: "agent-id".into(),
                    name: "Hjelpar".into(),
                    location: Some(SharedLocation {
                        latitude: 60.1235,
                        longitude: 5.9877,
                        accuracy_m: 12.0,
                        observed_at: Utc.with_ymd_and_hms(2026, 10, 10, 8, 0, 0).unwrap(),
                        expires_at: Utc.with_ymd_and_hms(2026, 10, 10, 8, 30, 0).unwrap(),
                    }),
                }],
            })
            .into_response(),
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        let value = serde_json::to_value(AgentLocationsView {
            agents: vec![AgentLocationView {
                id: "agent-id".into(),
                name: "Hjelpar".into(),
                location: None,
            }],
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"agents":[{"id":"agent-id","name":"Hjelpar","location":null}]})
        );
    }
}
