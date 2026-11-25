use super::{
    feedback_channel::{PINNED_JOB, SOURCE_AUTHORITY},
    *,
};
use crate::{
    chatbot::{CircleChatAgents, feedback::Decision},
    github::{CreateError, Destination, Issue},
};

macro_rules! run_query {
    ($service:expr,$statement:expr,[$($arg:expr),*],$constructor:expr,$action:ident) => {{
        let pg=matches!($service.store,Store::Pg(_));
        let statement=sql($statement,pg);
        match &$service.store {
            Store::Pg(pool)=>$constructor(&statement)$(.bind($arg))*.$action(pool).await.map_err(storage),
            Store::Sqlite(pool)=>$constructor(&statement)$(.bind($arg))*.$action(pool).await.map_err(storage),
        }
    }};
}
macro_rules! change {
    ($service:expr,$statement:expr,[$($arg:expr),*]) => {{
        let pg=matches!($service.store,Store::Pg(_));
        let statement=sql($statement,pg);
        match &$service.store {
            Store::Pg(pool)=>sqlx::query(&statement)$(.bind($arg))*.execute(pool).await.map_err(storage)?.rows_affected(),
            Store::Sqlite(pool)=>sqlx::query(&statement)$(.bind($arg))*.execute(pool).await.map_err(storage)?.rows_affected(),
        }
    }};
}

#[derive(sqlx::FromRow)]
struct Job {
    source: String,
    status: String,
    source_body: String,
    attempts: i64,
    installation_id: i64,
    repository_id: i64,
    repository_name: String,
    bot_login: String,
    published_title: Option<String>,
    published_body: Option<String>,
    issue_number: Option<i64>,
    issue_id: Option<i64>,
}
impl Job {
    fn target(&self) -> Destination {
        Destination {
            installation: self.installation_id,
            repository_id: self.repository_id,
            repository: self.repository_name.clone(),
            bot_login: self.bot_login.clone(),
        }
    }
    fn marker(&self) -> String {
        format!("<!-- sproyt-feedback:{} -->", self.source)
    }
}
struct Draft {
    kind: &'static str,
    title: Option<String>,
    body: String,
    number: Option<i64>,
}
enum Outcome {
    Review,
    Publish(Draft),
}

fn public_text(text: &str, max: usize) -> bool {
    let lower = text.to_lowercase();
    !text.trim().is_empty()
        && text.len() <= max
        && !text.chars().any(|c| c.is_control() && c != '\n')
        && ![
            "<!--",
            "@",
            "http://",
            "https://",
            "www.",
            "![",
            "ghp_",
            "github_pat_",
            "sk-",
            "-----begin",
        ]
        .iter()
        .any(|part| lower.contains(part))
}

impl WorkItems {
    pub(crate) fn start_feedback_worker(
        &self,
        agents: CircleChatAgents,
        chat: ChatEngine,
        mut stop: watch::Receiver<bool>,
    ) {
        if std::env::var("SPROYT_FEEDBACK_ENABLED").as_deref() != Ok("true") {
            return;
        }
        if self.github.is_none() || !agents.available() || self.heart_url.is_none() {
            tracing::error!(
                "feedback enabled but GitHub, Heart or the standard chat model is missing"
            );
            return;
        }
        let worker = self.clone();
        tokio::spawn(async move {
            while !*stop.borrow() {
                if let Err(error) = worker.feedback_tick(&agents, &chat).await {
                    tracing::warn!(error_kind = error.kind(), "feedback worker deferred");
                }
                tokio::select! {_=tokio::time::sleep(Duration::from_secs(4))=>{},_=stop.changed()=>{}}
            }
        });
    }

