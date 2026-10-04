//! Sprøyt owns case status and notes; Heart owns each status-change activation.
use super::*;

const DEFINITION: &str = include_str!("../../helm/sproyt/definitions/work-item-status-change.yaml");

#[cfg(test)]
#[path = "status_change_tests.rs"]
mod tests;

#[derive(Clone, Deserialize)]
pub(crate) struct StatusStart {
    pub source_task_id: Uuid,
    pub source_message_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: i64,
}
#[derive(Serialize)]
pub(crate) struct StatusStartReceipt {
    pub id: Uuid,
    pub work_item_id: Uuid,
    pub channel_name: String,
    pub start_status: String,
}
#[derive(Clone, Deserialize)]
pub(crate) struct StatusDecision {
    pub message_id: Uuid,
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub status: String,
    pub internal_note: String,
    pub public_feedback: String,
    pub no_change: bool,
}
#[derive(Serialize)]
pub(crate) struct History {
    pub from_status: String,
    pub to_status: String,
    pub actor_name: String,
    pub created_at: i64,
    pub internal_note: Option<String>,
    pub public_feedback: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct Lifecycle {
    pub case_status: String,
    pub can_start: bool,
    pub allowed_statuses: Vec<String>,
    pub internal_note: Option<String>,
    pub public_feedback: Option<String>,
    pub history: Vec<History>,
}
#[derive(Serialize)]
pub(crate) struct PublicHistory {
    from_status: String,
    to_status: String,
    created_at: i64,
    public_feedback: String,
}
pub(crate) struct PublicStatus {
    visible: bool,
    title: Option<String>,
    application_name: Option<String>,
    status: Option<String>,
    public_feedback: Option<String>,
    history: Vec<PublicHistory>,
}

impl Serialize for PublicStatus {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        if !self.visible {
            return json!({"visible": false}).serialize(serializer);
        }
        json!({"visible": true, "title": self.title, "application_name": self.application_name,
            "status": self.status, "public_feedback": self.public_feedback, "history": self.history})
            .serialize(serializer)
    }
}

fn transitions(from: &str) -> Vec<String> {
    match from {
        "planned" => vec!["in_development", "resolved", "rejected"],
        "in_development" => vec!["planned", "resolved", "rejected"],
        "resolved" | "rejected" => vec!["planned"],
        _ => vec![],
    }
    .into_iter()
    .map(str::to_owned)
    .collect()
}

// A new route cannot redirect an already-created task. Both identity and channel
// are checked again at admission, completion and task/history reads.
const ROUTE: &str = "select cast(r.id as text) as route,cast(r.channel_id as text) as channel,c.name from work_items w join work_applications a on a.id=w.application_id and a.enabled join channel_process_bindings b on b.id=w.binding_id and b.enabled join channel_process_applications ca on ca.binding_id=b.id and ca.application_id=a.id join channel_task_routes r on r.binding_id=b.id and r.task_key='change-status' and r.process_role='product-handler' and r.enabled join channels c on c.id=r.channel_id join channel_memberships cm on cm.channel_id=r.channel_id and cm.user_id=?uuid and cm.role<>'observer' join application_processors p on p.application_id=a.id and p.user_id=cm.user_id and p.can_review join application_process_roles pr on pr.application_id=a.id and pr.user_id=p.user_id and pr.process_role='product-handler' where w.id=?uuid and w.process_status='completed' and w.status in ('planned','in_development','resolved','rejected')";

