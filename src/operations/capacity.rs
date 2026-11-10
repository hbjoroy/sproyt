//! Bounded, process-local capacity observations. Pool timing covers selected calls only.
use sqlx::{PgPool, Postgres, pool::PoolConnection};
use std::{
    fmt::Write,
    sync::{Arc, Mutex, OnceLock},
    time::Instant,
};

const BOUNDS_MICROS: [u64; 8] = [
    1_000, 5_000, 25_000, 100_000, 500_000, 750_000, 1_000_000, 5_000_000,
];

#[derive(Clone, Copy)]
pub(crate) enum PoolOperation {
    Send,
    SendIdempotent,
    HistoryAccess,
    HistoryRead,
    PushSelect,
    PushClaim,
    PushComplete,
}
impl PoolOperation {
    const LABELS: [&str; 7] = [
        "send",
        "send_idempotent",
        "history_access",
        "history_read",
        "push_select",
        "push_claim",
        "push_complete",
    ];
}

#[derive(Clone, Copy)]
pub(crate) enum ChatOperation {
    ChannelCreated,
    Subscribe,
    Leave,
    Send,
    Reaction,
    Edit,
    Delete,
    ExternalMessage,
    ExternalReaction,
    ExternalUpdate,
    ExternalChannel,
    ExternalPresence,
    ExpirePresence,
}
impl ChatOperation {
    const LABELS: [&str; 13] = [
        "channel_created",
        "subscribe",
        "leave",
        "send",
        "reaction",
        "edit",
        "delete",
        "external_message",
        "external_reaction",
        "external_update",
        "external_channel",
        "external_presence",
        "expire_presence",
    ];
}

#[derive(Clone, Copy)]
pub(crate) enum ChatPhase {
    Admission,
    Queued,
    Execution,
}
impl ChatPhase {
    const LABELS: [&str; 3] = ["admission", "queued", "execution"];
    fn outcomes(self) -> &'static [(&'static str, usize)] {
        match self {
            Self::Admission => &[("admitted", 0), ("closed", 1), ("cancelled", 2)],
            Self::Queued => &[("dequeued", 0), ("discarded", 1)],
            Self::Execution => &[("completed", 0), ("cancelled", 1)],
        }
    }
}

#[derive(Clone, Default)]
struct Sample {
    count: u64,
    micros: u64,
    buckets: [u64; 9],
}
impl Sample {
    fn observe(&mut self, micros: u64) {
        self.count = self.count.saturating_add(1);
        self.micros = self.micros.saturating_add(micros);
        let bucket = BOUNDS_MICROS
            .iter()
            .position(|bound| micros <= *bound)
            .unwrap_or(8);
        self.buckets[bucket] = self.buckets[bucket].saturating_add(1);
    }
    fn render(&self, output: &mut String, name: &str, labels: &str) {
        // Unobserved combinations are absent, with a fixed upper cardinality.
        if self.count == 0 {
            return;
        }
        let mut cumulative = 0u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(*bucket);
            let bound = BOUNDS_MICROS
                .get(index)
                .map(|v| (*v as f64 / 1_000_000.0).to_string())
                .unwrap_or_else(|| "+Inf".into());
            writeln!(
                output,
                "{name}_bucket{{{labels},le=\"{bound}\"}} {cumulative}"
            )
            .unwrap();
        }
        writeln!(output, "{name}_count{{{labels}}} {}", self.count).unwrap();
        writeln!(
            output,
            "{name}_sum{{{labels}}} {}",
            self.micros as f64 / 1_000_000.0
        )
        .unwrap();
    }
}

#[derive(Clone, Default)]
struct Measurements {
    pool: [[Sample; 5]; 7],
    chat: [[[Sample; 3]; 3]; 13],
    in_flight: [u64; 3],
    peak: [u64; 3],
}
#[derive(Default)]
pub(crate) struct CapacityMetrics(Mutex<Measurements>);
pub(crate) fn shared() -> Arc<CapacityMetrics> {
    static METRICS: OnceLock<Arc<CapacityMetrics>> = OnceLock::new();
    METRICS.get_or_init(Default::default).clone()
}

