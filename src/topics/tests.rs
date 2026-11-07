use super::*;

async fn execute(store: &TopicStore, query: &str, params: &[&str]) {
    with_pool!(store, |pool, pg| {
        let query = sql(query, pg);
        let mut statement = sqlx::query(&query);
        for value in params {
            statement = statement.bind(*value);
        }
        statement.execute(pool).await.unwrap();
    });
}

async fn channel(store: &TopicStore, actor: &UserId) -> ChannelId {
    let channel = ChannelId::from_uuid(Uuid::now_v7());
    execute(
        store,
        "insert into channels(id,slug,name,kind,created_by) values(?uuid,?,'Test','private',?uuid)",
        &[
            &channel.to_string(),
            &channel.to_string(),
            &actor.to_string(),
        ],
    )
    .await;
    execute(
        store,
        "insert into channel_sequences(channel_id) values(?uuid)",
        &[&channel.to_string()],
    )
    .await;
    execute(
        store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'owner')",
        &[&channel.to_string(), &actor.to_string()],
    )
    .await;
    channel
}

async fn user(store: &TopicStore) -> UserId {
    let id = UserId::from_uuid(Uuid::now_v7());
    execute(
        store,
        "insert into users(id,kind,display_name) values(?uuid,'human','Test')",
        &[&id.to_string()],
    )
    .await;
    id
}

async fn message(
    store: &TopicStore,
    channel: &ChannelId,
    actor: &UserId,
    sequence: i64,
    body: &str,
) -> String {
    let message = Uuid::now_v7().to_string();
    with_pool!(store, |pool, pg| {
        sqlx::query(&sql("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Test',?int,?)",pg))
            .bind(&message).bind(channel.to_string()).bind(actor.to_string()).bind(sequence).bind(body)
            .execute(pool).await.unwrap();
    });
    message
}

async fn drain(store: &TopicStore, keyword: &str) {
    for _ in 0..100 {
        let Some(job) = store.claim().await.unwrap() else {
            return;
        };
        let input = store.load_input(&job).await.unwrap().unwrap();
        let images = input
            .images
            .into_iter()
            .map(|image| (image.id, vec![keyword.to_owned()]))
            .collect::<Vec<_>>();
        store
            .complete(&job, &[keyword.to_owned()], &images)
            .await
            .unwrap();
    }
    panic!("job queue did not drain");
}

