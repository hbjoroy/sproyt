//! Transactional capture and shared authorization for durable memory work.
use crate::chatbot::sql;

pub(crate) fn collection_enabled() -> bool {
    super::MemoryGates::from_lookup(|key| std::env::var(key).ok()).collect
}

/// Authority query shared by capture and builder. Call with profile/channel IDs;
/// PostgreSQL locks authority rows so a concurrent revocation fences the write.
pub(crate) fn eligible_scope_query(pg: bool, lock: bool) -> String {
    let query = "select cast(p.id as text) as profile_id,cast(p.circle_id as text) as circle_id,cast(p.agent_id as text) as agent_id,cast(p.user_id as text) as user_id,p.memory_epoch from agent_memory_profiles p join circle_chat_agents a on a.agent_id=p.agent_id and a.circle_id=p.circle_id join agent_profiles ap on ap.agent_id=p.agent_id join users u on u.id=p.user_id join circle_memberships cm on cm.circle_id=p.circle_id and cm.user_id=p.user_id join channels c on c.circle_id=p.circle_id join channel_memberships chm on chm.channel_id=c.id and chm.user_id=p.user_id where p.id=?uuid and c.id=?uuid and p.enabled=true and a.enabled=true and a.memory_enabled=true and u.kind='human' and ap.revoked_at is null and (ap.expires_at is null or ap.expires_at>current_timestamp) and c.kind!='direct' and coalesce((select s.enabled from channel_chat_agent_settings s where s.channel_id=c.id and s.agent_id=p.agent_id),c.kind!='private')";
    let query = if pg {
        query.to_owned()
    } else {
        query.replace(
            "ap.expires_at>current_timestamp",
            "datetime(ap.expires_at)>current_timestamp",
        )
    };
    sql(&query, pg)
        + if pg && lock {
            " for share of a,ap,u,cm,c,chm for update of p"
        } else {
            ""
        }
}

macro_rules! mark_message {
    ($tx:expr,$pg:expr,$message:expr) => {{
        if crate::chatbot::memory::collection::collection_enabled() {
            crate::chatbot::memory::collection::capture_message!($tx, $pg, $message);
        }
    }};
}
pub(crate) use mark_message;

// Separate gate-free primitive permits deterministic adapter contract tests.
macro_rules! capture_message {
    ($tx:expr,$pg:expr,$message:expr) => {{
        use sqlx::Row;
        use crate::chatbot::{sql,storage};
        let candidates=sqlx::query(&sql("select cast(p.id as text) as id,cast(m.channel_id as text) as channel_id,m.sequence from messages m join users u on u.id=m.sender_id join message_provenance mp on mp.message_id=m.id join channels c on c.id=m.channel_id join agent_memory_profiles p on p.circle_id=c.circle_id and p.user_id=m.sender_id where m.id=?uuid and m.deleted_at is null and u.kind='human' and mp.provenance='human' and p.enabled=true order by p.agent_id limit 10",$pg))
            .bind($message.to_string()).fetch_all(&mut *$tx).await.map_err(storage)?;
        for row in candidates {
            let profile:String=row.try_get("id").map_err(storage)?;
            let channel:String=row.try_get("channel_id").map_err(storage)?;
            let sequence:i64=row.try_get("sequence").map_err(storage)?;
            let authority=sqlx::query(&crate::chatbot::memory::collection::eligible_scope_query($pg,true))
                .bind(&profile).bind(&channel).fetch_optional(&mut *$tx).await.map_err(storage)?;
            if authority.is_none() {continue;}
            let now=chrono::Utc::now().timestamp().to_string();
            let maximum=if $pg {"greatest"} else {"max"};
            let query=format!("insert into agent_memory_scopes(profile_id,circle_id,channel_id,start_sequence,processed_sequence,dirty_sequence,available_at) select id,circle_id,?uuid,?int,?int,?int,?int from agent_memory_profiles where id=?uuid and (exists(select 1 from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid) or (select count(*) from agent_memory_scopes where profile_id=?uuid)<64) on conflict(profile_id,channel_id) do update set dirty_sequence={maximum}(agent_memory_scopes.dirty_sequence,excluded.dirty_sequence) where excluded.dirty_sequence>agent_memory_scopes.start_sequence");
            let captured=sqlx::query(&sql(&query,$pg)).bind(&channel).bind((sequence-1).to_string()).bind((sequence-1).to_string()).bind(sequence.to_string()).bind(&now).bind(&profile).bind(&profile).bind(&channel).bind(&profile)
                .execute(&mut *$tx).await.map_err(storage)?;
            if captured.rows_affected()==0 {continue;}
            sqlx::query(&sql("update agent_memory_profiles set collection_started_at=coalesce(collection_started_at,?int) where id=?uuid",$pg))
                .bind(&now).bind(&profile).execute(&mut *$tx).await.map_err(storage)?;
        }
    }};
}
pub(crate) use capture_message;
