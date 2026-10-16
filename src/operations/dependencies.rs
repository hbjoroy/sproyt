//! Process-local, fixed-cardinality transport and Heart observations.
use super::Transport;
use std::{
    fmt::Write,
    sync::{Mutex, OnceLock},
    time::Instant,
};

const BOUNDS: [u64; 8] = [
    5_000, 25_000, 100_000, 500_000, 1_000_000, 5_000_000, 15_000_000, 30_000_000,
];

#[derive(Clone, Copy)]
pub(crate) enum TransportEvent {
    EnvelopeSent,
    SendFailed,
    ReceiveFailed,
    BodyFrameYielded,
    QueueBackpressure,
    QueueClosed,
    BroadcastLagged,
    BroadcastSkipped,
    ResumeRequested,
}
impl TransportEvent {
    const ALL: [(Self, &'static str); 9] = [
        (Self::EnvelopeSent, "envelope_sent"),
        (Self::SendFailed, "send_failed"),
        (Self::ReceiveFailed, "receive_failed"),
        (Self::BodyFrameYielded, "body_frame_yielded"),
        (Self::QueueBackpressure, "queue_backpressure"),
        (Self::QueueClosed, "queue_closed"),
        (Self::BroadcastLagged, "broadcast_lagged"),
        (Self::BroadcastSkipped, "broadcast_skipped"),
        (Self::ResumeRequested, "resume_requested"),
    ];
}

#[derive(Clone, Copy)]
pub(crate) enum HeartMethod {
    Get,
    Post,
    Other,
}
impl HeartMethod {
    const ALL: [(Self, &'static str); 3] = [
        (Self::Get, "GET"),
        (Self::Post, "POST"),
        (Self::Other, "OTHER"),
    ];
}
#[derive(Clone, Copy)]
pub(crate) enum HeartResult {
    Success,
    Timeout,
    Transport,
    Http4xx,
    Http5xx,
    HttpOther,
    InvalidResponse,
    Cancelled,
}
impl HeartResult {
    const ALL: [(Self, &'static str); 8] = [
        (Self::Success, "success"),
        (Self::Timeout, "timeout"),
        (Self::Transport, "transport"),
        (Self::Http4xx, "http_4xx"),
        (Self::Http5xx, "http_5xx"),
        (Self::HttpOther, "http_other"),
        (Self::InvalidResponse, "invalid_response"),
        (Self::Cancelled, "cancelled"),
    ];
}

#[derive(Default)]
struct Sample {
    count: u64,
    micros: u64,
    buckets: [u64; 9],
}
#[derive(Default)]
struct Metrics {
    transport: [[u64; 9]; 2],
    heart: [[Sample; 8]; 3],
}
static METRICS: OnceLock<Mutex<Metrics>> = OnceLock::new();
fn metrics() -> &'static Mutex<Metrics> {
    METRICS.get_or_init(Mutex::default)
}
fn transport_index(transport: Transport) -> usize {
    match transport {
        Transport::WebSocket => 0,
        Transport::Sse => 1,
    }
}
pub(crate) fn transport_event(transport: Transport, event: TransportEvent, count: u64) {
    metrics()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .transport_event(transport, event, count);
}
impl Metrics {
    fn transport_event(&mut self, transport: Transport, event: TransportEvent, count: u64) {
        let counter = &mut self.transport[transport_index(transport)][event as usize];
        *counter = counter.saturating_add(count);
    }
    fn observe_heart(&mut self, method: HeartMethod, result: HeartResult, micros: u64) {
        let sample = &mut self.heart[method as usize][result as usize];
        sample.count = sample.count.saturating_add(1);
        sample.micros = sample.micros.saturating_add(micros);
        let bucket = BOUNDS
            .iter()
            .position(|bound| micros <= *bound)
            .unwrap_or(8);
        sample.buckets[bucket] = sample.buckets[bucket].saturating_add(1);
    }
    fn render(&self) -> String {
        let mut output = String::new();
        writeln!(output, "# HELP sproyt_transport_events_total Server transport observations; SSE body frames are not delivery acknowledgements; resume requests are not inferred reconnects.").unwrap();
        writeln!(output, "# TYPE sproyt_transport_events_total counter").unwrap();
        for (index, transport) in ["websocket", "sse"].iter().enumerate() {
            for (event, label) in TransportEvent::ALL {
                writeln!(output, "sproyt_transport_events_total{{transport=\"{transport}\",event=\"{label}\"}} {}", self.transport[index][event as usize]).unwrap();
            }
        }
        writeln!(output, "# HELP sproyt_heart_attempt_duration_seconds Individual Heart HTTP attempts including response decoding; retry backoff excluded; cancellation recorded.").unwrap();
        writeln!(
            output,
            "# TYPE sproyt_heart_attempt_duration_seconds histogram"
        )
        .unwrap();
        for (method, method_label) in HeartMethod::ALL {
            for (result, result_label) in HeartResult::ALL {
                let sample = &self.heart[method as usize][result as usize];
                let labels = format!("method=\"{method_label}\",result=\"{result_label}\"");
                let mut count = 0u64;
                for (index, bucket) in sample.buckets.iter().enumerate() {
                    count = count.saturating_add(*bucket);
                    let bound = BOUNDS
                        .get(index)
                        .map(|v| (*v as f64 / 1_000_000.0).to_string())
                        .unwrap_or_else(|| "+Inf".into());
                    writeln!(output, "sproyt_heart_attempt_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {count}").unwrap();
                }
                writeln!(
                    output,
                    "sproyt_heart_attempt_duration_seconds_count{{{labels}}} {}",
                    sample.count
                )
                .unwrap();
                writeln!(
                    output,
                    "sproyt_heart_attempt_duration_seconds_sum{{{labels}}} {}",
                    sample.micros as f64 / 1_000_000.0
                )
                .unwrap();
            }
        }
        output
    }
}
pub(crate) fn render() -> String {
    metrics().lock().unwrap_or_else(|e| e.into_inner()).render()
}

/// Dropping an in-flight future records cancellation instead of a false success.
pub(crate) struct HeartAttempt {
    method: HeartMethod,
    result: HeartResult,
    started: Instant,
    collector: &'static Mutex<Metrics>,
}
impl HeartAttempt {
    pub(crate) fn new(method: HeartMethod) -> Self {
        Self {
            method,
            result: HeartResult::Cancelled,
            started: Instant::now(),
            collector: metrics(),
        }
    }
    pub(crate) fn finish(&mut self, result: HeartResult) {
        self.result = result;
    }
}
impl Drop for HeartAttempt {
    fn drop(&mut self) {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.collector
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe_heart(self.method, self.result, micros);
    }
}

fn error_result(error: &reqwest::Error) -> HeartResult {
    if error.is_timeout() {
        HeartResult::Timeout
    } else {
        HeartResult::Transport
    }
}
fn status_result(status: reqwest::StatusCode) -> HeartResult {
    if status.is_success() {
        HeartResult::Success
    } else if status.is_client_error() {
        HeartResult::Http4xx
    } else if status.is_server_error() {
        HeartResult::Http5xx
    } else {
        HeartResult::HttpOther
    }
}

async fn begin_request(
    request: reqwest::RequestBuilder,
) -> Result<(reqwest::Response, HeartAttempt), String> {
    // RequestBuilder::send uses the same client's execute path. Extract only
    // the method; neither the URL nor body can become a metric label.
    let (client, request) = request.build_split();
    let request = request.map_err(|error| error.to_string())?;
    let method = match *request.method() {
        reqwest::Method::GET => HeartMethod::Get,
        reqwest::Method::POST => HeartMethod::Post,
        _ => HeartMethod::Other,
    };
    let mut observation = HeartAttempt::new(method);
    match client.execute(request).await {
        Ok(response) => Ok((response, observation)),
        Err(error) => {
            observation.finish(error_result(&error));
            Err(error.to_string())
        }
    }
}

/// Existing v2 adapters use the same 2 MiB bound and do not follow redirects.
pub(crate) async fn heart_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, String> {
    let (mut response, mut observation) = begin_request(request).await?;
    if response.error_for_status_ref().is_err() {
        observation.finish(status_result(response.status()));
        return Err(response.error_for_status().unwrap_err().to_string());
    }
    let mut bytes = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|error| {
            observation.finish(error_result(&error));
            error.to_string()
        })?;
        let Some(chunk) = chunk else {
            break;
        };
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            observation.finish(HeartResult::InvalidResponse);
            return Err("Heart response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let decoded = serde_json::from_slice(&bytes).map_err(|error| error.to_string());
    observation.finish(if decoded.is_ok() {
        HeartResult::Success
    } else {
        HeartResult::InvalidResponse
    });
    decoded
}

/// Completion callers intentionally reconcile authoritative task state after
/// ambiguous responses. Observe status/latency without consuming their bodies.
pub(crate) async fn heart_completion(request: reqwest::RequestBuilder) -> Result<(), String> {
    let (response, mut observation) = begin_request(request).await?;
    observation.finish(status_result(response.status()));
    response
        .error_for_status()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_labels_histogram_errors_and_transport_semantics() {
        let mut metrics = Metrics::default();
        metrics.observe_heart(HeartMethod::Get, HeartResult::Success, 5_000);
        metrics.observe_heart(HeartMethod::Get, HeartResult::Success, 2_000_000);
        metrics.observe_heart(HeartMethod::Post, HeartResult::Http5xx, 100_000);
        metrics.transport_event(Transport::Sse, TransportEvent::ResumeRequested, 1);
        metrics.transport_event(Transport::WebSocket, TransportEvent::BroadcastLagged, 1);
        metrics.transport_event(Transport::WebSocket, TransportEvent::BroadcastSkipped, 27);
        let text = metrics.render();
        assert!(text.contains("method=\"GET\",result=\"success\",le=\"0.005\"} 1"));
        assert!(text.contains("method=\"GET\",result=\"success\",le=\"5\"} 2"));
        assert!(text.contains(
            "sproyt_heart_attempt_duration_seconds_sum{method=\"GET\",result=\"success\"} 2.005"
        ));
        assert!(text.contains("transport=\"websocket\",event=\"broadcast_skipped\"} 27"));
        assert!(text.contains("transport=\"sse\",event=\"resume_requested\"} 1"));
        assert!(text.contains("transport=\"websocket\",event=\"resume_requested\"} 0"));
        assert_eq!(
            text.lines().filter(|line| !line.starts_with('#')).count(),
            18 + 24 * 11
        );
        assert!(!text.contains("channel_id"));
        assert!(!text.contains("user_id"));
        assert!(!text.contains("url="));
        assert!(!text.contains("reconnect="));
        assert!(
            Metrics::default()
                .render()
                .contains("method=\"POST\",result=\"http_5xx\"} 0")
        );
    }

    #[tokio::test]
    async fn cancelled_attempt_is_counted_once_without_success() {
        let collector: &'static Mutex<Metrics> =
            Box::leak(Box::new(Mutex::new(Metrics::default())));
        let guard = HeartAttempt {
            method: HeartMethod::Get,
            result: HeartResult::Cancelled,
            started: Instant::now(),
            collector,
        };
        let future = async move {
            let _guard = guard;
            std::future::pending::<()>().await;
        };
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(1), future)
                .await
                .is_err()
        );
        let metrics = collector.lock().unwrap();
        assert_eq!(
            metrics.heart[HeartMethod::Get as usize][HeartResult::Cancelled as usize].count,
            1
        );
        assert_eq!(
            metrics.heart[HeartMethod::Get as usize][HeartResult::Success as usize].count,
            0
        );
    }

