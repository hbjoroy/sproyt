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

async fn conversation_enqueue(store: &Store, message: &ChatMessage, enabled: bool) {
    match store {
        Store::Sqlite(pool) => {
            let mut tx = pool.begin().await.unwrap();
            enqueue_sqlite_with_followups(&mut tx, message, enabled)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        Store::Pg(pool) => {
            let mut tx = pool.begin().await.unwrap();
            enqueue_postgres_with_followups(&mut tx, message, enabled)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
    }
}

async fn conversation_publication(
    store: &Store,
    job: &Job,
    parent: Option<MessageId>,
) -> Result<()> {
    let command = SendMessage {
        actor: UserId::new(&job.agent_id).unwrap(),
        channel_id: ChannelId::new(&job.channel_id).unwrap(),
        parent_message_id: parent,
        body: MessageBody::new("Eit konkret svar").unwrap(),
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

async fn conversation_channel(
    store: &Store,
    circle: &str,
    owner: &UserId,
    other: &UserId,
) -> String {
    let channel = Uuid::now_v7().to_string();
    execute(store, "insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Conversation contract','public',?uuid,?uuid)", &[&channel, &format!("conversation-{channel}"), &owner.to_string(), circle]).await;
    for actor in [owner, other] {
        execute(
            store,
            "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",
            &[&channel, &actor.to_string()],
        )
        .await;
    }
    channel
}

async fn conversation_anchor(
    service: &CircleChatAgents,
    channel: &str,
    owner: &UserId,
    agent: &str,
) -> (ChatMessage, ChatMessage) {
    let original = message(
        &service.store,
        channel,
        owner,
        1,
        "Hjelp meg å forstå dette",
        None,
    )
    .await;
    // The initial trigger works with followups disabled; only continuation is gated.
    conversation_enqueue(&service.store, &original, false).await;
    let initial = service.claim().await.unwrap().unwrap();
    assert_eq!(initial.source_message_id, original.id.as_uuid().to_string());
    let anchor = message(
        &service.store,
        channel,
        &UserId::new(agent).unwrap(),
        2,
        "Eit konkret svar",
        None,
    )
    .await;
    execute(
        &service.store,
        "insert into command_receipts(principal_id,request_id,message_id) values(?uuid,?,?uuid)",
        &[
            agent,
            &format!("circle-chat-agent:{}", initial.id),
            &anchor.id.as_uuid().to_string(),
        ],
    )
    .await;
    execute(&service.store, "update circle_chat_agent_jobs set status='completed',reply_message_id=?uuid where id=?uuid", &[&anchor.id.as_uuid().to_string(), &initial.id]).await;
    (original, anchor)
}

async fn no_conversation_job(store: &Store, source: &ChatMessage) {
    assert!(
        values(
            store,
            "select cast(id as text) from circle_chat_agent_jobs where source_message_id=?uuid",
            &[&source.id.as_uuid().to_string()]
        )
        .await
        .is_empty()
    );
}

async fn conversation_job(
    service: &CircleChatAgents,
    source: &ChatMessage,
    anchor: &ChatMessage,
    mode: &str,
) -> (Job, JobSource) {
    let job = service.claim().await.unwrap().unwrap();
    assert_eq!(job.source_message_id, source.id.as_uuid().to_string());
    assert_eq!(
        values(
            &service.store,
            "select followup_mode from circle_chat_agent_jobs where id=?uuid",
            &[&job.id]
        )
        .await,
        vec![mode.to_owned()]
    );
    assert_eq!(values(&service.store, "select cast(followup_anchor_message_id as text) from circle_chat_agent_jobs where id=?uuid", &[&job.id]).await, vec![anchor.id.as_uuid().to_string()]);
    let source_data = service.source(&job).await.unwrap().unwrap();
    let followup = source_data
        .followup
        .as_ref()
        .expect("validated model anchor");
    assert_eq!(followup.mode, mode);
    assert_eq!(followup.anchor.id, anchor.id.as_uuid().to_string());
    assert_eq!(followup.anchor.body, anchor.body.as_str());
    let context = service.context(&job, &source_data).await.unwrap();
    assert!(
        context
            .iter()
            .any(|item| item.id == anchor.id.as_uuid().to_string()),
        "the committed root must not disappear from explicit thread context"
    );
    assert_eq!(context.last().unwrap().id, source.id.as_uuid().to_string());
    execute(
        &service.store,
        "update circle_chat_agent_jobs set reply_body='Eit konkret svar' where id=?uuid",
        &[&job.id],
    )
    .await;
    conversation_publication(&service.store, &job, source.parent_message_id)
        .await
        .unwrap();
    (job, source_data)
}

async fn conversation_denied(
    service: &CircleChatAgents,
    job: &Job,
    source: &JobSource,
    parent: Option<MessageId>,
    reason: &str,
) {
    assert!(
        service.source(job).await.unwrap().is_none(),
        "source: {reason}"
    );
    assert!(
        matches!(
            service.context(job, source).await,
            Err(RepositoryError::Conflict)
        ),
        "context: {reason}"
    );
    assert!(
        matches!(
            conversation_publication(&service.store, job, parent).await,
            Err(RepositoryError::PermissionDenied)
        ),
        "publication: {reason}"
    );
}

async fn age_conversation(
    store: &Store,
    original: &ChatMessage,
    anchor: &ChatMessage,
    minutes: u32,
) {
    let created = if matches!(store, Store::Pg(_)) {
        format!("now()-interval '{minutes} minutes'")
    } else {
        format!("strftime('%Y-%m-%dT%H:%M:%fZ','now','-{minutes} minutes')")
    };
    execute(
        store,
        &format!("update messages set created_at={created} where id=?uuid or id=?uuid"),
        &[
            &original.id.as_uuid().to_string(),
            &anchor.id.as_uuid().to_string(),
        ],
    )
    .await;
}

async fn conversation_contract(store: Store) {
    let service = CircleChatAgents {
        weather: None,
        store: store.clone(),
        model: None,
        worker_enabled: false,
    };
    let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let other = UserId::new(Uuid::now_v7().to_string()).unwrap();
    for actor in [&owner, &other] {
        execute(
            &store,
            "insert into users(id,kind,display_name) values(?uuid,'human','Conversation contract')",
            &[&actor.to_string()],
        )
        .await;
    }
    let circle = Uuid::now_v7().to_string();
    execute(&store, "insert into circles(id,slug,name,created_by) values(?uuid,?,'Conversation contract',?uuid)", &[&circle, &format!("conversation-{circle}"), &owner.to_string()]).await;
    for (actor, role) in [(&owner, "owner"), (&other, "member")] {
        execute(
            &store,
            "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,?)",
            &[&circle, &actor.to_string(), role],
        )
        .await;
    }
    let agent = service
        .create(
            &owner,
            &circle,
            AgentInput {
                weather: Some(None),
                display_name: "Samtalevennen".into(),
                trigger_words: vec!["hjelp".into()],
                response_phrases: vec!["Svar på konkrete spørsmål".into()],
                enabled: false,
                revision: None,
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

    // Direct @name starts a job without either a trigger or a previous reply.
    let mention_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let direct = message(
        &store,
        &mention_channel,
        &other,
        1,
        "Hei @SAMTALEVENNEN! Korleis går det?",
        None,
    )
    .await;
    conversation_enqueue(&store, &direct, false).await;
    no_conversation_job(&store, &direct).await;
    conversation_enqueue(&store, &direct, true).await;
    conversation_enqueue(&store, &direct, true).await;
    let direct_job = service.claim().await.unwrap().unwrap();
    assert_eq!(
        direct_job.source_message_id,
        direct.id.as_uuid().to_string()
    );
    let direct_source = service.source(&direct_job).await.unwrap().unwrap();
    assert!(direct_source.followup.is_none());
    assert_eq!(values(&store, "select cast(count(*) as text) from circle_chat_agent_jobs where source_message_id=?uuid", &[&direct.id.as_uuid().to_string()]).await, vec!["1"]);
    execute(
        &store,
        "update circle_chat_agent_jobs set status='completed' where id=?uuid",
        &[&direct_job.id],
    )
    .await;
    for (seq, body) in [
        (2, "@Samtalevennen_extra"),
        (3, "mail@Samtalevennen"),
        (4, "@Samtalevennen.example"),
    ] {
        let not_addressed = message(&store, &mention_channel, &owner, seq, body, None).await;
        conversation_enqueue(&store, &not_addressed, true).await;
        no_conversation_job(&store, &not_addressed).await;
    }
    execute(&store, "insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,false,?uuid,0)", &[&mention_channel, &agent.agent_id, &owner.to_string()]).await;
    let disabled_mention =
        message(&store, &mention_channel, &owner, 5, "@Samtalevennen!", None).await;
    conversation_enqueue(&store, &disabled_mention, true).await;
    no_conversation_job(&store, &disabled_mention).await;
    execute(
        &store,
        "delete from channel_chat_agent_settings where channel_id=?uuid",
        &[&mention_channel],
    )
    .await;
    execute(
        &store,
        "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    let revoked_mention =
        message(&store, &mention_channel, &owner, 6, "@Samtalevennen!", None).await;
    conversation_enqueue(&store, &revoked_mention, true).await;
    no_conversation_job(&store, &revoked_mention).await;
    execute(
        &store,
        "update agent_profiles set revoked_at=null where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    let agent_sender = UserId::new(&agent.agent_id).unwrap();
    let agent_mention = message(
        &store,
        &mention_channel,
        &agent_sender,
        7,
        "@Samtalevennen!",
        None,
    )
    .await;
    conversation_enqueue(&store, &agent_mention, true).await;
    no_conversation_job(&store, &agent_mention).await;

    let second = service
        .create(
            &owner,
            &circle,
            AgentInput {
                weather: Some(None),
                display_name: "Annan".into(),
                trigger_words: vec!["eigen-trigger".into()],
                response_phrases: vec!["Svar naturleg".into()],
                enabled: false,
                revision: None,
            },
        )
        .await
        .unwrap();
    execute(
        &store,
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        &[&second.agent_id],
    )
    .await;
    let routing_channel = conversation_channel(&store, &circle, &owner, &other).await;
    conversation_anchor(&service, &routing_channel, &owner, &agent.agent_id).await;
    let to_second = message(
        &store,
        &routing_channel,
        &owner,
        3,
        "@Annan! Kva meiner du?",
        None,
    )
    .await;
    conversation_enqueue(&store, &to_second, true).await;
    assert_eq!(values(&store, "select cast(agent_id as text) from circle_chat_agent_jobs where source_message_id=?uuid", &[&to_second.id.as_uuid().to_string()]).await, vec![second.agent_id.clone()], "explicit address must suppress the other agent's implicit followup");
    let second_job = service.claim().await.unwrap().unwrap();
    assert_eq!(second_job.agent_id, second.agent_id);
    execute(
        &store,
        "update circle_chat_agent_jobs set status='completed' where id=?uuid",
        &[&second_job.id],
    )
    .await;

    execute(
        &store,
        "update users set display_name='SAMTALEVENNEN' where id=?uuid",
        &[&second.agent_id],
    )
    .await;
    let ambiguous = message(&store, &mention_channel, &owner, 8, "@Samtalevennen!", None).await;
    conversation_enqueue(&store, &ambiguous, true).await;
    no_conversation_job(&store, &ambiguous).await;
    execute(
        &store,
        "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
        &[&second.agent_id],
    )
    .await;
    let unique_available =
        message(&store, &mention_channel, &owner, 9, "@Samtalevennen!", None).await;
    conversation_enqueue(&store, &unique_available, true).await;
    let unique_job = service.claim().await.unwrap().unwrap();
    assert_eq!(unique_job.agent_id, agent.agent_id);
    execute(
        &store,
        "update circle_chat_agent_jobs set status='completed' where id=?uuid",
        &[&unique_job.id],
    )
    .await;

    let explicit_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let (original, anchor) =
        conversation_anchor(&service, &explicit_channel, &owner, &agent.agent_id).await;
    let explicit = message(
        &store,
        &explicit_channel,
        &other,
        3,
        "Kan du utdjupe?",
        Some(anchor.id),
    )
    .await;
    conversation_enqueue(&store, &explicit, false).await;
    no_conversation_job(&store, &explicit).await;
    conversation_enqueue(&store, &explicit, true).await;
    let (explicit_job, explicit_source) =
        conversation_job(&service, &explicit, &anchor, "explicit").await;
    // Existing leases must lose source, context and publication authority together.
    for field in ["deleted_at", "edited_at"] {
        execute(
            &store,
            &format!("update messages set {field}=current_timestamp where id=?uuid"),
            &[&anchor.id.as_uuid().to_string()],
        )
        .await;
        conversation_denied(
            &service,
            &explicit_job,
            &explicit_source,
            Some(anchor.id),
            field,
        )
        .await;
        execute(
            &store,
            &format!("update messages set {field}=null where id=?uuid"),
            &[&anchor.id.as_uuid().to_string()],
        )
        .await;
    }
    execute(
        &store,
        "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    conversation_denied(
        &service,
        &explicit_job,
        &explicit_source,
        Some(anchor.id),
        "revoked profile",
    )
    .await;
    execute(
        &store,
        "update agent_profiles set revoked_at=null where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    execute(
        &store,
        "update circle_chat_agents set revision=revision+1 where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    conversation_denied(
        &service,
        &explicit_job,
        &explicit_source,
        Some(anchor.id),
        "configuration revision",
    )
    .await;
    execute(
        &store,
        "update circle_chat_agents set revision=revision-1 where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    execute(&store, "update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid", &[&explicit_channel]).await;
    conversation_denied(
        &service,
        &explicit_job,
        &explicit_source,
        Some(anchor.id),
        "channel revision",
    )
    .await;
    execute(&store, "update channels set chat_agent_access_revision=chat_agent_access_revision-1 where id=?uuid", &[&explicit_channel]).await;
    conversation_publication(&store, &explicit_job, Some(anchor.id))
        .await
        .unwrap();
    age_conversation(&store, &original, &anchor, 21).await;
    conversation_denied(
        &service,
        &explicit_job,
        &explicit_source,
        Some(anchor.id),
        "expired explicit anchor",
    )
    .await;
    let expired_explicit = message(
        &store,
        &explicit_channel,
        &other,
        4,
        "Og vidare?",
        Some(anchor.id),
    )
    .await;
    conversation_enqueue(&store, &expired_explicit, true).await;
    no_conversation_job(&store, &expired_explicit).await;
    execute(
        &store,
        "update circle_chat_agent_jobs set status='failed' where id=?uuid",
        &[&explicit_job.id],
    )
    .await;

    let implicit_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let (implicit_original, implicit_anchor) =
        conversation_anchor(&service, &implicit_channel, &owner, &agent.agent_id).await;
    let wrong_author = message(
        &store,
        &implicit_channel,
        &other,
        3,
        "Kan du forklare meir?",
        None,
    )
    .await;
    conversation_enqueue(&store, &wrong_author, true).await;
    no_conversation_job(&store, &wrong_author).await;
    // An unrelated, undeleted human message is the nearest predecessor, so the owner cannot continue implicitly.
    let interrupted = message(&store, &implicit_channel, &owner, 4, "Kva meiner du?", None).await;
    conversation_enqueue(&store, &interrupted, true).await;
    no_conversation_job(&store, &interrupted).await;
    for source in [&wrong_author, &interrupted] {
        execute(
            &store,
            "update messages set deleted_at=current_timestamp where id=?uuid",
            &[&source.id.as_uuid().to_string()],
        )
        .await;
    }
    let implicit = message(&store, &implicit_channel, &owner, 5, "Kva meiner du?", None).await;
    conversation_enqueue(&store, &implicit, true).await;
    let (implicit_job, implicit_source) =
        conversation_job(&service, &implicit, &implicit_anchor, "implicit").await;
    age_conversation(&store, &implicit_original, &implicit_anchor, 4).await;
    conversation_denied(
        &service,
        &implicit_job,
        &implicit_source,
        None,
        "implicit scope expires after three minutes",
    )
    .await;
    execute(
        &store,
        "update circle_chat_agent_jobs set status='failed' where id=?uuid",
        &[&implicit_job.id],
    )
    .await;

    // Four minutes is too old for implicit continuation but still inside explicit reply scope.
    let aged_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let (aged_original, aged_anchor) =
        conversation_anchor(&service, &aged_channel, &owner, &agent.agent_id).await;
    age_conversation(&store, &aged_original, &aged_anchor, 4).await;
    let stale_implicit = message(&store, &aged_channel, &owner, 3, "Kan du utdjupe?", None).await;
    conversation_enqueue(&store, &stale_implicit, true).await;
    no_conversation_job(&store, &stale_implicit).await;
    let still_explicit = message(
        &store,
        &aged_channel,
        &other,
        4,
        "Kan du utdjupe?",
        Some(aged_anchor.id),
    )
    .await;
    conversation_enqueue(&store, &still_explicit, true).await;
    conversation_job(&service, &still_explicit, &aged_anchor, "explicit").await;

    // A direct reply to the root wins over a newer implicit candidate in that thread.
    let priority_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let (_, priority_root) =
        conversation_anchor(&service, &priority_channel, &owner, &agent.agent_id).await;
    let child_source = message(
        &store,
        &priority_channel,
        &other,
        3,
        "Kan du utdjupe?",
        Some(priority_root.id),
    )
    .await;
    conversation_enqueue(&store, &child_source, true).await;
    let (child_job, _) =
        conversation_job(&service, &child_source, &priority_root, "explicit").await;
    let child = message(
        &store,
        &priority_channel,
        &UserId::new(&agent.agent_id).unwrap(),
        4,
        "Eit konkret svar",
        Some(priority_root.id),
    )
    .await;
    execute(
        &store,
        "insert into command_receipts(principal_id,request_id,message_id) values(?uuid,?,?uuid)",
        &[
            &agent.agent_id,
            &format!("circle-chat-agent:{}", child_job.id),
            &child.id.as_uuid().to_string(),
        ],
    )
    .await;
    execute(&store, "update circle_chat_agent_jobs set status='completed',reply_message_id=?uuid where id=?uuid", &[&child.id.as_uuid().to_string(), &child_job.id]).await;
    let direct_to_root = message(
        &store,
        &priority_channel,
        &other,
        5,
        "Og den andre delen?",
        Some(priority_root.id),
    )
    .await;
    conversation_enqueue(&store, &direct_to_root, true).await;
    conversation_job(&service, &direct_to_root, &priority_root, "explicit").await;

    // Enabling ordinary followups preserves the existing weather continuation mode.
    execute(
        &store,
        "update circle_chat_agents set weather=? where agent_id=?uuid",
        &[&serde_json::to_string(&weather()).unwrap(), &agent.agent_id],
    )
    .await;
    let weather_channel = conversation_channel(&store, &circle, &owner, &other).await;
    let (_, weather_anchor) =
        conversation_anchor(&service, &weather_channel, &owner, &agent.agent_id).await;
    let weather_continuation =
        message(&store, &weather_channel, &owner, 3, "Og i morgon?", None).await;
    conversation_enqueue(&store, &weather_continuation, true).await;
    let weather_job = service.claim().await.unwrap().unwrap();
    assert_eq!(
        weather_job.source_message_id,
        weather_continuation.id.as_uuid().to_string()
    );
    assert_eq!(
        values(
            &store,
            "select followup_mode from circle_chat_agent_jobs where id=?uuid",
            &[&weather_job.id]
        )
        .await,
        vec!["weather".to_owned()]
    );
    let weather_source = service.source(&weather_job).await.unwrap().unwrap();
    assert!(weather_source.weather.is_some());
    let weather_followup = weather_source.followup.unwrap();
    assert_eq!(weather_followup.mode, "weather");
    assert_eq!(
        weather_followup.anchor.id,
        weather_anchor.id.as_uuid().to_string()
    );
    execute(
        &store,
        "update circle_chat_agent_jobs set status='failed' where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
}

#[tokio::test]
async fn sqlite_conversation_followup_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    conversation_contract(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_conversation_followup_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    conversation_contract(Store::Pg(pool)).await;
}
