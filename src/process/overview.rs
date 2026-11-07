use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::domain::{CircleId, RepositoryError, UserId};

const STEP_LIMIT: usize = 20;

#[derive(Clone, Debug, Serialize)]
pub struct CircleProcessStep {
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
    pub circle: CircleId,
    pub actor: UserId,
    pub limit: u32,
    pub all: bool,
    pub after: Option<String>,
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
                circle: self.circle.to_string(),
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
        if cursor.circle != self.circle.to_string()
            || cursor.all != self.all
            || cursor.at < -1
            || cursor.key.len() > 100
            || (!cursor.key.is_empty()
                && !cursor.key.split_once(':').is_some_and(|(kind, id)| {
                    matches!(
                        kind,
                        "process" | "pilot" | "work_review" | "work_export" | "work_status"
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
        for index in (1..=6).rev() {
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

fn overview_sql(pg: bool) -> String {
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
    // Eligibility belongs inside the query, including private/public membership.
    // Every work source additionally requires all related source/task channels,
    // so even a task's existence cannot disclose a hidden conversation.
    sql(
        &format!(
            r#"
with eligible as (
 select c.id,c.name from channels c
 join channel_memberships m on m.channel_id=c.id and cast(m.user_id as text)=?1
 join circle_memberships cm on cm.circle_id=c.circle_id and cm.user_id=m.user_id and cm.role in ('owner','moderator')
 join users u on u.id=m.user_id and u.kind='human'
 where cast(c.circle_id as text)=?2
), visible_work as (
 select w.* from work_items w
 join eligible source on source.id=w.source_channel_id
 join eligible task on task.id=w.task_channel_id
 where not exists(select 1 from work_item_tasks t where t.work_item_id=w.id
   and not exists(select 1 from eligible e where e.id=t.channel_id))
), items as (
 select 'process:'||cast(p.id as text) as key,'process' as kind,cast(p.id as text) as source_id,
 cast(p.channel_id as text) as channel_id,e.name as channel_name,p.definition_name as title,
 coalesce((select {inspected_status} from process_events pe where pe.process_link_id=p.id and pe.event_type='process.inspected' and pe.occurred_at>=p.updated_at order by pe.occurred_at desc,pe.id desc limit 1),p.status) as status,
 p.definition_name,'v1' as runtime,{process_time} as created_at
 from process_links p join eligible e on e.id=p.channel_id
 union all
 select 'pilot:'||p.id,'pilot',p.id,cast(p.channel_id as text),e.name,'Oppgåveflyt',p.status,p.definition_name,p.runtime_model,
 coalesce((select {pilot_time} from audit_events a where a.target_kind='pilot_run' and a.target_id=p.id and a.action='process.pilot_started' order by a.sequence limit 1),0)
 from process_pilot_runs p join eligible e on e.id=p.channel_id
 union all
 select 'work_review:'||cast(w.id as text),'work_review',cast(w.id as text),cast(w.source_channel_id as text),e.name,'Vurdering: '||w.title,
 case when w.start_status='failed' then 'failed' else w.process_status end,'work-item-review','v2',{work_time}
 from visible_work w join eligible e on e.id=w.source_channel_id
 union all
 select 'work_export:'||cast(x.work_item_id as text),'work_export',cast(x.work_item_id as text),cast(x.channel_id as text),e.name,'GitHub-eksport: '||w.title,x.status,'work-item-github-export','v2',x.created_at
 from work_item_export_processes x join visible_work w on w.id=x.work_item_id join eligible e on e.id=x.channel_id
 union all
 select 'work_status:'||cast(x.id as text),'work_status',cast(x.id as text),cast(x.channel_id as text),e.name,'Statusendring: '||w.title,x.status,'work-item-status-change','v2',x.created_at
 from work_item_status_changes x join visible_work w on w.id=x.work_item_id join eligible e on e.id=x.channel_id
)
select * from items where (?3=1 or lower(status) not in ('completed','cancelled','canceled','terminated'))
 and (created_at>?4 or (created_at=?4 and key>?5))
order by created_at,key limit ?6
"#
        ),
        pg,
    )
}

fn step_sql(kind: &str, pg: bool) -> String {
    let template = match kind {
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
            "select node_id as name,status,null as occurred_at from process_pilot_tasks where run_id=?1 order by id limit 21"
        }
        "work_review" => {
            "select node_id as name,status,cast(updated_at as text) as occurred_at from work_item_tasks where cast(work_item_id as text)=?1 and (node_id<>'publish-github' or not exists(select 1 from work_item_export_processes x where x.work_item_id=work_item_tasks.work_item_id)) order by created_at,id limit 21"
        }
        "work_export" => {
            "select 'github-export' as name,status,null as occurred_at from work_item_export_processes where cast(work_item_id as text)=?1 limit 21"
        }
        _ => {
            "select 'status-change' as name,status,null as occurred_at from work_item_status_changes where cast(id as text)=?1 limit 21"
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
        "first" => "Første oppgåve",
        "second" => "Andre oppgåve",
        "product-review" => "Produktvurdering",
        "technical-review" => "Teknisk vurdering",
        "final-confirmation" => "Endeleg stadfesting",
        "review" => "Vurdering",
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
            let circle=query.circle.to_string();
            let allowed: Option<i32> = sqlx::query_scalar(&sql("select 1 from circle_memberships cm join users u on u.id=cm.user_id and u.kind='human' where cast(cm.user_id as text)=?1 and cast(cm.circle_id as text)=?2 and cm.role in ('owner','moderator')",$pg))
                .bind(&actor).bind(&circle).fetch_optional(&mut *transaction).await.map_err(storage)?;
            if allowed.is_none() {return Err(RepositoryError::PermissionDenied);}
            let rows=sqlx::query(&overview_sql($pg)).bind(&actor).bind(&circle).bind(i32::from(query.all))
                .bind(cursor.at).bind(&cursor.key).bind(i64::from(query.limit)+1).fetch_all(&mut *transaction).await.map_err(storage)?;
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
                    steps.push(CircleProcessStep{name:step_name(&name),status,occurred_at:timestamp(step.try_get("occurred_at").map_err(storage)?)});
                }
                let history_rows=sqlx::query(&history_sql(&kind,$pg)).bind(&source_id).fetch_all(&mut *transaction).await.map_err(storage)?;
                let history_truncated=history_rows.len()>STEP_LIMIT;
                let mut history=Vec::new();
                for event in history_rows.into_iter().take(STEP_LIMIT) {
                    let name:String=event.try_get("name").map_err(storage)?;
                    history.push(CircleProcessStep{name:step_name(&name),status:event.try_get("status").map_err(storage)?,occurred_at:timestamp(event.try_get("occurred_at").map_err(storage)?)});
                }
                history.reverse();
                let key:String=row.try_get("key").map_err(storage)?;
                last=Some(Cursor{circle:circle.clone(),all:query.all,at:row.try_get("created_at").map_err(storage)?,key:key.clone()});
                processes.push(CircleProcess{
                    id:key,kind,title:row.try_get::<String,_>("title").map_err(storage)?.chars().take(240).collect(),
                    channel_id:row.try_get("channel_id").map_err(storage)?,channel_name:row.try_get("channel_name").map_err(storage)?,
                    status:row.try_get::<String,_>("status").map_err(storage)?.chars().take(80).collect(),
                    definition_name:row.try_get::<String,_>("definition_name").map_err(storage)?.chars().take(120).collect(),
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
