//! Access to the current Heart intake task through its source message or channel receipt.
use super::feedback_channel::{PINNED_JOB, SOURCE_AUTHORITY};
use super::feedback_heart::{SOURCE, query_store};
use super::*;

#[derive(Serialize, Deserialize)]
pub(crate) struct IntakeCandidate {
    number: i64,
    title: String,
}

#[derive(Serialize)]
pub(crate) struct FeedbackIntakeView {
    pub stage: String,
    pub source_body: String,
    pub repository_name: String,
    pub issue_url: Option<String>,
    pub heart_instance_id: Option<String>,
    pub clarification_task_id: Option<String>,
    pub review_task_id: Option<String>,
    pub reason: Option<String>,
    pub candidates: Vec<IntakeCandidate>,
    pub can_clarify: bool,
    pub can_review: bool,
    pub dismiss_only: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeedbackClarification {
    task_id: Uuid,
    decision: String,
    issue_number: Option<i64>,
    note: String,
}

pub(super) const CLARIFICATION: &[&str] = &["clarify", "clarify-publication", "final-clarify"];

pub(super) fn owner_rights() -> String {
    format!("(j.reviewer_id=cm.user_id and exists(select 1 from {SOURCE_AUTHORITY} join feedback_jobs granted on {}
        where granted.source_message_id=j.source_message_id))",
        PINNED_JOB.replace("and j.source_body=m.body", "").replace("j.", "granted."))
}