    async fn feedback_tick(&self, agents: &CircleChatAgents, chat: &ChatEngine) -> Result<()> {
        self.admit_feedback().await?;
        let now = Utc::now().timestamp();
        // Serialise new decisions for this repository across replicas. An uncertain
        // write must reconcile before another message can create a competing issue.
        let next="select cast(j.source_message_id as text) from feedback_jobs j
            where j.status in ('pending','sending','uncertain','published','done') and j.available_at<=?
            and (j.status<>'pending' or exists(select 1 from feedback_routes route where route.channel_id=j.channel_id and route.enabled))
            and (j.status<>'pending' or not exists(select 1 from feedback_jobs p where p.repository_id=j.repository_id
                and p.status in ('pending','sending','uncertain') and
                (p.source_sequence,p.source_message_id)<(j.source_sequence,j.source_message_id)))
            and (j.status<>'done' or j.github_checked_at=0 or
                (j.publication_kind='create' and not exists(select 1 from feedback_heart h
                    where h.source_message_id=j.source_message_id and h.stage in ('resolved','rejected','failed'))) or
                (j.publication_kind<>'create' and j.github_state='open' and not exists(select 1 from feedback_jobs base
                    where base.repository_id=j.repository_id and base.issue_id=j.issue_id and base.status in ('published','done')
                    and (base.publication_kind='create' or base.source_message_id<j.source_message_id))))
            order by j.available_at,j.source_sequence,j.source_message_id limit 8";
        let ids = run_query!(
            self,
            next,
            [now],
            sqlx::query_scalar::<_, String>,
            fetch_all
        )?;
        for source in ids {
            let lease = Uuid::now_v7().to_string();
            let now = Utc::now().timestamp();
            let claim="update feedback_jobs set available_at=?,lease_token=?uuid,attempts=attempts+case when status='pending' then 1 else 0 end
                where source_message_id=?uuid and available_at<=? and status in ('pending','sending','uncertain','published','done')
                returning cast(source_message_id as text) as source,status,source_body,attempts,installation_id,repository_id,
                    repository_name,bot_login,published_title,published_body,issue_number,issue_id";
            let Some(job) = run_query!(
                self,
                claim,
                [now + 600, &lease, &source, now],
                sqlx::query_as::<_, Job>,
                fetch_optional
            )?
            else {
                continue;
            };
            let delay = match self.handle_feedback(&job, &lease, agents, chat).await {
                Ok(delay) => delay,
                Err(error) => {
                    tracing::warn!(source=%source,error_kind=error.kind(),"feedback job deferred");
                    if job.attempts >= 5 {
                        change!(
                            self,
                            "update feedback_jobs set status='needs_review' where source_message_id=?uuid and lease_token=?uuid and status='pending'",
                            [&source, &lease]
                        );
                    }
                    90
                }
            };
            change!(
                self,
                "update feedback_jobs set lease_token=null,available_at=? where source_message_id=?uuid and lease_token=?uuid",
                [Utc::now().timestamp() + delay, &source, &lease]
            );
        }
        Ok(())
    }
    pub(crate) async fn wake_feedback_issue(
        &self,
        installation: i64,
        repository: i64,
        issue: i64,
    ) -> Result<()> {
        // A signed event is a hint. A fresh, repository-scoped GET decides the outcome.
        change!(self,"update feedback_jobs set available_at=0,github_checked_at=0 where installation_id=? and repository_id=? and issue_id=?
            and status in ('published','done') and lease_token is null and not exists(select 1 from feedback_heart h
            where h.source_message_id=feedback_jobs.source_message_id and h.stage in ('resolved','rejected','failed'))",
            [installation,repository,issue]);
        Ok(())
    }
    async fn feedback_authorized(&self, job: &Job) -> Result<bool> {
        let query = format!(
            "select 1 from {SOURCE_AUTHORITY} join feedback_jobs j on {PINNED_JOB} where j.source_message_id=?uuid"
        );
        Ok(run_query!(
            self,
            &query,
            [&job.source],
            sqlx::query_scalar::<_, i32>,
            fetch_optional
        )?
        .is_some())
    }
    async fn feedback_state(&self, job: &Job, lease: &str, status: &str) -> Result<()> {
        if change!(
            self,
            "update feedback_jobs set status=? where source_message_id=?uuid and lease_token=?uuid",
            [status, &job.source, lease]
        ) == 0
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }

    async fn handle_feedback(
        &self,
        job: &Job,
        lease: &str,
        agents: &CircleChatAgents,
        chat: &ChatEngine,
    ) -> Result<i64> {
        let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
        let target = job.target();
        // If the chat grant changed, Heart can still follow the confirmed publication.
        if job.status == "published"
            && self
                .acknowledge_feedback(chat, Uuid::parse_str(&job.source).map_err(storage)?)
                .await
                .is_ok()
        {
            self.feedback_state(job, lease, "done").await?;
            return Ok(0);
        }
        if job.status == "pending" {
            if !self.feedback_authorized(job).await? {
                self.feedback_state(job, lease, "needs_review").await?;
                return Ok(0);
            }
            if !agents
                .qualifies_feedback(&job.source_body)
                .await
                .map_err(storage)?
            {
                self.feedback_state(job, lease, "ignored").await?;
                return Ok(0);
            }
        }
        let token = github.token(&target).await.map_err(storage)?;
        github.verify(&target, &token).await.map_err(storage)?;
        match job.status.as_str() {
            "sending" | "uncertain" => {
                let marker = job.marker();
                let text = format!(
                    "{}\n\n{marker}",
                    job.published_body
                        .as_deref()
                        .ok_or(RepositoryError::Conflict)?
                );
                let issue = if let Some(number) = job.issue_number {
                    github
                        .find_feedback_comment(&target, &token, number, &marker, &text)
                        .await
                } else {
                    github
                        .find(
                            &target,
                            &token,
                            &marker,
                            job.published_title
                                .as_deref()
                                .ok_or(RepositoryError::Conflict)?,
                            &text,
                        )
                        .await
                }
                .map_err(storage)?;
                if let Some(issue) = issue {
                    if job.published_title.is_some() {
                        let snapshot = github
                            .feedback_status(&target, &token, issue.number)
                            .await
                            .map_err(storage)?;
                        if snapshot.issue.id != issue.id {
                            return Err(RepositoryError::Conflict);
                        }
                        if !snapshot.proposed && snapshot.state == "open" {
                            change!(
                                self,
                                "update feedback_jobs set status='needs_review',issue_id=?,issue_number=?,issue_url=? where source_message_id=?uuid and lease_token=?uuid",
                                [issue.id, issue.number, &issue.url, &job.source, lease]
                            );
                            return Ok(0);
                        }
                    }
                    self.feedback_published(job, lease, &issue).await?;
                    return Ok(0);
                }
                self.feedback_state(job, lease, "uncertain").await?;
                Ok(300)
            }
            "done" | "published" => {
                let checked_at = Utc::now().timestamp();
                let snapshot = github
                    .feedback_status(
                        &target,
                        &token,
                        job.issue_number.ok_or(RepositoryError::Conflict)?,
                    )
                    .await
                    .map_err(storage)?;
                if Some(snapshot.issue.id) != job.issue_id {
                    return Err(RepositoryError::Conflict);
                }
                change!(self,"update feedback_jobs set github_state=?,github_state_reason=?,github_proposed=?,github_status_name=?,github_checked_at=?
                    where repository_id=? and issue_id=? and github_checked_at<=? and exists(select 1 from feedback_jobs owner_job
                    where owner_job.source_message_id=?uuid and owner_job.lease_token=?uuid)",
                    [&snapshot.state,snapshot.reason.as_deref(),snapshot.proposed,snapshot.status_name.as_deref(),checked_at,job.repository_id,snapshot.issue.id,checked_at,&job.source,lease]);
                Ok(120)
            }
            "pending" => {
                match self.feedback_draft(job, agents, &target, &token).await? {
                    Outcome::Review => self.feedback_state(job, lease, "needs_review").await?,
                    Outcome::Publish(draft) => {
                        let next = if draft.kind == "duplicate" {
                            "pending"
                        } else {
                            "sending"
                        };
                        let query=format!("update feedback_jobs set publication_kind=?,published_title=?,published_body=?,issue_number=?,status=?
                            where source_message_id=?uuid and lease_token=?uuid and status='pending' and exists(
                            select 1 from {SOURCE_AUTHORITY} join feedback_jobs j on {PINNED_JOB} where j.source_message_id=feedback_jobs.source_message_id)");
                        if change!(
                            self,
                            &query,
                            [
                                draft.kind,
                                draft.title.as_deref(),
                                &draft.body,
                                draft.number,
                                next,
                                &job.source,
                                lease
                            ]
                        ) != 1
                        {
                            return Err(RepositoryError::Conflict);
                        }
                        if draft.kind == "duplicate" {
                            let current = github
                                .feedback_status(
                                    &target,
                                    &token,
                                    draft.number.ok_or(RepositoryError::Conflict)?,
                                )
                                .await;
                            match current {
                                Ok(current) if current.eligible => {
                                    self.feedback_published(job, lease, &current.issue).await?
                                }
                                _ => self.feedback_state(job, lease, "pending").await?,
                            }
                        } else {
                            let body = format!("{}\n\n{}", draft.body, job.marker());
                            match github
                                .feedback_write(
                                    &target,
                                    &token,
                                    draft.title.as_deref(),
                                    draft.number,
                                    &body,
                                )
                                .await
                            {
                                Ok(issue) => self.feedback_published(job, lease, &issue).await?,
                                Err(CreateError::Rejected) => {
                                    self.feedback_state(
                                        job,
                                        lease,
                                        if job.attempts >= 5 {
                                            "needs_review"
                                        } else {
                                            "pending"
                                        },
                                    )
                                    .await?;
                                    return Ok(120);
                                }
                                Err(CreateError::Uncertain) => {
                                    self.feedback_state(job, lease, "uncertain").await?
                                }
                            }
                        }
                    }
                }
                Ok(0)
            }
            _ => Err(RepositoryError::Conflict),
        }
    }

    async fn feedback_draft(
        &self,
        job: &Job,
        agents: &CircleChatAgents,
        target: &Destination,
        token: &str,
    ) -> Result<Outcome> {
        let github = self.github.as_ref().ok_or(RepositoryError::Conflict)?;
        let Some(options) = github
            .feedback_candidates(target, token)
            .await
            .map_err(storage)?
        else {
            return Ok(Outcome::Review);
        };
        match agents
            .draft_feedback(&job.source_body, &options)
            .await
            .map_err(storage)?
        {
            Decision::New {
                title,
                need,
                criteria,
            } if public_text(&title, 160)
                && !title.contains('\n')
                && public_text(&need, 2500)
                && (1..=4).contains(&criteria.len())
                && criteria.iter().all(|c| public_text(c, 600)) =>
            {
                let body = format!(
                    "{}\n\n### Akseptansekriterium\n\n{}",
                    need.trim(),
                    criteria
                        .iter()
                        .map(|c| format!("- {}", c.trim()))
                        .collect::<Vec<_>>()
                        .join("\n")
                );
                Ok(Outcome::Publish(Draft {
                    kind: "create",
                    title: Some(title.trim().to_owned()),
                    body,
                    number: None,
                }))
            }
            Decision::Match { number } => {
                let Some(issue) = options.iter().find(|i| i.number == number && i.eligible) else {
                    return Ok(Outcome::Review);
                };
                let Some(comments) = github
                    .feedback_comments(target, token, number)
                    .await
                    .map_err(storage)?
                else {
                    return Ok(Outcome::Review);
                };
                let (kind, body) = match agents
                    .feedback_delta(&job.source_body, issue, &comments)
                    .await
                    .map_err(storage)?
                {
                    Decision::Duplicate => ("duplicate", String::new()),
                    Decision::Comment { body }
                        if public_text(&body, 4000) && body.trim() != job.source_body.trim() =>
                    {
                        ("comment", body)
                    }
                    _ => return Ok(Outcome::Review),
                };
                Ok(Outcome::Publish(Draft {
                    kind,
                    title: None,
                    body,
                    number: Some(number),
                }))
            }
            _ => Ok(Outcome::Review),
        }
    }
    async fn feedback_published(&self, job: &Job, lease: &str, issue: &Issue) -> Result<()> {
        if change!(
            self,
            "update feedback_jobs set status='published',issue_id=?,issue_number=?,issue_url=?
            where source_message_id=?uuid and lease_token=?uuid",
            [issue.id, issue.number, &issue.url, &job.source, lease]
        ) != 1
        {
            return Err(RepositoryError::Conflict);
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
