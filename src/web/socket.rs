use crate::{
    server::AppState,
    web::http::{WsQuery, auth_error_response},
    ws,
};
use axum::{
    extract::{Query, State, ws::WebSocketUpgrade},
    http::{HeaderMap, header::COOKIE},
    response::IntoResponse,
};

pub(crate) async fn ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> axum::response::Response {
    let cookie = headers
        .get(COOKIE)
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
    let shutdown = state.operations.subscribe_shutdown();
    upgrade
        .on_upgrade(move |socket| {
            ws::handle_socket(
                state.chat,
                ws::SocketAuthentication::new(state.auth, principal, requested_name, cookie),
                socket,
                shutdown,
                state.websocket_idle_timeout,
            )
        })
        .into_response()
}
