mod auth;
mod chat;
mod config;
mod db;
mod domain;
mod operations;
mod protocol;
mod ws;

use std::time::Duration;

use axum::{
    Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{
        HeaderMap, HeaderName, HeaderValue,
        header::{COOKIE, LOCATION, SET_COOKIE},
    },
    middleware,
    response::{Html, IntoResponse},
    routing::get,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tower_http::{
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::TraceLayer,
};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    auth::AuthService,
    chat::{ChatEngine, ChatError},
    config::{AppConfig, AuthMode, LogFormat},
    domain::{ChannelId, ChannelSequence, ChatEvent, ChatMessage, MessageBody, UserId},
    operations::{OperationalState, healthz, metrics, readyz, record_metrics},
};

#[derive(Clone)]
struct AppState {
    auth: AuthService,
    chat: ChatEngine,
    operations: OperationalState,
}

impl axum::extract::FromRef<AppState> for OperationalState {
    fn from_ref(state: &AppState) -> Self {
        state.operations.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let config = AppConfig::from_env()?;
    init_tracing(config.log_format())?;
    if std::env::args().nth(1).as_deref() == Some("migrate") {
        db::migrate(config.database()).await?;
        info!(database = %config.database().kind(), "database migrations applied");
        return Ok(());
    }
    let address = config.bind_address();
    let operations = OperationalState::default();
    let repository = db::connect_repository(config.database()).await?;
    let auth = match config.auth_mode() {
        AuthMode::Development => AuthService::development(),
        AuthMode::Oidc => {
            AuthService::oidc(
                config
                    .oidc()
                    .expect("OIDC config is present when OIDC mode is selected"),
            )
            .await?
        }
    };
    let state = AppState {
        auth,
        chat: ChatEngine::start(repository),
        operations: operations.clone(),
    };
    let request_id_header = HeaderName::from_static("x-request-id");

    let app = Router::new()
        .route("/", get(index))
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route("/metrics", get(metrics))
        .route("/auth/login", get(auth_login))
        .route("/auth/callback", get(auth_callback))
        .route("/auth/logout", get(auth_logout))
        .route("/ws", get(ws_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            operations.clone(),
            record_metrics,
        ))
        .layer(PropagateRequestIdLayer::new(request_id_header.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id_header, MakeRequestUuid));

    let listener = tokio::net::TcpListener::bind(address).await?;
    operations.set_ready(true);
    info!(
        %address,
        environment = %config.environment(),
        database = %config.database().kind(),
        "Sproyt is ready"
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(operations))
        .await?;

    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

fn init_tracing(log_format: LogFormat) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("sproyt=info"));
    match log_format {
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .try_init()?,
        LogFormat::Pretty => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()?,
    }
    Ok(())
}

async fn shutdown_signal(operations: OperationalState) {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            warn!(%error, "failed to install Ctrl+C handler");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => warn!(%error, "failed to install SIGTERM handler"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }

    operations.set_ready(false);
    info!(grace_period_seconds = 30, "shutdown requested");
    tokio::time::sleep(Duration::from_millis(100)).await;
}

async fn ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    let principal = match state.auth.authenticate_request(query.participant, cookie) {
        Ok(principal) => principal,
        Err(error) => {
            return (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response();
        }
    };
    upgrade
        .on_upgrade(move |socket| ws::handle_socket(state.chat, principal, socket))
        .into_response()
}

