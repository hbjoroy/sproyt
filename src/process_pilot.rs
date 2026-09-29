//! The two-step pilot projects real Heart user tasks into one enabled channel.
//! The local tables are delivery receipts, never a second workflow engine.
use crate::{
    chat::ChatEngine,
    config::{DatabaseConfig, DatabaseKind},
    domain::{MessageId, RepositoryError, UserId},
    process::{HeartGateway, ProcessGateway, StartProcess},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgPool, SqlitePool};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;
use uuid::Uuid;

const DEFINITION: &str = "sproyt-user-task-pilot";
const BOT: &str = "8f25a9ea-02ac-5b89-9cff-fb108a8e9ae0";
type Result<T> = std::result::Result<T, RepositoryError>;

#[derive(Clone)]
enum Store {
    Pg(PgPool),
    Sqlite(SqlitePool),
}

// Only static SQL enters this translator. Values always remain bound parameters.
fn sql(query: &str, pg: bool) -> String {
    let mut out = String::new();
    let mut rest = query;
    let mut index = 0;
    while let Some(pos) = rest.find('?') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos + 1..];
        index += 1;
        let suffix = if rest.starts_with("uuid") {
            rest = &rest[4..];
            "::uuid"
        } else if rest.starts_with("int") {
            rest = &rest[3..];
            "::bigint"
        } else {
            ""
        };
        if pg {
            out.push_str(&format!("${index}{suffix}"));
        } else {
            out.push('?');
        }
    }
    out.push_str(rest);
    out
}
fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

impl Store {
    async fn values(&self, query: &str, args: &[String]) -> Result<Vec<String>> {
        macro_rules! fetch {
            ($pool:expr,$pg:expr) => {{
                let text = sql(query, $pg);
                let mut q = sqlx::query_scalar::<_, String>(&text);
                for arg in args {
                    q = q.bind(arg);
                }
                q.fetch_all($pool).await.map_err(storage)
            }};
        }
        match self {
            Self::Pg(pool) => fetch!(pool, true),
            Self::Sqlite(pool) => fetch!(pool, false),
        }
    }
    async fn execute(&self, query: &str, args: &[String]) -> Result<u64> {
        macro_rules! exec {
            ($pool:expr,$pg:expr) => {{
                let text = sql(query, $pg);
                let mut q = sqlx::query(&text);
                for arg in args {
                    q = q.bind(arg);
                }
                q.execute($pool)
                    .await
                    .map(|r| r.rows_affected())
                    .map_err(storage)
            }};
        }
        match self {
            Self::Pg(pool) => exec!(pool, true),
            Self::Sqlite(pool) => exec!(pool, false),
        }
    }
}

#[derive(Clone)]
pub(crate) struct ProcessPilot {
    store: Store,
    configurator: Uuid,
    base: String,
    http: reqwest::Client,
    gateway: Arc<HeartGateway>,
}

#[derive(Serialize)]
pub(crate) struct Configuration {
    pub configured: bool,
    pub can_configure: bool,
    pub can_start: bool,
    pub assignee_name: Option<String>,
}

#[derive(Deserialize)]
struct HeartTask {
    id: Uuid,
    instance_id: Uuid,
    node_id: String,
    assignee_id: Uuid,
    status: String,
}

#[derive(Serialize)]
pub(crate) struct TaskView {
    pub id: String,
    pub instance_id: String,
    pub message_id: String,
    pub node_id: String,
    pub status: String,
    pub title: String,
    pub assignee_id: String,
    pub assignee_name: String,
    pub can_complete: bool,
    pub delivery_status: String,
}

