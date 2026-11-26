use super::*;
use crate::domain::InMemoryChatRepository;
use crate::work_items::github_export::tests::Fixture;
use std::sync::Arc;

async fn execute(store: &Store, query: &str, args: &[Uuid]) {
    macro_rules! run {
        ($pool:expr,$pg:expr) => {{
            let query = sql(query, $pg);
            let mut statement = sqlx::query(&query);
            for arg in args {
                statement = statement.bind(arg.to_string());
            }
            statement.execute($pool).await.unwrap();
        }};
    }
    match store {
        Store::Pg(pool) => run!(pool, true),
        Store::Sqlite(pool) => run!(pool, false),
    }
}

async fn count(store: &Store, query: &str, channel: Uuid) -> i64 {
    match store {
        Store::Pg(pool) => sqlx::query_scalar(&sql(query, true))
            .bind(channel.to_string())
            .fetch_one(pool)
            .await
            .unwrap(),
        Store::Sqlite(pool) => sqlx::query_scalar(&sql(query, false))
            .bind(channel.to_string())
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}

pub(crate) async fn seed_feedback_route(f: &Fixture) -> Uuid {
    let store = &f.service.store;
    let bot = Uuid::now_v7();
    execute(
        store,
        "insert into users(id,kind,display_name) values(?uuid,'agent','Sprøyt')",
        &[bot],
    )
    .await;
    execute(store, "insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,
        rate_limit_per_minute,created_at) values(?uuid,?uuid,?uuid,'sproyt-circle-chat',?,'Feedback',30,current_timestamp)",
        &[bot,f.owner,f.owner,bot]).await;
    execute(store,"insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,
        created_by,updated_by,created_at,updated_at) select ?uuid,owner_circle_id,'[]','[]',true,?uuid,?uuid,0,0
        from work_applications where id=?uuid", &[bot,f.owner,f.owner,f.application]).await;
    execute(
        store,
        "insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at)
        values(?uuid,?uuid,true,?uuid,0)",
        &[f.channel, bot, f.owner],
    )
    .await;
    execute(store,"insert into feedback_routes(channel_id,agent_id,reviewer_id,application_id,enabled,after_sequence)
        values(?uuid,?uuid,?uuid,?uuid,true,2)", &[f.channel,bot,f.reviewer,f.application]).await;
    bot
}

async fn feedback_contract(f: Fixture) {
    let store = &f.service.store;
    let bot = seed_feedback_route(&f).await;
    let root = Uuid::now_v7();
    let chat = ChatEngine::start(Arc::new(InMemoryChatRepository::default()));
    let add = "insert into messages(id,channel_id,parent_message_id,sender_id,sender_display_name,sequence,body)
        select ?uuid,?uuid,null,?uuid,'Tester',3,'Eg treng å finne tråden etter oppdatering'";
    execute(store, add, &[root, f.channel, f.owner]).await;
    execute(store,"insert into messages(id,channel_id,parent_message_id,sender_id,sender_display_name,sequence,body)
        values(?uuid,?uuid,?uuid,?uuid,'Tester',4,'Eit svar')",&[Uuid::now_v7(),f.channel,root,f.owner]).await;
    execute(
        store,
        "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body)
        values(?uuid,?uuid,?uuid,'Sprøyt',5,'Automatisk')",
        &[Uuid::now_v7(), f.channel, bot],
    )
    .await;
    execute(
        store,
        "update channel_sequences set next_sequence=6 where channel_id=?uuid",
        &[f.channel],
    )
    .await;
    let jobs = "select count(*) from feedback_jobs where channel_id=?uuid";
    f.service.admit_feedback().await.unwrap();
    assert_eq!(
        count(store, jobs, f.channel).await,
        0,
        "a granted private circle channel is outside Felles"
    );
    execute(
        store,
        "update channels set circle_id=null,kind='public' where id=?uuid",
        &[f.channel],
    )
    .await;
    f.service.admit_feedback().await.unwrap();
    f.service.admit_feedback().await.unwrap();
    assert_eq!(
        count(store, jobs, f.channel).await,
        1,
        "only the new human root enters the queue"
    );
    execute(store,"update feedback_jobs set status='published',publication_kind='create',issue_id=789,issue_number=9,
        issue_url='https://github.com/owner/repo/issues/9' where source_message_id=?uuid",&[root]).await;
    execute(
        store,
        "update channel_chat_agent_settings set enabled=false where channel_id=?uuid",
        &[f.channel],
    )
    .await;
    assert_eq!(
        f.service.acknowledge_feedback(&chat, root).await,
        Err(RepositoryError::PermissionDenied)
    );
    execute(
        store,
        "update channel_chat_agent_settings set enabled=true where channel_id=?uuid",
        &[f.channel],
    )
    .await;
    // A pause/resume sets a new admission fence. The earlier durable job still owns its receipt.
    execute(
        store,
        "update feedback_routes set after_sequence=5 where channel_id=?uuid",
        &[f.channel],
    )
    .await;
    let (first, replay) = tokio::join!(
        f.service.acknowledge_feedback(&chat, root),
        f.service.acknowledge_feedback(&chat, root)
    );
    first.unwrap();
    replay.unwrap();
    let receipts = "select count(*) from feedback_jobs j join messages reply on reply.id=j.ack_message_id
        and reply.parent_message_id=j.source_message_id and reply.sender_id=j.agent_id and reply.body like '%https://github.com/owner/repo/issues/9'
        join message_provenance p on p.message_id=reply.id and p.provenance='generated'
        join message_reactions mr on mr.user_id=j.agent_id and mr.message_id=j.source_message_id and mr.emoji='📋'
        where j.channel_id=?uuid";
    assert_eq!(count(store, receipts, f.channel).await, 1);
    assert_eq!(
        count(
            store,
            "select count(*) from messages where parent_message_id=?uuid",
            root
        )
        .await,
        2
    );
    f.service.admit_feedback().await.unwrap();
    assert_eq!(count(store, jobs, f.channel).await, 1);
    let actor = UserId::from_uuid(f.reviewer);
    let old = f.message;
    assert_eq!(
        f.service
            .feedback_activation(&actor, f.channel, old, false)
            .await
            .unwrap()
            .as_deref(),
        Some("available")
    );
    assert!(
        f.service
            .feedback_activation(&UserId::from_uuid(f.outsider), f.channel, old, true)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        f.service
            .feedback_activation(&actor, Uuid::now_v7(), old, true)
            .await
            .unwrap()
            .is_none()
    );
    let (first, retry) = tokio::join!(
        f.service.feedback_activation(&actor, f.channel, old, true),
        f.service.feedback_activation(&actor, f.channel, old, true),
    );
    for receipt in [first, retry] {
        assert_eq!(receipt.unwrap().as_deref(), Some("pending"));
    }
    f.service.admit_feedback().await.unwrap();
    assert_eq!(count(store, jobs, f.channel).await, 2);
    execute(
        store,
        "update feedback_jobs set intake_version=1 where source_message_id=?uuid",
        &[old],
    )
    .await;
    let task = Uuid::now_v7();
    execute(store,"insert into feedback_intakes(source_message_id,stage,task_id) values(?uuid,'clarify',?uuid)", &[old,task]).await;
    let viewer = UserId::from_uuid(f.owner); // A full member of the channel, distinct from its product owner.
    let member_view = f
        .service
        .feedback_intake(viewer.clone(), f.channel, old)
        .await
        .unwrap()
        .unwrap();
    assert!(!member_view.can_clarify);
    assert!(member_view.clarification_task_id.is_none());
    let reviewer_view = f
        .service
        .feedback_intake(actor.clone(), f.channel, old)
        .await
        .unwrap()
        .unwrap();
    assert!(reviewer_view.can_clarify);
    assert_eq!(reviewer_view.clarification_task_id, Some(task.to_string()));
    for (user, visible) in [(actor.clone(), true), (viewer.clone(), false)] {
        let page = f
            .service
            .task_queue(user, Some(f.channel), None, 50, None)
            .await
            .unwrap();
        let queue = serde_json::to_value(page).unwrap();
        assert_eq!(
            queue["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["kind"] == "feedback" && row["task"]["id"] == task.to_string()),
            visible
        );
    }
    let cmd = || {
        serde_json::from_value(json!({"task_id":task,"decision":"dismiss","issue_number":null,"note":"Allereie løyst"})).unwrap()
    };
    assert!(
        f.service
            .clarify_feedback(viewer, f.channel, old, cmd())
            .await
            .is_err()
    );
    for _ in 0..2 {
        let accepted = f
            .service
            .clarify_feedback(actor.clone(), f.channel, old, cmd())
            .await
            .unwrap();
        assert!(!accepted.can_clarify);
    }
    assert_eq!(
        count(
            store,
            "select count(*) from feedback_intake_results where source_message_id=?uuid",
            old
        )
        .await,
        1
    );
    execute(
        store,
        "update feedback_routes set enabled=false where channel_id=?uuid",
        &[f.channel],
    )
    .await;
    assert!(
        f.service
            .feedback_activation(&actor, f.channel, old, true)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sqlite_felles_feedback_admission_and_delivery() {
    feedback_contract(Fixture::sqlite(false).await).await;
}

#[tokio::test]
async fn postgres_felles_feedback_admission_and_delivery() {
    if let Some(fixture) = Fixture::postgres(false).await {
        feedback_contract(fixture).await;
    }
}