impl WorkItems {
    pub(super) async fn sync_status_changes(&self, chat: &ChatEngine, now: i64) -> Result<()> {
        if self.heart_url.is_none() {
            return Ok(());
        }
        macro_rules! ids { ($pool:expr,$pg:expr) => {{
            sqlx::query_scalar::<_,String>(&sql("select cast(id as text) from work_item_status_changes where status in ('pending','waiting') and lease_until<? order by lease_until,created_at limit 10",$pg)).bind(now).fetch_all($pool).await.map_err(storage)?
        }}; }
        let ids = match &self.store {
            Store::Pg(p) => ids!(p, true),
            Store::Sqlite(p) => ids!(p, false),
        };
        for id in ids {
            let now = Utc::now().timestamp();
            let lease = Uuid::now_v7().to_string();
            macro_rules! claim { ($pool:expr,$pg:expr) => {{
                sqlx::query(&sql("update work_item_status_changes set lease_until=?,lease_token=?uuid where id=?uuid and lease_until<? and status in ('pending','waiting')",$pg)).bind(now+180).bind(&lease).bind(&id).bind(now).execute($pool).await.map_err(storage)?.rows_affected()
            }}; }
            let changed = match &self.store {
                Store::Pg(p) => claim!(p, true),
                Store::Sqlite(p) => claim!(p, false),
            };
            if changed != 1 {
                continue;
            }
            let result = self.sync_status_one(chat, &id, &lease).await;
            let now = Utc::now().timestamp();
            macro_rules! release { ($pool:expr,$pg:expr) => {{
                sqlx::query(&sql("update work_item_status_changes set lease_until=?,lease_token=null where id=?uuid and lease_token=?uuid",$pg)).bind(now+if result.is_ok(){5}else{30}).bind(&id).bind(&lease).execute($pool).await.map_err(storage)?;
            }}; }
            match &self.store {
                Store::Pg(p) => release!(p, true),
                Store::Sqlite(p) => release!(p, false),
            };
            if result.is_err() {
                tracing::warn!("Case status process deferred; accepted receipt retained");
            }
        }
        Ok(())
    }

