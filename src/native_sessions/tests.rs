use super::*;

async fn sqlite() -> (NativeSessions, sqlx::SqlitePool) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/sqlite/0068_native_sessions.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let service = NativeSessions::new(
        Db::Sqlite(pool.clone()),
        "https://sproyt-canary.bjoroy.me",
        "https://issuer.example/",
        &URL_SAFE_NO_PAD.encode([31; 32]),
        &[],
    )
    .await
    .unwrap();
    (service, pool)
}
fn exchange_request(id: &str, code: &str, verifier: &str) -> ExchangeRequest {
    ExchangeRequest {
        transaction_id: id.into(),
        code: code.into(),
        verifier: verifier.into(),
        redemption_id: "a".repeat(32),
        environment: ENVIRONMENT.into(),
        destination_id: "windows-loopback".into(),
    }
}
async fn ready(service: &NativeSessions) -> (String, String, String, String) {
    let id = random().unwrap();
    let verifier = random().unwrap();
    let code = random().unwrap();
    let user =
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, random().unwrap().as_bytes()).to_string();
    let secret = AttemptSecret {
        provider_cookies: "sealed-provider-cookie-fixture".into(),
        provider_expires: now() + 240,
        display_name: "Synthetic Alice".into(),
        ..Default::default()
    };
    let sealed = service
        .0
        .codec
        .seal(&format!("attempt:{id}"), &secret)
        .unwrap();
    let mut tx = service.0.db.begin().await.unwrap();
    tx.exec("INSERT INTO native_attempts(id,state_hash,challenge,expected_user_id,start_epoch,epoch,user_id,callback_port,created_at,expires_at,phase,cookie_hash,confirm_hash,code_hash,handoff_expires,redemption_id,receipt_expires,family,secret,receipt) VALUES($1,$2,$3,$4,'0','0',$4,'19092',$5,$6,'ready','','',$7,$6,'','','',$8,'')",&[&id,&hash(&random().unwrap()),&hash(&verifier),&user,&now().to_string(),&(now()+60).to_string(),&hash(&code),&sealed]).await.unwrap();
    tx.commit().await.unwrap();
    (id, code, verifier, user)
}
async fn seed_grant(service: &NativeSessions) -> (Grant, String, String, String) {
    let (id, code, verifier, _) = ready(service).await;
    let grant = service
        .exchange(exchange_request(&id, &code, &verifier))
        .await
        .unwrap()
        .native_session;
    (grant, id, code, verifier)
}

#[tokio::test]
async fn encrypted_handoff_cancel_and_epoch_fences() {
    let (service, pool) = sqlite().await;
    let (grant, id, code, verifier) = seed_grant(&service).await;
    let retry = service
        .exchange(exchange_request(&id, &code, &verifier))
        .await
        .unwrap();
    assert!(retry.native_session == grant);
    let serialized = sqlx::query_scalar::<_, String>("SELECT secret FROM native_families")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!serialized.contains(&grant.access_token) && !serialized.contains("provider-cookie"));
    assert!(
        service
            .0
            .codec
            .open::<FamilySecret>("wrong-purpose", &serialized)
            .is_err()
    );
    let reference = NativeSessionRef {
        family: grant.family.clone(),
        access_hash: hash(&grant.access_token),
        generation: 0,
        epoch: 0,
    };
    assert!(service.referenced(&reference).await.is_ok());
    assert!(
        service
            .cancel_login(CancelRequest {
                transaction_id: id.clone(),
                verifier: random().unwrap(),
                environment: ENVIRONMENT.into(),
                destination_id: "windows-loopback".into()
            })
            .await
            .is_err()
    );
    service
        .cancel_login(CancelRequest {
            transaction_id: id.clone(),
            verifier: verifier.clone(),
            environment: ENVIRONMENT.into(),
            destination_id: "windows-loopback".into(),
        })
        .await
        .unwrap();
    assert!(service.referenced(&reference).await.is_err());
    assert!(
        service
            .exchange(exchange_request(&id, &code, &verifier))
            .await
            .is_err()
    );
    service
        .cancel_login(CancelRequest {
            transaction_id: id,
            verifier,
            environment: ENVIRONMENT.into(),
            destination_id: "windows-loopback".into(),
        })
        .await
        .unwrap();
    let (grant, id, code, verifier) = seed_grant(&service).await;
    sqlx::query("INSERT INTO native_epochs(user_id,epoch,changed_at) VALUES($1,'1',$2)")
        .bind(&grant.user_id)
        .bind(now().to_string())
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        service
            .exchange(exchange_request(&id, &code, &verifier))
            .await
            .is_err()
    );
    assert!(
        service
            .referenced(&NativeSessionRef {
                family: grant.family,
                access_hash: hash(&grant.access_token),
                generation: 0,
                epoch: 0
            })
            .await
            .is_err()
    );
}

