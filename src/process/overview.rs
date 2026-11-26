use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::domain::{CircleId, RepositoryError, UserId};

mod definition;
pub use definition::DefinitionStep;

const STEP_LIMIT: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct StepTask {
    pub kind: &'static str,
    pub id: String,
    pub message_id: String,
    pub channel_id: String,
    pub assignee_name: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CircleProcessStep {
    pub task: Option<StepTask>,
    pub node_id: Option<String>,
    pub name: String,
    pub status: String,
    pub occurred_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CircleProcess {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub channel_id: String,
    pub channel_name: String,
    pub status: String,
    pub definition_name: String,
    pub runtime: String,
    pub definition_namespace: String,
    pub definition_version: Option<String>,
    pub definition_steps: Option<Vec<DefinitionStep>>,
    pub active_node_ids: Option<Vec<String>>,
    #[serde(skip)]
    pub(crate) instance_id: Option<String>,
    #[serde(skip)]
    pub(crate) engine_url: Option<String>,
    pub steps: Vec<CircleProcessStep>,
    pub history: Vec<CircleProcessStep>,
    pub steps_truncated: bool,
    pub history_truncated: bool,
    pub remaining_steps: Option<Vec<CircleProcessStep>>,
    pub projection: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CircleProcessPage {
    pub processes: Vec<CircleProcess>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CircleProcessQuery {
    pub scope: ProcessScope,
    pub actor: UserId,
    pub limit: u32,
    pub all: bool,
    pub after: Option<String>,
}

#[derive(Clone, Debug)]
pub enum ProcessScope {
    ModeratedCircle(CircleId),
    Channel {
        channel: uuid::Uuid,
        process: Option<String>,
    },
}

impl ProcessScope {
    fn id(&self) -> String {
        match self {
            Self::ModeratedCircle(circle) => circle.to_string(),
            Self::Channel { channel, .. } => channel.to_string(),
        }
    }
    fn cursor_scope(&self) -> String {
        match self {
            Self::ModeratedCircle(_) => self.id(),
            Self::Channel { channel, .. } => format!("channel:{channel}"),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    circle: String,
    all: bool,
    at: i64,
    key: String,
}

impl CircleProcessQuery {
    fn cursor(&self) -> Result<Cursor, RepositoryError> {
        if !(1..=50).contains(&self.limit) {
            return Err(RepositoryError::Conflict);
        }
        let cursor = match &self.after {
            None => Cursor {
                circle: self.scope.cursor_scope(),
                all: self.all,
                at: -1,
                key: String::new(),
            },
            Some(value) if value.len() <= 512 => URL_SAFE_NO_PAD
                .decode(value)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Cursor>(&bytes).ok())
                .ok_or(RepositoryError::Conflict)?,
            Some(_) => return Err(RepositoryError::Conflict),
        };
        if cursor.circle != self.scope.cursor_scope()
            || cursor.all != self.all
            || cursor.at < -1
            || cursor.key.len() > 100
            || (!cursor.key.is_empty()
                && !cursor.key.split_once(':').is_some_and(|(kind, id)| {
                    matches!(
                        kind,
                        "process"
                            | "pilot"
                            | "work_review"
                            | "work_export"
                            | "work_status"
                            | "feedback"
                    ) && uuid::Uuid::parse_str(id).is_ok()
                }))
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(cursor)
    }

    pub fn validate(&self) -> Result<(), RepositoryError> {
        self.cursor().map(|_| ())
    }
}

fn timestamp(value: Option<String>) -> Option<String> {
    value.map(|value| {
        value
            .parse::<i64>()
            .ok()
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
            .map(|time| time.to_rfc3339())
            .unwrap_or(value)
    })
}

fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

fn sql(template: &str, pg: bool) -> String {
    let mut query = template.to_owned();
    // Only static SQL templates reach here; values are always bound.
    if pg {
        for index in (1..=7).rev() {
            query = query.replace(&format!("?{index}"), &format!("${index}"));
        }
    }
    query
}

fn epoch(value: &str, pg: bool) -> String {
    if pg {
        format!("cast(extract(epoch from {value}) as bigint)")
    } else {
        format!("cast(strftime('%s',{value}) as bigint)")
    }
}

fn eligible_channels(scope: &ProcessScope) -> String {
    let scope_filter = match scope {
        ProcessScope::ModeratedCircle(_) => "cast(c.circle_id as text)=?2 and cm.role in ('owner','moderator')",
        ProcessScope::Channel { .. } => "(cm.user_id is not null and c.circle_id=(select circle_id from channels where cast(id as text)=?2))
            or (cast(c.id as text)=?2 and c.circle_id is null and c.kind='public' and not c.hidden)",
    };
    format!(
        "select c.id,c.name from channels c
        join channel_memberships m on m.channel_id=c.id and cast(m.user_id as text)=?1
        join users u on u.id=m.user_id and u.kind='human'
        left join circle_memberships cm on cm.circle_id=c.circle_id and cm.user_id=m.user_id
        where ({scope_filter})"
    )
}

fn overview_sql(pg: bool, scope: &ProcessScope) -> String {
    let inspected_status = if pg {
        "payload->>'status'"
    } else {
        "json_extract(payload,'$.status')"
    };
    let process_time = epoch("p.created_at", pg);
    let pilot_time = epoch("a.occurred_at", pg);
    let work_time = if pg {
        epoch("w.created_at", true)
    } else {
        "cast(w.created_at as bigint)".into()
    };
    let eligible = eligible_channels(scope);
    let result_filter = match scope {
        ProcessScope::ModeratedCircle(_) => "",
        ProcessScope::Channel { .. } => "and channel_id=?2",
    };
    let feedback_time = epoch("m.created_at", pg);
    // Eligibility belongs inside the query, including private/public membership.
    // Every work source additionally requires all related source/task channels,
    // so even a task's existence cannot disclose a hidden conversation.
    sql(
        &format!(
            r#"
with eligible as ({eligible}), visible_work as (
 select w.* from work_items w
 join eligible source on source.id=w.source_channel_id
 join eligible task on task.id=w.task_channel_id
 where not exists(select 1 from work_item_tasks t where t.work_item_id=w.id
   and not exists(select 1 from eligible e where e.id=t.channel_id))
), items as (
 select 'process:'||cast(p.id as text) as key,'process' as kind,cast(p.id as text) as source_id,
 cast(p.channel_id as text) as channel_id,e.name as channel_name,p.definition_name as title,
 coalesce((select {inspected_status} from process_events pe where pe.process_link_id=p.id and pe.event_type='process.inspected' and pe.occurred_at>=p.updated_at order by pe.occurred_at desc,pe.id desc limit 1),p.status) as status,
 p.definition_name,'v1' as runtime,p.namespace,p.definition_version,cast(p.heart_instance_id as text) as instance_id,cast(null as text) as engine_url,{process_time} as created_at
 from process_links p join eligible e on e.id=p.channel_id
 union all
 select 'pilot:'||p.id,'pilot',p.id,cast(p.channel_id as text),e.name,'Oppgåveflyt',p.status,p.definition_name,p.runtime_model,'sproyt',p.definition_version,p.instance_id,p.engine_url,
 coalesce((select {pilot_time} from audit_events a where a.target_kind='pilot_run' and a.target_id=p.id and a.action='process.pilot_started' order by a.sequence limit 1),0)
 from process_pilot_runs p join eligible e on e.id=p.channel_id
 union all
 select 'work_review:'||cast(w.id as text),'work_review',cast(w.id as text),cast(w.source_channel_id as text),e.name,'Vurdering: '||w.title,
 case when w.start_status='failed' then 'failed' else w.process_status end,'work-item-review','v2','sproyt',w.definition_version,cast(w.heart_instance_id as text),null,{work_time}
 from visible_work w join eligible e on e.id=w.source_channel_id
 union all
 select 'work_export:'||cast(x.work_item_id as text),'work_export',cast(x.work_item_id as text),cast(x.channel_id as text),e.name,'GitHub-eksport: '||w.title,x.status,'work-item-github-export','v2','sproyt','1.0.0',cast(x.heart_instance_id as text),null,x.created_at
 from work_item_export_processes x join visible_work w on w.id=x.work_item_id join eligible e on e.id=x.channel_id
 union all
 select 'work_status:'||cast(x.id as text),'work_status',cast(x.id as text),cast(x.channel_id as text),e.name,'Statusendring: '||w.title,x.status,'work-item-status-change','v2','sproyt','1.0.0',cast(x.heart_instance_id as text),null,x.created_at
 from work_item_status_changes x join visible_work w on w.id=x.work_item_id join eligible e on e.id=x.channel_id
 union all
 select 'feedback:'||cast(h.source_message_id as text),'feedback',cast(h.source_message_id as text),
 cast(j.channel_id as text),e.name,'GitHub #'||cast(j.issue_number as text)||' · '||j.published_title,
 case h.stage when 'review' then 'reviewing' when 'review_submitted' then 'reviewing' when 'waiting_for_close' then 'waiting' else h.stage end,
 'feedback-issue','v2','sproyt','1.0.0',cast(h.instance_id as text),null,{feedback_time}
 from feedback_heart h join feedback_jobs j on j.source_message_id=h.source_message_id
 join eligible e on e.id=j.channel_id join messages m on m.id=j.source_message_id and m.channel_id=e.id and m.deleted_at is null
 where j.publication_kind='create' and j.status in ('published','done')
)
select * from items where (?3=1 or (lower(status) not in ('completed','cancelled','canceled','terminated') and not (kind='feedback' and status in ('resolved','rejected','failed'))))
 and (created_at>?4 or (created_at=?4 and key>?5))
{result_filter} and (?7='' or key=?7)
order by created_at,key limit ?6
"#
        ),
        pg,
    )
}

fn step_sql(kind: &str, pg: bool) -> String {
    let template = match kind {
        "feedback" => "select case stage when 'starting' then 'start' when 'review_submitted' then 'review'
            when 'waiting_for_close' then 'await-github' else stage end as name,
            case when stage in ('resolved','rejected','review_submitted') then 'completed'
                 when stage='failed' then 'failed' else 'waiting' end as status,
            cast(null as text) as occurred_at,cast(null as text) as message_id from feedback_heart where cast(source_message_id as text)=?1",
        "process" => {
            return sql(
                &format!(
                    "select {} as name,{} as status,cast(occurred_at as text) as occurred_at from process_events pe join process_links p on p.id=pe.process_link_id where cast(process_link_id as text)=?1 and event_type='process.inspected' and pe.occurred_at>=p.updated_at order by pe.occurred_at desc,pe.id desc limit 1",
                    if pg {
                        "payload->>'current_node'"
                    } else {
                        "json_extract(payload,'$.current_node')"
                    },
                    if pg {
                        "payload->>'status'"
                    } else {
                        "json_extract(payload,'$.status')"
                    }
                ),
                pg,
            );
        }
        "pilot" => {
            "select t.node_id as name,t.status,null as occurred_at,t.id as task_id,cast(m.id as text) as message_id,
            cast(r.channel_id as text) as task_channel,u.display_name from process_pilot_tasks t
            join process_pilot_runs r on r.id=t.run_id join users u on u.id=r.assignee_id
            left join messages m on m.id=t.message_id and m.channel_id=r.channel_id
            where t.run_id=?1 order by t.id limit 21"
        }
        "work_review" => {
            "select t.node_id as name,t.status,cast(t.updated_at as text) as occurred_at,cast(t.id as text) as task_id,
            cast(m.id as text) as message_id,cast(t.channel_id as text) as task_channel,u.display_name from work_item_tasks t
            join users u on u.id=t.assignee_id left join messages m on m.id=t.message_id and m.channel_id=t.channel_id
            where cast(t.work_item_id as text)=?1 and (t.node_id<>'publish-github' or not exists(select 1 from work_item_export_processes x where x.work_item_id=t.work_item_id)) order by t.created_at,t.id limit 21"
        }
        "work_export" => {
            "select 'github-export' as name,x.status,null as occurred_at,cast(t.id as text) as task_id,cast(m.id as text) as message_id,
            cast(t.channel_id as text) as task_channel,u.display_name from work_item_export_processes x
            left join work_item_tasks t on t.work_item_id=x.work_item_id and t.node_id='publish-github' and t.channel_id=x.channel_id
            left join users u on u.id=t.assignee_id left join messages m on m.id=t.message_id and m.channel_id=t.channel_id
            where cast(x.work_item_id as text)=?1 limit 21"
        }
        _ => {
            "select 'status-change' as name,x.status,null as occurred_at,cast(x.task_id as text) as task_id,cast(m.id as text) as message_id,
            cast(x.channel_id as text) as task_channel,u.display_name from work_item_status_changes x
            join users u on u.id=x.actor_id left join messages m on m.id=x.message_id and m.channel_id=x.channel_id
            where cast(x.id as text)=?1 limit 21"
        }
    };
    sql(template, pg)
}

fn history_sql(kind: &str, pg: bool) -> String {
    sql(
        match kind {
            "process" => {
                "select event_type as name,case when event_type='process.failed' then 'failed' else 'completed' end as status,cast(occurred_at as text) as occurred_at from process_events where cast(process_link_id as text)=?1 order by occurred_at desc,id desc limit 21"
            }
            "work_status" => {
                "select 'status-change' as name,to_status as status,cast(created_at as text) as occurred_at from work_item_status_history where cast(change_id as text)=?1 order by created_at desc,id desc limit 21"
            }
            _ => "select 'none' as name,'none' as status,null as occurred_at where ?1='' and 1=0",
        },
        pg,
    )
}

fn step_name(value: &str) -> String {
    match value {
        "start" => "Start",
        "end" | "completed" => "Slutt",
        "prepare" => "Førebu avgjerda",
        "wait-for-decision" => "Vent på avgjerd",
        "record-decision" => "Registrer avgjerda",
        "review-fork" => "Start parallelle vurderingar",
        "review-join" => "Samle vurderingane",
        "interpret-review" => "Tolk vurderinga",
        "review-route" => "Avgjer vidare behandling",
        "followup-review" => "Oppfølgingsvurdering",
        "publish-github" => "Publisering på GitHub",
        "change-status" => "Endre saksstatus",
        "first" => "Første oppgåve",
        "second" => "Andre oppgåve",
        "product-review" => "Produktvurdering",
        "technical-review" => "Teknisk vurdering",
        "final-confirmation" => "Endeleg stadfesting",
        "review" => "Vurdering",
        "await-github" => "Ventar på lukking i GitHub",
        "closure" => "Lukkegrunn frå GitHub",
        "resolved" => "Løyst",
        "rejected" => "Avvist",
        "request-information" | "provide-information" => "Innhenting av informasjon",
        "github-export" => "Eksport til GitHub",
        "status-change" => "Statusendring",
        "process.started" => "Prosessen vart starta",
        "process.inspected" => "Status vart henta frå Heart",
        "process.correlated" => "Svar vart sendt til Heart",
        "process.failed" => "Prosesshandling feila",
        _ => return value.chars().take(100).collect(),
    }
    .into()
}

macro_rules! implementation {
    ($name:ident,$pool:ty,$pg:expr) => {
        pub async fn $name(pool: &$pool, query: CircleProcessQuery) -> Result<CircleProcessPage,RepositoryError> {
            let cursor=query.cursor()?;
            // One consistent authorization/data snapshot: a concurrently added
            // private task or revoked membership cannot split summary and details.
            let mut transaction=pool.begin().await.map_err(storage)?;
            if $pg {
                sqlx::query("set transaction isolation level repeatable read read only")
                    .execute(&mut *transaction).await.map_err(storage)?;
            }
            let actor=query.actor.to_string();
            let circle=query.scope.id();
            let channel_authorization = format!("select 1 from ({}) eligible where cast(id as text)=?2", eligible_channels(&query.scope));
            let (authorization, key_filter) = match &query.scope {
                ProcessScope::ModeratedCircle(_) => ("select 1 from circle_memberships cm join users u on u.id=cm.user_id and u.kind='human' where cast(cm.user_id as text)=?1 and cast(cm.circle_id as text)=?2 and cm.role in ('owner','moderator')", ""),
                ProcessScope::Channel {process,..} => (&channel_authorization[..],process.as_deref().unwrap_or("")),
            };
            let allowed: Option<i32> = sqlx::query_scalar(&sql(authorization,$pg))
                .bind(&actor).bind(&circle).fetch_optional(&mut *transaction).await.map_err(storage)?;
            if allowed.is_none() {return Err(RepositoryError::PermissionDenied);}
            let rows=sqlx::query(&overview_sql($pg,&query.scope)).bind(&actor).bind(&circle).bind(i32::from(query.all))
                .bind(cursor.at).bind(&cursor.key).bind(i64::from(query.limit)+1).bind(key_filter).fetch_all(&mut *transaction).await.map_err(storage)?;
            let has_more=rows.len()>query.limit as usize;
            let mut processes=Vec::new();
            let mut last=None;
            for row in rows.into_iter().take(query.limit as usize) {
                let kind: String=row.try_get("kind").map_err(storage)?;
                let source_id: String=row.try_get("source_id").map_err(storage)?;
                let step_rows=sqlx::query(&step_sql(&kind,$pg)).bind(&source_id).fetch_all(&mut *transaction).await.map_err(storage)?;
                let truncated=step_rows.len()>STEP_LIMIT;
                let mut steps=Vec::new();
                for step in step_rows.into_iter().take(STEP_LIMIT) {
                    let Some(name)=step.try_get::<Option<String>,_>("name").map_err(storage)? else {continue};
                    let raw_status:String=step.try_get("status").map_err(storage)?;
                    let status=match raw_status.as_str() {
                        "process.started"|"process.correlated"|"process.inspected"=>"completed".into(),
                        "process.failed"=>"failed".into(),
                        _=>raw_status.chars().take(80).collect(),
                    };
                    let node_id=match (kind.as_str(),name.as_str()) {
                        ("work_export","github-export")=> "publish-github",
                        ("work_status","status-change")=> "change-status",
                        _=> &name,
                    };
                    // Task rows and all their channels were authorized in this transaction.
                    // Expose a card only when its actual message is in the expected channel.
                    let task = if kind!="process" && !key_filter.is_empty()
                        && let Some(message_id)=step.try_get::<Option<String>,_>("message_id").map_err(storage)?
                        && let Some(id)=step.try_get::<Option<String>,_>("task_id").map_err(storage)? {
                        Some(StepTask {kind:if kind=="pilot" {"process_pilot"}else{"work_item"},
                            id,message_id,channel_id:step.try_get("task_channel").map_err(storage)?,
                            assignee_name:step.try_get("display_name").map_err(storage)?})
                    } else {None};
                    steps.push(CircleProcessStep{task,node_id:Some(node_id.to_owned()),name:step_name(&name),status,occurred_at:timestamp(step.try_get("occurred_at").map_err(storage)?)});
                }
                let history_rows=sqlx::query(&history_sql(&kind,$pg)).bind(&source_id).fetch_all(&mut *transaction).await.map_err(storage)?;
                let history_truncated=history_rows.len()>STEP_LIMIT;
                let mut history=Vec::new();
                for event in history_rows.into_iter().take(STEP_LIMIT) {
                    let name:String=event.try_get("name").map_err(storage)?;
                    history.push(CircleProcessStep{task:None,node_id:None,name:step_name(&name),status:event.try_get("status").map_err(storage)?,occurred_at:timestamp(event.try_get("occurred_at").map_err(storage)?)});
                }
                history.reverse();
                let key:String=row.try_get("key").map_err(storage)?;
                last=Some(Cursor{circle:query.scope.cursor_scope(),all:query.all,at:row.try_get("created_at").map_err(storage)?,key:key.clone()});
                processes.push(CircleProcess{
                    id:key,kind,title:row.try_get::<String,_>("title").map_err(storage)?.chars().take(240).collect(),
                    channel_id:row.try_get("channel_id").map_err(storage)?,channel_name:row.try_get("channel_name").map_err(storage)?,
                    status:row.try_get::<String,_>("status").map_err(storage)?.chars().take(80).collect(),
                    definition_name:row.try_get::<String,_>("definition_name").map_err(storage)?.chars().take(120).collect(),
                    definition_namespace:row.try_get("namespace").map_err(storage)?,definition_version:row.try_get("definition_version").map_err(storage)?,definition_steps:None,active_node_ids:None,
                    instance_id:row.try_get("instance_id").map_err(storage)?,engine_url:row.try_get("engine_url").map_err(storage)?,
                    runtime:row.try_get("runtime").map_err(storage)?,history,steps,
                    steps_truncated:truncated,history_truncated,remaining_steps:None,projection:true,
                });
            }
            let next_cursor=if has_more {last.map(|c|serde_json::to_vec(&c).map(|v|URL_SAFE_NO_PAD.encode(v))).transpose().map_err(storage)?}else{None};
            transaction.commit().await.map_err(storage)?;
            Ok(CircleProcessPage{processes,next_cursor})
        }
    }
}
implementation!(list_sqlite, sqlx::SqlitePool, false);
implementation!(list_postgres, sqlx::PgPool, true);
#[cfg(test)]
mod tests;
