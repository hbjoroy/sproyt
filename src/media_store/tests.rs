use super::*;

#[tokio::test]
async fn postgres_monitoring_aggregates_eligibility_and_charged_quota() {
    super::monitoring::tests::postgres_contract().await;
}
use crate::domain::{
    ChannelKind, ChannelSlug, ChatRepository, CreateChannel, DisplayName, LeaveChannel,
    PrincipalKind, User,
};

fn limits() -> UploadLimits {
    UploadLimits {
        max_file_bytes: 4 * CHUNK_BYTES as u64,
        owner_bytes: 128 * CHUNK_BYTES as u64,
        total_bytes: 1024 * 1024 * 1024,
    }
}
async fn owner<R: ChatRepository>(repository: &R) -> (UserId, Uuid) {
    let id = UserId::from_uuid(Uuid::now_v7());
    repository
        .upsert_user(User {
            id: id.clone(),
            kind: PrincipalKind::Human,
            display_name: DisplayName::new("Upload tester").unwrap(),
            handle: None,
            external_provider: None,
            external_subject: None,
            created_at: chrono::Utc::now(),
        })
        .await
        .unwrap();
    let channel = repository
        .create_channel(CreateChannel {
            actor: id.clone(),
            slug: ChannelSlug::new(format!("uploads-{}", Uuid::now_v7().simple())).unwrap(),
            name: DisplayName::new("Uploads").unwrap(),
            kind: ChannelKind::Private,
            circle_id: None,
        })
        .await
        .unwrap();
    (id, *channel.id.as_uuid())
}
fn input(size: u64) -> NewUpload {
    NewUpload {
        request_id: Uuid::now_v7(),
        filename: "original.jpg".into(),
        content_type: "image/jpeg".into(),
        size_bytes: size,
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn contract<R: ChatRepository>(repository: &R, store: &MediaStore) {
    let (actor, channel) = owner(repository).await;
    let mut bytes: Vec<u8> = (0..CHUNK_BYTES + 13).map(|i| (i % 251) as u8).collect();
    bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    let request = input(bytes.len() as u64);
    let upload = store.create(&actor, channel, &request).await.unwrap();
    assert_eq!(
        store.create(&actor, channel, &request).await.unwrap().id,
        upload.id
    );
    let mut conflict = request.clone();
    conflict.filename = "other.jpg".into();
    assert!(matches!(
        store.create(&actor, channel, &conflict).await,
        Err(UploadError::Conflict)
    ));
    assert!(matches!(
        store.seal(&actor, upload.id).await,
        Err(UploadError::Conflict)
    ));
    assert!(matches!(
        store
            .put_chunk(
                &actor,
                upload.id,
                1,
                &bytes[CHUNK_BYTES..],
                &hash(&bytes[CHUNK_BYTES..])
            )
            .await,
        Err(UploadError::Conflict)
    ));
    assert!(matches!(
        store
            .put_chunk(&actor, upload.id, 0, &bytes[..CHUNK_BYTES], "wrong")
            .await,
        Err(UploadError::Invalid)
    ));
    let chunk_hash = hash(&bytes[..CHUNK_BYTES]);
    let first = store
        .put_chunk(&actor, upload.id, 0, &bytes[..CHUNK_BYTES], &chunk_hash)
        .await
        .unwrap();
    assert_eq!(first.next_chunk, 1);
    assert_eq!(
        store
            .put_chunk(&actor, upload.id, 0, &bytes[..CHUNK_BYTES], &chunk_hash)
            .await
            .unwrap()
            .received_bytes,
        CHUNK_BYTES as u64
    );
    let mut different = bytes[..CHUNK_BYTES].to_vec();
    different[9] ^= 1;
    assert!(matches!(
        store
            .put_chunk(&actor, upload.id, 0, &different, &hash(&different))
            .await,
        Err(UploadError::Conflict)
    ));
    let stranger = UserId::from_uuid(Uuid::now_v7());
    assert!(matches!(
        store.status(&stranger, upload.id).await,
        Err(UploadError::NotFound)
    ));
    assert!(matches!(
        store
            .put_chunk(&stranger, upload.id, 0, &bytes[..CHUNK_BYTES], &chunk_hash)
            .await,
        Err(UploadError::NotFound)
    ));
    store
        .put_chunk(
            &actor,
            upload.id,
            1,
            &bytes[CHUNK_BYTES..],
            &hash(&bytes[CHUNK_BYTES..]),
        )
        .await
        .unwrap();
    // Create retries must not deadlock against finalization's membership locks.
    let (retry, completed) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            store.create(&actor, channel, &request),
            store.complete(&actor, upload.id)
        )
    })
    .await
    .unwrap();
    retry.unwrap();
    let media = completed.unwrap();
    assert_eq!(store.complete(&actor, upload.id).await.unwrap(), media);
    assert_eq!(
        store.status(&actor, upload.id).await.unwrap().state,
        "complete"
    );
    assert!(matches!(
        store.abort(&actor, upload.id).await,
        Err(UploadError::Conflict)
    ));
    let metadata = repository
        .media_metadata(actor.clone(), media)
        .await
        .unwrap();
    assert_eq!(metadata.sha256, hash(&bytes));
    assert_eq!(metadata.size_bytes, bytes.len() as u64);
    assert_eq!(metadata.content_type, "image/jpeg");
    let mut actual = repository
        .read_media_slice(actor.clone(), media, 0, CHUNK_BYTES)
        .await
        .unwrap();
    actual.extend(
        repository
            .read_media_slice(actor.clone(), media, CHUNK_BYTES as u64, CHUNK_BYTES)
            .await
            .unwrap(),
    );
    assert_eq!(actual, bytes);
    assert_eq!(
        repository
            .read_media_slice(actor.clone(), media, CHUNK_BYTES as u64 - 5, 16)
            .await
            .unwrap(),
        bytes[CHUNK_BYTES - 5..CHUNK_BYTES + 11]
    );
    assert!(
        repository
            .read_media_slice(actor.clone(), media, bytes.len() as u64, 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        repository.read_media_slice(stranger, media, 0, 1).await,
        Err(RepositoryError::NotFound)
    );
    let job_count = with_store!(store, |pool, pg| {
        sqlx::query_scalar::<_, i64>(&sql(
            "select count(*) from media_preview_jobs where media_id=?uuid",
            pg,
        ))
        .bind(media.to_string())
        .fetch_one(pool)
        .await
        .unwrap()
    });
    assert_eq!(
        job_count, 1,
        "repeated completion creates one durable preview job"
    );
    let active = store.create(&actor, channel, &input(1)).await.unwrap();
    // A process killed by OOM is reclaimed, but cannot publish with its old token.
    let first = store.claim_preview().await.unwrap().unwrap();
    assert_eq!(
        repository
            .media_preview_state(actor.clone(), media)
            .await
            .unwrap(),
        "running"
    );
    assert_eq!(first.media_id, *media.as_uuid());
    assert!(store.claim_preview().await.unwrap().is_none());
    assert_eq!(
        store.preview_chunk(&first, 0).await.unwrap(),
        bytes[..CHUNK_BYTES]
    );
    with_store!(store, |pool, pg| {
        sqlx::query(&sql(
            "update media_preview_jobs set lease_until=0 where media_id=?uuid",
            pg,
        ))
        .bind(media.to_string())
        .execute(pool)
        .await
        .unwrap();
    });
    assert!(matches!(
        store.publish_preview(&first, vec![1], 1, 1).await,
        Err(UploadError::Conflict)
    ));
    let mut replacement = store.claim_preview().await.unwrap().unwrap();
    assert_ne!(replacement.token, first.token);
    assert!(matches!(
        store.preview_chunk(&first, 0).await,
        Err(UploadError::Conflict)
    ));
    store.fail_preview(&first).await.unwrap();
    assert_eq!(
        store.preview_chunk(&replacement, 1).await.unwrap(),
        bytes[CHUNK_BYTES..]
    );
    with_store!(store, |pool, pg| {
        if pg {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query(&sql(
                "update media_preview_jobs set lease_until=? where media_id=?uuid",
                pg,
            ))
            .bind(chrono::Utc::now().timestamp() + 2)
            .bind(media.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
            let blocked_store = store.clone();
            let blocked_job = replacement.clone();
            let blocked = tokio::spawn(async move {
                blocked_store
                    .publish_preview(&blocked_job, vec![9], 1, 1)
                    .await
            });
            tokio::time::sleep(Duration::from_secs(3)).await;
            assert!(!blocked.is_finished());
            tx.commit().await.unwrap();
            assert!(matches!(blocked.await.unwrap(), Err(UploadError::Conflict)));
            replacement = store.claim_preview().await.unwrap().unwrap();
        }
    });
    store
        .publish_preview(&replacement, vec![1, 2, 3], 10, 20)
        .await
        .unwrap();
    assert!(store.claim_preview().await.unwrap().is_none());
    assert!(matches!(
        store.publish_preview(&first, vec![4], 1, 1).await,
        Err(UploadError::Conflict)
    ));
    let preview = repository
        .load_media_preview(actor.clone(), media)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preview.content, vec![1, 2, 3]);
    assert_eq!(
        repository
            .media_preview_state(actor.clone(), media)
            .await
            .unwrap(),
        "ready"
    );
    repository
        .leave_channel(LeaveChannel {
            actor: actor.clone(),
            channel_id: crate::domain::ChannelId::from_uuid(channel),
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .put_chunk(&actor, active.id, 0, &[1], &hash(&[1]))
            .await,
        Err(UploadError::NotFound)
    ));
    assert!(matches!(
        store.complete(&actor, active.id).await,
        Err(UploadError::NotFound)
    ));
    assert_eq!(
        repository.read_media_slice(actor, media, 0, 1).await,
        Err(RepositoryError::NotFound)
    );

    let (actor, channel) = owner(repository).await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let store = store.clone();
        let actor = actor.clone();
        tasks.spawn(async move { store.create(&actor, channel, &input(1)).await });
    }
    let mut sessions = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Ok(status) => sessions.push(status),
            Err(UploadError::Quota) => {}
            Err(error) => panic!("{error:?}"),
        }
    }
    assert_eq!(sessions.len(), 8, "atomic per-owner active-session limit");
    for session in sessions {
        store.abort(&actor, session.id).await.unwrap();
    }
    let expired = store.create(&actor, channel, &input(1)).await.unwrap();
    with_store!(store, |pool, pg| {
        sqlx::query(&sql(
            "update media_uploads set created_at=?,expires_at=? where id=?uuid",
            pg,
        ))
        .bind(chrono::Utc::now().timestamp() - UPLOAD_TTL - 10)
        .bind(chrono::Utc::now().timestamp() - 1)
        .bind(expired.id.to_string())
        .execute(pool)
        .await
        .unwrap();
    });
    assert!(matches!(
        store.status(&actor, expired.id).await,
        Err(UploadError::NotFound)
    ));
    assert!(store.cleanup_expired().await.unwrap() >= 1);

    // Competing byte reservations must not both fit into a four-MiB owner budget.
    let mut limited = store.clone();
    limited.limits.owner_bytes = 4 * CHUNK_BYTES as u64;
    let a = input(3 * CHUNK_BYTES as u64);
    let b = input(3 * CHUNK_BYTES as u64);
    let (a, b) = tokio::join!(
        limited.create(&actor, channel, &a),
        limited.create(&actor, channel, &b)
    );
    let accepted = match (a, b) {
        (Ok(a), Err(UploadError::Quota)) | (Err(UploadError::Quota), Ok(a)) => a,
        other => panic!("bad quota outcome: {other:?}"),
    };
    limited.abort(&actor, accepted.id).await.unwrap();
    limited
        .create(&actor, channel, &input(3 * CHUNK_BYTES as u64))
        .await
        .unwrap();
}

