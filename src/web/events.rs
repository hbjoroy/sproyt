use crate::{
    chat::ChatError,
    domain::{ChannelId, ChannelSequence, MessageLimit},
    events::{EventInput, EventStore, reference_codes},
    server::AppState,
    web::{
        http::{WsQuery, chat_error_response},
        personal_space::authenticated_user,
    },
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header::CACHE_CONTROL},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
fn fail(e: crate::domain::RepositoryError) -> Response {
    chat_error_response(ChatError::Repository(e))
}
fn response(v: Value) -> Response {
    ([(CACHE_CONTROL, "private, no-store")], Json(v)).into_response()
}
async fn context(
    state: &AppState,
    q: WsQuery,
    h: &HeaderMap,
) -> Result<(String, EventStore), Response> {
    let actor = authenticated_user(state, q, h).await?.id.to_string();
    let store = state
        .topics
        .clone()
        .map(EventStore)
        .ok_or_else(|| StatusCode::SERVICE_UNAVAILABLE.into_response())?;
    Ok((actor, store))
}
#[derive(Deserialize, Default)]
pub(crate) struct Page {
    after: Option<String>,
    before: Option<i64>,
    prefix: Option<String>,
    search: Option<String>,
}
pub(crate) async fn list(
    State(s): State<AppState>,
    Path(c): Path<Uuid>,
    Query(q): Query<WsQuery>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    if let Some(search) = page.search {
        return Ok(response(
            e.search_suggestions(&a, &c.to_string(), &search)
                .await
                .map_err(fail)?,
        ));
    }
    if let Some(prefix) = page.prefix {
        return Ok(response(
            e.suggestions(&a, &c.to_string(), &prefix)
                .await
                .map_err(fail)?,
        ));
    }
    Ok(response(
        e.list(&a, &c.to_string(), page.after.as_deref().unwrap_or(""))
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn create(
    State(s): State<AppState>,
    Path(c): Path<Uuid>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<EventInput>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    let request_id = h
        .get("Idempotency-Key")
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|value| Uuid::parse_str(value).ok())
                .map(|id| id.to_string())
                .ok_or(StatusCode::BAD_REQUEST)
        })
        .transpose()
        .map_err(IntoResponse::into_response)?;
    Ok(response(
        e.save_with_request(&a, &c.to_string(), None, input, request_id.as_deref())
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn detail(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.detail(&a, &c.to_string(), &id.to_string())
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn update(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<EventInput>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.save(&a, &c.to_string(), Some(&id.to_string()), input)
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn cancel(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.cancel(&a, &c.to_string(), &id.to_string())
            .await
            .map_err(fail)?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rsvp {
    status: String,
}
pub(crate) async fn rsvp(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<Rsvp>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.rsvp(&a, &c.to_string(), &id.to_string(), &input.status)
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn responses(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.responses(
            &a,
            &c.to_string(),
            &id.to_string(),
            page.after.as_deref().unwrap_or(""),
        )
        .await
        .map_err(fail)?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Image {
    media_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cover {
    media_id: Option<String>,
}
pub(crate) async fn add_image(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<Image>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Uuid::parse_str(&input.media_id).map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
    Ok(response(
        e.image(&a, &c.to_string(), &id.to_string(), &input.media_id, false)
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn remove_image(
    State(s): State<AppState>,
    Path((c, id, media)): Path<(Uuid, Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.image(
            &a,
            &c.to_string(),
            &id.to_string(),
            &media.to_string(),
            true,
        )
        .await
        .map_err(fail)?,
    ))
}
pub(crate) async fn cover(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<Cover>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    if let Some(v) = &input.media_id {
        Uuid::parse_str(v).map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
    }
    Ok(response(
        e.cover(
            &a,
            &c.to_string(),
            &id.to_string(),
            input.media_id.as_deref(),
        )
        .await
        .map_err(fail)?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct References {
    codes: Vec<String>,
}
pub(crate) async fn references(
    State(s): State<AppState>,
    Path(c): Path<Uuid>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
    Json(input): Json<References>,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.references(&a, &c.to_string(), input.codes)
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn messages(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    let codes = e
        .codes(&a, &c.to_string(), &id.to_string())
        .await
        .map_err(fail)?;
    if page.before.is_some_and(|v| v <= 0) {
        return Err(StatusCode::BAD_REQUEST.into_response());
    }
    let before = page
        .before
        .map(ChannelSequence::try_from)
        .transpose()
        .map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
    let actor =
        crate::domain::UserId::new(a).map_err(|_| StatusCode::BAD_REQUEST.into_response())?;
    let all = s
        .chat
        .load_messages(
            actor,
            ChannelId::from_uuid(c),
            MessageLimit::new(200),
            None,
            before,
        )
        .await
        .map_err(chat_error_response)?;
    let scan_next = if all.len() == 200 {
        all.iter().map(|m| u64::from(m.sequence) as i64).min()
    } else {
        None
    };
    let mut messages = all
        .into_iter()
        .filter(|m| {
            m.deleted_at.is_none()
                && reference_codes(m.body.as_str())
                    .iter()
                    .any(|c| codes.contains(c))
        })
        .collect::<Vec<_>>();
    messages.sort_by_key(|m| std::cmp::Reverse(u64::from(m.sequence)));
    let more = messages.len() > 30;
    messages.truncate(30);
    let next = if more {
        messages.last().map(|m| u64::from(m.sequence) as i64)
    } else {
        scan_next
    };
    messages.reverse();

    Ok(response(json!({"messages":messages,"next_before":next})))
}

pub(crate) async fn acknowledge(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.acknowledge(&a, &c.to_string(), &id.to_string())
            .await
            .map_err(fail)?,
    ))
}
pub(crate) async fn images(
    State(s): State<AppState>,
    Path((c, id)): Path<(Uuid, Uuid)>,
    Query(q): Query<WsQuery>,
    Query(page): Query<Page>,
    h: HeaderMap,
) -> Result<Response, Response> {
    let (a, e) = context(&s, q, &h).await?;
    Ok(response(
        e.images(
            &a,
            &c.to_string(),
            &id.to_string(),
            page.after.as_deref().unwrap_or(""),
        )
        .await
        .map_err(fail)?,
    ))
}