impl CapacityMetrics {
    pub(crate) fn chat(self: &Arc<Self>, operation: ChatOperation, phase: ChatPhase) -> ChatGuard {
        let started = Instant::now();
        let index = phase as usize;
        let mut data = self.0.lock().unwrap_or_else(|e| e.into_inner());
        data.in_flight[index] += 1;
        data.peak[index] = data.peak[index].max(data.in_flight[index]);
        ChatGuard {
            metrics: self.clone(),
            operation,
            phase,
            started,
            finished: false,
        }
    }
    pub(crate) fn render(&self) -> String {
        // Formatting a scrape must not hold the recording lock.
        let data = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let mut output = String::new();
        writeln!(output, "# HELP sproyt_pool_selected_acquire_duration_seconds Selected PostgreSQL acquisitions only; excludes query and BEGIN time; cancellation recorded.").unwrap();
        writeln!(
            output,
            "# TYPE sproyt_pool_selected_acquire_duration_seconds histogram"
        )
        .unwrap();
        for (operation, label) in PoolOperation::LABELS.iter().enumerate() {
            for (outcome, result) in ["acquired", "timeout", "closed", "error", "cancelled"]
                .iter()
                .enumerate()
            {
                data.pool[operation][outcome].render(
                    &mut output,
                    "sproyt_pool_selected_acquire_duration_seconds",
                    &format!("pool=\"shared_postgres\",operation=\"{label}\",result=\"{result}\""),
                );
            }
        }
        writeln!(output, "# HELP sproyt_chat_phase_duration_seconds Mailbox phases; completed means handled, including domain rejection, not successful persistence.").unwrap();
        writeln!(
            output,
            "# TYPE sproyt_chat_phase_duration_seconds histogram"
        )
        .unwrap();
        for (operation, label) in ChatOperation::LABELS.iter().enumerate() {
            for phase in [
                ChatPhase::Admission,
                ChatPhase::Queued,
                ChatPhase::Execution,
            ] {
                for (result, outcome) in phase.outcomes() {
                    data.chat[operation][phase as usize][*outcome].render(
                        &mut output,
                        "sproyt_chat_phase_duration_seconds",
                        &format!(
                            "operation=\"{label}\",phase=\"{}\",result=\"{result}\"",
                            ChatPhase::LABELS[phase as usize]
                        ),
                    );
                }
            }
        }
        writeln!(output, "# HELP sproyt_chat_phase_in_flight Current phases across local engines; admission includes attempts that obtain space immediately.").unwrap();
        writeln!(output, "# TYPE sproyt_chat_phase_in_flight gauge").unwrap();
        writeln!(
            output,
            "# HELP sproyt_chat_phase_peak Process lifetime high water marks across local engines."
        )
        .unwrap();
        writeln!(output, "# TYPE sproyt_chat_phase_peak gauge").unwrap();
        for (index, phase) in ChatPhase::LABELS.iter().enumerate() {
            writeln!(
                output,
                "sproyt_chat_phase_in_flight{{phase=\"{phase}\"}} {}",
                data.in_flight[index]
            )
            .unwrap();
            writeln!(
                output,
                "sproyt_chat_phase_peak{{phase=\"{phase}\"}} {}",
                data.peak[index]
            )
            .unwrap();
        }
        output
    }
}

pub(crate) struct ChatGuard {
    metrics: Arc<CapacityMetrics>,
    operation: ChatOperation,
    phase: ChatPhase,
    started: Instant,
    finished: bool,
}
impl ChatGuard {
    pub(crate) fn finish(mut self) {
        self.record(0);
    }
    pub(crate) fn closed(mut self) {
        self.record(1);
    }
    fn record(&mut self, outcome: usize) {
        if self.finished {
            return;
        }
        let micros = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let mut data = self.metrics.0.lock().unwrap_or_else(|e| e.into_inner());
        data.in_flight[self.phase as usize] -= 1;
        data.chat[self.operation as usize][self.phase as usize][outcome].observe(micros);
        self.finished = true;
    }
}
impl Drop for ChatGuard {
    fn drop(&mut self) {
        self.record(match self.phase {
            ChatPhase::Admission => 2,
            ChatPhase::Queued | ChatPhase::Execution => 1,
        });
    }
}

struct AcquireGuard {
    metrics: Arc<CapacityMetrics>,
    operation: PoolOperation,
    started: Instant,
    outcome: usize,
}
impl Drop for AcquireGuard {
    fn drop(&mut self) {
        let micros = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        self.metrics
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pool[self.operation as usize][self.outcome]
            .observe(micros);
    }
}
pub(crate) async fn acquire(
    pool: &PgPool,
    operation: PoolOperation,
) -> Result<PoolConnection<Postgres>, sqlx::Error> {
    acquire_with(pool, operation, shared()).await
}
async fn acquire_with(
    pool: &PgPool,
    operation: PoolOperation,
    metrics: Arc<CapacityMetrics>,
) -> Result<PoolConnection<Postgres>, sqlx::Error> {
    // Constructed only when this future is polled; no timing of an unpolled attempt.
    let mut guard = AcquireGuard {
        metrics,
        operation,
        started: Instant::now(),
        outcome: 4,
    };
    let result = pool.acquire().await;
    guard.outcome = match &result {
        Ok(_) => 0,
        Err(sqlx::Error::PoolTimedOut) => 1,
        Err(sqlx::Error::PoolClosed) => 2,
        Err(_) => 3,
    };
    result
}

pub(crate) fn render_pool(pool: &PgPool) -> String {
    let mut output = String::new();
    for (name, value) in [
        ("size", pool.size() as u64),
        ("idle", pool.num_idle() as u64),
        ("max", pool.options().get_max_connections() as u64),
    ] {
        writeln!(output, "# TYPE sproyt_pool_connections_{name} gauge").unwrap();
        writeln!(
            output,
            "sproyt_pool_connections_{name}{{pool=\"shared_postgres\"}} {value}"
        )
        .unwrap();
    }
    output
}

#[cfg(test)]
mod tests;
