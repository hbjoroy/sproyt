//! Owner-only memory storage. No model or collection worker runs in M1.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{EvidenceKind, NoteKind, NoteOrigin, NoteText};
use crate::domain::RepositoryError;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoteContent {
    pub text: NoteText,
    #[serde(default)]
    pub participant_ids: Vec<Uuid>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct NoteView {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub kind: NoteKind,
    pub content: NoteContent,
    pub origin: NoteOrigin,
    pub evidence: EvidenceKind,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub expires_at: Option<i64>,
    pub source_message_ids: Vec<Uuid>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct MemoryView {
    pub circle_id: Uuid,
    pub agent_id: Uuid,
    pub enabled: bool,
    pub agent_enabled: bool,
    pub collection_available: bool,
    pub collection_started_at: Option<i64>,
    pub revision: i64,
    pub memory_epoch: i64,
    pub history_compactions: i64,
    pub notes: Vec<NoteView>,
    pub unavailable_notes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChoiceInput {
    pub revision: i64,
    pub enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct ActionInput {
    pub revision: i64,
    #[serde(flatten)]
    pub action: MemoryAction,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MemoryAction {
    Correct { note_id: Uuid, text: NoteText },
    Confirm { note_id: Uuid },
    Forget { note_id: Uuid },
    Reset,
}

pub(crate) enum Mutation {
    Choice(ChoiceInput),
    Action(ActionInput),
}

impl Mutation {
    fn revision(&self) -> i64 {
        match self {
            Self::Choice(input) => input.revision,
            Self::Action(input) => input.revision,
        }
    }
}

// Authority is always the profile's user, never a manager-supplied user ID.
pub(crate) const PROFILE_QUERY: &str = "select cast(p.id as text) as id,cast(p.circle_id as text) as circle_id,cast(p.agent_id as text) as agent_id,p.enabled,p.collection_started_at,p.revision,p.memory_epoch,p.history_compactions,a.memory_enabled as agent_enabled from agent_memory_profiles p join circle_chat_agents a on a.agent_id=p.agent_id and a.circle_id=p.circle_id join circle_memberships cm on cm.circle_id=p.circle_id and cm.user_id=p.user_id join users u on u.id=p.user_id where p.user_id=?uuid and u.kind='human' and p.circle_id=?uuid and p.agent_id=?uuid";

// Only produce a note after both channel access and every source are valid.
// Off/paused memory remains inspectable; revoked agent access does not.
pub(crate) fn visible_notes_query(pg: bool) -> String {
    let query = "select cast(n.id as text) as id,cast(n.channel_id as text) as channel_id,n.kind,cast(n.content as text) as content,n.origin,n.evidence,n.revision,n.created_at,n.updated_at,n.expires_at from agent_memory_notes n join agent_memory_profiles p on p.id=n.profile_id join channels c on c.id=n.channel_id and c.circle_id=p.circle_id join agent_profiles ap on ap.agent_id=p.agent_id where p.id=?uuid and (n.expires_at is null or n.expires_at>?int) and ap.revoked_at is null and (ap.expires_at is null or ap.expires_at>current_timestamp) and c.kind!='direct' and exists(select 1 from channel_memberships cm where cm.channel_id=c.id and cm.user_id=p.user_id) and coalesce((select s.enabled from channel_chat_agent_settings s where s.channel_id=c.id and s.agent_id=p.agent_id),c.kind!='private') order by n.created_at,n.id limit 25";
    if pg {
        query.into()
    } else {
        query.replace(
            "ap.expires_at>current_timestamp",
            "datetime(ap.expires_at)>current_timestamp",
        )
    }
}

pub(crate) fn decode_enum<T: serde::de::DeserializeOwned>(
    value: String,
) -> Result<T, RepositoryError> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(crate::chatbot::storage)
}

// Macro allows both adapters and the account exporter to use the exact same
// snapshot, without opening another transaction or a second pool connection.
macro_rules! read_memory {
    ($tx:expr, $pg:expr, $actor:expr, $circle:expr, $agent:expr) => {{
        use sqlx::Row;
        use crate::chatbot::{sql, storage};
        use crate::chatbot::memory::repository as memory;
        let profile = sqlx::query(&sql(memory::PROFILE_QUERY,$pg))
            .bind($actor.to_string()).bind($circle.to_string()).bind($agent.to_string())
            .fetch_optional(&mut *$tx).await.map_err(storage)?;
        let mut view = memory::MemoryView {
            circle_id: uuid::Uuid::parse_str(&$circle.to_string()).map_err(storage)?,
            agent_id: uuid::Uuid::parse_str(&$agent.to_string()).map_err(storage)?,
            enabled: false, agent_enabled: false, collection_available: false,
            collection_started_at: None, revision: 0, memory_epoch: 1,
            history_compactions: 0, notes: Vec::new(), unavailable_notes: 0,
        };
        if let Some(profile) = profile {
            let id: String = profile.try_get("id").map_err(storage)?;
            view.enabled = profile.try_get("enabled").map_err(storage)?;
            view.agent_enabled = profile.try_get("agent_enabled").map_err(storage)?;
            view.collection_started_at = profile.try_get("collection_started_at").map_err(storage)?;
            view.revision = profile.try_get("revision").map_err(storage)?;
            view.memory_epoch = profile.try_get("memory_epoch").map_err(storage)?;
            view.history_compactions = profile.try_get("history_compactions").map_err(storage)?;
            let total: i64 = sqlx::query_scalar(&sql("select count(*) from agent_memory_notes where profile_id=?uuid",$pg))
                .bind(&id).fetch_one(&mut *$tx).await.map_err(storage)?;
            let rows = sqlx::query(&sql(&memory::visible_notes_query($pg),$pg))
                .bind(&id).bind(chrono::Utc::now().timestamp().to_string())
                .fetch_all(&mut *$tx).await.map_err(storage)?;
            if rows.len()>crate::chatbot::memory::MAX_PROFILE_NOTES {
                return Err(storage("memory note budget exceeded"));
            }
            for row in rows {
                let note_id: String = row.try_get("id").map_err(storage)?;
                let source_json = crate::chatbot::context_message_json("m",$pg);
                let source_json = if $pg {format!("({source_json})->'source'")} else {format!("json_extract({source_json},'$.source')")};
                let query = format!("select cast({source_json} as text) as source,m.body,s.source_version,exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=m.sender_id) as participant_present,exists(select 1 from agent_memory_exclusions e join agent_memory_notes n on n.profile_id=e.profile_id where n.id=s.note_id and e.message_id=s.message_id) as excluded from agent_memory_note_sources s left join messages m on m.id=s.message_id left join channels c on c.id=m.channel_id where s.note_id=?uuid order by s.message_id limit 31");
                let sources = sqlx::query(&sql(&query,$pg)).bind(&note_id)
                    .fetch_all(&mut *$tx).await.map_err(storage)?;
                let mut valid = !sources.is_empty() && sources.len() <= 30;
                let mut source_ids = Vec::new();
                let mut participants = std::collections::HashSet::new();
                let mut has_owner = false;
                for source in sources {
                    let raw: String = source.try_get("source").map_err(storage)?;
                    let body: Option<String> = source.try_get("body").map_err(storage)?;
                    let expected: String = source.try_get("source_version").map_err(storage)?;
                    valid &= source.try_get::<bool,_>("participant_present").map_err(storage)?
                        && !source.try_get::<bool,_>("excluded").map_err(storage)?;
                    match (serde_json::from_str::<crate::chatbot::memory::SourceMetadata>(&raw), body) {
                        (Ok(mut metadata),Some(body)) => {
                            metadata.seal(&body).map_err(storage)?;
                            let channel: String = row.try_get("channel_id").map_err(storage)?;
                            valid &= metadata.is_human_evidence()
                                && metadata.circle_id == Some(view.circle_id)
                                && metadata.channel_id.to_string()==channel
                                && metadata.version.map(String::from).as_deref()==Some(expected.as_str());
                            source_ids.push(metadata.message_id);
                            participants.insert(metadata.sender_id);
                            has_owner |= metadata.sender_id.to_string()==$actor.to_string();
                        },
                        _ => valid=false,
                    }
                }
                if !valid || !has_owner { continue; }
                let content: String = row.try_get("content").map_err(storage)?;
                let content: memory::NoteContent=serde_json::from_str(&content).map_err(storage)?;
                if content.participant_ids.len()>30 || !content.participant_ids.iter().all(|id|participants.contains(id)) {continue;}
                view.notes.push(memory::NoteView {
                    id: uuid::Uuid::parse_str(&note_id).map_err(storage)?,
                    channel_id: uuid::Uuid::parse_str(&row.try_get::<String,_>("channel_id").map_err(storage)?).map_err(storage)?,
                    kind: memory::decode_enum(row.try_get("kind").map_err(storage)?)?,
                    content,
                    origin: memory::decode_enum(row.try_get("origin").map_err(storage)?)?,
                    evidence: memory::decode_enum(row.try_get("evidence").map_err(storage)?)?,
                    revision: row.try_get("revision").map_err(storage)?,
                    created_at: row.try_get("created_at").map_err(storage)?,
                    updated_at: row.try_get("updated_at").map_err(storage)?,
                    expires_at: row.try_get("expires_at").map_err(storage)?,
                    source_message_ids: source_ids,
                });
            }
            view.unavailable_notes = usize::try_from(total).map_err(storage)?.saturating_sub(view.notes.len());
        }
        view
    }};
}
pub(crate) use read_memory;

macro_rules! export_memory {
    ($tx:expr,$pg:expr,$actor:expr) => {{
        use crate::chatbot::{sql,storage};
        use crate::chatbot::memory::repository as memory;
        use sqlx::Row;
        let rows = sqlx::query(&sql("select cast(p.circle_id as text) as circle_id,cast(p.agent_id as text) as agent_id from agent_memory_profiles p join circle_memberships cm on cm.circle_id=p.circle_id and cm.user_id=p.user_id join users u on u.id=p.user_id where p.user_id=?uuid and u.kind='human' order by p.circle_id,p.agent_id",$pg))
            .bind($actor.to_string()).fetch_all(&mut *$tx).await.map_err(storage)?;
        let mut output=Vec::new();
        for row in rows {
            let circle: String=row.try_get("circle_id").map_err(storage)?;
            let agent: String=row.try_get("agent_id").map_err(storage)?;
            let view=memory::read_memory!($tx,$pg,$actor,circle,agent);
            output.push(serde_json::to_value(view).map_err(storage)?);
        }
        output
    }};
}
pub(crate) use export_memory;

macro_rules! authorize {
    ($tx:expr,$pg:expr,$actor:expr,$circle:expr,$agent:expr,$lock:expr) => {{
        let query=crate::chatbot::sql("select cm.role from circle_memberships cm join users u on u.id=cm.user_id where cm.circle_id=?uuid and cm.user_id=?uuid and u.kind='human'",$pg)
            + if $pg && $lock {" for share of cm,u"} else {""};
        let role: Option<String>=sqlx::query_scalar(&query).bind($circle).bind($actor.to_string())
            .fetch_optional(&mut *$tx).await.map_err(memory_error)?;
        if role.is_none() {return Err(RepositoryError::PermissionDenied);}
        let query=crate::chatbot::sql("select cast(agent_id as text) from circle_chat_agents where circle_id=?uuid and agent_id=?uuid",$pg)
            + if $pg && $lock {" for share"} else {""};
        let agent:Option<String>=sqlx::query_scalar(&query).bind($circle).bind($agent)
            .fetch_optional(&mut *$tx).await.map_err(memory_error)?;
        if agent.is_none() {return Err(RepositoryError::PermissionDenied);}
    }};
}

macro_rules! advance_floors {
    ($tx:expr,$pg:expr,$profile:expr) => {{
        let maximum=if $pg {"greatest"} else {"max"};
        let floor=format!("{maximum}(start_sequence,dirty_sequence,coalesce((select max(sequence) from messages where channel_id=agent_memory_scopes.channel_id),0))");
        let query=format!("update agent_memory_scopes set start_sequence={floor},processed_sequence={floor},dirty_sequence={floor},source_generation=source_generation+1,lease_token=null,leased_until=null,attempts=0 where profile_id=?uuid");
        sqlx::query(&crate::chatbot::sql(&query,$pg)).bind($profile).execute(&mut *$tx).await.map_err(memory_error)?;
    }};
}

impl crate::chatbot::CircleChatAgents {
    pub(crate) async fn read_memory(
        &self,
        actor: &crate::domain::UserId,
        circle: &str,
        agent: &str,
    ) -> crate::chatbot::Result<MemoryView> {
        macro_rules! read {
            ($pool:expr,$pg:expr) => {{
                let mut tx=$pool.begin().await.map_err(crate::chatbot::storage)?;
                if $pg { sqlx::query("set transaction isolation level repeatable read read only").execute(&mut *tx).await.map_err(crate::chatbot::storage)?; }
                authorize!(tx,$pg,actor,circle,agent,false);
                let mut view=read_memory!(tx,$pg,actor,circle,agent);
                if view.revision==0 {
                    view.agent_enabled=sqlx::query_scalar(&crate::chatbot::sql("select memory_enabled from circle_chat_agents where agent_id=?uuid and circle_id=?uuid",$pg)).bind(agent).bind(circle).fetch_one(&mut *tx).await.map_err(crate::chatbot::storage)?;
                }
                tx.commit().await.map_err(crate::chatbot::storage)?;
                Ok(view)
            }};
        }
        match &self.store {
            crate::chatbot::Store::Pg(pool) => read!(pool, true),
            crate::chatbot::Store::Sqlite(pool) => read!(pool, false),
        }
    }

    pub(crate) async fn mutate_memory(
        &self,
        actor: &crate::domain::UserId,
        circle: &str,
        agent: &str,
        mutation: Mutation,
    ) -> crate::chatbot::Result<MemoryView> {
        use crate::chatbot::{Store, sql, storage};
        use sqlx::Row;
        if mutation.revision() < 0 {
            return Err(RepositoryError::Conflict);
        }
        macro_rules! change {
            ($pool:expr,$pg:expr) => {{
                let mut tx=if $pg {$pool.begin().await.map_err(memory_error)?} else {$pool.begin_with("BEGIN IMMEDIATE").await.map_err(memory_error)?};
                if $pg {sqlx::query("set transaction isolation level serializable").execute(&mut *tx).await.map_err(memory_error)?;}
                // Same order as circle departure: circle authority before profile.
                authorize!(tx,$pg,actor,circle,agent,true);
                let now=chrono::Utc::now().timestamp();
                sqlx::query(&sql("insert into agent_memory_profiles(id,circle_id,agent_id,user_id,created_at,updated_at) values(?uuid,?uuid,?uuid,?uuid,?int,?int) on conflict(circle_id,agent_id,user_id) do nothing",$pg))
                    .bind(Uuid::now_v7().to_string()).bind(circle).bind(agent).bind(actor.to_string()).bind(now.to_string()).bind(now.to_string()).execute(&mut *tx).await.map_err(memory_error)?;
                let query=sql(PROFILE_QUERY,$pg)+if $pg {" for update of p"} else {""};
                let row=sqlx::query(&query).bind(actor.to_string()).bind(circle).bind(agent)
                    .fetch_one(&mut *tx).await.map_err(memory_error)?;
                let profile:String=row.try_get("id").map_err(storage)?;
                let revision:i64=row.try_get("revision").map_err(storage)?;
                let epoch:i64=row.try_get("memory_epoch").map_err(storage)?;
                if revision!=mutation.revision() {return Err(RepositoryError::Conflict);}
                let next_revision=revision.checked_add(1).ok_or(RepositoryError::Conflict)?;
                let next_epoch=super::MemoryEpoch::try_from(epoch).and_then(super::MemoryEpoch::next).map(i64::from).map_err(|_|RepositoryError::Conflict)?;
                match &mutation {
                    Mutation::Choice(input)=>{
                        // Consent can be saved, but no collection starts in M1.
                        // M3 must initialize its start boundary at activation.
                        sqlx::query(&sql("update agent_memory_profiles set enabled=case when ?='true' then true else false end,collection_started_at=null where id=?uuid",$pg))
                            .bind(input.enabled.to_string()).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                        sqlx::query(&sql("update agent_memory_scopes set lease_token=null,leased_until=null,source_generation=source_generation+1 where profile_id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                    },
                    Mutation::Action(input)=>match &input.action {
                        MemoryAction::Correct {note_id,..}|MemoryAction::Confirm {note_id}=>{
                            let view=read_memory!(tx,$pg,actor,circle,agent);
                            let mut note=view.notes.into_iter().find(|note|note.id==*note_id).ok_or(RepositoryError::PermissionDenied)?;
                            if let MemoryAction::Correct {text,..}=&input.action {
                                note.content.text=text.clone();
                                let length=if $pg {"octet_length(content->>'text')"} else {"length(cast(json_extract(content,'$.text') as blob))"};
                                let query=format!("select coalesce(sum({length}),0) from agent_memory_notes where profile_id=?uuid and id!=?uuid");
                                let bytes:i64=sqlx::query_scalar(&sql(&query,$pg)).bind(&profile).bind(note_id.to_string()).fetch_one(&mut *tx).await.map_err(memory_error)?;
                                let proposed=String::from(note.content.text.clone()).len();
                                if usize::try_from(bytes).map_err(storage)?.saturating_add(proposed)>super::MAX_PROFILE_NOTE_BYTES {return Err(RepositoryError::Conflict);}
                            }
                            let content=serde_json::to_value(&note.content).map_err(storage)?;
                            sqlx::query(&sql("update agent_memory_notes set content=?,origin='user',evidence='user_confirmed',revision=revision+1,updated_at=?int where id=?uuid and profile_id=?uuid",$pg))
                                .bind(sqlx::types::Json(content)).bind(now.to_string()).bind(note_id.to_string()).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                        },
                        MemoryAction::Forget {note_id}=>{
                            let exists:Option<String>=sqlx::query_scalar(&sql("select cast(id as text) from agent_memory_notes where id=?uuid and profile_id=?uuid",$pg)).bind(note_id.to_string()).bind(&profile).fetch_optional(&mut *tx).await.map_err(memory_error)?;
                            if exists.is_none(){return Err(RepositoryError::PermissionDenied);}
                            sqlx::query(&sql("insert into agent_memory_exclusions(profile_id,message_id) select n.profile_id,s.message_id from agent_memory_note_sources s join agent_memory_notes n on n.id=s.note_id where n.id=?uuid and n.profile_id=?uuid on conflict(profile_id,message_id) do nothing",$pg)).bind(note_id.to_string()).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                            // Remove every note depending on a forgotten source,
                            // including a user-corrected sibling of the same note.
                            sqlx::query(&sql("delete from agent_memory_notes where profile_id=?uuid and (id=?uuid or exists(select 1 from agent_memory_note_sources s join agent_memory_exclusions e on e.message_id=s.message_id where s.note_id=agent_memory_notes.id and e.profile_id=agent_memory_notes.profile_id))",$pg)).bind(&profile).bind(note_id.to_string()).execute(&mut *tx).await.map_err(memory_error)?;
                            let excluded:i64=sqlx::query_scalar(&sql("select count(*) from agent_memory_exclusions where profile_id=?uuid",$pg)).bind(&profile).fetch_one(&mut *tx).await.map_err(memory_error)?;
                            if excluded>super::MAX_PROFILE_EXCLUSIONS as i64 {
                                // Compact old source IDs into monotonic scope floors.
                                // Keep unrelated notes; never silently reopen history.
                                advance_floors!(tx,$pg,&profile);
                                sqlx::query(&sql("delete from agent_memory_exclusions where profile_id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                                sqlx::query(&sql("update agent_memory_profiles set history_compactions=history_compactions+1 where id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                            }
                        },
                        MemoryAction::Reset=>{
                            advance_floors!(tx,$pg,&profile);
                            sqlx::query(&sql("delete from agent_memory_notes where profile_id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                            sqlx::query(&sql("delete from agent_memory_exclusions where profile_id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                            sqlx::query(&sql("update agent_memory_profiles set collection_started_at=null where id=?uuid",$pg)).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                        },
                    },
                }
                sqlx::query(&sql("update agent_memory_profiles set revision=?int,memory_epoch=?int,updated_at=?int where id=?uuid",$pg))
                    .bind(next_revision.to_string()).bind(next_epoch.to_string()).bind(now.to_string()).bind(&profile).execute(&mut *tx).await.map_err(memory_error)?;
                let view=read_memory!(tx,$pg,actor,circle,agent);
                tx.commit().await.map_err(memory_error)?;
                Ok(view)
            }};
        }
        match &self.store {
            Store::Pg(pool) => change!(pool, true),
            Store::Sqlite(pool) => change!(pool, false),
        }
    }
}

fn memory_error(error: sqlx::Error) -> RepositoryError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .is_some_and(|code| matches!(code.as_ref(), "40001" | "40P01" | "5" | "6" | "517"))
    {
        RepositoryError::Conflict
    } else {
        crate::chatbot::storage(error)
    }
}

#[cfg(test)]
mod tests;
