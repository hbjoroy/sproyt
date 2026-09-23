use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{
        IntoResponse, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::{
    chat::{ChatEngine, ConnectionId},
    domain::{ChannelId, ChannelSequence, ChatEvent, UserId},
    protocol::{
        ClientCommand, ClientEnvelope, ProtocolVersion, ServerEnvelope, ServerEvent, check_protocol,
    },
    server::AppState,
    web::http::{WsQuery, auth_error_response, authenticate_http},
    ws,
};

#[derive(Deserialize)]
pub(crate) struct EventsQuery {
    participant: Option<String>,
    channel_id: Option<String>,
    request_id: Option<String>,
    after: Option<u64>,
}

fn same_origin(headers: &HeaderMap) -> bool {
    if headers
        .get("sec-fetch-site")
        .is_some_and(|value| value != "same-origin" && value != "none")
    {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let (Some(origin), Some(host)) = (
        origin
            .to_str()
            .ok()
            .and_then(|value| value.parse::<axum::http::Uri>().ok()),
        headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok()),
    ) else {
        return false;
    };
    origin
        .authority()
        .is_some_and(|authority| authority.as_str().eq_ignore_ascii_case(host))
        && matches!(origin.scheme_str(), Some("https") | Some("http"))
}

pub(crate) async fn command_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    Json(envelope): Json<ClientEnvelope>,
) -> axum::response::Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let principal = match authenticate_http(&state, query, &headers).await {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    if let Err(error) = state.chat.ensure_user(principal.user.clone()).await {
        return Json(ServerEnvelope::response(
            envelope.request_id,
            ws::error_event(error),
        ))
        .into_response();
    }
    let request_id = envelope.request_id.clone();
    let response = if check_protocol(&envelope.protocol) != ProtocolVersion::Supported {
        ServerEnvelope::response(
            request_id,
            ServerEvent::Error {
                code: "unsupported_protocol".to_owned(),
                message: "unsupported protocol".to_owned(),
            },
        )
    } else if request_id.trim().is_empty() || request_id.len() > 128 {
        ServerEnvelope::response(
            request_id,
            ServerEvent::Error {
                code: "invalid_request_id".to_owned(),
                message: "request_id must contain 1 to 128 bytes".to_owned(),
            },
        )
    } else if matches!(
        envelope.command,
        ClientCommand::SubscribeChannel { .. } | ClientCommand::UnsubscribeChannel { .. }
    ) {
        ServerEnvelope::response(
            request_id,
            ServerEvent::Error {
                code: "invalid_transport_command".to_owned(),
                message: "subscriptions belong to the event stream".to_owned(),
            },
        )
    } else {
        ws::execute_http_command(&state.chat, &principal.user.id, envelope).await
    };
    Json(response).into_response()
}

struct PresenceGuard {
    chat: ChatEngine,
    channel_id: ChannelId,
    participant_id: UserId,
    connection_id: ConnectionId,
}

impl Drop for PresenceGuard {
    fn drop(&mut self) {
        let (chat, channel_id, participant_id, connection_id) = (
            self.chat.clone(),
            self.channel_id.clone(),
            self.participant_id.clone(),
            self.connection_id,
        );
        tokio::spawn(async move {
            let _ = chat.leave(channel_id, participant_id, connection_id).await;
        });
    }
}

fn frame(envelope: ServerEnvelope) -> Result<Event, Infallible> {
    let mut event =
        Event::default().data(serde_json::to_string(&envelope).expect("server events serialize"));
    if let ServerEvent::Chat {
        event: ChatEvent::MessageAccepted { message },
    } = &envelope.event
    {
        event = event.id(u64::from(message.sequence).to_string());
    }
    Ok(event)
}

fn missing_before_history(after: Option<u64>, first_available: Option<u64>) -> Option<(u64, u64)> {
    let after = after.filter(|after| *after > 0)?;
    let skipped = first_available?.saturating_sub(after.saturating_add(1));
    (skipped > 0).then_some((after, skipped))
}