    async fn sync_status_one(&self, chat: &ChatEngine, id: &str, lease: &str) -> Result<()> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let row=sqlx::query(&sql("select cast(work_item_id as text) as item,cast(actor_id as text) as actor,cast(channel_id as text) as channel,cast(heart_instance_id as text) as instance,cast(decision_request_id as text) as request,decision_result from work_item_status_changes where id=?uuid and lease_token=?uuid",$pg)).bind(id).bind(lease).fetch_one($pool).await.map_err(storage)?;
            (row.try_get::<String,_>("item").map_err(storage)?,row.try_get::<String,_>("actor").map_err(storage)?,row.try_get::<String,_>("channel").map_err(storage)?,row.try_get::<Option<String>,_>("instance").map_err(storage)?,row.try_get::<Option<String>,_>("request").map_err(storage)?,row.try_get::<Option<String>,_>("decision_result").map_err(storage)?)
        }}; }
        let (item, actor, channel, existing, request, result) = match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        };
        // Status is already durably saved. Publish only an access-controlled marker,
        // independently of whether Heart is currently reachable. Notes stay in DB.
        if result.is_some()
            && let Some(message) = self.project_status_message(id, lease, None).await?
        {
            chat.announce_persisted_message(MessageId::from_uuid(message))
                .await
                .map_err(storage)?;
        }
        let base = self
            .heart_url
            .as_ref()
            .ok_or_else(|| storage("Heart unavailable"))?;
        let instance = if let Some(existing) = existing {
            existing
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
                || definition["name"] != "work-item-status-change"
                || definition["version"] != "1.0.0"
                || definition["runtime"] != "v2"
            {
                return Err(RepositoryError::Conflict);
            }
            let started=self.heart_response(self.http.post(format!("{base}/api/v2/instances")).header("X-Heart-Client","sproyt-work-items").header("Idempotency-Key",format!("case-status:{id}"))
                .json(&json!({"definition_id":definition["id"],"actor_id":actor,"input_metadata":{"status_change_id":id,"work_item_id":item,"handler_id":actor,"task_channel_id":channel}}))).await?;
            let instance = started["instance"]["id"]
                .as_str()
                .ok_or(RepositoryError::Conflict)?
                .to_owned();
            Uuid::parse_str(&instance).map_err(storage)?;
            macro_rules! save { ($pool:expr,$pg:expr) => {{
                if sqlx::query(&sql("update work_item_status_changes set heart_instance_id=?uuid where id=?uuid and lease_token=?uuid and heart_instance_id is null",$pg)).bind(&instance).bind(id).bind(lease).execute($pool).await.map_err(storage)?.rows_affected()!=1 {return Err(RepositoryError::Conflict);}
            }}; }
            match &self.store {
                Store::Pg(p) => save!(p, true),
                Store::Sqlite(p) => save!(p, false),
            };
            instance
        };
        let before = self
            .heart_response(self.http.get(format!("{base}/api/v2/instances/{instance}")))
            .await?;
        if before["id"] != instance
            || before["namespace"] != "sproyt"
            || before["runtime"] != "v2"
            || before["input_metadata"]["status_change_id"] != id
            || before["input_metadata"]["work_item_id"] != item
            || before["input_metadata"]["handler_id"] != actor
            || before["input_metadata"]["task_channel_id"] != channel
        {
            return Err(RepositoryError::Conflict);
        }
        let instance_status = before["status"].as_str().ok_or(RepositoryError::Conflict)?;
        if !matches!(
            instance_status,
            "running" | "waiting" | "completed" | "failed" | "cancelled"
        ) {
            return Err(RepositoryError::Conflict);
        }
        if matches!(instance_status, "failed" | "cancelled") {
            // Keep accepted business decisions; release this operation's slot.
            macro_rules! terminal { ($pool:expr,$pg:expr) => {{
                sqlx::query(&sql("update work_item_status_changes set status=? where id=?uuid and lease_token=?uuid",$pg))
                    .bind(instance_status).bind(id).bind(lease).execute($pool).await.map_err(storage)?;
            }}; }
            match &self.store {
                Store::Pg(p) => terminal!(p, true),
                Store::Sqlite(p) => terminal!(p, false),
            };
            return Ok(());
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
        if tasks.len() != 1 {
            return Err(RepositoryError::Conflict);
        }
        let task = &tasks[0];
        if instance_status == "completed" && task.status != "completed" {
            return Err(RepositoryError::Conflict);
        }
        if task.node_id != "change-status"
            || task.instance_id.to_string() != instance
            || task.assignee_id.to_string() != actor
            || !matches!(task.status.as_str(), "pending" | "completed" | "cancelled")
        {
            return Err(RepositoryError::Conflict);
        }
        if let Some(message) = self.project_status_message(id, lease, Some(task)).await? {
            chat.announce_persisted_message(MessageId::from_uuid(message))
                .await
                .map_err(storage)?;
        }
        if let (Some(request), Some(result)) = (request, result) {
            let expected: Value = serde_json::from_str(&result).map_err(storage)?;
            if task.status == "completed" {
                if task.result_metadata.as_ref() != Some(&expected) {
                    return Err(RepositoryError::Conflict);
                }
            } else if task.status == "pending" {
                let _ = self
                    .http
                    .post(format!("{base}/api/v2/user-tasks/{}/complete", task.id))
                    .header("X-Heart-Client", "sproyt-work-items")
                    .header("Idempotency-Key", request)
                    .json(&json!({"actor_id":actor,"result_metadata":expected}))
                    .send()
                    .await;
                // A later reconciliation must observe the exact committed result.
                return Ok(());
            } else if task.status != "cancelled" {
                return Err(RepositoryError::Conflict);
            }
        } else if task.status == "completed" {
            return Err(RepositoryError::Conflict);
        }
        let state = match task.status.as_str() {
            "completed" if instance_status == "completed" => "completed",
            "cancelled" => "cancelled",
            _ => "waiting",
        };
        macro_rules! settle { ($pool:expr,$pg:expr) => {{
            sqlx::query(&sql("update work_item_status_changes set status=? where id=?uuid and lease_token=?uuid",$pg)).bind(state).bind(id).bind(lease).execute($pool).await.map_err(storage)?;
        }}; }
        match &self.store {
            Store::Pg(p) => settle!(p, true),
            Store::Sqlite(p) => settle!(p, false),
        };
        // Never rewrite case status here: an old Heart receipt cannot regress it.
        Ok(())
    }

    async fn project_status_message(
        &self,
        id: &str,
        lease: &str,
        task: Option<&HeartTask>,
    ) -> Result<Option<Uuid>> {
        macro_rules! project { ($pool:expr,$pg:expr,$notify:path) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let row=sqlx::query(&sql("select cast(x.work_item_id as text) as item,cast(x.channel_id as text) as channel,cast(x.task_id as text) as task,cast(x.message_id as text) as message,cast(w.source_channel_id as text) as source_channel,x.no_change from work_item_status_changes x join work_items w on w.id=x.work_item_id where x.id=?uuid and x.lease_token=?uuid and x.lease_until>?",$pg)).bind(id).bind(lease).bind(Utc::now().timestamp()).fetch_one(&mut *tx).await.map_err(storage)?;
            let item:String=row.try_get("item").map_err(storage)?;
            let (channel,body)=if let Some(task)=task {
                if let Some(old)=row.try_get::<Option<String>,_>("task").map_err(storage)? {
                    if old!=task.id.to_string() {return Err(RepositoryError::Conflict);}
                    return Ok(None);
                }
                if task.status!="pending" {return Err(RepositoryError::Conflict);}
                (row.try_get::<String,_>("channel").map_err(storage)?,format!("[[work-item-task:{}]]",task.id))
            }else{
                if row.try_get::<Option<bool>,_>("no_change").map_err(storage)?==Some(true) {return Ok(None);}
                if sqlx::query_scalar::<_,i32>(&sql("select 1 from work_item_status_publications where work_item_id=?uuid",$pg)).bind(&item).fetch_optional(&mut *tx).await.map_err(storage)?.is_some(){return Ok(None);}
                (row.try_get::<String,_>("source_channel").map_err(storage)?,format!("[[work-item-status:{item}]]"))
            };
            let sequence:i64=sqlx::query_scalar(&sql("update channel_sequences set next_sequence=next_sequence+1 where channel_id=?uuid returning next_sequence-1",$pg)).bind(&channel).fetch_one(&mut *tx).await.map_err(storage)?;
            let bot=Uuid::new_v5(&Uuid::NAMESPACE_OID,format!("sproyt-work-item:{item}").as_bytes()).to_string();
            sqlx::query(&sql("insert into users(id,kind,display_name) values(?uuid,'agent','Heart') on conflict(id) do nothing",$pg)).bind(&bot).execute(&mut *tx).await.map_err(storage)?;
            sqlx::query(&sql("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) select ?uuid,requested_by,requested_by,'heart-work-items',?,'Present case tasks',60,current_timestamp from work_items where id=?uuid on conflict(agent_id) do nothing",$pg)).bind(&bot).bind(format!("work-item-review:{item}")).bind(&item).execute(&mut *tx).await.map_err(storage)?;
            let message=Uuid::now_v7();
            sqlx::query(&sql("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Heart',?,?)",$pg)).bind(message.to_string()).bind(&channel).bind(&bot).bind(sequence).bind(&body).execute(&mut *tx).await.map_err(storage)?;
            if let Some(task)=task {
                let changed=sqlx::query(&sql("update work_item_status_changes set task_id=?uuid,message_id=?uuid,status='waiting' where id=?uuid and lease_token=?uuid and task_id is null",$pg)).bind(task.id.to_string()).bind(message.to_string()).bind(id).bind(lease).execute(&mut *tx).await.map_err(storage)?.rows_affected();
                if changed!=1{return Err(RepositoryError::Conflict);}
                let projected=ChatMessage{id:MessageId::from_uuid(message),channel_id:ChannelId::new(&channel).map_err(storage)?,parent_message_id:None,sender_id:UserId::from_uuid(Uuid::parse_str(&bot).map_err(storage)?),sender_display_name:DisplayName::new("Heart").map_err(storage)?,sequence:ChannelSequence::try_from(sequence).map_err(storage)?,body:MessageBody::new(body).map_err(storage)?,sent_at:Utc::now(),edited_at:None,deleted_at:None};
                $notify(&mut tx,&projected).await.map_err(storage)?;
            }else{
                if sqlx::query(&sql("insert into work_item_status_publications(work_item_id,message_id,channel_id) values(?uuid,?uuid,?uuid) on conflict do nothing",$pg)).bind(&item).bind(message.to_string()).bind(&channel).execute(&mut *tx).await.map_err(storage)?.rows_affected()!=1{return Err(RepositoryError::Conflict);}
            }
            tx.commit().await.map_err(storage)?;
            Ok(Some(message))
        }}; }
        match &self.store {
            Store::Pg(p) => project!(p, true, crate::notification::enqueue_message_postgres),
            Store::Sqlite(p) => project!(p, false, crate::notification::enqueue_message_sqlite),
        }
    }

    async fn status_route(
        &self,
        actor: &str,
        item: &str,
    ) -> Result<Option<(String, String, String)>> {
        macro_rules! read {
            ($pool:expr,$pg:expr) => {{
                sqlx::query(&sql(ROUTE, $pg))
                    .bind(actor)
                    .bind(item)
                    .fetch_optional($pool)
                    .await
                    .map_err(storage)?
                    .map(|r| {
                        Ok::<_, RepositoryError>((
                            r.try_get("route").map_err(storage)?,
                            r.try_get("channel").map_err(storage)?,
                            r.try_get("name").map_err(storage)?,
                        ))
                    })
                    .transpose()
            }};
        }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }

    pub(super) async fn lifecycle(&self, actor: UserId, item: Uuid) -> Result<Option<Lifecycle>> {
        let actor = actor.to_string();
        let item = item.to_string();
        let handler = self.status_route(&actor, &item).await?.is_some();
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let row=sqlx::query(&sql("select status,cast(requested_by as text) as requester,exists(select 1 from work_item_status_changes x where x.work_item_id=w.id and x.status in ('pending','waiting')) as active from work_items w where id=?uuid",$pg)).bind(&item).fetch_one($pool).await.map_err(storage)?;
            let case_status:String=row.try_get("status").map_err(storage)?;
            let requester:String=row.try_get("requester").map_err(storage)?;
            if !handler && actor!=requester {return Ok(None);}
            let rows=sqlx::query(&sql("select h.from_status,h.to_status,u.display_name,h.created_at,h.internal_note,h.public_feedback from work_item_status_history h join users u on u.id=h.actor_id where h.work_item_id=?uuid order by h.revision desc limit 100",$pg)).bind(&item).fetch_all($pool).await.map_err(storage)?;
            let history=rows.into_iter().map(|r|Ok::<_,RepositoryError>(History{from_status:r.try_get("from_status").map_err(storage)?,to_status:r.try_get("to_status").map_err(storage)?,actor_name:r.try_get("display_name").map_err(storage)?,created_at:r.try_get("created_at").map_err(storage)?,internal_note:if handler {Some(r.try_get("internal_note").map_err(storage)?)}else{None},public_feedback:if handler || actor==requester {Some(r.try_get("public_feedback").map_err(storage)?)}else{None}})).collect::<Result<Vec<_>>>()?;
            Ok(Some(Lifecycle{allowed_statuses:transitions(&case_status),case_status,can_start:handler && !row.try_get::<bool,_>("active").map_err(storage)?,internal_note:history.first().and_then(|h|h.internal_note.clone()),public_feedback:history.first().and_then(|h|h.public_feedback.clone()),history}))
        }}; }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }

    pub async fn start_status(
        &self,
        actor: UserId,
        item: Uuid,
        command: StatusStart,
    ) -> Result<StatusStartReceipt> {
        if self.heart_url.is_none() {
            return Err(storage("Heart unavailable"));
        }
        let source = self
            .task(
                actor.clone(),
                command.source_task_id,
                command.source_message_id,
            )
            .await?;
        if source.work_item_id != item || source.status != "completed" {
            return Err(RepositoryError::Conflict);
        }
        let actor = actor.to_string();
        let item = item.to_string();
        let request = command.request_id.to_string();
        macro_rules! accept { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let route=sqlx::query(&sql(ROUTE,$pg)).bind(&actor).bind(&item).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
            let channel:String=route.try_get("channel").map_err(storage)?; let route_id:String=route.try_get("route").map_err(storage)?; let channel_name:String=route.try_get("name").map_err(storage)?;
            let old=sqlx::query(&sql("select cast(id as text) as id,cast(work_item_id as text) as item,cast(source_task_id as text) as source,cast(source_message_id as text) as message,start_revision,status,cast(route_id as text) as route,cast(channel_id as text) as channel from work_item_status_changes where actor_id=?uuid and request_id=?uuid",$pg)).bind(&actor).bind(&request).fetch_optional(&mut *tx).await.map_err(storage)?;
            if let Some(old)=old {
                if old.try_get::<String,_>("item").map_err(storage)?!=item || old.try_get::<String,_>("source").map_err(storage)?!=command.source_task_id.to_string() || old.try_get::<String,_>("message").map_err(storage)?!=command.source_message_id.to_string() || old.try_get::<i64,_>("start_revision").map_err(storage)?!=command.expected_revision || old.try_get::<String,_>("route").map_err(storage)?!=route_id || old.try_get::<String,_>("channel").map_err(storage)?!=channel {return Err(RepositoryError::Conflict);}
                return Ok(StatusStartReceipt{id:Uuid::parse_str(&old.try_get::<String,_>("id").map_err(storage)?).map_err(storage)?,work_item_id:Uuid::parse_str(&item).map_err(storage)?,channel_name,start_status:old.try_get("status").map_err(storage)?});
            }
            let revision:i64=sqlx::query_scalar(&sql("select revision from work_items where id=?uuid",$pg)).bind(&item).fetch_one(&mut *tx).await.map_err(storage)?;
            if revision!=command.expected_revision {return Err(RepositoryError::Conflict);}
            let id=Uuid::now_v7();
            let changed=sqlx::query(&sql("insert into work_item_status_changes(id,work_item_id,actor_id,channel_id,route_id,source_task_id,source_message_id,request_id,start_revision,created_at) values(?uuid,?uuid,?uuid,?uuid,?uuid,?uuid,?uuid,?uuid,?,?) on conflict do nothing",$pg)).bind(id.to_string()).bind(&item).bind(&actor).bind(&channel).bind(&route_id).bind(command.source_task_id.to_string()).bind(command.source_message_id.to_string()).bind(&request).bind(command.expected_revision).bind(Utc::now().timestamp()).execute(&mut *tx).await.map_err(storage)?.rows_affected();
            if changed!=1 {return Err(RepositoryError::Conflict);}
            tx.commit().await.map_err(storage)?;
            Ok(StatusStartReceipt{id,work_item_id:Uuid::parse_str(&item).map_err(storage)?,channel_name,start_status:"pending".into()})
        }}; }
        match &self.store {
            Store::Pg(p) => accept!(p, true),
            Store::Sqlite(p) => accept!(p, false),
        }
    }

    pub async fn change_status(
        &self,
        actor: UserId,
        task: Uuid,
        command: StatusDecision,
    ) -> Result<TaskView> {
        if command.internal_note.chars().count() > 2000
            || command.public_feedback.chars().count() > 2000
            || (command.no_change
                && (!command.status.is_empty()
                    || !command.internal_note.is_empty()
                    || !command.public_feedback.is_empty()))
        {
            return Err(RepositoryError::Conflict);
        }
        let actor_string = actor.to_string();
        let task_string = task.to_string();
        macro_rules! accept { ($pool:expr,$pg:expr) => {{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let row=sqlx::query(&sql("select cast(x.id as text) as id,cast(w.id as text) as item,w.status as case_status,w.revision,x.status,cast(x.route_id as text) as route,cast(x.channel_id as text) as channel,cast(x.decision_request_id as text) as request,x.decision_revision,x.decision_status,x.internal_note,x.public_feedback,x.no_change from work_item_status_changes x join work_items w on w.id=x.work_item_id where x.task_id=?uuid and x.message_id=?uuid and x.actor_id=?uuid",$pg)).bind(&task_string).bind(command.message_id.to_string()).bind(&actor_string).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
            let id:String=row.try_get("id").map_err(storage)?; let item:String=row.try_get("item").map_err(storage)?;
            let route=sqlx::query(&sql(ROUTE,$pg)).bind(&actor_string).bind(&item).fetch_optional(&mut *tx).await.map_err(storage)?.ok_or(RepositoryError::PermissionDenied)?;
            if route.try_get::<String,_>("route").map_err(storage)?!=row.try_get::<String,_>("route").map_err(storage)? || route.try_get::<String,_>("channel").map_err(storage)?!=row.try_get::<String,_>("channel").map_err(storage)? {return Err(RepositoryError::PermissionDenied);}
            if let Some(old)=row.try_get::<Option<String>,_>("request").map_err(storage)? {
                if old!=command.request_id.to_string() || row.try_get::<Option<i64>,_>("decision_revision").map_err(storage)?!=Some(command.expected_revision) || row.try_get::<Option<String>,_>("decision_status").map_err(storage)?.unwrap_or_default()!=command.status || row.try_get::<Option<String>,_>("internal_note").map_err(storage)?.unwrap_or_default()!=command.internal_note || row.try_get::<Option<String>,_>("public_feedback").map_err(storage)?.unwrap_or_default()!=command.public_feedback || row.try_get::<Option<bool>,_>("no_change").map_err(storage)?!=Some(command.no_change) {return Err(RepositoryError::Conflict);}
                tx.commit().await.map_err(storage)?;
                return self.task(actor.clone(),task,command.message_id).await;
            }
            let from:String=row.try_get("case_status").map_err(storage)?;
            if row.try_get::<String,_>("status").map_err(storage)?!="waiting" || row.try_get::<i64,_>("revision").map_err(storage)?!=command.expected_revision || (!command.no_change && !transitions(&from).contains(&command.status)) {return Err(RepositoryError::Conflict);}
            let revision=command.expected_revision+if command.no_change {0}else{1};
            let result=json!({"change_id":id,"from_status":from,"status":if command.no_change {&from}else{&command.status},"case_revision":revision,"no_change":command.no_change}).to_string();
            let updated=sqlx::query(&sql("update work_item_status_changes set decision_request_id=?uuid,decision_revision=?,decision_status=?,internal_note=?,public_feedback=?,no_change=?,decision_result=? where id=?uuid and decision_request_id is null and status='waiting'",$pg)).bind(command.request_id.to_string()).bind(command.expected_revision).bind(&command.status).bind(&command.internal_note).bind(&command.public_feedback).bind(command.no_change).bind(&result).bind(&id).execute(&mut *tx).await.map_err(storage)?.rows_affected();
            if updated!=1 {return Err(RepositoryError::Conflict);}
            if !command.no_change {
                let updated=sqlx::query(&sql("update work_items set status=?,revision=revision+1,updated_at=current_timestamp where id=?uuid and revision=?",$pg)).bind(&command.status).bind(&item).bind(command.expected_revision).execute(&mut *tx).await.map_err(storage)?.rows_affected();
                if updated!=1 {return Err(RepositoryError::Conflict);}
                sqlx::query(&sql("insert into work_item_status_history(id,change_id,work_item_id,revision,actor_id,from_status,to_status,internal_note,public_feedback,created_at) values(?uuid,?uuid,?uuid,?,?uuid,?,?,?,?,?)",$pg)).bind(Uuid::now_v7().to_string()).bind(&id).bind(&item).bind(revision).bind(&actor_string).bind(&from).bind(&command.status).bind(&command.internal_note).bind(&command.public_feedback).bind(Utc::now().timestamp()).execute(&mut *tx).await.map_err(storage)?;
            }
            tx.commit().await.map_err(storage)?;
            self.task(actor.clone(),task,command.message_id).await
        }}; }
        match &self.store {
            Store::Pg(p) => accept!(p, true),
            Store::Sqlite(p) => accept!(p, false),
        }
    }

    pub(super) async fn status_task(
        &self,
        actor: UserId,
        task: Uuid,
        message: Uuid,
    ) -> Result<Option<TaskView>> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let row=sqlx::query(&sql("select cast(x.work_item_id as text) as item,cast(x.actor_id as text) as actor,cast(x.route_id as text) as route,cast(x.channel_id as text) as channel,x.status,x.decision_status,x.internal_note,x.public_feedback,x.decision_result,w.title,w.description,w.revision,w.category,w.priority,a.name as application_name,u.display_name from work_item_status_changes x join work_items w on w.id=x.work_item_id join work_applications a on a.id=w.application_id join users u on u.id=x.actor_id join channel_memberships cm on cm.channel_id=x.channel_id and cm.user_id=?uuid where x.task_id=?uuid and x.message_id=?uuid",$pg)).bind(actor.to_string()).bind(task.to_string()).bind(message.to_string()).fetch_optional($pool).await.map_err(storage)?;
            let Some(row)=row else {return Ok(None);};
            let item=Uuid::parse_str(&row.try_get::<String,_>("item").map_err(storage)?).map_err(storage)?;
            let assigned:String=row.try_get("actor").map_err(storage)?; let state:String=row.try_get("status").map_err(storage)?;
            let route=self.status_route(&assigned,&item.to_string()).await?;
            let allowed=route.as_ref().is_some_and(|(r,c,_)|row.try_get::<String,_>("route").ok().as_ref()==Some(r) && row.try_get::<String,_>("channel").ok().as_ref()==Some(c));
            let accepted=row.try_get::<Option<String>,_>("decision_result").map_err(storage)?.is_some();
            let mut lifecycle=self.lifecycle(actor.clone(),item).await?;
            let private=self.status_route(&actor.to_string(),&item.to_string()).await?.is_some();
            if let Some(lifecycle)=lifecycle.as_mut() { lifecycle.internal_note=if private {row.try_get("internal_note").map_err(storage)?}else{None}; if private {lifecycle.public_feedback=row.try_get("public_feedback").map_err(storage)?;} }

            Ok(Some(TaskView{id:task,message_id:message,work_item_id:item,revision:row.try_get("revision").map_err(storage)?,application_name:row.try_get("application_name").map_err(storage)?,title:row.try_get("title").map_err(storage)?,description:row.try_get("description").map_err(storage)?,status:if state=="waiting" {"pending".into()}else if state=="failed" {"cancelled".into()}else{state.clone()},process_status:state.clone(),delivery_status:if accepted && matches!(state.as_str(),"failed"|"cancelled") {"failed".into()}else if accepted && state=="waiting" {"pending".into()}else{"ready".into()},category:row.try_get("category").map_err(storage)?,priority:row.try_get("priority").map_err(storage)?,decision_status:row.try_get("decision_status").map_err(storage)?,assignee_name:row.try_get("display_name").map_err(storage)?,can_decide:actor.to_string()==assigned && allowed && state=="waiting" && !accepted,blocked:state=="waiting" && !allowed,node_id:"change-status".into(),can_request_information:false,information_request:None,information_response:None,supplements:Vec::new(),github_export:None,lifecycle}))
        }}; }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }

    pub async fn public_status(
        &self,
        actor: UserId,
        item: Uuid,
        message: Uuid,
    ) -> Result<PublicStatus> {
        macro_rules! read { ($pool:expr,$pg:expr) => {{
            let row=sqlx::query(&sql("select w.title,w.status,a.name,cast(w.requested_by as text) as requester from work_item_status_publications p join work_items w on w.id=p.work_item_id join work_applications a on a.id=w.application_id join channel_memberships cm on cm.channel_id=p.channel_id and cm.user_id=?uuid where p.work_item_id=?uuid and p.message_id=?uuid and p.channel_id=w.source_channel_id",$pg)).bind(actor.to_string()).bind(item.to_string()).bind(message.to_string()).fetch_optional($pool).await.map_err(storage)?.ok_or(RepositoryError::NotFound)?;
            let visible=row.try_get::<String,_>("requester").map_err(storage)?==actor.to_string() || self.status_route(&actor.to_string(),&item.to_string()).await?.is_some();
            if !visible {return Ok(PublicStatus{visible:false,title:None,application_name:None,status:None,public_feedback:None,history:vec![]});}
            let history=sqlx::query(&sql("select from_status,to_status,created_at,public_feedback from work_item_status_history where work_item_id=?uuid order by revision desc limit 100",$pg)).bind(item.to_string()).fetch_all($pool).await.map_err(storage)?.into_iter().map(|r|Ok::<_,RepositoryError>(PublicHistory{from_status:r.try_get("from_status").map_err(storage)?,to_status:r.try_get("to_status").map_err(storage)?,created_at:r.try_get("created_at").map_err(storage)?,public_feedback:r.try_get("public_feedback").map_err(storage)?})).collect::<Result<Vec<_>>>()?;
            Ok(PublicStatus{visible:true,title:Some(row.try_get("title").map_err(storage)?),application_name:Some(row.try_get("name").map_err(storage)?),status:Some(row.try_get("status").map_err(storage)?),public_feedback:history.first().map(|h|h.public_feedback.clone()),history})
        }}; }
        match &self.store {
            Store::Pg(p) => read!(p, true),
            Store::Sqlite(p) => read!(p, false),
        }
    }
}
