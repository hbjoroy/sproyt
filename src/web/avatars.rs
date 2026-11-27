use crate::{
    domain::{PrincipalKind, UserId},
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http, chat_error_response},
};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

pub(crate) async fn published(
    State(state): State<AppState>,
    Path(person): Path<Uuid>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    let viewer = authenticate_http(&state, query, &headers)
        .await
        .map_err(auth_error_response)?;
    if viewer.user.kind != PrincipalKind::Human {
        return Err(StatusCode::FORBIDDEN.into_response());
    }
    let jpeg = state
        .chat
        .profile_avatar(UserId::from_uuid(person))
        .await
        .map_err(chat_error_response)?;
    let cache = [
        (header::CACHE_CONTROL, "private, max-age=60"),
        (header::VARY, "Cookie, Authorization"),
    ];
    Ok(match jpeg {
        Some(jpeg) => (
            cache,
            [
                (header::CONTENT_TYPE, "image/jpeg"),
                (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            ],
            jpeg,
        )
            .into_response(),
        None => (StatusCode::NO_CONTENT, cache).into_response(),
    })
}