#[tokio::test]
async fn sqlite_upload_lifecycle_preserves_originals_and_enforces_quota() {
    let repository = crate::db::SqliteChatRepository::connect("sqlite::memory:")
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let store = repository.media_store_for_test(limits());
    contract(&repository, &store).await;
    policy_contract(&repository, &store).await;
    global_policy_contract(&repository, &store).await;
    channel_pilot_contract(&repository, &store).await;
}

#[tokio::test]
async fn postgres_upload_lifecycle_preserves_originals_and_enforces_quota() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let repository = crate::db::PostgresChatRepository::connect(&url)
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let store = repository.media_store_for_test(limits());
    contract(&repository, &store).await;
    policy_contract(&repository, &store).await;
    global_policy_contract(&repository, &store).await;
    channel_pilot_contract(&repository, &store).await;
}

async fn channel_pilot_contract<R: ChatRepository>(repository: &R, store: &MediaStore) {
    let (actor, channel) = owner(repository).await;
    let mut pilot = store.clone();
    pilot.allowed_channels = Some(vec![Uuid::now_v7()]);
    assert!(matches!(
        pilot.create(&actor, channel, &input(3)).await,
        Err(UploadError::NotFound)
    ));
    pilot.allowed_channels = Some(vec![channel]);
    let upload = pilot.create(&actor, channel, &input(3)).await.unwrap();
    let bytes = [0xff, 0xd8, 0xff];
    pilot
        .put_chunk(&actor, upload.id, 0, &bytes, &hash(&bytes))
        .await
        .unwrap();
    pilot.allowed_channels = Some(vec![]);
    assert!(matches!(
        pilot.status(&actor, upload.id).await,
        Err(UploadError::NotFound)
    ));
    assert!(matches!(
        pilot
            .put_chunk(&actor, upload.id, 0, &bytes, &hash(&bytes))
            .await,
        Err(UploadError::NotFound)
    ));
    assert!(matches!(
        pilot.complete(&actor, upload.id).await,
        Err(UploadError::NotFound)
    ));
    // A restriction is an admission control, never a change to original readers.
    pilot.allowed_channels = Some(vec![channel]);
    let media = pilot.complete(&actor, upload.id).await.unwrap();
    let mut restored_reader = store.clone();
    restored_reader.allowed_channels = Some(vec![]);
    assert_eq!(
        repository
            .read_media_slice(actor.clone(), media, 0, 3)
            .await
            .unwrap(),
        bytes
    );
    restored_reader.cleanup_expired().await.unwrap();
}

