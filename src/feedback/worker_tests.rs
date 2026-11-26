use super::*;
use crate::domain::InMemoryChatRepository;
use crate::github::GitHub;
use crate::work_items::feedback_channel::tests::seed_feedback_route;
use crate::work_items::github_export::tests::Fixture;
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use std::sync::{Arc, Mutex};

const GREETING: &str = "God morgon!";
const REQUEST: &str = "Eg må kunne finne vedlegga igjen i tråden.";
const EXTRA: &str = "Det gjeld også PDF-filer.";
const REPEAT: &str = "Eg tenkjer framleis på PDF-filene.";
const UNCLEAR: &str = "Same behov for vedlegg, kanskje ei endring av den aktive saka.";
const NEED: &str = "Som deltakar vil eg finne vedlegga i tråden for å kunne lese dei om att.";
const DELTA: &str = "Attfinning av vedlegg må også omfatte PDF-formatet.";
const ISSUE_URL: &str = "https://github.com/owner/repo/issues/4";

#[derive(Default)]
struct Remote {
    issue: Option<Value>,
    comments: Vec<Value>,
    creations: usize,
    list_reads: usize,
    model_calls: usize,
}
type Shared = Arc<Mutex<Remote>>;

async fn model(State(remote): State<Shared>, Json(request): Json<Value>) -> Json<Value> {
    assert_eq!(request["model"], "santorini-test");
    let input: Value =
        serde_json::from_str(request["messages"][1]["content"].as_str().unwrap()).unwrap();
    let message = input["message"].as_str().unwrap();
    let answer = if let Some(issues) = input["issues"].as_array() {
        if message == REQUEST {
            assert!(issues.is_empty());
            json!({"action":"new","title":"Finn att vedlegg i tråden","need":NEED,
                "criteria":["Eit tidlegare vedlegg kan opnast frå tråden."]})
        } else {
            assert_eq!(issues.len(), 1);
            assert_eq!(issues[0]["number"], 4);
            assert_eq!(issues[0]["eligible"], message != UNCLEAR);
            json!({"action":"match","number":4})
        }
    } else if let Some(comments) = input["comments"].as_array() {
        assert_eq!(input["issue"]["number"], 4);
        match message {
            EXTRA => {
                assert!(comments.is_empty());
                json!({"action":"comment","body":DELTA})
            }
            REPEAT => {
                assert_eq!(comments.len(), 1);
                assert!(comments[0].as_str().unwrap().contains(DELTA));
                json!({"action":"duplicate"})
            }
            _ => panic!("unexpected delta generation for {message}"),
        }
    } else {
        json!({"qualifies":message != GREETING})
    };
    remote.lock().unwrap().model_calls += 1;
    Json(json!({"choices":[{"message":{"content":answer.to_string()}}]}))
}

async fn create_issue(State(remote): State<Shared>, Json(payload): Json<Value>) -> StatusCode {
    assert_eq!(payload["status"], "Backlog");
    let mut remote = remote.lock().unwrap();
    remote.creations += 1;
    remote.issue = Some(
        json!({"id":700,"number":4,"html_url":ISSUE_URL,"title":payload["title"],
        "body":payload["body"],"user":{"login":"our-app[bot]"},"state":"open","assignees":[],
        "status":{"name":"Backlog","category":"backlog"}}),
    );
    // The App committed the write before the gateway lost its response.
    StatusCode::BAD_GATEWAY
}

async fn add_comment(State(remote): State<Shared>, Json(payload): Json<Value>) -> StatusCode {
    let mut remote = remote.lock().unwrap();
    remote.comments.push(
        json!({"id":899,"html_url":format!("{ISSUE_URL}#issuecomment-899"),
        "body":payload["body"],"user":{"login":"our-app[bot]"}}),
    );
    StatusCode::BAD_GATEWAY
}

