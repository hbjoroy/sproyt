//! Execute a single, observed Heart task for an admitted channel message. The
//! GitHub receipt and the channel acknowledgement can each be recovered before
//! the corresponding Heart task result is delivered.
use super::*;
use sha2::{Digest, Sha256};

fn comparison_digest(options: &[crate::github::feedback::Candidate]) -> Result<String> {
    let mut sorted = options.iter().collect::<Vec<_>>();
    sorted.sort_by_key(|issue| issue.number);
    Ok(hex::encode(Sha256::digest(
        serde_json::to_vec(&sorted).map_err(storage)?,
    )))
}

fn story_digest(title: Option<&str>, body: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(title.unwrap_or_default().as_bytes());
    digest.update([0]);
    digest.update(body.as_bytes());
    hex::encode(digest.finalize())
}

#[derive(sqlx::FromRow)]
struct ActiveTask {
    stage: String,
    task: String,
}

impl WorkItems {
    pub(super) async fn handle_intake_task(
        &self,
        job: &Job,
        lease: &str,
        agents: &CircleChatAgents,
        chat: &ChatEngine,
    ) -> Result<i64> {
        let task = run_query!(
            self,
            "select i.stage,cast(i.task_id as text) as task from feedback_intakes i
             join feedback_jobs j on j.source_message_id=i.source_message_id
             where i.source_message_id=?uuid and j.lease_token=?uuid and i.task_id is not null
             and not exists(select 1 from feedback_intake_results x where x.task_id=i.task_id)",
            [&job.source, lease],
            sqlx::query_as::<_, ActiveTask>,
            fetch_optional
        )?;
        let Some(task) = task else {
            // The link's Heart process is complete. The channel receipt still
            // displays the issue's last observed status, including for issues
            // that were originally created outside Sprøyt.
            if job.status == "done" && job.publication_kind.as_deref() != Some("create") {
                let linked=run_query!(self,"select 1 from feedback_intakes where source_message_id=?uuid and stage='linked'",
                    [&job.source],sqlx::query_scalar::<_,i32>,fetch_optional)?.is_some();
                if linked {
                    let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
                    let target = job.target();
                    let token = github.token(&target).await.map_err(storage)?;
                    github.verify(&target, &token).await.map_err(storage)?;
                    return self
                        .observe_feedback(job, lease, github, &target, &token)
                        .await;
                }
            }
            return Ok(4);
        };
        let operation = task.stage.strip_prefix("retry-").unwrap_or(&task.stage);
        if matches!(
            operation,
            "qualify" | "match" | "create" | "link" | "comment"
        ) && job.status == "pending"
            && !self.feedback_authorized(job).await?
        {
            if self.intake_source_edited(&job.source).await? {
                return self.request_intake_clarification(job,lease,&task,
                    "Meldinga er endra etter at vurderinga starta. Den opphavlege teksten kan leggjast bort.",None).await;
            }
            // Suspend external access while the channel or agent grant is absent.
            return Ok(60);
        }
        if operation == "qualify" {
            if job.intake_clarification_needed {
                return self
                    .request_intake_clarification(
                        job,
                        lease,
                        &task,
                        "Meldinga venta på manuell avklaring før overgangen til Heart.",
                        None,
                    )
                    .await;
            }
            return match agents.qualifies_feedback(&job.source_body).await {
                Ok(qualified) => {
                    let outcome = if qualified { "qualified" } else { "ignored" };
                    self.record_intake_result(
                        &job.source,
                        &task.stage,
                        &task.task,
                        &json!({"outcome":outcome}),
                    )
                    .await?;
                    Ok(0)
                }
                Err(_) if job.attempts >= 5 => {
                    self.request_intake_clarification(
                        job,
                        lease,
                        &task,
                        "Agenten fekk ikkje fullført vurderinga av meldinga.",
                        None,
                    )
                    .await
                }
                Err(error) => Err(storage(error)),
            };
        }
        if operation == "ack" {
            self.assert_intake_activation(&job.source, &task.stage, &task.task)
                .await?;
            self.acknowledge_feedback(chat, Uuid::parse_str(&job.source).map_err(storage)?)
                .await?;
            self.feedback_state(job, lease, "done").await?;
            let outcome = if job.publication_kind.as_deref() == Some("create") {
                "created"
            } else {
                "linked"
            };
            self.record_intake_result(
                &job.source,
                &task.stage,
                &task.task,
                &json!({"outcome":outcome}),
            )
            .await?;
            return Ok(0);
        }
        let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
        let target = job.target();
        let token = github.token(&target).await.map_err(storage)?;
        github.verify(&target, &token).await.map_err(storage)?;
        match operation {
            "match" => {
                let Some(options) = github
                    .feedback_candidates(&target, &token)
                    .await
                    .map_err(storage)?
                else {
                    return self
                        .request_intake_clarification(
                            job,
                            lease,
                            &task,
                            "Det var for mange opne saker til at agenten kunne samanlikne alle.",
                            Some(&[]),
                        )
                        .await;
                };
                let prepared = async {
                    let decision = agents
                        .draft_feedback(&job.source_body, &options)
                        .await
                        .map_err(storage)?;
                    self.prepare_feedback_draft(job, agents, &target, &token, decision, &options)
                        .await
                }
                .await;
                let decision =
                    match prepared {
                        Ok(decision) => decision,
                        Err(_) if job.attempts >= 5 => return self
                            .request_intake_clarification(
                                job,
                                lease,
                                &task,
                                "Agenten fekk ikkje fullført samanlikninga med dei opne sakene.",
                                Some(&options),
                            )
                            .await,
                        Err(error) => return Err(error),
                    };
                let Outcome::Publish(draft) = decision else {
                    return self.request_intake_clarification(job,lease,&task,
                        "Agenten fann ikkje eitt sikkert val mellom ny sak og ei eksisterande sak.",Some(&options)).await;
                };
                let outcome = if draft.kind == "duplicate" {
                    "link"
                } else {
                    draft.kind
                };
                self.remember_intake_draft(job, lease, &task, &draft, "pending")
                    .await?;
                let mut result = json!({"outcome":outcome});
                if outcome == "create" {
                    result["comparison_digest"] = json!(comparison_digest(&options)?);
                    result["story_digest"] =
                        json!(story_digest(draft.title.as_deref(), &draft.body));
                }
                if let Some(number) = draft.number {
                    result["issue_number"] = json!(number)
                }
                self.record_intake_result(&job.source, &task.stage, &task.task, &result)
                    .await?;
                Ok(0)
            }
            "create" | "comment" | "link" => {
                if job.status == "published" {
                    self.record_intake_result(
                        &job.source,
                        &task.stage,
                        &task.task,
                        &json!({"outcome":"published"}),
                    )
                    .await?;
                    return Ok(0);
                }
                if matches!(job.status.as_str(), "uncertain" | "sending") {
                    return self
                        .reconcile_feedback(job, lease, github, &target, &token)
                        .await;
                }
                if job.status != "pending" {
                    return Err(RepositoryError::Conflict);
                }
                self.publish_intake_draft(job, lease, &task, operation, agents, &target, &token)
                    .await
            }
            "review" | "await-github" => {
                self.observe_feedback(job, lease, github, &target, &token)
                    .await
            }
            _ => Err(RepositoryError::Conflict),
        }
    }

