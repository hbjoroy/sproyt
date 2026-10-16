use super::*;
use crate::domain::{ChannelSequence, ChatMessage, DisplayName, MessageId};

async fn execute(store: &Store, query: &str, args: &[&str]) {
    store
        .execute(
            query,
            &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        )
        .await
        .unwrap();
}

async fn contract(store: Store) {
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
    let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
    let circle = Uuid::now_v7().to_string();
    let channel = Uuid::now_v7().to_string();
    execute(
        &store,
        "insert into users(id,kind,display_name) values(?uuid,'human','Routing owner')",
        &[&owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into circles(id,slug,name,created_by) values(?uuid,?,'Routing contract',?uuid)",
        &[&circle, &format!("routing-{circle}"), &owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&circle, &owner.to_string()],
    )
    .await;
    execute(&store,"insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Routing contract','public',?uuid,?uuid)",&[&channel,&format!("routing-{channel}"),&owner.to_string(),&circle]).await;
    execute(
        &store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&channel, &owner.to_string()],
    )
    .await;
    let input = || AgentInput {
        display_name: "Routing agent".into(),
        trigger_words: vec!["route".into()],
        response_phrases: vec!["Helpful".into()],
        enabled: false,
        memory_enabled: None,
        revision: None,
        weather: None,
        sea_temperature: None,
        ferry_port: None,
        vision_enabled: None,
        image_generation: None,
    };
    let source = service
        .create(&owner, &circle, input())
        .await
        .unwrap()
        .agent_id;
    let target = service
        .create(&owner, &circle, input())
        .await
        .unwrap()
        .agent_id;
    for id in [&source, &target] {
        execute(
            &store,
            "update circle_chat_agents set enabled=true where agent_id=?uuid",
            &[id],
        )
        .await;
    }
    execute(&store,"insert into agent_handoff_configs(agent_id,revision,target_agent_ids,guidance) values(?uuid,1,?,'Delegate route requests')",&[&source,&serde_json::to_string(&vec![&target]).unwrap()]).await;
    let view = service
        .handoff_config(&owner, &circle, &source)
        .await
        .unwrap();
    assert_eq!(view.revision, 1);
    assert_eq!(view.target_agent_ids, vec![target.clone()]);
    let outsider = UserId::new(Uuid::now_v7().to_string()).unwrap();
    assert!(matches!(
        service.handoff_config(&outsider, &circle, &source).await,
        Err(RepositoryError::PermissionDenied)
    ));
    for (sequence, scenario) in [
        "target",
        "retry",
        "self",
        "wrong_target",
        "edited",
        "deleted",
        "config_revoked",
        "source_revoked",
        "wrong_channel",
        "target_router",
        "model_busy",
        "expired",
        "restart",
        "access_revoked",
    ]
    .iter()
    .enumerate()
    {
        let message = ChatMessage {
            id: MessageId::generate(),
            channel_id: ChannelId::new(&channel).unwrap(),
            parent_message_id: None,
            sender_id: owner.clone(),
            sender_display_name: DisplayName::new("Owner").unwrap(),
            body: MessageBody::new("route this original human request").unwrap(),
            sequence: ChannelSequence::new(sequence as u64 + 1),
            sent_at: Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        let pg = matches!(store, Store::Pg(_));
        let timestamp = if pg {
            "now()"
        } else {
            "strftime('%Y-%m-%dT%H:%M:%fZ','now')"
        };
        execute(&store,&format!("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',?int,?,{timestamp})"),&[&message.id.as_uuid().to_string(),&channel,&owner.to_string(),&(sequence+1).to_string(),message.body.as_str()]).await;
        // Provenance trigger records human origin; route enqueue substitutes only
        // the source ordinary admission, never other directly triggered agents.
        match &store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.unwrap();
                assert!(
                    enqueue_postgres(&mut tx, &message, &source, 1, 1)
                        .await
                        .unwrap()
                );
                tx.commit().await.unwrap();
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin().await.unwrap();
                assert!(
                    enqueue_sqlite(&mut tx, &message, &source, 1, 1)
                        .await
                        .unwrap()
                );
                tx.commit().await.unwrap();
            }
        }
        assert!(
            service.claim().await.unwrap().is_none(),
            "old ordinary claim must never see routing jobs"
        );
        // Simulate old-production ingress; INSERT is ignored without errors.
        execute(&store,"insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,access_revision,status,available_at,created_at) values(?uuid,?uuid,?uuid,?uuid,1,1,'pending',0,0) on conflict(agent_id,source_message_id) do nothing",&[&Uuid::now_v7().to_string(),&source,&message.id.as_uuid().to_string(),&channel]).await;
        assert!(service.claim().await.unwrap().is_none());
        let (first, second) = tokio::join!(service.claim_handoff(), service.claim_handoff());
        let claims = [first.unwrap(), second.unwrap()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(claims.len(), 1, "concurrent claims must lease exactly once");
        let mut job = claims.into_iter().next().unwrap();
        assert!(
            service.claim_handoff().await.unwrap().is_none(),
            "leased jobs cannot be claimed twice"
        );
        let mut recipient = target.clone();
        match *scenario {
            "self"=>recipient=source.clone(),
            "wrong_target"=>recipient=Uuid::now_v7().to_string(),
            "edited"=>execute(&store,&format!("update messages set edited_at={timestamp} where id=?uuid"),&[&message.id.as_uuid().to_string()]).await,
            "deleted"=>{execute(&store,&format!("update messages set deleted_at={timestamp} where id=?uuid"),&[&message.id.as_uuid().to_string()]).await;assert!(service.handoff_message(&job).await.unwrap().is_none());},
            "config_revoked"=>execute(&store,"update agent_handoff_configs set revision=revision+1 where agent_id=?uuid",&[&source]).await,
            "source_revoked"=>execute(&store,"update circle_chat_agents set enabled=false where agent_id=?uuid",&[&source]).await,
            "wrong_channel"=>execute(&store,"insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,false,?uuid,0) on conflict(channel_id,agent_id) do update set enabled=false",&[&channel,&target,&owner.to_string()]).await,
            "target_router"=>{
                execute(&store,"insert into agent_handoff_configs(agent_id,revision,target_agent_ids,guidance) values(?uuid,1,?,'Other router') on conflict(agent_id) do nothing",&[&target,&serde_json::to_string(&vec![&source]).unwrap()]).await;
                match &store {Store::Pg(pool)=>{let mut tx=pool.begin().await.unwrap();assert!(enqueue_postgres(&mut tx,&message,&target,1,1).await.unwrap());tx.commit().await.unwrap();},Store::Sqlite(pool)=>{let mut tx=pool.begin().await.unwrap();assert!(enqueue_sqlite(&mut tx,&message,&target,1,1).await.unwrap());tx.commit().await.unwrap();}}
            },
            "model_busy"=>{
                for _ in 0..5 {
                    service.retry_handoff_model_busy(&job).await.unwrap();
                    assert!(service.claim_handoff().await.unwrap().is_none(),"busy permit backs off");
                    let attempts=store.values("select cast(attempts as text) from agent_handoff_jobs where id=?uuid",&[job.id.clone()]).await.unwrap();assert_eq!(attempts,vec!["0"]);
                    execute(&store,"update agent_handoff_jobs set available_at=0 where id=?uuid",&[&job.id]).await;
                    job=service.claim_handoff().await.unwrap().unwrap();
                }
            },
            "access_revoked"=>execute(&store,"update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid",&[&channel]).await,
            "expired"=>execute(&store,"update agent_handoff_jobs set expires_at=0 where id=?uuid",&[&job.id]).await,
            "restart"=>{execute(&store,"update agent_handoff_jobs set leased_until=0 where id=?uuid",&[&job.id]).await;let resumed=service.claim_handoff().await.unwrap().unwrap();assert_ne!(job.lease_token,resumed.lease_token);service.admit_handoff(&job,&target).await.unwrap();job=resumed;},
            _=>{}
        }
        service.admit_handoff(&job, &recipient).await.unwrap();
        if *scenario == "retry" {
            service.admit_handoff(&job, &source).await.unwrap();
        }
        let jobs=store.values("select cast(agent_id as text) from circle_chat_agent_jobs where source_message_id=?uuid",&[message.id.as_uuid().to_string()]).await.unwrap();
        if [
            "edited",
            "deleted",
            "config_revoked",
            "source_revoked",
            "expired",
            "access_revoked",
        ]
        .contains(scenario)
        {
            assert!(jobs.is_empty(), "revoked admission {scenario}");
        } else {
            assert_eq!(jobs.len(), 1);
            assert_eq!(
                &jobs[0],
                if ["self", "wrong_target", "wrong_channel", "target_router"].contains(scenario) {
                    &source
                } else {
                    &target
                }
            );
        }
        // Never carry source location grants to an independently admitted target.
        let grants=store.values("select cast(id as text) from circle_chat_agent_jobs where source_message_id=?uuid and location_share_id is not null",&[message.id.as_uuid().to_string()]).await.unwrap();
        assert!(grants.is_empty());
        if *scenario == "target_router" {
            let other = service.claim_handoff().await.unwrap().unwrap();
            service.admit_handoff(&other, &source).await.unwrap();
            let both=store.values("select cast(agent_id as text) from circle_chat_agent_jobs where source_message_id=?uuid",&[message.id.as_uuid().to_string()]).await.unwrap();
            assert_eq!(
                both.len(),
                2,
                "reciprocal routing reservations each terminate once through their source"
            );
        }
        execute(
            &store,
            "update circle_chat_agent_jobs set status='completed' where source_message_id=?uuid",
            &[&message.id.as_uuid().to_string()],
        )
        .await;
        execute(
            &store,
            "update agent_handoff_configs set revision=1 where agent_id=?uuid",
            &[&source],
        )
        .await;
        execute(
            &store,
            "update circle_chat_agents set enabled=true where agent_id=?uuid",
            &[&source],
        )
        .await;
        execute(
            &store,
            "update channels set chat_agent_access_revision=1 where id=?uuid",
            &[&channel],
        )
        .await;
        execute(
            &store,
            "delete from channel_chat_agent_settings where channel_id=?uuid and agent_id=?uuid",
            &[&channel, &target],
        )
        .await;
    }
    if let Store::Pg(pool) = &store {
        concurrent_postgres_reciprocal(&service, pool, &owner, &circle, &channel, &source, &target)
            .await;
    }
}

