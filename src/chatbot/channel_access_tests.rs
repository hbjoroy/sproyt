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

async fn publication(store: &Store, job: &Job) -> Result<()> {
    let command = SendMessage {
        actor: UserId::new(&job.agent_id).unwrap(),
        channel_id: ChannelId::new(&job.channel_id).unwrap(),
        parent_message_id: None,
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

async fn source_message(
    store: &Store,
    channel: &str,
    actor: &UserId,
    sequence: u64,
) -> ChatMessage {
    let id = MessageId::generate();
    let created = if matches!(store, Store::Pg(_)) {
        "now()"
    } else {
        "strftime('%Y-%m-%dT%H:%M:%fZ','now')"
    };
    let query = format!(
        "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Manager',?int,'Hjelp med privat arbeid',{created})"
    );
    execute(
        store,
        &query,
        &[
            &id.as_uuid().to_string(),
            channel,
            &actor.to_string(),
            &sequence.to_string(),
        ],
    )
    .await;
    ChatMessage {
        id,
        channel_id: ChannelId::new(channel).unwrap(),
        parent_message_id: None,
        sender_id: actor.clone(),
        sender_display_name: DisplayName::new("Manager").unwrap(),
        body: MessageBody::new("Hjelp med privat arbeid").unwrap(),
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

async fn contract(store: Store) {
    let service = CircleChatAgents {
        store: store.clone(),
        model: None,
        worker_enabled: false,
    };
    let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let manager = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let observer = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let outsider = UserId::new(Uuid::now_v7().to_string()).unwrap();
    for actor in [&owner, &manager, &observer, &outsider] {
        execute(
            &store,
            "insert into users(id,kind,display_name) values(?uuid,'human','Access test')",
            &[&actor.to_string()],
        )
        .await;
    }
    let circle = Uuid::now_v7().to_string();
    let other_circle = Uuid::now_v7().to_string();
    for (id, actor) in [(&circle, &owner), (&other_circle, &outsider)] {
        execute(
            &store,
            "insert into circles(id,slug,name,created_by) values(?uuid,?,'Access test',?uuid)",
            &[id, &format!("access-{id}"), &actor.to_string()],
        )
        .await;
        execute(
            &store,
            "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')",
            &[id, &actor.to_string()],
        )
        .await;
    }
    execute(
        &store,
        "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'moderator')",
        &[&circle, &observer.to_string()],
    )
    .await;
    let public = Uuid::now_v7().to_string();
    let local = Uuid::now_v7().to_string();
    let private = Uuid::now_v7().to_string();
    for (id, kind) in [
        (&public, "public"),
        (&local, "local"),
        (&private, "private"),
    ] {
        execute(&store, "insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Access test',?,?uuid,?uuid)", &[id, &format!("access-{id}"), kind, &owner.to_string(), &circle]).await;
        execute(
            &store,
            "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",
            &[id, &owner.to_string()],
        )
        .await;
        execute(&store, "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'moderator')", &[id, &manager.to_string()]).await;
        execute(&store, "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'observer')", &[id, &observer.to_string()]).await;
    }
    let input = || AgentInput {
        display_name: "Kanalhjelpar".into(),
        trigger_words: vec!["hjelp".into()],
        response_phrases: vec!["Eg kan hjelpe".into()],
        enabled: false,
        revision: None,
    };
    let agent = service.create(&owner, &circle, input()).await.unwrap();
    let foreign = service
        .create(&outsider, &other_circle, input())
        .await
        .unwrap();
    execute(
        &store,
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        &[&agent.agent_id],
    )
    .await;
    for channel in [&public, &local] {
        let choices = service.list_channel(&owner, channel).await.unwrap();
        assert_eq!(choices.access_revision, 1);
        assert_eq!(choices.agents.len(), 1);
        assert!(choices.agents[0].enabled && choices.agents[0].agent_enabled);
    }
    // Circle authority cannot opt a private channel in without channel authority.
    assert!(matches!(
        service.list_channel(&owner, &private).await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(
        !service
            .list_channel(&manager, &private)
            .await
            .unwrap()
            .agents[0]
            .enabled
    );
    for denied in [&observer, &outsider] {
        assert!(matches!(
            service.list_channel(denied, &public).await,
            Err(RepositoryError::PermissionDenied)
        ));
        assert!(matches!(
            service
                .update_channel(
                    denied,
                    &private,
                    &agent.agent_id,
                    ChannelAgentInput {
                        enabled: true,
                        access_revision: 1
                    }
                )
                .await,
            Err(RepositoryError::PermissionDenied)
        ));
    }
    assert!(matches!(
        service
            .update_channel(
                &manager,
                &private,
                &foreign.agent_id,
                ChannelAgentInput {
                    enabled: true,
                    access_revision: 1
                }
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    let first = source_message(&store, &private, &manager, 1).await;
    enqueue(&store, &first).await;
    assert!(
        service.claim().await.unwrap().is_none(),
        "private channels must start without access"
    );
    let opted_in = service
        .update_channel(
            &manager,
            &private,
            &agent.agent_id,
            ChannelAgentInput {
                enabled: true,
                access_revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(opted_in.access_revision, 2);
    assert!(opted_in.agents[0].enabled);
    assert!(matches!(
        service
            .update_channel(
                &manager,
                &private,
                &agent.agent_id,
                ChannelAgentInput {
                    enabled: false,
                    access_revision: 1
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert_eq!(
        service.list(&owner, &circle).await.unwrap()[0].revision,
        1,
        "channel choices do not rewrite agent configuration"
    );
    enqueue(&store, &first).await;
    let old_job = service.claim().await.unwrap().unwrap();
    assert_eq!(old_job.source_message_id, first.id.as_uuid().to_string());
    let old_source = service.source(&old_job).await.unwrap().unwrap();
    assert_eq!(
        service.context(&old_job, &old_source).await.unwrap().len(),
        1
    );
    execute(
        &store,
        "update circle_chat_agent_jobs set reply_body='Eit konkret svar' where id=?uuid",
        &[&old_job.id],
    )
    .await;
    publication(&store, &old_job).await.unwrap();
    let off = service
        .update_channel(
            &manager,
            &private,
            &agent.agent_id,
            ChannelAgentInput {
                enabled: false,
                access_revision: 2,
            },
        )
        .await
        .unwrap();
    assert_eq!(off.access_revision, 3);
    assert!(!off.agents[0].enabled);
    assert!(service.source(&old_job).await.unwrap().is_none());
    assert!(matches!(
        service.context(&old_job, &old_source).await,
        Err(RepositoryError::Conflict)
    ));
    assert!(matches!(
        publication(&store, &old_job).await,
        Err(RepositoryError::PermissionDenied)
    ));
    let second = source_message(&store, &private, &manager, 2).await;
    enqueue(&store, &second).await;
    assert!(
        service.claim().await.unwrap().is_none(),
        "off prevents new jobs"
    );
    let on = service
        .update_channel(
            &manager,
            &private,
            &agent.agent_id,
            ChannelAgentInput {
                enabled: true,
                access_revision: 3,
            },
        )
        .await
        .unwrap();
    assert_eq!(on.access_revision, 4);
    assert!(
        service.source(&old_job).await.unwrap().is_none(),
        "off/on must not revive an old job"
    );
    assert!(matches!(
        publication(&store, &old_job).await,
        Err(RepositoryError::PermissionDenied)
    ));
    let third = source_message(&store, &private, &manager, 3).await;
    enqueue(&store, &third).await;
    let fresh = service.claim().await.unwrap().unwrap();
    assert_eq!(fresh.source_message_id, third.id.as_uuid().to_string());
    let source = service.source(&fresh).await.unwrap().unwrap();
    assert_eq!(
        service
            .context(&fresh, &source)
            .await
            .unwrap()
            .last()
            .unwrap()
            .id,
        third.id.as_uuid().to_string()
    );
    assert_eq!(service.list(&owner, &circle).await.unwrap()[0].revision, 1);
}

#[tokio::test]
async fn sqlite_channel_agent_access_contract() {
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
async fn postgres_channel_agent_access_contract() {
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