async fn topics_contract(store: &TopicStore) {
    let owner = user(store).await;
    let viewer = user(store).await;
    let source_author = user(store).await;
    let shared = channel(store, &source_author).await;
    let personal = channel(store, &owner).await;
    execute(
        store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'observer')",
        &[&shared.to_string(), &owner.to_string()],
    )
    .await;
    execute(
        store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'observer')",
        &[&personal.to_string(), &viewer.to_string()],
    )
    .await;
    execute(
        store,
        "insert into personal_spaces(owner_id,channel_id) values(?uuid,?uuid)",
        &[&owner.to_string(), &personal.to_string()],
    )
    .await;
    assert!(!store.summary(&owner, &shared).await.unwrap().enabled);
    assert!(store.summary(&owner, &personal).await.unwrap().enabled);
    assert!(!store.summary(&viewer, &personal).await.unwrap().can_manage);
    assert!(matches!(
        store.set_enabled(&viewer, &personal, false).await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(matches!(
        store.summary(&source_author, &personal).await,
        Err(RepositoryError::PermissionDenied)
    ));

    let source = message(
        store,
        &shared,
        &source_author,
        1,
        "Original bird observation",
    )
    .await;
    execute(
        store,
        "update messages set sender_display_name='Source author' where id=?uuid",
        &[&source],
    )
    .await;
    let saved_body = crate::domain::personal_link_body(
        &shared,
        MessageId::from_uuid(Uuid::parse_str(&source).unwrap()),
        ChannelSequence::new(1),
        None,
    );
    let bookmark = message(store, &personal, &owner, 1, saved_body.as_str()).await;
    execute(
        store, "insert into personal_saved_links(id,owner_id,source_channel_id,source_message_id,log_message_id,saved_at)
                 values(?uuid,?uuid,?uuid,?uuid,?uuid,current_timestamp)",
        &[&Uuid::now_v7().to_string(),&owner.to_string(),&shared.to_string(),&source,&bookmark],
    ).await;

    let job = store.claim().await.unwrap().unwrap();
    assert_eq!(job.message_id, bookmark);
    let input = store.load_input(&job).await.unwrap().unwrap();
    assert_eq!(input.body, "Original bird observation");
    store
        .complete(&job, &["Birds".into(), "birds".into()], &[])
        .await
        .unwrap();
    assert_eq!(
        store.summary(&owner, &personal).await.unwrap().topics[0].count,
        1
    );
    assert!(
        store
            .summary(&viewer, &personal)
            .await
            .unwrap()
            .topics
            .is_empty()
    );
    assert!(
        store
            .matches(&viewer, &personal, "birds", i64::MAX)
            .await
            .unwrap()
            .matches
            .is_empty()
    );

    // A saved source's label must stay with the saved entry. Reusing that
    // label on another message would bypass the original source's access check.
    let media_id = MediaId::generate();
    let media_body = format!(
        "{}
[[media:{media_id}|image/jpeg|boat.jpg]]",
        "a".repeat(1200)
    );
    let ordinary = message(store, &personal, &owner, 2, &media_body).await;
    let ordinary_job = store.claim_due(false).await.unwrap().unwrap();
    assert_eq!(ordinary_job.message_id, ordinary);
    assert!(
        store
            .load_input(&ordinary_job)
            .await
            .unwrap()
            .unwrap()
            .existing_labels
            .is_empty()
    );
    store
        .complete(&ordinary_job, &["Boats".into()], &[])
        .await
        .unwrap();
    let media_results = store
        .matches(&owner, &personal, "boats", i64::MAX)
        .await
        .unwrap();
    let media_match = &media_results.matches[0];
    assert_eq!(media_match.preview.body.len(), 1200);
    assert_eq!(media_match.message.body.as_str(), media_body);
    let media_json = serde_json::to_value(media_match).unwrap();
    assert_eq!(media_json["message"]["body"], media_body);
    let probe = message(store, &personal, &owner, 3, "More notes").await;
    let probe_job = store.claim_due(false).await.unwrap().unwrap();
    assert_eq!(probe_job.message_id, probe);
    assert_eq!(
        store
            .load_input(&probe_job)
            .await
            .unwrap()
            .unwrap()
            .existing_labels,
        ["Boats"]
    );
    execute(
        store,
        "delete from messages where id in (?uuid,?uuid)",
        &[&ordinary, &probe],
    )
    .await;

    let results = store
        .matches(&owner, &personal, "birds", i64::MAX)
        .await
        .unwrap();
    let saved_match = &results.matches[0];
    assert_eq!(saved_match.preview.message_id, bookmark);
    assert_eq!(saved_match.preview.body, "Original bird observation");
    assert_eq!(saved_match.preview.sender_display_name, "Source author");
    assert_eq!(saved_match.message.channel_id, personal);
    assert_eq!(saved_match.message.sender_id, owner);
    assert_eq!(saved_match.message.body, saved_body);
    let saved_json = serde_json::to_value(saved_match).unwrap();
    assert_eq!(saved_json["message_id"], bookmark);
    assert_eq!(saved_json["body"], "Original bird observation");
    assert_eq!(saved_json["message"]["id"], bookmark);
    assert_eq!(saved_json["message"]["body"], saved_body.as_str());

    // Source membership removal immediately hides labels, counts and matches,
    // including from an old link already indexed before removal.
    execute(
        store,
        "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
        &[&shared.to_string(), &owner.to_string()],
    )
    .await;
    assert!(
        store
            .summary(&owner, &personal)
            .await
            .unwrap()
            .topics
            .is_empty()
    );
    assert!(
        store
            .matches(&owner, &personal, "birds", i64::MAX)
            .await
            .unwrap()
            .matches
            .is_empty()
    );
    execute(
        store,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'observer')",
        &[&shared.to_string(), &owner.to_string()],
    )
    .await;
    assert_eq!(
        store
            .matches(&owner, &personal, "birds", i64::MAX)
            .await
            .unwrap()
            .matches[0]
            .preview
            .message_id,
        bookmark
    );

    execute(
        store,
        "update messages set body='A different bird' where id=?uuid",
        &[&source],
    )
    .await;
    assert!(
        store
            .summary(&owner, &personal)
            .await
            .unwrap()
            .topics
            .is_empty()
    );
    let obsolete = store.claim().await.unwrap().unwrap();
    assert_eq!(
        store.load_input(&obsolete).await.unwrap().unwrap().body,
        "A different bird"
    );
    execute(
        store,
        "update messages set body='Fishing' where id=?uuid",
        &[&source],
    )
    .await;
    assert!(!store.job_current(&obsolete).await.unwrap());
    store
        .complete(&obsolete, &["obsolete".into()], &[])
        .await
        .unwrap();
    let current = store.claim().await.unwrap().unwrap();
    assert_eq!(
        store.load_input(&current).await.unwrap().unwrap().body,
        "Fishing"
    );
    store
        .complete(&current, &["Fishing".into()], &[])
        .await
        .unwrap();
    assert_eq!(
        store.summary(&owner, &personal).await.unwrap().topics[0].key,
        "fishing"
    );

    // Channel disablement suppresses the index and pauses work. An edit
    // during that interval is still picked up on re-enable.
    store.set_enabled(&owner, &personal, false).await.unwrap();
    execute(
        store,
        "update messages set body='Rowing' where id=?uuid",
        &[&source],
    )
    .await;
    assert!(store.claim().await.unwrap().is_none());
    store.set_enabled(&owner, &personal, true).await.unwrap();
    drain(store, "Rowing").await;
    assert_eq!(
        store.summary(&owner, &personal).await.unwrap().topics[0].key,
        "rowing"
    );
    execute(
        store,
        "update messages set deleted_at=current_timestamp where id=?uuid",
        &[&source],
    )
    .await;
    assert!(
        store
            .summary(&owner, &personal)
            .await
            .unwrap()
            .topics
            .is_empty()
    );
    assert!(
        store
            .matches(&owner, &personal, "rowing", i64::MAX)
            .await
            .unwrap()
            .matches
            .is_empty()
    );

    // Enable backfill for a channel with existing history and paginate by
    // sequence, with no dependence on the currently visible conversation.
    for sequence in 2..=32 {
        message(store, &shared, &source_author, sequence, "Coastal weather").await;
    }
    store
        .set_enabled(&source_author, &shared, true)
        .await
        .unwrap();
    drain(store, "Weather").await;
    let first = store
        .matches(&source_author, &shared, "weather", i64::MAX)
        .await
        .unwrap();
    assert_eq!(first.matches.len(), 30);
    assert_eq!(first.matches[0].preview.sequence, 32);
    assert_eq!(first.next_before, Some(3));
    let second = store
        .matches(
            &source_author,
            &shared,
            "weather",
            first.next_before.unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].preview.sequence, 2);
    assert_eq!(second.next_before, None);
}

