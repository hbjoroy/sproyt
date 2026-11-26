//! Heart assigns every activity, including the ones performed by Sprøyt. The local
//! projection and outbox let a worker retry after a crash or a lost HTTP reply.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

const FLOW: &str = include_str!("../../helm/sproyt/definitions/feedback-intake.yaml");
const CLIENT: &str = "sproyt-feedback-intake";
const OPEN: &str = "stage not in ('ignored','linked','resolved','rejected','failed')";

macro_rules! db {
    ($self:expr, $query:expr, [$($arg:expr),*], $ctor:expr, $operation:ident) => {{
        let q = sql($query, matches!($self.store, Store::Pg(_)));
        match &$self.store {
            Store::Pg(pool) => $ctor(&q)$(.bind($arg))*.$operation(pool).await.map_err(storage),
            Store::Sqlite(pool) => $ctor(&q)$(.bind($arg))*.$operation(pool).await.map_err(storage),
        }
    }};
}
macro_rules! update {
    ($self:expr, $query:expr, [$($arg:expr),*]) => {{
        let q = sql($query, matches!($self.store, Store::Pg(_)));
        match &$self.store {
            Store::Pg(pool) => sqlx::query(&q)$(.bind($arg))*.execute(pool).await.map(|r|r.rows_affected()),
            Store::Sqlite(pool) => sqlx::query(&q)$(.bind($arg))*.execute(pool).await.map(|r|r.rows_affected()),
        }.map_err(storage)
    }};
}

#[derive(sqlx::FromRow)]
struct Binding {
    agent: String,
    reviewer: String,
    channel: String,
    instance: Option<String>,
    definition: Option<String>,
    stage: String,
    task: Option<String>,
}

impl Binding {
    fn input(&self, source: &str) -> Value {
        json!({"source_message_id":source, "channel_id":self.channel,
            "reviewer_id":self.reviewer, "agent_id":self.agent})
    }

    fn verify(&self, view: &Value, source: &str, definition: &str) -> Result<()> {
        if view["runtime"] != "v2"
            || view["namespace"] != "sproyt"
            || view["definition_id"] != definition
            || view["input_metadata"] != self.input(source)
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    fn owner(&self, node: &str) -> Result<&str> {
        match node {
            "clarify" | "clarify-publication" | "final-clarify" | "review" => Ok(&self.reviewer),
            "qualify" | "match" | "create" | "comment" | "link" | "retry-create"
            | "retry-comment" | "retry-link" | "ack" | "await-github" => Ok(&self.agent),
            _ => Err(RepositoryError::Conflict),
        }
    }
}

fn validate_result(node: &str, value: &Value) -> Result<()> {
    let outcome = value["outcome"].as_str().unwrap_or_default();
    let valid = match node {
        "qualify" => matches!(outcome, "qualified" | "ignored" | "clarify"),
        "match" => matches!(outcome, "create" | "comment" | "link" | "clarify"),
        "clarify" | "clarify-publication" => {
            matches!(outcome, "create" | "comment" | "link" | "ignore")
        }
        "final-clarify" => outcome == "ignore",
        "create" | "comment" | "link" | "retry-create" | "retry-comment" | "retry-link" => {
            matches!(outcome, "published" | "clarify")
        }
        "ack" => matches!(outcome, "created" | "linked"),
        "review" => {
            value["reviewed"] == true
                && value["assessment_id"]
                    .as_str()
                    .is_some_and(|v| Uuid::parse_str(v).is_ok())
        }
        "await-github" => matches!(outcome, "resolved" | "rejected"),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(RepositoryError::Conflict)
    }
}

// Heart returns no active activation once an instance ends. Only derive the
// terminal label from a confirmed terminal-producing task, with matching receipt.
fn terminal(tasks: &[HeartTask]) -> Result<&'static str> {
    let mut found = None;
    for t in tasks {
        let value = t
            .result_metadata
            .as_ref()
            .ok_or(RepositoryError::Conflict)?;
        let label = match (t.node_id.as_str(), value["outcome"].as_str()) {
            ("qualify", Some("ignored"))
            | ("clarify" | "clarify-publication" | "final-clarify", Some("ignore")) => "ignored",
            ("ack", Some("linked")) => "linked",
            ("await-github", Some("resolved")) => "resolved",
            ("await-github", Some("rejected")) => "rejected",
            _ => continue,
        };
        if found.replace(label).is_some() {
            return Err(RepositoryError::Conflict);
        }
    }
    found.ok_or(RepositoryError::Conflict)
}

fn active(view: &Value, node: &str, task: &str) -> bool {
    view["status"] == "waiting"
        && view["active_activations"].as_array().is_some_and(|list| {
            list.len() == 1
                && list[0]["instance_id"] == view["id"]
                && list[0]["status"] == "waiting"
                && list[0]["id"] == task
                && list[0]["node_id"] == node
        })
}

