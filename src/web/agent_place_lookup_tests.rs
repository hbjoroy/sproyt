use super::*;
use crate::chatbot::{location, maps_lookup, place_lookup};
use sqlx::SqlitePool;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

async fn batch(
    state: &AppState,
    user: &str,
    account: &str,
    channel: &str,
    reply: &str,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("x-sproyt-account", account.parse().unwrap());
    available(
        State(state.clone()),
        Path(channel.into()),
        Query(WsQuery {
            participant: Some(user.into()),
        }),
        headers,
        Json(MessageBatch {
            message_ids: vec![reply.into()],
        }),
    )
    .await
}
async fn lookup_card(
    state: &AppState,
    user: &str,
    account: &str,
    channel: &str,
    reply: &str,
    consent: bool,
    binding: &str,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("x-sproyt-account", account.parse().unwrap());
    lookup(
        State(state.clone()),
        Path((channel.into(), reply.into())),
        Query(WsQuery {
            participant: Some(user.into()),
        }),
        headers,
        Json(LookupRequest {
            allow_shared_location: consent,
            preview_binding: binding.to_owned(),
        }),
    )
    .await
}
async fn route_card(
    state: &AppState,
    user: &str,
    account: &str,
    channel: &str,
    reply: &str,
    token: &str,
    consent: bool,
) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert("x-sproyt-account", account.parse().unwrap());
    route(
        State(state.clone()),
        Path((channel.into(), reply.into())),
        Query(WsQuery {
            participant: Some(user.into()),
        }),
        headers,
        Json(RouteRequest {
            token: token.into(),
            allow_shared_location: consent,
        }),
    )
    .await
}
async fn body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
async fn seed(pool: &SqlitePool, owner: &str, other: &str) -> (String, String, String, String) {
    let (agent, circle, channel, source, reply, job, share) = (
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
        Uuid::now_v7(),
    );
    for (id, kind) in [
        (owner, "human"),
        (other, "human"),
        (&agent.to_string(), "agent"),
    ] {
        sqlx::query("insert into users(id,kind,display_name) values(?,?,'Test')")
            .bind(id)
            .bind(kind)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("insert into circles(id,slug,name,created_by) values(?,'place-test','Place',?)")
        .bind(circle.to_string())
        .bind(owner)
        .execute(pool)
        .await
        .unwrap();
    for id in [owner, other] {
        sqlx::query("insert into circle_memberships(circle_id,user_id,role) values(?,?,'member')")
            .bind(circle.to_string())
            .bind(id)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values(?,'place-test','Place','public',?,?)").bind(channel.to_string()).bind(owner).bind(circle.to_string()).execute(pool).await.unwrap();
    for id in [owner, other] {
        sqlx::query(
            "insert into channel_memberships(channel_id,user_id,role) values(?,?,'member')",
        )
        .bind(channel.to_string())
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    }
    sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?,?,?,'test',?,'test',60,current_timestamp)").bind(agent.to_string()).bind(owner).bind(owner).bind(format!("test-{agent}")).execute(pool).await.unwrap();
    sqlx::query("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values(?,?,'[]','[]',1,?,?,0,0)").bind(agent.to_string()).bind(circle.to_string()).bind(owner).bind(owner).execute(pool).await.unwrap();
    sqlx::query("insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) values(?,?,?,?,55.1,12.1,12,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now','+30 minutes'))").bind(owner).bind(agent.to_string()).bind(channel.to_string()).bind(share.to_string()).execute(pool).await.unwrap();
    sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,1,'bars near me','Test')").bind(source.to_string()).bind(channel.to_string()).bind(owner).execute(pool).await.unwrap();
    sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,2,?,'Agent')").bind(reply.to_string()).bind(channel.to_string()).bind(agent.to_string()).bind("Opne stadssøket for ferske barforslag.").execute(pool).await.unwrap();
    sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,location_share_id,place_lookup_plan) values(?,?,?,?,1,'leased',0,0,?,?,?)").bind(job.to_string()).bind(agent.to_string()).bind(source.to_string()).bind(channel.to_string()).bind("Opne stadssøket for ferske barforslag.").bind(share.to_string()).bind(r#"{"query":"bars","origin_mode":"shared"}"#).execute(pool).await.unwrap();
    (
        channel.to_string(),
        source.to_string(),
        reply.to_string(),
        format!("circle-chat-agent:{job}"),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn authorised_place_card_is_transient_consent_gated_and_revocable() {
    let calls = Arc::new(AtomicUsize::new(0));
    maps_lookup::TEST_PROVIDER_CALLS.scope(calls.clone(),place_lookup::TEST_ENABLED.scope(true,location::TEST_ENABLED.scope(true,async {
        let auth=crate::auth::AuthService::development();
        let owner=auth.authenticate_development(Some("place-owner".into())).unwrap().user;
        let other=auth.authenticate_development(Some("place-other".into())).unwrap().user;
        let outsider=auth.authenticate_development(Some("place-outsider".into())).unwrap().user;
        let (service,pool,_)=place_lookup::database_tests::fixture().await;
        let (channel,source,reply,request)=seed(&pool,&owner.id.to_string(),&other.id.to_string()).await;
        let mut state=crate::web::native::test_state(auth,None).await;
        state.chat_agents=Some(service);
        let account=owner.id.to_string();
        let other_account=other.id.to_string();
        let outsider_account=outsider.id.to_string();
        let unavailable=batch(&state,"place-owner",&account,&channel,&reply).await;
        assert_eq!(unavailable.status(),StatusCode::OK);
        assert_eq!(unavailable.headers()[CACHE_CONTROL],"no-store");
        assert_eq!(body(unavailable).await["message_ids"],serde_json::json!([]),"neutral reply alone is not a receipt");
        let agent:String=sqlx::query_scalar("select sender_id from messages where id=?").bind(&reply).fetch_one(&pool).await.unwrap();
        sqlx::query("insert into command_receipts(principal_id,request_id,message_id) values(?,?,?)").bind(agent).bind(request).bind(&reply).execute(&pool).await.unwrap();
        let available=batch(&state,"place-owner",&account,&channel,&reply).await;
        let availability=body(available).await;
        assert_eq!(availability["message_ids"],serde_json::json!([reply]),"receipt is sufficient before job finish");
        assert_eq!(availability["google_message_ids"],serde_json::json!([reply]),"the configured Google source is advertised");
        assert_eq!(availability["geoapify_message_ids"],serde_json::json!([]),"an unconfigured source remains hidden");
        let changed_account=batch(&state,"place-owner",&other_account,&channel,&reply).await;
        assert_eq!(changed_account.status(),StatusCode::CONFLICT);
        let mut preview_headers=HeaderMap::new();
        preview_headers.insert("x-sproyt-account",account.parse().unwrap());
        let preview_response=preview(State(state.clone()),Path((channel.clone(),reply.clone())),Query(WsQuery{participant:Some("place-owner".into())}),preview_headers).await;
        assert_eq!(preview_response.status(),StatusCode::OK);
        assert_eq!(preview_response.headers()[CACHE_CONTROL],"no-store");
        let preview_body=body(preview_response).await;
        let binding=preview_body["preview_binding"].as_str().unwrap();
        let mismatch=lookup_card(&state,"place-owner",&account,&channel,&reply,true,"stale").await;
        assert_eq!(mismatch.status(),StatusCode::NOT_FOUND);
        assert_eq!(calls.load(Ordering::SeqCst),0);
        let no_consent=lookup_card(&state,"place-owner",&account,&channel,&reply,false,binding).await;
        assert_eq!(no_consent.status(),StatusCode::FORBIDDEN);
        assert_eq!(no_consent.headers()[CACHE_CONTROL],"no-store");
        let other_result=lookup_card(&state,"place-other",&other_account,&channel,&reply,true,binding).await;
        assert_eq!(other_result.status(),StatusCode::NOT_FOUND,"other channel members must not receive the shared coordinates");
        let outside=lookup_card(&state,"place-outsider",&outsider_account,&channel,&reply,true,binding).await;
        assert_eq!(outside.status(),StatusCode::NOT_FOUND);
        assert_eq!(calls.load(Ordering::SeqCst),0,"no external provider call for missing consent or other users");
        let allowed=lookup_card(&state,"place-owner",&account,&channel,&reply,true,binding).await;
        assert_eq!(allowed.status(),StatusCode::OK);
        assert_eq!(allowed.headers()[CACHE_CONTROL],"no-store");
        let result=body(allowed).await;
        assert_eq!(result["search"]["summary"],"mock");
        let token=result["route_tokens"][0]["token"].as_str().unwrap();
        assert_eq!(calls.load(Ordering::SeqCst),1);
        let bad=route_card(&state,"place-owner",&account,&channel,&reply,"bogus",true).await;
        assert_eq!(bad.status(),StatusCode::NOT_FOUND);
        let other_route=route_card(&state,"place-other",&other_account,&channel,&reply,token,true).await;
        assert_eq!(other_route.status(),StatusCode::NOT_FOUND);
        let no_route_consent=route_card(&state,"place-owner",&account,&channel,&reply,token,false).await;
        assert_eq!(no_route_consent.status(),StatusCode::FORBIDDEN);
        assert_eq!(calls.load(Ordering::SeqCst),1);
        sqlx::query("update place_lookup_admissions set next_allowed=0 where user_id=?").bind(&account).execute(&pool).await.unwrap();
        let route_response=route_card(&state,"place-owner",&account,&channel,&reply,token,true).await;
        assert_eq!(route_response.status(),StatusCode::OK);
        assert_eq!(route_response.headers()[CACHE_CONTROL],"no-store");
        assert!(body(route_response).await["warning"].as_str().unwrap().contains("beta"));
        assert_eq!(calls.load(Ordering::SeqCst),2);
        sqlx::query("update messages set edited_at=current_timestamp where id=?").bind(&source).execute(&pool).await.unwrap();
        assert_eq!(body(batch(&state,"place-owner",&account,&channel,&reply).await).await["message_ids"],serde_json::json!([]));
        sqlx::query("update messages set edited_at=null where id=?").bind(&source).execute(&pool).await.unwrap();
        sqlx::query("update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?").bind(&channel).execute(&pool).await.unwrap();
        assert_eq!(body(batch(&state,"place-owner",&account,&channel,&reply).await).await["message_ids"],serde_json::json!([]));
    }))).await;
}

#[tokio::test(flavor = "current_thread")]
async fn geoapify_http_requires_the_source_author_and_explicit_matching_consent() {
    crate::chatbot::GEOAPIFY_TEST_ENABLED.scope(true, place_lookup::TEST_ENABLED.scope(true, location::TEST_ENABLED.scope(true, async {
        let auth = crate::auth::AuthService::development();
        let owner = auth.authenticate_development(Some("place-owner".into())).unwrap().user;
        let other = auth.authenticate_development(Some("place-other".into())).unwrap().user;
        let (service, pool, _) = place_lookup::database_tests::fixture().await;
        let (channel, _, reply, request) = seed(&pool, &owner.id.to_string(), &other.id.to_string()).await;
        let agent: String = sqlx::query_scalar("select sender_id from messages where id=?").bind(&reply).fetch_one(&pool).await.unwrap();
        sqlx::query("insert into command_receipts(principal_id,request_id,message_id) values(?,?,?)").bind(agent).bind(request).bind(&reply).execute(&pool).await.unwrap();
        let mut state = crate::web::native::test_state(auth, None).await;
        state.chat_agents = Some(service);
        let headers_for = |id: &str| { let mut headers = HeaderMap::new(); headers.insert("x-sproyt-account", id.parse().unwrap()); headers };
        let owner_account = owner.id.to_string();
        let other_account = other.id.to_string();
        let query_for = |name: &str| Query(WsQuery { participant: Some(name.to_owned()) });
        let path = || Path((channel.clone(), reply.clone()));
        let invalid_account = geoapify_preview(State(state.clone()),path(),query_for("place-owner"),headers_for(&other_account)).await;
        assert_eq!(invalid_account.status(), StatusCode::CONFLICT);
        assert_eq!(invalid_account.headers()[CACHE_CONTROL], "no-store");
        let outsider = geoapify_preview(State(state.clone()),path(),query_for("place-other"),headers_for(&other_account)).await;
        assert_ne!(outsider.status(), StatusCode::OK);
        let preview = geoapify_preview(State(state.clone()),path(),query_for("place-owner"),headers_for(&owner_account)).await;
        assert_eq!(preview.status(), StatusCode::OK);
        assert_eq!(preview.headers()[CACHE_CONTROL], "no-store");
        let preview = body(preview).await;
        let binding = preview["preview_binding"].as_str().unwrap().to_owned();
        let request = |preview_binding: String, allow_shared_location: bool| Json(GeoapifyConsent { preview_binding, allow_shared_location, origin_text: None });
        let mismatch = geoapify_consent(State(state.clone()),path(),query_for("place-owner"),headers_for(&owner_account),request("wrong".into(),true)).await;
        assert_ne!(mismatch.status(), StatusCode::OK);
        let declined = geoapify_consent(State(state.clone()),path(),query_for("place-owner"),headers_for(&owner_account),request(binding.clone(),false)).await;
        assert_ne!(declined.status(), StatusCode::OK);
        let pending_count: i64 = sqlx::query_scalar("select count(*) from agent_place_rolls").fetch_one(&pool).await.unwrap();
        assert_eq!(pending_count, 0, "denied requests must not start a job");
        let accepted = geoapify_consent(State(state.clone()),path(),query_for("place-owner"),headers_for(&owner_account),request(binding.clone(),true)).await;
        assert_eq!(accepted.status(), StatusCode::OK);
        assert_eq!(accepted.headers()[CACHE_CONTROL], "no-store");
        let id = body(accepted).await["id"].as_str().unwrap().to_owned();
        let repeated = geoapify_consent(State(state.clone()),path(),query_for("place-owner"),headers_for(&owner_account),request(binding,true)).await;
        assert_eq!(body(repeated).await["id"], id);
        let count: i64 = sqlx::query_scalar("select count(*) from agent_place_rolls").fetch_one(&pool).await.unwrap();
        assert_eq!(count, 1, "retry must not create another job");
    }))).await;
}