async fn leases_and_fanout(store: &TopicStore) {
    let owner = user(store).await;
    let target = channel(store, &owner).await;
    store.set_enabled(&owner, &target, true).await.unwrap();
    let id = message(store, &target, &owner, 1, "Lease test").await;
    let expired = store.claim_due(false).await.unwrap().unwrap();
    assert_eq!(expired.message_id, id);
    execute(
        store,
        "update channel_topic_jobs set due_at=0 where message_id=?uuid",
        &[&id],
    )
    .await;
    let current = store.claim_due(false).await.unwrap().unwrap();
    assert_eq!(current.message_id, id);
    assert_ne!(expired.lease, current.lease);
    store
        .complete(&expired, &["Obsolete".into()], &[])
        .await
        .unwrap();
    assert!(
        store
            .summary(&owner, &target)
            .await
            .unwrap()
            .topics
            .is_empty()
    );
    store
        .complete(
            &current,
            &[
                "Alpha".into(),
                "Beta".into(),
                "Gamma".into(),
                "Delta".into(),
                "Epsilon".into(),
            ],
            &[],
        )
        .await
        .unwrap();
    message(store, &target, &owner, 2, "More terms").await;
    let second = store.claim_due(false).await.unwrap().unwrap();
    store
        .complete(
            &second,
            &["Alpha".into(), "Zeta".into(), "Eta".into(), "Theta".into()],
            &[],
        )
        .await
        .unwrap();
    let pills = store.summary(&owner, &target).await.unwrap().topics;
    assert_eq!(pills.len(), 6);
    assert_eq!(pills[0].key, "alpha");
    assert_eq!(pills[0].count, 2);

    let shared = channel(store, &owner).await;
    let source = message(store, &shared, &owner, 1, "Shared source").await;
    let mut viewers = Vec::new();
    for _ in 0..70 {
        let viewer = user(store).await;
        let personal = channel(store, &viewer).await;
        execute(
            store,
            "insert into personal_spaces(owner_id,channel_id) values(?uuid,?uuid)",
            &[&viewer.to_string(), &personal.to_string()],
        )
        .await;
        execute(store,"insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'observer')",
            &[&shared.to_string(),&viewer.to_string()]).await;
        let log = message(store, &personal, &viewer, 1, "Link").await;
        execute(store,
            "insert into personal_saved_links(id,owner_id,source_channel_id,source_message_id,log_message_id,saved_at)
             values(?uuid,?uuid,?uuid,?uuid,?uuid,current_timestamp)",
            &[&Uuid::now_v7().to_string(),&viewer.to_string(),&shared.to_string(),&source,&log]).await;
        viewers.push((viewer, personal));
    }
    for _ in &viewers {
        let job = store.claim_due(false).await.unwrap().unwrap();
        store
            .complete(&job, &["Original".into()], &[])
            .await
            .unwrap();
    }
    let (viewer, personal) = &viewers[0];
    assert_eq!(
        store.summary(viewer, personal).await.unwrap().topics.len(),
        1
    );
    execute(store, "delete from messages where id=?uuid", &[&source]).await;
    assert!(
        store
            .summary(viewer, personal)
            .await
            .unwrap()
            .topics
            .is_empty()
    );

    // Only one saved-link batch is physically cleared per worker tick.
    let count_stale = || async {
        with_pool!(store, |pool, pg| {
            sqlx::query_scalar::<_, i64>(&sql(
                "select count(*) from channel_topic_evidence where topic_key='original'",
                pg,
            ))
            .fetch_one(pool)
            .await
            .unwrap()
        })
    };
    assert_eq!(count_stale().await, 70);
    store.refresh_saved_sources().await.unwrap();
    assert_eq!(count_stale().await, 6);
    store.refresh_saved_sources().await.unwrap();
    assert_eq!(count_stale().await, 0);
}