async fn concurrent_postgres_reciprocal(
    service: &CircleChatAgents,
    pool: &PgPool,
    owner: &UserId,
    circle: &str,
    channel: &str,
    source: &str,
    target: &str,
) {
    let store = Store::Pg(pool.clone());
    let other = Uuid::now_v7().to_string();
    execute(&store,"insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Concurrent routing','public',?uuid,?uuid)",&[&other,&format!("routing-{other}"),&owner.to_string(),circle]).await;
    execute(
        &store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&other, &owner.to_string()],
    )
    .await;
    let mut messages = Vec::new();
    for (agent, channel, sequence) in [(source, channel, 100), (target, other.as_str(), 1)] {
        let message = ChatMessage {
            id: MessageId::generate(),
            channel_id: ChannelId::new(channel).unwrap(),
            parent_message_id: None,
            sender_id: owner.clone(),
            sender_display_name: DisplayName::new("Owner").unwrap(),
            body: MessageBody::new("route original request").unwrap(),
            sequence: ChannelSequence::new(sequence),
            sent_at: Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        execute(&store,"insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',?int,?,now())",&[&message.id.as_uuid().to_string(),channel,&owner.to_string(),&sequence.to_string(),message.body.as_str()]).await;
        let mut tx = pool.begin().await.unwrap();
        assert!(
            enqueue_postgres(&mut tx, &message, agent, 1, 1)
                .await
                .unwrap()
        );
        tx.commit().await.unwrap();
        messages.push(message);
    }
    // Spawned tasks plus a barrier force separate concurrent pool operations;
    // this contract runs only against an actual PostgreSQL test database.
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let service = service.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            service.claim_handoff().await.unwrap().unwrap()
        }));
    }
    let mut jobs = Vec::new();
    for task in tasks {
        jobs.push(task.await.unwrap());
    }
    assert_ne!(jobs[0].id, jobs[1].id);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for job in jobs {
        let service = service.clone();
        let barrier = barrier.clone();
        let recipient = if job.agent_id == source {
            target.to_owned()
        } else {
            source.to_owned()
        };
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            service.admit_handoff(&job, &recipient).await.unwrap();
            service.admit_handoff(&job, &recipient).await.unwrap();
        }));
    }
    tokio::time::timeout(Duration::from_secs(15), async {
        for task in tasks {
            task.await.unwrap();
        }
    })
    .await
    .expect("opposite delegations in different channels must make progress without deadlock");
    for (index, message) in messages.iter().enumerate() {
        let jobs=store.values("select cast(agent_id as text) from circle_chat_agent_jobs where source_message_id=?uuid",&[message.id.as_uuid().to_string()]).await.unwrap();
        assert_eq!(
            jobs,
            vec![if index == 0 {
                target.to_owned()
            } else {
                source.to_owned()
            }]
        );
    }
}

#[tokio::test]
async fn sqlite_handoff_admission_contract() {
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
async fn postgres_handoff_admission_contract() {
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
