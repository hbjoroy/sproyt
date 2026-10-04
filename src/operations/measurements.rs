//! Bounded server-owned measurements. Never label by a raw request path or user.
use super::OperationalState;
use std::{collections::BTreeMap, fmt::Write, sync::atomic::Ordering, time::Instant};

const BOUNDS_MICROS: [u64; 11] = [
    5_000, 10_000, 25_000, 50_000, 100_000, 250_000, 500_000, 1_000_000, 2_500_000, 5_000_000,
    10_000_000,
];
const MAX_SERIES: usize = 512;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Transport {
    WebSocket,
    Sse,
}
impl Transport {
    fn index(self) -> usize {
        match self {
            Self::WebSocket => 0,
            Self::Sse => 1,
        }
    }
}

pub(crate) struct ConnectionGuard {
    operations: OperationalState,
    transport: Transport,
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let index = self.transport.index();
        self.operations.inner.active_connections[index].fetch_sub(1, Ordering::Relaxed);
        self.operations.inner.closed_connections[index].fetch_add(1, Ordering::Relaxed);
    }
}

#[derive(Debug, Default)]
pub(super) struct Measurements {
    series: BTreeMap<(String, &'static str, &'static str), Sample>,
}
#[derive(Debug, Default)]
struct Sample {
    count: u64,
    micros: u64,
    buckets: [u64; 12],
}

pub(super) fn probe(route: &str) -> bool {
    matches!(
        route,
        "/healthz" | "/readyz" | "/metrics" | "/versionz" | "overflow-probe"
    )
}
fn method(method: &str) -> &'static str {
    match method {
        "GET" => "GET",
        "POST" => "POST",
        "PATCH" => "PATCH",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "OPTIONS" => "OPTIONS",
        "HEAD" => "HEAD",
        _ => "OTHER",
    }
}
fn status(status: u16) -> &'static str {
    match status {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        500..=599 => "5xx",
        _ => "cancelled",
    }
}

impl Measurements {
    pub(super) fn observe(
        &mut self,
        route: &str,
        method_name: &str,
        status_code: u16,
        micros: u64,
    ) {
        let mut key = (route.to_owned(), method(method_name), status(status_code));
        if self.series.len() >= MAX_SERIES && !self.series.contains_key(&key) {
            key = (
                if probe(route) {
                    "overflow-probe"
                } else {
                    "overflow"
                }
                .into(),
                "OTHER",
                "other",
            );
        }
        let sample = self.series.entry(key).or_default();
        sample.count = sample.count.saturating_add(1);
        sample.micros = sample.micros.saturating_add(micros);
        let bucket = BOUNDS_MICROS
            .iter()
            .position(|bound| micros <= *bound)
            .unwrap_or(11);
        sample.buckets[bucket] = sample.buckets[bucket].saturating_add(1);
    }
    pub(super) fn render(&self, output: &mut String) {
        writeln!(output,"# HELP sproyt_http_response_duration_seconds Time to response headers, including cancelled handlers; excludes streaming body lifetime.").unwrap();
        writeln!(
            output,
            "# TYPE sproyt_http_response_duration_seconds histogram"
        )
        .unwrap();
        for ((route, method, status), sample) in &self.series {
            let traffic = if probe(route) { "probe" } else { "application" };
            let route = route
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n");
            let labels = format!(
                "traffic=\"{traffic}\",route=\"{route}\",method=\"{method}\",status_class=\"{status}\""
            );
            let mut cumulative = 0u64;
            for (index, count) in sample.buckets.iter().enumerate() {
                cumulative = cumulative.saturating_add(*count);
                let bound = BOUNDS_MICROS
                    .get(index)
                    .map(|v| (*v as f64 / 1_000_000.0).to_string())
                    .unwrap_or_else(|| "+Inf".into());
                writeln!(output,"sproyt_http_response_duration_seconds_bucket{{{labels},le=\"{bound}\"}} {cumulative}").unwrap();
            }
            writeln!(
                output,
                "sproyt_http_response_duration_seconds_count{{{labels}}} {}",
                sample.count
            )
            .unwrap();
            writeln!(
                output,
                "sproyt_http_response_duration_seconds_sum{{{labels}}} {}",
                sample.micros as f64 / 1_000_000.0
            )
            .unwrap();
        }
    }
}