async fn policy_contract<R: ChatRepository>(repository: &R, store: &MediaStore) {
    use crate::domain::MediaUpload;
    let (actor, channel) = owner(repository).await;
    let mut limited = store.clone();
    limited.limits.owner_bytes = 4 * CHUNK_BYTES as u64;
    limited.check_policy(true).await.unwrap();
    limited.check_policy(false).await.unwrap();
    assert!(
        matches!(store.check_policy(true).await, Err(UploadError::Conflict)),
        "pods must not overwrite shared policy"
    );
    let legacy = |size: usize| MediaUpload {
        actor: actor.clone(),
        channel_id: crate::domain::ChannelId::from_uuid(channel),
        filename: "generated.png".into(),
        content_type: "image/png".into(),
        content: vec![7; size],
        dimensions: None,
        preview: None,
    };
    let upload_input = input(3 * CHUNK_BYTES as u64);
    let (upload, old) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            limited.create(&actor, channel, &upload_input),
            repository.store_media(legacy(2 * CHUNK_BYTES), "legacy".into())
        )
    })
    .await
    .unwrap();
    let upload = match (upload, old) {
        (Ok(upload), Err(RepositoryError::Quota)) => {
            repository
                .store_media(legacy(1), "small".into())
                .await
                .unwrap();
            upload
        }
        (Err(UploadError::Quota), Ok(_)) => limited
            .create(&actor, channel, &input(2 * CHUNK_BYTES as u64))
            .await
            .unwrap(),
        other => panic!("race bypassed shared owner quota: {other:?}"),
    };
    let mut bytes = vec![4; upload.size_bytes as usize];
    bytes[..3].copy_from_slice(&[0xff, 0xd8, 0xff]);
    for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        limited
            .put_chunk(&actor, upload.id, index as u32, chunk, &hash(chunk))
            .await
            .unwrap();
    }
    let media = limited.complete(&actor, upload.id).await.unwrap();
    assert_eq!(
        repository
            .media_metadata(actor.clone(), media)
            .await
            .unwrap()
            .size_bytes,
        upload.size_bytes
    );
    assert_eq!(
        repository.store_media(legacy(1), "overflow".into()).await,
        Err(RepositoryError::Quota)
    );
    with_store!(store, |pool, pg| {
        let result = sqlx::query(&sql(
            "update media_objects set size_bytes=size_bytes+1 where id=?uuid",
            pg,
        ))
        .bind(media.to_string())
        .execute(pool)
        .await;
        assert!(result.is_err(), "immutable size must not bypass quota");
        // This is isolated CI fixture cleanup, never application rollback logic.
        sqlx::query("update media_upload_budget set max_file_bytes=null,owner_bytes=null,total_bytes=null where id=1").execute(pool).await.unwrap();
    });
}

