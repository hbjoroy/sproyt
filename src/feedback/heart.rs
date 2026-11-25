//! Sprøyt stores the product owner's assessment and GitHub observation. Heart
//! owns assignment, completion, the wait and the two terminal branches.
use super::*;

const FLOW: &str = include_str!("../../helm/sproyt/definitions/feedback-issue.yaml");
const CLIENT: &str = "sproyt-feedback";
const ACTIVE: &str = "stage in ('starting','review','review_submitted','waiting_for_close')";

// The source and its acknowledgement are interchangeable only when both the
// persisted receipt and an actual live message in this channel prove the link.
const SOURCE: &str = "
 join messages m on m.channel_id=j.channel_id and m.deleted_at is null
   and (m.id=j.source_message_id or (m.id=j.ack_message_id and m.sender_id=j.agent_id
        and m.parent_message_id=j.source_message_id))
 join messages original on original.id=j.source_message_id and original.channel_id=j.channel_id
   and original.deleted_at is null
 join channel_memberships cm on cm.channel_id=j.channel_id
";
const OWNER: &str = "
 j.reviewer_id=cm.user_id and cm.role<>'observer' and exists(
   select 1 from feedback_routes r
   join work_applications a on a.id=r.application_id and a.enabled
   join application_processors p on p.application_id=a.id and p.user_id=r.reviewer_id and p.can_review
   join application_process_roles pr on pr.application_id=a.id and pr.user_id=p.user_id
     and pr.process_role='product-handler'
   where r.channel_id=j.channel_id and r.application_id=j.application_id and r.enabled
     and r.reviewer_id=j.reviewer_id and r.agent_id=j.agent_id)
";

