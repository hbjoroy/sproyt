use super::{
    ASSIGNMENT_RIGHTS, Result, Store, TaskView, WorkItems, sql,
    status_change::{ROUTE_JOINS, ROUTE_PHASE},
    storage,
};
use crate::{
    domain::{RepositoryError, UserId},
    process_pilot,
};
use serde::Serialize;
use std::{fmt, str::FromStr};
use uuid::Uuid;

// Discover candidates from local Heart delivery projections. The task views
// apply the same live permissions as the individual task and completion APIs.
const CANDIDATES: &str = "
with assigned as (
    select 'work_item' as kind, cast(t.id as text) as task_id, t.message_id, t.channel_id
    from work_item_tasks t
    join work_items w on w.id=t.work_item_id
    where t.assignee_id=?uuid and t.status='pending' and t.delivery_status='ready'
      and t.decision_request_id is null
      and {ASSIGNMENT_RIGHTS}
      and (
        (t.node_id<>'publish-github' and w.process_status='waiting')
        or (t.node_id='publish-github' and w.status='planned' and exists (
            select 1 from work_item_export_processes x
            where x.work_item_id=w.id and x.channel_id=t.channel_id and x.assignee_id=t.assignee_id
              and x.status='waiting'
        ))
      )
    union all
    select 'status_change', cast(x.task_id as text), x.message_id, x.channel_id
    from work_item_status_changes x
    where x.actor_id=?uuid and x.status='waiting' and x.decision_result is null
      and x.task_id is not null and x.message_id is not null
      and exists (
          select 1 {ROUTE_JOINS}
          where cm.user_id=x.actor_id and w.id=x.work_item_id and {ROUTE_PHASE}
            and r.id=x.route_id and r.channel_id=x.channel_id
      )
    union all
    select 'process_pilot', t.id, t.message_id, r.channel_id
    from process_pilot_tasks t
    join process_pilot_runs r on r.id=t.run_id
    where ? and r.assignee_id=?uuid and r.status in ('starting','waiting')
      and (r.engine_url='' or r.engine_url=?)
      and t.status='pending' and t.delivery_status='ready' and t.command_id=''
)
select a.kind, a.task_id, cast(a.message_id as text) as message_id,
       cast(c.id as text) as channel_id, c.name as channel_name,
       cast(r.id as text) as circle_id, r.name as circle_name
from assigned a
join channels c on c.id=a.channel_id
join channel_memberships cm on cm.channel_id=c.id and cm.user_id=?uuid and cm.role<>'observer'
left join circles r on r.id=c.circle_id
where a.channel_id=coalesce(?uuid,a.channel_id)
  and (?uuid is null or (a.message_id, a.kind) > (?uuid, ?))
order by a.message_id, a.kind
limit ?";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskKind {
    WorkItem,
    StatusChange,
    ProcessPilot,
}

impl TaskKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::WorkItem => "work_item",
            Self::StatusChange => "status_change",
            Self::ProcessPilot => "process_pilot",
        }
    }
}

impl FromStr for TaskKind {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "work_item" => Ok(Self::WorkItem),
            "status_change" => Ok(Self::StatusChange),
            "process_pilot" => Ok(Self::ProcessPilot),
            _ => Err(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TaskQueueCursor {
    message: Uuid,
    kind: TaskKind,
}

impl fmt::Display for TaskQueueCursor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.message, self.kind.as_str())
    }
}

impl FromStr for TaskQueueCursor {
    type Err = ();

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (message, kind) = value.split_once(':').ok_or(())?;
        Ok(Self {
            message: Uuid::parse_str(message).map_err(|_| ())?,
            kind: kind.parse()?,
        })
    }
}

#[derive(sqlx::FromRow)]
struct Candidate {
    kind: String,
    task_id: String,
    message_id: String,
    channel_id: String,
    channel_name: String,
    circle_id: Option<String>,
    circle_name: Option<String>,
}

