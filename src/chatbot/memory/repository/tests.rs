use super::*;
use crate::chatbot::{CircleChatAgents, Store, context_message_json};
use crate::domain::UserId;
use serde_json::json;

struct Fixture {
    service: CircleChatAgents,
    owner: UserId,
    member: UserId,
    circle: String,
    agent: String,
    channel: String,
    private: String,
    profile: String,
    source_a: String,
    source_b: String,
}

impl Fixture {
    async fn create(store: Store) -> Self {
        let owner = UserId::from_uuid(Uuid::now_v7());
        let member = UserId::from_uuid(Uuid::now_v7());
        let circle = Uuid::now_v7().to_string();
        let agent = Uuid::now_v7().to_string();
        let channel = Uuid::now_v7().to_string();
        let private = Uuid::now_v7().to_string();
        for (id, kind) in [
            (owner.to_string(), "human"),
            (member.to_string(), "human"),
            (agent.clone(), "agent"),
        ] {
            store
                .execute(
                    "insert into users(id,kind,display_name) values(?uuid,?,'Same name')",
                    &[id, kind.into()],
                )
                .await
                .unwrap();
        }
        store
            .execute(
                "insert into circles(id,slug,name,created_by) values(?uuid,?,'Memory test',?uuid)",
                &[circle.clone(), circle.clone(), owner.to_string()],
            )
            .await
            .unwrap();
        for (id, role) in [
            (owner.to_string(), "owner"),
            (member.to_string(), "moderator"),
        ] {
            store
                .execute(
                    "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,?)",
                    &[circle.clone(), id, role.into()],
                )
                .await
                .unwrap();
        }
        for (id, kind) in [(&channel, "public"), (&private, "private")] {
            store.execute("insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Memory',?,?uuid,?uuid)",&[id.clone(),id.clone(),kind.into(),circle.clone(),owner.to_string()]).await.unwrap();
            store.execute("insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",&[id.clone(),owner.to_string()]).await.unwrap();
        }
        store.execute("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?uuid,?uuid,?uuid,'memory-test',?,'Test',10,current_timestamp)",&[agent.clone(),owner.to_string(),owner.to_string(),agent.clone()]).await.unwrap();
        store.execute("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,memory_enabled,created_by,updated_by,created_at,updated_at) values(?uuid,?uuid,'[\"hi\"]','[\"warm\"]',false,true,?uuid,?uuid,1,1)",&[agent.clone(),circle.clone(),owner.to_string(),owner.to_string()]).await.unwrap();
        let service = CircleChatAgents {
            store,
            model: None,
            worker_enabled: false,
            weather: None,
            ferry: None,
            observations: None,
            imagegen: None,
        };
        let view = service
            .mutate_memory(
                &owner,
                &circle,
                &agent,
                Mutation::Choice(ChoiceInput {
                    revision: 0,
                    enabled: true,
                }),
            )
            .await
            .unwrap();
        assert!(view.enabled && view.agent_enabled);
        assert!(!view.collection_available);
        assert!(view.collection_started_at.is_none());
        let profile=service.store.values("select cast(id as text) from agent_memory_profiles where user_id=?uuid and agent_id=?uuid",&[owner.to_string(),agent.clone()]).await.unwrap().remove(0);
        for id in [&channel, &private] {
            service.store.execute("insert into agent_memory_scopes(profile_id,circle_id,channel_id,start_sequence,processed_sequence,dirty_sequence,available_at) values(?uuid,?uuid,?uuid,0,0,0,0)",&[profile.clone(),circle.clone(),id.clone()]).await.unwrap();
        }
        let source_a = Uuid::now_v7().to_string();
        let source_b = Uuid::now_v7().to_string();
        for (id, user, sequence) in [(&source_a, &owner, 1), (&source_b, &member, 2)] {
            service.store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Same name',?int,'We discussed Greek lessons',current_timestamp)",&[id.clone(),channel.clone(),user.to_string(),sequence.to_string()]).await.unwrap();
        }
        Self {
            service,
            owner,
            member,
            circle,
            agent,
            channel,
            private,
            profile,
            source_a,
            source_b,
        }
    }