async fn global_policy_contract<R: ChatRepository>(repository: &R, store: &MediaStore) {
    let (a, channel_a) = owner(repository).await;
    let (b, channel_b) = owner(repository).await;
    let mut limited = store.clone();
    limited.limits.owner_bytes = 4 * CHUNK_BYTES as u64;
    limited.check_policy(true).await.unwrap();
    let used: i64 = with_store!(store, |pool, pg| {
        let used:i64=sqlx::query_scalar("select cast(coalesce((select sum(case when size_bytes<1048576 then 1048576 else size_bytes end) from media_objects),0)+coalesce((select sum(case when expected_bytes<1048576 then 1048576 else expected_bytes end) from media_uploads where state!='complete'),0) as bigint)").fetch_one(pool).await.unwrap();
        sqlx::query(&sql(
            "update media_upload_budget set total_bytes=? where id=1",
            pg,
        ))
        .bind(used + 3 * CHUNK_BYTES as i64)
        .execute(pool)
        .await
        .unwrap();
        used
    });
    limited.limits.total_bytes = (used + 3 * CHUNK_BYTES as i64) as u64;
    let input_a = input(3 * CHUNK_BYTES as u64);
    let input_b = input(3 * CHUNK_BYTES as u64);
    let (left, right) = tokio::join!(
        limited.create(&a, channel_a, &input_a),
        limited.create(&b, channel_b, &input_b)
    );
    let (actor, channel, accepted) = match (left, right) {
        (Ok(u), Err(UploadError::Quota)) => (a, channel_a, u),
        (Err(UploadError::Quota), Ok(u)) => (b, channel_b, u),
        other => panic!("global quota race: {other:?}"),
    };
    with_store!(store, |pool, pg| {
        // The reservation is still OPEN, so an insertion cannot borrow it.
        let result=sqlx::query(&sql("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'forged.jpg','image/jpeg',?,'forged',current_timestamp)",pg))
            .bind(Uuid::now_v7().to_string()).bind(actor.to_string()).bind(channel.to_string()).bind(format!("chunks:v1:{}",accepted.id)).bind(accepted.size_bytes as i64).execute(pool).await;
        assert!(
            result.is_err(),
            "open upload must not confer reservation credit"
        );
    });
    limited.abort(&actor, accepted.id).await.unwrap();
    with_store!(store, |pool, pg| {
        if pg {
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("update media_upload_budget set revision=revision where id=1")
                .execute(&mut *tx)
                .await
                .unwrap();
            let actor_clone = actor.clone();
            let pool_clone = pool.clone();
            let waiting = tokio::spawn(async move {
                sqlx::query(&sql("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'wait.jpg','image/jpeg',1,'wait',current_timestamp)",pg))
                    .bind(Uuid::now_v7().to_string()).bind(actor_clone.to_string()).bind(channel.to_string()).bind("wait:legacy").execute(&pool_clone).await
            });
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert!(!waiting.is_finished());
            sqlx::query(&sql("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,created_at) values(?uuid,?uuid,?uuid,?,'fill.jpg','image/jpeg',?,'fill',current_timestamp)",pg))
                .bind(Uuid::now_v7().to_string()).bind(actor.to_string()).bind(channel.to_string()).bind("fill:legacy").bind(3*CHUNK_BYTES as i64).execute(&mut *tx).await.unwrap();
            tx.commit().await.unwrap();
            let error = tokio::time::timeout(Duration::from_secs(5), waiting)
                .await
                .unwrap()
                .unwrap()
                .unwrap_err();
            assert!(
                matches!(error,sqlx::Error::Database(ref db) if db.message()=="SPROYT_MEDIA_QUOTA"),
                "waiter used stale totals"
            );
        }
        sqlx::query("update media_upload_budget set max_file_bytes=null,owner_bytes=null,total_bytes=null where id=1").execute(pool).await.unwrap();
    });
}
