use super::{PgPool, PgPoolOptions, run_migrations};
use sqlx::{migrate::Migration, postgres::PgConnectOptions};
use uuid::Uuid;

const CANONICAL_81: &str = "9a9874153860cb492da8fd7fd7cd51f4be18fe26ad8d1e91e4ec5aed761b7c451fdd7b5940e8a7a136cca6b08e0b5d8d";
const DEPLOYED_81: &str = "5526afe8feaf070f6c90bbb57ffe7d95f7fc121a68ddb54308f2731d243b1bcce8d39a00dbf2c96a75f716eb141898eb";

async fn old_ledger(pool: &PgPool) -> serde_json::Value {
    sqlx::query_scalar(
        "select jsonb_agg(to_jsonb(m) order by version) from _sqlx_migrations m where version<=81",
    )
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test]
#[ignore = "requires an isolated loopback PostgreSQL in SPROYT_POSTGRES_TEST_URL"]
async fn postgres_historical_0081_checksum_compatibility() {
    let url = std::env::var("SPROYT_POSTGRES_TEST_URL").unwrap();
    let target = reqwest::Url::parse(&url).unwrap();
    assert!(matches!(
        target.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]")
    ));
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    for case in ["canonical", "deployed", "unknown", "other_migration"] {
        // Each case is installed by SQLx from genuine SQL in a fresh private schema.
        let schema = format!("intake_migration_{}", Uuid::now_v7().simple());
        sqlx::query(&format!("create schema {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let opts = url
            .parse::<PgConnectOptions>()
            .unwrap()
            .options([("search_path", schema.as_str())]);
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await
            .unwrap();
        let mut previous = sqlx::migrate!("./migrations/postgres");
        previous.migrations.to_mut().retain(|m| m.version <= 81);
        for migration in previous.migrations.to_mut() {
            let sql = match (case, migration.version) {
                ("deployed" | "other_migration", 81) => {
                    migration.sql.replace("\r\n", "\n").replace('\n', "\r\n")
                }
                ("unknown", 81) | ("other_migration", 80) => {
                    format!("-- Unexpected historical content\n{}", migration.sql)
                }
                _ => continue,
            };
            *migration = Migration::new(
                migration.version,
                migration.description.clone(),
                migration.migration_type,
                sql.into(),
                migration.no_tx,
            );
        }
        previous.run(&pool).await.unwrap();
        let checksum: String = sqlx::query_scalar(
            "select encode(checksum,'hex') from _sqlx_migrations where version=81",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        match case {
            "canonical" => assert_eq!(checksum, CANONICAL_81),
            "unknown" => assert!(checksum != CANONICAL_81 && checksum != DEPLOYED_81),
            _ => assert_eq!(checksum, DEPLOYED_81),
        }
        let before = old_ledger(&pool).await;
        let allowed = matches!(case, "canonical" | "deployed");
        for _ in 0..2 {
            let result = run_migrations(&pool).await;
            assert_eq!(result.is_ok(), allowed, "{case}: {result:?}");
            assert_eq!(
                old_ledger(&pool).await,
                before,
                "historical ledger changed for {case}"
            );
            let state: (bool, bool) = sqlx::query_as(
                "select exists(select 1 from _sqlx_migrations where version=82 and success),
                to_regclass('feedback_intakes') is not null",
            )
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(state, (allowed, allowed), "{case}");
        }
        pool.close().await;
        sqlx::query(&format!("drop schema {schema} cascade"))
            .execute(&admin)
            .await
            .unwrap();
    }
    admin.close().await;
}
