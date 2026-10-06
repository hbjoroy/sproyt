use super::*;
use crate::chatbot::{ContextMessage, Store, context_message_json};
use serde_json::json;

fn source() -> SourceMetadata {
    SourceMetadata {
        message_id: Uuid::from_u128(1),
        circle_id: Some(Uuid::from_u128(2)),
        channel_id: Uuid::from_u128(3),
        sender_id: Uuid::from_u128(4),
        sender_kind: Some(PrincipalKind::Human),
        provenance: Some(ActivityProvenance::Human),
        parent_message_id: None,
        sequence: 7,
        created_at: "2026-10-06T10:00:00Z".parse().unwrap(),
        edited_at: None,
        deleted_at: None,
        version: None,
    }
}

#[test]
fn source_version_tracks_body_identity_provenance_and_edit_not_display_name() {
    let mut original = source();
    original.seal("Καλημέρα! [[internal:token]]").unwrap();
    assert!(original.is_human_evidence());
    for variant in 0..5 {
        let mut changed = original.clone();
        let mut body = "Καλημέρα! [[internal:token]]";
        match variant {
            0 => changed.sender_id = Uuid::from_u128(5),
            1 => changed.channel_id = Uuid::from_u128(6),
            2 => changed.provenance = Some(ActivityProvenance::HumanApproved),
            3 => changed.edited_at = Some("2026-10-06T10:00:01Z".parse().unwrap()),
            _ => body = "Καλημέρα!",
        }
        changed.seal(body).unwrap();
        assert_ne!(changed.version, original.version);
    }
    let mut message = ContextMessage {
        id: original.message_id.to_string(),
        author: "Alex".into(),
        body: "Καλημέρα! [[internal:token]]".into(),
        source: Some(original.clone()),
    };
    message.author = "New name".into();
    message.seal_source().unwrap();
    assert_eq!(message.source.unwrap().version, original.version);
}

#[test]
fn source_digest_is_server_owned_and_unknown_or_approved_sources_are_not_human_evidence() {
    let mut input = serde_json::to_value(source()).unwrap();
    input["version"] = json!("a".repeat(64));
    input["created_at"] = json!("2026-10-06 10:00:00.123456");
    assert!(serde_json::from_value::<SourceMetadata>(input.clone()).is_err());
    input.as_object_mut().unwrap().remove("version");
    let mut parsed: SourceMetadata = serde_json::from_value(input).unwrap();
    assert!(parsed.version.is_none());
    assert!(!parsed.is_human_evidence());
    assert_eq!(
        parsed.created_at.to_rfc3339(),
        "2026-10-06T10:00:00.123456+00:00"
    );
    parsed.seal("A real statement").unwrap();
    assert!(parsed.is_human_evidence());
    for provenance in [
        None,
        Some(ActivityProvenance::Generated),
        Some(ActivityProvenance::Delegated),
        Some(ActivityProvenance::HumanApproved),
    ] {
        parsed.provenance = provenance;
        assert!(!parsed.is_human_evidence());
    }
    parsed.provenance = Some(ActivityProvenance::Human);
    parsed.sender_kind = Some(PrincipalKind::Agent);
    assert!(!parsed.is_human_evidence());
    parsed.sender_kind = Some(PrincipalKind::Human);
    parsed.deleted_at = Some(Utc::now());
    assert!(!parsed.is_human_evidence());
}

#[test]
fn candidate_cannot_choose_owner_or_reference_another_scope_or_unprovided_source() {
    let mut owner_source = source();
    owner_source.seal("I like Greek lessons").unwrap();
    let scope = MemoryScope {
        owner: MemoryOwner {
            circle_id: owner_source.circle_id.unwrap(),
            agent_id: Uuid::from_u128(8),
            user_id: owner_source.sender_id,
        },
        channel_id: owner_source.channel_id,
    };
    let mut candidate: MemoryCandidate = serde_json::from_value(json!({
        "kind":"preference", "text":"Likes Greek lessons", "source_message_ids":[owner_source.message_id]
    })).unwrap();
    assert!(
        candidate
            .validate_sources(&scope, &[owner_source.clone()])
            .is_ok()
    );
    let mut forged = serde_json::to_value(&candidate).unwrap();
    forged["owner_id"] = json!(Uuid::from_u128(9));
    assert!(serde_json::from_value::<MemoryCandidate>(forged).is_err());
    candidate.source_message_ids.push(Uuid::from_u128(10));
    assert_eq!(
        candidate.validate_sources(&scope, &[owner_source.clone()]),
        Err("unknown_memory_source")
    );
    candidate.source_message_ids = vec![owner_source.message_id];
    owner_source.channel_id = Uuid::from_u128(11);
    assert_eq!(
        candidate.validate_sources(&scope, &[owner_source.clone()]),
        Err("invalid_memory_source_scope")
    );
    owner_source.channel_id = scope.channel_id;
    owner_source.sender_id = Uuid::from_u128(12);
    assert_eq!(
        candidate.validate_sources(&scope, &[owner_source]),
        Err("missing_memory_owner_evidence")
    );
}