    async fn note(&self, channel: &str, sources: &[String], text: &str) -> Uuid {
        let id = Uuid::now_v7();
        let cast = if matches!(self.service.store, Store::Pg(_)) {
            "jsonb"
        } else {
            "text"
        };
        self.service.store.execute(&format!("insert into agent_memory_notes(id,profile_id,channel_id,kind,content,origin,evidence,created_at,updated_at) values(?uuid,?uuid,?uuid,'interaction',cast(? as {cast}),'automatic','conversation_event',1,1)"),&[id.to_string(),self.profile.clone(),channel.into(),serde_json::to_string(&json!({"text":text,"participant_ids":[]})).unwrap()]).await.unwrap();
        for source in sources {
            let pg = matches!(self.service.store, Store::Pg(_));
            let object = context_message_json("m", pg);
            let query = format!(
                "select cast({object} as text) from messages m join channels c on c.id=m.channel_id where m.id=?uuid"
            );
            let raw = self
                .service
                .store
                .values(&query, std::slice::from_ref(source))
                .await
                .unwrap()
                .remove(0);
            let mut message: crate::chatbot::ContextMessage = serde_json::from_str(&raw).unwrap();
            message.seal_source().unwrap();
            let version = String::from(message.source.unwrap().version.unwrap());
            self.service.store.execute("insert into agent_memory_note_sources(note_id,message_id,source_version) values(?uuid,?uuid,?)",&[id.to_string(),source.clone(),version]).await.unwrap();
        }
        id
    }

    async fn view(&self) -> MemoryView {
        self.service
            .read_memory(&self.owner, &self.circle, &self.agent)
            .await
            .unwrap()
    }

    async fn action(
        &self,
        revision: i64,
        action: MemoryAction,
    ) -> Result<MemoryView, RepositoryError> {
        self.service
            .mutate_memory(
                &self.owner,
                &self.circle,
                &self.agent,
                Mutation::Action(ActionInput { revision, action }),
            )
            .await
    }

    async fn export(&self) -> Result<Vec<serde_json::Value>, RepositoryError> {
        macro_rules! snapshot {
            ($pool:expr,$pg:expr) => {{
                let mut tx = $pool.begin().await.map_err(crate::chatbot::storage)?;
                if $pg {
                    sqlx::query("set transaction isolation level repeatable read read only")
                        .execute(&mut *tx)
                        .await
                        .map_err(crate::chatbot::storage)?;
                }
                let output = export_memory!(tx, $pg, self.owner);
                tx.commit().await.map_err(crate::chatbot::storage)?;
                Ok(output)
            }};
        }
        match &self.service.store {
            Store::Pg(pool) => snapshot!(pool, true),
            Store::Sqlite(pool) => snapshot!(pool, false),
        }
    }
}

