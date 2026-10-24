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
        let directory = std::env::temp_dir().join(format!(
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
    store.execute_test_sql("PRAGMA user_version=2");
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
