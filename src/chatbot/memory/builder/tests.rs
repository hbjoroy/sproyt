use super::*;
use crate::chatbot::memory::repository::tests::Fixture;

async fn pending(f: &Fixture, from: i64, to: i64) {
    // CI runs prior reply contracts in this isolated test database, without a
    // worker to settle their synthetic pending jobs. Quota tests run serially;
    // finish those fixtures before asserting memory's global admission.
    f.service.store.execute("update circle_chat_agent_jobs set status='skipped',lease_token=null,leased_until=null where status in ('pending','leased')",&[]).await.unwrap();
    f.service
        .store
        .execute(
            "update circle_chat_agents set enabled=true where agent_id=?uuid",
            std::slice::from_ref(&f.agent),
        )
        .await
        .unwrap();
    for sequence in from..=to {
        let id = Uuid::now_v7().to_string();
        f.service.store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',?int,'I prefer Greek examples',current_timestamp)",&[id,f.channel.clone(),f.owner.to_string(),sequence.to_string()]).await.unwrap();
    }
    f.service.store.execute("update agent_memory_scopes set start_sequence=2,processed_sequence=2,dirty_sequence=?int,available_at=0,attempts=0 where profile_id=?uuid and channel_id=?uuid",&[to.to_string(),f.profile.clone(),f.channel.clone()]).await.unwrap();
    f.service.store.execute("update agent_memory_model_quota set lease_token=null,leased_until=0,memory_calls=0,memory_window_started=0 where id=1",&[]).await.unwrap();
}

async fn contracts(store: Store) {
    let f = Fixture::create(store).await;
    pending(&f, 3, 62).await;
    let first = f.service.claim_memory_batch().await.unwrap().unwrap();
    assert_eq!(first.cursor, 22);
    assert_eq!(first.messages.len(), 20);
    assert!(f.service.claim_memory_batch().await.unwrap().is_none());
    assert!(
        f.service
            .acquire_model_permit(false)
            .await
            .unwrap()
            .is_none()
    );
    // Work arriving during the call is not overwritten by the captured highwater.
    f.service.store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',63,'New during model call',current_timestamp)",&[Uuid::now_v7().to_string(),f.channel.clone(),f.owner.to_string()]).await.unwrap();
    f.service.store.execute("update agent_memory_scopes set dirty_sequence=63 where profile_id=?uuid and channel_id=?uuid",&[f.profile.clone(),f.channel.clone()]).await.unwrap();
    f.service.commit_memory_batch(&first, &[]).await.unwrap();
    first.permit.release(true).await.unwrap();
    let query = "select cast(processed_sequence as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid";
    let args = [f.profile.clone(), f.channel.clone()];
    assert_eq!(f.service.store.values(query, &args).await.unwrap(), ["22"]);
    f.service
        .store
        .execute(
            "update agent_memory_scopes set available_at=0 where profile_id=?uuid",
            std::slice::from_ref(&f.profile),
        )
        .await
        .unwrap();
    let second = f.service.claim_memory_batch().await.unwrap().unwrap();
    assert_eq!(second.cursor, 42);
    assert!(decode_output("not json", &second).is_err());
    assert!(decode_output(&json!({"notes":[{"kind":"preference","text":"Greek examples","source_message_ids":[Uuid::now_v7()]}]}).to_string(),&second).is_err());
    let candidate=decode_output(&json!({"notes":[{"kind":"preference","text":"Prefers Greek examples","source_message_ids":[second.messages[0].id]}]}).to_string(),&second).unwrap();
    f.service
        .commit_memory_batch(&second, &candidate)
        .await
        .unwrap();
    second.permit.release(true).await.unwrap();
    let view = f
        .service
        .read_memory(&f.owner, &f.circle, &f.agent)
        .await
        .unwrap();
    assert_eq!(view.notes.len(), 1);
    assert_eq!(view.notes[0].source_message_ids.len(), 20);
    // The two-call global window survives worker restarts; reply calls can proceed.
    f.service
        .store
        .execute(
            "update agent_memory_scopes set available_at=0 where profile_id=?uuid",
            std::slice::from_ref(&f.profile),
        )
        .await
        .unwrap();
    assert!(f.service.claim_memory_batch().await.unwrap().is_none());
    let reply = f
        .service
        .acquire_model_permit(false)
        .await
        .unwrap()
        .unwrap();
    reply.release(false).await.unwrap();
    assert!(
        f.service
            .acquire_model_permit(false)
            .await
            .unwrap()
            .is_none()
    );
    f.service
        .store
        .execute(
            "update agent_memory_model_quota set leased_until=0,memory_window_started=0 where id=1",
            &[],
        )
        .await
        .unwrap();
    let successor = f
        .service
        .acquire_model_permit(false)
        .await
        .unwrap()
        .unwrap();
    reply.release(true).await.unwrap();
    assert!(
        f.service
            .acquire_model_permit(false)
            .await
            .unwrap()
            .is_none()
    );
    successor.release(true).await.unwrap();
    let third = f.service.claim_memory_batch().await.unwrap().unwrap();
    assert_eq!(third.cursor, 62);
    // Source edits invalidate the old batch even though its cursor did not change.
    f.service
        .store
        .execute(
            "update messages set body='Corrected',edited_at=current_timestamp where id=?uuid",
            &[third.messages[0].id.clone()],
        )
        .await
        .unwrap();
    f.service.commit_memory_batch(&third, &[]).await.unwrap();
    assert_ne!(f.service.store.values(query, &args).await.unwrap(), ["62"]);
    third.permit.release(true).await.unwrap();
    // Restore expiration permits recovery, but stale holders cannot persist.
    f.service
        .store
        .execute(
            "update agent_memory_scopes set available_at=0,leased_until=0 where profile_id=?uuid",
            std::slice::from_ref(&f.profile),
        )
        .await
        .unwrap();
    f.service
        .store
        .execute(
            "update agent_memory_model_quota set memory_window_started=0 where id=1",
            &[],
        )
        .await
        .unwrap();
    let recovered = f.service.claim_memory_batch().await.unwrap().unwrap();
    f.service.commit_memory_batch(&third, &[]).await.unwrap();
    recovered.permit.release(true).await.unwrap();
}