impl ProcessPilot {
    pub async fn from_env(
        config: &DatabaseConfig,
        pool: Option<&PgPool>,
    ) -> std::result::Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        if std::env::var("SPROYT_PROCESS_PILOT_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        let base = std::env::var("SPROYT_PROCESS_PILOT_HEART_URL")?;
        let gateway = Arc::new(HeartGateway::new(base.clone(), Duration::from_secs(10), 0)?);
        let store = match config.kind() {
            DatabaseKind::Postgres => {
                Store::Pg(pool.ok_or("Missing shared PostgreSQL pool")?.clone())
            }
            DatabaseKind::Sqlite => Store::Sqlite(SqlitePool::connect(config.url()).await?),
        };
        Ok(Some(Self {
            store,
            configurator: Uuid::parse_str(&std::env::var("SPROYT_PROCESS_PILOT_ASSIGNEE_ID")?)?,
            base: base.trim_end_matches('/').into(),
            gateway,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        }))
    }
    async fn member(&self, actor: &UserId, channel: &str) -> Result<String> {
        self.store.values("select m.role from channel_memberships m where m.channel_id=?uuid and m.user_id=?uuid", &[channel.into(),actor.to_string()]).await?.into_iter().next().ok_or(RepositoryError::PermissionDenied)
    }
    pub async fn configuration(&self, actor: &UserId, channel: &str) -> Result<Configuration> {
        let role = self.member(actor, channel).await?;
        let owner = self.store.values("select c.name from channels c join circles r on r.id=c.circle_id join circle_memberships m on m.circle_id=r.id and m.user_id=?uuid where c.id=?uuid and m.role='owner' and lower(r.name)='rocket-admins' and lower(c.name)='prosesstest'", &[actor.to_string(),channel.into()]).await?;
        let names = self.store.values("select u.display_name from process_pilot_channels p join users u on u.id=p.assignee_id where p.channel_id=?uuid and p.enabled=1", &[channel.into()]).await?;
        let assigned = self.store.values("select cast(assignee_id as text) from process_pilot_channels where channel_id=?uuid and enabled=1", &[channel.into()]).await?;
        Ok(Configuration {
            configured: !names.is_empty(),
            can_configure: role == "owner"
                && !owner.is_empty()
                && actor.as_uuid() == &self.configurator,
            can_start: role != "observer" && assigned.first() == Some(&actor.to_string()),
            assignee_name: names.into_iter().next(),
        })
    }
    pub async fn configure(
        &self,
        actor: &UserId,
        channel: &str,
        enabled: bool,
    ) -> Result<Configuration> {
        if !self.configuration(actor, channel).await?.can_configure {
            return Err(RepositoryError::PermissionDenied);
        }
        self.store.execute("insert into process_pilot_channels(channel_id,assignee_id,enabled) values(?uuid,?uuid,?int) on conflict(channel_id) do update set enabled=excluded.enabled", &[channel.into(),actor.to_string(),i32::from(enabled).to_string()]).await?;
        self.configuration(actor, channel).await
    }
    pub async fn start(&self, actor: &UserId, channel: &str, request: Uuid) -> Result<Value> {
        if !self.configuration(actor, channel).await?.can_start {
            return Err(RepositoryError::PermissionDenied);
        }
        let id = Uuid::now_v7().to_string();
        self.store.execute("insert into process_pilot_runs(id,channel_id,actor_id,assignee_id,request_id) values(?,?uuid,?uuid,?uuid,?) on conflict(actor_id,request_id) do nothing", &[id,channel.into(),actor.to_string(),actor.to_string(),request.to_string()]).await?;
        let run = self.store.values("select id from process_pilot_runs where actor_id=?uuid and request_id=? and channel_id=?uuid", &[actor.to_string(),request.to_string(),channel.into()]).await?.into_iter().next().ok_or(RepositoryError::Conflict)?;
        Ok(json!({"id":run,"status":"starting"}))
    }
    pub async fn task(&self, actor: &UserId, id: &str, message: &str) -> Result<TaskView> {
        let fields = self.store.values("select cast(r.channel_id as text) || '|' || coalesce(r.instance_id,'') || '|' || t.node_id || '|' || t.status || '|' || cast(r.assignee_id as text) || '|' || t.delivery_status || '|' || u.display_name from process_pilot_tasks t join process_pilot_runs r on r.id=t.run_id join users u on u.id=r.assignee_id where t.id=? and t.message_id=?uuid", &[id.into(),message.into()]).await?.into_iter().next().ok_or(RepositoryError::NotFound)?;
        let p: Vec<_> = fields.splitn(7, '|').collect();
        let role = self.member(actor, p[0]).await?;
        Ok(TaskView {
            id: id.into(),
            instance_id: p[1].into(),
            message_id: message.into(),
            node_id: p[2].into(),
            status: p[3].into(),
            assignee_id: p[4].into(),
            delivery_status: p[5].into(),
            assignee_name: p[6].into(),
            title: match p[2] {
                "first" => "Steg 1 – Første stadfesting",
                _ => "Steg 2 – Stadfest overlevering",
            }
            .into(),
            can_complete: p[3] == "pending" && p[4] == actor.to_string() && role != "observer",
        })
    }
    pub async fn complete(
        &self,
        actor: &UserId,
        id: &str,
        message: &str,
        request: Uuid,
    ) -> Result<TaskView> {
        let task = self.task(actor, id, message).await?;
        if task.status == "completed"
            && task.assignee_id == actor.to_string()
            && self.member(actor, &self.task_channel(id).await?).await? != "observer"
        {
            return Ok(task);
        }
        if !task.can_complete {
            return Err(RepositoryError::PermissionDenied);
        }
        self.store.execute("update process_pilot_tasks set command_id=?,delivery_status='pending' where id=? and status='pending' and command_id=''", &[request.to_string(),id.into()]).await?;
        self.task(actor, id, message).await
    }
    async fn task_channel(&self, id: &str) -> Result<String> {
        self.store.values("select cast(r.channel_id as text) from process_pilot_runs r join process_pilot_tasks t on t.run_id=r.id where t.id=?", &[id.into()]).await?.into_iter().next().ok_or(RepositoryError::NotFound)
    }
    pub fn start_worker(&self, chat: ChatEngine, mut shutdown: watch::Receiver<bool>) {
        let service = self.clone();
        tokio::spawn(async move {
            loop {
                if *shutdown.borrow() {
                    break;
                }
                if let Err(error) = service.tick(&chat).await {
                    tracing::warn!(
                        error_kind = error.kind(),
                        "process pilot synchronization deferred"
                    );
                }
                tokio::select! { _=tokio::time::sleep(Duration::from_secs(3))=>{}, _=shutdown.changed()=>{} }
            }
        });
    }
    async fn tick(&self, chat: &ChatEngine) -> Result<()> {
        let now = Utc::now().timestamp();
        let runs=self.store.values("select id from process_pilot_runs where status<>'completed' and lease_until<?int order by lease_until,id limit 10", &[now.to_string()]).await?;
        for id in runs {
            let token = Uuid::now_v7().to_string();
            if self.store.execute("update process_pilot_runs set lease_until=?int,lease_token=? where id=? and lease_until<?int", &[(now+120).to_string(),token.clone(),id.clone(),now.to_string()]).await?==0 {continue;}
            let result = self.sync_run(chat, &id, &token).await;
            self.store.execute("update process_pilot_runs set lease_until=?int,lease_token='' where id=? and lease_token=?", &[Utc::now().timestamp().to_string(),id,token]).await?;
            if let Err(error) = result {
                tracing::warn!(
                    error_kind = error.kind(),
                    "process pilot run synchronization deferred"
                );
            }
        }
        Ok(())
    }
    async fn sync_run(&self, chat: &ChatEngine, id: &str, token: &str) -> Result<()> {
        let values=self.store.values("select cast(channel_id as text) || '|' || cast(assignee_id as text) || '|' || coalesce(instance_id,'') from process_pilot_runs where id=? and lease_token=?", &[id.into(),token.into()]).await?;
        let Some(row) = values.first() else {
            return Ok(());
        };
        let p: Vec<_> = row.split('|').collect();
        let channel = p[0];
        let assignee = p[1];
        // Revoked membership stops presentation and execution. Never auto-join users.
        let actor = UserId::from_uuid(Uuid::parse_str(assignee).map_err(storage)?);
        if self.member(&actor, channel).await? == "observer" {
            return Err(RepositoryError::PermissionDenied);
        }
        let instance = if p[2].is_empty() {
            let result = self
                .gateway
                .start(
                    &StartProcess {
                        namespace: "sproyt".into(),
                        definition_name: DEFINITION.into(),
                        version: Some("1.0.0".into()),
                        metadata: json!({"assignee_id":assignee,"pilot_run_id":id}),
                    },
                    Uuid::parse_str(id).map_err(storage)?,
                )
                .await
                .map_err(storage)?;
            let instance = result.instance_id.to_string();
            self.store.execute("update process_pilot_runs set instance_id=?,status='waiting' where id=? and lease_token=?", &[instance.clone(),id.into(),token.into()]).await?;
            instance
        } else {
            p[2].into()
        };
        let commands=self.store.values("select id || '|' || command_id from process_pilot_tasks where run_id=? and status='pending' and command_id<>''", &[id.into()]).await?;
        for command in commands {
            let (task, key) = command.split_once('|').ok_or(RepositoryError::Conflict)?;
            let response = self
                .http
                .post(format!("{}/api/v1/user-tasks/{task}/complete", self.base))
                .header("X-Heart-Client", "sproyt-pilot")
                .header("Idempotency-Key", key)
                .json(&json!({"actor_id":assignee,"result_metadata":{}}))
                .send()
                .await
                .map_err(storage)?;
            if !response.status().is_success() {
                return Err(storage("Heart completion not confirmed"));
            }
        }
        let response = self
            .http
            .get(format!("{}/api/v1/user-tasks", self.base))
            .query(&[("instance_id", &instance)])
            .send()
            .await
            .map_err(storage)?
            .error_for_status()
            .map_err(storage)?;
        let bytes = response.bytes().await.map_err(storage)?;
        if bytes.len() > 64 * 1024 {
            return Err(storage("Heart task response too large"));
        }
        let tasks: Vec<HeartTask> = serde_json::from_slice(&bytes).map_err(storage)?;
        if !valid_tasks(&tasks, &instance, assignee) {
            return Err(RepositoryError::Conflict);
        }
        for task in &tasks {
            if task.instance_id.to_string() != instance
                || task.assignee_id.to_string() != assignee
                || !matches!(task.node_id.as_str(), "first" | "second")
                || !matches!(task.status.as_str(), "pending" | "completed")
            {
                return Err(RepositoryError::Conflict);
            }
            let (message, created) = self.project(id, channel, token, task).await?;
            if created {
                chat.announce_persisted_message(MessageId::from_uuid(message))
                    .await
                    .map_err(storage)?;
            }
        }
        if tasks.len() == 2 && tasks.iter().all(|t| t.status == "completed") {
            let view = self
                .gateway
                .inspect(Uuid::parse_str(&instance).map_err(storage)?, Uuid::now_v7())
                .await
                .map_err(storage)?;
            if view.status == "completed" {
                self.store.execute("update process_pilot_runs set status='completed' where id=? and lease_token=?", &[id.into(),token.into()]).await?;
            }
        }
        Ok(())
    }
    async fn project(
        &self,
        run: &str,
        channel: &str,
        token: &str,
        task: &HeartTask,
    ) -> Result<(Uuid, bool)> {
        // Message + delivery receipt commit together. Existing message triggers
        // provide the normal PostgreSQL realtime delivery path.
        macro_rules! project {($pool:expr,$pg:expr,$notify:path)=>{{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let query=sql("update process_pilot_runs set lease_until=lease_until where id=? and lease_token=? and channel_id=?uuid",$pg);
            if sqlx::query(&query).bind(run).bind(token).bind(channel).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1 { return Err(RepositoryError::Conflict); }
            let query=sql("select cast(message_id as text) from process_pilot_tasks where id=? and run_id=?",$pg);
            if let Some(message)=sqlx::query_scalar::<_,String>(&query).bind(task.id.to_string()).bind(run).fetch_optional(&mut *tx).await.map_err(storage)? {
                let query=sql("update process_pilot_tasks set status=case when status='completed' then status else ? end,delivery_status=case when ?='completed' then 'ready' else delivery_status end where id=?",$pg);
                sqlx::query(&query).bind(&task.status).bind(&task.status).bind(task.id.to_string()).execute(&mut *tx).await.map_err(storage)?;
                tx.commit().await.map_err(storage)?; Ok((Uuid::parse_str(&message).map_err(storage)?,false))
            }else{
                let query=sql("update channel_sequences set next_sequence=next_sequence+1 where channel_id=?uuid returning cast(next_sequence-1 as text)",$pg);
                let sequence=sqlx::query_scalar::<_,String>(&query).bind(channel).fetch_one(&mut *tx).await.map_err(storage)?;
                let query=sql("insert into users(id,kind,display_name) values(?uuid,'agent','Heart') on conflict(id) do nothing",$pg);
                sqlx::query(&query).bind(BOT).execute(&mut *tx).await.map_err(storage)?;
                let query=sql("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) select ?uuid,actor_id,actor_id,'heart-pilot','user-task-pilot','Present Heart user tasks',60,current_timestamp from process_pilot_runs where id=? on conflict(agent_id) do nothing",$pg);
                sqlx::query(&query).bind(BOT).bind(run).execute(&mut *tx).await.map_err(storage)?;
                let message=Uuid::now_v7();
                let query=sql("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Heart',?int,?)",$pg);
                sqlx::query(&query).bind(message.to_string()).bind(channel).bind(BOT).bind(&sequence).bind(format!("[[process-task:{}]]",task.id)).execute(&mut *tx).await.map_err(storage)?;
                let projected=crate::domain::ChatMessage {
                    id:MessageId::from_uuid(message),channel_id:crate::domain::ChannelId::new(channel).map_err(storage)?,parent_message_id:None,
                    sender_id:UserId::from_uuid(Uuid::parse_str(BOT).map_err(storage)?),sender_display_name:crate::domain::DisplayName::new("Heart").map_err(storage)?,
                    sequence:crate::domain::ChannelSequence::try_from(sequence.parse::<i64>().map_err(storage)?).map_err(storage)?,
                    body:crate::domain::MessageBody::new(format!("[[process-task:{}]]",task.id)).map_err(storage)?,sent_at:Utc::now(),edited_at:None,deleted_at:None,
                };
                $notify(&mut tx,&projected).await?;
                let query=sql("insert into process_pilot_tasks(id,run_id,message_id,node_id,status) values(?,?,?uuid,?,?)",$pg);
                sqlx::query(&query).bind(task.id.to_string()).bind(run).bind(message.to_string()).bind(&task.node_id).bind(&task.status).execute(&mut *tx).await.map_err(storage)?;
                tx.commit().await.map_err(storage)?; Ok((message,true))
            }
        }};}
        match &self.store {
            Store::Pg(pool) => project!(pool, true, crate::notification::enqueue_message_postgres),
            Store::Sqlite(pool) => {
                project!(pool, false, crate::notification::enqueue_message_sqlite)
            }
        }
    }
}

