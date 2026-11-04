use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    directory: PathBuf,
    database: String,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::var_os("SPROYT_DRAFT_TEST_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let directory = root.join(format!(
            "sproyt-native-probe-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let database = directory
            .join("fixture.sqlite")
            .to_str()
            .unwrap()
            .to_owned();
        Self {
            directory,
            database,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Only the exact test-owned files. Never recursively remove a directory.
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.database));
        }
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn call(value: Value) -> Value {
    let input = serde_json::to_vec(&value).unwrap();
    let mut output = vec![0; RESPONSE_CAPACITY];
    // SAFETY: distinct owned buffers remain alive and have the stated sizes.
    let length = unsafe {
        sproyt_probe_call(
            input.as_ptr(),
            input.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    assert!(length > 0, "ABI status {length}");
    serde_json::from_slice(&output[..length as usize]).unwrap()
}

#[test]
fn restart_preserves_exact_payload_and_uncertain_delivery() {
    let fixture = Fixture::new();
    let scenario: Value =
        serde_json::from_str(include_str!("../../scenarios/persistence-v1.json")).unwrap();
    assert_eq!(scenario["schema"], 1);
    let account = scenario["account"].as_str().unwrap();
    let channel = scenario["channel"].as_str().unwrap();
    let request_id = scenario["request_id"].as_str().unwrap();
    let body = scenario["body"].as_str().unwrap();
    let original;
    {
        let mut store = Store::open(&fixture.database, account).unwrap();
        store.save_draft(1, channel, body).unwrap();
        store.enqueue(1, request_id, channel, body).unwrap();
        store.delivery(1, request_id, "dispatch").unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["drafts"], json!([]));
        original = snapshot["outbox"][0]["payload"].clone();
    }
    let mut store = Store::open(&fixture.database, account).unwrap();
    let recovered = store.snapshot().unwrap();
    assert_eq!(recovered["outbox"][0]["state"], "uncertain");
    assert_eq!(recovered["outbox"][0]["payload"], original);
    assert_eq!(
        store.enqueue(1, request_id, channel, "Anna innhald"),
        Err("request_conflict")
    );
    store.enqueue(1, request_id, channel, body).unwrap();
    store.delivery(1, request_id, "dispatch").unwrap();
    store.delivery(1, request_id, "acknowledge").unwrap();
    store.delivery(1, request_id, "acknowledge").unwrap();
    assert_eq!(
        store.snapshot().unwrap()["outbox"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        store.delivery(1, request_id, "dispatch"),
        Err("invalid_delivery_transition")
    );
}

#[test]
fn invalid_negative_schema_version_is_not_reinterpreted_as_v1() {
    let fixture = Fixture::new();
    let store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store.execute_test_sql("PRAGMA user_version=-1");
    drop(store);
    assert_eq!(
        Store::open(&fixture.database, "fixture-alice").err(),
        Some("unsupported_schema")
    );
}

#[test]
fn definitive_rejection_never_becomes_an_automatic_retry() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store.enqueue(1, "request", "channel", "Hei").unwrap();
    store.delivery(1, "request", "dispatch").unwrap();
    store.delivery(1, "request", "reject").unwrap();
    assert_eq!(
        store.delivery(1, "request", "dispatch"),
        Err("invalid_delivery_transition")
    );
    assert_eq!(
        store.delivery(1, "request", "acknowledge"),
        Err("invalid_delivery_transition")
    );
}

#[test]
fn storage_failure_keeps_the_draft_and_never_claims_admission() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store.save_draft(1, "channel", "Bevar meg").unwrap();
    store.execute_test_sql("PRAGMA query_only=ON");
    assert_eq!(
        store.enqueue(1, "request", "channel", "Bevar meg"),
        Err("storage_error")
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot["drafts"][0]["body"], "Bevar meg");
    assert_eq!(snapshot["outbox"], json!([]));
}

#[test]
fn account_schema_owner_and_generation_are_fenced() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    assert_eq!(
        Store::open(&fixture.database, "fixture-alice").err(),
        Some("store_busy")
    );
    store.invalidate(1).unwrap();
    assert_eq!(
        store.save_draft(1, "channel", "Seint resultat"),
        Err("stale_generation")
    );
    assert_eq!(
        store.save_draft(2, "channel", "Framleis avslått"),
        Err("stale_generation")
    );
    drop(store);
    assert_eq!(
        Store::open(&fixture.database, "fixture-bob").err(),
        Some("account_mismatch")
    );
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    assert_eq!(store.snapshot().unwrap()["active"], false);
    assert_eq!(
        store.apply_message(1, "message", 2, "Late", false),
        Err("stale_generation")
    );
    store.execute_test_sql("PRAGMA user_version=4");
    drop(store);
    assert_eq!(
        Store::open(&fixture.database, "fixture-alice").err(),
        Some("unsupported_schema")
    );
}