    #[tokio::test]
    async fn heart_adapters_preserve_bounded_decode_status_and_completion_behavior() {
        use axum::{
            Router,
            http::StatusCode,
            routing::{get, post},
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route(
                "/ok",
                get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
            )
            .route("/invalid", get(|| async { "not json" }))
            .route(
                "/oversized",
                get(|| async { "x".repeat(2 * 1024 * 1024 + 1) }),
            )
            .route(
                "/unavailable",
                get(|| async { StatusCode::SERVICE_UNAVAILABLE }),
            )
            .route("/complete", post(|| async { StatusCode::NO_CONTENT }));
        let server = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let client = reqwest::Client::new();
        let url = |path| format!("http://{address}/{path}");
        let decoded: serde_json::Value = heart_json(client.get(url("ok"))).await.unwrap();
        assert_eq!(decoded["ok"], true);
        assert!(
            heart_json::<serde_json::Value>(client.get(url("invalid")))
                .await
                .is_err()
        );
        assert_eq!(
            heart_json::<serde_json::Value>(client.get(url("oversized")))
                .await
                .unwrap_err(),
            "Heart response too large"
        );
        assert!(
            heart_json::<serde_json::Value>(client.get(url("unavailable")))
                .await
                .is_err()
        );
        assert!(heart_completion(client.post(url("complete"))).await.is_ok());
        assert!(matches!(
            status_result(StatusCode::NOT_FOUND),
            HeartResult::Http4xx
        ));
        assert!(matches!(
            status_result(StatusCode::SERVICE_UNAVAILABLE),
            HeartResult::Http5xx
        ));
        assert!(matches!(
            status_result(StatusCode::TEMPORARY_REDIRECT),
            HeartResult::HttpOther
        ));
        server.abort();
    }
}