impl WorkItems {
    async fn intake_binding(&self, source: &str) -> Result<Binding> {
        db!(
            self,
            "select cast(j.agent_id as text) as agent,cast(j.reviewer_id as text) as reviewer,
             cast(j.channel_id as text) as channel,cast(i.instance_id as text) as instance,
             cast(i.definition_id as text) as definition,i.stage,cast(i.task_id as text) as task
             from feedback_intakes i join feedback_jobs j on i.source_message_id=j.source_message_id
             where j.source_message_id=?uuid and j.intake_version=1",
            [source],
            sqlx::query_as::<_, Binding>,
            fetch_one
        )
    }

    pub(super) async fn sync_feedback_intakes(&self, now: i64) -> Result<()> {
        self.admit_feedback().await?;
        update!(self,"insert into feedback_intakes(source_message_id)
            select j.source_message_id from feedback_jobs j where j.intake_version=1
            and not exists(select 1 from feedback_intakes i where i.source_message_id=j.source_message_id)
            order by j.source_sequence,j.source_message_id limit 32 on conflict(source_message_id) do nothing",[])?;
        let query = format!(
            "select cast(source_message_id as text) from feedback_intakes
            where due_at<=? and {OPEN} order by due_at,source_message_id limit 12"
        );
        let sources = db!(
            self,
            &query,
            [now],
            sqlx::query_scalar::<_, String>,
            fetch_all
        )?;
        for source in sources {
            let lease = Uuid::now_v7().to_string();
            let claim=format!("update feedback_intakes set due_at=?,lease_token=?uuid where source_message_id=?uuid
                              and due_at<=? and {OPEN}");
            if update!(self, &claim, [now + 120, &lease, &source, now])? == 0 {
                continue;
            }
            let delay = match self.drive_intake(&source, &lease).await {
                Ok(delay) => delay,
                Err(error) => {
                    tracing::warn!(source=%source,error_kind=error.kind(),"feedback intake Heart reconciliation deferred");
                    15
                }
            };
            update!(
                self,
                "update feedback_intakes set due_at=?,lease_token=null where source_message_id=?uuid and lease_token=?uuid",
                [Utc::now().timestamp() + delay, &source, &lease]
            )?;
        }
        Ok(())
    }

    async fn intake_instance(&self, source: &str, binding: &Binding) -> Result<Value> {
        let base = self
            .heart_url
            .as_deref()
            .ok_or_else(|| storage("Heart unavailable"))?;
        let instance = binding
            .instance
            .as_deref()
            .ok_or(RepositoryError::Conflict)?;
        let definition = binding
            .definition
            .as_deref()
            .ok_or(RepositoryError::Conflict)?;
        let view = self
            .heart_response(self.http.get(format!("{base}/api/v2/instances/{instance}")))
            .await?;
        binding.verify(&view, source, definition)?;
        if view["id"] != instance {
            return Err(RepositoryError::Conflict);
        }
        Ok(view)
    }

    pub(super) async fn assert_intake_activation(
        &self,
        source: &str,
        node: &str,
        task: &str,
    ) -> Result<()> {
        let binding = self.intake_binding(source).await?;
        if binding.stage != node || binding.task.as_deref() != Some(task) {
            return Err(RepositoryError::Conflict);
        }
        let view = self.intake_instance(source, &binding).await?;
        if !active(&view, node, task) {
            return Err(RepositoryError::Conflict);
        }
        let base = self.heart_url.as_deref().ok_or(RepositoryError::Conflict)?;
        let instance = binding
            .instance
            .as_deref()
            .ok_or(RepositoryError::Conflict)?;
        let tasks: Vec<HeartTask> = serde_json::from_value(
            self.heart_response(
                self.http
                    .get(format!("{base}/api/v2/user-tasks"))
                    .query(&[("instance_id", instance)]),
            )
            .await?,
        )
        .map_err(storage)?;
        let owner = binding.owner(node)?;
        if !tasks.iter().any(|t| {
            t.status == "pending"
                && t.id.to_string() == task
                && t.node_id == node
                && t.instance_id.to_string() == instance
                && t.assignee_id.to_string() == owner
                && t.result_metadata.is_none()
        }) {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    async fn intake_projection(
        &self,
        source: &str,
        lease: &str,
        stage: &str,
        task: Option<String>,
    ) -> Result<()> {
        if update!(
            self,
            "update feedback_intakes set stage=?,task_id=?uuid where source_message_id=?uuid and lease_token=?uuid",
            [stage, task, source, lease]
        )? != 1
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    async fn drive_intake(&self, source: &str, lease: &str) -> Result<i64> {
        let binding = self.intake_binding(source).await?;
        let base = self
            .heart_url
            .as_deref()
            .ok_or_else(|| storage("Heart unavailable"))?;
        let Some(instance) = binding.instance.as_deref() else {
            let definition = self
                .heart_response(
                    self.http
                        .post(format!("{base}/api/v2/definitions"))
                        .header("Content-Type", "application/yaml")
                        .body(FLOW),
                )
                .await?;
            if definition["runtime"] != "v2"
                || definition["namespace"] != "sproyt"
                || definition["name"] != "feedback-intake"
                || definition["version"] != "1.0.0"
            {
                return Err(RepositoryError::Conflict);
            }
            let def = definition["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(RepositoryError::Conflict)?
                .to_string();
            let start=self.heart_response(self.http.post(format!("{base}/api/v2/instances"))
                .header("X-Heart-Client",CLIENT).header("Idempotency-Key",source)
                .json(&json!({"definition_id":def,"actor_id":binding.agent,"input_metadata":binding.input(source)}))).await?;
            let view = &start["instance"];
            binding.verify(view, source, &def)?;
            let id = view["id"]
                .as_str()
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or(RepositoryError::Conflict)?
                .to_string();
            if update!(
                self,
                "update feedback_intakes set instance_id=?uuid,definition_id=?uuid
                where source_message_id=?uuid and instance_id is null and lease_token=?uuid",
                [id, def, source, lease]
            )? != 1
            {
                return Err(RepositoryError::Conflict);
            }
            return Ok(0);
        };
        let view = self.intake_instance(source, &binding).await?;
        if matches!(view["status"].as_str(), Some("failed" | "cancelled")) {
            self.intake_projection(source, lease, "failed", None)
                .await?;
            return Ok(0);
        }
        if !matches!(
            view["status"].as_str(),
            Some("running" | "waiting" | "completed")
        ) {
            return Err(RepositoryError::Conflict);
        }
        let tasks: Vec<HeartTask> = serde_json::from_value(
            self.heart_response(
                self.http
                    .get(format!("{base}/api/v2/user-tasks"))
                    .query(&[("instance_id", instance)]),
            )
            .await?,
        )
        .map_err(storage)?;
        let rows = db!(
            self,
            "select node_id,cast(task_id as text),result_json from feedback_intake_results where source_message_id=?uuid",
            [source],
            sqlx::query_as::<_, (String, String, String)>,
            fetch_all
        )?;
        let mut receipts = BTreeMap::new();
        for (node, id, result) in rows {
            let value: Value = serde_json::from_str(&result).map_err(storage)?;
            validate_result(&node, &value)?;
            receipts.insert((node, id), value);
        }
        let mut nodes = BTreeSet::new();
        let mut pending = None;
        for task in &tasks {
            if task.instance_id.to_string() != instance
                || binding.owner(&task.node_id)? != task.assignee_id.to_string()
                || !nodes.insert(&task.node_id)
            {
                return Err(RepositoryError::Conflict);
            }
            let receipt = receipts.remove(&(task.node_id.clone(), task.id.to_string()));
            match task.status.as_str() {
                "completed" if receipt.is_some() && task.result_metadata == receipt => {}
                "pending" if task.result_metadata.is_none() && pending.is_none() => {
                    pending = Some((task, receipt))
                }
                _ => return Err(RepositoryError::Conflict),
            }
        }
        if !receipts.is_empty() {
            return Err(RepositoryError::Conflict);
        }
        if view["status"] == "completed" {
            if pending.is_some() {
                return Err(RepositoryError::Conflict);
            }
            self.intake_projection(source, lease, terminal(&tasks)?, None)
                .await?;
            return Ok(0);
        }
        let Some((task, receipt)) = pending else {
            // The engine can be running a gate between two activities.
            return Ok(1);
        };
        let node = &task.node_id;
        let id = task.id.to_string();
        if !active(&view, node, &id) {
            return Err(RepositoryError::Conflict);
        }
        if binding.task.as_deref() != Some(&id) || binding.stage != *node {
            self.intake_projection(source, lease, node, Some(id.clone()))
                .await?;
        }
        if let Some(result) = receipt {
            crate::operations::dependencies::heart_completion(
                self.http
                    .post(format!("{base}/api/v2/user-tasks/{id}/complete"))
                    .header("X-Heart-Client", CLIENT)
                    .header("Idempotency-Key", &id)
                    .json(&json!({"actor_id":task.assignee_id,"result_metadata":result})),
            )
            .await
            .map_err(storage)?;
            return Ok(0);
        }
        if node == "await-github" {
            // Freeze the closure and its Heart command together. An intervening
            // reopening before this insert makes it ineligible; later retries
            // always replay the originally accepted closure.
            let inserted = update!(
                self,
                r#"insert into feedback_intake_results(source_message_id,node_id,task_id,result_json)
                select i.source_message_id,i.stage,i.task_id,case j.github_state_reason
                    when 'completed' then '{"outcome":"resolved"}' else '{"outcome":"rejected"}' end
                from feedback_intakes i join feedback_jobs j on i.source_message_id=j.source_message_id
                where i.source_message_id=?uuid and i.stage='await-github' and i.task_id=?uuid and i.lease_token=?uuid
                and j.github_state='closed' and j.github_checked_at>i.reviewed_at
                and j.github_state_reason in ('completed','not_planned') on conflict do nothing"#,
                [source, &id, lease]
            )?;
            if inserted == 1 {
                return Ok(0);
            }
        }
        Ok(
            if matches!(
                node.as_str(),
                "clarify" | "clarify-publication" | "final-clarify" | "review" | "await-github"
            ) {
                20
            } else {
                3
            },
        )
    }
}