fn routes(remote: Shared) -> Router {
    Router::new()
        .route("/models", get(|| async { Json(json!({"data":[{"id":"santorini-test"}]})) }))
        .route("/chat/completions", post(model))
        .route("/app/installations/7/access_tokens", post(|| async {
            Json(json!({"token":"mock-token","repositories":[{"id":42}],"permissions":{"issues":"write"}}))
        }))
        .route("/repos/owner/repo", get(|| async {
            Json(json!({"id":42,"full_name":"owner/repo","has_issues":true}))
        }))
        .route("/repos/owner/repo/issues", post(create_issue).get(|State(state):State<Shared>| async move {
            let mut remote = state.lock().unwrap();
            remote.list_reads += 1;
            Json(remote.issue.iter().cloned().collect::<Vec<_>>())
        }))
        .route("/repos/owner/repo/issues/4", get(|State(state):State<Shared>| async move {
            Json(state.lock().unwrap().issue.clone().unwrap())
        }))
        .route("/repos/owner/repo/issues/4/comments", post(add_comment).get(|State(state):State<Shared>| async move {
            Json(state.lock().unwrap().comments.clone())
        }))
        .with_state(remote)
}

struct Harness {
    f: Fixture,
    pool: SqlitePool,
    agents: CircleChatAgents,
    chat: ChatEngine,
}
impl Harness {
    async fn source(&self, body: &str) -> Uuid {
        let source = Uuid::now_v7();
        let sequence:i64 = sqlx::query_scalar("update channel_sequences set next_sequence=next_sequence+1 where channel_id=? returning next_sequence-1")
            .bind(self.f.channel.to_string()).fetch_one(&self.pool).await.unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,'Tester',?,?)")
            .bind(source.to_string()).bind(self.f.channel.to_string()).bind(self.f.owner.to_string())
            .bind(sequence).bind(body).execute(&self.pool).await.unwrap();
        source
    }

    async fn tick(&self, source: Uuid, expected: &str) {
        sqlx::query("update feedback_jobs set available_at=0 where source_message_id=?")
            .bind(source.to_string())
            .execute(&self.pool)
            .await
            .unwrap();
        self.f
            .service
            .feedback_tick(&self.agents, &self.chat)
            .await
            .unwrap();
        let status: String =
            sqlx::query_scalar("select status from feedback_jobs where source_message_id=?")
                .bind(source.to_string())
                .fetch_one(&self.pool)
                .await
                .unwrap();
        assert_eq!(status, expected);
    }

    async fn linked(&self, source: Uuid, kind: &str) {
        let projection = self
            .f
            .service
            .feedback_issue(UserId::from_uuid(self.f.reviewer), self.f.channel, source)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(projection.issue_url, ISSUE_URL);
        assert_eq!(projection.publication_kind, kind);
        let (replies, reactions): (i64, i64) = sqlx::query_as(
            "select
            (select count(*) from messages where parent_message_id=? and body like ?),
            (select count(*) from message_reactions where message_id=?)",
        )
        .bind(source.to_string())
        .bind(format!("%{ISSUE_URL}"))
        .bind(source.to_string())
        .fetch_one(&self.pool)
        .await
        .unwrap();
        assert_eq!((replies, reactions), (1, 1));
    }
}

