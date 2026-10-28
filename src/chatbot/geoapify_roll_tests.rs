use super::*;
use crate::{chat::ChatEngine, db::SqliteChatRepository};
use std::sync::Arc;

#[tokio::test(flavor = "current_thread")]
async fn sqlite_consent_retry_and_receipt_preserve_one_roll() {
    geoapify::TEST_ENABLED.scope(true,place_lookup::TEST_ENABLED.scope(true,location::TEST_ENABLED.scope(true,async {
        let db=format!("sqlite://./target/geoapify-roll-{}.sqlite",Uuid::now_v7());
        let repository=SqliteChatRepository::connect(&db).await.unwrap();
        repository.migrate().await.unwrap();
        let pool=SqlitePool::connect(&db).await.unwrap();
        let (owner,other,agent,circle,channel,source,reply,job,share)=(Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7());
        for (id,kind) in [(owner,"human"),(other,"human"),(agent,"agent")] {
            sqlx::query("insert into users(id,kind,display_name) values(?,?,'Test')").bind(id.to_string()).bind(kind).execute(&pool).await.unwrap();
        }
        sqlx::query("insert into circles(id,slug,name,created_by) values(?,'geo-roll','Geo roll',?)").bind(circle.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        for id in [owner,other] {sqlx::query("insert into circle_memberships(circle_id,user_id,role) values(?,?,'member')").bind(circle.to_string()).bind(id.to_string()).execute(&pool).await.unwrap();}
        sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values(?,'geo-roll','Geo roll','public',?,?)").bind(channel.to_string()).bind(owner.to_string()).bind(circle.to_string()).execute(&pool).await.unwrap();
        for id in [owner,other] {sqlx::query("insert into channel_memberships(channel_id,user_id,role) values(?,?,'member')").bind(channel.to_string()).bind(id.to_string()).execute(&pool).await.unwrap();}
        sqlx::query("insert into channel_sequences(channel_id,next_sequence) values(?,3)").bind(channel.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?,?,?,'test',?,'test',60,current_timestamp)").bind(agent.to_string()).bind(owner.to_string()).bind(owner.to_string()).bind(format!("geo-{agent}")).execute(&pool).await.unwrap();
        sqlx::query("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values(?,?,'[]','[]',1,?,?,0,0)").bind(agent.to_string()).bind(circle.to_string()).bind(owner.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) values(?,?,?,?,55.1,12.1,12,strftime('%Y-%m-%dT%H:%M:%fZ','now'),strftime('%Y-%m-%dT%H:%M:%fZ','now','+30 minutes'))").bind(owner.to_string()).bind(agent.to_string()).bind(channel.to_string()).bind(share.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,1,'bars near me','Test')").bind(source.to_string()).bind(channel.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,2,?,'Agent')").bind(reply.to_string()).bind(channel.to_string()).bind(agent.to_string()).bind(place_lookup::NEUTRAL_REPLY).execute(&pool).await.unwrap();
        sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,location_share_id,place_lookup_plan) values(?,?,?,?,1,'leased',0,0,?,?,?)").bind(job.to_string()).bind(agent.to_string()).bind(source.to_string()).bind(channel.to_string()).bind(place_lookup::NEUTRAL_REPLY).bind(share.to_string()).bind(r#"{"query":"bars","origin_mode":"shared"}"#).execute(&pool).await.unwrap();
        sqlx::query("insert into command_receipts(principal_id,request_id,message_id) values(?,?,?)").bind(agent.to_string()).bind(format!("circle-chat-agent:{job}")).bind(reply.to_string()).execute(&pool).await.unwrap();
        let service=CircleChatAgents {store:Store::Sqlite(pool.clone()),model:None,worker_enabled:false,weather:None,sea_temperature:None,ferry:None,observations:None,imagegen:None};
        let actor=UserId::from_uuid(owner);let outsider=UserId::from_uuid(other);
        let preview=service.geoapify_roll_preview(&actor,&channel.to_string(),&reply.to_string()).await.unwrap();
        assert!(service.geoapify_roll_preview(&outsider,&channel.to_string(),&reply.to_string()).await.is_err());
        assert!(service.consent_geoapify_roll(&actor,&channel.to_string(),&reply.to_string(),&preview.preview_binding,false,None).await.is_err());
        assert!(service.consent_geoapify_roll(&outsider,&channel.to_string(),&reply.to_string(),&preview.preview_binding,true,None).await.is_err());
        let view=service.consent_geoapify_roll(&actor,&channel.to_string(),&reply.to_string(),&preview.preview_binding,true,None).await.unwrap();
        assert_eq!(view.status,"pending");
        for _ in 0..5 {
            let busy=service.claim_geoapify_roll().await.unwrap().unwrap();
            service.fail_geoapify_roll(&busy,"model_busy").await.unwrap();
            let (attempts,state,eligible):(i64,String,i64)=sqlx::query_as("select attempts,state,leased_until from agent_place_rolls where id=?").bind(&view.id).fetch_one(&pool).await.unwrap();
            assert_eq!(attempts,0,"busy admission does not consume execution attempts");
            assert_eq!(state,"pending");
            assert!(eligible>Utc::now().timestamp());
            assert!(service.claim_geoapify_roll().await.unwrap().is_none(),"backoff prevents immediate reclaim");
            sqlx::query("update agent_place_rolls set leased_until=0 where id=?").bind(&view.id).execute(&pool).await.unwrap();
        }
        let claimed=service.claim_geoapify_roll().await.unwrap().unwrap();
        let sample=geoapify::PlaceSearch{places:vec![geoapify::Place{id:"id-1".into(),name:"Test pub".into(),address:None,point:geoapify::Point{latitude:55.1,longitude:12.1}}],potentially_truncated:false,radius_m:1500,attribution:geoapify::Attribution{osm_label:"© OpenStreetMap contributors",osm_url:"https://www.openstreetmap.org/copyright",provider_label:"Powered by Geoapify",provider_url:"https://www.geoapify.com/"}};
        service.save_geoapify_roll_result(&claimed,&sample,1).await.unwrap();
        service.fail_geoapify_roll(&claimed,"model_transport").await.unwrap();
        let retried=service.claim_geoapify_roll().await.unwrap().unwrap();
        assert_eq!(retried.outcome.as_ref().unwrap()["value"],1);
        assert_eq!(retried.outcome.as_ref().unwrap()["selected"]["name"],"Test pub");
        assert!(service.save_geoapify_roll_result(&retried,&sample,1).await.is_err(),"persisted outcome cannot be rolled again");
        let chat=ChatEngine::start(Arc::new(repository));
        // Simulate revocation after the worker's last check but before message
        // insertion; the DB transaction itself must reject the stale result.
        sqlx::query("delete from channel_memberships where channel_id=? and user_id=?").bind(channel.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        let stale_body:String=sqlx::query_scalar("select reply_body from agent_place_rolls where id=?").bind(&view.id).fetch_one(&pool).await.unwrap();
        let request=format!("circle-place-roll:{}",view.id);
        assert!(chat.send_message_idempotent(ChannelId::new(channel.to_string()).unwrap(),UserId::from_uuid(agent),MessageBody::new(stale_body).unwrap(),request.clone()).await.is_err());
        let blocked:i64=sqlx::query_scalar("select count(*) from command_receipts where request_id=?").bind(&request).fetch_one(&pool).await.unwrap();
        assert_eq!(blocked,0,"revoked access cannot produce a receipt");
        sqlx::query("insert into channel_memberships(channel_id,user_id,role) values(?,?,'member')").bind(channel.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        service.publish_geoapify_roll(&retried,&chat).await.unwrap();
        let receipt:String=sqlx::query_scalar("select cast(message_id as text) from command_receipts where request_id=?").bind(format!("circle-place-roll:{}",view.id)).fetch_one(&pool).await.unwrap();
        let stored:String=sqlx::query_scalar("select cast(result_message_id as text) from agent_place_rolls where id=?").bind(&view.id).fetch_one(&pool).await.unwrap();
        assert_eq!(receipt,stored);
        let count:i64=sqlx::query_scalar("select count(*) from command_receipts where request_id=?").bind(format!("circle-place-roll:{}",view.id)).fetch_one(&pool).await.unwrap();
        assert_eq!(count,1);

        // A crashed named-origin geocode must reuse exactly the original consent,
        // avoid duplicating a still-active call, and reclaim an expired lease.
        let (named_source,named_reply,named_job,named_roll)=(Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7());
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,4,'bars Bergen station','Test')").bind(named_source.to_string()).bind(channel.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,5,?,'Agent')").bind(named_reply.to_string()).bind(channel.to_string()).bind(agent.to_string()).bind(place_lookup::NEUTRAL_REPLY).execute(&pool).await.unwrap();
        let named_plan=r#"{"query":"bars","origin_mode":"named","named_origin":"Bergen station"}"#;
        sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,place_lookup_plan) values(?,?,?,?,1,'leased',0,0,?,?)").bind(named_job.to_string()).bind(agent.to_string()).bind(named_source.to_string()).bind(channel.to_string()).bind(place_lookup::NEUTRAL_REPLY).bind(named_plan).execute(&pool).await.unwrap();
        sqlx::query("insert into command_receipts(principal_id,request_id,message_id) values(?,?,?)").bind(agent.to_string()).bind(format!("circle-chat-agent:{named_job}")).bind(named_reply.to_string()).execute(&pool).await.unwrap();
        let descriptor=service.place_lookup_for_message(&actor,&channel.to_string(),&named_reply.to_string()).await.unwrap().unwrap();
        let named_preview=service.geoapify_roll_preview(&actor,&channel.to_string(),&named_reply.to_string()).await.unwrap();
        sqlx::query("insert into agent_place_rolls(id,source_job_id,original_reply_id,author_id,descriptor_snapshot,plan_snapshot,state,origin_query,expires_at,lease_token,leased_until) values(?,?,?,?,?,?,'geocoding','Bergen station',?,?,?)")
            .bind(named_roll.to_string()).bind(named_job.to_string()).bind(named_reply.to_string()).bind(owner.to_string()).bind(serde_json::to_string(&descriptor).unwrap()).bind(named_plan)
            .bind(Utc::now().timestamp()+900).bind("existing-token").bind(Utc::now().timestamp()+30).execute(&pool).await.unwrap();
        assert!(service.consent_geoapify_roll(&actor,&channel.to_string(),&named_reply.to_string(),&named_preview.preview_binding,false,Some("Different station")).await.is_err());
        let active=service.consent_geoapify_roll(&actor,&channel.to_string(),&named_reply.to_string(),&named_preview.preview_binding,false,Some("Bergen station")).await.unwrap();
        assert_eq!(active.status,"geocoding");
        let token:String=sqlx::query_scalar("select lease_token from agent_place_rolls where id=?").bind(named_roll.to_string()).fetch_one(&pool).await.unwrap();
        assert_eq!(token,"existing-token","active retry does not issue a second provider call");
        sqlx::query("update agent_place_rolls set leased_until=0 where id=?").bind(named_roll.to_string()).execute(&pool).await.unwrap();
        let failed=service.consent_geoapify_roll(&actor,&channel.to_string(),&named_reply.to_string(),&named_preview.preview_binding,false,Some("Bergen station")).await.unwrap();
        assert_eq!(failed.status,"failed");
        assert_eq!(failed.error_code.as_deref(),Some("geocode_failed"));
        assert!(failed.can_retry_geocoding);
        let lease:Option<String>=sqlx::query_scalar("select lease_token from agent_place_rolls where id=?").bind(named_roll.to_string()).fetch_one(&pool).await.unwrap();
        assert!(lease.is_none(),"failed reclaimed request releases the lease");
        sqlx::query("update agent_place_rolls set expires_at=0 where id=?").bind(named_roll.to_string()).execute(&pool).await.unwrap();
        assert!(service.claim_geoapify_roll().await.unwrap().is_none());
        let (stored_origin,state):(Option<String>,String)=sqlx::query_as("select origin_query,state from agent_place_rolls where id=?").bind(named_roll.to_string()).fetch_one(&pool).await.unwrap();
        assert!(stored_origin.is_none(),"expiry clears the named origin");
        assert_eq!(state,"cancelled");
    }))).await;
}

