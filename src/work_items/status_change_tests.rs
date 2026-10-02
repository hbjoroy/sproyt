use super::super::github_export::tests::Fixture;
use super::*;
use std::sync::Arc;
use tokio::sync::Mutex;

macro_rules! run {
    ($fixture:expr,$query:expr $(,$value:expr)* $(,)?) => {{
        match &$fixture.service.store {
            Store::Pg(pool) => {
                let query=sql($query,true); let mut request=sqlx::query(&query);
                $(request=request.bind($value);)* request.execute(pool).await.unwrap();
            }
            Store::Sqlite(pool) => {
                let query=sql($query,false); let mut request=sqlx::query(&query);
                $(request=request.bind($value);)* request.execute(pool).await.unwrap();
            }
        }
    }};
}

async fn scalar_string(fixture: &Fixture, query: &str, id: Uuid) -> String {
    match &fixture.service.store {
        Store::Pg(pool) => sqlx::query_scalar(&sql(query, true))
            .bind(id.to_string())
            .fetch_one(pool)
            .await
            .unwrap(),
        Store::Sqlite(pool) => sqlx::query_scalar(&sql(query, false))
            .bind(id.to_string())
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}

async fn status_fixture(postgres: bool) -> Option<Fixture> {
    let mut fixture = if postgres {
        Fixture::postgres(false).await?
    } else {
        Fixture::sqlite(false).await
    };
    let binding = scalar_string(
        &fixture,
        "select cast(binding_id as text) from work_items where id=?uuid",
        fixture.item,
    )
    .await;
    run!(
        fixture,
        "insert into channel_process_applications(binding_id,application_id) values(?uuid,?uuid)",
        binding.clone(),
        fixture.application.to_string()
    );
    run!(
        fixture,
        "insert into channel_task_routes(id,binding_id,channel_id,task_key,process_role,enabled) values(?uuid,?uuid,?uuid,'change-status','product-handler',true)",
        Uuid::now_v7().to_string(),
        binding,
        fixture.channel.to_string()
    );
    run!(
        fixture,
        "update work_item_tasks set node_id='review',status='completed' where id=?uuid",
        fixture.task.to_string()
    );
    // This completed source task belongs to a reviewed, planned case. Heart is
    // configured, but no network call is needed merely to accept a start.
    fixture.service.heart_url = Some("http://127.0.0.1:1".into());
    Some(fixture)
}

fn start(fixture: &Fixture) -> StatusStart {
    StatusStart {
        source_task_id: fixture.task,
        source_message_id: fixture.message,
        request_id: Uuid::now_v7(),
        expected_revision: 1,
    }
}

async fn attach_waiting_task(fixture: &Fixture, change: Uuid) -> (Uuid, Uuid) {
    let task = Uuid::now_v7();
    let message = Uuid::now_v7();
    run!(
        fixture,
        "insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?uuid,?uuid,?uuid,'Tester',3,?)",
        message.to_string(),
        fixture.channel.to_string(),
        fixture.reviewer.to_string(),
        format!("[[work-item-task:{task}]]")
    );
    run!(
        fixture,
        "update channel_sequences set next_sequence=4 where channel_id=?uuid",
        fixture.channel.to_string()
    );
    run!(
        fixture,
        "update work_item_status_changes set status='waiting',task_id=?uuid,message_id=?uuid,heart_instance_id=?uuid where id=?uuid",
        task.to_string(),
        message.to_string(),
        Uuid::now_v7().to_string(),
        change.to_string()
    );
    (task, message)
}

fn decision(message: Uuid, no_change: bool) -> StatusDecision {
    StatusDecision {
        message_id: message,
        request_id: Uuid::now_v7(),
        expected_revision: 1,
        status: if no_change {
            String::new()
        } else {
            "in_development".into()
        },
        internal_note: if no_change {
            String::new()
        } else {
            "Private implementation detail".into()
        },
        public_feedback: if no_change {
            String::new()
        } else {
            "Work has started".into()
        },
        no_change,
    }
}

async fn exercise_start_and_permissions(fixture: Fixture) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let start = start(&fixture);
    let lifecycle = fixture
        .service
        .lifecycle(reviewer.clone(), fixture.item)
        .await
        .unwrap()
        .unwrap();
    assert!(lifecycle.can_start);
    assert_eq!(
        lifecycle.allowed_statuses,
        vec!["in_development", "resolved", "rejected"]
    );
    assert!(
        !fixture
            .service
            .lifecycle(UserId::from_uuid(fixture.owner), fixture.item)
            .await
            .unwrap()
            .unwrap()
            .can_start
    );
    assert!(matches!(
        fixture
            .service
            .start_status(
                UserId::from_uuid(fixture.owner),
                fixture.item,
                start.clone()
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    let first = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start.clone())
        .await
        .unwrap();
    assert_eq!(first.start_status, "pending");
    assert_eq!(first.channel_name, "Issues");
    let repeat = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start.clone())
        .await
        .unwrap();
    assert_eq!(first.id, repeat.id);
    assert!(matches!(
        fixture
            .service
            .start_status(
                reviewer.clone(),
                fixture.item,
                StatusStart {
                    source_message_id: Uuid::now_v7(),
                    ..start.clone()
                }
            )
            .await,
        Err(RepositoryError::NotFound) | Err(RepositoryError::Conflict)
    ));
    assert!(matches!(
        fixture
            .service
            .start_status(
                reviewer.clone(),
                fixture.item,
                StatusStart {
                    request_id: Uuid::now_v7(),
                    ..start.clone()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    assert!(
        !fixture
            .service
            .lifecycle(reviewer.clone(), fixture.item)
            .await
            .unwrap()
            .unwrap()
            .can_start
    );
    run!(
        fixture,
        "update work_item_status_changes set status='completed' where id=?uuid",
        first.id.to_string()
    );
    assert!(
        fixture
            .service
            .lifecycle(reviewer.clone(), fixture.item)
            .await
            .unwrap()
            .unwrap()
            .can_start
    );
    let next = fixture
        .service
        .start_status(
            reviewer.clone(),
            fixture.item,
            StatusStart {
                request_id: Uuid::now_v7(),
                ..start.clone()
            },
        )
        .await
        .unwrap();
    assert_ne!(first.id, next.id);
    assert_eq!(
        fixture
            .service
            .start_status(reviewer, fixture.item, start)
            .await
            .unwrap()
            .id,
        first.id
    );
}

#[tokio::test]
async fn sqlite_status_start_idempotent_one_active_and_authorized() {
    exercise_start_and_permissions(status_fixture(false).await.unwrap()).await;
}

#[tokio::test]
async fn postgres_status_start_idempotent_one_active_and_authorized() {
    if let Some(fixture) = status_fixture(true).await {
        exercise_start_and_permissions(fixture).await;
    }
}

async fn exercise_status_decision_and_publication(fixture: Fixture) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let started = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start(&fixture))
        .await
        .unwrap();
    let (task, message) = attach_waiting_task(&fixture, started.id).await;
    let current = fixture
        .service
        .task(reviewer.clone(), task, message)
        .await
        .unwrap();
    assert!(current.can_decide);
    assert_eq!(current.node_id, "change-status");
    let command = decision(message, false);
    assert!(matches!(
        fixture
            .service
            .change_status(UserId::from_uuid(fixture.owner), task, command.clone())
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    run!(
        fixture,
        "update application_processors set can_review=false,can_export=false where application_id=?uuid and user_id=?uuid",
        fixture.application.to_string(),
        fixture.reviewer.to_string()
    );
    assert!(matches!(
        fixture
            .service
            .change_status(reviewer.clone(), task, command.clone())
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    run!(
        fixture,
        "update application_processors set can_review=true,can_export=true where application_id=?uuid and user_id=?uuid",
        fixture.application.to_string(),
        fixture.reviewer.to_string()
    );
    assert!(matches!(
        fixture
            .service
            .change_status(
                reviewer.clone(),
                task,
                StatusDecision {
                    expected_revision: 2,
                    ..command.clone()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    let accepted = fixture
        .service
        .change_status(reviewer.clone(), task, command.clone())
        .await
        .unwrap();
    assert_eq!(accepted.delivery_status, "pending");
    assert_eq!(
        accepted.lifecycle.as_ref().unwrap().case_status,
        "in_development"
    );
    fixture
        .service
        .change_status(reviewer.clone(), task, command.clone())
        .await
        .unwrap();
    assert!(matches!(
        fixture
            .service
            .change_status(
                reviewer.clone(),
                task,
                StatusDecision {
                    public_feedback: "Changed".into(),
                    ..command.clone()
                }
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    let persisted = scalar_string(
        &fixture,
        "select decision_result from work_item_status_changes where id=?uuid",
        started.id,
    )
    .await;
    assert!(!persisted.contains("Private implementation detail"));
    assert!(!persisted.contains("Work has started"));
    let history = fixture
        .service
        .lifecycle(reviewer.clone(), fixture.item)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(history.history.len(), 1);
    assert_eq!(
        history.history[0].internal_note.as_deref(),
        Some("Private implementation detail")
    );
    assert_eq!(
        history.history[0].public_feedback.as_deref(),
        Some("Work has started")
    );
    let requester = fixture
        .service
        .lifecycle(UserId::from_uuid(fixture.owner), fixture.item)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requester.history[0].internal_note, None);
    assert_eq!(
        requester.history[0].public_feedback.as_deref(),
        Some("Work has started")
    );
    run!(
        fixture,
        "update work_item_status_changes set lease_token=?uuid,lease_until=? where id=?uuid",
        Uuid::now_v7().to_string(),
        Utc::now().timestamp() + 90,
        started.id.to_string()
    );
    let lease = scalar_string(
        &fixture,
        "select cast(lease_token as text) from work_item_status_changes where id=?uuid",
        started.id,
    )
    .await;
    let status_message = fixture
        .service
        .project_status_message(&started.id.to_string(), &lease, None)
        .await
        .unwrap()
        .unwrap();
    let public = fixture
        .service
        .public_status(
            UserId::from_uuid(fixture.owner),
            fixture.item,
            status_message,
        )
        .await
        .unwrap();
    let wire = serde_json::to_value(public).unwrap();
    assert_eq!(wire["status"], "in_development");
    assert_eq!(wire["public_feedback"], "Work has started");
    assert!(wire.to_string().contains("Work has started"));
    assert!(!wire.to_string().contains("Private implementation detail"));
    assert!(matches!(
        fixture
            .service
            .public_status(
                UserId::from_uuid(fixture.owner),
                fixture.item,
                Uuid::now_v7()
            )
            .await,
        Err(RepositoryError::NotFound)
    ));
    run!(
        fixture,
        "insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",
        fixture.channel.to_string(),
        fixture.outsider.to_string()
    );
    let invisible = fixture
        .service
        .public_status(
            UserId::from_uuid(fixture.outsider),
            fixture.item,
            status_message,
        )
        .await
        .unwrap();
    let hidden = serde_json::to_value(invisible).unwrap();
    assert_eq!(hidden, json!({"visible":false}));
    assert!(!hidden.to_string().contains("Private implementation detail"));
    assert!(!hidden.to_string().contains("Reviewed bug"));
    assert!(
        fixture
            .service
            .lifecycle(UserId::from_uuid(fixture.outsider), fixture.item)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn sqlite_status_decision_private_history_public_projection_and_message_proof() {
    exercise_status_decision_and_publication(status_fixture(false).await.unwrap()).await;
}

#[tokio::test]
async fn postgres_status_decision_private_history_public_projection_and_message_proof() {
    if let Some(fixture) = status_fixture(true).await {
        exercise_status_decision_and_publication(fixture).await;
    }
}

async fn exercise_no_change(fixture: Fixture) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let started = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start(&fixture))
        .await
        .unwrap();
    let (task, message) = attach_waiting_task(&fixture, started.id).await;
    let command = decision(message, true);
    fixture
        .service
        .change_status(reviewer.clone(), task, command.clone())
        .await
        .unwrap();
    fixture
        .service
        .change_status(reviewer, task, command)
        .await
        .unwrap();
    let state = scalar_string(
        &fixture,
        "select status from work_items where id=?uuid",
        fixture.item,
    )
    .await;
    assert_eq!(state, "planned");
    let revision: i64 = match &fixture.service.store {
        Store::Pg(pool) => {
            sqlx::query_scalar(&sql("select revision from work_items where id=?uuid", true))
                .bind(fixture.item.to_string())
                .fetch_one(pool)
                .await
                .unwrap()
        }
        Store::Sqlite(pool) => sqlx::query_scalar(&sql(
            "select revision from work_items where id=?uuid",
            false,
        ))
        .bind(fixture.item.to_string())
        .fetch_one(pool)
        .await
        .unwrap(),
    };
    assert_eq!(revision, 1);
    assert_eq!(fixture.count("work_item_status_history").await, 0);
    let lease = Uuid::now_v7();
    run!(
        fixture,
        "update work_item_status_changes set lease_token=?uuid,lease_until=? where id=?uuid",
        lease.to_string(),
        Utc::now().timestamp() + 90,
        started.id.to_string()
    );
    assert!(
        fixture
            .service
            .project_status_message(&started.id.to_string(), &lease.to_string(), None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(fixture.count("work_item_status_publications").await, 0);
}

#[tokio::test]
async fn sqlite_status_no_change_completes_without_case_mutation() {
    exercise_no_change(status_fixture(false).await.unwrap()).await;
}

#[tokio::test]
async fn postgres_status_no_change_completes_without_case_mutation() {
    if let Some(fixture) = status_fixture(true).await {
        exercise_no_change(fixture).await;
    }
}

async fn exercise_lost_heart_completion(mut fixture: Fixture) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let started = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start(&fixture))
        .await
        .unwrap();
    let (task, message) = attach_waiting_task(&fixture, started.id).await;
    fixture
        .service
        .change_status(reviewer, task, decision(message, false))
        .await
        .unwrap();
    let instance = scalar_string(
        &fixture,
        "select cast(heart_instance_id as text) from work_item_status_changes where id=?uuid",
        started.id,
    )
    .await;
    let completed = Arc::new(Mutex::new(None::<Value>));
    let posted = completed.clone();
    let listed = completed.clone();
    let instance_status = Arc::new(Mutex::new("waiting"));
    let viewed_status = instance_status.clone();
    let item = fixture.item;
    let actor = fixture.reviewer;
    let channel = fixture.channel;
    let change = started.id;
    let instance_for_view = instance.clone();
    let instance_for_tasks = instance.clone();
    let heart=axum::Router::new()
        .route("/api/v2/instances/{id}",axum::routing::get(move || {
            let instance=instance_for_view.clone(); let status=viewed_status.clone(); async move {axum::Json(json!({
                "id":instance,"namespace":"sproyt","runtime":"v2","status":*status.lock().await,
                "input_metadata":{"status_change_id":change,"work_item_id":item,"handler_id":actor,"task_channel_id":channel}
            }))}
        }))
        .route("/api/v2/user-tasks",axum::routing::get(move || {
            let result=listed.clone(); let instance=instance_for_tasks.clone(); async move {
                let result=result.lock().await.clone();
                axum::Json(json!([{"id":task,"instance_id":instance,"node_id":"change-status",
                    "assignee_id":actor,"status":if result.is_some(){"completed"}else{"pending"},
                    "result_metadata":result}]))
            }
        }))
        .route("/api/v2/user-tasks/{id}/complete",axum::routing::post(move |axum::Json(body):axum::Json<Value>| {
            let result=posted.clone(); async move {
                *result.lock().await=Some(body["result_metadata"].clone());
                axum::http::StatusCode::BAD_GATEWAY
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.service.heart_url = Some(format!("http://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        axum::serve(listener, heart).await.unwrap();
    });
    let lease = Uuid::now_v7();
    run!(
        fixture,
        "update work_item_status_changes set lease_token=?uuid,lease_until=? where id=?uuid",
        lease.to_string(),
        Utc::now().timestamp() + 90,
        started.id.to_string()
    );
    fixture
        .service
        .project_status_message(&started.id.to_string(), &lease.to_string(), None)
        .await
        .unwrap()
        .unwrap();
    let repository = Arc::new(
        crate::db::SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let chat = ChatEngine::start(repository);
    fixture
        .service
        .sync_status_one(&chat, &started.id.to_string(), &lease.to_string())
        .await
        .unwrap();
    let result = completed.lock().await.clone().unwrap();
    assert_eq!(result["status"], "in_development");
    assert!(!result.to_string().contains("Private implementation detail"));
    assert!(!result.to_string().contains("Work has started"));
    fixture
        .service
        .sync_status_one(&chat, &started.id.to_string(), &lease.to_string())
        .await
        .unwrap();
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_item_status_changes where id=?uuid",
            started.id
        )
        .await,
        "waiting"
    );
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_items where id=?uuid",
            fixture.item
        )
        .await,
        "in_development"
    );
    *instance_status.lock().await = "completed";
    fixture
        .service
        .sync_status_one(&chat, &started.id.to_string(), &lease.to_string())
        .await
        .unwrap();
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_item_status_changes where id=?uuid",
            started.id
        )
        .await,
        "completed"
    );
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_items where id=?uuid",
            fixture.item
        )
        .await,
        "in_development"
    );
    assert_eq!(fixture.count("work_item_status_history").await, 1);
    assert_eq!(fixture.count("work_item_status_publications").await, 1);
    server.abort();
}

#[tokio::test]
async fn sqlite_status_lost_heart_response_reconciles_without_note_leak_or_regression() {
    exercise_lost_heart_completion(status_fixture(false).await.unwrap()).await;
}

#[tokio::test]
async fn postgres_status_lost_heart_response_reconciles_without_note_leak_or_regression() {
    if let Some(fixture) = status_fixture(true).await {
        exercise_lost_heart_completion(fixture).await;
    }
}

async fn exercise_terminal_heart(mut fixture: Fixture, terminal: &'static str) {
    let reviewer = UserId::from_uuid(fixture.reviewer);
    let started = fixture
        .service
        .start_status(reviewer.clone(), fixture.item, start(&fixture))
        .await
        .unwrap();
    let (task, message) = attach_waiting_task(&fixture, started.id).await;
    fixture
        .service
        .change_status(reviewer, task, decision(message, false))
        .await
        .unwrap();
    let instance = scalar_string(
        &fixture,
        "select cast(heart_instance_id as text) from work_item_status_changes where id=?uuid",
        started.id,
    )
    .await;
    let item = fixture.item;
    let actor = fixture.reviewer;
    let channel = fixture.channel;
    let change = started.id;
    let instance_for_view = instance.clone();
    let instance_for_tasks = instance.clone();
    let heart=axum::Router::new()
        .route("/api/v2/instances/{id}",axum::routing::get(move || {
            let instance=instance_for_view.clone(); async move {axum::Json(json!({
                "id":instance,"namespace":"sproyt","runtime":"v2","status":terminal,
                "input_metadata":{"status_change_id":change,"work_item_id":item,"handler_id":actor,"task_channel_id":channel}
            }))}
        }))
        .route("/api/v2/user-tasks",axum::routing::get(move || {
            let instance=instance_for_tasks.clone(); async move {axum::Json(json!([{
                "id":task,"instance_id":instance,"node_id":"change-status","assignee_id":actor,
                "status":"cancelled","result_metadata":null
            }]))}
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    fixture.service.heart_url = Some(format!("http://{}", listener.local_addr().unwrap()));
    let server = tokio::spawn(async move {
        axum::serve(listener, heart).await.unwrap();
    });
    let lease = Uuid::now_v7();
    run!(
        fixture,
        "update work_item_status_changes set lease_token=?uuid,lease_until=? where id=?uuid",
        lease.to_string(),
        Utc::now().timestamp() + 90,
        started.id.to_string()
    );
    fixture
        .service
        .project_status_message(&started.id.to_string(), &lease.to_string(), None)
        .await
        .unwrap()
        .unwrap();
    let repository = Arc::new(
        crate::db::SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap(),
    );
    repository.migrate().await.unwrap();
    let chat = ChatEngine::start(repository);
    fixture
        .service
        .sync_status_one(&chat, &started.id.to_string(), &lease.to_string())
        .await
        .unwrap();
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_item_status_changes where id=?uuid",
            started.id
        )
        .await,
        terminal
    );
    assert_eq!(
        scalar_string(
            &fixture,
            "select status from work_items where id=?uuid",
            fixture.item
        )
        .await,
        "in_development"
    );
    assert_eq!(fixture.count("work_item_status_history").await, 1);
    assert_eq!(fixture.count("work_item_status_publications").await, 1);
    server.abort();
}

#[tokio::test]
async fn sqlite_failed_heart_closes_operation_without_reverting_accepted_case_status() {
    exercise_terminal_heart(status_fixture(false).await.unwrap(), "failed").await;
}

#[tokio::test]
async fn sqlite_cancelled_heart_closes_operation_without_reverting_accepted_case_status() {
    exercise_terminal_heart(status_fixture(false).await.unwrap(), "cancelled").await;
}

#[tokio::test]
async fn postgres_failed_heart_closes_operation_without_reverting_accepted_case_status() {
    if let Some(fixture) = status_fixture(true).await {
        exercise_terminal_heart(fixture, "failed").await;
    }
}