async fn auth_login(State(state): State<AppState>) -> axum::response::Response {
    match state.auth.login() {
        Ok(login) => redirect_with_cookies(&login.authorization_url, &[login.set_cookie]),
        Err(error) => (axum::http::StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct OidcCallbackQuery {
    code: String,
    state: String,
}

async fn auth_callback(
    State(state): State<AppState>,
    Query(query): Query<OidcCallbackQuery>,
    headers: HeaderMap,
) -> axum::response::Response {
    let cookie = headers.get(COOKIE).and_then(|value| value.to_str().ok());
    match state.auth.callback(query.code, query.state, cookie).await {
        Ok(login) => {
            if let Err(error) = state.chat.ensure_user(login.principal.user).await {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    error.to_string(),
                )
                    .into_response();
            }
            redirect_with_cookies("/", &[login.set_cookie, login.clear_transaction_cookie])
        }
        Err(error) => (axum::http::StatusCode::UNAUTHORIZED, error.to_string()).into_response(),
    }
}

async fn auth_logout(State(state): State<AppState>) -> axum::response::Response {
    let logout = state.auth.logout();
    redirect_with_cookies(&logout.redirect_url, &[logout.clear_cookie])
}

fn redirect_with_cookies(location: &str, cookies: &[String]) -> axum::response::Response {
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

#[allow(dead_code)]
async fn handle_socket(chat: ChatEngine, query: WsQuery, mut socket: WebSocket) {
    let channel_name = query.channel.unwrap_or_else(|| "general".to_owned());
    let participant_name = query.participant.unwrap_or_else(|| "guest".to_owned());
    let participant_id = UserId::named(&participant_name);
    let channel_id = match chat
        .prepare_development_session(participant_id.clone(), &participant_name, &channel_name)
        .await
    {
        Ok(channel_id) => channel_id,
        Err(error) => {
            send_error(&mut socket, error).await;
            return;
        }
    };

    let mut subscription = match chat
        .subscribe(channel_id.clone(), participant_id.clone())
        .await
    {
        Ok(subscription) => subscription,
        Err(error) => {
            send_error(&mut socket, error).await;
            return;
        }
    };

    let mut last_seen_sequence = subscription
        .history
        .last()
        .map_or(ChannelSequence::new(0), |message| message.sequence);
    if send_server_event(
        &mut socket,
        &ServerEvent::History {
            messages: std::mem::take(&mut subscription.history),
        },
    )
    .await
    .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            event = subscription.receiver.recv() => {
                match event {
                    Ok(event) => {
                        if let ChatEvent::MessageAccepted { message } = &event {
                            last_seen_sequence = message.sequence;
                        }
                        if send_server_event(&mut socket, &ServerEvent::Chat { event }).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let latest_known_sequence = match chat.latest_sequence(channel_id.clone()).await {
                            Ok(sequence) => sequence,
                            Err(error) => {
                                send_error(&mut socket, error).await;
                                break;
                            }
                        };
                        let event = ServerEvent::Lagged {
                            channel_id: channel_id.clone(),
                            last_seen_sequence,
                            latest_known_sequence,
                            skipped,
                            hint: "load_recent_messages_after",
                        };
                        if send_server_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = socket.recv() => {
                match frame {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<ClientCommand>(&text) {
                            Ok(ClientCommand::Send { body }) => {
                                match MessageBody::new(body) {
                                    Ok(body) => {
                                        if let Err(error) = chat.send_message(channel_id.clone(), participant_id.clone(), body).await {
                                            send_error(&mut socket, error).await;
                                        }
                                    }
                                    Err(error) => send_error(&mut socket, error.into()).await,
                                }
                            }
                            Err(_) => {
                                let event = ServerEvent::ProtocolError {
                                    reason: "expected JSON like {\"type\":\"send\",\"body\":\"...\"}".to_owned(),
                                };
                                let _ = send_server_event(&mut socket, &event).await;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(_))) | Some(Ok(Message::Pong(_))) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }

    let _ = chat
        .leave(channel_id, participant_id, subscription.connection_id)
        .await;
}

async fn send_error(socket: &mut WebSocket, error: ChatError) {
    let event = ServerEvent::Error {
        message: error.to_string(),
    };
    let _ = send_server_event(socket, &event).await;
}

async fn send_server_event(socket: &mut WebSocket, event: &ServerEvent) -> Result<(), axum::Error> {
    let payload = serde_json::to_string(event).expect("server events must serialize");
    socket.send(Message::Text(payload.into())).await
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    channel: Option<String>,
    participant: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientCommand {
    Send { body: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerEvent {
    Chat {
        event: ChatEvent,
    },
    Error {
        message: String,
    },
    History {
        messages: Vec<ChatMessage>,
    },
    Lagged {
        channel_id: ChannelId,
        last_seen_sequence: ChannelSequence,
        latest_known_sequence: ChannelSequence,
        skipped: u64,
        hint: &'static str,
    },
    ProtocolError {
        reason: String,
    },
}

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="nn">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Sproyt - Hello Chat</title>
    <style>
      :root {
        color-scheme: light dark;
        font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        background: #f7f7f4;
        color: #18201d;
      }

      * {
        box-sizing: border-box;
      }

      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
      }

      main {
        width: min(860px, 100%);
        display: grid;
        grid-template-rows: auto 1fr auto;
        min-height: min(760px, calc(100vh - 48px));
        border: 1px solid #d7d8d0;
        border-radius: 8px;
        background: #ffffff;
        box-shadow: 0 18px 50px rgb(24 32 29 / 12%);
        overflow: hidden;
      }

      header,
      form,
      .messages {
        padding: 18px;
      }

      header {
        display: grid;
        gap: 12px;
        border-bottom: 1px solid #e4e5de;
      }

      h1 {
        margin: 0;
        font-size: 1.6rem;
        line-height: 1.1;
      }

      label {
        display: grid;
        gap: 4px;
        color: #506057;
        font-size: 0.9rem;
      }

      input,
      textarea,
      button {
        min-height: 40px;
        border: 1px solid #cbd1c8;
        border-radius: 6px;
        font: inherit;
      }

      input,
      textarea {
        width: 100%;
        padding: 8px 10px;
        background: #ffffff;
        color: #18201d;
      }

      textarea {
        min-height: 84px;
        resize: vertical;
      }

      button {
        padding: 8px 14px;
        background: #245b45;
        color: #ffffff;
        cursor: pointer;
      }

      button:disabled {
        cursor: default;
        opacity: 0.55;
      }

      .connect {
        display: grid;
        grid-template-columns: 1fr 1fr auto;
        gap: 12px;
        align-items: end;
      }

      .status {
        color: #506057;
        min-height: 1.2em;
      }

      .view-controls {
        display: flex;
        gap: 8px;
      }

      .view-controls button {
        min-height: 34px;
        background: #eef2ed;
        color: #253128;
      }

      .view-controls button[aria-pressed="true"] {
        background: #245b45;
        color: #ffffff;
      }

      .messages {
        align-content: start;
        display: grid;
        gap: 10px;
        overflow-y: auto;
        background: #fbfbf8;
      }

      .message {
        display: grid;
        gap: 4px;
        padding: 12px;
        border: 1px solid #dfe3dc;
        border-radius: 8px;
        background: #ffffff;
      }

      .meta {
        color: #506057;
        font-size: 0.85rem;
      }

      .rendered {
        display: grid;
        gap: 10px;
        line-height: 1.45;
      }

      .rendered h1,
      .rendered h2,
      .rendered h3 {
        margin: 0;
        line-height: 1.2;
      }

      .rendered h1 {
        font-size: 1.35rem;
      }

      .rendered h2 {
        font-size: 1.2rem;
      }

      .rendered h3 {
        font-size: 1.05rem;
      }

      .rendered p,
      .rendered ul,
      .rendered ol,
      .rendered blockquote {
        margin: 0;
      }

      .rendered ul,
      .rendered ol {
        padding-left: 22px;
      }

      .rendered blockquote {
        padding-left: 12px;
        border-left: 3px solid #b9c6bd;
        color: #506057;
      }

      .rendered pre,
      .raw-body {
        overflow-x: auto;
        margin: 0;
        padding: 12px;
        border: 1px solid #d6ddd5;
        border-radius: 6px;
        background: #f4f6f3;
        color: #18201d;
      }

      .rendered code,
      .raw-body {
        font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
        font-size: 0.92rem;
      }

      .rendered p code,
      .rendered li code {
        padding: 1px 5px;
        border-radius: 4px;
        background: #eef2ed;
      }

      .mermaid-shell {
        overflow-x: auto;
        padding: 12px;
        border: 1px solid #d6ddd5;
        border-radius: 6px;
        background: #ffffff;
      }

      .system {
        color: #506057;
        font-size: 0.9rem;
      }

      form.send {
        display: grid;
        grid-template-columns: 1fr auto;
        gap: 12px;
        border-top: 1px solid #e4e5de;
      }

      @media (max-width: 640px) {
        body {
          padding: 12px;
        }

        main {
          min-height: calc(100vh - 24px);
        }

        .connect,
        form.send {
          grid-template-columns: 1fr;
        }
      }

      @media (prefers-color-scheme: dark) {
        :root {
          background: #111613;
          color: #eef3ee;
        }

        main,
        input,
        textarea,
        .message {
          background: #19211c;
          border-color: #344038;
          color: #eef3ee;
        }

        header,
        form.send {
          border-color: #344038;
        }

        .messages {
          background: #121814;
        }

        label,
        .meta,
        .status,
        .system,
        .rendered blockquote {
          color: #b6c1b9;
        }

        .view-controls button,
        .rendered pre,
        .raw-body,
        .rendered p code,
        .rendered li code {
          background: #111713;
          border-color: #344038;
          color: #eef3ee;
        }

        .mermaid-shell {
          background: #eef3ee;
          border-color: #344038;
        }
      }
    </style>
  </head>
  <body>
    <main>
      <header>
        <h1>Hello Chat</h1>
        <form class="connect" id="connect-form">
          <label>
            Kanal
            <input id="channel" name="channel" value="general" autocomplete="off">
          </label>
          <label>
            Namn
            <input id="participant" name="participant" value="alice" autocomplete="off">
          </label>
          <button id="connect" type="submit">Kople til</button>
        </form>
        <div class="status" id="status">Ikkje tilkopla</div>
        <div class="view-controls" aria-label="Meldingsvising">
          <button id="view-mode" type="button" aria-pressed="true">View</button>
          <button id="raw-mode" type="button" aria-pressed="false">Raw</button>
        </div>
      </header>
      <section class="messages" id="messages" aria-live="polite"></section>
      <form class="send" id="send-form">
        <textarea id="body" name="body" placeholder="Skriv Markdown, kode eller Mermaid" autocomplete="off" disabled></textarea>
        <button id="send" type="submit" disabled>Send</button>
      </form>
    </main>

    <script type="module">
      import mermaid from "https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs";

      mermaid.initialize({
        startOnLoad: false,
        securityLevel: "strict",
        theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "default"
      });

      const connectForm = document.querySelector("#connect-form");
      const sendForm = document.querySelector("#send-form");
      const channelInput = document.querySelector("#channel");
      const participantInput = document.querySelector("#participant");
      const bodyInput = document.querySelector("#body");
      const sendButton = document.querySelector("#send");
      const viewModeButton = document.querySelector("#view-mode");
      const rawModeButton = document.querySelector("#raw-mode");
      const statusEl = document.querySelector("#status");
      const messagesEl = document.querySelector("#messages");

      let socket = null;
      let renderMode = "view";
      let requestNumber = 0;
      let activeChannelId = null;
      let requestedChannelSlug = "general";
      const timeline = [];

      connectForm.addEventListener("submit", (event) => {
        event.preventDefault();
        connect();
      });

      sendForm.addEventListener("submit", (event) => {
        event.preventDefault();
        const body = bodyInput.value.trim();
        if (!socket || socket.readyState !== WebSocket.OPEN || !activeChannelId || body.length === 0) {
          return;
        }
        sendCommand("send_message", { channel_id: activeChannelId, body });
        bodyInput.value = "";
        bodyInput.focus();
      });

      viewModeButton.addEventListener("click", () => setRenderMode("view"));
      rawModeButton.addEventListener("click", () => setRenderMode("raw"));

      function connect() {
        if (socket) {
          socket.close();
        }

        timeline.length = 0;
        activeChannelId = null;
        messagesEl.replaceChildren();
        requestedChannelSlug = (channelInput.value.trim() || "general")
          .toLowerCase()
          .replace(/[^a-z0-9_-]+/g, "-");
        const participant = encodeURIComponent(participantInput.value.trim() || "guest");
        const protocol = window.location.protocol === "https:" ? "wss" : "ws";
        socket = new WebSocket(`${protocol}://${window.location.host}/ws?participant=${participant}`);
        setConnected(false, "Koplar til ...");

        socket.addEventListener("open", () => {
          setConnected(true, `Tilkopla ${requestedChannelSlug} som ${decodeURIComponent(participant)}`);
          sendCommand("hello");
          sendCommand("list_my_channels");
        });

        socket.addEventListener("message", (event) => {
          renderServerEvent(JSON.parse(event.data));
        });

        socket.addEventListener("close", () => {
          setConnected(false, "Fråkopla");
        });

        socket.addEventListener("error", () => {
          setConnected(false, "WebSocket-feil");
        });
      }

      function sendCommand(type, payload) {
        requestNumber += 1;
        const command = {
          protocol: "sproyt.chat.v1",
          request_id: `browser-${requestNumber}`,
          type
        };
        if (payload !== undefined) {
          command.payload = payload;
        }
        socket.send(JSON.stringify(command));
      }

      function setConnected(connected, status) {
        statusEl.textContent = status;
        bodyInput.disabled = !connected;
        sendButton.disabled = !connected;
      }

      function setRenderMode(mode) {
        renderMode = mode;
        viewModeButton.setAttribute("aria-pressed", String(mode === "view"));
        rawModeButton.setAttribute("aria-pressed", String(mode === "raw"));
        renderTimeline();
      }

      function renderServerEvent(event) {
        if (event.protocol !== "sproyt.chat.v1") {
          pushSystem("Serveren svarte med ein ukjend protokoll.");
          return;
        }
        const payload = event.payload || {};

        if (event.type === "channels_listed") {
          const existing = payload.channels.find((channel) => channel.slug === requestedChannelSlug);
          if (existing) {
            activeChannelId = existing.id;
            sendCommand("subscribe_channel", { channel_id: activeChannelId });
          } else {
            sendCommand("create_channel", {
              slug: requestedChannelSlug,
              name: requestedChannelSlug,
              kind: "private"
            });
          }
          return;
        }

        if (event.type === "channel_created") {
          activeChannelId = payload.channel.id;
          sendCommand("subscribe_channel", { channel_id: activeChannelId });
          return;
        }

        if (event.type === "subscription_started") {
          activeChannelId = payload.channel_id;
          payload.history.forEach((message) => timeline.push({ type: "message", message }));
          renderTimeline();
          return;
        }

        if (event.type === "chat") {
          const chatEvent = payload.event;
          if (chatEvent.type === "message_accepted") {
            timeline.push({ type: "message", message: chatEvent.message });
            renderTimeline();
          } else if (chatEvent.type === "participant_joined") {
            pushSystem(`${chatEvent.participant_id} kom inn i ${chatEvent.channel_id}`);
          } else if (chatEvent.type === "participant_left") {
            pushSystem(`${chatEvent.participant_id} gjekk ut av ${chatEvent.channel_id}`);
          }
          return;
        }

        if (event.type === "lagged") {
          pushSystem(`Klienten låg etter og hoppa over ${payload.skipped} event; lastar inn att.`);
          sendCommand("load_recent_messages", {
            channel_id: payload.channel_id,
            after: payload.last_seen_sequence,
            limit: 200
          });
          return;
        }

        if (event.type === "messages_loaded") {
          payload.messages.forEach((message) => timeline.push({ type: "message", message }));
          renderTimeline();
          return;
        }

        if (event.type === "error") {
          pushSystem(payload.message || payload.code);
        }
      }

      function pushSystem(text) {
        timeline.push({ type: "system", text });
        renderTimeline();
      }

      function renderTimeline() {
        messagesEl.replaceChildren();
        for (const item of timeline) {
          if (item.type === "message") {
            appendMessage(item.message);
          } else {
            appendSystem(item.text);
          }
        }
        renderMermaidDiagrams();
        messagesEl.scrollTop = messagesEl.scrollHeight;
      }

      function renderMessage(message) {
        timeline.push({ type: "message", message });
        renderTimeline();
      }

      function appendMessage(message) {
        const wrapper = document.createElement("article");
        wrapper.className = "message";

        const meta = document.createElement("div");
        meta.className = "meta";
        meta.textContent = `${message.sender_id} #${message.sequence}`;

        const body = document.createElement("div");
        if (renderMode === "raw") {
          const pre = document.createElement("pre");
          pre.className = "raw-body";
          pre.textContent = message.body;
          body.append(pre);
        } else {
          body.className = "rendered";
          renderMarkdown(message.body, body);
        }

        wrapper.append(meta, body);
        messagesEl.append(wrapper);
      }

      function renderSystem(text) {
        pushSystem(text);
      }

      function appendSystem(text) {
        const line = document.createElement("div");
        line.className = "system";
        line.textContent = text;
        messagesEl.append(line);
      }

      function renderMarkdown(source, target) {
        const lines = source.replace(/\r\n/g, "\n").split("\n");
        let paragraph = [];
        let list = null;
        let inFence = false;
        let fenceLanguage = "";
        let fenceLines = [];

        const flushParagraph = () => {
          if (paragraph.length === 0) {
            return;
          }
          const p = document.createElement("p");
          appendInline(p, paragraph.join(" "));
          target.append(p);
          paragraph = [];
        };

        const flushList = () => {
          if (!list) {
            return;
          }
          target.append(list.element);
          list = null;
        };

        const flushFence = () => {
          const code = fenceLines.join("\n");
          if (fenceLanguage.toLowerCase() === "mermaid") {
            const shell = document.createElement("div");
            shell.className = "mermaid-shell";
            const diagram = document.createElement("div");
            diagram.className = "mermaid";
            diagram.textContent = code;
            shell.append(diagram);
            target.append(shell);
          } else {
            const pre = document.createElement("pre");
            const codeEl = document.createElement("code");
            if (fenceLanguage) {
              codeEl.dataset.language = fenceLanguage;
            }
            codeEl.textContent = code;
            pre.append(codeEl);
            target.append(pre);
          }
          inFence = false;
          fenceLanguage = "";
          fenceLines = [];
        };

        for (const line of lines) {
          const fence = line.match(/^```([A-Za-z0-9_-]+)?\s*$/);
          if (fence) {
            if (inFence) {
              flushFence();
            } else {
              flushParagraph();
              flushList();
              inFence = true;
              fenceLanguage = fence[1] || "";
              fenceLines = [];
            }
            continue;
          }

          if (inFence) {
            fenceLines.push(line);
            continue;
          }

          if (/^\s*$/.test(line)) {
            flushParagraph();
            flushList();
            continue;
          }

          const heading = line.match(/^(#{1,3})\s+(.+)$/);
          if (heading) {
            flushParagraph();
            flushList();
            const level = String(heading[1].length);
            const h = document.createElement(`h${level}`);
            appendInline(h, heading[2]);
            target.append(h);
            continue;
          }

          const quote = line.match(/^>\s?(.+)$/);
          if (quote) {
            flushParagraph();
            flushList();
            const blockquote = document.createElement("blockquote");
            appendInline(blockquote, quote[1]);
            target.append(blockquote);
            continue;
          }

          const unordered = line.match(/^\s*[-*]\s+(.+)$/);
          const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
          if (unordered || ordered) {
            flushParagraph();
            const kind = ordered ? "ol" : "ul";
            if (!list || list.kind !== kind) {
              flushList();
              list = { kind, element: document.createElement(kind) };
            }
            const li = document.createElement("li");
            appendInline(li, (unordered || ordered)[1]);
            list.element.append(li);
            continue;
          }

          flushList();
          paragraph.push(line.trim());
        }

        if (inFence) {
          flushFence();
        }
        flushParagraph();
        flushList();
      }

      function appendInline(parent, text) {
        const parts = text.split(/(`[^`]+`)/g);
        for (const part of parts) {
          if (part.startsWith("`") && part.endsWith("`") && part.length > 1) {
            const code = document.createElement("code");
            code.textContent = part.slice(1, -1);
            parent.append(code);
          } else if (part.length > 0) {
            parent.append(document.createTextNode(part));
          }
        }
      }

      async function renderMermaidDiagrams() {
        if (renderMode !== "view") {
          return;
        }
        const diagrams = [...messagesEl.querySelectorAll(".mermaid")];
        for (const diagram of diagrams) {
          if (diagram.dataset.rendered) {
            continue;
          }
          diagram.dataset.rendered = "true";
          try {
            await mermaid.run({ nodes: [diagram] });
          } catch (error) {
            diagram.textContent = `Mermaid-feil: ${error.message || error}`;
          }
        }
      }
    </script>
  </body>
</html>
"##;