pub(super) struct RequestGuard {
    operations: OperationalState,
    route: String,
    method: String,
    started: Instant,
    status: u16,
}
impl RequestGuard {
    pub(super) fn new(operations: OperationalState, route: &str, method: &str) -> Self {
        let route = if route.len() <= 240 { route } else { "other" }.to_owned();
        if !probe(&route) {
            operations.inner.requests.fetch_add(1, Ordering::Relaxed);
            operations.inner.in_flight.fetch_add(1, Ordering::Relaxed);
        }
        Self {
            operations,
            route,
            method: method.to_owned(),
            started: Instant::now(),
            status: 0,
        }
    }
    pub(super) fn finish(&mut self, status: u16) {
        self.status = status;
    }
}
impl Drop for RequestGuard {
    fn drop(&mut self) {
        let micros = u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX);
        if !probe(&self.route) {
            self.operations
                .inner
                .in_flight
                .fetch_sub(1, Ordering::Relaxed);
            self.operations
                .inner
                .duration_micros
                .fetch_add(micros, Ordering::Relaxed);
            if self.status >= 500 {
                self.operations
                    .inner
                    .server_errors
                    .fetch_add(1, Ordering::Relaxed);
            }
        }
        self.operations
            .inner
            .measurements
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .observe(&self.route, &self.method, self.status, micros);
    }
}

impl OperationalState {
    pub(crate) fn connection_started(&self, transport: Transport) -> ConnectionGuard {
        let index = transport.index();
        self.inner.active_connections[index].fetch_add(1, Ordering::Relaxed);
        self.inner.opened_connections[index].fetch_add(1, Ordering::Relaxed);
        ConnectionGuard {
            operations: self.clone(),
            transport,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_is_cumulative_in_seconds_and_series_are_bounded() {
        let mut metrics = Measurements::default();
        metrics.observe("/api/v1/channels/{id}", "GET", 200, 5_000);
        metrics.observe("/api/v1/channels/{id}", "GET", 200, 2_000_000);
        let mut text = String::new();
        metrics.render(&mut text);
        assert!(text.contains("le=\"0.005\"} 1"));
        assert!(text.contains("le=\"2.5\"} 2"));
        assert!(text.contains("le=\"+Inf\"} 2"));
        assert!(text.contains("} 2.005"));
        for n in 0..600 {
            metrics.observe(&format!("/fixed-{n}"), "UNRECOGNIZED", 404, 1);
        }
        metrics.observe("/metrics", "GET", 200, 3_000);
        assert!(metrics.series.len() <= MAX_SERIES + 2);
        text.clear();
        metrics.render(&mut text);
        assert!(text.contains("traffic=\"probe\",route=\"overflow-probe\""));
        assert!(!text.contains("traffic=\"application\",route=\"overflow-probe\""));
    }
    #[tokio::test]
    async fn cancellation_releases_server_owned_gauges_and_does_not_count_probe_as_app() {
        let ops = OperationalState::default();
        {
            let _probe = RequestGuard::new(ops.clone(), "/healthz", "GET");
        }
        assert_eq!(ops.inner.requests.load(Ordering::Relaxed), 0);
        let task_ops = ops.clone();
        let (started, ready) = tokio::sync::oneshot::channel();
        let handle = tokio::spawn(async move {
            let _request = RequestGuard::new(task_ops.clone(), "/api/v1/{id}", "GET");
            let _ws = task_ops.connection_started(Transport::WebSocket);
            let _sse = task_ops.connection_started(Transport::Sse);
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        assert_eq!(ops.inner.active_connections[0].load(Ordering::Relaxed), 1);
        assert_eq!(ops.inner.active_connections[1].load(Ordering::Relaxed), 1);
        handle.abort();
        let _ = handle.await;
        assert_eq!(ops.inner.in_flight.load(Ordering::Relaxed), 0);
        assert_eq!(ops.inner.active_connections[0].load(Ordering::Relaxed), 0);
        assert_eq!(ops.inner.active_connections[1].load(Ordering::Relaxed), 0);
        assert_eq!(ops.inner.closed_connections[0].load(Ordering::Relaxed), 1);
        assert_eq!(
            OperationalState::default().inner.active_connections[0].load(Ordering::Relaxed),
            0
        );
    }
}
