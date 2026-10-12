//! Cached, shared-database aggregates. Scraping never reads media or queries SQL.
use super::*;
use std::{fmt::Write, sync::Mutex};

#[derive(Clone, Copy, Debug, Default)]
struct Snapshot {
    jobs: [i64; 4],
    oldest_eligible_at: i64,
    original_bytes: i64,
    reserved_bytes: i64,
    quota_bytes: i64,
}

struct Cache {
    enabled: bool,
    snapshot: Option<(Snapshot, i64)>,
    errors: u64,
}

impl Cache {
    const fn new() -> Self {
        Self {
            enabled: false,
            snapshot: None,
            errors: 0,
        }
    }

    fn record(&mut self, result: Result<Snapshot, UploadError>, now: i64) {
        match result {
            Ok(snapshot) => self.snapshot = Some((snapshot, now)),
            Err(_) => self.errors = self.errors.saturating_add(1),
        }
    }

    fn render(&self, now: i64) -> String {
        let mut out = String::new();
        let (snapshot, observed_at) = self.snapshot.unwrap_or_default();
        for (name, value) in [
            ("sproyt_media_monitor_enabled", i64::from(self.enabled)),
            ("sproyt_media_snapshot_timestamp_seconds", observed_at),
            (
                "sproyt_media_oldest_eligible_seconds",
                if snapshot.oldest_eligible_at > 0 {
                    (now - snapshot.oldest_eligible_at).max(0)
                } else {
                    0
                },
            ),
            (
                "sproyt_media_original_charged_bytes",
                snapshot.original_bytes,
            ),
            ("sproyt_media_reserved_bytes", snapshot.reserved_bytes),
            ("sproyt_media_quota_bytes", snapshot.quota_bytes),
        ] {
            writeln!(out, "# TYPE {name} gauge\n{name} {value}").unwrap();
        }
        writeln!(out, "# TYPE sproyt_media_preview_jobs gauge").unwrap();
        for (state, value) in ["pending", "running", "ready", "failed"]
            .into_iter()
            .zip(snapshot.jobs)
        {
            writeln!(
                out,
                "sproyt_media_preview_jobs{{state=\"{state}\"}} {value}"
            )
            .unwrap();
        }
        writeln!(out, "# TYPE sproyt_media_snapshot_errors_total counter\nsproyt_media_snapshot_errors_total {}", self.errors).unwrap();
        out
    }
}

static CACHE: Mutex<Cache> = Mutex::new(Cache::new());

pub(crate) fn metrics() -> String {
    CACHE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .render(chrono::Utc::now().timestamp())
}

