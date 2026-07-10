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
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};

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
            }),
        }
    }
}

impl OperationalState {
    pub fn set_ready(&self, ready: bool) {
        self.inner.ready.store(ready, Ordering::Release);
    }

    fn is_ready(&self) -> bool {
        self.inner.ready.load(Ordering::Acquire)
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
        output
    }
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

pub async fn readyz(State(operations): State<OperationalState>) -> Response {
    if operations.is_ready() {
        (StatusCode::OK, "ready\n").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready\n").into_response()
    }
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
        let metrics = operations.metrics();
        assert!(metrics.contains("sproyt_ready 0"));
        assert!(!metrics.contains("message"));
    }
}
