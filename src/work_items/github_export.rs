//! The reviewed case stays in Sprøyt; Heart owns a linked publication task.
//! Command acceptance, external delivery and Heart completion are separate receipts.
use super::*;
use crate::github::{Destination, Issue};

const DEFINITION: &str = include_str!("../../helm/sproyt/definitions/work-item-github-export.yaml");

#[cfg(test)]
#[path = "github_export_tests.rs"]
pub(crate) mod tests;

#[derive(Clone, Deserialize)]
pub(crate) struct ExportCommand {
    pub message_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub title: String,
    pub body: String,
    pub send: bool,
    pub expected_repository_id: Option<i64>,
    pub expected_binding_revision: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct ExportView {
    pub repository: Option<String>,
    pub repository_id: Option<i64>,
    pub binding_revision: Option<i64>,
    pub can_publish: bool,
    pub status: String,
    pub issue_url: Option<String>,
    pub title: Option<String>,
    pub body: Option<String>,
}

struct Receipt {
    item: String,
    task: String,
    actor: String,
    target: Destination,
    binding_revision: i64,
    title: String,
    body: String,
    marker: String,
    status: String,
}

impl WorkItems {
    pub(super) async fn export_view(&self, actor: UserId, item: Uuid) -> Result<ExportView> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let query=sql("select coalesce(e.repository_name,b.repository_name) as repository,coalesce(e.repository_id,b.repository_id) as repository_id,coalesce(e.binding_revision,b.revision) as binding_revision,coalesce(e.status,'ready') as status,e.issue_url,e.title,e.body from work_items w left join work_github_bindings b on b.application_id=w.application_id left join work_item_github_exports e on e.work_item_id=w.id where w.id=?uuid",$pg);
            let row=sqlx::query(&query).bind(item.to_string()).fetch_one($pool).await.map_err(storage)?;
            let can_publish=self.github.is_some() && self.export_allowed(&item.to_string(),&actor.to_string(),None).await?;
            Ok(ExportView {repository:row.try_get("repository").map_err(storage)?,repository_id:row.try_get("repository_id").map_err(storage)?,binding_revision:row.try_get("binding_revision").map_err(storage)?,can_publish,status:row.try_get("status").map_err(storage)?,issue_url:row.try_get("issue_url").map_err(storage)?,title:row.try_get("title").map_err(storage)?,body:row.try_get("body").map_err(storage)?})
        }}; }
        match &self.store {
            Store::Pg(pool) => read!(pool, true),
            Store::Sqlite(pool) => read!(pool, false),
        }
    }

    // Re-evaluated before every first external create. Read-only reconciliation
    // of an already attempted create deliberately does not depend on these rights.
    async fn export_allowed(
        &self,
        item: &str,
        actor: &str,
        receipt: Option<&Receipt>,
    ) -> Result<bool> {
        macro_rules! check { ($pool:expr,$pg:expr) => {{
            let query=sql("select b.installation_id,b.repository_id,b.repository_name,b.bot_login,b.revision from work_items w join work_item_export_processes x on x.work_item_id=w.id and x.assignee_id=?uuid join work_applications a on a.id=w.application_id and a.enabled join channel_task_routes er on er.binding_id=w.binding_id and er.channel_id=w.task_channel_id and er.task_key='publish-github' and er.process_role='product-handler' and er.enabled join work_github_bindings b on b.application_id=a.id and b.enabled and b.export_tasks_enabled join application_processors p on p.application_id=a.id and p.user_id=x.assignee_id and p.can_review and p.can_export join application_process_roles r on r.application_id=a.id and r.user_id=p.user_id and r.process_role='product-handler' join channel_memberships cm on cm.channel_id=x.channel_id and cm.user_id=p.user_id and cm.role<>'observer' where w.id=?uuid and w.process_status='completed' and w.status='planned' and x.status='waiting'",$pg);
            let row=sqlx::query(&query).bind(actor).bind(item).fetch_optional($pool).await.map_err(storage)?;
            match row {
                None=>Ok(false),
                Some(row)=>Ok(receipt.is_none_or(|e| row.get::<i64,_>("installation_id")==e.target.installation && row.get::<i64,_>("repository_id")==e.target.repository_id && row.get::<String,_>("repository_name")==e.target.repository && row.get::<String,_>("bot_login")==e.target.bot_login && row.get::<i64,_>("revision")==e.binding_revision)),
            }
        }}; }
        match &self.store {
            Store::Pg(pool) => check!(pool, true),
            Store::Sqlite(pool) => check!(pool, false),
        }
    }

    pub(crate) async fn export_github(
        &self,
        actor: UserId,
        task: Uuid,
        command: ExportCommand,
    ) -> Result<TaskView> {
        if command.expected_revision < 1
            || command.title.chars().count() > 160
            || command.body.len() > 8000
            || (command.send
                && (command.title.trim().is_empty()
                    || command.body.trim().is_empty()
                    || self.github.is_none()))
            || (command.send
                && (command.expected_repository_id.is_none()
                    || command.expected_binding_revision.is_none()))
            || (!command.send
                && (!command.title.is_empty()
                    || !command.body.is_empty()
                    || command.expected_repository_id.is_some()
                    || command.expected_binding_revision.is_some()))
            || command.body.contains("<!-- sproyt-work-item-export:")
        {
            return Err(RepositoryError::Conflict);
        }
        macro_rules! accept { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let rights=sql("select cast(w.id as text) as item,w.revision,w.status as case_status,x.status as process_status,t.status as task_status,p.can_export from work_item_tasks t join work_items w on w.id=t.work_item_id join work_item_export_processes x on x.work_item_id=w.id and x.assignee_id=t.assignee_id and x.channel_id=t.channel_id join channel_memberships cm on cm.channel_id=t.channel_id and cm.user_id=?uuid and cm.role<>'observer' join application_processors p on p.application_id=w.application_id and p.user_id=cm.user_id and p.can_review join application_process_roles r on r.application_id=w.application_id and r.user_id=p.user_id and r.process_role='product-handler' where t.id=?uuid and t.message_id=?uuid and t.assignee_id=cm.user_id and t.node_id='publish-github'",$pg);
            let row=sqlx::query(&rights).bind(actor.to_string()).bind(task.to_string()).bind(command.message_id.to_string()).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
            if command.send && !row.try_get::<bool,_>("can_export").map_err(storage)? {return Err(RepositoryError::PermissionDenied);}
            let item:String=row.try_get("item").map_err(storage)?;
            let existing=sql("select cast(request_id as text) as request_id,expected_revision,disposition,title,body,repository_id,binding_revision from work_item_github_exports where work_item_id=?uuid",$pg);
            if let Some(saved)=sqlx::query(&existing).bind(&item).fetch_optional(&mut *tx).await.map_err(storage)? {
                if saved.try_get::<String,_>("request_id").map_err(storage)?!=command.request_id.to_string() || saved.try_get::<i64,_>("expected_revision").map_err(storage)?!=command.expected_revision
                    || saved.try_get::<String,_>("disposition").map_err(storage)?!=if command.send {"publish"} else {"skip"}
                    || saved.try_get::<String,_>("title").map_err(storage)?!=command.title || saved.try_get::<String,_>("body").map_err(storage)?!=command.body
                    || (command.send && (saved.try_get::<Option<i64>,_>("repository_id").map_err(storage)?!=command.expected_repository_id || saved.try_get::<Option<i64>,_>("binding_revision").map_err(storage)?!=command.expected_binding_revision)) {return Err(RepositoryError::Conflict);}
                tx.commit().await.map_err(storage)?;
                return self.task(actor,task,command.message_id).await;
            }
            if row.try_get::<String,_>("task_status").map_err(storage)?!="pending" || row.try_get::<String,_>("process_status").map_err(storage)?!="waiting" || row.try_get::<String,_>("case_status").map_err(storage)?!="planned" || row.try_get::<i64,_>("revision").map_err(storage)?!=command.expected_revision {return Err(RepositoryError::Conflict);}
            let binding=sql("select b.installation_id,b.repository_id,b.repository_name,b.bot_login,b.revision from work_github_bindings b join work_items w on w.application_id=b.application_id join work_applications a on a.id=b.application_id and a.enabled where w.id=?uuid and b.enabled and b.export_tasks_enabled and exists(select 1 from channel_task_routes er where er.binding_id=w.binding_id and er.channel_id=w.task_channel_id and er.task_key='publish-github' and er.process_role='product-handler' and er.enabled)",$pg);
            let target=sqlx::query(&binding).bind(&item).fetch_optional(&mut *tx).await.map_err(storage)?;
            if command.send && target.is_none() {return Err(RepositoryError::PermissionDenied);}
            if command.send && target.as_ref().is_none_or(|r| Some(r.get::<i64,_>("repository_id"))!=command.expected_repository_id || Some(r.get::<i64,_>("revision"))!=command.expected_binding_revision) {return Err(RepositoryError::Conflict);}
            let marker=format!("<!-- sproyt-work-item-export:{} -->",command.request_id);
            let insert=sql("insert into work_item_github_exports(work_item_id,task_id,actor_id,request_id,expected_revision,disposition,title,body,installation_id,repository_id,repository_name,bot_login,binding_revision,marker,status,created_at) values(?uuid,?uuid,?uuid,?uuid,?,?,?,?,?,?,?,?,?,?,?,?)",$pg);
            sqlx::query(&insert).bind(&item).bind(task.to_string()).bind(actor.to_string()).bind(command.request_id.to_string()).bind(command.expected_revision).bind(if command.send {"publish"} else {"skip"}).bind(&command.title).bind(&command.body)
                .bind(target.as_ref().map(|r|r.get::<i64,_>("installation_id"))).bind(target.as_ref().map(|r|r.get::<i64,_>("repository_id"))).bind(target.as_ref().map(|r|r.get::<String,_>("repository_name"))).bind(target.as_ref().map(|r|r.get::<String,_>("bot_login"))).bind(target.as_ref().map(|r|r.get::<i64,_>("revision"))).bind(marker).bind(if command.send {"pending"} else {"skipped"}).bind(Utc::now().timestamp()).execute(&mut *tx).await.map_err(storage)?;
            let bump=sql("update work_items set revision=revision+1 where id=?uuid and revision=?",$pg);
            if sqlx::query(&bump).bind(&item).bind(command.expected_revision).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
            let accepted=sql("update work_item_tasks set decision_request_id=?uuid,decision_revision=?,delivery_status='pending',decision_note=? where id=?uuid and decision_request_id is null and status='pending'",$pg);
            if sqlx::query(&accepted).bind(command.request_id.to_string()).bind(command.expected_revision).bind((!command.send).then(||json!({"github_export":"skipped"}).to_string())).bind(task.to_string()).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
            let audit=sql("insert into audit_events(actor_id,action,target_kind,target_id,payload) values(?uuid,'work.github_decision','work_item',?,?json)",$pg).replace("?json","?");
            // PostgreSQL's JSONB needs an explicit cast; payload excludes text and credentials.
            let audit=if $pg {audit.replace("$3json","$3::jsonb")} else {audit.replace("?json","?")};
            sqlx::query(&audit).bind(actor.to_string()).bind(&item).bind(json!({"request_id":command.request_id,"send":command.send}).to_string()).execute(&mut *tx).await.map_err(storage)?;
            tx.commit().await.map_err(storage)?;
            self.task(actor,task,command.message_id).await
        }}; }
        match &self.store {
            Store::Pg(pool) => accept!(pool, true),
            Store::Sqlite(pool) => accept!(pool, false),
        }
    }

    pub(super) async fn sync_exports(&self, chat: &ChatEngine, now: i64) -> Result<()> {
        if self.github.is_none() {
            return Ok(());
        }
        macro_rules! seed { ($pool:expr,$pg:expr) => {{
            let query=sql("insert into work_item_export_processes(work_item_id,channel_id,assignee_id,created_at) select w.id,w.task_channel_id,w.reviewer_id,? from work_items w join work_applications a on a.id=w.application_id and a.enabled join channel_task_routes er on er.binding_id=w.binding_id and er.channel_id=w.task_channel_id and er.task_key='publish-github' and er.process_role='product-handler' and er.enabled join work_github_bindings b on b.application_id=w.application_id and b.enabled and b.export_tasks_enabled where w.process_status='completed' and w.start_status='started' and w.status='planned' on conflict(work_item_id) do nothing",$pg);
            sqlx::query(&query).bind(now).execute($pool).await.map_err(storage)?;
            let ids=sql("select cast(work_item_id as text) from work_item_export_processes where status in ('pending','waiting') and lease_until<? order by lease_until,created_at limit 10",$pg);
            sqlx::query_scalar::<_,String>(&ids).bind(now).fetch_all($pool).await.map_err(storage)?
        }}; }
        let ids = match &self.store {
            Store::Pg(pool) => seed!(pool, true),
            Store::Sqlite(pool) => seed!(pool, false),
        };
        for item in ids {
            let now = Utc::now().timestamp();
            let lease = Uuid::now_v7().to_string();
            macro_rules! claim { ($pool:expr,$pg:expr) => {{
                let query=sql("update work_item_export_processes set lease_until=?,lease_token=?uuid where work_item_id=?uuid and lease_until<? and status in ('pending','waiting')",$pg);
                sqlx::query(&query).bind(now+180).bind(&lease).bind(&item).bind(now).execute($pool).await.map_err(storage)?.rows_affected()
            }}; }
            let claimed = match &self.store {
                Store::Pg(pool) => claim!(pool, true),
                Store::Sqlite(pool) => claim!(pool, false),
            };
            if claimed == 0 {
                continue;
            }
            let outcome = self.sync_export_process(chat, &item, &lease).await;
            let now = Utc::now().timestamp();
            macro_rules! release { ($pool:expr,$pg:expr) => {{
                let query=sql("update work_item_export_processes set lease_until=?,lease_token=null where work_item_id=?uuid and lease_token=?uuid",$pg);
                sqlx::query(&query).bind(now+if outcome.is_ok(){5}else{30}).bind(&item).bind(&lease).execute($pool).await.map_err(storage)?;
            }}; }
            match &self.store {
                Store::Pg(pool) => release!(pool, true),
                Store::Sqlite(pool) => release!(pool, false),
            };
            if outcome.is_err() {
                tracing::warn!("GitHub user-task synchronization deferred");
            }
        }
        self.deliver_exports(now).await
    }

    async fn sync_export_process(&self, chat: &ChatEngine, item: &str, lease: &str) -> Result<()> {
        let base = self
            .heart_url
            .as_ref()
            .ok_or_else(|| storage("Heart unavailable"))?;
        macro_rules! details { ($pool:expr,$pg:expr) => {{
            let query=sql("select cast(x.heart_instance_id as text) as instance,cast(x.assignee_id as text) as assignee,cast(x.channel_id as text) as channel,cast(w.application_id as text) as application from work_item_export_processes x join work_items w on w.id=x.work_item_id where x.work_item_id=?uuid and x.lease_token=?uuid",$pg);
            let row=sqlx::query(&query).bind(item).bind(lease).fetch_one($pool).await.map_err(storage)?;
            (row.try_get::<Option<String>,_>("instance").map_err(storage)?,row.try_get::<String,_>("assignee").map_err(storage)?,row.try_get::<String,_>("channel").map_err(storage)?,row.try_get::<String,_>("application").map_err(storage)?)
        }}; }
        let (existing, assignee, channel, application) = match &self.store {
            Store::Pg(pool) => details!(pool, true),
            Store::Sqlite(pool) => details!(pool, false),
        };
        let instance = if let Some(instance) = existing {
            instance
        } else {
            let definition = self
                .heart_response(
                    self.http
                        .post(format!("{base}/api/v2/definitions"))
                        .header("Content-Type", "application/yaml")
                        .body(DEFINITION),
                )
                .await?;
            if definition["namespace"] != "sproyt"
                || definition["name"] != "work-item-github-export"
                || definition["version"] != "1.0.0"
                || definition["runtime"] != "v2"
            {
                return Err(RepositoryError::Conflict);
            }
            let started=self.heart_response(self.http.post(format!("{base}/api/v2/instances")).header("X-Heart-Client","sproyt-work-items").header("Idempotency-Key",format!("github-export:{item}"))
                .json(&json!({"definition_id":definition["id"],"actor_id":assignee,"input_metadata":{"work_item_id":item,"reviewer_id":assignee,"application_id":application,"task_channel_id":channel}}))).await?;
            let instance = started["instance"]["id"]
                .as_str()
                .ok_or(RepositoryError::Conflict)?
                .to_owned();
            Uuid::parse_str(&instance).map_err(storage)?;
            macro_rules! save { ($pool:expr,$pg:expr) => {{
                let query=sql("update work_item_export_processes set heart_instance_id=?uuid,status='waiting' where work_item_id=?uuid and lease_token=?uuid and heart_instance_id is null",$pg);
                if sqlx::query(&query).bind(&instance).bind(item).bind(lease).execute($pool).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
            }}; }
            match &self.store {
                Store::Pg(pool) => save!(pool, true),
                Store::Sqlite(pool) => save!(pool, false),
            };
            instance
        };
        macro_rules! completion { ($pool:expr,$pg:expr) => {{
            let query=sql("select cast(t.id as text) as id,cast(t.decision_request_id as text) as request,t.decision_note from work_item_tasks t join work_item_github_exports e on e.task_id=t.id and e.status in ('sent','skipped') where t.work_item_id=?uuid and t.node_id='publish-github' and t.status='pending' and t.decision_note is not null",$pg);
            sqlx::query(&query).bind(item).fetch_optional($pool).await.map_err(storage)?.map(|r|Ok::<_,RepositoryError>((r.try_get::<String,_>("id").map_err(storage)?,r.try_get::<String,_>("request").map_err(storage)?,r.try_get::<String,_>("decision_note").map_err(storage)?))).transpose()?
        }}; }
        let completion = match &self.store {
            Store::Pg(pool) => completion!(pool, true),
            Store::Sqlite(pool) => completion!(pool, false),
        };
        if let Some((task, request, note)) = completion {
            let result: Value = serde_json::from_str(&note).map_err(storage)?;
            let _ = self
                .http
                .post(format!("{base}/api/v2/user-tasks/{task}/complete"))
                .header("X-Heart-Client", "sproyt-work-items")
                .header("Idempotency-Key", request)
                .json(&json!({"actor_id":assignee,"result_metadata":result}))
                .send()
                .await;
        }
        let view = self
            .heart_response(self.http.get(format!("{base}/api/v2/instances/{instance}")))
            .await?;
        if view["id"] != instance
            || view["namespace"] != "sproyt"
            || view["runtime"] != "v2"
            || view["input_metadata"]["work_item_id"] != item
            || view["input_metadata"]["reviewer_id"] != assignee
            || view["input_metadata"]["application_id"] != application
            || view["input_metadata"]["task_channel_id"] != channel
        {
            return Err(RepositoryError::Conflict);
        }
        let state = view["status"].as_str().ok_or(RepositoryError::Conflict)?;
        if !matches!(
            state,
            "running" | "waiting" | "completed" | "cancelled" | "failed"
        ) {
            return Err(RepositoryError::Conflict);
        }
        let tasks: Vec<HeartTask> = serde_json::from_value(
            self.heart_response(
                self.http
                    .get(format!("{base}/api/v2/user-tasks"))
                    .query(&[("instance_id", &instance)]),
            )
            .await?,
        )
        .map_err(storage)?;
        if tasks.len() > 1
            || (state == "completed" && (tasks.len() != 1 || tasks[0].status != "completed"))
        {
            return Err(RepositoryError::Conflict);
        }
        for task in tasks {
            if task.node_id != "publish-github"
                || task.instance_id.to_string() != instance
                || task.assignee_id.to_string() != assignee
                || !matches!(task.status.as_str(), "pending" | "completed" | "cancelled")
            {
                return Err(RepositoryError::Conflict);
            }
            if let Some(message) = self.project(item, lease, &channel, &task).await? {
                chat.announce_persisted_message(MessageId::from_uuid(message))
                    .await
                    .map_err(storage)?;
            }
        }
        macro_rules! save { ($pool:expr,$pg:expr) => {{
            let query=sql("update work_item_export_processes set status=? where work_item_id=?uuid and lease_token=?uuid",$pg);
            sqlx::query(&query).bind(if state=="running" {"waiting"} else {state}).bind(item).bind(lease).execute($pool).await.map_err(storage)?;
        }}; }
        match &self.store {
            Store::Pg(pool) => save!(pool, true),
            Store::Sqlite(pool) => save!(pool, false),
        };
        Ok(())
    }

    async fn deliver_exports(&self, now: i64) -> Result<()> {
        macro_rules! ids { ($pool:expr,$pg:expr) => {{
            let query=sql("select cast(work_item_id as text) from work_item_github_exports where status in ('pending','blocked','sending','uncertain') and lease_until<? order by lease_until,created_at limit 5",$pg);
            sqlx::query_scalar::<_,String>(&query).bind(now).fetch_all($pool).await.map_err(storage)?
        }}; }
        let ids = match &self.store {
            Store::Pg(pool) => ids!(pool, true),
            Store::Sqlite(pool) => ids!(pool, false),
        };
        for item in ids {
            let now = Utc::now().timestamp();
            let lease = Uuid::now_v7().to_string();
            macro_rules! claim { ($pool:expr,$pg:expr) => {{
                let query=sql("update work_item_github_exports set lease_until=?,lease_token=?uuid,status=case when status='sending' then 'uncertain' else status end where work_item_id=?uuid and lease_until<? and status in ('pending','blocked','sending','uncertain')",$pg);
                sqlx::query(&query).bind(now+600).bind(&lease).bind(&item).bind(now).execute($pool).await.map_err(storage)?.rows_affected()
            }}; }
            let changed = match &self.store {
                Store::Pg(pool) => claim!(pool, true),
                Store::Sqlite(pool) => claim!(pool, false),
            };
            if changed == 0 {
                continue;
            }
            let outcome = self.deliver_export(&item, &lease, now).await;
            let now = Utc::now().timestamp();
            macro_rules! release { ($pool:expr,$pg:expr) => {{
                let query=sql("update work_item_github_exports set lease_until=?,lease_token=null where work_item_id=?uuid and lease_token=?uuid",$pg);
                sqlx::query(&query).bind(now+60).bind(&item).bind(&lease).execute($pool).await.map_err(storage)?;
            }}; }
            match &self.store {
                Store::Pg(pool) => release!(pool, true),
                Store::Sqlite(pool) => release!(pool, false),
            };
            if outcome.is_err() {
                tracing::warn!("GitHub export deferred; receipt retained");
            }
        }
        Ok(())
    }

    async fn deliver_export(&self, item: &str, lease: &str, now: i64) -> Result<()> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let query=sql("select cast(task_id as text) as task,cast(actor_id as text) as actor,installation_id,repository_id,repository_name,bot_login,binding_revision,title,body,marker,status from work_item_github_exports where work_item_id=?uuid and lease_token=?uuid",$pg);
            let row=sqlx::query(&query).bind(item).bind(lease).fetch_one($pool).await.map_err(storage)?;
            Receipt {item:item.into(),task:row.try_get("task").map_err(storage)?,actor:row.try_get("actor").map_err(storage)?,target:Destination{installation:row.try_get("installation_id").map_err(storage)?,repository_id:row.try_get("repository_id").map_err(storage)?,repository:row.try_get("repository_name").map_err(storage)?,bot_login:row.try_get("bot_login").map_err(storage)?},binding_revision:row.try_get("binding_revision").map_err(storage)?,title:row.try_get("title").map_err(storage)?,body:row.try_get("body").map_err(storage)?,marker:row.try_get("marker").map_err(storage)?,status:row.try_get("status").map_err(storage)?}
        }}; }
        let receipt = match &self.store {
            Store::Pg(pool) => read!(pool, true),
            Store::Sqlite(pool) => read!(pool, false),
        };
        let github = self
            .github
            .as_ref()
            .ok_or_else(|| storage("GitHub not configured"))?;
        let body = format!("{}\n\n{}", receipt.body, receipt.marker);
        if receipt.status != "uncertain"
            && !self
                .export_allowed(item, &receipt.actor, Some(&receipt))
                .await?
        {
            self.export_state(item, lease, "blocked", None).await?;
            return Ok(());
        }
        let token = github.token(&receipt.target).await.map_err(storage)?;
        if receipt.status == "uncertain" {
            if let Some(issue) = github
                .find(
                    &receipt.target,
                    &token,
                    &receipt.marker,
                    &receipt.title,
                    &body,
                )
                .await
                .map_err(storage)?
            {
                self.settle_export(&receipt, lease, &issue).await?;
            }
            return Ok(());
        }
        github
            .verify(&receipt.target, &token)
            .await
            .map_err(storage)?;
        // A durable attempted state precedes the POST. Expired workers reconcile
        // this state; they cannot reinterpret it as permission to send again.
        if !self
            .export_allowed(item, &receipt.actor, Some(&receipt))
            .await?
        {
            self.export_state(item, lease, "blocked", None).await?;
            return Ok(());
        }
        self.export_state(item, lease, "sending", Some(now)).await?;
        match github
            .create(&receipt.target, &token, &receipt.title, &body)
            .await
        {
            Ok(issue) => self.settle_export(&receipt, lease, &issue).await,
            Err(crate::github::CreateError::Rejected) => {
                self.export_state(item, lease, "blocked", None).await
            }
            Err(crate::github::CreateError::Uncertain) => {
                self.export_state(item, lease, "uncertain", None).await
            }
        }
    }

    async fn export_state(
        &self,
        item: &str,
        lease: &str,
        state: &str,
        attempt: Option<i64>,
    ) -> Result<()> {
        macro_rules! update { ($pool:expr,$pg:expr) => {{
            let query=sql("update work_item_github_exports set status=?,attempted_at=coalesce(?,attempted_at) where work_item_id=?uuid and lease_token=?uuid",$pg);
            if sqlx::query(&query).bind(state).bind(attempt).bind(item).bind(lease).execute($pool).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
        }}; }
        match &self.store {
            Store::Pg(pool) => update!(pool, true),
            Store::Sqlite(pool) => update!(pool, false),
        };
        Ok(())
    }

    async fn settle_export(&self, receipt: &Receipt, lease: &str, issue: &Issue) -> Result<()> {
        macro_rules! settle { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let update=sql("update work_item_github_exports set status='sent',issue_id=?,issue_number=?,issue_url=? where work_item_id=?uuid and lease_token=?uuid and status in ('sending','uncertain')",$pg);
            if sqlx::query(&update).bind(issue.id).bind(issue.number).bind(&issue.url).bind(&receipt.item).bind(lease).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
            let task=sql("update work_item_tasks set decision_note=? where id=?uuid and work_item_id=?uuid and decision_request_id is not null",$pg);
            sqlx::query(&task).bind(json!({"github_export":"sent","issue_number":issue.number,"issue_url":issue.url}).to_string()).bind(&receipt.task).bind(&receipt.item).execute(&mut *tx).await.map_err(storage)?;
            tx.commit().await.map_err(storage)?;
        }}; }
        match &self.store {
            Store::Pg(pool) => settle!(pool, true),
            Store::Sqlite(pool) => settle!(pool, false),
        };
        Ok(())
    }
}
