use super::*;
use crate::chatbot::{CircleChatAgents, Store, context_message_json};
use crate::domain::UserId;
use serde_json::json;

pub(in crate::chatbot) struct Fixture {
    pub(in crate::chatbot) service: CircleChatAgents,
    pub(in crate::chatbot) owner: UserId,
    pub(in crate::chatbot) member: UserId,
    pub(in crate::chatbot) circle: String,
    pub(in crate::chatbot) agent: String,
    pub(in crate::chatbot) channel: String,
    pub(in crate::chatbot) private: String,
    pub(in crate::chatbot) profile: String,
    pub(in crate::chatbot) source_a: String,
    pub(in crate::chatbot) source_b: String,
}

impl Fixture {
    pub(in crate::chatbot) async fn create(store: Store) -> Self {
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

    pub(crate) async fn note(&self, channel: &str, sources: &[String], text: &str) -> Uuid {
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
    let _expired_note = f
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
    assert!(exported.notes.is_empty());
    // Expiry/revocation permanently withdraws learned notes; restored authority
    // can create fresh evidence, without resurrecting the previous note.
    let note = f
        .note(
            &f.channel,
            &[f.source_a.clone(), f.source_b.clone()],
            "Discussed Greek lessons together",
        )
        .await;
    let view = f.view().await;
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
    assert_eq!(hidden.unavailable_notes, 0);
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
    // The mutation removes derived notes atomically, including confirmed ones.
    assert!(matches!(
        f.action(hidden.revision, MemoryAction::Forget { note_id: note })
            .await,
        Err(RepositoryError::PermissionDenied)
    ));
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
    let _private_note = f
        .note(
            &f.private,
            std::slice::from_ref(&private_message),
            "Private preference",
        )
        .await;
    assert!(f.view().await.notes.is_empty());
    f.service.store.execute("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,1)",&[f.private.clone(),f.agent.clone(),f.owner.to_string()]).await.unwrap();
    assert_eq!(f.view().await.unavailable_notes, 0);
    let _private_note = f
        .note(
            &f.private,
            std::slice::from_ref(&private_message),
            "Private preference after grant",
        )
        .await;
    f.service
        .store
        .execute(
            "delete from channel_memberships where user_id=?uuid and channel_id=?uuid",
            &[f.owner.to_string(), f.private.clone()],
        )
        .await
        .unwrap();
    let hidden = f.view().await;
    assert_eq!(hidden.unavailable_notes, 0);
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
async fn collection_contract(store: Store) -> crate::chatbot::Result<()> {
    let f = Fixture::create(store).await;
    let st = &f.service.store;
    // Start from consent with no scope: historical messages must not backfill.
    st.execute(
        "delete from agent_memory_scopes where profile_id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await?;
    st.execute(
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        std::slice::from_ref(&f.agent),
    )
    .await?;
    let message = Uuid::now_v7().to_string();
    st.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Same name',3,'A plain message without trigger',current_timestamp)", &[message.clone(),f.channel.clone(),f.owner.to_string()]).await?;
    macro_rules! capture {
        ($pool:expr,$pg:expr,$message:expr) => {{
            let mut tx = $pool.begin().await.map_err(crate::chatbot::storage)?;
            crate::chatbot::memory::collection::capture_message!(tx, $pg, $message);
            tx.commit().await.map_err(crate::chatbot::storage)?;
        }};
    }
    match st {
        Store::Pg(pool) => capture!(pool, true, message),
        Store::Sqlite(pool) => capture!(pool, false, message),
    }
    match st {
        Store::Pg(pool) => capture!(pool, true, message),
        Store::Sqlite(pool) => capture!(pool, false, message),
    }
    assert_eq!(st.values("select cast(start_sequence as text)||':'||cast(processed_sequence as text)||':'||cast(dirty_sequence as text) from agent_memory_scopes where profile_id=?uuid", std::slice::from_ref(&f.profile)).await?, ["2:2:3"]);
    let note = f
        .note(&f.channel, std::slice::from_ref(&message), "New preference")
        .await;
    st.execute(
        "update agent_memory_scopes set processed_sequence=3 where profile_id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await?;
    let before = f.view().await.memory_epoch;
    st.execute(
        "update messages set body='Corrected' where id=?uuid",
        std::slice::from_ref(&message),
    )
    .await?;
    assert_eq!(f.view().await.memory_epoch, before + 1);
    assert_eq!(
        st.values(
            "select cast(count(*) as text) from agent_memory_notes where id=?uuid",
            &[note.to_string()]
        )
        .await?,
        ["0"]
    );
    assert_eq!(st.values("select cast(processed_sequence as text)||':'||cast(dirty_sequence as text)||':'||cast(repair_sequence as text) from agent_memory_scopes where profile_id=?uuid", std::slice::from_ref(&f.profile)).await?,["3:3:2"]);
    st.execute(
        "update agent_memory_profiles set enabled=false where id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await?;
    st.execute(
        "update agent_memory_profiles set enabled=true where id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await?;
    assert_eq!(st.values("select cast(start_sequence as text)||':'||cast(dirty_sequence as text) from agent_memory_scopes where profile_id=?uuid", std::slice::from_ref(&f.profile)).await?,["3:3"]);
    // A stale capture/replay cannot reopen a reset floor or claim a new start.
    st.execute(
        "update agent_memory_profiles set collection_started_at=null where id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await?;
    match st {
        Store::Pg(pool) => capture!(pool, true, message),
        Store::Sqlite(pool) => capture!(pool, false, message),
    }
    assert!(f.view().await.collection_started_at.is_none());
    // An explicitly opted-in private scope still requires explicit agent access.
    let private_message = Uuid::now_v7().to_string();
    st.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Same name',1,'Secret',current_timestamp)", &[private_message.clone(),f.private.clone(),f.owner.to_string()]).await?;
    let message = private_message;
    match st {
        Store::Pg(pool) => capture!(pool, true, message),
        Store::Sqlite(pool) => capture!(pool, false, message),
    }
    assert_eq!(st.values("select cast(count(*) as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid", &[f.profile.clone(),f.private.clone()]).await?,["0"]);
    Ok(())
}

#[tokio::test]
async fn sqlite_memory_collection_and_invalidation_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    collection_contract(Store::Sqlite(pool)).await.unwrap();
}

#[tokio::test]
async fn postgres_memory_collection_and_invalidation_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    collection_contract(Store::Pg(pool)).await.unwrap();
}
async fn reply_memory_contract(store: Store) {
    use crate::chatbot::{Job, memory::reply};
    for mutation in [
        "forget",
        "correct",
        "source_delete",
        "revoke",
        "disabled",
        "membership",
    ] {
        let f = Fixture::create(store.clone()).await;
        let note = f
            .note(
                &f.channel,
                std::slice::from_ref(&f.source_a),
                "Prefers Greek lessons",
            )
            .await;
        let st = &f.service.store;
        let job = Job {
            id: Uuid::now_v7().to_string(),
            agent_id: f.agent.clone(),
            source_message_id: f.source_b.clone(),
            channel_id: f.channel.clone(),
            attempts: 1,
            reply_body: None,
            location_share_id: None,
            lease_token: Uuid::now_v7().to_string(),
        };
        // Target is the owner; another human's notes cannot be selected.
        st.execute("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,lease_token,leased_until,created_at) values(?uuid,?uuid,?uuid,?uuid,1,'leased',0,?uuid,9999999999,0)", &[job.id.clone(),job.agent_id.clone(),job.source_message_id.clone(),job.channel_id.clone(),job.lease_token.clone()]).await.unwrap();
        assert!(
            reply::snapshot(st, &job).await.unwrap().is_none(),
            "another target human must not use owner notes"
        );
        st.execute(
            "update messages set sender_id=?uuid where id=?uuid",
            &[f.owner.to_string(), f.source_b.clone()],
        )
        .await
        .unwrap();
        let input = reply::snapshot(st, &job).await.unwrap().unwrap();
        assert_eq!(input["target_user_id"], f.owner.to_string());
        assert_eq!(input["notes"].as_array().unwrap().len(), 1);
        st.execute("update circle_chat_agent_jobs set reply_body='Cached answer from model' where id=?uuid", std::slice::from_ref(&job.id)).await.unwrap();
        macro_rules! valid {
            ($pool:expr,$pg:ident) => {{
                let mut tx = $pool.begin().await.unwrap();
                let result = reply::$pg(&mut tx, &job.id).await;
                tx.rollback().await.unwrap();
                result.is_ok()
            }};
        }
        let before = match st {
            Store::Pg(p) => valid!(p, authorize_postgres),
            Store::Sqlite(p) => valid!(p, authorize_sqlite),
        };
        assert!(before, "initial {mutation} snapshot");
        match mutation {
            "forget" => {
                f.action(1, MemoryAction::Forget { note_id: note })
                    .await
                    .unwrap();
            }
            "correct" => {
                f.action(
                    1,
                    MemoryAction::Correct {
                        note_id: note,
                        text: "Now prefers Norwegian".to_owned().try_into().unwrap(),
                    },
                )
                .await
                .unwrap();
            }
            "source_delete" => {
                st.execute(
                    "delete from messages where id=?uuid",
                    std::slice::from_ref(&f.source_a),
                )
                .await
                .unwrap();
            }
            "revoke" => {
                st.execute(
                    "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
                    std::slice::from_ref(&f.agent),
                )
                .await
                .unwrap();
            }
            "disabled" => {
                st.execute(
                    "update agent_memory_profiles set enabled=false where id=?uuid",
                    std::slice::from_ref(&f.profile),
                )
                .await
                .unwrap();
            }
            "membership" => {
                st.execute(
                    "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
                    &[f.channel.clone(), f.owner.to_string()],
                )
                .await
                .unwrap();
            }
            _ => unreachable!(),
        }
        let after = match st {
            Store::Pg(p) => valid!(p, authorize_postgres),
            Store::Sqlite(p) => valid!(p, authorize_sqlite),
        };
        assert!(!after, "must fence cached reply after {mutation}");
    }
}
#[tokio::test]
async fn sqlite_memory_reply_publication_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    reply_memory_contract(Store::Sqlite(pool)).await;
}
#[tokio::test]
async fn postgres_memory_reply_publication_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    reply_memory_contract(Store::Pg(pool)).await;
}

async fn memory_reply_job(f: &Fixture, channel: &str, sequence: i64) -> crate::chatbot::Job {
    let st = &f.service.store;
    let source = Uuid::now_v7().to_string();
    st.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Owner',?int,'What do you remember about me?',current_timestamp)", &[source.clone(),channel.into(),f.owner.to_string(),sequence.to_string()]).await.unwrap();
    let job = crate::chatbot::Job {
        id: Uuid::now_v7().to_string(),
        agent_id: f.agent.clone(),
        source_message_id: source,
        channel_id: channel.into(),
        attempts: 1,
        reply_body: None,
        location_share_id: None,
        lease_token: Uuid::now_v7().to_string(),
    };
    st.execute("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,lease_token,leased_until,created_at) values(?uuid,?uuid,?uuid,?uuid,1,'leased',0,?uuid,9999999999,0)", &[job.id.clone(),job.agent_id.clone(),job.source_message_id.clone(),job.channel_id.clone(),job.lease_token.clone()]).await.unwrap();
    job
}

async fn memory_reply_authorized(store: &Store, job: &crate::chatbot::Job) -> bool {
    match store {
        Store::Pg(pool) => {
            let mut tx = pool.begin().await.unwrap();
            let valid = crate::chatbot::memory::reply::authorize_postgres(&mut tx, &job.id)
                .await
                .is_ok();
            tx.rollback().await.unwrap();
            valid
        }
        Store::Sqlite(pool) => {
            let mut tx = pool.begin().await.unwrap();
            let valid = crate::chatbot::memory::reply::authorize_sqlite(&mut tx, &job.id)
                .await
                .is_ok();
            tx.rollback().await.unwrap();
            valid
        }
    }
}

async fn cross_channel_memory_contract(store: Store) {
    use crate::chatbot::memory::reply;
    let f = Fixture::create(store.clone()).await;
    let local = Uuid::now_v7().to_string();
    let second_private = Uuid::now_v7().to_string();
    for (channel, kind) in [(&local, "local"), (&second_private, "private")] {
        store.execute("insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Other memory',?,?uuid,?uuid)",&[channel.clone(),channel.clone(),kind.into(),f.circle.clone(),f.owner.to_string()]).await.unwrap();
        store.execute("insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",&[channel.clone(),f.owner.to_string()]).await.unwrap();
        store.execute("insert into agent_memory_scopes(profile_id,circle_id,channel_id,start_sequence,processed_sequence,dirty_sequence,available_at) values(?uuid,?uuid,?uuid,0,0,0,0)",&[f.profile.clone(),f.circle.clone(),channel.clone()]).await.unwrap();
    }
    for channel in [&f.channel, &f.private, &second_private] {
        store.execute("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,0)",&[channel.clone(),f.agent.clone(),f.owner.to_string()]).await.unwrap();
    }
    let open_note = f
        .note(
            &f.channel,
            std::slice::from_ref(&f.source_a),
            "Likes Greek lessons",
        )
        .await;
    let local_source = memory_reply_job(&f, &local, 1).await;
    let local_note = f
        .note(
            &local,
            std::slice::from_ref(&local_source.source_message_id),
            "Enjoys public conversation",
        )
        .await;
    let private_source = memory_reply_job(&f, &f.private, 1).await;
    let private_note = f
        .note(
            &f.private,
            std::slice::from_ref(&private_source.source_message_id),
            "Private preference",
        )
        .await;
    let second_private_source = memory_reply_job(&f, &second_private, 1).await;
    let second_private_note = f
        .note(
            &second_private,
            std::slice::from_ref(&second_private_source.source_message_id),
            "Another private preference",
        )
        .await;
    for (channel, expected) in [
        (&f.channel, vec![open_note, local_note]),
        (&local, vec![open_note, local_note]),
        (&f.private, vec![open_note, local_note, private_note]),
        (
            &second_private,
            vec![open_note, local_note, second_private_note],
        ),
    ] {
        let job = memory_reply_job(&f, channel, 3).await;
        let input = reply::snapshot(&store, &job).await.unwrap().unwrap();
        let actual: std::collections::HashSet<_> = input["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| Uuid::parse_str(n["id"].as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(
            actual,
            expected.into_iter().collect(),
            "eligible memory in {channel}"
        );
        assert!(
            memory_reply_authorized(&store, &job).await,
            "cross-channel publication"
        );
    }

    // An already generated answer must not disclose a source made private
    // before publication, even when the cached notes themselves are unchanged.
    let cached = memory_reply_job(&f, &local, 4).await;
    reply::snapshot(&store, &cached).await.unwrap().unwrap();
    store
        .execute(
            "update circle_chat_agent_jobs set reply_body='Cached memory answer' where id=?uuid",
            std::slice::from_ref(&cached.id),
        )
        .await
        .unwrap();
    store
        .execute(
            "update channels set kind='private' where id=?uuid",
            std::slice::from_ref(&f.channel),
        )
        .await
        .unwrap();
    assert!(
        !memory_reply_authorized(&store, &cached).await,
        "source became private"
    );
    let fresh = memory_reply_job(&f, &local, 5).await;
    let input = reply::snapshot(&store, &fresh).await.unwrap().unwrap();
    assert_eq!(input["notes"].as_array().unwrap().len(), 1);
    assert_eq!(input["notes"][0]["id"], local_note.to_string());
    // Access loss is checked in the source channel, not just the destination.
    let inbound = memory_reply_job(&f, &f.private, 4).await;
    reply::snapshot(&store, &inbound).await.unwrap().unwrap();
    store
        .execute(
            "delete from channel_memberships where channel_id=?uuid and user_id=?uuid",
            &[local.clone(), f.owner.to_string()],
        )
        .await
        .unwrap();
    assert!(
        !memory_reply_authorized(&store, &inbound).await,
        "source membership lost"
    );
}

#[tokio::test]
async fn sqlite_cross_channel_memory_reply_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    cross_channel_memory_contract(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_cross_channel_memory_reply_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    cross_channel_memory_contract(Store::Pg(pool)).await;
}

#[tokio::test]
async fn postgres_cross_channel_memory_publication_waits_for_source_visibility() {
    use crate::chatbot::memory::reply;
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let f = Fixture::create(Store::Pg(pool.clone())).await;
    f.service.store.execute("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,0)",&[f.private.clone(),f.agent.clone(),f.owner.to_string()]).await.unwrap();
    f.service.store.execute("insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,0)",&[f.channel.clone(),f.agent.clone(),f.owner.to_string()]).await.unwrap();
    f.note(
        &f.channel,
        std::slice::from_ref(&f.source_a),
        "Public preference",
    )
    .await;
    let job = memory_reply_job(&f, &f.private, 1).await;
    reply::snapshot(&f.service.store, &job)
        .await
        .unwrap()
        .unwrap();
    let mut visibility = pool.begin().await.unwrap();
    sqlx::query("update channels set kind='private' where id=$1::text::uuid")
        .bind(&f.channel)
        .execute(&mut *visibility)
        .await
        .unwrap();
    let publisher_store = f.service.store.clone();
    let mut publisher =
        tokio::spawn(async move { memory_reply_authorized(&publisher_store, &job).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut publisher)
            .await
            .is_err(),
        "publication must wait for source visibility lock"
    );
    visibility.commit().await.unwrap();
    assert!(
        !tokio::time::timeout(std::time::Duration::from_secs(5), publisher)
            .await
            .unwrap()
            .unwrap(),
        "fresh private boundary must fence the answer"
    );
}
async fn ordinary_changes_preserve_memory(store: Store) {
    let f = Fixture::create(store).await;
    let note = f
        .note(
            &f.channel,
            std::slice::from_ref(&f.source_a),
            "Owner preference",
        )
        .await;
    let st = &f.service.store;
    st.execute(
        "update agent_memory_notes set origin='user',evidence='user_confirmed' where id=?uuid",
        &[note.to_string()],
    )
    .await
    .unwrap();
    let before = f.view().await.memory_epoch;
    st.execute("update circle_chat_agents set revision=revision+1,response_phrases='[\"New greeting\"]' where agent_id=?uuid", std::slice::from_ref(&f.agent)).await.unwrap();
    assert_eq!(f.view().await.memory_epoch, before);
    assert_eq!(f.view().await.notes[0].id, note);
    st.execute(
        "delete from circle_memberships where circle_id=?uuid and user_id=?uuid",
        &[f.circle.clone(), f.member.to_string()],
    )
    .await
    .unwrap();
    assert_eq!(f.view().await.notes[0].id, note);
    let epoch = f.view().await.memory_epoch;
    st.execute("update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid", std::slice::from_ref(&f.channel)).await.unwrap();
    assert_eq!(f.view().await.memory_epoch, epoch);
    assert_eq!(f.view().await.notes[0].id, note);
}
#[tokio::test]
async fn sqlite_memory_ordinary_changes_preserve_confirmed_notes() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    ordinary_changes_preserve_memory(Store::Sqlite(pool)).await;
}
#[tokio::test]
async fn postgres_memory_ordinary_changes_preserve_confirmed_notes() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    ordinary_changes_preserve_memory(Store::Pg(pool)).await;
}
// Environment gates live in an isolated copy of the test binary. No parallel
// test in the parent process observes the collection flag changing.
fn actual_send_child(test_name: &str) -> bool {
    const CHILD: &str = "SPROYT_MEMORY_SEND_CONTRACT_CHILD";
    if std::env::var(CHILD).as_deref() == Ok(test_name) {
        return true;
    }
    let sqlite_path =
        std::env::temp_dir().join(format!("sproyt-memory-send-{}.sqlite", Uuid::now_v7()));
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture"])
        .env(CHILD, test_name)
        .env("SPROYT_MEMORY_SEND_SQLITE_PATH", &sqlite_path)
        .env("SPROYT_CHAT_AGENT_MEMORY_COLLECT_ENABLED", "true")
        .env("SPROYT_CHAT_AGENT_MEMORY_BUILD_ENABLED", "false")
        .env("SPROYT_CHAT_AGENT_MEMORY_USE_ENABLED", "false")
        .output()
        .unwrap();
    if sqlite_path.exists() {
        std::fs::remove_file(&sqlite_path).unwrap();
    }
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
        "isolated send contract failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

async fn actual_send_contract(
    store: Store,
    repository: std::sync::Arc<dyn crate::domain::ChatRepository>,
) {
    use crate::domain::{ChannelId, MessageBody, SendMessage};
    let f = Fixture::create(store).await;
    let st = &f.service.store;
    st.execute(
        "delete from agent_memory_scopes where profile_id=?uuid",
        std::slice::from_ref(&f.profile),
    )
    .await
    .unwrap();
    st.execute(
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        std::slice::from_ref(&f.agent),
    )
    .await
    .unwrap();
    st.execute("insert into channel_sequences(channel_id,next_sequence) values(?uuid,3) on conflict(channel_id) do update set next_sequence=3", std::slice::from_ref(&f.channel)).await.unwrap();
    let command = || SendMessage {
        actor: f.owner.clone(),
        channel_id: ChannelId::from_uuid(Uuid::parse_str(&f.channel).unwrap()),
        parent_message_id: None,
        body: MessageBody::new("A plain durable sentence").unwrap(),
    };
    let direct = repository.append_message(command()).await.unwrap();
    let request = Uuid::now_v7().to_string();
    let once = repository
        .append_message_idempotent(command(), request.clone())
        .await
        .unwrap();
    let replay = repository
        .append_message_idempotent(command(), request)
        .await
        .unwrap();
    assert_eq!(once, replay);
    assert_eq!(u64::from(direct.sequence), 3);
    assert_eq!(u64::from(once.sequence), 4);
    assert_eq!(st.values("select cast(start_sequence as text)||':'||cast(processed_sequence as text)||':'||cast(dirty_sequence as text) from agent_memory_scopes where profile_id=?uuid", std::slice::from_ref(&f.profile)).await.unwrap(),["2:2:4"]);
    assert_eq!(
        st.values(
            "select provenance from message_provenance where message_id=?uuid",
            &[once.id.as_uuid().to_string()]
        )
        .await
        .unwrap(),
        ["human"]
    );
    assert_eq!(
        st.values(
            "select cast(count(*) as text) from circle_chat_agent_jobs where agent_id=?uuid",
            std::slice::from_ref(&f.agent)
        )
        .await
        .unwrap(),
        ["0"]
    );
    assert_eq!(
        st.values(
            "select cast(count(*) as text) from messages where channel_id=?uuid",
            std::slice::from_ref(&f.channel)
        )
        .await
        .unwrap(),
        ["4"]
    );
    assert!(f.view().await.collection_started_at.is_some());
    if let Store::Pg(pool) = st {
        // A real second connection attempts capture while consent is changing.
        // It must wait for the profile fence and then see the committed opt-out.
        let mut fence = pool.begin().await.unwrap();
        sqlx::query("select id from agent_memory_profiles where id=$1 for update")
            .bind(Uuid::parse_str(&f.profile).unwrap())
            .fetch_one(&mut *fence)
            .await
            .unwrap();
        let repo = repository.clone();
        let send = command();
        let mut pending = tokio::spawn(async move { repo.append_message(send).await });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
                .await
                .is_err()
        );
        sqlx::query("update agent_memory_profiles set enabled=false where id=$1")
            .bind(Uuid::parse_str(&f.profile).unwrap())
            .execute(&mut *fence)
            .await
            .unwrap();
        fence.commit().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .expect("send/consent fence deadlocked")
            .unwrap()
            .unwrap();
        assert_eq!(st.values("select cast(dirty_sequence as text) from agent_memory_scopes where profile_id=?uuid", std::slice::from_ref(&f.profile)).await.unwrap(),["4"]);
        assert!(!f.view().await.enabled);
    }
}

#[tokio::test]
async fn sqlite_memory_actual_send_capture_contract() {
    if !actual_send_child(
        "chatbot::memory::repository::tests::sqlite_memory_actual_send_capture_contract",
    ) {
        return;
    }
    let path = std::path::PathBuf::from(std::env::var("SPROYT_MEMORY_SEND_SQLITE_PATH").unwrap());
    let url = format!("sqlite://{}", path.to_string_lossy().replace('\\', "/"));
    let repository = crate::db::SqliteChatRepository::connect(&url)
        .await
        .unwrap();
    repository.migrate().await.unwrap();
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    actual_send_contract(Store::Sqlite(pool.clone()), std::sync::Arc::new(repository)).await;
    pool.close().await;
    // The parent removes the file after this process closes all connections.
}

#[tokio::test]
async fn postgres_memory_actual_send_capture_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    if !actual_send_child(
        "chatbot::memory::repository::tests::postgres_memory_actual_send_capture_contract",
    ) {
        return;
    }
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let repository = crate::db::PostgresChatRepository::connect_with_pool(&url, pool.clone())
        .await
        .unwrap();
    actual_send_contract(Store::Pg(pool), std::sync::Arc::new(repository)).await;
}
#[tokio::test]
async fn postgres_memory_reply_rechecks_lease_after_profile_lock_wait() {
    use crate::chatbot::{Job, authorize_reply_postgres, memory::reply};
    use crate::domain::{ChannelId, MessageBody, SendMessage};
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let f = Fixture::create(Store::Pg(pool.clone())).await;
    let st = &f.service.store;
    st.execute(
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        std::slice::from_ref(&f.agent),
    )
    .await
    .unwrap();
    f.note(
        &f.channel,
        std::slice::from_ref(&f.source_a),
        "Prefers Greek lessons",
    )
    .await;
    let job = Job {
        id: Uuid::now_v7().to_string(),
        agent_id: f.agent.clone(),
        source_message_id: f.source_a.clone(),
        channel_id: f.channel.clone(),
        attempts: 1,
        reply_body: None,
        location_share_id: None,
        lease_token: Uuid::now_v7().to_string(),
    };
    st.execute("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,lease_token,leased_until,created_at) values(?uuid,?uuid,?uuid,?uuid,1,'leased',0,?uuid,9999999999,0)", &[job.id.clone(),job.agent_id.clone(),job.source_message_id.clone(),job.channel_id.clone(),job.lease_token.clone()]).await.unwrap();
    reply::snapshot(st, &job).await.unwrap().unwrap();
    st.execute("update circle_chat_agent_jobs set reply_body='Cached answer',leased_until=cast(extract(epoch from clock_timestamp()) as bigint)+2 where id=?uuid", std::slice::from_ref(&job.id)).await.unwrap();
    let command = SendMessage {
        actor: UserId::new(&f.agent).unwrap(),
        channel_id: ChannelId::new(&f.channel).unwrap(),
        parent_message_id: None,
        body: MessageBody::new("Cached answer").unwrap(),
    };
    let mut holder = pool.begin().await.unwrap();
    sqlx::query("select id from agent_memory_profiles where id=$1::text::uuid for update")
        .bind(&f.profile)
        .fetch_one(&mut *holder)
        .await
        .unwrap();
    let request = format!("circle-chat-agent:{}", job.id);
    let publisher = tokio::spawn(async move {
        let mut tx = pool.begin().await.unwrap();
        let result = authorize_reply_postgres(&mut tx, &command, &request).await;
        tx.rollback().await.unwrap();
        result
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !publisher.is_finished(),
        "publication must wait on the held memory profile"
    );
    // The authority remains unchanged while only the wall-clock lease expires.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    holder.commit().await.unwrap();
    assert!(matches!(
        publisher.await.unwrap(),
        Err(RepositoryError::PermissionDenied)
    ));
}
async fn pause_preserves_inspectable_memory(store: Store) {
    let f = Fixture::create(store).await;
    let note = f
        .note(
            &f.channel,
            std::slice::from_ref(&f.source_a),
            "Owner preference retained during pause",
        )
        .await;
    let st = &f.service.store;
    st.execute(
        "update agent_memory_notes set origin='user',evidence='user_confirmed' where id=?uuid",
        &[note.to_string()],
    )
    .await
    .unwrap();
    let before = f.view().await;
    let paused = f
        .service
        .mutate_memory(
            &f.owner,
            &f.circle,
            &f.agent,
            Mutation::Choice(ChoiceInput {
                revision: before.revision,
                enabled: false,
            }),
        )
        .await
        .unwrap();
    assert!(!paused.enabled);
    assert_eq!(paused.memory_epoch, before.memory_epoch + 1);
    assert_eq!(paused.notes.len(), 1);
    assert_eq!(paused.notes[0].id, note);
    let resumed = f
        .service
        .mutate_memory(
            &f.owner,
            &f.circle,
            &f.agent,
            Mutation::Choice(ChoiceInput {
                revision: paused.revision,
                enabled: true,
            }),
        )
        .await
        .unwrap();
    assert!(resumed.enabled);
    assert_eq!(resumed.notes[0].id, note);
    assert_eq!(st.values("select cast(start_sequence as text)||':'||cast(processed_sequence as text)||':'||cast(dirty_sequence as text) from agent_memory_scopes where profile_id=?uuid and channel_id=?uuid", &[f.profile.clone(),f.channel.clone()]).await.unwrap(),["2:2:2"]);
    for statement in [
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
        "update circle_chat_agents set memory_enabled=false where agent_id=?uuid",
        "update circle_chat_agents set memory_enabled=true where agent_id=?uuid",
        "update circle_chat_agents set enabled=false where agent_id=?uuid",
        "update circle_chat_agents set enabled=true where agent_id=?uuid",
    ] {
        let epoch = f.view().await.memory_epoch;
        st.execute(statement, std::slice::from_ref(&f.agent))
            .await
            .unwrap();
        let after = f.view().await;
        assert_eq!(after.memory_epoch, epoch + 1);
        assert_eq!(after.notes.len(), 1);
        assert_eq!(after.notes[0].id, note);
        assert_eq!(after.notes[0].origin, NoteOrigin::User);
    }
    // Preserving a paused note does not relax the original source dependency.
    st.execute(
        "update messages set body='Retracted preference' where id=?uuid",
        std::slice::from_ref(&f.source_a),
    )
    .await
    .unwrap();
    assert!(f.view().await.notes.is_empty());
}
#[tokio::test]
async fn sqlite_memory_pause_preserves_inspectable_notes() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    pause_preserves_inspectable_memory(Store::Sqlite(pool)).await;
}
#[tokio::test]
async fn postgres_memory_pause_preserves_inspectable_notes() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    pause_preserves_inspectable_memory(Store::Pg(pool)).await;
}