#[test]
fn stale_entity_versions_cannot_resurrect_a_deleted_message() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store.apply_message(1, "message", 1, "Før", false).unwrap();
    store
        .apply_message(1, "message", 2, "Etter", false)
        .unwrap();
    assert_eq!(store.snapshot().unwrap()["messages"][0]["body"], "Etter");
    store.apply_message(1, "message", 3, "", true).unwrap();
    store
        .apply_message(1, "message", 1, "Gammalt svar", false)
        .unwrap();
    assert_eq!(store.snapshot().unwrap()["messages"][0]["deleted"], true);
    assert_eq!(store.snapshot().unwrap()["messages"][0]["body"], "");
}

#[test]
fn small_output_invalid_utf8_and_oversize_requests_have_no_mutation() {
    let fixture = Fixture::new();
    let request = serde_json::to_vec(
        &json!({"op":"open","database":fixture.database,"account":"fixture-alice"}),
    )
    .unwrap();
    let mut small = [0_u8; 32];
    // SAFETY: the deliberately undersized output is still a valid stated buffer.
    assert_eq!(
        unsafe {
            sproyt_probe_call(
                request.as_ptr(),
                request.len(),
                small.as_mut_ptr(),
                small.len(),
            )
        },
        -2
    );
    assert!(!PathBuf::from(&fixture.database).exists());
    let mut output = vec![0; RESPONSE_CAPACITY];
    let invalid = [0xff];
    // SAFETY: all supplied buffers and lengths are valid and disjoint.
    let size = unsafe {
        sproyt_probe_call(
            invalid.as_ptr(),
            invalid.len(),
            output.as_mut_ptr(),
            output.len(),
        )
    };
    let response: Value = serde_json::from_slice(&output[..size as usize]).unwrap();
    assert_eq!(response["error"], "invalid_request");
    let oversized = vec![0; INPUT_LIMIT + 1];
    // SAFETY: all supplied buffers and lengths are valid and disjoint.
    assert_eq!(
        unsafe {
            sproyt_probe_call(
                oversized.as_ptr(),
                oversized.len(),
                output.as_mut_ptr(),
                output.len(),
            )
        },
        -3
    );
    assert!(!PathBuf::from(&fixture.database).exists());
}

#[test]
fn handles_are_not_reused_and_close_is_serialized_with_calls() {
    let fixture = Fixture::new();
    let first =
        call(json!({"op":"open","database":fixture.database,"account":"fixture-alice"}))["result"]
            ["handle"]
            .as_u64()
            .unwrap();
    let reader = std::thread::spawn(move || {
        for _ in 0..10 {
            let response = call(json!({"op":"snapshot","handle":first}));
            assert!(response["ok"] == true || response["error"] == "invalid_handle");
        }
    });
    assert_eq!(call(json!({"op":"close","handle":first}))["ok"], true);
    reader.join().unwrap();
    assert_eq!(
        call(json!({"op":"close","handle":first}))["error"],
        "invalid_handle"
    );
    let second =
        call(json!({"op":"open","database":fixture.database,"account":"fixture-alice"}))["result"]
            ["handle"]
            .as_u64()
            .unwrap();
    assert!(second > first);
    assert_eq!(
        call(json!({"op":"snapshot","handle":first}))["error"],
        "invalid_handle"
    );
    assert_eq!(call(json!({"op":"close","handle":second}))["ok"], true);
}

#[test]
fn escaped_text_and_item_limits_keep_snapshot_inside_the_abi_bound() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    let body = "\u{1}".repeat(store::TEXT_LIMIT);
    for index in 0..8 {
        let id = index.to_string();
        store.enqueue(1, &id, &id, &body).unwrap();
        store.save_draft(1, &id, &body).unwrap();
        store.apply_message(1, &id, 1, &body, false).unwrap();
    }
    assert!(
        serde_json::to_vec(&store.snapshot().unwrap())
            .unwrap()
            .len()
            < RESPONSE_CAPACITY
    );
    assert_eq!(
        store.enqueue(1, "overflow", "channel", "Hei"),
        Err("capacity")
    );
    assert_eq!(store.save_draft(1, "overflow", "Hei"), Err("capacity"));
    assert_eq!(
        store.save_draft(1, "channel", &"ø".repeat(1024)),
        Err("text_too_large")
    );
}

