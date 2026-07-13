use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::{sleep, timeout};
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::domain::{ChannelId, RepositoryError, UserId};

pub type GatewayFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ProcessError>> + Send + 'a>>;
pub type ProcessRepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcessLinkId(Uuid);

impl ProcessLinkId {
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }

    pub fn parse(value: &str) -> Result<Self, uuid::Error> {
        Uuid::parse_str(value).map(Self)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct OutboxId(Uuid);

impl OutboxId {
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnqueueProcessStart {
    pub channel_id: ChannelId,
    pub actor: UserId,
    pub request_id: String,
    pub namespace: String,
    pub definition_name: String,
    pub definition_version: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug)]
pub struct EnqueueCorrelation {
    pub process_link_id: ProcessLinkId,
    pub actor: UserId,
    pub request_id: String,
    pub payload: Value,
}

#[derive(Clone, Debug)]
pub struct EnqueueInspection {
    pub process_link_id: ProcessLinkId,
    pub actor: UserId,
    pub request_id: String,
}

#[derive(Clone, Debug)]
pub struct SetCircleFeature {
    pub circle_id: crate::domain::CircleId,
    pub actor: UserId,
    pub feature: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessLink {
    pub id: ProcessLinkId,
    pub channel_id: ChannelId,
    pub heart_instance_id: Option<Uuid>,
    pub namespace: String,
    pub definition_name: String,
    pub definition_version: Option<String>,
    pub initiated_by: UserId,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessEvent {
    pub id: Uuid,
    pub event_type: String,
    pub payload: Value,
    pub actor_id: UserId,
    pub occurred_at: DateTime<Utc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProcessView {
    pub process: ProcessLink,
    pub events: Vec<ProcessEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum OutboxOperation {
    Start { command: StartProcess },
    Correlate { command: CorrelateMessage },
    Inspect { instance_id: Uuid },
}

#[derive(Clone, Debug)]
pub struct OutboxJob {
    pub id: OutboxId,
    pub process_link_id: ProcessLinkId,
    pub operation: OutboxOperation,
    pub attempts: u32,
}

pub trait ProcessRepository: Send + Sync + 'static {
    fn enqueue_start<'a>(
        &'a self,
        command: EnqueueProcessStart,
    ) -> ProcessRepositoryFuture<'a, ProcessLink>;
    fn enqueue_correlation<'a>(
        &'a self,
        command: EnqueueCorrelation,
    ) -> ProcessRepositoryFuture<'a, OutboxId>;
    fn get_process<'a>(
        &'a self,
        actor: UserId,
        process_link_id: ProcessLinkId,
    ) -> ProcessRepositoryFuture<'a, ProcessView>;
    fn enqueue_inspection<'a>(
        &'a self,
        command: EnqueueInspection,
    ) -> ProcessRepositoryFuture<'a, OutboxId>;
    fn set_circle_feature<'a>(
        &'a self,
        command: SetCircleFeature,
    ) -> ProcessRepositoryFuture<'a, ()>;
    fn lease_next<'a>(
        &'a self,
        lease_for: Duration,
    ) -> ProcessRepositoryFuture<'a, Option<OutboxJob>>;
    fn complete_start<'a>(
        &'a self,
        job: OutboxJob,
        result: StartedProcess,
    ) -> ProcessRepositoryFuture<'a, ()>;
    fn complete_operation<'a>(
        &'a self,
        job: OutboxJob,
        event_type: &'a str,
        payload: Value,
    ) -> ProcessRepositoryFuture<'a, ()>;
    fn reschedule<'a>(
        &'a self,
        job: OutboxJob,
        error: ProcessError,
        delay: Duration,
    ) -> ProcessRepositoryFuture<'a, ()>;
}

pub type SharedProcessRepository = Arc<dyn ProcessRepository>;

#[derive(Clone)]
pub struct ProcessService {
    repository: SharedProcessRepository,
}

impl ProcessService {
    pub fn start(
        repository: SharedProcessRepository,
        gateway: Option<SharedProcessGateway>,
    ) -> Self {
        if let Some(gateway) = gateway {
            tokio::spawn(run_outbox(repository.clone(), gateway));
        }
        Self { repository }
    }