async fn contracts(store: Store) {
    // Ordinary members discover names and IDs, without configuration or memory.
    let discovery = Fixture::create(store.clone()).await;
    discovery
        .service
        .store
        .execute(
            "update circle_memberships set role='member' where circle_id=?uuid and user_id=?uuid",
            &[discovery.circle.clone(), discovery.member.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        discovery
            .service
            .memory_agents(&discovery.member, &discovery.circle)
            .await
            .unwrap(),
        vec![json!({"agent_id": discovery.agent, "display_name": "Same name"})]
    );
    assert!(matches!(
        discovery
            .service
            .memory_agents(&UserId::from_uuid(Uuid::now_v7()), &discovery.circle)
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(matches!(
        discovery
            .service
            .memory_agents(
                &UserId::new(discovery.agent.clone()).unwrap(),
                &discovery.circle
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    let f = Fixture::create(store).await;
    let note = f
        .note(
            &f.channel,
            &[f.source_a.clone(), f.source_b.clone()],
            "Discussed Greek lessons together",
        )
        .await;
    let view = f.view().await;
    assert_eq!(view.notes.len(), 1);
    assert_eq!(view.notes[0].source_message_ids.len(), 2);
    let expiry = (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    let query = if matches!(f.service.store, Store::Pg(_)) {
        "update agent_profiles set expires_at=cast(? as timestamptz) where agent_id=?uuid"
    } else {
        "update agent_profiles set expires_at=? where agent_id=?uuid"
    };
    f.service
        .store
        .execute(query, &[expiry, f.agent.clone()])
        .await
        .unwrap();
    assert!(f.view().await.notes.is_empty());
    f.service
        .store
        .execute(
            "update agent_profiles set expires_at=null where agent_id=?uuid",
            std::slice::from_ref(&f.agent),
        )
        .await
        .unwrap();
    let exported: MemoryView = serde_json::from_value(f.export().await.unwrap().remove(0)).unwrap();
    assert_eq!(exported.notes[0].id, note);
    // Circle moderators do not gain read or write access to another's profile.
    let other = f
        .service
        .read_memory(&f.member, &f.circle, &f.agent)
        .await
        .unwrap();
    assert!(other.notes.is_empty());
    assert_eq!(other.revision, 0);
    assert!(matches!(
        f.service
            .mutate_memory(
                &f.member,
                &f.circle,
                &f.agent,
                Mutation::Action(ActionInput {
                    revision: 0,
                    action: MemoryAction::Forget { note_id: note }
                })
            )
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    let agent = UserId::new(f.agent.clone()).unwrap();
    assert!(matches!(
        f.service.read_memory(&agent, &f.circle, &f.agent).await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(matches!(
        f.service
            .read_memory(&f.owner, &Uuid::now_v7().to_string(), &f.agent)
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
    assert!(matches!(
        f.action(0, MemoryAction::Confirm { note_id: note }).await,
        Err(RepositoryError::Conflict)
    ));
    let corrected = f
        .action(
            view.revision,
            MemoryAction::Correct {
                note_id: note,
                text: NoteText::try_from("I enjoy Greek lessons".to_owned()).unwrap(),
            },
        )
        .await
        .unwrap();
    assert_eq!(corrected.revision, view.revision + 1);
    assert_eq!(corrected.memory_epoch, view.memory_epoch + 1);
    assert_eq!(corrected.notes[0].origin, NoteOrigin::User);
    assert_eq!(corrected.notes[0].evidence, EvidenceKind::UserConfirmed);
    assert_eq!(corrected.notes[0].source_message_ids.len(), 2);
    let confirmed = f
        .action(corrected.revision, MemoryAction::Confirm { note_id: note })
        .await
        .unwrap();
    assert_eq!(confirmed.notes[0].revision, 3);
    // Two commands using the same revision: exactly one may commit.
    let (a, b) = tokio::join!(
        f.action(confirmed.revision, MemoryAction::Confirm { note_id: note }),
        f.action(confirmed.revision, MemoryAction::Confirm { note_id: note })
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(
        matches!(a, Err(RepositoryError::Conflict)) || matches!(b, Err(RepositoryError::Conflict))
    );
    // Editing a source invalidates even a user-confirmed derived note.
    f.service
        .store
        .execute(
            "update messages set body='Edited',edited_at=current_timestamp where id=?uuid",
            std::slice::from_ref(&f.source_b),
        )
        .await
        .unwrap();
    let hidden = f.view().await;
    assert!(hidden.notes.is_empty());
    assert_eq!(hidden.unavailable_notes, 1);
    assert!(
        f.export().await.unwrap()[0]["notes"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(matches!(
        f.action(
            hidden.revision,
            MemoryAction::Correct {
                note_id: note,
                text: NoteText::try_from("Forged".to_owned()).unwrap()
            }
        )
        .await,
        Err(RepositoryError::PermissionDenied)
    ));
    // Forget still works when the source/note is no longer readable.
    let forgotten = f
        .action(hidden.revision, MemoryAction::Forget { note_id: note })
        .await
        .unwrap();
    assert_eq!(forgotten.unavailable_notes, 0);
    let excluded = f
        .service
        .store
        .values(
            "select cast(count(*) as text) from agent_memory_exclusions where profile_id=?uuid",
            std::slice::from_ref(&f.profile),
        )
        .await
        .unwrap();
    assert_eq!(excluded, ["2"]);
    assert_eq!(
        f.service
            .store
            .values(
                "select body from messages where id=?uuid",
                std::slice::from_ref(&f.source_a)
            )
            .await
            .unwrap(),
        ["We discussed Greek lessons"]
    );

    // Private channel requires both human membership and explicit agent access.
    let private_message = Uuid::now_v7().to_string();
    f.service.store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Same name',1,'Private statement',current_timestamp)",&[private_message.clone(),f.private.clone(),f.owner.to_string()]).await.unwrap();
    let private_note = f
        .note(&f.private, &[private_message], "Private preference")
        .await;
    assert!(f.view().await.notes.is_empty());
    f.service.store.execute("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,1)",&[f.private.clone(),f.agent.clone(),f.owner.to_string()]).await.unwrap();
    assert_eq!(f.view().await.notes[0].id, private_note);
    f.service
        .store
        .execute(
            "delete from channel_memberships where user_id=?uuid and channel_id=?uuid",
            &[f.owner.to_string(), f.private.clone()],
        )
        .await
        .unwrap();
    let hidden = f.view().await;
    assert_eq!(hidden.unavailable_notes, 1);
    assert!(hidden.notes.is_empty());
    // Reset is allowed for hidden scopes and advances every source boundary.
    let reset = f
        .action(hidden.revision, MemoryAction::Reset)
        .await
        .unwrap();
    assert_eq!(reset.unavailable_notes, 0);
    assert_eq!(reset.memory_epoch, hidden.memory_epoch + 1);
    let floor=f.service.store.values("select cast(start_sequence as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid",&[f.profile.clone(),f.channel.clone()]).await.unwrap();
    assert_eq!(floor, ["2"]);
    assert_eq!(
        f.service
            .store
            .values(
                "select cast(count(*) as text) from agent_memory_exclusions where profile_id=?uuid",
                std::slice::from_ref(&f.profile)
            )
            .await
            .unwrap(),
        ["0"]
    );
    let disabled = f
        .service
        .mutate_memory(
            &f.owner,
            &f.circle,
            &f.agent,
            Mutation::Choice(ChoiceInput {
                revision: reset.revision,
                enabled: false,
            }),
        )
        .await
        .unwrap();
    assert!(!disabled.enabled);
    assert_eq!(disabled.memory_epoch, reset.memory_epoch + 1);
    // Membership FK makes own profile/scopes/notes disappear atomically.
    f.service
        .store
        .execute(
            "delete from circle_memberships where circle_id=?uuid and user_id=?uuid",
            &[f.circle.clone(), f.owner.to_string()],
        )
        .await
        .unwrap();
    assert_eq!(
        f.service
            .store
            .values(
                "select cast(count(*) as text) from agent_memory_profiles where id=?uuid",
                &[f.profile]
            )
            .await
            .unwrap(),
        ["0"]
    );
}

async fn exclusion_and_budget_contract(store: Store) {
    let f = Fixture::create(store).await;
    let first = f
        .note(&f.channel, std::slice::from_ref(&f.source_a), "Forget this")
        .await;
    let sibling = f
        .note(
            &f.channel,
            &[f.source_a.clone(), f.source_b.clone()],
            "Shares the forgotten source",
        )
        .await;
    let other = f
        .note(&f.channel, std::slice::from_ref(&f.source_b), "Unrelated")
        .await;
    // An unrelated note about only another user is not exposed as own memory.
    assert!(!f.view().await.notes.iter().any(|note| note.id == other));
    for _ in 0..super::super::MAX_PROFILE_EXCLUSIONS {
        f.service
            .store
            .execute(
                "insert into agent_memory_exclusions(profile_id,message_id) values(?uuid,?uuid)",
                &[f.profile.clone(), Uuid::now_v7().to_string()],
            )
            .await
            .unwrap();
    }
    let view = f.view().await;
    let result = f
        .action(view.revision, MemoryAction::Forget { note_id: first })
        .await
        .unwrap();
    assert_eq!(result.history_compactions, 1);
    assert_eq!(
        f.service
            .store
            .values(
                "select cast(count(*) as text) from agent_memory_exclusions where profile_id=?uuid",
                std::slice::from_ref(&f.profile)
            )
            .await
            .unwrap(),
        ["0"]
    );
    assert_eq!(
        f.service
            .store
            .values(
                "select cast(id as text) from agent_memory_notes where profile_id=?uuid",
                std::slice::from_ref(&f.profile)
            )
            .await
            .unwrap(),
        [other.to_string()]
    );
    assert!(!result.notes.iter().any(|note| note.id == sibling));
    assert_eq!(f.service.store.values("select cast(start_sequence as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid",&[f.profile.clone(),f.channel.clone()]).await.unwrap(),["2"]);
    // Aggregate text budget counts hidden notes as well as the editable one.
    let editable = f
        .note(&f.channel, std::slice::from_ref(&f.source_a), "Small")
        .await;
    for _ in 0..15 {
        f.note(
            &f.channel,
            std::slice::from_ref(&f.source_b),
            &"x".repeat(1024),
        )
        .await;
    }
    let view = f.view().await;
    let before = view.memory_epoch;
    let too_large = f
        .action(
            view.revision,
            MemoryAction::Correct {
                note_id: editable,
                text: NoteText::try_from("x".repeat(1024)).unwrap(),
            },
        )
        .await;
    assert!(matches!(too_large, Err(RepositoryError::Conflict)));
    assert_eq!(f.view().await.memory_epoch, before);
    // No source disappears silently when the original message is hard-deleted.
    f.service
        .store
        .execute(
            "delete from messages where id=?uuid",
            std::slice::from_ref(&f.source_a),
        )
        .await
        .unwrap();
    assert!(f.view().await.notes.is_empty());
}

#[tokio::test]
async fn sqlite_exclusion_compaction_and_aggregate_budget_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    exclusion_and_budget_contract(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_exclusion_compaction_and_aggregate_budget_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    exclusion_and_budget_contract(Store::Pg(pool)).await;
}

#[tokio::test]
async fn postgres_representative_relational_footprint() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let f = Fixture::create(Store::Pg(pool.clone())).await;
    let before: String = sqlx::query_scalar("select pg_current_wal_insert_lsn()::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    for _ in 2..64 {
        let channel = Uuid::now_v7().to_string();
        f.service.store.execute("insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Size','public',?uuid,?uuid)",&[channel.clone(),channel.clone(),f.circle.clone(),f.owner.to_string()]).await.unwrap();
        f.service.store.execute("insert into agent_memory_scopes(profile_id,circle_id,channel_id,start_sequence,processed_sequence,dirty_sequence,available_at) values(?uuid,?uuid,?uuid,0,0,0,0)",&[f.profile.clone(),f.circle.clone(),channel]).await.unwrap();
    }
    for _ in 0..24 {
        let note = f
            .note(
                &f.channel,
                std::slice::from_ref(&f.source_a),
                &"x".repeat(682),
            )
            .await;
        for _ in 1..30 {
            f.service.store.execute("insert into agent_memory_note_sources(note_id,message_id,source_version) values(?uuid,?uuid,?)",&[note.to_string(),Uuid::now_v7().to_string(),"a".repeat(64)]).await.unwrap();
        }
        for id in [&f.owner, &f.member] {
            f.service.store.execute("insert into agent_memory_note_participants(note_id,user_id) values(?uuid,?uuid)",&[note.to_string(),id.to_string()]).await.unwrap();
        }
    }
    for _ in 0..1024 {
        f.service
            .store
            .execute(
                "insert into agent_memory_exclusions(profile_id,message_id) values(?uuid,?uuid)",
                &[f.profile.clone(), Uuid::now_v7().to_string()],
            )
            .await
            .unwrap();
    }
    let mut row_bytes = 0_i64;
    for table in [
        "agent_memory_profiles",
        "agent_memory_scopes",
        "agent_memory_notes",
        "agent_memory_note_sources",
        "agent_memory_note_participants",
        "agent_memory_exclusions",
    ] {
        let condition = match table {
            "agent_memory_profiles" => "id=$1::uuid",
            "agent_memory_note_sources" | "agent_memory_note_participants" => {
                "note_id in (select id from agent_memory_notes where profile_id=$1::uuid)"
            }
            _ => "profile_id=$1::uuid",
        };
        let query = format!(
            "select coalesce(sum(pg_column_size(t)),0)::bigint from {table} t where {condition}"
        );
        let bytes: i64 = sqlx::query_scalar(&query)
            .bind(&f.profile)
            .fetch_one(&pool)
            .await
            .unwrap();
        row_bytes += bytes;
        let indices: i64 = sqlx::query_scalar("select pg_indexes_size($1::regclass)::bigint")
            .bind(table)
            .fetch_one(&pool)
            .await
            .unwrap();
        println!(
            "memory footprint {table}: {bytes} tuple bytes for representative profile; {indices} index bytes for entire CI table"
        );
    }
    let wal: i64 = sqlx::query_scalar(
        "select pg_wal_lsn_diff(pg_current_wal_insert_lsn(),$1::pg_lsn)::bigint",
    )
    .bind(before)
    .fetch_one(&pool)
    .await
    .unwrap();
    println!(
        "memory representative profile: {row_bytes} tuple bytes; {wal} cluster WAL bytes during fixture including channel setup; backup drill separately verifies restored memory schema"
    );
    assert!(row_bytes < 512 * 1024);
}

#[tokio::test]
async fn sqlite_owner_controls_source_gates_and_atomic_privacy_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    contracts(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_owner_controls_source_gates_and_atomic_privacy_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    contracts(Store::Pg(pool)).await;
}

#[test]
fn action_and_choice_reject_unknown_owner_and_unbounded_text() {
    let id = Uuid::now_v7();
    assert!(
        serde_json::from_value::<ActionInput>(
            json!({"revision":1,"action":"correct","note_id":id,"text":"Correct"})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<ActionInput>(
            json!({"revision":1,"action":"forget","note_id":id,"user_id":id})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ChoiceInput>(json!({"revision":1,"enabled":true,"user_id":id}))
            .is_err()
    );
    assert!(
        serde_json::from_value::<ActionInput>(
            json!({"revision":1,"action":"correct","note_id":id,"text":"x".repeat(1025)})
        )
        .is_err()
    );
}
