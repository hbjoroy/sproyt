use std::{
    fmt::Write,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use tokio::sync::watch;

#[derive(Clone, Debug)]
pub struct OperationalState {
    inner: Arc<OperationalStateInner>,
}

#[derive(Debug)]
struct OperationalStateInner {
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
    shutdown: watch::Sender<bool>,
}

impl Default for OperationalState {
    fn default() -> Self {
        Self {
            inner: Arc::new(OperationalStateInner {
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
                shutdown: watch::channel(false).0,
            }),
        }
    }
}

impl OperationalState {
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
        output
    }
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
    operations.inner.requests.fetch_add(1, Ordering::Relaxed);
    operations.inner.in_flight.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();

    let response = next.run(request).await;

    operations.inner.in_flight.fetch_sub(1, Ordering::Relaxed);
    if response.status().is_server_error() {
        operations
            .inner
            .server_errors
            .fetch_add(1, Ordering::Relaxed);
    }
    let elapsed = started.elapsed().as_micros();
    let elapsed = u64::try_from(elapsed).unwrap_or(u64::MAX);
    operations
        .inner
        .duration_micros
        .fetch_add(elapsed, Ordering::Relaxed);
    response
}

pub async fn healthz() -> &'static str {
    "ok\n"
}

pub async fn metrics(State(operations): State<OperationalState>) -> String {
    operations.metrics()
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
        let metrics = operations.metrics();
        assert!(metrics.contains("sproyt_ready 0"));
        assert!(metrics.contains("sproyt_client_events_total{event=\"websocket_disconnected\"} 1"));
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
}