fn valid_tasks(tasks: &[HeartTask], instance: &str, assignee: &str) -> bool {
    if tasks.is_empty()
        || tasks.len() > 2
        || tasks.iter().any(|t| {
            t.instance_id.to_string() != instance
                || t.assignee_id.to_string() != assignee
                || !matches!(t.status.as_str(), "pending" | "completed")
        })
    {
        return false;
    }
    let first = tasks.iter().find(|t| t.node_id == "first");
    match (
        tasks.len(),
        first,
        tasks.iter().find(|t| t.node_id == "second"),
    ) {
        (1, Some(first), None) => first.status == "pending",
        (2, Some(first), Some(second)) => first.status == "completed" && first.id != second.id,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn fixture(pg: Option<&str>) -> (ProcessPilot, UserId, UserId, String) {
        let store = if let Some(url) = pg {
            let pool = PgPool::connect(url).await.unwrap();
            sqlx::migrate!("./migrations/postgres")
                .run(&pool)
                .await
                .unwrap();
            Store::Pg(pool)
        } else {
            let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
            sqlx::migrate!("./migrations/sqlite")
                .run(&pool)
                .await
                .unwrap();
            Store::Sqlite(pool)
        };
        let mut pilot = ProcessPilot {
            store,
            base: "http://127.0.0.1:1".into(),
            configurator: Uuid::nil(),
            http: reqwest::Client::new(),
            gateway: Arc::new(
                HeartGateway::new("http://127.0.0.1:1", Duration::from_secs(1), 0).unwrap(),
            ),
        };
        let owner = UserId::from_uuid(Uuid::now_v7());
        let reader = UserId::from_uuid(Uuid::now_v7());
        pilot.configurator = *owner.as_uuid();
        let circle = Uuid::now_v7().to_string();
        let channel = Uuid::now_v7().to_string();
        for user in [&owner, &reader] {
            pilot
                .store
                .execute(
                    "insert into users(id,kind,display_name) values(?uuid,'human','Tester')",
                    &[user.to_string()],
                )
                .await
                .unwrap();
        }
        pilot.store.execute("insert into circles(id,slug,name,created_by) values(?uuid,?,'Rocket-admins',?uuid)",&[circle.clone(),circle.clone(),owner.to_string()]).await.unwrap();
        pilot.store.execute("insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')", &[circle.clone(),owner.to_string()]).await.unwrap();
        pilot.store.execute("insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Prosesstest','private',?uuid,?uuid)",&[channel.clone(),channel.clone(),owner.to_string(),circle]).await.unwrap();
        pilot
            .store
            .execute(
                "insert into channel_sequences(channel_id) values(?uuid)",
                std::slice::from_ref(&channel),
            )
            .await
            .unwrap();
        for (user, role) in [(&owner, "owner"), (&reader, "member")] {
            pilot.store.execute("insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,?)", &[channel.clone(),user.to_string(),role.into()]).await.unwrap();
        }
        (pilot, owner, reader, channel)
    }
    async fn exercise(pilot: ProcessPilot, owner: UserId, reader: UserId, channel: String) {
        assert!(
            !pilot
                .configuration(&owner, &channel)
                .await
                .unwrap()
                .configured
        );
        assert!(matches!(
            pilot.configure(&reader, &channel, true).await,
            Err(RepositoryError::PermissionDenied)
        ));
        assert!(
            pilot
                .configure(&owner, &channel, true)
                .await
                .unwrap()
                .can_start
        );
        assert!(
            !pilot
                .configuration(&reader, &channel)
                .await
                .unwrap()
                .can_start
        );
        let key = Uuid::now_v7();
        let run = pilot.start(&owner, &channel, key).await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(pilot.start(&owner, &channel, key).await.unwrap()["id"], run);
        let instance = Uuid::now_v7();
        pilot
            .store
            .execute(
                "update process_pilot_runs set instance_id=?,status='waiting' where id=?",
                &[instance.to_string(), run.clone()],
            )
            .await
            .unwrap();
        let mut task = HeartTask {
            id: Uuid::now_v7(),
            instance_id: instance,
            node_id: "first".into(),
            assignee_id: *owner.as_uuid(),
            status: "pending".into(),
        };
        let (message, created) = pilot.project(&run, &channel, "", &task).await.unwrap();
        assert!(created);
        assert_eq!(
            pilot.project(&run, &channel, "", &task).await.unwrap(),
            (message, false)
        );
        assert!(
            !pilot
                .task(&reader, &task.id.to_string(), &message.to_string())
                .await
                .unwrap()
                .can_complete
        );
        assert!(matches!(
            pilot
                .complete(
                    &reader,
                    &task.id.to_string(),
                    &message.to_string(),
                    Uuid::now_v7()
                )
                .await,
            Err(RepositoryError::PermissionDenied)
        ));
        assert!(
            pilot
                .task(&owner, &task.id.to_string(), &Uuid::now_v7().to_string())
                .await
                .is_err()
        );
        let completion = Uuid::now_v7();
        let view = pilot
            .complete(
                &owner,
                &task.id.to_string(),
                &message.to_string(),
                completion,
            )
            .await
            .unwrap();
        assert_eq!(view.status, "pending");
        assert_eq!(view.delivery_status, "pending");
        pilot
            .complete(
                &owner,
                &task.id.to_string(),
                &message.to_string(),
                Uuid::now_v7(),
            )
            .await
            .unwrap();
        assert_eq!(
            pilot
                .store
                .values(
                    "select command_id from process_pilot_tasks where id=?",
                    &[task.id.to_string()]
                )
                .await
                .unwrap(),
            vec![completion.to_string()]
        );
        task.status = "completed".into();
        pilot.project(&run, &channel, "", &task).await.unwrap();
        assert_eq!(
            pilot
                .complete(
                    &owner,
                    &task.id.to_string(),
                    &message.to_string(),
                    completion
                )
                .await
                .unwrap()
                .status,
            "completed"
        );
        let second = HeartTask {
            id: Uuid::now_v7(),
            instance_id: instance,
            node_id: "second".into(),
            assignee_id: *owner.as_uuid(),
            status: "pending".into(),
        };
        let (second_message, _) = pilot.project(&run, &channel, "", &second).await.unwrap();
        assert_ne!(message, second_message);
        assert_eq!(
            pilot
                .store
                .values(
                    "select cast(count(*) as text) from messages where channel_id=?uuid",
                    std::slice::from_ref(&channel)
                )
                .await
                .unwrap(),
            vec!["2"]
        );
        pilot
            .store
            .execute(
                "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
                &[channel.clone(), owner.to_string()],
            )
            .await
            .unwrap();
        assert!(
            pilot
                .complete(
                    &owner,
                    &second.id.to_string(),
                    &second_message.to_string(),
                    Uuid::now_v7()
                )
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn sqlite_projection_permissions_and_completion_receipts() {
        let (p, o, r, c) = fixture(None).await;
        exercise(p, o, r, c).await;
    }
    #[tokio::test]
    async fn scheduler_reaches_runs_beyond_first_batch_even_when_heart_is_down() {
        let (pilot, owner, _, channel) = fixture(None).await;
        pilot.configure(&owner, &channel, true).await.unwrap();
        for _ in 0..11 {
            pilot.start(&owner, &channel, Uuid::now_v7()).await.unwrap();
        }
        let repo = Arc::new(
            crate::db::SqliteChatRepository::connect("sqlite::memory:")
                .await
                .unwrap(),
        );
        repo.migrate().await.unwrap();
        let chat = ChatEngine::start(repo);
        pilot.tick(&chat).await.unwrap();
        pilot.tick(&chat).await.unwrap();
        assert_eq!(
            pilot
                .store
                .values(
                    "select cast(count(*) as text) from process_pilot_runs where lease_until>0",
                    &[]
                )
                .await
                .unwrap(),
            vec!["11"]
        );
    }
    #[tokio::test]
    async fn stale_worker_cannot_regress_status_or_publish_into_another_run() {
        let (pilot, owner, _, channel) = fixture(None).await;
        pilot.configure(&owner, &channel, true).await.unwrap();
        let run = pilot.start(&owner, &channel, Uuid::now_v7()).await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let mut task = HeartTask {
            id: Uuid::now_v7(),
            instance_id: Uuid::now_v7(),
            assignee_id: *owner.as_uuid(),
            node_id: "first".into(),
            status: "completed".into(),
        };
        let (message, _) = pilot.project(&run, &channel, "", &task).await.unwrap();
        task.status = "pending".into();
        pilot.project(&run, &channel, "", &task).await.unwrap();
        assert_eq!(
            pilot
                .task(&owner, &task.id.to_string(), &message.to_string())
                .await
                .unwrap()
                .status,
            "completed"
        );
        pilot
            .store
            .execute(
                "update process_pilot_runs set lease_token='new-worker' where id=?",
                std::slice::from_ref(&run),
            )
            .await
            .unwrap();
        assert!(pilot.project(&run, &channel, "", &task).await.is_err());
        let other = pilot.start(&owner, &channel, Uuid::now_v7()).await.unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(pilot.project(&other, &channel, "", &task).await.is_err());
    }
    #[test]
    fn malformed_heart_task_sets_are_rejected_as_a_whole() {
        let instance = Uuid::now_v7();
        let assignee = Uuid::now_v7();
        let mut tasks = vec![
            HeartTask {
                id: Uuid::now_v7(),
                instance_id: instance,
                assignee_id: assignee,
                node_id: "first".into(),
                status: "pending".into(),
            },
            HeartTask {
                id: Uuid::now_v7(),
                instance_id: instance,
                assignee_id: assignee,
                node_id: "second".into(),
                status: "pending".into(),
            },
        ];
        assert!(!valid_tasks(
            &tasks,
            &instance.to_string(),
            &assignee.to_string()
        ));
        tasks[0].status = "completed".into();
        assert!(valid_tasks(
            &tasks,
            &instance.to_string(),
            &assignee.to_string()
        ));
        tasks[1].id = tasks[0].id;
        assert!(!valid_tasks(
            &tasks,
            &instance.to_string(),
            &assignee.to_string()
        ));
    }
    #[tokio::test]
    #[ignore = "requires dedicated migrated PostgreSQL test database"]
    async fn postgres_projection_permissions_and_completion_receipts() {
        let url = std::env::var("SPROYT_TEST_DATABASE_URL").unwrap();
        let (p, o, r, c) = fixture(Some(&url)).await;
        exercise(p, o, r, c).await;
    }

    #[tokio::test]
    #[ignore = "requires actual Heart user-task API and dedicated PostgreSQL database"]
    async fn actual_heart_two_steps_restart_and_uncertain_completion() {
        let url = std::env::var("SPROYT_TEST_DATABASE_URL").unwrap();
        let base = std::env::var("SPROYT_TEST_HEART_URL").unwrap();
        let (mut pilot, owner, reader, channel) = fixture(Some(&url)).await;
        pilot.base = base.clone();
        pilot.gateway =
            Arc::new(HeartGateway::new(base.clone(), Duration::from_secs(10), 0).unwrap());
        let response = pilot
            .http
            .post(format!("{base}/api/v1/definitions"))
            .header("Content-Type", "text/plain")
            .body(include_str!(
                "../helm/sproyt/definitions/user-task-pilot.yaml"
            ))
            .send()
            .await
            .unwrap();
        assert!(
            response.status().is_success(),
            "definition registration: {}",
            response.text().await.unwrap()
        );
        let Store::Pg(pool) = &pilot.store else {
            unreachable!()
        };
        let repository = Arc::new(
            crate::db::PostgresChatRepository::connect_with_pool(&url, pool.clone())
                .await
                .unwrap(),
        );
        let chat = ChatEngine::start(repository);
        pilot.configure(&owner, &channel, true).await.unwrap();
        pilot.start(&owner, &channel, Uuid::now_v7()).await.unwrap();
        pilot.tick(&chat).await.unwrap();
        let first=pilot.store.values("select t.id || '|' || cast(t.message_id as text) from process_pilot_tasks t join process_pilot_runs r on r.id=t.run_id where r.channel_id=?uuid and t.node_id='first'",std::slice::from_ref(&channel)).await.unwrap();
        assert_eq!(first.len(), 1);
        let (first_id, first_message) = first[0].split_once('|').unwrap();
        assert!(
            pilot
                .complete(&reader, first_id, first_message, Uuid::now_v7())
                .await
                .is_err()
        );
        pilot
            .complete(&owner, first_id, first_message, Uuid::now_v7())
            .await
            .unwrap();
        let mut disconnected = pilot.clone();
        disconnected.base = "http://127.0.0.1:1".into();
        disconnected.tick(&chat).await.unwrap();
        assert_eq!(
            pilot
                .task(&owner, first_id, first_message)
                .await
                .unwrap()
                .delivery_status,
            "pending"
        );
        assert_eq!(
            pilot
                .store
                .values(
                    "select cast(count(*) as text) from messages where channel_id=?uuid",
                    std::slice::from_ref(&channel)
                )
                .await
                .unwrap(),
            vec!["1"]
        );
        // New service object represents restarting Sprøyt; all state lives in DB.
        let restarted = pilot.clone();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        restarted.tick(&chat).await.unwrap();
        restarted.tick(&chat).await.unwrap();
        assert_eq!(
            restarted
                .task(&owner, first_id, first_message)
                .await
                .unwrap()
                .status,
            "completed"
        );
        let second=restarted.store.values("select t.id || '|' || cast(t.message_id as text) from process_pilot_tasks t join process_pilot_runs r on r.id=t.run_id where r.channel_id=?uuid and t.node_id='second'",std::slice::from_ref(&channel)).await.unwrap();
        assert_eq!(second.len(), 1);
        let (second_id, second_message) = second[0].split_once('|').unwrap();
        // A replay of the first completion cannot advance the second task.
        restarted
            .complete(&owner, first_id, first_message, Uuid::now_v7())
            .await
            .unwrap();
        assert_eq!(
            restarted
                .task(&owner, second_id, second_message)
                .await
                .unwrap()
                .status,
            "pending"
        );
        restarted
            .complete(&owner, second_id, second_message, Uuid::now_v7())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(1100)).await;
        restarted.tick(&chat).await.unwrap();
        assert_eq!(
            restarted
                .store
                .values(
                    "select status from process_pilot_runs where channel_id=?uuid",
                    std::slice::from_ref(&channel)
                )
                .await
                .unwrap(),
            vec!["completed"]
        );
        assert_eq!(
            restarted
                .store
                .values(
                    "select cast(count(*) as text) from messages where channel_id=?uuid",
                    &[channel]
                )
                .await
                .unwrap(),
            vec!["2"]
        );
    }
}