    pub async fn enqueue_start(
        &self,
        command: EnqueueProcessStart,
    ) -> Result<ProcessLink, RepositoryError> {
        self.repository.enqueue_start(command).await
    }

    pub async fn enqueue_correlation(
        &self,
        command: EnqueueCorrelation,
    ) -> Result<OutboxId, RepositoryError> {
        self.repository.enqueue_correlation(command).await
    }

    pub async fn get_process(
        &self,
        actor: UserId,
        process_link_id: ProcessLinkId,
    ) -> Result<ProcessView, RepositoryError> {
        self.repository.get_process(actor, process_link_id).await
    }

    pub async fn enqueue_inspection(
        &self,
        command: EnqueueInspection,
    ) -> Result<OutboxId, RepositoryError> {
        self.repository.enqueue_inspection(command).await
    }

    pub async fn set_circle_feature(
        &self,
        command: SetCircleFeature,
    ) -> Result<(), RepositoryError> {
        self.repository.set_circle_feature(command).await
    }
}

async fn run_outbox(repository: SharedProcessRepository, gateway: SharedProcessGateway) {
    loop {
        match repository.lease_next(Duration::from_secs(30)).await {
            Ok(Some(job)) => {
                let outcome: Result<WorkerResult, ProcessError> = match &job.operation {
                    OutboxOperation::Start { command } => gateway.start(command, job.id.as_uuid()).await.map(WorkerResult::Started),
                    OutboxOperation::Correlate { command } => gateway.correlate(command, job.id.as_uuid()).await.map(|result| WorkerResult::Event("process.correlated", serde_json::json!({"matched_instances": result.matched_instances, "instance_ids": result.instance_ids}))),
                    OutboxOperation::Inspect { instance_id } => gateway.inspect(*instance_id, job.id.as_uuid()).await.map(|result| WorkerResult::Event("process.inspected", serde_json::json!({"instance_id": result.id, "status": result.status, "current_node": result.current_node}))),
                };
                match outcome {
                    Ok(WorkerResult::Started(result)) => {
                        if let Err(error) = repository.complete_start(job, result).await {
                            warn!(
                                error_kind = error.kind(),
                                "failed to complete process outbox job"
                            );
                        }
                    }
                    Ok(WorkerResult::Event(event_type, payload)) => {
                        if let Err(error) = repository
                            .complete_operation(job, event_type, payload)
                            .await
                        {
                            warn!(
                                error_kind = error.kind(),
                                "failed to complete process outbox job"
                            );
                        }
                    }
                    Err(error) => {
                        let delay = backoff(job.attempts as usize);
                        if let Err(repository_error) =
                            repository.reschedule(job, error, delay).await
                        {
                            warn!(
                                error_kind = repository_error.kind(),
                                "failed to reschedule process outbox job"
                            );
                        }
                    }
                }
            }
            Ok(None) => sleep(Duration::from_millis(500)).await,
            Err(error) => {
                warn!(error_kind = error.kind(), "process outbox poll failed");
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
}

enum WorkerResult {
    Started(StartedProcess),
    Event(&'static str, Value),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessErrorKind {
    InvalidRequest,
    Unauthorized,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    Timeout,
    InvalidResponse,
}

impl ProcessErrorKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::Unauthorized => "unauthorized",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "unavailable",
            Self::Timeout => "timeout",
            Self::InvalidResponse => "invalid_response",
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
#[error("Heart {kind:?}: {message}")]
pub struct ProcessError {
    pub kind: ProcessErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl ProcessError {
    fn status(status: StatusCode) -> Self {
        let (kind, message, retryable) = match status {
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => (
                ProcessErrorKind::InvalidRequest,
                "Heart rejected request",
                false,
            ),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => (
                ProcessErrorKind::Unauthorized,
                "Heart authorization failed",
                false,
            ),
            StatusCode::NOT_FOUND => (
                ProcessErrorKind::NotFound,
                "Heart resource not found",
                false,
            ),
            StatusCode::CONFLICT => (ProcessErrorKind::Conflict, "Heart conflict", false),
            StatusCode::TOO_MANY_REQUESTS => {
                (ProcessErrorKind::RateLimited, "Heart rate limited", true)
            }
            _ if status.is_server_error() => {
                (ProcessErrorKind::Unavailable, "Heart unavailable", true)
            }
            _ => (
                ProcessErrorKind::InvalidResponse,
                "Heart returned an invalid response",
                false,
            ),
        };
        Self {
            kind,
            message: message.to_owned(),
            retryable,
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            kind: ProcessErrorKind::Unavailable,
            message: message.into(),
            retryable: true,
        }
    }

    fn timeout() -> Self {
        Self {
            kind: ProcessErrorKind::Timeout,
            message: "request timed out; external outcome is uncertain".to_owned(),
            retryable: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StartProcess {
    pub namespace: String,
    pub definition_name: String,
    pub version: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StartedProcess {
    pub instance_id: Uuid,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ProcessInstance {
    pub id: Uuid,
    pub definition_id: Uuid,
    pub namespace: String,
    pub status: String,
    pub current_node: Option<String>,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CorrelateMessage {
    pub namespace: String,
    pub correlation_key: String,
    pub correlation_value: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CorrelatedMessage {
    pub matched_instances: usize,
    pub instance_ids: Vec<Uuid>,
}

pub trait ProcessGateway: Send + Sync {
    fn start<'a>(
        &'a self,
        command: &'a StartProcess,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, StartedProcess>;

    fn inspect<'a>(
        &'a self,
        instance_id: Uuid,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, ProcessInstance>;

    fn correlate<'a>(
        &'a self,
        command: &'a CorrelateMessage,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, CorrelatedMessage>;
}

pub type SharedProcessGateway = Arc<dyn ProcessGateway>;

#[derive(Clone)]
pub struct HeartGateway {
    client: Client,
    base_url: String,
    timeout: Duration,
    retries: usize,
}

impl HeartGateway {
    pub fn new(
        base_url: impl Into<String>,
        timeout: Duration,
        retries: usize,
    ) -> Result<Self, ProcessError> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        let parsed = reqwest::Url::parse(&base_url).map_err(|_| ProcessError {
            kind: ProcessErrorKind::InvalidRequest,
            message: "Heart base URL must be an absolute http or https URL".to_owned(),
            retryable: false,
        })?;
        if !matches!(parsed.scheme(), "http" | "https")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(ProcessError {
                kind: ProcessErrorKind::InvalidRequest,
                message: "Heart base URL must not contain credentials, query, or fragment"
                    .to_owned(),
                retryable: false,
            });
        }
        Ok(Self {
            client: Client::new(),
            base_url,
            timeout,
            retries,
        })
    }

    async fn execute<T>(
        &self,
        make_request: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<T, ProcessError>
    where
        T: for<'de> Deserialize<'de>,
    {
        for attempt in 0..=self.retries {
            let result = timeout(self.timeout, make_request().send()).await;
            let response = match result {
                Err(_) if attempt < self.retries => {
                    sleep(backoff(attempt)).await;
                    continue;
                }
                Err(_) => return Err(ProcessError::timeout()),
                Ok(Err(_)) if attempt < self.retries => {
                    warn!(attempt, error_kind = "transport", "retrying Heart request");
                    sleep(backoff(attempt)).await;
                    continue;
                }
                Ok(Err(_)) => return Err(ProcessError::unavailable("Heart transport failure")),
                Ok(Ok(response)) => response,
            };
            let status = response.status();
            if !status.is_success() {
                let error = ProcessError::status(status);
                if error.retryable && attempt < self.retries {
                    warn!(
                        attempt,
                        error_kind = error.kind.as_str(),
                        "retrying Heart request"
                    );
                    sleep(backoff(attempt)).await;
                    continue;
                }
                return Err(error);
            }
            return response.json().await.map_err(|_| ProcessError {
                kind: ProcessErrorKind::InvalidResponse,
                message: "Heart returned invalid JSON".to_owned(),
                retryable: false,
            });
        }
        unreachable!("retry loop always returns")
    }
}

impl ProcessGateway for HeartGateway {
    #[instrument(skip_all, fields(%correlation_id))]
    fn start<'a>(
        &'a self,
        command: &'a StartProcess,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, StartedProcess> {
        Box::pin(async move {
            self.execute(|| {
                self.client
                    .post(format!("{}/api/v1/instances", self.base_url))
                    .header("x-correlation-id", correlation_id.to_string())
                    .json(command)
            })
            .await
        })
    }

    #[instrument(skip_all, fields(%correlation_id, %instance_id))]
    fn inspect<'a>(
        &'a self,
        instance_id: Uuid,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, ProcessInstance> {
        Box::pin(async move {
            self.execute(|| {
                self.client
                    .get(format!("{}/api/v1/instances/{instance_id}", self.base_url))
                    .header("x-correlation-id", correlation_id.to_string())
            })
            .await
        })
    }

    #[instrument(skip_all, fields(%correlation_id))]
    fn correlate<'a>(
        &'a self,
        command: &'a CorrelateMessage,
        correlation_id: Uuid,
    ) -> GatewayFuture<'a, CorrelatedMessage> {
        Box::pin(async move {
            self.execute(|| {
                self.client
                    .post(format!("{}/api/v1/messages", self.base_url))
                    .header("x-correlation-id", correlation_id.to_string())
                    .json(command)
            })
            .await
        })
    }
}

fn backoff(attempt: usize) -> Duration {
    Duration::from_millis(100_u64.saturating_mul(1_u64 << attempt.min(4)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{self, Write},
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{Path, State},
        http::HeaderMap,
        response::IntoResponse,
        routing::{get, post},
    };

    #[derive(Clone)]
    struct HeartContractState {
        starts: Arc<AtomicUsize>,
        instance_id: Uuid,
        timeout_id: Uuid,
    }

    #[derive(Clone, Default)]
    struct LogCapture(Arc<std::sync::Mutex<Vec<u8>>>);

    struct LogWriter(Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogCapture {
        type Writer = LogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            LogWriter(self.0.clone())
        }
    }

    fn assert_correlation(headers: &HeaderMap) {
        assert!(
            headers
                .get("x-correlation-id")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| Uuid::parse_str(value).ok())
                .is_some(),
            "missing valid correlation id"
        );
    }

    async fn start_contract(
        State(state): State<HeartContractState>,
        headers: HeaderMap,
        Json(command): Json<StartProcess>,
    ) -> impl IntoResponse {
        assert_correlation(&headers);
        assert_eq!(command.namespace, "friends");
        assert_eq!(command.definition_name, "event-planning");
        if state.starts.fetch_add(1, Ordering::SeqCst) == 0 {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error":"try again","private":"DO_NOT_LOG_HEART_BODY"})),
            );
        }
        (
            StatusCode::CREATED,
            Json(serde_json::json!({"instance_id":state.instance_id})),
        )
    }

    async fn inspect_contract(
        State(state): State<HeartContractState>,
        Path(instance_id): Path<Uuid>,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        assert_correlation(&headers);
        if instance_id == state.timeout_id {
            sleep(Duration::from_millis(100)).await;
        }
        Json(serde_json::json!({
            "id":instance_id,
            "definition_id":Uuid::nil(),
            "namespace":"friends",
            "status":"waiting",
            "current_node":"collect-rsvp",
            "metadata":{"channel":"private"}
        }))
    }

    async fn correlate_contract(
        headers: HeaderMap,
        Json(command): Json<CorrelateMessage>,
    ) -> impl IntoResponse {
        assert_correlation(&headers);
        if command.correlation_value == "forbidden" {
            return (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({"error":"not a participant"})),
            );
        }
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "matched_instances":1,
                "instance_ids":[Uuid::nil()]
            })),
        )
    }

    async fn heart_contract_server() -> (HeartGateway, HeartContractState) {
        let state = HeartContractState {
            starts: Arc::new(AtomicUsize::new(0)),
            instance_id: Uuid::now_v7(),
            timeout_id: Uuid::now_v7(),
        };
        let app = Router::new()
            .route("/api/v1/instances", post(start_contract))
            .route("/api/v1/instances/{id}", get(inspect_contract))
            .route("/api/v1/messages", post(correlate_contract))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (
            HeartGateway::new(format!("http://{address}"), Duration::from_millis(30), 1).unwrap(),
            state,
        )
    }

    #[test]
    fn classifies_remote_failures_for_retry() {
        assert!(ProcessError::status(StatusCode::SERVICE_UNAVAILABLE).retryable);
        assert!(ProcessError::status(StatusCode::TOO_MANY_REQUESTS).retryable);
        assert!(!ProcessError::status(StatusCode::FORBIDDEN).retryable);
        assert!(!ProcessError::status(StatusCode::CONFLICT).retryable);
    }

    #[test]
    fn heart_status_errors_do_not_retain_provider_response_bodies() {
        let error = ProcessError::status(StatusCode::BAD_REQUEST);
        assert_eq!(error.message, "Heart rejected request");
        assert!(!error.message.contains("provider-secret"));
    }

    #[test]
    fn validates_heart_endpoint() {
        assert!(HeartGateway::new("https://heart.example", Duration::from_secs(2), 2).is_ok());
        assert!(HeartGateway::new("heart.example", Duration::from_secs(2), 2).is_err());
        assert!(
            HeartGateway::new(
                "https://service:secret@heart.example",
                Duration::from_secs(2),
                2
            )
            .is_err()
        );
        assert!(
            HeartGateway::new(
                "https://heart.example?token=secret",
                Duration::from_secs(2),
                2
            )
            .is_err()
        );
    }

    #[test]
    fn retry_backoff_is_bounded() {
        assert_eq!(backoff(0), Duration::from_millis(100));
        assert_eq!(backoff(100), Duration::from_millis(1600));
    }

    #[tokio::test]
    async fn heart_gateway_contract_covers_success_failure_timeout_and_retry() {
        let (gateway, state) = heart_contract_server().await;
        let correlation_id = Uuid::now_v7();
        let logs = LogCapture::default();
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .without_time()
            .with_writer(logs.clone())
            .finish();
        let _subscriber = tracing::subscriber::set_default(subscriber);
        let started = gateway
            .start(
                &StartProcess {
                    namespace: "friends".to_owned(),
                    definition_name: "event-planning".to_owned(),
                    version: Some("1".to_owned()),
                    metadata: serde_json::json!({"circle":"weekend"}),
                },
                correlation_id,
            )
            .await
            .unwrap();
        assert_eq!(started.instance_id, state.instance_id);
        assert_eq!(
            state.starts.load(Ordering::SeqCst),
            2,
            "start was not retried"
        );
        let rendered_logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(rendered_logs.contains("error_kind=\"unavailable\""));
        assert!(!rendered_logs.contains("DO_NOT_LOG_HEART_BODY"));

        let inspected = gateway
            .inspect(state.instance_id, correlation_id)
            .await
            .unwrap();
        assert_eq!(inspected.id, state.instance_id);
        assert_eq!(inspected.current_node.as_deref(), Some("collect-rsvp"));

        let correlated = gateway
            .correlate(
                &CorrelateMessage {
                    namespace: "friends".to_owned(),
                    correlation_key: "event".to_owned(),
                    correlation_value: "weekend".to_owned(),
                    payload: serde_json::json!({"answer":"yes"}),
                },
                correlation_id,
            )
            .await
            .unwrap();
        assert_eq!(correlated.matched_instances, 1);

        let rejected = gateway
            .correlate(
                &CorrelateMessage {
                    namespace: "friends".to_owned(),
                    correlation_key: "event".to_owned(),
                    correlation_value: "forbidden".to_owned(),
                    payload: serde_json::json!({}),
                },
                correlation_id,
            )
            .await
            .unwrap_err();
        assert_eq!(rejected.kind, ProcessErrorKind::Unauthorized);
        assert!(!rejected.retryable);

        let timed_out = gateway
            .inspect(state.timeout_id, correlation_id)
            .await
            .unwrap_err();
        assert_eq!(timed_out.kind, ProcessErrorKind::Timeout);
        assert!(timed_out.retryable);
    }
}