#[test]
fn failures_between_insert_draft_removal_and_commit_roll_back_together() {
    use crate::fault::{Fault, Mode, Site};
    for site in [Site::AfterInsert, Site::BeforeCommit] {
        let fixture = Fixture::new();
        let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
        store.save_draft(1, "channel", "Bevar utkastet").unwrap();
        store
            .arm_fault(
                1,
                Fault {
                    site,
                    mode: Mode::Error,
                },
            )
            .unwrap();
        assert_eq!(
            store.enqueue(1, "request", "channel", "Bevar utkastet"),
            Err("injected_failure")
        );
        assert_eq!(
            store.snapshot().unwrap()["drafts"][0]["body"],
            "Bevar utkastet"
        );
        assert_eq!(store.snapshot().unwrap()["outbox"], json!([]));
        drop(store);
        let mut reopened = Store::open(&fixture.database, "fixture-alice").unwrap();
        assert_eq!(reopened.snapshot().unwrap()["outbox"], json!([]));
        assert_eq!(
            reopened.snapshot().unwrap()["drafts"][0]["body"],
            "Bevar utkastet"
        );
        reopened
            .enqueue(1, "request", "channel", "Bevar utkastet")
            .unwrap();
        assert_eq!(
            reopened.snapshot().unwrap()["outbox"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn lost_api_result_after_commit_preserves_one_admission() {
    use crate::fault::{Fault, Mode, Site};
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store.save_draft(1, "channel", "Send meg").unwrap();
    store
        .arm_fault(
            1,
            Fault {
                site: Site::AfterCommit,
                mode: Mode::Error,
            },
        )
        .unwrap();
    assert_eq!(
        store.enqueue(1, "request", "channel", "Send meg"),
        Err("injected_unknown_outcome")
    );
    assert_eq!(store.snapshot().unwrap()["drafts"], json!([]));
    let payload = store.snapshot().unwrap()["outbox"][0]["payload"].clone();
    drop(store);
    let mut reopened = Store::open(&fixture.database, "fixture-alice").unwrap();
    reopened
        .enqueue(1, "request", "channel", "Send meg")
        .unwrap();
    let snapshot = reopened.snapshot().unwrap();
    assert_eq!(snapshot["outbox"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["outbox"][0]["payload"], payload);
}

#[test]
fn actual_deferred_commit_failure_restores_draft_and_leaves_connection_usable() {
    use crate::fault::{Fault, Mode, Site};
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store
        .save_draft(1, "channel", "Commit må vere atomisk")
        .unwrap();
    store.execute_test_sql("PRAGMA foreign_keys=ON;
        create table fault_parent(id integer primary key);
        create table fault_child(parent_id integer references fault_parent(id) deferrable initially deferred);
        create trigger fail_commit after insert on outbox begin insert into fault_child values(99); end;");
    // Reaching this diagnostic point proves the violation was deferred, and
    // both insertion and draft removal completed before the commit boundary.
    store
        .arm_fault(
            1,
            Fault {
                site: Site::BeforeCommit,
                mode: Mode::Error,
            },
        )
        .unwrap();
    assert_eq!(
        store.enqueue(1, "request", "channel", "Commit må vere atomisk"),
        Err("injected_failure")
    );
    assert_eq!(
        store.enqueue(1, "request", "channel", "Commit må vere atomisk"),
        Err("storage_error")
    );
    assert_eq!(
        store.snapshot().unwrap()["drafts"][0]["body"],
        "Commit må vere atomisk"
    );
    assert_eq!(store.snapshot().unwrap()["outbox"], json!([]));
    // A readable snapshot alone would not prove that the failed commit left
    // the connection able to start and commit another write transaction.
    store
        .save_draft(1, "write-after-failure", "Connection remains writable")
        .unwrap();
    drop(store);
    let mut reopened = Store::open(&fixture.database, "fixture-alice").unwrap();
    assert_eq!(reopened.snapshot().unwrap()["outbox"], json!([]));
    assert_eq!(
        reopened.snapshot().unwrap()["drafts"][1]["body"],
        "Connection remains writable"
    );
    assert_eq!(
        reopened.snapshot().unwrap()["drafts"][0]["body"],
        "Commit må vere atomisk"
    );
    reopened.execute_test_sql("drop trigger fail_commit");
    reopened
        .enqueue(1, "request", "channel", "Commit må vere atomisk")
        .unwrap();
}

#[test]
fn armed_fault_is_consumed_by_validation_or_duplicate_fast_path() {
    use crate::fault::{Fault, Mode, Site};
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "fixture-alice").unwrap();
    store
        .arm_fault(
            1,
            Fault {
                site: Site::BeforeCommit,
                mode: Mode::Error,
            },
        )
        .unwrap();
    assert_eq!(
        store.enqueue(1, "", "channel", "Hei"),
        Err("invalid_identifier")
    );
    store.enqueue(1, "first", "channel", "Hei").unwrap();
    store
        .arm_fault(
            1,
            Fault {
                site: Site::BeforeCommit,
                mode: Mode::Error,
            },
        )
        .unwrap();
    store.enqueue(1, "first", "channel", "Hei").unwrap();
    store.enqueue(1, "second", "channel", "Hei igjen").unwrap();
}

#[test]
#[cfg(not(feature = "fault-injection"))]
fn ordinary_abi_rejects_diagnostic_requests() {
    assert_eq!(
        call(
            json!({"op":"arm_fault","handle":1,"generation":1,"site":"before_commit","mode":"pause"})
        )["error"],
        "invalid_request"
    );
}

#[test]
fn only_server_acknowledged_sends_can_use_automatic_forget() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "native-pilot").unwrap();
    for index in 0..20 {
        let id = format!("request-{index}");
        store.enqueue(1, &id, "channel", "Hei").unwrap();
        assert_eq!(
            store.delivery(1, &id, "forget"),
            Err("invalid_delivery_transition")
        );
        store.delivery(1, &id, "dispatch").unwrap();
        store.delivery(1, &id, "uncertain").unwrap();
        assert_eq!(
            store.delivery(1, &id, "forget"),
            Err("invalid_delivery_transition")
        );
        store.delivery(1, &id, "dispatch").unwrap();
        store.delivery(1, &id, "acknowledge").unwrap();
        store.delivery(1, &id, "forget").unwrap();
    }
    assert!(
        store.snapshot().unwrap()["outbox"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn discard_rejected_only_removes_the_named_terminal_rejection() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "alice").unwrap();
    store
        .save_draft(1, "draft-channel", "Keep this draft")
        .unwrap();
    store
        .apply_message(1, "message", 1, "Keep history", false)
        .unwrap();
    let mut prior = None;
    for (id, actions) in [
        ("queued", vec![]),
        ("dispatching", vec!["dispatch"]),
        ("uncertain", vec!["dispatch", "uncertain"]),
        ("acknowledged", vec!["dispatch", "acknowledge"]),
        ("rejected-other", vec!["dispatch", "reject"]),
        ("rejected-target", vec!["dispatch", "reject"]),
    ] {
        store.enqueue(1, id, "channel", "Payload").unwrap();
        for action in actions {
            store.delivery(1, id, action).unwrap();
        }
        if !id.starts_with("rejected") {
            let before = store.snapshot().unwrap();
            assert_eq!(
                store.delivery(1, id, "discard_rejected"),
                Err("invalid_delivery_transition")
            );
            assert_eq!(store.snapshot().unwrap(), before);
        }
        prior = Some(store.snapshot().unwrap());
    }
    assert_eq!(
        store.delivery(1, "missing", "discard_rejected"),
        Err("unknown_request")
    );
    assert_eq!(
        store.delivery(2, "rejected-target", "discard_rejected"),
        Err("stale_generation")
    );
    let mut expected = prior.unwrap();
    expected["outbox"]
        .as_array_mut()
        .unwrap()
        .retain(|item| item["request_id"] != "rejected-target");
    store
        .delivery(1, "rejected-target", "discard_rejected")
        .unwrap();
    assert_eq!(store.snapshot().unwrap(), expected);
    assert_eq!(
        store.delivery(1, "rejected-target", "discard_rejected"),
        Err("unknown_request")
    );
    drop(store);
    let reopened = Store::open(&fixture.database, "alice").unwrap();
    // Opening converts the interrupted dispatch to uncertain, independently
    // of discard. Every other field remains unchanged across process lifetime.
    for item in expected["outbox"].as_array_mut().unwrap() {
        if item["state"] == "dispatching" {
            item["state"] = json!("uncertain");
        }
    }
    assert_eq!(reopened.snapshot().unwrap(), expected);
}

#[test]
fn abi_discard_rejected_is_explicit_and_additive_for_legacy_and_revisioned_handles() {
    for revisioned in [false, true] {
        let fixture = Fixture::new();
        let handle = call(json!({"op":"open","database":fixture.database,"account":"alice","draft_revisions":revisioned}))["result"]["handle"].as_u64().unwrap();
        let enqueue = |id: &str| {
            let mut request = json!({"op":"enqueue","handle":handle,"generation":1,"channel":"channel","body":"Payload","request_id":id});
            if revisioned {
                request["expected_draft_revision"] = json!(0);
            }
            assert_eq!(call(request)["ok"], true);
        };
        let delivery = |id: &str, action: &str| {
            call(
                json!({"op":"delivery","handle":handle,"generation":1,"request_id":id,"action":action}),
            )
        };
        let snapshot = || call(json!({"op":"snapshot","handle":handle}))["result"].clone();
        for (id, actions) in [
            ("queued", vec![]),
            ("dispatching", vec!["dispatch"]),
            ("uncertain", vec!["dispatch", "uncertain"]),
            ("acknowledged", vec!["dispatch", "acknowledge"]),
            ("rejected", vec!["dispatch", "reject"]),
        ] {
            enqueue(id);
            for action in actions {
                assert_eq!(delivery(id, action)["ok"], true);
            }
            if id != "rejected" {
                let before = snapshot();
                assert_eq!(
                    delivery(id, "discard_rejected")["error"],
                    "invalid_delivery_transition"
                );
                assert_eq!(snapshot(), before);
            }
        }
        let mut expected = snapshot();
        expected["outbox"]
            .as_array_mut()
            .unwrap()
            .retain(|item| item["request_id"] != "rejected");
        assert_eq!(delivery("rejected", "discard_rejected")["ok"], true);
        assert_eq!(snapshot(), expected);
        // Existing acknowledged cleanup remains its own unchanged action.
        assert_eq!(
            delivery("queued", "forget")["error"],
            "invalid_delivery_transition"
        );
        assert_eq!(delivery("acknowledged", "forget")["ok"], true);
        assert_eq!(call(json!({"op":"close","handle":handle}))["ok"], true);
    }
}

#[test]
fn explicit_discard_rejected_releases_exactly_one_full_queue_slot() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "alice").unwrap();
    for index in 0..8 {
        let id = format!("rejected-{index}");
        store
            .enqueue(1, &id, "channel", "Rejected payload")
            .unwrap();
        store.delivery(1, &id, "dispatch").unwrap();
        store.delivery(1, &id, "reject").unwrap();
    }
    store.save_draft(1, "channel", "New draft").unwrap();
    assert_eq!(
        store.enqueue(1, "new-id", "channel", "New draft"),
        Err("capacity")
    );
    store.delivery(1, "rejected-3", "discard_rejected").unwrap();
    assert_eq!(store.snapshot().unwrap()["drafts"][0]["body"], "New draft");
    store.enqueue(1, "new-id", "channel", "New draft").unwrap();
    let outbox = store.snapshot().unwrap()["outbox"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(outbox.len(), 8);
    assert_eq!(
        outbox
            .iter()
            .filter(|item| item["state"] == "rejected")
            .count(),
        7
    );
    assert_eq!(
        outbox
            .iter()
            .filter(|item| item["request_id"] == "new-id")
            .count(),
        1
    );
    assert_eq!(
        store.enqueue(1, "overflow", "channel", "Another send"),
        Err("capacity")
    );
}

#[test]
fn cleared_channel_drafts_do_not_exhaust_live_storage_slots() {
    let fixture = Fixture::new();
    let mut store = Store::open(&fixture.database, "native-pilot").unwrap();
    for index in 0..20 {
        let channel = format!("channel-{index}");
        store.save_draft(1, &channel, "eit utkast").unwrap();
        assert_eq!(store.clear_draft(2, &channel), Err("stale_generation"));
        store.clear_draft(1, &channel).unwrap();
    }
    assert!(
        store.snapshot().unwrap()["drafts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn revision_opt_in_rejects_unbound_admission_and_never_reuses_a_cleared_revision() {
    let fixture = Fixture::new();
    let mut store = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
    let first = store.save_draft(1, "channel", "Same body").unwrap();
    assert_eq!(
        store.enqueue(1, "unbound", "channel", "Same body"),
        Err("draft_revision_required")
    );
    assert_eq!(
        store.enqueue_draft(1, "future", "channel", "Same body", Some(first + 100)),
        Err("invalid_revision")
    );
    store
        .enqueue_draft(1, "first", "channel", "Same body", Some(first))
        .unwrap();
    assert_eq!(store.snapshot().unwrap()["drafts"], json!([]));
    let second = store.save_draft(1, "channel", "Same body").unwrap();
    assert!(second > first);
    let cleared = store.clear_draft(1, "channel").unwrap();
    assert!(cleared > second);
    drop(store);
    let mut reopened = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
    let third = reopened.save_draft(1, "channel", "Same body").unwrap();
    assert!(third > cleared);
    reopened
        .enqueue_draft(1, "old-capture", "channel", "Same body", Some(first))
        .unwrap();
    assert_eq!(reopened.snapshot().unwrap()["drafts"][0]["revision"], third);
    assert_eq!(
        reopened.enqueue_draft(1, "first", "channel", "Same body", Some(third)),
        Err("request_conflict")
    );
}

#[test]
fn lost_enqueue_results_preserve_newer_identical_drafts_without_repair_writes() {
    use crate::fault::{Fault, Mode, Site};
    for site in [Site::AfterInsert, Site::BeforeCommit, Site::AfterCommit] {
        let fixture = Fixture::new();
        let mut store = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
        let captured = store.save_draft(1, "channel", "Same body").unwrap();
        store
            .arm_fault(
                1,
                Fault {
                    site,
                    mode: Mode::Error,
                },
            )
            .unwrap();
        let error = if site == Site::AfterCommit {
            "injected_unknown_outcome"
        } else {
            "injected_failure"
        };
        assert_eq!(
            store.enqueue_draft(1, "request", "channel", "Same body", Some(captured)),
            Err(error)
        );
        store.save_draft(1, "channel", "Away").unwrap();
        let newer = store.save_draft(1, "channel", "Same body").unwrap();
        store
            .enqueue_draft(1, "request", "channel", "Same body", Some(captured))
            .unwrap();
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["drafts"][0]["revision"], newer);
        assert_eq!(snapshot["drafts"][0]["body"], "Same body");
        assert_eq!(snapshot["outbox"].as_array().unwrap().len(), 1);
        let payload = snapshot["outbox"][0]["payload"].clone();
        drop(store);
        let mut store = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
        store
            .enqueue_draft(1, "request", "channel", "Same body", Some(captured))
            .unwrap();
        assert_eq!(store.snapshot().unwrap()["drafts"][0]["revision"], newer);
        assert_eq!(store.snapshot().unwrap()["outbox"][0]["payload"], payload);
    }
}

fn seed_v1(fixture: &Fixture) -> String {
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    connection.execute_batch("PRAGMA application_id=1397772880; PRAGMA user_version=1;
        create table identity(id integer primary key,account text not null,generation integer not null,active integer not null);
        create table drafts(channel text primary key,body text not null);
        create table outbox(request_id text primary key,channel text not null,payload text not null,state text not null);
        create table messages(id text primary key,revision integer not null,body text not null,deleted integer not null);
        insert into identity values(1,'alice',1,1);
        insert into drafts values('channel','Legacy draft');
        insert into messages values('message',7,'Keep history',0);").unwrap();
    let payload = "{\"unchanged\":\"legacy payload bytes\"}".to_owned();
    connection
        .execute(
            "insert into outbox values('legacy-id','channel',?1,'dispatching')",
            [&payload],
        )
        .unwrap();
    payload
}

#[test]
fn schema_one_migration_preserves_drafts_outbox_ids_payloads_and_delivery_recovery() {
    let fixture = Fixture::new();
    let payload = seed_v1(&fixture);
    assert_eq!(
        Store::open_with_draft_revisions(&fixture.database, "bob", true).err(),
        Some("account_mismatch")
    );
    {
        let connection = rusqlite::Connection::open(&fixture.database).unwrap();
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
    let mut store = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot["schema"], 3);
    assert_eq!(
        snapshot["drafts"][0],
        json!({"channel":"channel","body":"Legacy draft","revision":1})
    );
    assert_eq!(snapshot["outbox"][0]["request_id"], "legacy-id");
    assert_eq!(snapshot["outbox"][0]["payload"], payload);
    assert_eq!(snapshot["outbox"][0]["state"], "uncertain");
    assert_eq!(snapshot["messages"][0]["revision"], 7);
    store
        .enqueue_draft(1, "new-id", "channel", "Legacy draft", Some(1))
        .unwrap();
    assert_eq!(store.snapshot().unwrap()["drafts"], json!([]));
    assert!(store.save_draft(1, "channel", "New draft").unwrap() > 1);
    drop(store);
    let store = Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
    assert_eq!(
        store.snapshot().unwrap()["outbox"][0]["request_id"],
        "legacy-id"
    );
}

fn seed_v2(fixture: &Fixture) -> &'static str {
    seed_v1(fixture);
    // The schema-2 writer used serde_json's sorted keys and omitted the parent.
    const ROOT: &str = r#"{"payload":{"body":"Legacy draft","channel":{"type":"id","value":"channel"}},"protocol":"sproyt.chat.v1","request_id":"legacy-id","type":"send_message"}"#;
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    connection
        .execute_batch(
            "alter table identity add column draft_clock integer not null default 17;
        alter table drafts add column revision integer not null default 16;
        alter table outbox add column draft_revision integer;
        PRAGMA user_version=2;",
        )
        .unwrap();
    connection
        .execute("update outbox set payload=?1,draft_revision=16", [ROOT])
        .unwrap();
    ROOT
}

#[test]
fn schema_two_root_replays_byte_identically_without_consuming_migrated_draft() {
    let fixture = Fixture::new();
    let root = seed_v2(&fixture);
    let mut store = Store::open_with_options(&fixture.database, "alice", true, true).unwrap();
    store
        .enqueue_target(1, "legacy-id", "channel", None, "Legacy draft", Some(16))
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(
        snapshot["drafts"][0],
        json!({"channel":"channel","body":"Legacy draft","revision":16})
    );
    assert_eq!(snapshot["outbox"][0]["payload"], root);
    assert_eq!(snapshot["outbox"][0]["state"], "uncertain");
    assert_eq!(
        store
            .save_target_draft(1, "channel", Some("t"), "New")
            .unwrap(),
        18
    );
}

#[test]
fn schema_two_bad_row_rolls_back_the_table_copy_and_version() {
    let fixture = Fixture::new();
    let root = seed_v2(&fixture);
    // SQLite's old non-integer primary key allows NULL on a damaged database.
    {
        let db = rusqlite::Connection::open(&fixture.database).unwrap();
        db.execute(
            "insert into drafts(channel,body,revision) values(null,'bad',15)",
            [],
        )
        .unwrap();
    }
    assert_eq!(
        Store::open_with_options(&fixture.database, "alice", true, true).err(),
        Some("storage_error")
    );
    let db = rusqlite::Connection::open(&fixture.database).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row("select count(*) from drafts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "select payload from outbox where state='dispatching'",
            [],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        root
    );
    assert_eq!(
        db.query_row(
            "select count(*) from sqlite_master where name='drafts_v2'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn thread_outbox_shares_root_capacity_and_rolls_back_on_precommit_error() {
    use crate::fault::{Fault, Mode, Site};
    let fixture = Fixture::new();
    let mut store = Store::open_with_options(&fixture.database, "alice", true, true).unwrap();
    let body = "ø".repeat(512);
    let root = store.save_draft(1, "channel", &body).unwrap();
    let revision = store
        .save_target_draft(1, "channel", Some("t"), &body)
        .unwrap();
    for site in [Site::AfterInsert, Site::BeforeCommit] {
        store
            .arm_fault(
                1,
                Fault {
                    site,
                    mode: Mode::Error,
                },
            )
            .unwrap();
        assert_eq!(
            store.enqueue_target(1, "reply", "channel", Some("t"), &body, Some(revision)),
            Err("injected_failure")
        );
        let snapshot = store.snapshot().unwrap();
        assert_eq!(snapshot["drafts"].as_array().unwrap().len(), 2);
        assert_eq!(snapshot["outbox"], json!([]));
    }
    store
        .enqueue_target(1, "reply", "channel", Some("t"), &body, Some(revision))
        .unwrap();
    for i in 1..8 {
        store
            .enqueue_target(1, &format!("other-{i}"), "channel", None, &body, Some(root))
            .unwrap();
    }
    let ninth = store
        .save_target_draft(1, "channel", Some("t2"), "Keep")
        .unwrap();
    assert_eq!(
        store.enqueue_target(1, "ninth", "channel", Some("t2"), "Keep", Some(ninth)),
        Err("capacity")
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot["outbox"].as_array().unwrap().len(), 8);
    assert_eq!(snapshot["drafts"][0]["parent_message_id"], "t2");
}

#[test]
fn thread_targets_keep_drafts_payload_and_revision_independent_across_restart() {
    let fixture = Fixture::new();
    let mut store = Store::open_with_options(&fixture.database, "alice", true, true).unwrap();
    let root = store
        .save_target_draft(1, "channel", None, "Same body")
        .unwrap();
    let first = store
        .save_target_draft(1, "channel", Some("thread-1"), "Same body")
        .unwrap();
    store
        .save_target_draft(1, "channel", Some("thread-2"), "Same body")
        .unwrap();
    store
        .enqueue_target(
            1,
            "reply-id",
            "channel",
            Some("thread-1"),
            "Same body",
            Some(first),
        )
        .unwrap();
    let newer = store
        .save_target_draft(1, "channel", Some("thread-1"), "Same body")
        .unwrap();
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot["drafts"].as_array().unwrap().len(), 3);
    let wire: Value =
        serde_json::from_str(snapshot["outbox"][0]["payload"].as_str().unwrap()).unwrap();
    assert_eq!(wire["payload"]["channel"]["value"], "channel");
    assert_eq!(wire["payload"]["parent_message_id"], "thread-1");
    let exact_payload = snapshot["outbox"][0]["payload"].clone();
    store.delivery(1, "reply-id", "dispatch").unwrap();
    drop(store);
    assert_eq!(
        Store::open_with_draft_revisions(&fixture.database, "alice", true).err(),
        Some("thread_targets_required")
    );
    let mut store = Store::open_with_options(&fixture.database, "alice", true, true).unwrap();
    assert_eq!(store.snapshot().unwrap()["outbox"][0]["state"], "uncertain");
    store
        .enqueue_target(
            1,
            "reply-id",
            "channel",
            Some("thread-1"),
            "Same body",
            Some(first),
        )
        .unwrap();
    assert_eq!(
        store.enqueue_target(
            1,
            "reply-id",
            "channel",
            Some("thread-2"),
            "Same body",
            Some(first)
        ),
        Err("request_conflict")
    );
    assert_eq!(
        store.enqueue_target(1, "reply-id", "channel", None, "Same body", Some(first)),
        Err("request_conflict")
    );
    let snapshot = store.snapshot().unwrap();
    assert_eq!(snapshot["outbox"][0]["payload"], exact_payload);
    assert_eq!(
        snapshot["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["parent_message_id"] == "thread-1")
            .unwrap()["revision"],
        newer
    );
    assert_eq!(
        snapshot["drafts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["parent_message_id"].is_null())
            .unwrap()["revision"],
        root
    );
    store
        .clear_target_draft(1, "channel", Some("thread-2"))
        .unwrap();
    assert_eq!(
        store.snapshot().unwrap()["drafts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn thread_target_limits_and_post_commit_loss_do_not_change_the_destination() {
    let fixture = Fixture::new();
    let mut store = Store::open_with_options(&fixture.database, "alice", true, true).unwrap();
    let revision = store
        .save_target_draft(1, "channel", Some("parent"), "Reply")
        .unwrap();
    store
        .arm_fault(
            1,
            crate::fault::Fault {
                site: crate::fault::Site::AfterCommit,
                mode: crate::fault::Mode::Error,
            },
        )
        .unwrap();
    assert_eq!(
        store.enqueue_target(
            1,
            "reply",
            "channel",
            Some("parent"),
            "Reply",
            Some(revision)
        ),
        Err("injected_unknown_outcome")
    );
    let payload = store.snapshot().unwrap()["outbox"][0]["payload"].clone();
    store
        .enqueue_target(
            1,
            "reply",
            "channel",
            Some("parent"),
            "Reply",
            Some(revision),
        )
        .unwrap();
    assert_eq!(store.snapshot().unwrap()["outbox"][0]["payload"], payload);
    assert_eq!(
        store.save_target_draft(1, "channel", Some("parent"), &"ø".repeat(513)),
        Err("text_too_large")
    );
    for i in 0..8 {
        store
            .save_target_draft(1, "channel", Some(&format!("parent-{i}")), "draft")
            .unwrap();
    }
    assert_eq!(
        store.save_target_draft(1, "channel", None, "root"),
        Err("capacity")
    );
    store
        .clear_target_draft(1, "channel", Some("parent-0"))
        .unwrap();
    assert!(store.save_target_draft(1, "channel", None, "root").is_ok());
}

#[test]
fn failed_schema_migration_rolls_back_all_column_and_version_changes() {
    let fixture = Fixture::new();
    let payload = seed_v1(&fixture);
    {
        let connection = rusqlite::Connection::open(&fixture.database).unwrap();
        // Force the final ALTER to fail after the earlier ALTERs ran.
        connection
            .execute_batch("alter table outbox add column draft_revision integer")
            .unwrap();
    }
    assert_eq!(
        Store::open_with_draft_revisions(&fixture.database, "alice", true).err(),
        Some("storage_error")
    );
    let connection = rusqlite::Connection::open(&fixture.database).unwrap();
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(
        connection
            .prepare("select draft_clock from identity")
            .is_err()
    );
    assert!(connection.prepare("select revision from drafts").is_err());
    assert_eq!(
        connection
            .query_row("select payload from outbox", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        payload
    );
}

#[test]
fn abi_revision_opt_in_is_explicit_while_legacy_admission_keeps_its_contract() {
    for revisioned in [false, true] {
        let fixture = Fixture::new();
        let handle = call(json!({"op":"open","database":fixture.database,"account":"alice","draft_revisions":revisioned}))["result"]["handle"].as_u64().unwrap();
        let saved = call(
            json!({"op":"save_draft","handle":handle,"generation":1,"channel":"channel","body":"Same body"}),
        );
        let revision = saved["result"]["revision"].as_u64().unwrap();
        let mut request = json!({"op":"enqueue","handle":handle,"generation":1,"channel":"channel","body":"Same body","request_id":"request"});
        if revisioned {
            assert_eq!(call(request.clone())["error"], "draft_revision_required");
            request["expected_draft_revision"] = json!(revision);
        }
        assert_eq!(call(request)["ok"], true);
        assert_eq!(
            call(json!({"op":"snapshot","handle":handle}))["result"]["drafts"],
            json!([])
        );
        assert_eq!(call(json!({"op":"close","handle":handle}))["ok"], true);
    }
}

#[test]
#[cfg(feature = "fault-injection")]
fn revision_crash_child() {
    use crate::fault::{Fault, Mode, Site};
    let Some(path) = std::env::var_os("SPROYT_DRAFT_CRASH_DATABASE") else {
        return;
    };
    let site: Site = serde_json::from_str(&format!(
        "\"{}\"",
        std::env::var("SPROYT_DRAFT_CRASH_SITE").unwrap()
    ))
    .unwrap();
    let newer = std::env::var("SPROYT_DRAFT_CRASH_NEWER").unwrap() == "true";
    let mut store =
        Store::open_with_draft_revisions(path.to_str().unwrap(), "alice", true).unwrap();
    let captured = store.save_draft(1, "channel", "Same body").unwrap();
    if newer {
        store.save_draft(1, "channel", "Away").unwrap();
        store.save_draft(1, "channel", "Same body").unwrap();
    }
    store
        .arm_fault(
            1,
            Fault {
                site,
                mode: Mode::Pause,
            },
        )
        .unwrap();
    store
        .enqueue_draft(1, "request", "channel", "Same body", Some(captured))
        .unwrap();
    panic!("Diagnostic pause returned instead of waiting for process termination");
}

#[test]
#[cfg(feature = "fault-injection")]
fn hard_process_death_at_each_commit_boundary_preserves_revision_and_exact_replay() {
    use std::{
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::mpsc,
        time::Duration,
    };
    struct ChildGuard(Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for site in ["after_insert", "before_commit", "after_commit"] {
        for newer in [false, true] {
            let fixture = Fixture::new();
            let mut child = ChildGuard(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "tests::revision_crash_child", "--nocapture"])
                    .env("SPROYT_DRAFT_CRASH_DATABASE", &fixture.database)
                    .env("SPROYT_DRAFT_CRASH_SITE", site)
                    .env("SPROYT_DRAFT_CRASH_NEWER", newer.to_string())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let stdout = child.0.stdout.take().unwrap();
            let (sender, receiver) = mpsc::channel();
            let reader = std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let line = line.unwrap();
                    if let Some(marker) = line.strip_prefix("PROBE_FAULT ") {
                        sender.send(marker.to_owned()).unwrap();
                        return;
                    }
                }
            });
            let marker: Value = serde_json::from_str(
                &receiver
                    .recv_timeout(Duration::from_secs(15))
                    .expect("Child never reached the requested commit boundary"),
            )
            .unwrap();
            assert_eq!(marker["site"], site);
            child.0.kill().unwrap();
            assert!(!child.0.wait().unwrap().success());
            reader.join().unwrap();
            let mut store =
                Store::open_with_draft_revisions(&fixture.database, "alice", true).unwrap();
            let before_retry = store.snapshot().unwrap();
            let committed = site == "after_commit";
            assert_eq!(
                before_retry["outbox"].as_array().unwrap().len(),
                usize::from(committed)
            );
            if newer || !committed {
                assert_eq!(before_retry["drafts"][0]["body"], "Same body");
                assert_eq!(
                    before_retry["drafts"][0]["revision"],
                    if newer { 4 } else { 2 }
                );
            } else {
                assert_eq!(before_retry["drafts"], json!([]));
            }
            for _ in 0..2 {
                store
                    .enqueue_draft(1, "request", "channel", "Same body", Some(2))
                    .unwrap();
            }
            let after_retry = store.snapshot().unwrap();
            assert_eq!(after_retry["outbox"].as_array().unwrap().len(), 1);
            assert_eq!(after_retry["outbox"][0]["payload"], marker["payload"]);
            if newer {
                assert_eq!(after_retry["drafts"], before_retry["drafts"]);
            } else {
                assert_eq!(after_retry["drafts"], json!([]));
            }
        }
    }
}
