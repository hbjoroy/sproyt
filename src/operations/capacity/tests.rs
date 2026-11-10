use super::*;
use futures_util::poll;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[tokio::test]
async fn postgres_acquire_records_wait_timeout_cancellation_and_closed_pool() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let metrics = Arc::new(CapacityMetrics::default());
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(100))
        .connect(&url)
        .await
        .unwrap();
    let held = pool.acquire().await.unwrap();
    let mut waiting = Box::pin(acquire_with(&pool, PoolOperation::Send, metrics.clone()));
    assert!(poll!(&mut waiting).is_pending());
    tokio::time::sleep(Duration::from_millis(10)).await;
    drop(held);
    let held = waiting.await.unwrap();
    assert!(matches!(
        acquire_with(&pool, PoolOperation::Send, metrics.clone()).await,
        Err(sqlx::Error::PoolTimedOut)
    ));
    let mut cancelled = Box::pin(acquire_with(&pool, PoolOperation::Send, metrics.clone()));
    assert!(poll!(&mut cancelled).is_pending());
    drop(cancelled);
    drop(held);
    pool.close().await;
    assert!(matches!(
        acquire_with(&pool, PoolOperation::Send, metrics.clone()).await,
        Err(sqlx::Error::PoolClosed)
    ));
    let data = metrics.0.lock().unwrap();
    for outcome in [0, 1, 2, 4] {
        assert_eq!(data.pool[PoolOperation::Send as usize][outcome].count, 1);
    }
    assert!(data.pool[PoolOperation::Send as usize][0].micros >= 10_000);
    assert!(data.pool[PoolOperation::Send as usize][1].micros >= 90_000);
    assert_eq!(data.pool[PoolOperation::Send as usize][3].count, 0);
    drop(data);
    assert!(render_pool(&pool).contains("sproyt_pool_connections_max{pool=\"shared_postgres\"} 1"));
}

#[test]
fn capacity_metrics_have_a_closed_label_set_and_bounded_series() {
    let metrics = CapacityMetrics::default();
    {
        let mut data = metrics.0.lock().unwrap();
        for operation in &mut data.pool {
            for sample in operation {
                sample.observe(2_000);
            }
        }
        for operation in &mut data.chat {
            for phase in [
                ChatPhase::Admission,
                ChatPhase::Queued,
                ChatPhase::Execution,
            ] {
                for (_, outcome) in phase.outcomes() {
                    operation[phase as usize][*outcome].observe(3_000);
                }
            }
        }
    }
    let output = metrics.render();
    // 7 pool operations x 5 outcomes x 11 samples; 13 chat operations x
    // 7 valid phase/outcome pairs x 11 samples; 3 current and 3 peak gauges.
    assert_eq!(output.lines().filter(|l| !l.starts_with('#')).count(), 1392);
    for line in output.lines().filter(|l| !l.starts_with('#')) {
        let labels = line.split('{').nth(1).unwrap().split('}').next().unwrap();
        for label in labels.split(',') {
            assert!(matches!(
                label.split('=').next().unwrap(),
                "operation" | "phase" | "result" | "pool" | "le"
            ));
        }
    }
    assert!(!output.contains("user_id"));
    assert!(!output.contains("channel_id"));
    assert!(!output.contains("request_id"));
    assert!(output.contains("phase=\"execution\",result=\"completed\""));
    assert!(output.contains("pool=\"shared_postgres\",operation=\"send\",result=\"cancelled\""));
}

#[tokio::test]
async fn unpolled_acquire_does_not_record_an_attempt() {
    let metrics = Arc::new(CapacityMetrics::default());
    let pool = PgPoolOptions::new()
        .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
        .unwrap();
    drop(acquire_with(&pool, PoolOperation::Send, metrics.clone()));
    assert!(
        !metrics
            .render()
            .contains("sproyt_pool_selected_acquire_duration_seconds_count{")
    );
}

#[tokio::test]
async fn metrics_endpoint_exposes_only_the_registered_pool_snapshot() {
    let operations = crate::operations::OperationalState::default();
    let empty = crate::operations::metrics(axum::extract::State(operations.clone())).await;
    assert!(!empty.contains("sproyt_pool_connections_max{"));
    let pool = PgPoolOptions::new()
        .max_connections(3)
        .connect_lazy("postgres://private-fixture:private-password@127.0.0.1:1/private-database")
        .unwrap();
    operations.observe_postgres_pool(Some(&pool));
    let text = crate::operations::metrics(axum::extract::State(operations)).await;
    assert!(text.contains("sproyt_pool_connections_max{pool=\"shared_postgres\"} 3"));
    assert!(text.contains("sproyt_pool_connections_size{pool=\"shared_postgres\"} 0"));
    assert!(text.contains("sproyt_pool_connections_idle{pool=\"shared_postgres\"} 0"));
    assert!(!text.contains("private-fixture"));
    assert!(!text.contains("private-password"));
    assert!(!text.contains("private-database"));
}
