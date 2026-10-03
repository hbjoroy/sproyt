use super::*;
use crate::domain::{ChannelSequence, ChatMessage, DisplayName, MessageId, SendMessage};

async fn execute(store: &Store, query: &str, args: &[&str]) {
    store
        .execute(
            query,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        )
        .await
        .unwrap();
}

async fn values(store: &Store, query: &str, args: &[&str]) -> Vec<String> {
    store
        .values(
            query,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        )
        .await
        .unwrap()
}

async fn message(
    store: &Store,
    channel: &str,
    actor: &UserId,
    sequence: u64,
    body: &str,
    parent: Option<MessageId>,
) -> ChatMessage {
    let id = MessageId::generate();
    let created = if matches!(store, Store::Pg(_)) {
        "now()"
    } else {
        "strftime('%Y-%m-%dT%H:%M:%fZ','now')"
    };
    let parent_text = parent
        .map(|id| id.as_uuid().to_string())
        .unwrap_or_default();
    let parent_sql = if matches!(store, Store::Pg(_)) {
        "cast(nullif(?,'') as uuid)"
    } else {
        "nullif(?,'')"
    };
    let query = format!(
        "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,parent_message_id,created_at) values(?uuid,?uuid,?uuid,'Weather contract',?int,?,{parent_sql},{created})"
    );
    execute(
        store,
        &query,
        &[
            &id.as_uuid().to_string(),
            channel,
            &actor.to_string(),
            &sequence.to_string(),
            body,
            &parent_text,
        ],
    )
    .await;
    ChatMessage {
        id,
        channel_id: ChannelId::new(channel).unwrap(),
        parent_message_id: parent,
        sender_id: actor.clone(),
        sender_display_name: DisplayName::new("Weather contract").unwrap(),
        body: MessageBody::new(body).unwrap(),
        sequence: ChannelSequence::new(sequence),
        sent_at: Utc::now(),
        edited_at: None,
        deleted_at: None,
    }
}

async fn enqueue(store: &Store, message: &ChatMessage) {
    match store {
        Store::Sqlite(pool) => {
            let mut tx = pool.begin().await.unwrap();
            enqueue_sqlite(&mut tx, message).await.unwrap();
            tx.commit().await.unwrap();
        }
        Store::Pg(pool) => {
            let mut tx = pool.begin().await.unwrap();
            enqueue_postgres(&mut tx, message).await.unwrap();
            tx.commit().await.unwrap();
        }
    }
}

async fn publication(store: &Store, job: &Job) -> Result<()> {
    let command = SendMessage {
        actor: UserId::new(&job.agent_id).unwrap(),
        channel_id: ChannelId::new(&job.channel_id).unwrap(),
        parent_message_id: None,
        body: MessageBody::new("Eit konkret vêrsvar").unwrap(),
    };
    let request = format!("circle-chat-agent:{}", job.id);
    match store {
        Store::Sqlite(pool) => {
            let mut tx = pool.begin().await.unwrap();
            let result = authorize_reply_sqlite(&mut tx, &command, &request).await;
            tx.rollback().await.unwrap();
            result
        }
        Store::Pg(pool) => {
            let mut tx = pool.begin().await.unwrap();
            let result = authorize_reply_postgres(&mut tx, &command, &request).await;
            tx.rollback().await.unwrap();
            result
        }
    }
}

async fn denied(service: &CircleChatAgents, job: &Job, reason: &str) {
    assert!(
        service.source(job).await.unwrap().is_none(),
        "source: {reason}"
    );
    assert!(
        matches!(
            publication(&service.store, job).await,
            Err(RepositoryError::PermissionDenied)
        ),
        "publication: {reason}"
    );
}

fn weather() -> WeatherConfig {
    WeatherConfig {
        location: "Paros, Greece".into(),
        latitude: 37.085,
        longitude: 25.148,
    }
}

fn input(weather: Option<WeatherConfig>, revision: Option<i64>) -> AgentInput {
    AgentInput {
        weather: Some(weather),
        display_name: "Vêrvennen".into(),
        trigger_words: vec!["vêret".into()],
        response_phrases: vec!["Bruk det oppgitte vêret".into()],
        enabled: false,
        revision,
    }
}