impl WorkItems {
    pub(crate) async fn feedback_intake(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
    ) -> Result<Option<FeedbackIntakeView>> {
        let query=format!("select coalesce(i.stage,'starting'),original.body,cast(i.instance_id as text),
            cast(i.task_id as text),i.reason,i.candidates,j.repository_name,
            case when j.status in ('published','done') then j.issue_url end,{}, receipt.task_id is not null,original.body<>j.source_body
            from feedback_jobs j {SOURCE}
            left join feedback_intakes i on i.source_message_id=j.source_message_id
            left join feedback_intake_results receipt on receipt.task_id=i.task_id
            where j.intake_version=1 and j.channel_id=?uuid and m.id=?uuid and cm.user_id=?uuid",owner_rights());
        type ViewRow = (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            String,
            Option<String>,
            bool,
            bool,
            bool,
        );
        let row = query_store!(
            self,
            query,
            pool,
            q,
            sqlx::query_as::<_, ViewRow>(&q)
                .bind(channel.to_string())
                .bind(message.to_string())
                .bind(actor.to_string())
                .fetch_optional(pool)
                .await
        )?;
        row.map(
            |(
                stage,
                source_body,
                heart_instance_id,
                task,
                reason,
                candidates,
                repository_name,
                issue_url,
                owner,
                submitted,
                edited,
            )| {
                let actionable = owner && !submitted && task.is_some();
                let can_clarify = actionable && CLARIFICATION.contains(&stage.as_str());
                let can_review = actionable && stage == "review";
                let dismiss_only = edited || stage == "final-clarify";
                let candidates = if can_clarify && !dismiss_only {
                    serde_json::from_str(candidates.as_deref().unwrap_or("[]")).map_err(storage)?
                } else {
                    vec![]
                };
                Ok(FeedbackIntakeView {
                    source_body,
                    repository_name,
                    issue_url,
                    heart_instance_id,
                    candidates,
                    reason,
                    clarification_task_id: if can_clarify { task.clone() } else { None },
                    review_task_id: if can_review { task } else { None },
                    stage,
                    can_clarify,
                    can_review,
                    dismiss_only,
                })
            },
        )
        .transpose()
    }

    // Workers commit the result against the exact activation that authorised the work.
    pub(super) async fn record_intake_result(
        &self,
        source: &str,
        node: &str,
        task: &str,
        result: &Value,
    ) -> Result<()> {
        self.intake_receipt(source, node, task, result, None, None)
            .await
    }

    async fn intake_receipt(
        &self,
        source: &str,
        node: &str,
        task: &str,
        result: &Value,
        note: Option<&str>,
        owner: Option<&UserId>,
    ) -> Result<()> {
        let access = if owner.is_some() {
            format!(
                " and exists(select 1 from feedback_jobs j
            join channel_memberships cm on cm.channel_id=j.channel_id and cm.user_id=?uuid
            where j.source_message_id=i.source_message_id and {})",
                owner_rights()
            )
        } else {
            String::new()
        };
        let unchanged = if owner.is_some()
            && CLARIFICATION.contains(&node)
            && result["outcome"] != "ignore"
        {
            " and exists(select 1 from feedback_jobs j join messages m on m.id=j.source_message_id where j.source_message_id=i.source_message_id and m.body=j.source_body)"
        } else {
            ""
        };
        let write=format!("insert into feedback_intake_results(source_message_id,node_id,task_id,result_json,owner_note)
            select i.source_message_id,i.stage,i.task_id,?,? from feedback_intakes i
            where i.source_message_id=?uuid and i.stage=? and i.task_id=?uuid {access}{unchanged}
            on conflict(task_id) do nothing");
        let read = format!(
            "select rr.result_json,rr.owner_note from feedback_intake_results rr
            join feedback_intakes i on i.source_message_id=rr.source_message_id
            where rr.source_message_id=?uuid and rr.node_id=? and rr.task_id=?uuid {access}"
        );
        let result_text = result.to_string();
        macro_rules! run {($pool:expr,$pg:expr)=>{{
            let mut tx=$pool.begin().await.map_err(storage)?;
            let write=sql(&write,$pg);
            let mut ins=sqlx::query(&write).bind(&result_text).bind(note).bind(source).bind(node).bind(task);
            if let Some(actor)=owner { ins=ins.bind(actor.to_string()); }
            ins.execute(&mut *tx).await.map_err(storage)?;
            let read=sql(&read,$pg);
            let mut found=sqlx::query_as::<_,(String,Option<String>)>(&read).bind(source).bind(node).bind(task);
            if let Some(actor)=owner {found=found.bind(actor.to_string());}
            let saved=found.fetch_optional(&mut *tx).await.map_err(storage)?;
            if saved.as_ref().is_none_or(|(value,owner_note)|value!=&result_text || owner_note.as_deref()!=note) {
                return Err(RepositoryError::Conflict);
            }
            sqlx::query(&sql("update feedback_intakes set
                reviewed_at=case when ?='review' then coalesce(reviewed_at,?) else reviewed_at end,
                due_at=case when lease_token is null then 0 else due_at end where source_message_id=?uuid and task_id=?uuid",$pg))
                .bind(node).bind(Utc::now().timestamp()).bind(source).bind(task).execute(&mut *tx).await.map_err(storage)?;
            tx.commit().await.map_err(storage)
        }}}
        match &self.store {
            Store::Pg(p) => run!(p, true),
            Store::Sqlite(p) => run!(p, false),
        }
    }

    // Authorise both the location the user opened and the immutable activation ID;
    // a replay can return its receipt even after Heart has moved to another node.
    async fn intake_owner_task(
        &self,
        actor: &UserId,
        channel: Uuid,
        message: Uuid,
        task: Uuid,
    ) -> Result<(String, String, String)> {
        let query=format!("select cast(j.source_message_id as text),coalesce(rr.node_id,i.stage),i.candidates
            from feedback_jobs j {SOURCE} join feedback_intakes i on i.source_message_id=j.source_message_id
            left join feedback_intake_results rr on rr.source_message_id=i.source_message_id and rr.task_id=?uuid
            where j.channel_id=?uuid and m.id=?uuid and cm.user_id=?uuid and
            (rr.task_id is not null or i.task_id=?uuid) and {}",owner_rights());
        query_store!(
            self,
            query,
            pool,
            q,
            sqlx::query_as(&q)
                .bind(task.to_string())
                .bind(channel.to_string())
                .bind(message.to_string())
                .bind(actor.to_string())
                .bind(task.to_string())
                .fetch_optional(pool)
                .await
        )?
        .ok_or(RepositoryError::PermissionDenied)
    }

    pub(crate) async fn clarify_feedback(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
        command: FeedbackClarification,
    ) -> Result<FeedbackIntakeView> {
        let (source, node, candidates) = self
            .intake_owner_task(&actor, channel, message, command.task_id)
            .await?;
        let note = command.note.trim();
        if !CLARIFICATION.contains(&node.as_str())
            || note.is_empty()
            || note.chars().count() > 2000
            || note.contains('\0')
        {
            return Err(RepositoryError::Conflict);
        }
        let result = match (command.decision.as_str(), command.issue_number) {
            ("dismiss", None) => json!({"outcome":"ignore"}),
            ("create", None) if node != "final-clarify" => json!({"outcome":"create"}),
            ("existing", Some(number)) if node != "final-clarify" && number > 0 => {
                let candidates: Vec<IntakeCandidate> =
                    serde_json::from_str(&candidates).map_err(storage)?;
                if !candidates.iter().any(|c| c.number == number) {
                    // A later clarification can have a different list. Only an exact saved command may replay.
                    let saved = query_store!(
                        self,
                        "select result_json from feedback_intake_results where task_id=?uuid",
                        pool,
                        q,
                        sqlx::query_scalar::<_, String>(&q)
                            .bind(command.task_id.to_string())
                            .fetch_optional(pool)
                            .await
                    )?;
                    let selected = json!({"outcome":"comment","issue_number":number}).to_string();
                    if let Some(saved) = saved {
                        if saved != selected {
                            return Err(RepositoryError::Conflict);
                        }
                    } else {
                        self.verify_intake_target(&source, number).await?;
                    }
                }
                json!({"outcome":"comment","issue_number":number})
            }
            _ => return Err(RepositoryError::Conflict),
        };
        self.intake_receipt(
            &source,
            &node,
            &command.task_id.to_string(),
            &result,
            Some(note),
            Some(&actor),
        )
        .await?;
        self.feedback_intake(actor, channel, message)
            .await?
            .ok_or(RepositoryError::NotFound)
    }

    pub(super) async fn intake_source_edited(&self, source: &str) -> Result<bool> {
        query_store!(
            self,
            "select j.source_body<>m.body from feedback_jobs j join messages m on m.id=j.source_message_id where j.source_message_id=?uuid",
            pool,
            q,
            sqlx::query_scalar::<_, bool>(&q)
                .bind(source)
                .fetch_one(pool)
                .await
        )
    }

    async fn verify_intake_target(&self, source: &str, number: i64) -> Result<()> {
        let (installation, repository_id, repository, bot_login) = query_store!(
            self,
            "select installation_id,repository_id,repository_name,bot_login from feedback_jobs where source_message_id=?uuid",
            pool,
            q,
            sqlx::query_as::<_, (i64, i64, String, String)>(&q)
                .bind(source)
                .fetch_one(pool)
                .await
        )?;
        let target = crate::github::Destination {
            installation,
            repository_id,
            repository,
            bot_login,
        };
        let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
        let token = github.token(&target).await.map_err(storage)?;
        github.verify(&target, &token).await.map_err(storage)?;
        let status = github
            .feedback_status(&target, &token, number)
            .await
            .map_err(storage)?;
        if status.issue.number != number || !status.eligible {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    pub(super) async fn submit_intake_review(
        &self,
        actor: &UserId,
        channel: Uuid,
        message: Uuid,
        task: Uuid,
        request: Uuid,
        note: &str,
    ) -> Result<()> {
        let (source, node, _) = self
            .intake_owner_task(actor, channel, message, task)
            .await?;
        if node != "review" {
            return Err(RepositoryError::Conflict);
        }
        self.intake_receipt(
            &source,
            &node,
            &task.to_string(),
            &json!({"reviewed":true,"assessment_id":request}),
            Some(note),
            Some(actor),
        )
        .await
    }
}