async fn atomic_exchange_cancel(service: NativeSessions) {
    let (id, code, verifier, _) = ready(&service).await;
    let (a, b) = tokio::join!(
        service.exchange(exchange_request(&id, &code, &verifier)),
        service.exchange(exchange_request(&id, &code, &verifier))
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(a.native_session == b.native_session);
    let rows = service
        .0
        .db
        .rows("SELECT CAST(count(*) AS TEXT) FROM native_families", &[])
        .await
        .unwrap();
    assert!(rows[0][0] == "1");
    let reference = NativeSessionRef {
        family: a.native_session.family,
        access_hash: hash(&a.native_session.access_token),
        generation: 0,
        epoch: 0,
    };
    service
        .cancel_login(CancelRequest {
            transaction_id: id.clone(),
            verifier: verifier.clone(),
            environment: ENVIRONMENT.into(),
            destination_id: "windows-loopback".into(),
        })
        .await
        .unwrap();
    assert!(service.referenced(&reference).await.is_err());
    let (id, code, verifier, _) = ready(&service).await;
    let (_issued, cancel) = tokio::join!(
        service.exchange(exchange_request(&id, &code, &verifier)),
        service.cancel_login(CancelRequest {
            transaction_id: id.clone(),
            verifier: verifier.clone(),
            environment: ENVIRONMENT.into(),
            destination_id: "windows-loopback".into()
        })
    );
    cancel.unwrap();
    assert!(
        service
            .exchange(exchange_request(&id, &code, &verifier))
            .await
            .is_err()
    );
    let rows = service
        .0
        .db
        .rows(
            "SELECT family FROM native_families WHERE phase='active'",
            &[],
        )
        .await
        .unwrap();
    assert!(rows.is_empty());
}
#[tokio::test]
async fn sqlite_atomic_exchange_and_cancel() {
    let (service, _) = sqlite().await;
    atomic_exchange_cancel(service).await;
}

#[tokio::test]
async fn lease_loss_cannot_retry_provider_grant_or_resurrect_receipts() {
    let (service, pool) = sqlite().await;
    let (grant, id, code, verifier) = seed_grant(&service).await;
    sqlx::query(
        "UPDATE native_families SET phase='refreshing',lease_id='unknown-result',lease_expires='0'",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        service
            .refresh(
                &AuthService::development(),
                &grant.refresh_token,
                RefreshRequest {
                    operation_id: "a".repeat(32),
                    generation: 0
                }
            )
            .await
            == Err(NativeError::Unauthorized)
    );
    let phase: String = sqlx::query_scalar("SELECT phase FROM native_families")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(phase == "reauth");
    assert!(
        service
            .exchange(exchange_request(&id, &code, &verifier))
            .await
            .is_err()
    );
    let (grant, _, _, _) = seed_grant(&service).await;
    // A provider error AFTER the durable claim must fence, not retry the provider.
    assert!(
        service
            .refresh(
                &AuthService::development(),
                &grant.refresh_token,
                RefreshRequest {
                    operation_id: "b".repeat(32),
                    generation: 0
                }
            )
            .await
            .is_err()
    );
    assert!(
        service
            .refresh(
                &AuthService::development(),
                &grant.refresh_token,
                RefreshRequest {
                    operation_id: "b".repeat(32),
                    generation: 0
                }
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn key_rotation_is_purpose_and_instance_bound() {
    let old = URL_SAFE_NO_PAD.encode([1; 32]);
    let new = URL_SAFE_NO_PAD.encode([2; 32]);
    let old_codec = Codec::new(&old, &[], "instance").unwrap();
    let value = old_codec.seal("receipt", &"private").unwrap();
    let rotated = Codec::new(&new, &[old], "instance").unwrap();
    assert!(rotated.open::<String>("receipt", &value).unwrap() == "private");
    assert!(rotated.open::<String>("family", &value).is_err());
    assert!(
        Codec::new(&new, &[], "other")
            .unwrap()
            .open::<String>("receipt", &value)
            .is_err()
    );
}

#[tokio::test]
async fn postgres_atomic_exchange_and_cancel_if_configured() {
    let Ok(url) = std::env::var("TEST_DATABASE_URL") else {
        return;
    };
    let admin = sqlx::PgPool::connect(&url).await.unwrap();
    let schema = format!(
        "native_test_{}",
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, random().unwrap().as_bytes()).simple()
    );
    sqlx::query(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let configured = schema.clone();
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .after_connect(move |conn, _| {
            let schema = configured.clone();
            Box::pin(async move {
                sqlx::query(&format!("SET search_path TO {schema}"))
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    sqlx::raw_sql(include_str!(
        "../../migrations/postgres/0068_native_sessions.sql"
    ))
    .execute(&pool)
    .await
    .unwrap();
    let service = NativeSessions::new(
        Db::Pg(pool.clone()),
        "https://sproyt-canary.bjoroy.me",
        "https://issuer.example/",
        &URL_SAFE_NO_PAD.encode([31; 32]),
        &[],
    )
    .await
    .unwrap();
    atomic_exchange_cancel(service).await;
    pool.close().await;
    sqlx::query(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
