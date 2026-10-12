use super::*;
use crate::db::PostgresChatRepository;
use crate::domain::{ChatRepository, SendMessage};

/// Run with both SPROYT_POSTGRES_TEST_URL and
/// SPROYT_SEA_TEMPERATURE_AGENTS_ENABLED=true. Never mutate process environment:
/// other tests can read the gate concurrently.
#[tokio::test]
async fn postgres_marine_reply_expiring_while_waiting_for_sequence_is_not_published() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    assert!(
        sea_temperature_enabled(),
        "run this contract with SPROYT_SEA_TEMPERATURE_AGENTS_ENABLED=true"
    );
    let pool = PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let repository = Arc::new(
        PostgresChatRepository::connect_with_pool(&url, pool.clone())
            .await
            .unwrap(),
    );
    let store = Store::Pg(pool.clone());
    let service = CircleChatAgents {
        store: store.clone(),
        model: None,
        worker_enabled: false,
        weather: None,
        sea_temperature: None,
        ferry: None,
        observations: None,
        imagegen: None,
    };
    let owner = UserId::from_uuid(Uuid::now_v7());
    let circle = Uuid::now_v7().to_string();
    let channel = Uuid::now_v7().to_string();
    use super::weather_followup_tests::{execute, message};
    execute(
        &store,
        "insert into users(id,kind,display_name) values(?uuid,'human','Marine expiry contract')",
        &[&owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into circles(id,slug,name,created_by) values(?uuid,?,'Marine expiry contract',?uuid)",
        &[&circle, &format!("marine-{circle}"), &owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&circle, &owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Marine expiry contract','public',?uuid,?uuid)",
        &[&channel, &format!("marine-{channel}"), &owner.to_string(), &circle],
    )
    .await;
    execute(
        &store,
        "insert into channel_sequences(channel_id,next_sequence) values(?uuid,2)",
        &[&channel],
    )
    .await;
    let agent = service
        .create(
            &owner,
            &circle,
            AgentInput {
                display_name: "Marine expiry contract".into(),
                trigger_words: vec!["sea".into()],
                response_phrases: vec!["Use the authorised sea snapshot".into()],
                enabled: false,
                revision: None,
                weather: None,
                sea_temperature: Some(Some(sea_temperature::SeaTemperatureConfig {
                    default_location: "bjoroy".into(),
                    locations: vec!["bjoroy".into()],
                })),
                ferry_port: None,
                memory_enabled: None,
                vision_enabled: None,
                image_generation: None,
            },
        )
        .await
        .unwrap();
    execute(
        &store,
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    let source = message(&store, &channel, &owner, 1, "Sea temperature?", None).await;
    let job = Uuid::now_v7();
    let request = format!("circle-chat-agent:{job}");
    let command = SendMessage {
        actor: UserId::new(&agent.agent_id).unwrap(),
        channel_id: ChannelId::new(&channel).unwrap(),
        parent_message_id: None,
        body: MessageBody::new("Marine snapshot expiry contract reply").unwrap(),
    };
    // Deadline and lease use database wall time; a fixture delay cannot renew them.
    sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,access_revision,status,available_at,created_at,lease_token,leased_until,reply_body,sea_temperature_snapshot,sea_temperature_valid_until) select $1,a.agent_id,$2,c.id,a.revision,c.chat_agent_access_revision,'leased',extract(epoch from clock_timestamp())::bigint,extract(epoch from clock_timestamp())::bigint,$3,extract(epoch from clock_timestamp())::bigint+120,$4,'{}',extract(epoch from clock_timestamp())::bigint+5 from circle_chat_agents a join channels c on c.id=$5 where a.agent_id=$6")
        .bind(job)
        .bind(*source.id.as_uuid())
        .bind(Uuid::now_v7())
        .bind(command.body.as_str())
        .bind(*command.channel_id.as_uuid())
        .bind(*command.actor.as_uuid())
        .execute(&pool)
        .await
        .unwrap();
    let mut baseline = pool.begin().await.unwrap();
    authorize_reply_postgres(&mut baseline, &command, &request)
        .await
        .expect("fresh marine reply must be authorized before the sequence wait");
    baseline.rollback().await.unwrap();

    let mut blocker = pool.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("select pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("select channel_id from channel_sequences where channel_id=$1 for update")
        .bind(*command.channel_id.as_uuid())
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let publishing = repository.clone();
    let pending_request = request.clone();
    let pending = tokio::spawn(async move {
        publishing
            .append_message_idempotent(command, pending_request)
            .await
    });
    // Observe the actual lock wait: a sleep alone could pass without testing it.
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let waiting: bool = sqlx::query_scalar("select exists(select 1 from pg_stat_activity where $1=any(pg_blocking_pids(pid)) and query like 'update channel_sequences%')")
                .bind(blocker_pid)
                .fetch_one(&pool)
                .await
                .unwrap();
            if waiting {
                break;
            }
            assert!(!pending.is_finished(), "publication ended before reaching sequence lock");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("publication must reach the sequence lock while the snapshot is fresh");
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let expired: bool = sqlx::query_scalar("select sea_temperature_valid_until <= extract(epoch from clock_timestamp()) from circle_chat_agent_jobs where id=$1")
                .bind(job)
                .fetch_one(&pool)
                .await
                .unwrap();
            if expired {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        !pending.is_finished(),
        "sequence lock must still block publication"
    );
    blocker.rollback().await.unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), pending)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(RepositoryError::PermissionDenied)));
    let published: i64 =
        sqlx::query_scalar("select count(*) from messages where channel_id=$1 and sender_id=$2")
            .bind(Uuid::parse_str(&channel).unwrap())
            .bind(Uuid::parse_str(&agent.agent_id).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(published, 0);
    let receipts: i64 = sqlx::query_scalar(
        "select count(*) from command_receipts where principal_id=$1 and request_id=$2",
    )
    .bind(Uuid::parse_str(&agent.agent_id).unwrap())
    .bind(&request)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipts, 0, "failed publication must roll back its receipt");
}
