use super::*;
use crate::{chat::ChatEngine, db::SqliteChatRepository, domain::MessageLimit};
use axum::{
    Json, Router,
    routing::{get, post},
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn search_publishes_for_two_users_and_followup_roll_needs_no_model_or_provider() {
    let database = format!("sqlite://./target/shared-place-{}.sqlite", Uuid::now_v7());
    let repository = SqliteChatRepository::connect(&database).await.unwrap();
    repository.migrate().await.unwrap();
    let pool = SqlitePool::connect(&database).await.unwrap();
    let (owner, other, circle, channel) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    for id in [owner, other] {
        sqlx::query("insert into users(id,kind,display_name) values(?,'human','Test')")
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "insert into circles(id,slug,name,created_by) values(?,'shared-bars','Shared bars',?)",
    )
    .bind(circle.to_string())
    .bind(owner.to_string())
    .execute(&pool)
    .await
    .unwrap();
    for id in [owner, other] {
        sqlx::query("insert into circle_memberships(circle_id,user_id,role) values(?,?,'owner')")
            .bind(circle.to_string())
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values(?,'shared-bars','Shared bars','public',?,?)").bind(channel.to_string()).bind(owner.to_string()).bind(circle.to_string()).execute(&pool).await.unwrap();
    for id in [owner, other] {
        sqlx::query("insert into channel_memberships(channel_id,user_id,role) values(?,?,'owner')")
            .bind(channel.to_string())
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
    }
    sqlx::query("insert into channel_sequences(channel_id) values(?)")
        .bind(channel.to_string())
        .execute(&pool)
        .await
        .unwrap();
    let model_calls = Arc::new(AtomicUsize::new(0));
    let seen = model_calls.clone();
    let model_app=Router::new().route("/v1/models",get(|| async {Json(json!({"data":[{"id":"qwen-test"}]}))})).route("/v1/chat/completions",post(move || { let seen=seen.clone(); async move {
        seen.fetch_add(1,Ordering::SeqCst);
        Json(json!({"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"search-1","type":"function","function":{"name":"find_bars","arguments":r#"{"query":"Bergen","origin_mode":"none"}"#}}]}}]}))
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let model_server = tokio::spawn(async move { axum::serve(listener, model_app).await.unwrap() });
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let seen = provider_calls.clone();
    let provider_app=Router::new().route("/mcp",post(move |Json(request):Json<Value>| {let seen=seen.clone(); async move {
        assert_eq!(request["params"]["name"],"search_places"); seen.fetch_add(1,Ordering::SeqCst);
        Json(json!({"jsonrpc":"2.0","id":1,"result":{"structuredContent":{
            "summary":"To pubar i Bergen [0] [1]","contentToken":"NEVER-PERSIST-RAW-TOKEN","places":[
                {"id":"secret-a","attribution":{"title":"Pub A","url":"https://www.google.com/maps?cid=1"},"googleMapsLinks":{"placeUrl":"https://www.google.com/maps?cid=1"}},
                {"id":"secret-b","attribution":{"title":"Pub B","url":"https://www.google.com/maps?cid=2"},"googleMapsLinks":{"placeUrl":"https://www.google.com/maps?cid=2"}}
            ]}}}))
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let provider_address = listener.local_addr().unwrap();
    let provider_server =
        tokio::spawn(async move { axum::serve(listener, provider_app).await.unwrap() });
    let provider = maps::MapsService::new(
        "local-test-key-123456",
        Url::parse(&format!("http://{provider_address}/mcp")).unwrap(),
    )
    .unwrap();
    let service = CircleChatAgents {
        store: Store::Sqlite(pool.clone()),
        model: Some(Arc::new(VllmChat {
            base: format!("http://{address}/v1"),
            key: None,
            http: reqwest::Client::new(),
        })),
        worker_enabled: false,
        weather: None,
        sea_temperature: None,
        ferry: None,
        observations: None,
        imagegen: None,
    };
    let actor = UserId::from_uuid(owner);
    let agent = service
        .create(
            &actor,
            &circle.to_string(),
            AgentInput {
                display_name: "Bars".into(),
                trigger_words: vec!["bars".into()],
                response_phrases: vec!["Hjelper med barar".into()],
                enabled: false,
                revision: None,
                memory_enabled: None,
                vision_enabled: None,
                image_generation: None,
                weather: None,
                sea_temperature: None,
                ferry_port: None,
            },
        )
        .await
        .unwrap();
    sqlx::query("update circle_chat_agents set enabled=1 where agent_id=?")
        .bind(&agent.agent_id)
        .execute(&pool)
        .await
        .unwrap();
    let chat = ChatEngine::start(Arc::new(repository));
    let channel_id = ChannelId::from_uuid(channel);
    place_lookup::TEST_ENABLED.scope(true,maps::TEST_SERVICE.scope(provider,async {
        chat.send_message(channel_id.clone(),actor.clone(),MessageBody::new("@Bars finn barar i Bergen").unwrap()).await.unwrap();
        let job=service.claim().await.unwrap().unwrap();
        service.process_inner(&job,&chat).await.unwrap();
        assert_eq!(model_calls.load(Ordering::SeqCst),1); assert_eq!(provider_calls.load(Ordering::SeqCst),1);
        for user in [owner,other] {
            let messages=chat.load_messages(UserId::from_uuid(user),channel_id.clone(),MessageLimit::new(20),None,None).await.unwrap();
            assert!(messages.iter().any(|m| m.body.as_str().contains("| 1 | [Pub A]")));
        }
        let (body,plan,status):(String,Option<String>,String)=sqlx::query_as("select reply_body,place_lookup_plan,status from circle_chat_agent_jobs where id=?").bind(&job.id).fetch_one(&pool).await.unwrap();
        assert!(plan.is_none()); assert_eq!(status,"completed"); assert!(!body.contains("NEVER-PERSIST")); assert!(!body.contains("secret-a"));
        // Trigger directly, so this test is independent of the followup feature flag.
        chat.send_message(channel_id.clone(),actor,MessageBody::new("@Bars kan du trille terningen og velje for meg?").unwrap()).await.unwrap();
        let next=service.claim().await.unwrap().unwrap(); service.process_inner(&next,&chat).await.unwrap();
        assert_eq!(model_calls.load(Ordering::SeqCst),1); assert_eq!(provider_calls.load(Ordering::SeqCst),1);
        let selection:String=sqlx::query_scalar("select reply_body from circle_chat_agent_jobs where id=?").bind(&next.id).fetch_one(&pool).await.unwrap();
        assert!(selection.contains("🎲 Terningen gav")); assert!(selection.contains("av 2:"));
        // Re-publication uses the persisted result and receipt; never a new roll.
        sqlx::query("update circle_chat_agent_jobs set status='leased',lease_token=?,leased_until=? where id=?")
            .bind(&next.lease_token).bind(Utc::now().timestamp()+180).bind(&next.id).execute(&pool).await.unwrap();
        let cached=Job {reply_body:Some(selection.clone()),..next};
        assert!(service.process_inner(&cached,&chat).await.is_ok());
        let count:i64=sqlx::query_scalar("select count(*) from messages where sender_id=? and body=?").bind(&agent.agent_id).bind(&selection).fetch_one(&pool).await.unwrap(); assert_eq!(count,1);
    })).await;
    model_server.abort();
    provider_server.abort();
}
