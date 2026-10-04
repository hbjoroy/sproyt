use super::*;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

pub(crate) struct Fixture {
    pub(crate) service: WorkItems,
    pub(crate) owner: Uuid,
    pub(crate) reviewer: Uuid,
    pub(crate) outsider: Uuid,
    pub(crate) channel: Uuid,
    pub(crate) application: Uuid,
    pub(crate) item: Uuid,
    pub(crate) task: Uuid,
    pub(crate) message: Uuid,
    lease: Uuid,
    export_instance: Uuid,
}

impl Fixture {
    pub(crate) async fn sqlite(projected: bool) -> Self {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        Self::create(Store::Sqlite(pool), projected).await
    }
    pub(crate) async fn postgres(projected: bool) -> Option<Self> {
        let url = std::env::var("SPROYT_POSTGRES_TEST_URL").ok()?;
        let pool = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .unwrap();
        Some(Self::create(Store::Pg(pool), projected).await)
    }
    async fn create(store: Store, projected: bool) -> Self {
        macro_rules! run { ($query:expr $(,$value:expr)* $(,)?) => {{
            match &store {
                Store::Pg(pool) => { let query=sql($query,true); let mut request=sqlx::query(&query); $(request=request.bind($value);)* request.execute(pool).await.unwrap(); }
                Store::Sqlite(pool) => { let query=sql($query,false); let mut request=sqlx::query(&query); $(request=request.bind($value);)* request.execute(pool).await.unwrap(); }
            }
        }}; }
        let owner = Uuid::now_v7();
        let reviewer = Uuid::now_v7();
        let outsider = Uuid::now_v7();
        let circle = Uuid::now_v7();
        let channel = Uuid::now_v7();
        let application = Uuid::now_v7();
        let binding = Uuid::now_v7();
        let source = Uuid::now_v7();
        let item = Uuid::now_v7();
        let task = Uuid::now_v7();
        let message = Uuid::now_v7();
        let lease = Uuid::now_v7();
        let export_instance = Uuid::now_v7();
        for user in [owner, reviewer, outsider] {
            run!(
                "insert into users(id,kind,display_name) values(?uuid,'human','Tester')",
                user.to_string()
            );
        }
        run!(
            "insert into circles(id,slug,name,created_by) values(?uuid,?,'Circle',?uuid)",
            circle.to_string(),
            circle.to_string(),
            owner.to_string()
        );
        run!(
            "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,'owner')",
            circle.to_string(),
            owner.to_string()
        );
        run!(
            "insert into channels(id,slug,name,kind,created_by,circle_id) values(?uuid,?,'Issues','private',?uuid,?uuid)",
            channel.to_string(),
            channel.to_string(),
            owner.to_string(),
            circle.to_string()
        );
        for (user, role) in [(owner, "owner"), (reviewer, "member")] {
            run!(
                "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,?)",
                channel.to_string(),
                user.to_string(),
                role
            );
        }
        run!(
            "insert into channel_sequences(channel_id,next_sequence) values(?uuid,3)",
            channel.to_string()
        );
        run!(
            "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Tester',1,'A reviewed case')",
            source.to_string(),
            channel.to_string(),
            owner.to_string()
        );
        run!(
            "insert into work_applications(id,owner_circle_id,key,name,enabled,updated_by,created_at,updated_at) values(?uuid,?uuid,?,'Sprøyt',true,?uuid,current_timestamp,current_timestamp)",
            application.to_string(),
            circle.to_string(),
            format!("test-{}", application.simple()),
            owner.to_string()
        );
        run!(
            "insert into channel_process_bindings(id,channel_id,process_key,namespace,definition_name,definition_version,enabled,updated_by,updated_at) values(?uuid,?uuid,'work-item','sproyt','work-item-review','1.0.0',true,?uuid,current_timestamp)",
            binding.to_string(),
            channel.to_string(),
            owner.to_string()
        );
        run!(
            "insert into channel_task_routes(id,binding_id,channel_id,task_key,process_role,enabled) values(?uuid,?uuid,?uuid,'publish-github','product-handler',true)",
            Uuid::now_v7().to_string(),
            binding.to_string(),
            channel.to_string()
        );
        run!(
            "insert into application_processors(application_id,user_id,can_review,can_export) values(?uuid,?uuid,true,true)",
            application.to_string(),
            reviewer.to_string()
        );
        run!(
            "insert into application_process_roles(application_id,user_id,process_role) values(?uuid,?uuid,'product-handler')",
            application.to_string(),
            reviewer.to_string()
        );
        run!(
            "insert into work_github_bindings(application_id,installation_id,repository_id,repository_name,bot_login,enabled,export_tasks_enabled,revision,updated_by) values(?uuid,7,42,'owner/repo','our-app[bot]',true,true,3,?uuid)",
            application.to_string(),
            owner.to_string()
        );
        run!(
            "insert into work_items(id,source_channel_id,source_message_id,source_body,application_id,binding_id,binding_revision,title,description,status,requested_by,request_id,reviewer_id,task_channel_id,start_status,process_status,revision) values(?uuid,?uuid,?uuid,'A reviewed case',?uuid,?uuid,1,'Reviewed bug','The editor disappears','planned',?uuid,?uuid,?uuid,?uuid,'started','completed',1)",
            item.to_string(),
            channel.to_string(),
            source.to_string(),
            application.to_string(),
            binding.to_string(),
            owner.to_string(),
            Uuid::now_v7().to_string(),
            reviewer.to_string(),
            channel.to_string()
        );
        run!(
            "insert into work_item_export_processes(work_item_id,channel_id,assignee_id,heart_instance_id,status,lease_token,created_at) values(?uuid,?uuid,?uuid,?uuid,'waiting',?uuid,1)",
            item.to_string(),
            channel.to_string(),
            reviewer.to_string(),
            export_instance.to_string(),
            lease.to_string()
        );
        if !projected {
            run!(
                "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Tester',2,?)",
                message.to_string(),
                channel.to_string(),
                reviewer.to_string(),
                format!("[[work-item-task:{task}]]")
            );
            run!(
                "insert into work_item_tasks(id,work_item_id,message_id,channel_id,assignee_id,node_id,status) values(?uuid,?uuid,?uuid,?uuid,?uuid,'publish-github','pending')",
                task.to_string(),
                item.to_string(),
                message.to_string(),
                channel.to_string(),
                reviewer.to_string()
            );
        }
        let service = WorkItems {
            supplements_enabled: true,
            store,
            heart_url: None,
            vllm_url: None,
            vllm_key: None,
            http: reqwest::Client::new(),
            github: Some(crate::github::GitHub::test_client(
                "http://127.0.0.1:1".into(),
            )),
        };
        Self {
            service,
            owner,
            reviewer,
            outsider,
            channel,
            application,
            item,
            task,
            message,
            lease,
            export_instance,
        }
    }
    fn command(&self, send: bool) -> ExportCommand {
        ExportCommand {
            message_id: self.message,
            request_id: Uuid::now_v7(),
            expected_revision: 1,
            title: if send {
                "Reviewed bug".into()
            } else {
                String::new()
            },
            body: if send {
                "Approved public report".into()
            } else {
                String::new()
            },
            send,
            expected_repository_id: send.then_some(42),
            expected_binding_revision: send.then_some(3),
        }
    }
    pub(crate) async fn count(&self, table: &str) -> i64 {
        let query = format!("select count(*) from {table} where work_item_id=?uuid");
        match &self.service.store {
            Store::Pg(pool) => sqlx::query_scalar::<_, i64>(&sql(&query, true))
                .bind(self.item.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
            Store::Sqlite(pool) => sqlx::query_scalar::<_, i64>(&sql(&query, false))
                .bind(self.item.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
        }
    }
    async fn revoke_export(&self) {
        let query = "update application_processors set can_export=false where application_id=?uuid and user_id=?uuid";
        match &self.service.store {
            Store::Pg(pool) => {
                sqlx::query(&sql(query, true))
                    .bind(self.application.to_string())
                    .bind(self.reviewer.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
            Store::Sqlite(pool) => {
                sqlx::query(&sql(query, false))
                    .bind(self.application.to_string())
                    .bind(self.reviewer.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }
    async fn change_binding_revision(&self, revision: i64) {
        let query = "update work_github_bindings set revision=? where application_id=?uuid";
        match &self.service.store {
            Store::Pg(pool) => {
                sqlx::query(&sql(query, true))
                    .bind(revision)
                    .bind(self.application.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
            Store::Sqlite(pool) => {
                sqlx::query(&sql(query, false))
                    .bind(revision)
                    .bind(self.application.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }
    async fn export_status(&self) -> String {
        let query = "select status from work_item_github_exports where work_item_id=?uuid";
        match &self.service.store {
            Store::Pg(pool) => sqlx::query_scalar(&sql(query, true))
                .bind(self.item.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
            Store::Sqlite(pool) => sqlx::query_scalar(&sql(query, false))
                .bind(self.item.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
        }
    }
    async fn reset_export_lease(&self) {
        let query = "update work_item_github_exports set lease_until=0 where work_item_id=?uuid";
        match &self.service.store {
            Store::Pg(pool) => {
                sqlx::query(&sql(query, true))
                    .bind(self.item.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
            Store::Sqlite(pool) => {
                sqlx::query(&sql(query, false))
                    .bind(self.item.to_string())
                    .execute(pool)
                    .await
                    .unwrap();
            }
        }
    }
    async fn decision_note(&self) -> String {
        let query = "select decision_note from work_item_tasks where id=?uuid";
        match &self.service.store {
            Store::Pg(pool) => sqlx::query_scalar(&sql(query, true))
                .bind(self.task.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
            Store::Sqlite(pool) => sqlx::query_scalar(&sql(query, false))
                .bind(self.task.to_string())
                .fetch_one(pool)
                .await
                .unwrap(),
        }
    }
}

async fn exercise_admission_and_revoke(fixture: Fixture) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let command = fixture.command(true);
    let view = fixture
        .service
        .task(reviewer.clone(), fixture.task, fixture.message)
        .await
        .unwrap();
    assert!(view.can_decide);
    assert_eq!(
        view.github_export.as_ref().unwrap().repository.as_deref(),
        Some("owner/repo")
    );
    assert!(matches!(
        fixture
            .service
            .task(
                UserId::from_uuid(fixture.outsider),
                fixture.task,
                fixture.message
            )
            .await,
        Err(RepositoryError::NotFound)
    ));
    assert!(matches!(
        fixture
            .service
            .export_github(
                UserId::from_uuid(fixture.owner),
                fixture.task,
                command.clone()
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    let accepted = fixture
        .service
        .export_github(reviewer.clone(), fixture.task, command.clone())
        .await
        .unwrap();
    assert_eq!(accepted.delivery_status, "pending");
    assert!(!accepted.can_decide);
    assert_eq!(fixture.count("work_item_github_exports").await, 1);
    fixture
        .service
        .export_github(reviewer.clone(), fixture.task, command.clone())
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .service
            .export_github(
                reviewer,
                fixture.task,
                ExportCommand {
                    title: "Changed".into(),
                    ..command
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert!(matches!(
        fixture
            .service
            .export_github(
                UserId::from_uuid(fixture.reviewer),
                fixture.task,
                ExportCommand {
                    expected_binding_revision: Some(4),
                    ..fixture.command(true)
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    fixture.revoke_export().await;
    fixture
        .service
        .deliver_exports(Utc::now().timestamp())
        .await
        .unwrap();
    assert_eq!(fixture.export_status().await, "blocked");
    assert_eq!(fixture.count("work_item_github_exports").await, 1);
}

#[tokio::test]
async fn sqlite_github_admission_idempotence_rights_and_revocation() {
    exercise_admission_and_revoke(Fixture::sqlite(false).await).await;
}

#[tokio::test]
async fn postgres_github_admission_idempotence_rights_and_revocation() {
    if let Some(fixture) = Fixture::postgres(false).await {
        exercise_admission_and_revoke(fixture).await;
    }
}

async fn exercise_changed_destination(fixture: Fixture) {
    fixture.change_binding_revision(4).await;
    assert!(matches!(
        fixture
            .service
            .export_github(
                UserId::from_uuid(fixture.reviewer),
                fixture.task,
                fixture.command(true)
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert_eq!(fixture.count("work_item_github_exports").await, 0);
}

#[tokio::test]
async fn sqlite_changed_github_destination_rejects_stale_preview() {
    exercise_changed_destination(Fixture::sqlite(false).await).await;
}

#[tokio::test]
async fn postgres_changed_github_destination_rejects_stale_preview() {
    if let Some(fixture) = Fixture::postgres(false).await {
        exercise_changed_destination(fixture).await;
    }
}

async fn exercise_projection(fixture: Fixture) {
    let heart = HeartTask {
        id: fixture.task,
        instance_id: Uuid::now_v7(),
        node_id: "publish-github".into(),
        assignee_id: fixture.reviewer,
        status: "pending".into(),
        result_metadata: None,
    };
    let projected = fixture
        .service
        .project(
            &fixture.item.to_string(),
            &fixture.lease.to_string(),
            &fixture.channel.to_string(),
            &heart,
        )
        .await
        .unwrap()
        .unwrap();
    assert!(
        fixture
            .service
            .project(
                &fixture.item.to_string(),
                &fixture.lease.to_string(),
                &fixture.channel.to_string(),
                &heart
            )
            .await
            .unwrap()
            .is_none()
    );
    let view = fixture
        .service
        .task(UserId::from_uuid(fixture.reviewer), fixture.task, projected)
        .await
        .unwrap();
    assert_eq!(view.status, "pending");
    assert_eq!(view.process_status, "waiting");
    assert_eq!(fixture.count("work_item_tasks").await, 1);
}

#[tokio::test]
async fn sqlite_github_projection_preserves_historical_case_and_deduplicates() {
    exercise_projection(Fixture::sqlite(true).await).await;
}

#[tokio::test]
async fn postgres_github_projection_preserves_historical_case_and_deduplicates() {
    if let Some(fixture) = Fixture::postgres(true).await {
        exercise_projection(fixture).await;
    }
}

async fn exercise_skip(fixture: Fixture) {
    fixture.revoke_export().await;
    let decision = fixture.command(false);
    fixture
        .service
        .export_github(UserId::from_uuid(fixture.reviewer), fixture.task, decision)
        .await
        .unwrap();
    assert_eq!(fixture.export_status().await, "skipped");
    fixture
        .service
        .deliver_exports(Utc::now().timestamp())
        .await
        .unwrap();
    assert_eq!(fixture.export_status().await, "skipped");
}

#[tokio::test]
async fn sqlite_skip_is_durable_without_export_right_or_external_post() {
    exercise_skip(Fixture::sqlite(false).await).await;
}

#[tokio::test]
async fn postgres_skip_is_durable_without_export_right_or_external_post() {
    if let Some(fixture) = Fixture::postgres(false).await {
        exercise_skip(fixture).await;
    }
}

async fn exercise_lost_github_response(mut fixture: Fixture) {
    let issue = Arc::new(Mutex::new(None::<Value>));
    let posts = Arc::new(AtomicUsize::new(0));
    let posted_issue = issue.clone();
    let post_count = posts.clone();
    let listed_issue = issue.clone();
    let mock=axum::Router::new()
        .route("/app/installations/7/access_tokens",axum::routing::post(|| async {
            axum::Json(json!({"token":"test-installation-token","repositories":[{"id":42}],"permissions":{"issues":"write"}}))
        }))
        .route("/repos/owner/repo",axum::routing::get(|| async {
            axum::Json(json!({"id":42,"full_name":"owner/repo","has_issues":true}))
        }))
        .route("/repos/owner/repo/issues",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
            let issue=posted_issue.clone(); let posts=post_count.clone(); async move {
                posts.fetch_add(1,Ordering::SeqCst);
                let result=json!({"id":123,"number":4,"html_url":"https://github.com/owner/repo/issues/4",
                    "title":body["title"],"body":body["body"],"user":{"login":"our-app[bot]"}});
                *issue.lock().await=Some(result);
                axum::http::StatusCode::INTERNAL_SERVER_ERROR
            }
        }).get(move || {
            let issue=listed_issue.clone(); async move {
                axum::Json(issue.lock().await.iter().cloned().collect::<Vec<_>>())
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.service.github = Some(crate::github::GitHub::test_client(format!(
        "http://{}",
        listener.local_addr().unwrap()
    )));
    let server = tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    fixture
        .service
        .export_github(
            UserId::from_uuid(fixture.reviewer),
            fixture.task,
            fixture.command(true),
        )
        .await
        .unwrap();
    fixture
        .service
        .deliver_exports(Utc::now().timestamp())
        .await
        .unwrap();
    assert_eq!(fixture.export_status().await, "uncertain");
    assert_eq!(posts.load(Ordering::SeqCst), 1);
    fixture.reset_export_lease().await;
    fixture
        .service
        .deliver_exports(Utc::now().timestamp())
        .await
        .unwrap();
    assert_eq!(fixture.export_status().await, "sent");
    assert_eq!(
        posts.load(Ordering::SeqCst),
        1,
        "an uncertain create must never issue another POST"
    );
    let note: Value = serde_json::from_str(&fixture.decision_note().await).unwrap();
    assert_eq!(note["github_export"], "sent");
    assert_eq!(note["issue_url"], "https://github.com/owner/repo/issues/4");
    server.abort();

    let heart_result = Arc::new(Mutex::new(None::<Value>));
    let completed = heart_result.clone();
    let listed = heart_result.clone();
    let instance = fixture.export_instance;
    let item = fixture.item;
    let reviewer = fixture.reviewer;
    let application = fixture.application;
    let channel = fixture.channel;
    let task = fixture.task;
    let heart = axum::Router::new()
        .route("/api/v2/user-tasks/{id}/complete", axum::routing::post(move |axum::Json(body): axum::Json<Value>| {
            let result = completed.clone(); async move {
                *result.lock().await = Some(body["result_metadata"].clone());
                axum::http::StatusCode::NO_CONTENT
            }
        }))
        .route("/api/v2/instances/{id}", axum::routing::get(move || async move {
            axum::Json(json!({"id":instance,"namespace":"sproyt","runtime":"v2","status":"completed",
                "input_metadata":{"work_item_id":item,"reviewer_id":reviewer,"application_id":application,"task_channel_id":channel}}))
        }))
        .route("/api/v2/user-tasks", axum::routing::get(move || {
            let result = listed.clone(); async move {
                axum::Json(json!([{"id":task,"instance_id":instance,"node_id":"publish-github",
                    "assignee_id":reviewer,"status":"completed","result_metadata":result.lock().await.clone()}]))
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.service.heart_url = Some(format!("http://{}", listener.local_addr().unwrap()));
    let heart_server = tokio::spawn(async move {
        axum::serve(listener, heart).await.unwrap();
    });
    let repository = Arc::new(
        crate::db::SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let chat = ChatEngine::start(repository);
    fixture
        .service
        .sync_export_process(&chat, &item.to_string(), &fixture.lease.to_string())
        .await
        .unwrap();
    assert_eq!(heart_result.lock().await.as_ref(), Some(&note));
    assert_eq!(
        fixture
            .service
            .task(UserId::from_uuid(reviewer), task, fixture.message)
            .await
            .unwrap()
            .status,
        "completed"
    );
    heart_server.abort();
}

#[tokio::test]
async fn sqlite_lost_github_response_reconciles_one_post_by_marker() {
    exercise_lost_github_response(Fixture::sqlite(false).await).await;
}

#[tokio::test]
async fn postgres_lost_github_response_reconciles_one_post_by_marker() {
    if let Some(fixture) = Fixture::postgres(false).await {
        exercise_lost_github_response(fixture).await;
    }
}
