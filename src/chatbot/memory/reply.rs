//! Bounded reply memory and publication fences, independent of runtime use flag.
use crate::chatbot::Result;
use crate::chatbot::{Job, Store, sql, storage};
use crate::domain::RepositoryError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const PROMPT_BYTES: usize = 4096;
#[derive(Serialize, Deserialize)]
struct Dependencies {
    user: String,
    circle: String,
    agent: String,
    channel: String,
    epoch: i64,
    notes: Vec<Value>,
    sources: Vec<(String, String, String)>,
}

fn select(
    view: super::repository::MemoryView,
    user: String,
    channel: String,
    eligible_channels: &std::collections::HashSet<String>,
    target: &str,
) -> Option<Dependencies> {
    if !view.enabled || !view.agent_enabled {
        return None;
    }
    let mut notes = Vec::new();
    let words: std::collections::HashSet<String> = target
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect();
    let mut candidates: Vec<_> = view
        .notes
        .into_iter()
        .filter(|n| eligible_channels.contains(&n.channel_id.to_string()))
        .collect();
    candidates.sort_by_key(|n| {
        let text = serde_json::to_value(&n.content)
            .ok()
            .and_then(|v| v["text"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let overlap = text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| words.contains(&w.to_lowercase()))
            .count();
        (
            std::cmp::Reverse(overlap),
            std::cmp::Reverse(n.updated_at),
            n.id,
        )
    });
    for note in candidates.into_iter().take(6) {
        let value = serde_json::to_value(note).ok()?;
        let mut candidate = notes.clone();
        candidate.push(value.clone());
        if serde_json::to_vec(&candidate).ok()?.len() > PROMPT_BYTES - 256 {
            break;
        }
        notes.push(value);
    }
    if notes.is_empty() {
        return None;
    }
    Some(Dependencies {
        user,
        circle: view.circle_id.to_string(),
        agent: view.agent_id.to_string(),
        channel,
        epoch: view.memory_epoch,
        notes,
        sources: Vec::new(),
    })
}

// Query current channel state, including at publication: private evidence may
// stay in its own channel only. Open evidence may follow its owner within the
// same circle, including into private channels. read_memory! still checks every
// source, owner membership, agent access and forgotten/expired evidence.
macro_rules! eligible_channels {
    ($tx:expr,$pg:expr,$job:expr,$user:expr,$circle:expr,$agent:expr,$channel:expr) => {{
        let ids: Vec<String> = sqlx::query_scalar(&sql("select cast(c.id as text) from channels c join channels destination on destination.circle_id=c.circle_id join circle_chat_agent_jobs j on j.channel_id=destination.id join messages target on target.id=j.source_message_id where j.id=?uuid and target.sender_id=?uuid and destination.circle_id=?uuid and j.agent_id=?uuid and destination.id=?uuid and destination.kind!='direct' and exists(select 1 from channel_memberships cm where cm.channel_id=destination.id and cm.user_id=target.sender_id) and coalesce((select s.enabled from channel_chat_agent_settings s where s.channel_id=destination.id and s.agent_id=j.agent_id),destination.kind!='private') and (c.id=destination.id or c.kind in ('public','local'))",$pg))
            .bind(&$job).bind(&$user).bind(&$circle).bind(&$agent).bind(&$channel)
            .fetch_all(&mut *$tx).await.map_err(storage)?;
        ids.into_iter().collect::<std::collections::HashSet<_>>()
    }};
}

macro_rules! sources {
    ($tx:expr,$pg:expr,$deps:expr) => {{
        use sqlx::Row;
        let mut entries=Vec::new();
        for note in &$deps.notes {
            let id=note["id"].as_str().ok_or(RepositoryError::PermissionDenied)?;
            let rows=sqlx::query(&sql("select cast(message_id as text) as id,source_version from agent_memory_note_sources where note_id=?uuid order by message_id",$pg)).bind(id).fetch_all(&mut *$tx).await.map_err(storage)?;
            for row in rows { entries.push((id.to_owned(),row.try_get::<String,_>("id").map_err(storage)?,row.try_get::<String,_>("source_version").map_err(storage)?)); }
        }
        entries
    }};
}
macro_rules! snapshot_tx {
    ($tx:ident,$pg:expr,$job:expr) => {{
        use sqlx::Row;
        let row=sqlx::query(&sql("select cast(m.sender_id as text) as owner,m.body as target,cast(c.circle_id as text) as circle from circle_chat_agent_jobs j join messages m on m.id=j.source_message_id join users u on u.id=m.sender_id join channels c on c.id=j.channel_id join message_provenance v on v.message_id=m.id where j.id=?uuid and u.kind='human' and v.provenance='human' and m.deleted_at is null and m.edited_at is null",$pg)).bind(&$job.id).fetch_optional(&mut *$tx).await.map_err(storage)?;
        if let Some(row)=row {
            let owner:String=row.try_get("owner").map_err(storage)?;
            let circle:String=row.try_get("circle").map_err(storage)?;
            let view=super::repository::read_memory!($tx,$pg,owner,circle,$job.agent_id);
            let target:String=row.try_get("target").map_err(storage)?;
            let eligible=eligible_channels!($tx,$pg,$job.id,owner,circle,$job.agent_id,$job.channel_id);
            let mut selected=select(view,owner,$job.channel_id.clone(),&eligible,&target);
            if let Some(deps)=&mut selected { deps.sources=sources!($tx,$pg,deps); }
            selected
        } else { None }
    }};
}

pub(in crate::chatbot) async fn snapshot(store: &Store, job: &Job) -> Result<Option<Value>> {
    let deps = match store {
        Store::Pg(pool) => {
            let mut tx = pool.begin().await.map_err(storage)?;
            let d = snapshot_tx!(tx, true, job);
            tx.commit().await.map_err(storage)?;
            d
        }
        Store::Sqlite(pool) => {
            let mut tx = pool.begin().await.map_err(storage)?;
            let d = snapshot_tx!(tx, false, job);
            tx.commit().await.map_err(storage)?;
            d
        }
    };
    let serialized = deps
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(storage)?
        .unwrap_or_default();
    if store.execute("update circle_chat_agent_jobs set memory_dependencies=nullif(?,'') where id=?uuid and lease_token=?uuid and status='leased' and reply_body is null",&[serialized,job.id.clone(),job.lease_token.clone()]).await?!=1 { return Err(RepositoryError::Conflict); }
    Ok(deps.map(|d| json!({"target_user_id":d.user,"channel_id":d.channel,"notes":d.notes})))
}

macro_rules! authorize {
    ($tx:ident,$id:expr,$pg:expr) => {{
        let raw: Option<String> = sqlx::query_scalar(&sql(
            "select memory_dependencies from circle_chat_agent_jobs where id=?uuid",
            $pg,
        ))
        .bind($id.to_string())
        .fetch_one(&mut **$tx)
        .await
        .map_err(storage)?;
        if let Some(raw) = raw {
            let deps: Dependencies =
                serde_json::from_str(&raw).map_err(|_| RepositoryError::PermissionDenied)?;
            let view =
                super::repository::read_memory!(*$tx, $pg, deps.user, deps.circle, deps.agent);
            if !view.enabled || !view.agent_enabled || view.memory_epoch != deps.epoch {
                return Err(RepositoryError::PermissionDenied);
            }
            if sources!(*$tx, $pg, deps) != deps.sources {
                return Err(RepositoryError::PermissionDenied);
            }
            let eligible = eligible_channels!(
                *$tx,
                $pg,
                $id,
                deps.user,
                deps.circle,
                deps.agent,
                deps.channel
            );
            for expected in &deps.notes {
                if !view.notes.iter().any(|n| {
                    eligible.contains(&n.channel_id.to_string())
                        && serde_json::to_value(n).ok().as_ref() == Some(expected)
                }) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
        }
    }};
}
/// Evidence -> authority -> profile matches source-edit and membership hooks.
pub(crate) async fn prepare_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
) -> Result<()> {
    let raw: Option<String> = sqlx::query_scalar(
        "select memory_dependencies from circle_chat_agent_jobs where id=$1::text::uuid",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .ok_or(RepositoryError::PermissionDenied)?;
    let deps = raw
        .as_ref()
        .map(|s| serde_json::from_str::<Dependencies>(s))
        .transpose()
        .map_err(|_| RepositoryError::PermissionDenied)?;
    let evidence: Vec<uuid::Uuid> = deps
        .as_ref()
        .map(|d| {
            d.sources
                .iter()
                .map(|(_, id, _)| uuid::Uuid::parse_str(id))
                .collect()
        })
        .transpose()
        .map_err(|_| RepositoryError::PermissionDenied)?
        .unwrap_or_default();
    // Include target/anchor evidence in the same ordered lock pass. Locking
    // the job first would invert physical source deletion's FK cascade.
    sqlx::query("select m.id from messages m where m.id=any($2) or m.id in (select j.source_message_id from circle_chat_agent_jobs j where j.id=$1::text::uuid union select j.followup_anchor_message_id from circle_chat_agent_jobs j where j.id=$1::text::uuid union select previous.source_message_id from circle_chat_agent_jobs previous join command_receipts r on r.principal_id=previous.agent_id and r.request_id='circle-chat-agent:' || cast(previous.id as text) join circle_chat_agent_jobs j on j.followup_anchor_message_id=r.message_id where j.id=$1::text::uuid) order by m.id for share of m").bind(id).bind(evidence).fetch_all(&mut **tx).await.map_err(storage)?;
    let current: Option<String> = sqlx::query_scalar(
        "select memory_dependencies from circle_chat_agent_jobs where id=$1::text::uuid for share",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .ok_or(RepositoryError::PermissionDenied)?;
    if current != raw {
        return Err(RepositoryError::PermissionDenied);
    }
    if let Some(deps) = deps.as_ref() {
        let channels: Vec<uuid::Uuid> = deps
            .notes
            .iter()
            .map(|note| {
                note["channel_id"]
                    .as_str()
                    .ok_or(RepositoryError::PermissionDenied)
                    .and_then(|id| {
                        uuid::Uuid::parse_str(id).map_err(|_| RepositoryError::PermissionDenied)
                    })
            })
            .collect::<Result<_>>()?;
        sqlx::query("select cm.user_id from circle_memberships cm where cm.circle_id in (select c.circle_id from circle_chat_agent_jobs j join channels c on c.id=j.channel_id where j.id=$1::text::uuid) order by cm.user_id for share of cm").bind(id).fetch_all(&mut **tx).await.map_err(storage)?;
        // Lock source and destination together, before channel memberships and
        // the memory profile, matching channel access changes' lock order.
        sqlx::query("select c.id from channels c where c.id=any($2) or c.id in(select j.channel_id from circle_chat_agent_jobs j where j.id=$1::text::uuid) order by c.id for share of c").bind(id).bind(&channels).fetch_all(&mut **tx).await.map_err(storage)?;
        sqlx::query("select cm.user_id from channel_memberships cm where cm.channel_id=any($2) or cm.channel_id in (select j.channel_id from circle_chat_agent_jobs j where j.id=$1::text::uuid) order by cm.channel_id,cm.user_id for share of cm").bind(id).bind(&channels).fetch_all(&mut **tx).await.map_err(storage)?;
        sqlx::query("select p.agent_id from agent_profiles p join circle_chat_agent_jobs j on j.agent_id=p.agent_id where j.id=$1::text::uuid for share of p").bind(id).fetch_all(&mut **tx).await.map_err(storage)?;
    }
    Ok(())
}
pub(crate) async fn authorize_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
) -> Result<()> {
    prepare_postgres(tx, id).await?;
    // User mutations and builder writes serialize on the profile. Do not
    // take note/source locks: those would invert source edits and deletion.
    sqlx::query("select p.id from agent_memory_profiles p join circle_chat_agent_jobs j on j.agent_id=p.agent_id join messages m on m.id=j.source_message_id and m.sender_id=p.user_id where j.id=$1::text::uuid for share of p").bind(id).fetch_all(&mut **tx).await.map_err(storage)?;
    authorize!(tx, id, true);
    Ok(())
}
pub(crate) async fn authorize_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> Result<()> {
    authorize!(tx, id, false);
    Ok(())
}