#[test]
fn note_epoch_and_aggregate_limits_reject_bad_or_exhausted_values() {
    assert!(serde_json::from_value::<MemoryEpoch>(json!(0)).is_err());
    assert!(MemoryEpoch::try_from(i64::MAX).unwrap().next().is_err());
    assert!(SourceVersion::try_from("A".repeat(64)).is_err());
    assert!(NoteText::try_from(" ".to_owned()).is_err());
    assert!(NoteText::try_from("ø".repeat(MAX_NOTE_BYTES)).is_err());
    assert!(NoteText::try_from("text\0".to_owned()).is_err());
    let usage = MemoryUsage {
        notes: MAX_PROFILE_NOTES,
        note_bytes: MAX_PROFILE_NOTE_BYTES,
        source_references: MAX_PROFILE_SOURCE_REFERENCES,
        exclusions: MAX_PROFILE_EXCLUSIONS,
        scopes: MAX_PROFILE_SCOPES,
        serialized_bytes: MAX_PROFILE_SERIALIZED_BYTES,
    };
    assert!(usage.within_limits());
    for index in 0..6 {
        let mut overflow = usage;
        match index {
            0 => overflow.notes += 1,
            1 => overflow.note_bytes += 1,
            2 => overflow.source_references += 1,
            3 => overflow.exclusions += 1,
            4 => overflow.scopes += 1,
            _ => overflow.serialized_bytes += 1,
        }
        assert!(!overflow.within_limits());
    }
}

#[test]
fn memory_admissions_default_off_and_each_gate_is_independent() {
    assert_eq!(MemoryGates::from_lookup(|_| None), MemoryGates::default());
    assert_eq!(
        MemoryGates::from_lookup(|_| Some("TRUE".into())),
        MemoryGates::default()
    );
    let gates = MemoryGates::from_lookup(|key| {
        (key == "SPROYT_CHAT_AGENT_MEMORY_BUILD_ENABLED").then(|| "true".into())
    });
    assert_eq!(
        gates,
        MemoryGates {
            build: true,
            ..MemoryGates::default()
        }
    );
}

fn budget_fixture() -> serde_json::Value {
    let scope = MemoryScope {
        owner: MemoryOwner {
            circle_id: Uuid::from_u128(1),
            agent_id: Uuid::from_u128(2),
            user_id: Uuid::from_u128(3),
        },
        channel_id: Uuid::from_u128(4),
    };
    let notes: Vec<_> = (0..MAX_PROFILE_NOTES)
        .map(|index| MemoryCandidate {
            kind: NoteKind::Interaction,
            text: NoteText::try_from("x".repeat(MAX_PROFILE_NOTE_BYTES / MAX_PROFILE_NOTES))
                .unwrap(),
            source_message_ids: (0..MAX_BATCH_MESSAGES + MAX_NEIGHBOUR_MESSAGES)
                .map(|offset| Uuid::from_u128((100 + index * 30 + offset) as u128))
                .collect(),
        })
        .collect();
    let references: Vec<_> = (0..MAX_PROFILE_SOURCE_REFERENCES)
        .map(|index| {
            json!({
                "note_id":Uuid::from_u128((1000+index/30) as u128),
                "source":SourceReference {
                    message_id:Uuid::from_u128((100+index) as u128),
                    version:SourceVersion::try_from("a".repeat(64)).unwrap(),
                }
            })
        })
        .collect();
    let exclusions: Vec<_> = (0..MAX_PROFILE_EXCLUSIONS)
        .map(|index| Uuid::from_u128((10_000 + index) as u128))
        .collect();
    let scopes: Vec<_> = (0..MAX_PROFILE_SCOPES)
        .map(|index| {
            json!({"scope":MemoryScope { channel_id:Uuid::from_u128((20_000+index) as u128), ..scope },
                "start_sequence":100, "processed_sequence":100, "dirty_sequence":120,
                "source_generation":1, "attempts":0, "lease_token":null,
                "available_at":1_791_000_000, "leased_until":null})
        })
        .collect();
    json!({"owner":scope.owner,"epoch":1,"notes":notes,"sources":references,
        "exclusions":exclusions,"scopes":scopes})
}

#[test]
fn bounded_profile_fixture_accounts_for_sources_exclusions_and_work() {
    let fixture = budget_fixture();
    let bytes = serde_json::to_vec(&fixture).unwrap().len();
    assert!(
        bytes <= MAX_PROFILE_SERIALIZED_BYTES,
        "profile fixture has {bytes} bytes"
    );
    assert!(bytes > MAX_PROFILE_NOTE_BYTES * 5);
    println!(
        "memory profile contract fixture: {bytes} serialized bytes; DB/index/WAL overhead measured separately"
    );
}