#[derive(Serialize)]
pub(crate) struct FeedbackIssue {
    pub issue_number: i64,
    pub issue_url: String,
    pub publication_kind: String,
    pub published_title: Option<String>,
    pub published_body: String,
    pub stage: String,
    pub heart_instance_id: Option<String>,
    pub can_review: bool,
    pub github_state: Option<String>,
    pub github_proposed: Option<bool>,
    pub github_status_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeedbackReview {
    pub assessment: String,
    pub request_id: Uuid,
}

// Match the repository's UUID representation on each backend. All values
// remain bound SQL parameters, including when composing shared predicates.
macro_rules! query_store {
    ($service:expr, $query:expr, $pool:ident, $statement:ident, $expression:expr) => {{
        let template = $query;
        match &$service.store {
            Store::Pg($pool) => {
                let $statement = sql(template.as_ref(), true);
                $expression
            }
            Store::Sqlite($pool) => {
                let $statement = sql(template.as_ref(), false);
                $expression
            }
        }
        .map_err(storage)
    }};
}

struct FeedbackRun {
    agent: String,
    reviewer: String,
    instance: Option<String>,
    review_request: Option<String>,
    closure: Option<String>,
}

impl WorkItems {
    pub(crate) async fn feedback_issue(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
    ) -> Result<Option<FeedbackIssue>> {
        let query = format!("select j.issue_number,j.issue_url,j.publication_kind,j.published_title,
            j.published_body,coalesce(h.stage,case when c.source_message_id is null then 'linked' else 'starting' end),
            cast(h.instance_id as text),case when h.stage='review' and j.publication_kind='create' and {OWNER}
              then true else false end,coalesce(c.github_state,j.github_state),coalesce(c.github_proposed,j.github_proposed),coalesce(c.github_status_name,j.github_status_name)
            from feedback_jobs j {SOURCE}
            left join feedback_jobs c on c.repository_id=j.repository_id and c.issue_id=j.issue_id
              and c.channel_id=j.channel_id and c.publication_kind='create' and c.status in ('published','done')
            left join feedback_heart h on h.source_message_id=c.source_message_id
            where cm.user_id=?uuid and j.channel_id=?uuid and m.id=?uuid and j.status in ('published','done')");
        type Projection = (
            i64,
            String,
            String,
            Option<String>,
            String,
            String,
            Option<String>,
            bool,
            Option<String>,
            Option<bool>,
            Option<String>,
        );
        let row = query_store!(
            self,
            query,
            pool,
            statement,
            sqlx::query_as::<_, Projection>(&statement)
                .bind(actor.to_string())
                .bind(channel.to_string())
                .bind(message.to_string())
                .fetch_optional(pool)
                .await
        )?;
        Ok(row.map(
            |(
                issue_number,
                issue_url,
                publication_kind,
                published_title,
                published_body,
                stage,
                heart_instance_id,
                can_review,
                github_state,
                github_proposed,
                github_status_name,
            )| FeedbackIssue {
                issue_number,
                issue_url,
                publication_kind,
                published_title,
                published_body,
                stage,
                heart_instance_id,
                can_review,
                github_state,
                github_proposed,
                github_status_name,
            },
        ))
    }

    pub(crate) async fn review_feedback(
        &self,
        actor: UserId,
        channel: Uuid,
        message: Uuid,
        command: FeedbackReview,
    ) -> Result<FeedbackIssue> {
        let assessment = command.assessment.trim();
        if assessment.is_empty() || assessment.chars().count() > 2000 || assessment.contains('\0') {
            return Err(RepositoryError::Conflict);
        }
        let query = format!("update feedback_heart set assessment=?,review_request_id=?uuid,
            reviewed_at=coalesce(reviewed_at,?),
            stage=case when stage='review' then 'review_submitted' else stage end,
            due_at=case when lease_token is null then 0 else due_at end
            where ((stage='review' and review_request_id is null) or (review_request_id=?uuid and assessment=?))
              and exists(select 1 from feedback_jobs j {SOURCE}
                where j.source_message_id=feedback_heart.source_message_id and j.channel_id=?uuid and m.id=?uuid
                and j.publication_kind='create' and cm.user_id=?uuid and {OWNER})");
        let updated = query_store!(
            self,
            query,
            pool,
            statement,
            sqlx::query(&statement)
                .bind(assessment)
                .bind(command.request_id.to_string())
                .bind(Utc::now().timestamp())
                .bind(command.request_id.to_string())
                .bind(assessment)
                .bind(channel.to_string())
                .bind(message.to_string())
                .bind(actor.to_string())
                .execute(pool)
                .await
                .map(|result| result.rows_affected())
        )?;
        if updated != 1 {
            return Err(RepositoryError::Conflict);
        }
        self.feedback_issue(actor, channel, message)
            .await?
            .ok_or(RepositoryError::NotFound)
    }

    pub(super) async fn sync_feedback_reviews(&self, now: i64) -> Result<()> {
        // A crash between GitHub success and this insert is recoverable.
        // Only the creation receipt can introduce a process for an issue.
        query_store!(
            self,
            "insert into feedback_heart(source_message_id)
            select j.source_message_id from feedback_jobs j where j.publication_kind='create'
            and j.status in ('published','done') and not exists(
              select 1 from feedback_heart h where h.source_message_id=j.source_message_id)
            order by j.source_message_id limit 10 on conflict(source_message_id) do nothing",
            pool,
            q,
            sqlx::query(&q).execute(pool).await.map(|_| ())
        )?;
        let select = format!(
            "select cast(source_message_id as text) from feedback_heart
            where {ACTIVE} and due_at<=? order by due_at,source_message_id limit 5"
        );
        let ids = query_store!(
            self,
            select,
            pool,
            q,
            sqlx::query_scalar::<_, String>(&q)
                .bind(now)
                .fetch_all(pool)
                .await
        )?;
        for source in ids {
            let token = Uuid::now_v7().to_string();
            let claim = format!(
                "update feedback_heart set due_at=?,lease_token=?uuid
                where source_message_id=?uuid and due_at<=? and {ACTIVE}"
            );
            let claimed = query_store!(
                self,
                claim,
                pool,
                q,
                sqlx::query(&q)
                    .bind(now + 120)
                    .bind(&token)
                    .bind(&source)
                    .bind(now)
                    .execute(pool)
                    .await
                    .map(|res| res.rows_affected())
            )?;
            if claimed == 0 {
                continue;
            }
            let result = self.drive_feedback_review(&source, &token).await;
            if let Err(error) = &result {
                tracing::warn!(
                    error_kind = error.kind(),
                    "feedback Heart synchronization deferred"
                );
            }
            let delay = match result {
                Ok(true) => 2,
                Ok(false) => 30,
                Err(_) => 30,
            };
            query_store!(
                self,
                "update feedback_heart set lease_token=null,due_at=?
                where source_message_id=?uuid and lease_token=?uuid",
                pool,
                q,
                sqlx::query(&q)
                    .bind(now + delay)
                    .bind(&source)
                    .bind(&token)
                    .execute(pool)
                    .await
                    .map(|_| ())
            )?;
        }
        Ok(())
    }

    async fn drive_feedback_review(&self, source: &str, token: &str) -> Result<bool> {
        type Run = (
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let (agent,reviewer,instance,review_request,closure) = query_store!(self,
            "select cast(j.agent_id as text),cast(j.reviewer_id as text),cast(h.instance_id as text),
             cast(h.review_request_id as text),h.closure from feedback_heart h join feedback_jobs j
             on j.source_message_id=h.source_message_id where h.source_message_id=?uuid and h.lease_token=?uuid",
            pool,q,sqlx::query_as::<_,Run>(&q).bind(source).bind(token).fetch_one(pool).await)?;
        let run = FeedbackRun {
            agent,
            reviewer,
            instance,
            review_request,
            closure,
        };
        let base = self
            .heart_url
            .as_deref()
            .ok_or_else(|| storage("Heart is unavailable"))?;
        let Some(instance) = &run.instance else {
            let definition = self
                .heart_response(
                    self.http
                        .post(format!("{base}/api/v2/definitions"))
                        .header("Content-Type", "application/yaml")
                        .body(FLOW),
                )
                .await?;
            if definition["namespace"] != "sproyt"
                || definition["name"] != "feedback-issue"
                || definition["version"] != "1.0.0"
                || definition["runtime"] != "v2"
            {
                return Err(RepositoryError::Conflict);
            }
            let def_id = json_uuid(&definition["id"])?;
            let input =
                json!({"source_message_id":source,"reviewer_id":run.reviewer,"agent_id":run.agent});
            let start=self.heart_response(self.http.post(format!("{base}/api/v2/instances"))
                .header("X-Heart-Client",CLIENT).header("Idempotency-Key",source)
                .json(&json!({"definition_id":def_id,"actor_id":run.agent,"input_metadata":input}))).await?;
            let created = &start["instance"];
            if created["definition_id"] != def_id.to_string()
                || created["input_metadata"] != input
                || created["runtime"] != "v2"
            {
                return Err(RepositoryError::Conflict);
            }
            let id = json_uuid(&created["id"])?;
            query_store!(
                self,
                "update feedback_heart set instance_id=?uuid where source_message_id=?uuid
                and lease_token=?uuid and instance_id is null",
                pool,
                q,
                sqlx::query(&q)
                    .bind(id.to_string())
                    .bind(source)
                    .bind(token)
                    .execute(pool)
                    .await
                    .map(|_| ())
            )?;
            return Ok(true);
        };
        let view = self
            .heart_response(self.http.get(format!("{base}/api/v2/instances/{instance}")))
            .await?;
        if view["id"] != *instance
            || view["runtime"] != "v2"
            || view["namespace"] != "sproyt"
            || view["input_metadata"]["source_message_id"] != source
            || view["input_metadata"]["reviewer_id"] != run.reviewer
            || view["input_metadata"]["agent_id"] != run.agent
        {
            return Err(RepositoryError::Conflict);
        }
        let status = view["status"].as_str().ok_or(RepositoryError::Conflict)?;
        if matches!(status, "cancelled" | "failed") {
            return self.feedback_stage(source, token, "failed").await;
        }
        if !matches!(status, "running" | "waiting" | "completed") {
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
        let mut review = None;
        let mut wait = None;
        for task in &tasks {
            if task.instance_id.to_string() != *instance
                || !matches!(task.status.as_str(), "pending" | "completed")
            {
                return Err(RepositoryError::Conflict);
            }
            let slot = match task.node_id.as_str() {
                "review" if task.assignee_id.to_string() == run.reviewer => &mut review,
                "await-github" if task.assignee_id.to_string() == run.agent => &mut wait,
                _ => return Err(RepositoryError::Conflict),
            };
            if slot.replace(task).is_some() {
                return Err(RepositoryError::Conflict);
            }
        }
        let Some(review) = review else {
            return if wait.is_none() && status != "completed" {
                Ok(false)
            } else {
                Err(RepositoryError::Conflict)
            };
        };
        if review.status == "pending" {
            if wait.is_some() || status == "completed" {
                return Err(RepositoryError::Conflict);
            }
            if let Some(request) = &run.review_request {
                self.complete_feedback_task(base, review, request, review_result(request))
                    .await?;
                return Ok(true);
            }
            return self.feedback_stage(source, token, "review").await;
        }
        let request = run
            .review_request
            .as_ref()
            .ok_or(RepositoryError::Conflict)?;
        if review.result_metadata != Some(review_result(request)) {
            return Err(RepositoryError::Conflict);
        }
        let Some(wait) = wait else {
            return if status == "completed" {
                Err(RepositoryError::Conflict)
            } else {
                Ok(false)
            };
        };
        if wait.status == "pending" {
            if status == "completed" {
                return Err(RepositoryError::Conflict);
            }
            let changed = self
                .feedback_stage(source, token, "waiting_for_close")
                .await?;
            if let Some(closure) = self.freeze_feedback_closure(source, token).await? {
                // Use Heart's globally unique activation ID as the immutable command key.
                self.complete_feedback_task(
                    base,
                    wait,
                    &wait.id.to_string(),
                    closure_result(&closure),
                )
                .await?;
                return Ok(true);
            }
            return Ok(changed);
        }
        let closure = run.closure.ok_or(RepositoryError::Conflict)?;
        if wait.result_metadata != Some(closure_result(&closure)) {
            return Err(RepositoryError::Conflict);
        }
        if status == "completed" {
            return self.feedback_stage(source, token, &closure).await;
        }
        Ok(false)
    }

    async fn feedback_stage(&self, source: &str, token: &str, next: &str) -> Result<bool> {
        // The HTTP assessment may arrive during a poll. Never revert that
        // accepted command to an actionable review.
        query_store!(
            self,
            "update feedback_heart set stage=? where source_message_id=?uuid and lease_token=?uuid
            and stage<>? and (?<>'review' or review_request_id is null)",
            pool,
            q,
            sqlx::query(&q)
                .bind(next)
                .bind(source)
                .bind(token)
                .bind(next)
                .bind(next)
                .execute(pool)
                .await
                .map(|r| r.rows_affected() > 0)
        )
    }

    async fn freeze_feedback_closure(&self, source: &str, token: &str) -> Result<Option<String>> {
        // Pin the outcome before sending. Reopening before this point clears
        // eligibility; a lost completion response replays the frozen result.
        query_store!(self,"update feedback_heart set closure=coalesce(closure,(
            select case j.github_state_reason when 'completed' then 'resolved' when 'not_planned' then 'rejected' end
            from feedback_jobs j where j.source_message_id=feedback_heart.source_message_id and j.github_state='closed'
            and j.github_checked_at>feedback_heart.reviewed_at))
            where source_message_id=?uuid and lease_token=?uuid and stage='waiting_for_close'
            and review_request_id is not null returning closure",pool,q, sqlx::query_scalar::<_,Option<String>>(&q)
            .bind(source).bind(token).fetch_one(pool).await)
    }

    async fn complete_feedback_task(
        &self,
        base: &str,
        task: &HeartTask,
        key: &str,
        result: Value,
    ) -> Result<()> {
        crate::operations::dependencies::heart_completion(
            self.http
                .post(format!("{base}/api/v2/user-tasks/{}/complete", task.id))
                .header("X-Heart-Client", CLIENT)
                .header("Idempotency-Key", key)
                .json(&json!({"actor_id":task.assignee_id,"result_metadata":result})),
        )
        .await
        .map_err(storage)
    }
}

fn json_uuid(value: &Value) -> Result<Uuid> {
    value
        .as_str()
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(RepositoryError::Conflict)
}
fn review_result(request: &str) -> Value {
    json!({"reviewed":true,"assessment_id":request})
}
fn closure_result(outcome: &str) -> Value {
    json!({"outcome":outcome})
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::{Path, State},
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use std::sync::{Arc, Mutex};

    // Simulate both writes being committed even though the HTTP reply is lost.
    #[derive(Clone)]
    struct Fixture {
        input: Value,
        definition: Uuid,
        instance: Uuid,
        review: Uuid,
        wait: Uuid,
        results: Arc<Mutex<(Option<Value>, Option<Value>)>>,
    }
    impl Fixture {
        fn view(&self) -> Value {
            let status = if self.results.lock().unwrap().1.is_some() {
                "completed"
            } else {
                "waiting"
            };
            json!({"id":self.instance,"definition_id":self.definition,"namespace":"sproyt",
                "runtime":"v2","status":status,"input_metadata":self.input})
        }
    }
    async fn complete(
        State(f): State<Fixture>,
        Path(id): Path<Uuid>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        let mut results = f.results.lock().unwrap();
        let (target, key, actor) = if id == f.review {
            (
                &mut results.0,
                body["result_metadata"]["assessment_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                &f.input["reviewer_id"],
            )
        } else {
            assert_eq!(id, f.wait);
            (&mut results.1, f.wait.to_string(), &f.input["agent_id"])
        };
        assert_eq!(headers["idempotency-key"], key);
        assert_eq!(body["actor_id"], *actor);
        assert!(
            target.is_none(),
            "reading Heart after a lost reply must suppress a second completion"
        );
        *target = Some(body["result_metadata"].clone());
        StatusCode::BAD_GATEWAY
    }

    async fn seed(pool: &SqlitePool, query: &str, args: &[Uuid]) {
        let mut statement = sqlx::query(query);
        for id in args {
            statement = statement.bind(id.to_string());
        }
        statement.execute(pool).await.unwrap();
    }

    async fn exercise(reason: &str, outcome: &str) {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        let [reviewer, agent, source, channel, app, circle, ack, other] =
            std::array::from_fn(|_| Uuid::now_v7());
        for (id, kind) in [(reviewer, "human"), (agent, "agent"), (other, "human")] {
            sqlx::query("insert into users(id,kind,display_name) values(?,?,'Test')")
                .bind(id.to_string())
                .bind(kind)
                .execute(&pool)
                .await
                .unwrap();
        }
        seed(
            &pool,
            "insert into circles(id,slug,name,created_by) values(?,'circle','Krets',?)",
            &[circle, reviewer],
        )
        .await;
        seed(&pool,"insert into channels(id,slug,name,kind,created_by,circle_id) values(?,'feed','Tilbakemeldingar','private',?,?)",&[channel,reviewer,circle]).await;
        seed(
            &pool,
            "insert into channel_memberships(channel_id,user_id,role) values(?,?,'owner')",
            &[channel, reviewer],
        )
        .await;
        seed(&pool,"insert into work_applications(id,owner_circle_id,key,name,enabled,updated_by,created_at,updated_at) values(?,?,'sproyt','Sprøyt',1,?,1,1)",&[app,circle,reviewer]).await;
        seed(
            &pool,
            "insert into application_processors(application_id,user_id,can_review) values(?,?,1)",
            &[app, reviewer],
        )
        .await;
        seed(&pool,"insert into application_process_roles(application_id,user_id,process_role) values(?,?,'product-handler')",&[app,reviewer]).await;
        seed(&pool,"insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?,?,?,'feedback','feedback','Test',1,current_timestamp)",&[agent,reviewer,reviewer]).await;
        seed(&pool,"insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values(?,?,'[]','[]',1,?,?,1,1)",&[agent,circle,reviewer,reviewer]).await;
        seed(&pool,"insert into work_github_bindings(application_id,installation_id,repository_id,repository_name,bot_login,enabled,updated_by) values(?,1,2,'test/test','test[bot]',1,?)",&[app,reviewer]).await;
        seed(&pool,"insert into feedback_routes(channel_id,agent_id,reviewer_id,application_id,enabled,after_sequence) values(?,?,?,?,1,0)",&[channel,agent,reviewer,app]).await;
        seed(&pool,"insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,'Test',1,'Ny sak')",&[source,channel,reviewer]).await;
        seed(&pool,"insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,parent_message_id) values(?,?,?,'Agent',2,'Kvittering',?)",&[ack,channel,agent,source]).await;
        seed(&pool,"insert into feedback_jobs(source_message_id,channel_id,agent_id,reviewer_id,application_id,ack_message_id,
            source_sequence,source_body,binding_revision,repository_id,installation_id,repository_name,bot_login,status,publication_kind,
            published_title,published_body,issue_id,issue_number,issue_url,github_state,github_state_reason)
            values(?,?,?,?,?,?,1,'Ny sak',1,2,1,'test/test','test[bot]','done','create','Tittel','Omtale',3,4,'https://github.com/test/test/issues/4','closed','completed')",
            &[source,channel,agent,reviewer,app,ack]).await;
        let input = json!({"source_message_id":source,"reviewer_id":reviewer,"agent_id":agent});
        let f = Fixture {
            input,
            definition: Uuid::now_v7(),
            instance: Uuid::now_v7(),
            review: Uuid::now_v7(),
            wait: Uuid::now_v7(),
            results: Default::default(),
        };
        let router=Router::new().route("/api/v2/definitions",post(|State(f):State<Fixture>,body:String| async move {
            assert_eq!(body,FLOW);
            Json(json!({"id":f.definition,"runtime":"v2","namespace":"sproyt","name":"feedback-issue","version":"1.0.0"}))
        })).route("/api/v2/instances",post(|State(f):State<Fixture>,headers:HeaderMap,Json(body):Json<Value>|async move {
            assert_eq!(headers["idempotency-key"].to_str().unwrap(),f.input["source_message_id"].as_str().unwrap());
            assert_eq!(body["input_metadata"],f.input);
            Json(json!({"instance":f.view()}))
        })).route("/api/v2/instances/{id}",get(|State(f):State<Fixture>|async move {Json(f.view())}))
        .route("/api/v2/user-tasks",get(|State(f):State<Fixture>|async move {
            let results=f.results.lock().unwrap();
            let task=|id:Uuid,node:&str,actor:&Value,result:&Option<Value>| json!({"id":id,"instance_id":f.instance,"node_id":node,
                "assignee_id":actor,"status":if result.is_none() {"pending"} else {"completed"},"result_metadata":result});
            let mut tasks=vec![task(f.review,"review",&f.input["reviewer_id"],&results.0)];
            if results.0.is_some(){tasks.push(task(f.wait,"await-github",&f.input["agent_id"],&results.1));}
            Json(tasks)
        })).route("/api/v2/user-tasks/{id}/complete",post(complete)).with_state(f.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let service = WorkItems {
            heart_url: Some(format!("http://{}", listener.local_addr().unwrap())),
            store: Store::Sqlite(pool.clone()),
            vllm_url: None,
            vllm_key: None,
            http: reqwest::Client::new(),
            github: None,
            supplements_enabled: false,
        };
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        for ts in [100, 300, 500] {
            service.sync_feedback_reviews(ts).await.unwrap();
        }
        let actor = || UserId::from_uuid(reviewer);
        let user = || UserId::from_uuid(other);
        assert!(
            service
                .feedback_issue(user(), channel, source)
                .await
                .unwrap()
                .is_none()
        );
        assert_eq!(
            service
                .feedback_issue(actor(), channel, ack)
                .await
                .unwrap()
                .unwrap()
                .stage,
            "review"
        );
        let request = Uuid::now_v7();
        let cmd = |note: &str| FeedbackReview {
            assessment: note.into(),
            request_id: request,
        };
        assert!(
            service
                .review_feedback(actor(), channel, source, cmd(" "))
                .await
                .is_err()
        );
        assert!(
            service
                .review_feedback(user(), channel, source, cmd("OK"))
                .await
                .is_err()
        );
        assert!(f.results.lock().unwrap().1.is_none());
        if outcome == "rejected" {
            // Closed before review, then reopened: no stale closing signal.
            seed(
                &pool,
                "update feedback_jobs set github_state='open',github_state_reason='reopened'",
                &[],
            )
            .await;
        }
        let accept = service
            .review_feedback(actor(), channel, ack, cmd("Vurdert for neste leveranse"))
            .await
            .unwrap();
        assert_eq!(accept.stage, "review_submitted");
        assert!(
            service
                .review_feedback(actor(), channel, source, cmd("Endra"))
                .await
                .is_err()
        );
        service.sync_feedback_reviews(700).await.unwrap(); // Heart commits, 502 returned.
        service.sync_feedback_reviews(900).await.unwrap();
        assert!(
            f.results.lock().unwrap().1.is_none(),
            "A stale pre-review closure must never complete Heart"
        );
        // Simulate the first successful GitHub GET started after acceptance.
        sqlx::query(
            "update feedback_jobs set github_checked_at=(select reviewed_at+1 from feedback_heart)",
        )
        .execute(&pool)
        .await
        .unwrap();
        if outcome == "rejected" {
            assert_eq!(
                service
                    .feedback_issue(actor(), channel, source)
                    .await
                    .unwrap()
                    .unwrap()
                    .stage,
                "waiting_for_close"
            );
            assert!(f.results.lock().unwrap().1.is_none());
            seed(
                &pool,
                "update feedback_jobs set github_state='closed',github_state_reason='duplicate'",
                &[],
            )
            .await;
            service.sync_feedback_reviews(1100).await.unwrap();
            assert!(f.results.lock().unwrap().1.is_none());
            sqlx::query("update feedback_jobs set github_state_reason=?")
                .bind(reason)
                .execute(&pool)
                .await
                .unwrap();
        }
        for ts in [1300, 1500, 1700] {
            service.sync_feedback_reviews(ts).await.unwrap();
        }
        assert_eq!(
            service
                .feedback_issue(actor(), channel, source)
                .await
                .unwrap()
                .unwrap()
                .stage,
            outcome
        );
        assert_eq!(f.results.lock().unwrap().1, Some(closure_result(outcome)));
        assert_eq!(
            service
                .review_feedback(actor(), channel, source, cmd("Vurdert for neste leveranse"))
                .await
                .unwrap()
                .stage,
            outcome
        );
        server.abort();
    }
    #[tokio::test]
    async fn feedback_heart_follows_verified_review_and_actual_closure_after_lost_responses() {
        exercise("completed", "resolved").await;
        exercise("not_planned", "rejected").await;
    }
}