pub(crate) async fn events_handler(
    State(state): State<AppState>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let requested_name = query.participant;
    let principal = match state
        .auth
        .authenticate_request(requested_name.clone(), cookie.as_deref())
        .await
    {
        Ok(principal) => principal,
        Err(error) => return auth_error_response(error),
    };
    if state
        .chat
        .ensure_user(principal.user.clone())
        .await
        .is_err()
    {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let participant_id = principal.user.id;
    let channel_id = match query.channel_id {
        Some(id) => match ChannelId::new(id) {
            Ok(id) => Some(id),
            Err(_) => return StatusCode::BAD_REQUEST.into_response(),
        },
        None => None,
    };
    let request_id = query.request_id;
    let after = query.after.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|value| value.to_str().ok()?.parse::<u64>().ok())
    });
    if request_id
        .as_ref()
        .is_some_and(|id| id.trim().is_empty() || id.len() > 128)
        || channel_id.is_some() != request_id.is_some()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let subscription = if let Some(channel_id) = &channel_id {
        match state
            .chat
            .subscribe(channel_id.clone(), participant_id.clone())
            .await
        {
            Ok(subscription) => Some(subscription),
            Err(error) => {
                let status = match error.kind() {
                    "permission_denied" => StatusCode::FORBIDDEN,
                    "not_found" => StatusCode::NOT_FOUND,
                    _ => StatusCode::SERVICE_UNAVAILABLE,
                };
                return status.into_response();
            }
        }
    } else {
        None
    };
    let presence =
        subscription
            .as_ref()
            .zip(channel_id.as_ref())
            .map(|(subscription, channel_id)| PresenceGuard {
                chat: state.chat.clone(),
                channel_id: channel_id.clone(),
                participant_id: participant_id.clone(),
                connection_id: subscription.connection_id,
            });
    let mut shutdown = state.operations.subscribe_shutdown();
    let chat = state.chat;
    let auth = state.auth;
    let stream = async_stream::stream! {
        yield Ok::<Event, Infallible>(Event::default().comment("connected"));
        if let (Some(channel_id), Some(request_id), Some(mut subscription)) = (channel_id, request_id, subscription) {
            let _presence = presence;
            let history = subscription.history;
            let mut last_seen = history.last().map_or(ChannelSequence::new(0), |message| message.sequence);
            let first_available = history.first().map(|message| u64::from(message.sequence));
            yield frame(ServerEnvelope::response(request_id, ServerEvent::SubscriptionStarted { channel_id: channel_id.clone(), history }));
            if let Some((after, skipped)) = missing_before_history(after, first_available) {
                yield frame(ServerEnvelope::event(ServerEvent::Lagged {
                    channel_id: channel_id.clone(), last_seen_sequence: ChannelSequence::new(after),
                    latest_known_sequence: last_seen, skipped,
                    hint: "load_recent_messages_after".to_owned(),
                }));
            }
            let mut auth_tick = tokio::time::interval(Duration::from_secs(30));
            let mut presence_tick = tokio::time::interval(Duration::from_secs(20));
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            auth_tick.tick().await;
            presence_tick.tick().await;
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    result = subscription.receiver.recv() => match result {
                        Ok(event) => {
                            if !channel_visible(&chat, &participant_id, &channel_id).await { break; }
                            if let ChatEvent::MessageAccepted { message } = &event { last_seen = message.sequence; }
                            yield frame(ServerEnvelope::event(ServerEvent::Chat { event }));
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            match chat.latest_sequence(channel_id.clone()).await {
                                Ok(latest_known_sequence) => yield frame(ServerEnvelope::event(ServerEvent::Lagged {
                                    channel_id: channel_id.clone(), last_seen_sequence: last_seen,
                                    latest_known_sequence, skipped,
                                    hint: "load_recent_messages_after".to_owned(),
                                })),
                                Err(error) => { yield frame(ServerEnvelope::event(ws::error_event(error))); break; }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = auth_tick.tick() => {
                        if auth.authenticate_request(requested_name.clone(), cookie.as_deref()).await.is_err() { break; }
                    }
                    _ = presence_tick.tick() => {
                        if !channel_visible(&chat, &participant_id, &channel_id).await { break; }
                        if chat.renew_presence(participant_id.clone(), vec![(channel_id.clone(), subscription.connection_id)]).await.is_err() { break; }
                    }
                    _ = heartbeat.tick() => yield Ok::<Event, Infallible>(Event::default().event("heartbeat").data("1")),
                    result = shutdown.changed() => if result.is_err() || *shutdown.borrow() { break; },
                }
            }
        } else {
            let mut auth_tick = tokio::time::interval(Duration::from_secs(30));
            let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
            auth_tick.tick().await;
            heartbeat.tick().await;
            loop {
                tokio::select! {
                    _ = auth_tick.tick() => if auth.authenticate_request(requested_name.clone(), cookie.as_deref()).await.is_err() { break; },
                    _ = heartbeat.tick() => yield Ok::<Event, Infallible>(Event::default().event("heartbeat").data("1")),
                    result = shutdown.changed() => if result.is_err() || *shutdown.borrow() { break; },
                }
            }
        }
    };
    let mut response = Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("heartbeat"),
        )
        .into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

async fn channel_visible(
    chat: &ChatEngine,
    participant_id: &UserId,
    channel_id: &ChannelId,
) -> bool {
    chat.list_channels(participant_id.clone())
        .await
        .is_ok_and(|channels| channels.iter().any(|channel| &channel.id == channel_id))
}

#[cfg(test)]
mod tests {
    use super::missing_before_history;

    #[test]
    fn reconnect_only_reports_messages_missing_from_recent_history() {
        assert_eq!(missing_before_history(Some(1), Some(11)), Some((1, 9)));
        assert_eq!(missing_before_history(Some(55), Some(11)), None);
        assert_eq!(missing_before_history(Some(60), Some(11)), None);
        assert_eq!(missing_before_history(Some(u64::MAX), Some(11)), None);
    }
}
