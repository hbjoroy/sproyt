use axum::{
    Json,
    extract::{State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::{
    protocol::{
        ClientCommand, ClientEnvelope, ProtocolVersion, ServerEnvelope, ServerEvent, check_protocol,
    },
    server::AppState,
    ws,
};

fn bearer(headers: &HeaderMap) -> Option<&str> {
    // Native transport never converts browser cookies to a native principal.
    if headers.contains_key(header::COOKIE) {
        return None;
    }
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    value
        .strip_prefix("Bearer ")
        .filter(|token| token.len() <= 128 && !token.bytes().any(|byte| byte.is_ascii_whitespace()))
}

pub(crate) async fn socket(
    State(state): State<AppState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(sessions) = state.native_sessions.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(token) = bearer(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let (principal, reference) = match sessions.authenticate_ref(&state.auth, token).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let shutdown = state.operations.subscribe_shutdown();
    upgrade
        .on_upgrade(move |socket| async move {
            let _connection = state
                .operations
                .connection_started(crate::operations::Transport::WebSocket);
            ws::handle_socket(
                state.chat,
                ws::SocketAuthentication::native(state.auth, principal, sessions, reference),
                socket,
                shutdown,
                state.websocket_idle_timeout,
            )
            .await;
        })
        .into_response()
}

pub(crate) async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(envelope): Json<ClientEnvelope>,
) -> Response {
    let Some(sessions) = &state.native_sessions else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(token) = bearer(&headers) else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let principal = match sessions.authenticate(&state.auth, token).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    if let Err(error) = state.chat.ensure_user(principal.user.clone()).await {
        return crate::web::http::chat_error_response(error);
    }
    let request_id = envelope.request_id.clone();
    let error = if check_protocol(&envelope.protocol) != ProtocolVersion::Supported {
        Some(("unsupported_protocol", "unsupported protocol"))
    } else if request_id.trim().is_empty() || request_id.len() > 128 {
        Some((
            "invalid_request_id",
            "request_id must contain 1 to 128 bytes",
        ))
    } else if matches!(
        envelope.command,
        ClientCommand::SubscribeChannel { .. } | ClientCommand::UnsubscribeChannel { .. }
    ) {
        Some((
            "invalid_transport_command",
            "subscriptions belong to the websocket",
        ))
    } else {
        None
    };
    if let Some((code, message)) = error {
        return Json(ServerEnvelope::response(
            request_id,
            ServerEvent::Error {
                code: code.into(),
                message: message.into(),
            },
        ))
        .into_response();
    }
    let chat = state.chat.clone();
    match tokio::spawn(async move {
        ws::execute_http_command(&chat, &principal.user.id, envelope).await
    })
    .await
    {
        Ok(response) => Json(response).into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_transport_rejects_cookie_and_duplicate_authority() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            "Bearer sproyt-access.test".parse().unwrap(),
        );
        assert_eq!(bearer(&headers), Some("sproyt-access.test"));
        headers.insert(header::COOKIE, "sproyt_session=browser".parse().unwrap());
        assert!(bearer(&headers).is_none());
        headers.remove(header::COOKIE);
        headers.append(header::AUTHORIZATION, "Bearer second".parse().unwrap());
        assert!(bearer(&headers).is_none());
    }
}
