use std::{
    fmt::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use tokio::sync::watch;
mod measurements;
pub(crate) use measurements::Transport;

#[derive(Clone, Debug)]
pub struct OperationalState {
    inner: Arc<OperationalStateInner>,
}

#[derive(Debug)]
struct OperationalStateInner {
    measurements: Mutex<measurements::Measurements>,
    active_connections: [AtomicU64; 2],
    opened_connections: [AtomicU64; 2],
    closed_connections: [AtomicU64; 2],
    ready: AtomicBool,
    requests: AtomicU64,
    in_flight: AtomicU64,
    server_errors: AtomicU64,
    duration_micros: AtomicU64,
    client_ws_connected: AtomicU64,
    client_ws_disconnected: AtomicU64,
    client_ws_errors: AtomicU64,
    client_session_refresh_succeeded: AtomicU64,
    client_session_refresh_failed: AtomicU64,
    client_upload_succeeded: AtomicU64,
    client_upload_failed: AtomicU64,
    client_resume_recovery: AtomicU64,
    client_connect_timeout: AtomicU64,
    client_liveness_timeout: AtomicU64,
    integration_accepted: AtomicU64,
    integration_duplicate: AtomicU64,
    integration_ignored: AtomicU64,
    integration_rejected: AtomicU64,
    integration_error: AtomicU64,
    integration_duration_micros: AtomicU64,
    shutdown: watch::Sender<bool>,
}

impl Default for OperationalState {
    fn default() -> Self {
        Self {
            inner: Arc::new(OperationalStateInner {
                measurements: Mutex::default(),
                active_connections: Default::default(),
                opened_connections: Default::default(),
                closed_connections: Default::default(),
                ready: AtomicBool::new(false),
                requests: AtomicU64::new(0),
                in_flight: AtomicU64::new(0),
                server_errors: AtomicU64::new(0),
                duration_micros: AtomicU64::new(0),
                client_ws_connected: AtomicU64::new(0),
                client_ws_disconnected: AtomicU64::new(0),
                client_ws_errors: AtomicU64::new(0),
                client_session_refresh_succeeded: AtomicU64::new(0),
                client_session_refresh_failed: AtomicU64::new(0),
                client_upload_succeeded: AtomicU64::new(0),
                client_upload_failed: AtomicU64::new(0),
                client_resume_recovery: AtomicU64::new(0),
                client_connect_timeout: AtomicU64::new(0),
                client_liveness_timeout: AtomicU64::new(0),
                integration_accepted: AtomicU64::new(0),
                integration_duplicate: AtomicU64::new(0),
                integration_ignored: AtomicU64::new(0),
                integration_rejected: AtomicU64::new(0),
                integration_error: AtomicU64::new(0),
                integration_duration_micros: AtomicU64::new(0),
                shutdown: watch::channel(false).0,
            }),
        }
    }
}

impl OperationalState {
    pub fn record_integration(&self, outcome: IntegrationOutcome, elapsed_micros: u64) {
        let counter = match outcome {
            IntegrationOutcome::Accepted => &self.inner.integration_accepted,
            IntegrationOutcome::Duplicate => &self.inner.integration_duplicate,
            IntegrationOutcome::Ignored => &self.inner.integration_ignored,
            IntegrationOutcome::Rejected => &self.inner.integration_rejected,
            IntegrationOutcome::Error => &self.inner.integration_error,
        };
        counter.fetch_add(1, Ordering::Relaxed);
        self.inner
            .integration_duration_micros
            .fetch_add(elapsed_micros, Ordering::Relaxed);
    }

    pub fn record_client_event(&self, event: ClientEvent) {
        let counter = match event {
            ClientEvent::WebSocketConnected => &self.inner.client_ws_connected,
            ClientEvent::WebSocketDisconnected => &self.inner.client_ws_disconnected,
            ClientEvent::WebSocketError => &self.inner.client_ws_errors,
            ClientEvent::SessionRefreshSucceeded => &self.inner.client_session_refresh_succeeded,
            ClientEvent::SessionRefreshFailed => &self.inner.client_session_refresh_failed,
            ClientEvent::UploadSucceeded => &self.inner.client_upload_succeeded,
            ClientEvent::UploadFailed => &self.inner.client_upload_failed,
            ClientEvent::ResumeRecovery => &self.inner.client_resume_recovery,
            ClientEvent::ConnectTimeout => &self.inner.client_connect_timeout,
            ClientEvent::LivenessTimeout => &self.inner.client_liveness_timeout,
        };
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_ready(&self, ready: bool) {
        self.inner.ready.store(ready, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::Acquire)
    }

    pub fn begin_shutdown(&self) {
        self.set_ready(false);
        self.inner.shutdown.send_replace(true);
    }

    pub fn subscribe_shutdown(&self) -> watch::Receiver<bool> {
        self.inner.shutdown.subscribe()
    }

    fn metrics(&self) -> String {
        let mut output = String::with_capacity(512);
        let ready = u8::from(self.is_ready());
        let requests = self.inner.requests.load(Ordering::Relaxed);
        let in_flight = self.inner.in_flight.load(Ordering::Relaxed);
        let server_errors = self.inner.server_errors.load(Ordering::Relaxed);
        let duration_micros = self.inner.duration_micros.load(Ordering::Relaxed);

        writeln!(output, "# TYPE sproyt_ready gauge").expect("writing to a String cannot fail");
        writeln!(output, "sproyt_ready {ready}").expect("writing to a String cannot fail");
        writeln!(output, "# TYPE sproyt_http_requests_total counter")
            .expect("writing to a String cannot fail");
        writeln!(output, "sproyt_http_requests_total {requests}")
            .expect("writing to a String cannot fail");
        writeln!(output, "# TYPE sproyt_http_requests_in_flight gauge")
            .expect("writing to a String cannot fail");
        writeln!(output, "sproyt_http_requests_in_flight {in_flight}")
            .expect("writing to a String cannot fail");
        writeln!(output, "# TYPE sproyt_http_server_errors_total counter")
            .expect("writing to a String cannot fail");
        writeln!(output, "sproyt_http_server_errors_total {server_errors}")
            .expect("writing to a String cannot fail");
        writeln!(
            output,
            "# TYPE sproyt_http_request_duration_microseconds_total counter"
        )
        .expect("writing to a String cannot fail");
        writeln!(
            output,
            "sproyt_http_request_duration_microseconds_total {duration_micros}"
        )
        .expect("writing to a String cannot fail");
        writeln!(output, "# TYPE sproyt_client_events_total counter")
            .expect("writing to a String cannot fail");
        for (event, value) in [
            (
                "websocket_connected",
                self.inner.client_ws_connected.load(Ordering::Relaxed),
            ),
            (
                "websocket_disconnected",
                self.inner.client_ws_disconnected.load(Ordering::Relaxed),
            ),
            (
                "websocket_error",
                self.inner.client_ws_errors.load(Ordering::Relaxed),
            ),
            (
                "session_refresh_succeeded",
                self.inner
                    .client_session_refresh_succeeded
                    .load(Ordering::Relaxed),
            ),
            (
                "session_refresh_failed",
                self.inner
                    .client_session_refresh_failed
                    .load(Ordering::Relaxed),
            ),
            (
                "upload_succeeded",
                self.inner.client_upload_succeeded.load(Ordering::Relaxed),
            ),
            (
                "upload_failed",
                self.inner.client_upload_failed.load(Ordering::Relaxed),
            ),
            (
                "resume_recovery",
                self.inner.client_resume_recovery.load(Ordering::Relaxed),
            ),
            (
                "connect_timeout",
                self.inner.client_connect_timeout.load(Ordering::Relaxed),
            ),
            (
                "liveness_timeout",
                self.inner.client_liveness_timeout.load(Ordering::Relaxed),
            ),
        ] {
            writeln!(
                output,
                "sproyt_client_events_total{{event=\"{event}\"}} {value}"
            )
            .expect("writing to a String cannot fail");
        }
        writeln!(output, "# TYPE sproyt_integration_deliveries_total counter")
            .expect("writing to a String cannot fail");
        for (outcome, value) in [
            (
                "accepted",
                self.inner.integration_accepted.load(Ordering::Relaxed),
            ),
            (
                "duplicate",
                self.inner.integration_duplicate.load(Ordering::Relaxed),
            ),
            (
                "ignored",
                self.inner.integration_ignored.load(Ordering::Relaxed),
            ),
            (
                "rejected",
                self.inner.integration_rejected.load(Ordering::Relaxed),
            ),
            (
                "error",
                self.inner.integration_error.load(Ordering::Relaxed),
            ),
        ] {
            writeln!(
                output,
                "sproyt_integration_deliveries_total{{outcome=\"{outcome}\"}} {value}"
            )
            .expect("writing to a String cannot fail");
        }
        writeln!(
            output,
            "# TYPE sproyt_integration_duration_microseconds_total counter"
        )
        .expect("writing to a String cannot fail");
        writeln!(
            output,
            "sproyt_integration_duration_microseconds_total {}",
            self.inner
                .integration_duration_micros
                .load(Ordering::Relaxed)
        )
        .expect("writing to a String cannot fail");
        self.inner
            .measurements
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .render(&mut output);
        writeln!(output, "# HELP sproyt_connections_active Authenticated transport connections; not unique users. SSE channel streams count individually.").unwrap();
        writeln!(output, "# TYPE sproyt_connections_active gauge").unwrap();
        writeln!(output, "# TYPE sproyt_connections_opened_total counter").unwrap();
        writeln!(output, "# TYPE sproyt_connections_closed_total counter").unwrap();
        for (index, transport) in ["websocket", "sse"].iter().enumerate() {
            writeln!(
                output,
                "sproyt_connections_active{{transport=\"{transport}\"}} {}",
                self.inner.active_connections[index].load(Ordering::Relaxed)
            )
            .unwrap();
            writeln!(
                output,
                "sproyt_connections_opened_total{{transport=\"{transport}\"}} {}",
                self.inner.opened_connections[index].load(Ordering::Relaxed)
            )
            .unwrap();
            writeln!(
                output,
                "sproyt_connections_closed_total{{transport=\"{transport}\"}} {}",
                self.inner.closed_connections[index].load(Ordering::Relaxed)
            )
            .unwrap();
        }
        output
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IntegrationOutcome {
    Accepted,
    Duplicate,
    Ignored,
    Rejected,
    Error,
}

#[derive(Clone, Copy, Debug)]
pub enum ClientEvent {
    WebSocketConnected,
    WebSocketDisconnected,
    WebSocketError,
    SessionRefreshSucceeded,
    SessionRefreshFailed,
    UploadSucceeded,
    UploadFailed,
    ResumeRecovery,
    ConnectTimeout,
    LivenessTimeout,
}

pub async fn record_metrics(
    State(operations): State<OperationalState>,
    request: Request,
    next: Next,
) -> Response {
    // MatchedPath comes from the router, never from a client's path/query/IDs.
    let route = request
        .extensions()
        .get::<axum::extract::MatchedPath>()
        .map_or("unmatched", |route| route.as_str());
    let mut guard = measurements::RequestGuard::new(operations, route, request.method().as_str());
    let response = next.run(request).await;
    guard.finish(response.status().as_u16());
    response
}

pub async fn healthz() -> &'static str {
    "ok\n"
}

pub async fn metrics(State(operations): State<OperationalState>) -> String {
    operations.metrics() + &crate::chatbot::memory::builder::metrics()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_defaults_to_false_and_can_change() {
        let operations = OperationalState::default();
        assert!(!operations.is_ready());
        operations.set_ready(true);
        assert!(operations.is_ready());
    }

    #[test]
    fn metrics_do_not_expose_application_data() {
        let operations = OperationalState::default();
        operations.record_client_event(ClientEvent::WebSocketDisconnected);
        operations.record_integration(IntegrationOutcome::Duplicate, 42);
        let metrics = operations.metrics();
        assert!(metrics.contains("sproyt_ready 0"));
        assert!(metrics.contains("sproyt_client_events_total{event=\"websocket_disconnected\"} 1"));
        assert!(metrics.contains("sproyt_integration_deliveries_total{outcome=\"duplicate\"} 1"));
        assert!(metrics.contains("sproyt_integration_duration_microseconds_total 42"));
        assert!(!metrics.contains("message"));
    }

    #[tokio::test]
    async fn shutdown_is_retained_for_existing_and_late_subscribers() {
        let operations = OperationalState::default();
        operations.set_ready(true);
        let mut existing = operations.subscribe_shutdown();

        operations.begin_shutdown();

        existing.changed().await.unwrap();
        assert!(*existing.borrow());
        assert!(*operations.subscribe_shutdown().borrow());
        assert!(!operations.is_ready());
    }

    #[tokio::test]
    async fn middleware_uses_route_templates_and_separates_probe_traffic() {
        use axum::{Router, middleware, routing::get};
        let operations = OperationalState::default();
        let router = Router::new()
            .route("/items/{id}", get(|| async { "ok" }))
            .route("/healthz", get(healthz))
            .layer(middleware::from_fn_with_state(
                operations.clone(),
                record_metrics,
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::new();
        for path in ["/items/private-person?token=private-token", "/healthz"] {
            let response = client
                .get(format!("http://{address}{path}"))
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
        }
        let metrics = operations.metrics();
        assert!(metrics.contains("sproyt_http_requests_total 1\n"));
        assert!(metrics.contains("sproyt_http_requests_in_flight 0\n"));
        assert!(metrics.contains("traffic=\"application\",route=\"/items/{id}\""));
        assert!(metrics.contains("traffic=\"probe\",route=\"/healthz\""));
        assert!(!metrics.contains("private-person"));
        assert!(!metrics.contains("private-token"));
        server.abort();
    }
}