impl Candidate {
    fn cursor(&self) -> Result<TaskQueueCursor> {
        Ok(TaskQueueCursor {
            message: Uuid::parse_str(&self.message_id).map_err(storage)?,
            kind: self
                .kind
                .parse()
                .map_err(|()| storage("unknown queue task kind"))?,
        })
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", content = "task", rename_all = "snake_case")]
enum QueueTask {
    WorkItem(Box<TaskView>),
    ProcessPilot(Box<process_pilot::TaskView>),
}

#[derive(Serialize)]
struct QueueEntry {
    channel_id: String,
    channel_name: String,
    circle_id: Option<String>,
    circle_name: Option<String>,
    #[serde(flatten)]
    task: QueueTask,
}

#[derive(Serialize)]
pub(crate) struct TaskQueuePage {
    tasks: Vec<QueueEntry>,
    next_cursor: Option<String>,
}

impl WorkItems {
    pub(crate) async fn task_queue(
        &self,
        actor: UserId,
        channel: Option<Uuid>,
        cursor: Option<TaskQueueCursor>,
        limit: usize,
        pilot: Option<&process_pilot::ProcessPilot>,
    ) -> Result<TaskQueuePage> {
        let candidates = self
            .queue_candidates(
                &actor,
                channel,
                cursor,
                limit + 1,
                pilot.map(|pilot| pilot.overview_connection().1),
            )
            .await?;
        // Access may change after discovery, so reuse the task projection as a
        // second authorization check. A concurrent change can shorten a page.
        let has_more = candidates.len() > limit;
        let mut next_cursor = None;
        let mut tasks = Vec::with_capacity(limit);
        for candidate in candidates.into_iter().take(limit) {
            let key = candidate.cursor()?;
            if has_more {
                next_cursor = Some(key.to_string());
            }
            if let Some(task) = self.queue_task(&actor, &candidate, key, pilot).await? {
                tasks.push(QueueEntry {
                    channel_id: candidate.channel_id,
                    channel_name: candidate.channel_name,
                    circle_id: candidate.circle_id,
                    circle_name: candidate.circle_name,
                    task,
                });
            }
        }
        Ok(TaskQueuePage { tasks, next_cursor })
    }

    async fn queue_candidates(
        &self,
        actor: &UserId,
        channel: Option<Uuid>,
        cursor: Option<TaskQueueCursor>,
        limit: usize,
        pilot_engine: Option<&str>,
    ) -> Result<Vec<Candidate>> {
        let actor = actor.to_string();
        let channel = channel.map(|id| id.to_string());
        let message = cursor.map(|key| key.message.to_string());
        let kind = cursor.map(|key| key.kind.as_str());
        let query = CANDIDATES
            .replace("{ASSIGNMENT_RIGHTS}", ASSIGNMENT_RIGHTS)
            .replace("{ROUTE_JOINS}", ROUTE_JOINS)
            .replace("{ROUTE_PHASE}", ROUTE_PHASE);
        macro_rules! fetch {
            ($pool:expr, $pg:expr) => {
                sqlx::query_as(&sql(&query, $pg))
                    .bind(&actor)
                    .bind(&actor)
                    .bind(pilot_engine.is_some())
                    .bind(&actor)
                    .bind(pilot_engine)
                    .bind(&actor)
                    .bind(channel)
                    .bind(&message)
                    .bind(&message)
                    .bind(kind)
                    .bind(limit as i64)
                    .fetch_all($pool)
                    .await
                    .map_err(storage)
            };
        }
        match &self.store {
            Store::Pg(pool) => fetch!(pool, true),
            Store::Sqlite(pool) => fetch!(pool, false),
        }
    }

    async fn queue_task(
        &self,
        actor: &UserId,
        candidate: &Candidate,
        key: TaskQueueCursor,
        pilot: Option<&process_pilot::ProcessPilot>,
    ) -> Result<Option<QueueTask>> {
        match key.kind {
            TaskKind::WorkItem | TaskKind::StatusChange => {
                let id = Uuid::parse_str(&candidate.task_id).map_err(storage)?;
                match self.task(actor.clone(), id, key.message).await {
                    Ok(task) if task.can_decide => Ok(Some(QueueTask::WorkItem(Box::new(task)))),
                    Ok(_) | Err(RepositoryError::NotFound | RepositoryError::PermissionDenied) => {
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
            TaskKind::ProcessPilot => {
                let Some(pilot) = pilot else {
                    return Ok(None);
                };
                match pilot
                    .task(actor, &candidate.task_id, &candidate.message_id)
                    .await
                {
                    Ok(task) if task.can_complete && task.delivery_status == "ready" => {
                        Ok(Some(QueueTask::ProcessPilot(Box::new(task))))
                    }
                    Ok(_) | Err(RepositoryError::NotFound | RepositoryError::PermissionDenied) => {
                        Ok(None)
                    }
                    Err(error) => Err(error),
                }
            }
        }
    }
}
