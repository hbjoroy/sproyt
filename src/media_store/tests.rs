use super::*;
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
}
