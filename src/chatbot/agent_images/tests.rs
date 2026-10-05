use super::super::weather_followup_tests::execute;
use super::*;
use crate::domain::ChatRepository;

async fn value(store: &Store, query: &str, args: &[&str]) -> String {
    store
        .values(
            query,
            &args.iter().map(|v| v.to_string()).collect::<Vec<_>>(),
        )
        .await
        .unwrap()
        .into_iter()
        .next()
        .unwrap()
}
fn input(revision: Option<i64>, config: Option<Option<ImageGenerationConfig>>) -> AgentInput {
    AgentInput {
        display_name: "Picture friend".into(),
        trigger_words: vec!["hjelp".into()],
        response_phrases: vec!["Be warm and natural".into()],
        enabled: false,
        revision,
        weather: None,
        ferry_port: None,
        vision_enabled: None,
        image_generation: config,
    }
}
async fn admission(store: &Store, source: &crate::domain::ChatMessage, images: bool) {
    match store {
        Store::Pg(p) => {
            let mut tx = p.begin().await.unwrap();
            enqueue_postgres_with_images(&mut tx, source, false, false, false, images)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
        Store::Sqlite(p) => {
            let mut tx = p.begin_with("BEGIN IMMEDIATE").await.unwrap();
            enqueue_sqlite_with_images(&mut tx, source, false, false, false, images)
                .await
                .unwrap();
            tx.commit().await.unwrap();
        }
    }
}
async fn seed_text_reply(
    service: &CircleChatAgents,
    chat: &ChatEngine,
    source: &crate::domain::ChatMessage,
    agent: &str,
) -> String {
    let id=value(&service.store,"select cast(id as text) from circle_chat_agent_jobs where agent_id=?uuid and source_message_id=?uuid",&[agent,&source.id.as_uuid().to_string()]).await;
    execute(&service.store,"update circle_chat_agent_jobs set status='leased',lease_token=?uuid,leased_until=?int,reply_body='A concrete fresh reply.' where id=?uuid",&[&Uuid::now_v7().to_string(),&(Utc::now().timestamp()+90).to_string(),&id]).await;
    chat.send_message_idempotent(
        source.channel_id.clone(),
        UserId::new(agent).unwrap(),
        MessageBody::new("A concrete fresh reply.").unwrap(),
        format!("circle-chat-agent:{id}"),
    )
    .await
    .unwrap();
    id
}
async fn prepare_candidate(
    service: &CircleChatAgents,
    chat: &ChatEngine,
    channel: &str,
    owner: &UserId,
    agent: &str,
) -> PictureRequest {
    let source = chat
        .send_message(
            ChannelId::new(channel).unwrap(),
            owner.clone(),
            MessageBody::new("Hjelp: lag eit nytt bilete av deg på ein gresk kafé.").unwrap(),
        )
        .await
        .unwrap();
    admission(&service.store, &source, true).await;
    let id=value(&service.store,"select cast(id as text) from agent_image_publications where source_message_id=?uuid and agent_id=?uuid",&[&source.id.as_uuid().to_string(),agent]).await;
    assert!(
        service
            .image_request(&id, &Uuid::now_v7().to_string())
            .await
            .unwrap()
            .is_none(),
        "no image before its committed text receipt"
    );
    seed_text_reply(service, chat, &source, agent).await;
    let token = Uuid::now_v7().to_string();
    execute(&service.store,"update agent_image_publications set state='admitting',lease_token=?uuid,leased_until=?int where id=?uuid",&[&token,&(Utc::now().timestamp()+90).to_string(),&id]).await;
    // The text worker may have crashed after receipt commit and before status completion.
    service.image_request(&id, &token).await.unwrap().unwrap()
}
async fn contract(
    store: Store,
    repository: Arc<dyn ChatRepository>,
    imagegen: crate::imagegen::ImageGeneration,
) {
    let service = CircleChatAgents {
        store: store.clone(),
        model: None,
        worker_enabled: false,
        weather: None,
        ferry: None,
        observations: None,
        imagegen: Some(imagegen.clone()),
    };
    let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
    execute(
        &store,
        "insert into users(id,kind,display_name) values(?uuid,'human','Image owner')",
        &[&owner.to_string()],
    )
    .await;
    let circle = Uuid::now_v7().to_string();
    let channel = Uuid::now_v7().to_string();
    execute(
        &store,
        "insert into circles(id,slug,name,created_by) values(?uuid,?,'Image circle',?uuid)",
        &[&circle, &format!("image-{circle}"), &owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&circle, &owner.to_string()],
    )
    .await;
    execute(&store,"insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Image channel','public',?uuid,?uuid)",&[&channel,&format!("image-{channel}"),&circle,&owner.to_string()]).await;
    execute(
        &store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&channel, &owner.to_string()],
    )
    .await;
    execute(
        &store,
        "insert into channel_sequences(channel_id) values(?uuid)",
        &[&channel],
    )
    .await;
    let config = ImageGenerationConfig {
        identity_id: "maria-v1".into(),
        enabled: true,
        occasional: false,
    };
    assert!(
        service
            .create(
                &owner,
                &circle,
                input(
                    None,
                    Some(Some(ImageGenerationConfig {
                        identity_id: "untrusted-external-url".into(),
                        ..config.clone()
                    }))
                )
            )
            .await
            .is_err()
    );
    let created = service
        .create(&owner, &circle, input(None, Some(Some(config.clone()))))
        .await
        .unwrap();
    assert_eq!(created.image_generation, Some(config.clone()));
    assert!(!created.image_generation_available);
    let preserved = service
        .update(
            &owner,
            &circle,
            &created.agent_id,
            input(Some(created.revision), None),
        )
        .await
        .unwrap();
    assert_eq!(preserved.image_generation, Some(config));
    execute(
        &store,
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        &[&created.agent_id],
    )
    .await;
    let chat = ChatEngine::start(repository);
    let request = prepare_candidate(&service, &chat, &channel, &owner, &created.agent_id).await;
    assert_ne!(request.agent_name, "Maria");
    let intent = Intent {
        mode: "explicit".into(),
        scene: "At a sunny Greek cafe, casual everyday clothes.".into(),
    };
    // An occupied per-agent GPU slot must roll back the entire reservation/quota.
    let blocker = crate::imagegen::ImageGeneration::agent_job(
        &Uuid::now_v7().to_string(),
        UserId::new(&created.agent_id).unwrap(),
        ChannelId::new(&channel).unwrap(),
        "existing job".into(),
        "maria-v1".into(),
        crate::imagegen::identity::get("maria-v1").unwrap().sha256(),
    );
    execute(&store,"insert into image_generation_jobs(id,owner_id,channel_id,state,slot,updated_at,data) values(?,?,?,'queued',8,0,?)",&[&blocker.id,&blocker.owner_id.to_string(),&channel,&serde_json::to_string(&blocker).unwrap()]).await;
    service.reserve_picture(&request, &intent).await.unwrap();
    assert_eq!(value(&store,"select cast(count(*) as text) from agent_image_publications where id=?uuid and reserved_at is not null",&[&request.id]).await,"0");
    execute(
        &store,
        "delete from image_generation_jobs where id=?",
        &[&blocker.id],
    )
    .await;
    service.reserve_picture(&request, &intent).await.unwrap();
    let id = value(
        &store,
        "select image_job_id from agent_image_publications where id=?uuid",
        &[&request.id],
    )
    .await;
    let mut job = imagegen.get(&id).await.unwrap().unwrap();
    assert_eq!(
        job.owner_id.to_string(),
        created.agent_id,
        "not the human's private image queue"
    );
    assert!(imagegen.list(owner.clone()).await.unwrap().is_empty());
    assert_eq!(job.agent_identity.as_ref().unwrap().identity_id, "maria-v1");
    // A repeated reservation cannot consume another quota/GPU slot.
    service.reserve_picture(&request, &intent).await.unwrap();
    assert_eq!(
        value(
            &store,
            "select cast(count(*) as text) from image_generation_jobs where id=?",
            &[&id]
        )
        .await,
        "1"
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(12, 12, image::Rgb([255, 0, 0]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    use base64::Engine;
    job.image = Some(base64::engine::general_purpose::STANDARD.encode(png.into_inner()));
    job.transition("ready");
    assert!(imagegen.save(&mut job).await.unwrap());
    let stale = job.clone();
    service.publish_ready_picture(&chat).await.unwrap();
    let reply=value(&store,"select cast(reply_message_id as text) from agent_image_publications where id=?uuid and state='published'",&[&request.id]).await;
    assert_eq!(
        value(
            &store,
            "select cast(count(*) as text) from message_attachments where message_id=?uuid",
            &[&reply]
        )
        .await,
        "1"
    );
    let finalized = imagegen.get(&id).await.unwrap().unwrap();
    assert_eq!(finalized.state, "dismissed");
    assert!(finalized.image.is_none());
    assert!(finalized.revision > stale.revision);
    let mut stale = stale;
    assert!(
        !imagegen.save(&mut stale).await.unwrap(),
        "a pre-publication worker cannot overwrite final state/data"
    );
    service.publish_ready_picture(&chat).await.unwrap();
    assert_eq!(value(&store,"select cast(count(*) as text) from messages where body like 'Generert bilete%' and channel_id=?uuid",&[&channel]).await,"1");
    // Authority/source changes invalidate admission before any external generation.
    let second = prepare_candidate(&service, &chat, &channel, &owner, &created.agent_id).await;
    let source_id = value(
        &store,
        "select cast(source_message_id as text) from agent_image_publications where id=?uuid",
        &[&second.id],
    )
    .await;
    for (change, restore, target) in [
        (
            "update circle_chat_agents set revision=revision+1 where agent_id=?uuid",
            "update circle_chat_agents set revision=revision-1 where agent_id=?uuid",
            created.agent_id.as_str(),
        ),
        (
            "update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid",
            "update channels set chat_agent_access_revision=chat_agent_access_revision-1 where id=?uuid",
            channel.as_str(),
        ),
        (
            "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
            "update agent_profiles set revoked_at=null where agent_id=?uuid",
            created.agent_id.as_str(),
        ),
        (
            "update messages set edited_at=current_timestamp where id=?uuid",
            "update messages set edited_at=null where id=?uuid",
            source_id.as_str(),
        ),
        (
            "update messages set deleted_at=current_timestamp where id=?uuid",
            "update messages set deleted_at=null where id=?uuid",
            source_id.as_str(),
        ),
    ] {
        execute(&store, change, &[target]).await;
        assert!(
            service
                .image_request(&second.id, &second.lease_token)
                .await
                .unwrap()
                .is_none()
        );
        service.reserve_picture(&second, &intent).await.unwrap();
        execute(&store, restore, &[target]).await;
    }
    execute(
        &store,
        "update channels set kind='private' where id=?uuid",
        &[&channel],
    )
    .await;
    assert!(
        service
            .image_request(&second.id, &second.lease_token)
            .await
            .unwrap()
            .is_none()
    );
    execute(&store,"insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,0)",&[&channel,&created.agent_id,&owner.to_string()]).await;
    assert!(
        service
            .image_request(&second.id, &second.lease_token)
            .await
            .unwrap()
            .is_some()
    );
    service.reserve_picture(&second, &intent).await.unwrap();
    let second_id = value(
        &store,
        "select image_job_id from agent_image_publications where id=?uuid",
        &[&second.id],
    )
    .await;
    let mut second_job = imagegen.get(&second_id).await.unwrap().unwrap();
    // No generation is authorised after a config revision change, even with a frozen queued job.
    execute(
        &store,
        "update circle_chat_agents set revision=revision+1 where agent_id=?uuid",
        &[&created.agent_id],
    )
    .await;
    second_job.transition("submitting");
    let allowed = match &store {
        Store::Pg(p) => {
            let mut tx = p.begin().await.unwrap();
            let r = submit_postgres(&mut tx, &mut second_job).await.unwrap();
            tx.rollback().await.unwrap();
            r
        }
        Store::Sqlite(p) => {
            let mut tx = p.begin().await.unwrap();
            let r = submit_sqlite(&mut tx, &mut second_job).await.unwrap();
            tx.rollback().await.unwrap();
            r
        }
    };
    assert!(!allowed);
    execute(
        &store,
        "update circle_chat_agents set revision=revision-1 where agent_id=?uuid",
        &[&created.agent_id],
    )
    .await;
    // Two actual reservations exhaust the rolling daily quota; rejected retries do not refund it.
    execute(
        &store,
        "update agent_image_publications set state='failed' where id=?uuid",
        &[&second.id],
    )
    .await;
    second_job.transition("dismissed");
    assert!(imagegen.save(&mut second_job).await.unwrap());
    let third = prepare_candidate(&service, &chat, &channel, &owner, &created.agent_id).await;
    service.reserve_picture(&third, &intent).await.unwrap();
    assert_eq!(
        value(
            &store,
            "select state from agent_image_publications where id=?uuid",
            &[&third.id]
        )
        .await,
        "skipped"
    );
    assert_eq!(
        value(
            &store,
            "select error_code from agent_image_publications where id=?uuid",
            &[&third.id]
        )
        .await,
        "daily_limit"
    );
    execute(
        &store,
        "update circle_chat_agent_jobs set status='skipped' where agent_id=?uuid",
        &[&created.agent_id],
    )
    .await;
}
#[tokio::test]
async fn sqlite_agent_image_publication_contract() {
    let path = std::env::temp_dir().join(format!("sproyt-agent-picture-{}.sqlite", Uuid::now_v7()));
    let url = format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"));
    let repository = crate::db::SqliteChatRepository::connect(&url)
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let pool = SqlitePool::connect(&url).await.unwrap();
    contract(
        Store::Sqlite(pool.clone()),
        Arc::new(repository),
        crate::imagegen::ImageGeneration::test_sqlite_pool(pool.clone()),
    )
    .await;
    pool.close().await;
}
#[tokio::test]
async fn postgres_agent_image_publication_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let repository = crate::db::PostgresChatRepository::connect_with_pool(&url, pool.clone())
        .await
        .unwrap();
    contract(
        Store::Pg(pool.clone()),
        Arc::new(repository),
        crate::imagegen::ImageGeneration::test_postgres_pool(pool),
    )
    .await;
}

#[tokio::test]
async fn picture_intent_is_bounded_target_only_and_rejects_unselected_occasions() {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    let answer = Arc::new(tokio::sync::Mutex::new(
        json!({"mode":"explicit","scene":"At a sunny cafe"}).to_string(),
    ));
    let captured = Arc::new(tokio::sync::Mutex::new(Value::Null));
    let writer = captured.clone();
    let output = answer.clone();
    let app = Router::new()
        .route(
            "/v1/models",
            get(|| async { Json(json!({"data":[{"id":"test-model"}]})) }),
        )
        .route(
            "/v1/chat/completions",
            post(move |Json(input): Json<Value>| {
                let writer = writer.clone();
                let output = output.clone();
                async move {
                    *writer.lock().await = input;
                    Json(json!({"choices":[{"message":{"content":output.lock().await.clone()}}]}))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let model = VllmChat {
        base: format!("http://{address}/v1"),
        key: None,
        http: reqwest::Client::new(),
    };
    let request = PictureRequest {
        id: "01000000-0000-0000-0000-000000000001".into(),
        agent_id: Uuid::now_v7().to_string(),
        channel_id: Uuid::now_v7().to_string(),
        source_body: "Please create a new portrait of yourself.".into(),
        source_sha256: String::new(),
        parent_message_id: None,
        agent_name: "Another configured agent".into(),
        image_generation: json!({"identity_id":"maria-v1","enabled":true,"occasional":true})
            .to_string(),
        identity_id: "maria-v1".into(),
        identity_sha256: crate::imagegen::identity::get("maria-v1").unwrap().sha256(),
        lease_token: Uuid::now_v7().to_string(),
        image_job_id: None,
        mode: None,
        image_revision: None,
    };
    assert_eq!(
        model.picture_intent(&request).await.unwrap().unwrap().mode,
        "explicit"
    );
    let payload = captured.lock().await.clone();
    let user: Value =
        serde_json::from_str(payload["messages"][1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(user["target"], request.source_body);
    assert_eq!(user["occasional_allowed"], false);
    assert!(!user.as_object().unwrap().contains_key("history"));
    *answer.lock().await = json!({"mode":"occasional","scene":"At a sunny cafe"}).to_string();
    assert!(model.picture_intent(&request).await.is_err());
    *answer.lock().await = json!({"mode":"explicit","scene":"x".repeat(501)}).to_string();
    assert!(model.picture_intent(&request).await.is_err());
    *answer.lock().await = json!({"mode":"none","scene":""}).to_string();
    assert!(model.picture_intent(&request).await.unwrap().is_none());
    task.abort();
}

#[tokio::test]
async fn admitted_picture_guidance_is_truthful_and_unavailable_agent_cannot_promise() {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    let captured = Arc::new(tokio::sync::Mutex::new(Value::Null));
    let writer = captured.clone();
    let app=Router::new().route("/v1/models",get(||async{Json(json!({"data":[{"id":"test-model"}]}))})).route("/v1/chat/completions",post(move|Json(input):Json<Value>|{let writer=writer.clone();async move{*writer.lock().await=input;Json(json!({"choices":[{"message":{"content":"Det kan bli eit fint kafébilete!"}}]}))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let model = VllmChat {
        base: format!("http://{address}/v1"),
        key: None,
        http: reqwest::Client::new(),
    };
    let target = Uuid::now_v7().to_string();
    let messages = vec![ContextMessage {
        id: target.clone(),
        author: "Human".into(),
        body: "@Maria vis oss eit nytt bilete av deg ved kaféen".into(),
    }];
    for available in [true, false] {
        model
            .reply_with_capabilities(
                "Maria",
                &[],
                &["Warm and natural.".into()],
                &target,
                &messages,
                None,
                None,
                None,
                None,
                None,
                available,
            )
            .await
            .unwrap();
        let request = captured.lock().await.clone();
        let system = request["messages"][0]["content"].as_str().unwrap();
        let user: Value =
            serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(user["image_request_planned"], available);
        if available {
            assert!(system.contains("separate server-owned image service"));
            assert!(system.contains("never promise a result"));
            assert!(system.contains("cannot choose tools, workflows, URLs"));
        } else {
            assert!(system.contains("No new generated picture has been admitted"));
            assert!(!system.contains("separate server-owned image service"));
        }
    }
    task.abort();
}