#[test]
fn external_names_cannot_inject_markdown_macros_links_or_mentions() {
    let input = "[[media:123]] [x](https://evil.invalid) @agent <script> `code`";
    let output = safe_place_name(input);
    for dangerous in ["[[", "]]", "[x]", "](", "://", "@agent", "<script>", "`"] {
        assert!(
            !output.contains(dangerous),
            "external venue name contained {dangerous}"
        );
    }
}

#[tokio::test]
async fn postgres_geoapify_roll_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    geoapify::TEST_ENABLED.scope(true,place_lookup::TEST_ENABLED.scope(true,location::TEST_ENABLED.scope(true,async {
        let pool=PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations/postgres").run(&pool).await.unwrap();
        let repository=crate::db::PostgresChatRepository::connect_with_pool(&url,pool.clone()).await.unwrap();
        let (owner,other,agent,circle,channel,source,reply,job,share)=(Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7(),Uuid::now_v7());
        sqlx::query("insert into users(id,kind,display_name) values($1,'human','Test owner'),($2,'human','Other'),($3,'agent','Agent')").bind(owner).bind(other).bind(agent).execute(&pool).await.unwrap();
        sqlx::query("insert into circles(id,slug,name,created_by) values($1,$2,'Geo roll',$3)").bind(circle).bind(format!("geo-roll-{circle}")).bind(owner).execute(&pool).await.unwrap();
        sqlx::query("insert into circle_memberships(circle_id,user_id,role) values($1,$2,'owner'),($1,$3,'member')").bind(circle).bind(owner).bind(other).execute(&pool).await.unwrap();
        sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values($1,$2,'Geo roll','public',$3,$4)").bind(channel).bind(format!("geo-roll-{channel}")).bind(owner).bind(circle).execute(&pool).await.unwrap();
        sqlx::query("insert into channel_memberships(channel_id,user_id,role) values($1,$2,'owner'),($1,$3,'member')").bind(channel).bind(owner).bind(other).execute(&pool).await.unwrap();
        sqlx::query("insert into channel_sequences(channel_id,next_sequence) values($1,3)").bind(channel).execute(&pool).await.unwrap();
        sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values($1,$2,$2,'test',$3,'test',60,clock_timestamp())").bind(agent).bind(owner).bind(format!("geo-roll-{agent}")).execute(&pool).await.unwrap();
        sqlx::query("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values($1,$2,'[]','[]',true,$3,$3,0,0)").bind(agent).bind(circle).bind(owner).execute(&pool).await.unwrap();
        sqlx::query("insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) values($1,$2,$3,$4,55.1,12.1,12,clock_timestamp(),clock_timestamp()+interval '30 minutes')").bind(owner).bind(agent).bind(channel).bind(share).execute(&pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values($1,$2,$3,1,'bars near me','Test')").bind(source).bind(channel).bind(owner).execute(&pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values($1,$2,$3,2,$4,'Agent')").bind(reply).bind(channel).bind(agent).bind(place_lookup::NEUTRAL_REPLY).execute(&pool).await.unwrap();
        sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,location_share_id,place_lookup_plan) values($1,$2,$3,$4,1,'leased',0,0,$5,$6,$7)").bind(job).bind(agent).bind(source).bind(channel).bind(place_lookup::NEUTRAL_REPLY).bind(share).bind(r#"{"query":"bars","origin_mode":"shared"}"#).execute(&pool).await.unwrap();
        sqlx::query("insert into command_receipts(principal_id,request_id,message_id) values($1,$2,$3)").bind(agent).bind(format!("circle-chat-agent:{job}")).bind(reply).execute(&pool).await.unwrap();
        let service=CircleChatAgents{store:Store::Pg(pool.clone()),model:None,worker_enabled:false,weather:None,sea_temperature:None,ferry:None,observations:None,imagegen:None};
        let actor=UserId::from_uuid(owner);
        let preview=service.geoapify_roll_preview(&actor,&channel.to_string(),&reply.to_string()).await.unwrap();
        let view=service.consent_geoapify_roll(&actor,&channel.to_string(),&reply.to_string(),&preview.preview_binding,true,None).await.unwrap();
        let leased=service.claim_geoapify_roll().await.unwrap().unwrap();
        let sample=geoapify::PlaceSearch{places:vec![geoapify::Place{id:"id-1".into(),name:"Test pub".into(),address:None,point:geoapify::Point{latitude:55.1,longitude:12.1}}],potentially_truncated:false,radius_m:1500,attribution:geoapify::Attribution{osm_label:"© OpenStreetMap contributors",osm_url:"https://www.openstreetmap.org/copyright",provider_label:"Powered by Geoapify",provider_url:"https://www.geoapify.com/"}};
        service.save_geoapify_roll_result(&leased,&sample,1).await.unwrap();
        service.fail_geoapify_roll(&leased,"model_transport").await.unwrap();
        let retried=service.claim_geoapify_roll().await.unwrap().unwrap();
        assert_eq!(retried.outcome.as_ref().unwrap()["value"],1);
        let body:String=sqlx::query_scalar("select reply_body from agent_place_rolls where id=$1::text::uuid").bind(&view.id).fetch_one(&pool).await.unwrap();
        let command=SendMessage{actor:UserId::from_uuid(agent),channel_id:ChannelId::new(channel.to_string()).unwrap(),parent_message_id:None,body:MessageBody::new(body).unwrap()};
        let request=format!("circle-place-roll:{}",view.id);
        let mut transaction=pool.begin().await.unwrap();
        authorize_postgres(&mut transaction,&command,&request).await.unwrap();
        let revoke_pool=pool.clone();
        let mut revoke=tokio::spawn(async move {sqlx::query("delete from agent_location_shares where share_id=$1").bind(share).execute(&revoke_pool).await.unwrap();});
        assert!(tokio::time::timeout(Duration::from_millis(100),&mut revoke).await.is_err(),"revocation must wait for transaction's share lock");
        transaction.rollback().await.unwrap();
        revoke.await.unwrap();
        let chat=ChatEngine::start(Arc::new(repository));
        assert!(chat.send_message_idempotent(command.channel_id.clone(),command.actor.clone(),command.body.clone(),request.clone()).await.is_err());
        let count:i64=sqlx::query_scalar("select count(*) from command_receipts where request_id=$1").bind(&request).fetch_one(&pool).await.unwrap();
        assert_eq!(count,0,"revocation must roll back message and receipt");
        sqlx::query("insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) values($1,$2,$3,$4,55.1,12.1,12,clock_timestamp(),clock_timestamp()+interval '30 minutes')").bind(owner).bind(agent).bind(channel).bind(share).execute(&pool).await.unwrap();
        service.publish_geoapify_roll(&retried,&chat).await.unwrap();
        let receipt:Uuid=sqlx::query_scalar("select message_id from command_receipts where request_id=$1").bind(&request).fetch_one(&pool).await.unwrap();
        let stored:Uuid=sqlx::query_scalar("select result_message_id from agent_place_rolls where id=$1::text::uuid").bind(&view.id).fetch_one(&pool).await.unwrap();
        assert_eq!(receipt,stored);
        assert_eq!(sqlx::query_scalar::<_,i64>("select count(*) from command_receipts where request_id=$1").bind(&request).fetch_one(&pool).await.unwrap(),1);
    }))).await;
}