    async fn request_intake_clarification(
        &self,
        job: &Job,
        lease: &str,
        task: &ActiveTask,
        reason: &str,
        options: Option<&[crate::github::feedback::Candidate]>,
    ) -> Result<i64> {
        let candidates = options.map(|items| {
            let list = items
                .iter()
                .filter(|i| i.eligible)
                .map(|i| json!({"number":i.number,"title":i.title}))
                .collect::<Vec<_>>();
            json!(list).to_string()
        });
        let changed=change!(self,"update feedback_intakes set reason=?,candidates=coalesce(?,candidates)
            where source_message_id=?uuid and task_id=?uuid and not exists(select 1 from feedback_intake_results x
            where x.task_id=feedback_intakes.task_id) and exists(select 1 from feedback_jobs j where
            j.source_message_id=feedback_intakes.source_message_id and j.lease_token=?uuid)",
            [reason,candidates.as_deref(),&job.source,&task.task,lease]);
        if changed != 1 {
            return Err(RepositoryError::Conflict);
        }
        self.record_intake_result(
            &job.source,
            &task.stage,
            &task.task,
            &json!({"outcome":"clarify"}),
        )
        .await?;
        Ok(0)
    }

    async fn remember_intake_draft(
        &self,
        job: &Job,
        lease: &str,
        task: &ActiveTask,
        draft: &Draft,
        next: &str,
    ) -> Result<()> {
        let statement=format!("update feedback_jobs set publication_kind=?,published_title=?,published_body=?,issue_number=?,status=?
            where source_message_id=?uuid and lease_token=?uuid and status='pending' and issue_id is null
            and exists(select 1 from {SOURCE_AUTHORITY} join feedback_jobs j on {PINNED_JOB} where j.source_message_id=feedback_jobs.source_message_id)
            and exists(select 1 from feedback_intakes i where i.source_message_id=feedback_jobs.source_message_id
            and i.stage=? and i.task_id=?uuid and not exists(select 1 from feedback_intake_results x where x.task_id=i.task_id))");
        if change!(
            self,
            &statement,
            [
                draft.kind,
                draft.title.as_deref(),
                &draft.body,
                draft.number,
                next,
                &job.source,
                lease,
                &task.stage,
                &task.task
            ]
        ) != 1
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    async fn publish_intake_draft(
        &self,
        job: &Job,
        lease: &str,
        task: &ActiveTask,
        operation: &str,
        agents: &CircleChatAgents,
        target: &Destination,
        token: &str,
    ) -> Result<i64> {
        let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
        // A retry always uses this owner's recovery decision. An ordinary
        // publication uses the initial owner decision if Heart took that branch.
        let manual_node = if task.stage.starts_with("retry-") {
            "clarify-publication"
        } else {
            "clarify"
        };
        let owner = run_query!(
            self,
            "select result_json from feedback_intake_results where source_message_id=?uuid and node_id=?",
            [&job.source, manual_node],
            sqlx::query_scalar::<_, String>,
            fetch_optional
        )?;
        let owner = owner
            .as_deref()
            .map(serde_json::from_str::<Value>)
            .transpose()
            .map_err(storage)?;
        let decision = if let Some(owner) = owner.as_ref() {
            owner.clone()
        } else {
            let pinned: String = run_query!(
                self,
                "select result_json from feedback_intake_results where source_message_id=?uuid and node_id='match'",
                [&job.source],
                sqlx::query_scalar::<_, String>,
                fetch_one
            )?;
            serde_json::from_str(&pinned).map_err(storage)?
        };
        if decision["outcome"] != operation {
            return Err(RepositoryError::Conflict);
        }
        let draft = if operation == "create" {
            if owner.is_none() {
                // Re-check both the comparison set and the draft tied to Heart's
                // decision. A stale worker cannot substitute a different story.
                let body = job
                    .published_body
                    .as_deref()
                    .ok_or(RepositoryError::Conflict)?;
                if job.publication_kind.as_deref() != Some("create")
                    || decision["story_digest"]
                        != story_digest(job.published_title.as_deref(), body)
                {
                    return self
                        .request_intake_clarification(
                            job,
                            lease,
                            task,
                            "Utkastet samsvarar ikkje lenger med den registrerte avgjerda.",
                            None,
                        )
                        .await;
                }
                let latest = github
                    .feedback_candidates(target, token)
                    .await
                    .map_err(storage)?;
                if latest
                    .as_ref()
                    .map(|value| comparison_digest(value))
                    .transpose()?
                    .as_deref()
                    != decision["comparison_digest"].as_str()
                    || latest.is_none()
                {
                    return self
                        .request_intake_clarification(
                            job,
                            lease,
                            task,
                            "Dei opne GitHub-sakene har endra seg sidan samanlikninga.",
                            latest.as_deref(),
                        )
                        .await;
                }
                Draft {
                    kind: "create",
                    title: Some(
                        job.published_title
                            .clone()
                            .ok_or(RepositoryError::Conflict)?,
                    ),
                    body: job
                        .published_body
                        .clone()
                        .ok_or(RepositoryError::Conflict)?,
                    number: None,
                }
            } else {
                let decision = agents
                    .draft_feedback(&job.source_body, &[])
                    .await
                    .map_err(storage)?;
                match self
                    .prepare_feedback_draft(job, agents, target, token, decision, &[])
                    .await?
                {
                    Outcome::Publish(draft) if draft.kind == "create" => draft,
                    _ => {
                        return self
                            .request_intake_clarification(
                                job,
                                lease,
                                task,
                                "Agenten kunne ikkje lage ei brukarhistorie som kan publiserast.",
                                None,
                            )
                            .await;
                    }
                }
            }
        } else {
            let number = decision["issue_number"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or(RepositoryError::Conflict)?;
            let Some(candidate) = github
                .feedback_candidate(target, token, number)
                .await
                .map_err(storage)?
            else {
                return self
                    .request_intake_clarification(
                        job,
                        lease,
                        task,
                        "Den valde GitHub-saka er utilgjengeleg eller for stor til å vurdere.",
                        None,
                    )
                    .await;
            };
            if !candidate.eligible {
                return self.request_intake_clarification(job,lease,task,
                    "Den valde GitHub-saka er lukka, påbegynt eller utan avklart arbeidsstatus.",None).await;
            }
            if operation == "link" {
                Draft {
                    kind: "duplicate",
                    number: Some(number),
                    body: String::new(),
                    title: None,
                }
            } else {
                // Re-read the comments at publication, including contributions
                // made after the match was originally proposed.
                match self
                    .prepare_feedback_draft(
                        job,
                        agents,
                        target,
                        token,
                        Decision::Match { number },
                        &[candidate],
                    )
                    .await?
                {
                    Outcome::Publish(draft) => draft,
                    Outcome::Review => {
                        return self
                            .request_intake_clarification(
                                job,
                                lease,
                                task,
                                "Agenten kunne ikkje avgjere kva meldinga tilfører den valde saka.",
                                None,
                            )
                            .await;
                    }
                }
            }
        };
        self.assert_intake_activation(&job.source, &task.stage, &task.task)
            .await?;
        let next = if draft.kind == "duplicate" {
            "pending"
        } else {
            "sending"
        };
        self.remember_intake_draft(job, lease, task, &draft, next)
            .await?;
        if draft.kind == "duplicate" {
            let number = draft.number.ok_or(RepositoryError::Conflict)?;
            let snapshot = github
                .feedback_status(target, token, number)
                .await
                .map_err(storage)?;
            if !snapshot.eligible {
                return self
                    .request_intake_clarification(
                        job,
                        lease,
                        task,
                        "Saka endra status før tilbakemeldinga vart knytt til henne.",
                        None,
                    )
                    .await;
            }
            self.feedback_published(job, lease, &snapshot.issue).await?;
        } else {
            let body = format!("{}\n\n{}", draft.body, job.marker());
            match github
                .feedback_write(target, token, draft.title.as_deref(), draft.number, &body)
                .await
            {
                Ok(issue) => self.feedback_published(job, lease, &issue).await?,
                Err(CreateError::Uncertain) => self.feedback_state(job, lease, "uncertain").await?,
                Err(CreateError::Rejected) => {
                    self.feedback_state(job, lease, "pending").await?;
                    return self.request_intake_clarification(job,lease,task,"GitHub avviste publiseringa. Kontroller saka og vel vidare behandling.",None).await;
                }
            }
        }
        Ok(0)
    }
}
