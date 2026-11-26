//! Exercise PostgreSQL's UUID, JSON receipt and timestamp bindings as well as
//! the Heart handoff. The model and GitHub endpoints are local test servers.
use super::*;

async fn until(
    f: &Fixture,
    agents: &CircleChatAgents,
    chat: &ChatEngine,
    source: Uuid,
    expected: &str,
) {
    let Store::Pg(pool) = &f.service.store else {
        unreachable!()
    };
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        // Advance only the test message's retry timers, without bypassing a lease.
        sqlx::query("update feedback_jobs set available_at=0 where source_message_id=$1::uuid and lease_token is null")
            .bind(source.to_string()).execute(pool).await.unwrap();
        sqlx::query("update feedback_intakes set due_at=0 where source_message_id=$1::uuid and lease_token is null")
            .bind(source.to_string()).execute(pool).await.unwrap();
        f.service.admit_feedback().await.unwrap();
        f.service
            .sync_feedback_intakes(Utc::now().timestamp())
            .await
            .unwrap();
        f.service.feedback_tick(agents, chat).await.unwrap();
        let stage: Option<String> = sqlx::query_scalar(
            "select stage from feedback_intakes where source_message_id=$1::uuid",
        )
        .bind(source.to_string())
        .fetch_optional(pool)
        .await
        .unwrap();
        if stage.as_deref() == Some(expected) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "expected {expected}, got {stage:?}"
        );
        tokio::time::sleep(Duration::from_millis(85)).await;
    }
}

#[tokio::test]
#[ignore = "requires isolated loopback PostgreSQL in SPROYT_POSTGRES_TEST_URL and Heart in SPROYT_HEART_INTAKE_TEST_URL"]
async fn postgres_real_heart_drives_manual_and_automatic_intake() {
    let heart = std::env::var("SPROYT_HEART_INTAKE_TEST_URL").unwrap();
    for url in [&heart, &std::env::var("SPROYT_POSTGRES_TEST_URL").unwrap()] {
        let url = reqwest::Url::parse(url).unwrap();
        assert!(
            matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")),
            "requires isolated loopback services"
        );
    }
    let mut f = Fixture::postgres(false).await.unwrap();
    seed_feedback_route(&f).await;
    let Store::Pg(pool) = &f.service.store else {
        unreachable!()
    };
    let pool = pool.clone();
    let remote = Shared::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mock = routes(remote.clone());
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    f.service.heart_url = Some(heart);
    f.service.github = Some(GitHub::test_client(base.clone()));
    // The test client only uses its model transport. Sprøyt domain data and
    // every worker transaction in this test use the PostgreSQL store above.
    let agents = CircleChatAgents::feedback_test_client(
        SqlitePool::connect("sqlite::memory:").await.unwrap(),
        base,
    );
    let chat = ChatEngine::start(Arc::new(InMemoryChatRepository::default()));
    sqlx::query("update channels set circle_id=null,kind='public' where id=$1::uuid")
        .bind(f.channel.to_string())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("update feedback_routes set intake_enabled=true where channel_id=$1::uuid")
        .bind(f.channel.to_string())
        .execute(&pool)
        .await
        .unwrap();
    // An old message enters by intentional activation; replay reuses its run.
    sqlx::query("update messages set body=$1 where id=$2::uuid")
        .bind(REQUEST)
        .bind(f.message.to_string())
        .execute(&pool)
        .await
        .unwrap();
    let reviewer = UserId::from_uuid(f.reviewer);
    for _ in 0..2 {
        assert_eq!(
            f.service
                .feedback_activation(&reviewer, f.channel, f.message, true)
                .await
                .unwrap()
                .as_deref(),
            Some("pending")
        );
    }
    until(&f, &agents, &chat, f.message, "review").await;
    let original = f
        .service
        .feedback_intake(reviewer.clone(), f.channel, f.message)
        .await
        .unwrap()
        .unwrap();
    let task = Uuid::parse_str(original.review_task_id.as_deref().unwrap()).unwrap();
    let request_id = Uuid::now_v7();
    for _ in 0..2 {
        f.service
            .review_feedback(
                reviewer.clone(),
                f.channel,
                f.message,
                FeedbackReview {
                    assessment: "Saka skal følgjast til GitHub lukkar ho.".into(),
                    request_id,
                    task_id: Some(task),
                },
            )
            .await
            .unwrap();
    }
    until(&f, &agents, &chat, f.message, "await-github").await;
    assert_eq!(remote.lock().unwrap().creations, 1);
    remote.lock().unwrap().issue.as_mut().unwrap()["status"] =
        json!({"category":"started","name":"In progress"});
    let second = Uuid::now_v7();
    let sequence:i64=sqlx::query_scalar("update channel_sequences set next_sequence=next_sequence+1 where channel_id=$1::uuid returning next_sequence-1")
        .bind(f.channel.to_string()).fetch_one(&pool).await.unwrap();
    sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values($1::uuid,$2::uuid,$3::uuid,'Tester',$4,$5)")
        .bind(second.to_string()).bind(f.channel.to_string()).bind(f.owner.to_string()).bind(sequence).bind(UNCLEAR).execute(&pool).await.unwrap();
    until(&f, &agents, &chat, second, "clarify").await;
    let second_view = f
        .service
        .feedback_intake(reviewer.clone(), f.channel, second)
        .await
        .unwrap()
        .unwrap();
    assert!(second_view.can_clarify);
    assert_ne!(second_view.heart_instance_id, original.heart_instance_id);
    let clarify = || {
        serde_json::from_value(json!({"task_id":second_view.clarification_task_id,"decision":"dismiss","note":"Behovet er allereie dekt."})).unwrap()
    };
    assert!(
        f.service
            .clarify_feedback(UserId::from_uuid(f.outsider), f.channel, second, clarify())
            .await
            .is_err()
    );
    for _ in 0..2 {
        f.service
            .clarify_feedback(reviewer.clone(), f.channel, second, clarify())
            .await
            .unwrap();
    }
    until(&f, &agents, &chat, second, "ignored").await;
    {
        let mut r = remote.lock().unwrap();
        let issue = r.issue.as_mut().unwrap();
        issue["state"] = json!("closed");
        issue["state_reason"] = json!("not_planned");
    }
    f.service.wake_feedback_issue(7, 42, 700).await.unwrap();
    until(&f, &agents, &chat, f.message, "rejected").await;
    let ended = f
        .service
        .feedback_intake(reviewer, f.channel, f.message)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ended.heart_instance_id, original.heart_instance_id);
    assert_eq!(remote.lock().unwrap().creations, 1);
    let audit:(i64,i64,i64)=sqlx::query_as("select (select count(*) from feedback_intake_results where source_message_id=$1::uuid and node_id='review'),
        (select count(*) from feedback_intake_results where source_message_id=$2::uuid and node_id='clarify'),
        (select count(*) from feedback_heart where source_message_id=$1::uuid or source_message_id=$2::uuid)")
        .bind(f.message.to_string()).bind(second.to_string()).fetch_one(&pool).await.unwrap();
    assert_eq!(audit, (1, 1, 0));
    server.abort();
}