impl MediaStore {
    pub(super) fn start_monitoring(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let store = self.clone();
        CACHE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .enabled = true;
        tokio::spawn(async move {
            // Spread the small aggregate reads across replicas; never overlap polls.
            let jitter = u64::from(Uuid::now_v7().as_bytes()[15] % 30);
            tokio::select! {
                _ = shutdown.changed() => return,
                _ = tokio::time::sleep(Duration::from_secs(jitter)) => {}
            }
            let mut timer = tokio::time::interval(Duration::from_secs(30));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    _ = timer.tick() => {}
                }
                let now = chrono::Utc::now().timestamp();
                let result = tokio::select! {
                    _ = shutdown.changed() => break,
                    result = tokio::time::timeout(Duration::from_secs(3), store.monitor_snapshot(now)) =>
                        result.unwrap_or_else(|_| Err(UploadError::Storage(sqlx::Error::PoolTimedOut))),
                };
                CACHE
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .record(result, now);
            }
        });
    }

    async fn monitor_snapshot(&self, now: i64) -> Result<Snapshot, UploadError> {
        // One statement gives a consistent view without row/blob reads or writer locks.
        with_store!(self, |pool, pg| {
            let row = sqlx::query(&sql(
                "select
                  (select count(*) from media_preview_jobs where state='pending') as pending,
                  (select count(*) from media_preview_jobs where state='running') as running,
                  (select count(*) from media_preview_jobs where state='ready') as ready,
                  (select count(*) from media_preview_jobs where state='failed') as failed,
                  (select coalesce(min(case when state='pending' then available_at else lease_until end),0)
                   from media_preview_jobs where (state='pending' and available_at<=?)
                      or (state='running' and lease_until<=?)) as oldest_eligible_at,
                  (select cast(coalesce(sum(case when size_bytes<1048576 then 1048576 else size_bytes end),0) as bigint) from media_objects) as original_bytes,
                  (select cast(coalesce(sum(case when expected_bytes<1048576 then 1048576 else expected_bytes end),0) as bigint) from media_uploads where state<>'complete') as reserved_bytes,
                  (select coalesce(total_bytes,0) from media_upload_budget where id=1) as quota_bytes", pg))
                .bind(now).bind(now).fetch_one(pool).await.map_err(UploadError::from_storage)?;
            let snapshot = (|| -> Result<Snapshot, sqlx::Error> {
                Ok(Snapshot {
                    jobs: [
                        row.try_get("pending")?,
                        row.try_get("running")?,
                        row.try_get("ready")?,
                        row.try_get("failed")?,
                    ],
                    oldest_eligible_at: row.try_get("oldest_eligible_at")?,
                    original_bytes: row.try_get("original_bytes")?,
                    reserved_bytes: row.try_get("reserved_bytes")?,
                    quota_bytes: row.try_get("quota_bytes")?,
                })
            })();
            snapshot.map_err(UploadError::from_storage)
        })
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    async fn contract(store: &MediaStore) {
        with_store!(store, |pool, pg| {
            for statement in [
                "create table media_preview_jobs (state text, available_at bigint, lease_until bigint)",
                "create table media_objects (size_bytes bigint)",
                "create table media_uploads (state text, expected_bytes bigint)",
                "create table media_upload_budget (id integer primary key, total_bytes bigint)",
                "insert into media_upload_budget values (1,67108864)",
            ] {
                sqlx::query(&sql(statement, pg))
                    .execute(pool)
                    .await
                    .unwrap();
            }
        });
        let empty = store.monitor_snapshot(1000).await.unwrap();
        assert_eq!(empty.jobs, [0; 4]);
        assert_eq!(empty.oldest_eligible_at, 0);
        assert_eq!(empty.original_bytes + empty.reserved_bytes, 0);
        assert_eq!(empty.quota_bytes, 64 * 1048576);
        with_store!(store, |pool, pg| {
            for statement in [
                "insert into media_preview_jobs values ('pending',900,null),('pending',1200,null),('running',800,950),('running',800,1200),('ready',800,null),('failed',800,null)",
                "insert into media_objects values (3),(2097152)",
                "insert into media_uploads values ('open',2),('sealed',3145728),('complete',17825792)",
            ] {
                sqlx::query(&sql(statement, pg))
                    .execute(pool)
                    .await
                    .unwrap();
            }
        });
        let snapshot = store.monitor_snapshot(1000).await.unwrap();
        assert_eq!(snapshot.jobs, [2, 2, 1, 1]);
        assert_eq!(snapshot.oldest_eligible_at, 900);
        assert_eq!(snapshot.original_bytes, 3 * 1048576);
        assert_eq!(snapshot.reserved_bytes, 4 * 1048576);
        with_store!(store, |pool, pg| {
            sqlx::query(&sql(
                "update media_preview_jobs set available_at=1200 where state='pending'",
                pg,
            ))
            .execute(pool)
            .await
            .unwrap();
        });
        assert_eq!(
            store
                .monitor_snapshot(1000)
                .await
                .unwrap()
                .oldest_eligible_at,
            950
        );
        with_store!(store, |pool, pg| {
            for statement in [
                "update media_preview_jobs set lease_until=1200 where state='running'",
                "delete from media_preview_jobs where state='failed'",
            ] {
                sqlx::query(&sql(statement, pg))
                    .execute(pool)
                    .await
                    .unwrap();
            }
        });
        let snapshot = store.monitor_snapshot(1000).await.unwrap();
        assert_eq!(snapshot.oldest_eligible_at, 0);
        assert_eq!(snapshot.jobs[3], 0); // A gauge, not a monotonic failure counter.
    }

    fn store(store: Store) -> MediaStore {
        MediaStore {
            store,
            limits: UploadLimits {
                max_file_bytes: 1048576,
                owner_bytes: 67108864,
                total_bytes: 67108864,
            },
            allowed_channels: None,
        }
    }

    #[tokio::test]
    async fn sqlite_aggregates_eligibility_and_charged_quota() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        contract(&store(Store::Sqlite(pool))).await;
    }

    pub(in crate::media_store) async fn postgres_contract() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let admin = PgPool::connect(&url).await.unwrap();
        // Isolated schema: parallel integration tests must not affect global counts.
        let schema = format!("media_metrics_{}", Uuid::now_v7().simple());
        sqlx::query(&format!("create schema {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let search_path = format!("set search_path to {schema}");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .after_connect(move |connection, _| {
                let query = search_path.clone();
                Box::pin(async move {
                    sqlx::query(&query).execute(connection).await?;
                    Ok(())
                })
            })
            .connect(&url)
            .await
            .unwrap();
        contract(&store(Store::Postgres(pool.clone()))).await;
        pool.close().await;
        sqlx::query(&format!("drop schema {schema} cascade"))
            .execute(&admin)
            .await
            .unwrap();
        admin.close().await;
    }

    #[test]
    fn failed_poll_preserves_snapshot_and_does_not_relabel_old_data_as_fresh() {
        let mut cache = Cache::new();
        cache.enabled = true;
        assert!(
            cache
                .render(1000)
                .contains("sproyt_media_snapshot_timestamp_seconds 0")
        );
        cache.record(
            Ok(Snapshot {
                jobs: [2, 1, 3, 4],
                oldest_eligible_at: 900,
                ..Snapshot::default()
            }),
            1000,
        );
        cache.record(Err(UploadError::Storage(sqlx::Error::PoolTimedOut)), 1100);
        let output = cache.render(1200);
        assert!(output.contains("sproyt_media_snapshot_timestamp_seconds 1000"));
        assert!(output.contains("sproyt_media_snapshot_errors_total 1"));
        assert!(output.contains("sproyt_media_oldest_eligible_seconds 300"));
        assert!(output.contains("sproyt_media_preview_jobs{state=\"failed\"} 4"));
        assert!(!output.contains("owner_id"));
        assert!(!output.contains("channel_id"));
    }
}