#[tokio::test]
async fn sqlite_feedback_worker_qualifies_links_reconciles_and_observes_closure() {
    let remote = Shared::default();
    let mock = routes(remote.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let mut f = Fixture::sqlite(false).await;
    seed_feedback_route(&f).await;
    let Store::Sqlite(pool) = &f.service.store else {
        unreachable!()
    };
    let pool = pool.clone();
    sqlx::query("update channels set circle_id=null,kind='public' where id=?")
        .bind(f.channel.to_string())
        .execute(&pool)
        .await
        .unwrap();
    f.service.github = Some(GitHub::test_client(base.clone()));
    let agents = CircleChatAgents::feedback_test_client(pool.clone(), base);
    let chat = ChatEngine::start(Arc::new(InMemoryChatRepository::default()));
    let h = Harness {
        f,
        pool,
        agents,
        chat,
    };

    let greeting = h.source(GREETING).await;
    h.tick(greeting, "ignored").await;
    assert_eq!(
        remote.lock().unwrap().list_reads,
        0,
        "chatter never triggers a GitHub issue list"
    );

    let original = h.source(REQUEST).await;
    h.tick(original, "uncertain").await;
    h.tick(original, "published").await;
    h.tick(original, "done").await;
    h.linked(original, "create").await;
    {
        let state = remote.lock().unwrap();
        let body = state.issue.as_ref().unwrap()["body"].as_str().unwrap();
        assert!(body.contains(NEED) && body.contains("### Akseptansekriterium"));
        assert!(body.ends_with(&format!("<!-- sproyt-feedback:{original} -->")));
        assert_eq!(state.creations, 1);
    }
    let extra = h.source(EXTRA).await;
    // Select an old message while keeping the automatic channel fence in place.
    sqlx::query("update feedback_routes set after_sequence=(select sequence from messages where id=?) where channel_id=?")
        .bind(extra.to_string()).bind(h.f.channel.to_string()).execute(&h.pool).await.unwrap();
    let receipt =
        h.f.service
            .feedback_activation(&UserId::from_uuid(h.f.reviewer), h.f.channel, extra, true)
            .await
            .unwrap();
    assert_eq!(receipt.as_deref(), Some("pending"));
    // A selected historical message waits for a newer decision already in flight.
    let newer = h.source("Ei ny melding i køen").await;
    h.f.service.admit_feedback().await.unwrap();
    sqlx::query("update feedback_jobs set lease_token=?,available_at=? where source_message_id=?")
        .bind(Uuid::now_v7().to_string())
        .bind(Utc::now().timestamp() + 600)
        .bind(newer.to_string())
        .execute(&h.pool)
        .await
        .unwrap();
    h.tick(extra, "pending").await;
    sqlx::query(
        "update feedback_jobs set status='ignored',lease_token=null where source_message_id=?",
    )
    .bind(newer.to_string())
    .execute(&h.pool)
    .await
    .unwrap();
    h.tick(extra, "uncertain").await;
    h.tick(extra, "published").await;
    h.tick(extra, "done").await;
    h.linked(extra, "comment").await;
    let repeat = h.source(REPEAT).await;
    h.tick(repeat, "published").await;
    h.tick(repeat, "done").await;
    h.linked(repeat, "duplicate").await;
    {
        let mut state = remote.lock().unwrap();
        assert_eq!(
            state.comments.len(),
            1,
            "lost response and repetition cannot post another comment"
        );
        assert_eq!(
            state.comments[0]["body"],
            format!("{DELTA}\n\n<!-- sproyt-feedback:{extra} -->")
        );
        state.issue.as_mut().unwrap()["status"] =
            json!({"name":"In progress","category":"started"});
    }
    let unclear = h.source(UNCLEAR).await;
    h.tick(unclear, "needs_review").await;
    {
        let mut state = remote.lock().unwrap();
        assert_eq!(state.creations, 1);
        assert_eq!(state.comments.len(), 1);
        assert_eq!(state.model_calls, 11);
        let issue = state.issue.as_mut().unwrap();
        issue["state"] = json!("closed");
        issue["state_reason"] = json!("not_planned");
    }
    sqlx::query("update feedback_jobs set available_at=9999999999 where issue_id=700")
        .execute(&h.pool)
        .await
        .unwrap();
    h.f.service.wake_feedback_issue(7, 999, 700).await.unwrap();
    let due: i64 =
        sqlx::query_scalar("select available_at from feedback_jobs where source_message_id=?")
            .bind(original.to_string())
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(due, 9999999999, "another repository cannot wake this job");
    h.f.service.wake_feedback_issue(7, 42, 700).await.unwrap();
    h.f.service.feedback_tick(&h.agents, &h.chat).await.unwrap();
    let observation:(String,String,bool)=sqlx::query_as("select github_state,github_state_reason,github_proposed from feedback_jobs where source_message_id=?")
        .bind(original.to_string()).fetch_one(&h.pool).await.unwrap();
    assert_eq!(observation, ("closed".into(), "not_planned".into(), false));
    server.abort();
}