async fn contract(store: Store) {
    // Configuration can be retained and edited while the external service is unavailable.
    let service = CircleChatAgents {
        weather: None,
        store: store.clone(),
        model: None,
        worker_enabled: false,
    };
    assert!(!service.weather_available());
    let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let other = UserId::new(Uuid::now_v7().to_string()).unwrap();
    for actor in [&owner, &other] {
        execute(
            &store,
            "insert into users(id,kind,display_name) values(?uuid,'human','Weather contract')",
            &[&actor.to_string()],
        )
        .await;
    }
    let circle = Uuid::now_v7().to_string();
    execute(
        &store,
        "insert into circles(id,slug,name,created_by) values(?uuid,?,'Weather contract',?uuid)",
        &[&circle, &format!("weather-{circle}"), &owner.to_string()],
    )
    .await;
    for (actor, role) in [(&owner, "owner"), (&other, "member")] {
        execute(
            &store,
            "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,?)",
            &[&circle, &actor.to_string(), role],
        )
        .await;
    }
    let channel = Uuid::now_v7().to_string();
    execute(&store, "insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Weather contract','public',?uuid,?uuid)", &[&channel, &format!("weather-{channel}"), &owner.to_string(), &circle]).await;
    for actor in [&owner, &other] {
        execute(
            &store,
            "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",
            &[&channel, &actor.to_string()],
        )
        .await;
    }

    let plain = service
        .create(&owner, &circle, input(None, None))
        .await
        .unwrap();
    assert!(plain.weather.is_none());
    assert!(!plain.worker_available);
    assert!(
        service.list(&owner, &circle).await.unwrap()[0]
            .weather
            .is_none()
    );
    let configured = service
        .update(
            &owner,
            &circle,
            &plain.agent_id,
            input(Some(weather()), Some(plain.revision)),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(configured.weather.as_ref().unwrap()).unwrap(),
        serde_json::to_value(weather()).unwrap()
    );
    let listed = service.list(&owner, &circle).await.unwrap();
    assert_eq!(
        serde_json::to_value(listed[0].weather.as_ref().unwrap()).unwrap(),
        serde_json::to_value(weather()).unwrap()
    );
    assert!(!listed[0].worker_available);
    let cleared = service
        .update(
            &owner,
            &circle,
            &plain.agent_id,
            input(None, Some(configured.revision)),
        )
        .await
        .unwrap();
    assert!(cleared.weather.is_none());
    assert!(
        service.list(&owner, &circle).await.unwrap()[0]
            .weather
            .is_none()
    );
    let agent = service
        .update(
            &owner,
            &circle,
            &plain.agent_id,
            input(Some(weather()), Some(cleared.revision)),
        )
        .await
        .unwrap();
    // Exercise editing an existing weather configuration with weatherAvailable=false too.
    let agent = service
        .update(
            &owner,
            &circle,
            &agent.agent_id,
            input(Some(weather()), Some(agent.revision)),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(agent.weather.as_ref().unwrap()).unwrap(),
        serde_json::to_value(weather()).unwrap()
    );
    assert!(!agent.enabled && !agent.worker_available);
    let mut old_client = input(None, Some(agent.revision));
    old_client.weather = None; // Omitted by a pre-weather client: preserve, do not clear.
    let agent = service
        .update(&owner, &circle, &agent.agent_id, old_client)
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(agent.weather.as_ref().unwrap()).unwrap(),
        serde_json::to_value(weather()).unwrap()
    );
    execute(
        &store,
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;

    let trigger = message(&store, &channel, &owner, 1, "Korleis blir vêret?", None).await;
    enqueue(&store, &trigger).await;
    let first = service.claim().await.unwrap().unwrap();
    assert_eq!(first.source_message_id, trigger.id.as_uuid().to_string());
    assert!(
        service
            .source(&first)
            .await
            .unwrap()
            .unwrap()
            .weather
            .is_some()
    );
    assert_eq!(values(&store, "select coalesce(cast(followup_anchor_message_id as text),'') from circle_chat_agent_jobs where id=?uuid", &[&first.id]).await, vec![String::new()]);

    // The reply transaction committed, but the worker died before settling the job.
    let bot = UserId::new(&agent.agent_id).unwrap();
    let anchor = message(&store, &channel, &bot, 2, "Eit konkret vêrsvar", None).await;
    execute(
        &store,
        "insert into command_receipts(principal_id,request_id,message_id) values(?uuid,?,?uuid)",
        &[
            &agent.agent_id,
            &format!("circle-chat-agent:{}", first.id),
            &anchor.id.as_uuid().to_string(),
        ],
    )
    .await;
    assert_eq!(
        values(
            &store,
            "select status from circle_chat_agent_jobs where id=?uuid and reply_message_id is null",
            &[&first.id]
        )
        .await,
        vec!["leased".to_owned()]
    );

    let followup = message(&store, &channel, &owner, 3, "Og i morgon?", None).await;
    enqueue(&store, &followup).await;
    let job = service.claim().await.unwrap().unwrap();
    assert_eq!(job.source_message_id, followup.id.as_uuid().to_string());
    assert_eq!(values(&store, "select cast(followup_anchor_message_id as text) from circle_chat_agent_jobs where id=?uuid", &[&job.id]).await, vec![anchor.id.as_uuid().to_string()]);
    assert!(
        service.source(&job).await.unwrap().is_some(),
        "unfetched weather is a valid job source"
    );
    for (sequence, actor, body, parent) in [
        (4, &other, "Og i morgon?", None),
        (5, &owner, "Og i morgon?", Some(trigger.id)),
        (6, &owner, "Skål", None),
    ] {
        let unrelated = message(&store, &channel, actor, sequence, body, parent).await;
        enqueue(&store, &unrelated).await;
        assert!(
            values(
                &store,
                "select cast(id as text) from circle_chat_agent_jobs where source_message_id=?uuid",
                &[&unrelated.id.as_uuid().to_string()]
            )
            .await
            .is_empty(),
            "must not enqueue: {body}, sequence {sequence}"
        );
    }

    let valid_until = (Utc::now().timestamp() + 300).to_string();
    let snapshot =
        json!({"location": "Paros, Greece", "fetched_at": Utc::now().timestamp()}).to_string();
    execute(&store, "update circle_chat_agent_jobs set reply_body=?,weather_snapshot=?,weather_valid_until=?int where id=?uuid", &["Eit konkret vêrsvar", &snapshot, &valid_until, &job.id]).await;
    assert_eq!(
        values(
            &store,
            "select weather_snapshot from circle_chat_agent_jobs where id=?uuid",
            &[&job.id]
        )
        .await,
        vec![snapshot]
    );
    assert!(service.source(&job).await.unwrap().is_some());
    publication(&store, &job).await.unwrap();

    execute(
        &store,
        "update messages set deleted_at=current_timestamp where id=?uuid",
        &[&anchor.id.as_uuid().to_string()],
    )
    .await;
    denied(&service, &job, "deleted committed anchor").await;
    execute(
        &store,
        "update messages set deleted_at=null where id=?uuid",
        &[&anchor.id.as_uuid().to_string()],
    )
    .await;
    publication(&store, &job).await.unwrap();

    execute(
        &store,
        "update circle_chat_agents set revision=revision+1 where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    denied(&service, &job, "changed configuration revision").await;
    execute(
        &store,
        "update circle_chat_agents set revision=revision-1 where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    publication(&store, &job).await.unwrap();

    execute(&store, "update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid", &[&channel]).await;
    denied(&service, &job, "changed channel access revision").await;
    execute(&store, "update channels set chat_agent_access_revision=chat_agent_access_revision-1 where id=?uuid", &[&channel]).await;
    publication(&store, &job).await.unwrap();

    execute(
        &store,
        "update circle_chat_agent_jobs set weather_valid_until=?int where id=?uuid",
        &[&(Utc::now().timestamp() - 1).to_string(), &job.id],
    )
    .await;
    denied(&service, &job, "expired stored weather snapshot").await;
    // Leave no claimable jobs behind for other PostgreSQL contracts.
    execute(
        &store,
        "update circle_chat_agent_jobs set status='failed' where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
}

#[tokio::test]
async fn sqlite_weather_followup_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    contract(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_weather_followup_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    contract(Store::Pg(pool)).await;
}