async fn verify_context_attribution(store: Store) {
    let circle = Uuid::now_v7();
    let channel = Uuid::now_v7();
    let human_a = Uuid::now_v7();
    let human_b = Uuid::now_v7();
    let agent = Uuid::now_v7();
    for (id, kind) in [(human_a, "human"), (human_b, "human"), (agent, "agent")] {
        store
            .execute(
                "insert into users(id,kind,display_name) values(?uuid,?,'Alex')",
                &[id.to_string(), kind.into()],
            )
            .await
            .unwrap();
    }
    store
        .execute(
            "insert into circles(id,slug,name,created_by) values(?uuid,?,'Memory contract',?uuid)",
            &[circle.to_string(), circle.to_string(), human_a.to_string()],
        )
        .await
        .unwrap();
    store.execute("insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Memory contract','public',?uuid,?uuid)", &[channel.to_string(),channel.to_string(),circle.to_string(),human_a.to_string()]).await.unwrap();
    store.execute("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?uuid,?uuid,?uuid,'memory-contract',?,'Test',10,current_timestamp)", &[agent.to_string(),human_a.to_string(),human_a.to_string(),agent.to_string()]).await.unwrap();
    for (index, sender) in [human_a, human_b, agent].into_iter().enumerate() {
        store.execute("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?uuid,?uuid,?uuid,'Alex',?int,'Same words',current_timestamp)", &[Uuid::now_v7().to_string(),channel.to_string(),sender.to_string(),(index+1).to_string()]).await.unwrap();
    }
    let pg = matches!(&store, Store::Pg(_));
    for alias in ["m", "anchor"] {
        let object = context_message_json(alias, pg);
        let query = format!(
            "select cast({object} as text) from messages {alias} join channels c on c.id={alias}.channel_id where c.id=?uuid order by {alias}.sequence"
        );
        let rows = store.values(&query, &[channel.to_string()]).await.unwrap();
        let mut context: Vec<ContextMessage> = rows
            .into_iter()
            .map(|row| serde_json::from_str(&row).unwrap())
            .collect();
        for message in &mut context {
            message.seal_source().unwrap();
        }
        assert_eq!(context.len(), 3);
        assert!(context.iter().all(|message| message.author == "Alex"));
        let sources: Vec<_> = context
            .iter()
            .map(|message| message.source.as_ref().unwrap())
            .collect();
        assert_eq!(sources[0].sender_id, human_a);
        assert_eq!(sources[1].sender_id, human_b);
        assert_eq!(sources[2].sender_id, agent);
        assert!(sources[0].is_human_evidence());
        assert!(sources[1].is_human_evidence());
        assert!(!sources[2].is_human_evidence());
        assert_eq!(sources[2].provenance, Some(ActivityProvenance::Generated));
        assert_ne!(sources[0].version, sources[1].version);
        assert!(
            sources
                .iter()
                .all(|source| source.circle_id == Some(circle) && source.channel_id == channel)
        );
        let old = sources[0].version.clone();
        store
            .execute(
                "update messages set sender_display_name='Renamed' where id=?uuid",
                &[context[0].id.clone()],
            )
            .await
            .unwrap();
        let row = store
            .values(&query, &[channel.to_string()])
            .await
            .unwrap()
            .remove(0);
        let mut renamed: ContextMessage = serde_json::from_str(&row).unwrap();
        renamed.seal_source().unwrap();
        assert_eq!(renamed.source.unwrap().version, old);
        store
            .execute(
                "update messages set sender_display_name='Alex' where id=?uuid",
                &[context[0].id.clone()],
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn sqlite_memory_context_attribution_contract() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::migrate!("./migrations/sqlite")
        .run(&pool)
        .await
        .unwrap();
    verify_context_attribution(Store::Sqlite(pool)).await;
}

#[tokio::test]
async fn postgres_memory_context_attribution_contract() {
    let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    sqlx::migrate!("./migrations/postgres")
        .run(&pool)
        .await
        .unwrap();
    let fixture = serde_json::to_string(&budget_fixture()).unwrap();
    let jsonb_bytes: i64 = sqlx::query_scalar("select pg_column_size($1::text::jsonb)::bigint")
        .bind(&fixture)
        .fetch_one(&pool)
        .await
        .unwrap();
    println!(
        "memory profile fixture: {} JSON bytes / {jsonb_bytes} PostgreSQL JSONB bytes; not relational-table/index disk usage",
        fixture.len()
    );
    verify_context_attribution(Store::Pg(pool)).await;
}