#[tokio::test]
async fn sqlite_bounded_builder_and_shared_admission() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    contracts(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_bounded_builder_and_shared_admission() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    contracts(Store::Pg(pool)).await;
}

#[test]
fn bounded_text_keeps_utf8_boundaries() {
    assert_eq!(bounded_text("αβγ", 5), "αβ");
    assert_eq!(bounded_text("short", 600), "short");
}

#[tokio::test]
async fn sqlite_low_traffic_fairness_and_completed_invalid_output() {
    use axum::{
        Router,
        routing::{get, post},
    };
    use std::sync::Arc;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    let mut f = Fixture::create(Store::Sqlite(pool.clone())).await;
    pending(&f, 3, 3).await;
    assert!(f.service.claim_memory_batch().await.unwrap().is_none());
    f.service.store.execute("update messages set created_at=datetime('now','-6 minutes') where channel_id=?uuid and sequence=3",std::slice::from_ref(&f.channel)).await.unwrap();
    let batch = f.service.claim_memory_batch().await.unwrap().unwrap();
    assert_eq!(batch.cursor, 3);
    assert!(input(&batch).unwrap().len() <= super::super::MAX_MODEL_INPUT_BYTES);
    let app = Router::new()
        .route(
            "/models",
            get(|| async { axum::Json(json!({"data":[{"id":"test"}]})) }),
        )
        .route(
            "/chat/completions",
            post(|| async {
                axum::Json(json!({"choices":[{"message":{"content":"invalid JSON"}}]}))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    f.service.model = Some(Arc::new(crate::chatbot::VllmChat {
        base,
        key: None,
        http: reqwest::Client::new(),
    }));
    let (complete, result) = f.service.build_memory(&batch).await;
    assert!(complete && result.is_err());
    batch.permit.release(complete).await.unwrap();
    let permit = f
        .service
        .acquire_model_permit(false)
        .await
        .unwrap()
        .unwrap();
    permit.release(true).await.unwrap();
    f.service.commit_memory_batch(&batch, &[]).await.unwrap();
    let other = Fixture::create(Store::Sqlite(pool.clone())).await;
    pending(&other, 3, 62).await;
    // A waiting second person runs before the just-completed busy scope.
    f.service.store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',4,'More conversation',datetime('now','-6 minutes'))",&[Uuid::now_v7().to_string(),f.channel.clone(),f.owner.to_string()]).await.unwrap();
    f.service.store.execute("update agent_memory_scopes set dirty_sequence=4 where profile_id=?uuid and channel_id=?uuid",&[f.profile.clone(),f.channel.clone()]).await.unwrap();
    let next = f.service.claim_memory_batch().await.unwrap().unwrap();
    assert_eq!(next.scope.owner.user_id, *other.owner.as_uuid());
    next.permit.release(true).await.unwrap();
    server.abort();
}

#[tokio::test]
async fn postgres_model_admission_two_connections_and_edit_before_builder_profile() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let f = Fixture::create(Store::Pg(pool.clone())).await;
    pending(&f, 3, 22).await;
    let (a, b) = tokio::join!(
        f.service.acquire_model_permit(false),
        f.service.acquire_model_permit(false)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.is_some(), b.is_some());
    a.or(b).unwrap().release(true).await.unwrap();
    let batch = f.service.claim_memory_batch().await.unwrap().unwrap();
    batch.permit.release(true).await.unwrap();
    let mut editing = pool.begin().await.unwrap();
    sqlx::query("select id from messages where id=$1::text::uuid for update")
        .bind(&batch.messages[0].id)
        .fetch_one(&mut *editing)
        .await
        .unwrap();
    let id = batch.messages[0].id.clone();
    let service = f.service.clone();
    let committing = tokio::spawn(async move { service.commit_memory_batch(&batch, &[]).await });
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Old ordering would hold profile and make this AFTER-trigger deadlock.
    tokio::time::timeout(Duration::from_secs(3),sqlx::query("update messages set body='Changed',edited_at=clock_timestamp() where id=$1::text::uuid").bind(&id).execute(&mut *editing)).await.expect("builder must not hold profile while waiting for edited source").unwrap();
    editing.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), committing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(f.service.store.values("select cast(processed_sequence as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid",&[f.profile.clone(),f.channel.clone()]).await.unwrap(),["2"]);
}