#[tokio::test]
async fn sqlite_topics_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    let store = TopicStore::Sqlite(pool);
    topics_contract(&store).await;
    leases_and_fanout(&store).await;
}

#[tokio::test]
async fn postgres_topics_contract_and_preview_race() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("topics_{}", Uuid::now_v7().simple());
    sqlx::query(&format!("create schema {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("set search_path to {schema}");
    let pool = sqlx::postgres::PgPoolOptions::new()
        .after_connect(move |conn, _| {
            let search_path = search_path.clone();
            Box::pin(async move {
                sqlx::query(&search_path).execute(conn).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let store = TopicStore::Postgres(pool.clone());
    topics_contract(&store).await;
    leases_and_fanout(&store).await;

    // Publish a preview while the attachment transaction is still uncommitted.
    // The worker could also run before that attachment is visible.
    let owner = user(&store).await;
    let channel = channel(&store, &owner).await;
    store.set_enabled(&owner, &channel, true).await.unwrap();
    let message = message(&store, &channel, &owner, 1, "An attached image").await;
    drain(&store, "Earlier text").await;
    let media_id = MediaId::generate();
    execute(&store,
        "insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at)
         values(?uuid,?uuid,?uuid,?,'photo.jpg','image/jpeg',3,'hash',current_timestamp)",
        &[&media_id.to_string(),&owner.to_string(),&channel.to_string(),&media_id.to_string()]
    ).await;
    let mut attachment = pool.begin().await.unwrap();
    sqlx::query("insert into message_attachments(message_id,media_id,position) values($1,$2,0)")
        .bind(Uuid::parse_str(&message).unwrap())
        .bind(*media_id.as_uuid())
        .execute(&mut *attachment)
        .await
        .unwrap();
    sqlx::query("insert into media_variants(media_id,variant,content_type,size_bytes,width,height,content,created_at)
                 values($1,'preview','image/jpeg',3,1,1,$2,current_timestamp)")
        .bind(*media_id.as_uuid()).bind(vec![1_u8,2,3])
        .execute(&pool).await.unwrap();
    store.refresh_media().await.unwrap();
    attachment.commit().await.unwrap();

    let job = store.claim().await.unwrap().unwrap();
    let input = store.load_input(&job).await.unwrap().unwrap();
    assert_eq!(input.images[0].id, media_id);
    assert_eq!(input.images[0].bytes, vec![1, 2, 3]);
    store
        .complete(
            &job,
            &["Nature".into()],
            &[(media_id, vec!["Nature".into()])],
        )
        .await
        .unwrap();
    assert_eq!(
        store.summary(&owner, &channel).await.unwrap().topics[0].count,
        1
    );
    let origin = sqlx::query_scalar::<_, String>(
        "select origin_id from channel_topic_evidence where kind='image'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(origin, media_id.to_string());

    pool.close().await;
    sqlx::query(&format!("drop schema {schema} cascade"))
        .execute(&admin)
        .await
        .unwrap();
}
