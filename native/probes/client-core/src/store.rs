use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};
use sproyt_client_core::{DurableSendEvent, DurableSendState, SendAdmission, transition};
use std::{path::Path, time::Duration};

pub const TEXT_LIMIT: usize = 1024;
const ITEM_LIMIT: i64 = 8;
const APPLICATION_ID: i64 = 0x53505250;
pub type Result<T> = std::result::Result<T, &'static str>;

pub struct Store {
    connection: Connection,
    draft_revisions: bool,
    thread_targets: bool,
    upload_drafts: bool,
    #[cfg(any(test, feature = "fault-injection"))]
    fault: Option<crate::fault::Fault>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Composer {
    pub text: String,
    pub attachments: Vec<String>,
}

impl Composer {
    fn validate(&self, body: &str) -> Result<()> {
        text(body)?;
        let mut wire = self.text.clone();
        for marker in &self.attachments {
            validate_marker(marker)?;
            if !wire.is_empty() && !wire.ends_with('\n') {
                wire.push('\n');
            }
            wire.push_str(marker);
            text(&wire)?;
        }
        if wire != body {
            return Err("invalid_composer");
        }
        Ok(())
    }
}

pub struct DraftAttachment<'a> {
    pub job_id: &'a str,
    pub operation_id: &'a str,
    pub receipt_only: bool,
    pub channel: &'a str,
    pub parent: Option<&'a str>,
    pub body: &'a str,
    pub marker: &'a str,
    pub composer: Option<&'a Composer>,
}

fn db<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|error| match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            "store_busy"
        }
        _ => "storage_error",
    })
}

pub fn identifier(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        return Err("invalid_identifier");
    }
    Ok(())
}

pub fn text(value: &str) -> Result<()> {
    if value.len() > TEXT_LIMIT {
        return Err("text_too_large");
    }
    Ok(())
}

fn validate_marker(marker: &str) -> Result<()> {
    text(marker)?;
    let fields = marker
        .strip_prefix("[[media:")
        .and_then(|s| s.strip_suffix("]]"))
        .ok_or("invalid_media_marker")?;
    let mut parts = fields.split('|');
    let id = parts.next().unwrap_or_default();
    let mime = parts.next().unwrap_or_default();
    let valid_id = id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        });
    let valid_mime = mime.len() <= 128
        && mime.split_once('/').is_some_and(|(major, minor)| {
            !major.is_empty() && !minor.is_empty() && !minor.contains('/')
        })
        && mime
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/+.-".contains(&b));
    if !valid_id
        || !valid_mime
        || parts.next().is_none()
        || parts.next().is_some()
        || fields.contains(['[', ']'])
        || fields.chars().any(char::is_control)
    {
        return Err("invalid_media_marker");
    }
    Ok(())
}

impl Store {
    #[cfg(test)]
    pub fn open(path: &str, account: &str) -> Result<Self> {
        Self::open_with_draft_revisions(path, account, false)
    }

    #[cfg(test)]
    pub fn open_with_draft_revisions(
        path: &str,
        account: &str,
        draft_revisions: bool,
    ) -> Result<Self> {
        Self::open_with_options(path, account, draft_revisions, false)
    }

    #[cfg(test)]
    pub fn open_with_options(
        path: &str,
        account: &str,
        draft_revisions: bool,
        thread_targets: bool,
    ) -> Result<Self> {
        Self::open_with_capabilities(path, account, draft_revisions, thread_targets, false)
    }

    pub fn open_with_capabilities(
        path: &str,
        account: &str,
        draft_revisions: bool,
        thread_targets: bool,
        upload_drafts: bool,
    ) -> Result<Self> {
        if upload_drafts && !(draft_revisions && thread_targets) {
            return Err("upload_drafts_require_targets");
        }
        identifier(account)?;
        if !Path::new(path).is_absolute() || path.len() > 4096 {
            return Err("invalid_path");
        }
        let mut connection = db(Connection::open(path))?;
        db(connection.busy_timeout(Duration::from_millis(250)))?;
        db(connection.pragma_update(None, "synchronous", "FULL"))?;
        // A probe has one owner, even across processes. This is deliberately
        // not a production concurrency or multi-process architecture decision.
        db(connection.pragma_update(None, "locking_mode", "EXCLUSIVE"))?;
        let tx =
            db(connection.transaction_with_behavior(rusqlite::TransactionBehavior::Exclusive))?;
        let version: i64 = db(tx.pragma_query_value(None, "user_version", |row| row.get(0)))?;
        let application: i64 = db(tx.pragma_query_value(None, "application_id", |row| row.get(0)))?;
        if !(0..=4).contains(&version) || (version != 0 && application != APPLICATION_ID) {
            return Err("unsupported_schema");
        }
        if version == 0 {
            let tables: i64 = db(tx.query_row(
                "select count(*) from sqlite_master where type='table' and name not like 'sqlite_%'",
                [], |row| row.get(0),
            ))?;
            if application != 0 || tables != 0 {
                return Err("unsupported_schema");
            }
            db(tx.execute_batch("create table identity(id integer primary key check(id=1), account text not null, generation integer not null, active integer not null);
                create table drafts(channel text primary key, body text not null);
                create table outbox(request_id text primary key, channel text not null, payload text not null, state text not null check(state in ('queued','dispatching','uncertain','acknowledged','rejected')));
                create table messages(id text primary key, revision integer not null, body text not null, deleted integer not null);"))?;
            db(tx.execute("insert into identity values(1,?1,1,1)", [account]))?;
            db(tx.pragma_update(None, "application_id", APPLICATION_ID))?;
            db(tx.pragma_update(None, "user_version", 1))?;
        }
        let owner: String = db(tx.query_row(
            "select account from identity where id=1",
            [],
            |row| row.get(0),
        ))?;
        if owner != account {
            return Err("account_mismatch");
        }
        if version < 2 {
            // Migrate in the same exclusive transaction as the identity check.
            // Revision 1 names existing drafts; the identity clock survives
            // removal/recreation so an old capture can never match a new draft.
            db(tx.execute_batch("alter table identity add column draft_clock integer not null default 1 check(draft_clock>=1);
                alter table drafts add column revision integer not null default 1 check(revision>=1);
                alter table outbox add column draft_revision integer;
                PRAGMA user_version=2;"))?;
        }
        if version < 3 {
            db(tx.execute_batch("alter table drafts rename to drafts_v2;
                create table drafts(channel text not null, parent_message_id text not null default '', body text not null, revision integer not null check(revision>=1), primary key(channel,parent_message_id));
                insert into drafts(channel,parent_message_id,body,revision) select channel,'',body,revision from drafts_v2;
                drop table drafts_v2;
                PRAGMA user_version=3;"))?;
        }
        if version < 4 {
            db(tx.execute_batch("alter table drafts add column composer_text text; alter table drafts add column attachments_json text; create table upload_receipts(job_id text primary key, operation_id text not null, channel text not null, parent_message_id text not null, marker text not null, draft_revision integer not null check(draft_revision>=1)); PRAGMA user_version=4;"))?;
        }
        if !upload_drafts
            && db(
                tx.query_row("select exists(select 1 from upload_receipts) or exists(select 1 from drafts where attachments_json is not null)", [], |row| {
                    row.get::<_, bool>(0)
                }),
            )?
        {
            return Err("upload_drafts_required");
        }
        if !thread_targets {
            let threads: i64 = db(tx.query_row(
                "select count(*) from drafts where parent_message_id<>''",
                [],
                |row| row.get(0),
            ))?;
            let sends: i64 = db(tx.query_row("select count(*) from outbox where json_extract(payload,'$.payload.parent_message_id') is not null", [], |row| row.get(0)))?;
            if threads != 0 || sends != 0 {
                return Err("thread_targets_required");
            }
        }
        // The pure core does not yet define a process-recovery event. This
        // adapter explicitly makes an interrupted dispatch uncertain.
        db(tx.execute(
            "update outbox set state='uncertain' where state='dispatching'",
            [],
        ))?;
        db(tx.commit())?;
        Ok(Self {
            connection,
            draft_revisions,
            thread_targets,
            upload_drafts,
            #[cfg(any(test, feature = "fault-injection"))]
            fault: None,
        })
    }

    fn transaction(&mut self, generation: u64) -> Result<Transaction<'_>> {
        let tx = db(self.connection.transaction())?;
        let (stored, active): (i64, bool) = db(tx.query_row(
            "select generation,active from identity where id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ))?;
        if stored < 1 || stored as u64 != generation || !active {
            return Err("stale_generation");
        }
        Ok(tx)
    }

    fn next_draft_revision(tx: &Transaction<'_>) -> Result<u64> {
        let clock: i64 = db(tx.query_row(
            "select draft_clock from identity where id=1",
            [],
            |row| row.get(0),
        ))?;
        let revision = clock.checked_add(1).ok_or("revision_capacity")?;
        db(tx.execute("update identity set draft_clock=?1 where id=1", [revision]))?;
        Ok(revision as u64)
    }

    #[cfg(test)]
    pub fn save_draft(&mut self, generation: u64, channel: &str, body: &str) -> Result<u64> {
        self.save_target_draft(generation, channel, None, body)
    }

    #[cfg(test)]
    pub fn save_target_draft(
        &mut self,
        generation: u64,
        channel: &str,
        parent: Option<&str>,
        body: &str,
    ) -> Result<u64> {
        self.save_composer(generation, channel, parent, body, None)
    }

    pub fn save_composer(
        &mut self,
        generation: u64,
        channel: &str,
        parent: Option<&str>,
        body: &str,
        composer: Option<&Composer>,
    ) -> Result<u64> {
        if let Some(value) = composer {
            if !self.upload_drafts {
                return Err("upload_drafts_required");
            }
            value.validate(body)?;
        }
        identifier(channel)?;
        self.validate_parent(parent)?;
        let parent = parent.unwrap_or("");
        text(body)?;
        let tx = self.transaction(generation)?;
        let revision = Self::write_draft(&tx, channel, parent, body, composer)?;
        db(tx.commit())?;
        Ok(revision)
    }

    fn write_draft(
        tx: &Transaction<'_>,
        channel: &str,
        parent: &str,
        body: &str,
        composer: Option<&Composer>,
    ) -> Result<u64> {
        let count: i64 = db(tx.query_row(
            "select count(*) from drafts where channel<>?1 or parent_message_id<>?2",
            params![channel, parent],
            |row| row.get(0),
        ))?;
        if count >= ITEM_LIMIT {
            return Err("capacity");
        }
        let revision = Self::next_draft_revision(tx)?;
        let with_attachments = composer.filter(|value| !value.attachments.is_empty());
        let editor_text = with_attachments.map(|value| value.text.as_str());
        let markers = with_attachments.map(|value| json!(value.attachments).to_string());
        db(tx.execute("insert into drafts(channel,parent_message_id,body,revision,composer_text,attachments_json) values(?1,?2,?3,?4,?5,?6)
            on conflict(channel,parent_message_id) do update set body=excluded.body,revision=excluded.revision,
            composer_text=excluded.composer_text,attachments_json=excluded.attachments_json", params![channel, parent, body, revision as i64, editor_text, markers]))?;
        Ok(revision)
    }

    pub fn attach_upload(
        &mut self,
        generation: u64,
        attachment: DraftAttachment<'_>,
    ) -> Result<Value> {
        if !self.upload_drafts {
            return Err("upload_drafts_required");
        }
        let DraftAttachment {
            job_id,
            operation_id,
            receipt_only,
            channel,
            parent,
            body,
            marker,
            composer,
        } = attachment;
        identifier(job_id)?;
        identifier(operation_id)?;
        identifier(channel)?;
        self.validate_parent(parent)?;
        validate_marker(marker)?;
        #[cfg(any(test, feature = "fault-injection"))]
        let fault = self.fault.take();
        let parent = parent.unwrap_or("");
        let tx = self.transaction(generation)?;
        let receipt: Option<(String,String,String,String,i64)> = db(tx.query_row(
            "select channel,parent_message_id,marker,operation_id,draft_revision from upload_receipts where job_id=?1",[job_id],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?))).optional())?;
        if let Some((saved_channel, saved_parent, saved_marker, original_operation, revision)) =
            receipt
        {
            if saved_channel != channel || saved_parent != parent || saved_marker != marker {
                return Err("request_conflict");
            }
            // Replaying after the draft was sent or replaced must never write or return an old draft as current.
            let current: Option<String> = db(tx.query_row("select body from drafts where channel=?1 and parent_message_id=?2 and revision=?3",
                params![channel,parent,revision], |r| r.get(0)).optional())?;
            return Ok(
                json!({"already_applied":true,"operation_id":original_operation,"body":current,"revision":revision}),
            );
        }
        if receipt_only {
            // Shutdown/retirement must not create a new draft after logout.
            return Err("receipt_not_found");
        }
        text(body)?;
        if db(
            tx.query_row("select count(*) from upload_receipts", [], |r| {
                r.get::<_, i64>(0)
            }),
        )? >= 16
        {
            return Err("receipt_capacity");
        }
        let separator = if body.is_empty() || body.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        let merged = format!("{body}{separator}{marker}");
        text(&merged)?;
        if let Some(value) = composer {
            value.validate(body)?;
        }
        let mut markers = composer
            .map(|value| value.attachments.clone())
            .unwrap_or_default();
        markers.push(marker.to_owned());
        let updated = Composer {
            text: composer
                .map_or(body, |value| value.text.as_str())
                .to_owned(),
            attachments: markers,
        };
        let revision = Self::write_draft(&tx, channel, parent, &merged, Some(&updated))?;
        db(tx.execute("insert into upload_receipts(job_id,operation_id,channel,parent_message_id,marker,draft_revision) values(?1,?2,?3,?4,?5,?6)",
            params![job_id,operation_id,channel,parent,marker,revision as i64]))?;
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::BeforeCommit, marker)?;
        db(tx.commit())?;
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::AfterCommit, marker)?;
        Ok(
            json!({"already_applied":false,"operation_id":operation_id,"body":merged,"revision":revision}),
        )
    }

    pub fn reconcile_upload_receipts(
        &mut self,
        generation: u64,
        active_job_ids: &[String],
    ) -> Result<()> {
        if !self.upload_drafts {
            return Err("upload_drafts_required");
        }
        if active_job_ids.len() > 16 {
            return Err("capacity");
        }
        for job in active_job_ids {
            identifier(job)?;
        }
        let active = serde_json::to_string(active_job_ids).map_err(|_| "serialization_error")?;
        let tx = self.transaction(generation)?;
        db(tx.execute(
            "delete from upload_receipts where job_id not in (select value from json_each(?1))",
            [active],
        ))?;
        db(tx.commit())
    }

    #[cfg(test)]
    pub fn clear_draft(&mut self, generation: u64, channel: &str) -> Result<u64> {
        self.clear_target_draft(generation, channel, None)
    }

    pub fn clear_target_draft(
        &mut self,
        generation: u64,
        channel: &str,
        parent: Option<&str>,
    ) -> Result<u64> {
        identifier(channel)?;
        self.validate_parent(parent)?;
        let tx = self.transaction(generation)?;
        let revision = Self::next_draft_revision(&tx)?;
        db(tx.execute(
            "delete from drafts where channel=?1 and parent_message_id=?2",
            params![channel, parent.unwrap_or("")],
        ))?;
        db(tx.commit())?;
        Ok(revision)
    }

    #[cfg(test)]
    pub fn enqueue(&mut self, generation: u64, id: &str, channel: &str, body: &str) -> Result<()> {
        self.enqueue_draft(generation, id, channel, body, None)
    }

    #[cfg(test)]
    pub fn enqueue_draft(
        &mut self,
        generation: u64,
        id: &str,
        channel: &str,
        body: &str,
        expected_revision: Option<u64>,
    ) -> Result<()> {
        self.enqueue_target(generation, id, channel, None, body, expected_revision)
    }

    fn validate_parent(&self, parent: Option<&str>) -> Result<()> {
        if let Some(parent) = parent {
            if !self.thread_targets {
                return Err("thread_targets_required");
            }
            identifier(parent)?;
        }
        Ok(())
    }

    pub fn enqueue_target(
        &mut self,
        generation: u64,
        id: &str,
        channel: &str,
        parent: Option<&str>,
        body: &str,
        expected_revision: Option<u64>,
    ) -> Result<()> {
        // One-shot means the next decoded enqueue attempt, even if validation
        // or duplicate-ID checks return before the chosen site is reached.
        #[cfg(any(test, feature = "fault-injection"))]
        let fault = self.fault.take();
        identifier(id)?;
        identifier(channel)?;
        self.validate_parent(parent)?;
        text(body)?;
        if self.draft_revisions && expected_revision.is_none() {
            return Err("draft_revision_required");
        }
        if expected_revision.is_some_and(|revision| revision > i64::MAX as u64) {
            return Err("invalid_revision");
        }
        let expected_sql = expected_revision.map(|revision| revision as i64);
        if body.trim().is_empty() {
            return Err("empty_body");
        }
        let mut content = json!({"channel":{"type":"id","value":channel},"body":body});
        if let Some(parent) = parent {
            content["parent_message_id"] = json!(parent);
        }
        let payload = serde_json::to_string(&json!({"protocol":"sproyt.chat.v1","request_id":id,
            "type":"send_message","payload":content}))
        .map_err(|_| "serialization_error")?;
        let tx = self.transaction(generation)?;
        if let Some(revision) = expected_sql {
            let clock: i64 = db(tx.query_row(
                "select draft_clock from identity where id=1",
                [],
                |row| row.get(0),
            ))?;
            if revision > clock {
                return Err("invalid_revision");
            }
        }
        let existing: Option<(String, Option<i64>)> = db(tx
            .query_row(
                "select payload,draft_revision from outbox where request_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional())?;
        if let Some((existing, revision)) = existing {
            // Replay is a read: it must never clear a draft created after the
            // original admission, including one with exactly the same body.
            return if existing == payload
                && (expected_revision.is_none() || revision == expected_sql)
            {
                Ok(())
            } else {
                Err("request_conflict")
            };
        }
        let count: i64 = db(tx.query_row("select count(*) from outbox", [], |row| row.get(0)))?;
        if count >= ITEM_LIMIT {
            return Err("capacity");
        }
        db(tx.execute(
            "insert into outbox(request_id,channel,payload,state,draft_revision) values(?1,?2,?3,'queued',?4)",
            params![id, channel, payload, expected_sql],
        ))?;
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::AfterInsert, &payload)?;
        // The legacy laboratory contract remains opt-out compatible. Updated
        // clients explicitly opt in and bind removal to the saved revision.
        if let Some(revision) = expected_revision {
            db(tx.execute(
                "delete from drafts where channel=?1 and parent_message_id=?2 and body=?3 and revision=?4",
                params![channel, parent.unwrap_or(""), body, revision as i64],
            ))?;
        } else {
            db(tx.execute(
                "delete from drafts where channel=?1 and parent_message_id=?2 and body=?3",
                params![channel, parent.unwrap_or(""), body],
            ))?;
        }
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::BeforeCommit, &payload)?;
        db(tx.commit())?;
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::AfterCommit, &payload)?;
        Ok(())
    }

    #[cfg(any(test, feature = "fault-injection"))]
    pub fn arm_fault(&mut self, generation: u64, fault: crate::fault::Fault) -> Result<()> {
        drop(self.transaction(generation)?);
        if self.fault.is_some() {
            return Err("fault_already_armed");
        }
        self.fault = Some(fault);
        Ok(())
    }

    pub fn delivery(&mut self, generation: u64, id: &str, action: &str) -> Result<()> {
        identifier(id)?;
        let tx = self.transaction(generation)?;
        let state: Option<String> = db(tx
            .query_row(
                "select state from outbox where request_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional())?;
        let state = state.ok_or("unknown_request")?;
        if action == "forget" || action == "discard_rejected" {
            // A terminal rejection may only be removed by the caller's
            // explicit discard action. No uncertain/unsent work is dropped.
            let required = if action == "forget" {
                "acknowledged"
            } else {
                "rejected"
            };
            if state != required {
                return Err("invalid_delivery_transition");
            }
            db(tx.execute("delete from outbox where request_id=?1", [id]))?;
            return db(tx.commit());
        }
        let from = match state.as_str() {
            "queued" => DurableSendState::Queued,
            "dispatching" => DurableSendState::Dispatching,
            "uncertain" => DurableSendState::Uncertain,
            "acknowledged" => DurableSendState::Acknowledged,
            "rejected" => DurableSendState::Rejected,
            _ => return Err("storage_error"),
        };
        let next = match action {
            "uncertain"
                if matches!(
                    from,
                    DurableSendState::Dispatching | DurableSendState::Uncertain
                ) =>
            {
                // A wire attempt may already have committed remotely. Keep
                // that uncertainty explicit while retaining the same ID.
                DurableSendState::Uncertain
            }
            "dispatch"
                if matches!(from, DurableSendState::Queued | DurableSendState::Uncertain) =>
            {
                transition(
                    DurableSendState::Queued,
                    DurableSendEvent::Persisted(SendAdmission::DispatchNow),
                )
            }
            "acknowledge"
                if matches!(
                    from,
                    DurableSendState::Dispatching
                        | DurableSendState::Uncertain
                        | DurableSendState::Acknowledged
                ) =>
            {
                transition(from, DurableSendEvent::Acknowledged)
            }
            "reject"
                if matches!(
                    from,
                    DurableSendState::Dispatching
                        | DurableSendState::Uncertain
                        | DurableSendState::Rejected
                ) =>
            {
                transition(from, DurableSendEvent::Rejected)
            }
            _ => return Err("invalid_delivery_transition"),
        };
        let next = match next {
            DurableSendState::Dispatching => "dispatching",
            DurableSendState::Acknowledged => "acknowledged",
            DurableSendState::Rejected => "rejected",
            DurableSendState::Uncertain => "uncertain",
            _ => return Err("invalid_delivery_transition"),
        };
        db(tx.execute(
            "update outbox set state=?1 where request_id=?2",
            params![next, id],
        ))?;
        db(tx.commit())
    }

    pub fn apply_message(
        &mut self,
        generation: u64,
        id: &str,
        revision: u64,
        body: &str,
        deleted: bool,
    ) -> Result<()> {
        identifier(id)?;
        text(body)?;
        if revision == 0 || revision > i64::MAX as u64 {
            return Err("invalid_revision");
        }
        let tx = self.transaction(generation)?;
        let count: i64 = db(tx.query_row(
            "select count(*) from messages where id<>?1",
            [id],
            |row| row.get(0),
        ))?;
        if count >= ITEM_LIMIT {
            return Err("capacity");
        }
        db(tx.execute("insert into messages values(?1,?2,?3,?4) on conflict(id) do update set revision=excluded.revision,body=excluded.body,deleted=excluded.deleted where excluded.revision>messages.revision", params![id,revision as i64,if deleted { "" } else { body },deleted]))?;
        db(tx.commit())
    }

    pub fn invalidate(&mut self, generation: u64) -> Result<()> {
        let tx = self.transaction(generation)?;
        db(tx.execute_batch("update identity set generation=generation+1,active=0 where id=1; delete from drafts; delete from upload_receipts; delete from outbox; delete from messages;"))?;
        db(tx.commit())
    }

    pub fn snapshot(&self) -> Result<Value> {
        let (account, generation, active): (String, i64, bool) = db(self.connection.query_row(
            "select account,generation,active from identity where id=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ))?;
        let mut drafts = db(self
            .connection
            .prepare("select channel,body,revision,parent_message_id,composer_text,attachments_json from drafts order by channel,parent_message_id limit 8"))?;
        let drafts = db(db(drafts.query_map([], |row| {
            let mut draft = json!({"channel":row.get::<_,String>(0)?,"body":row.get::<_,String>(1)?,"revision":row.get::<_,i64>(2)?});
            let parent: String = row.get(3)?;
            if let Some(markers) = row.get::<_, Option<String>>(5)? {
                draft["composer_text"] = json!(row.get::<_,String>(4)?);
                draft["attachments"] = serde_json::from_str(&markers).map_err(|error| rusqlite::Error::FromSqlConversionFailure(5,rusqlite::types::Type::Text,Box::new(error)))?;
            }
            if !parent.is_empty() { draft["parent_message_id"] = json!(parent); }
            Ok(draft)
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        let mut outbox = db(self
            .connection
            .prepare("select request_id,payload,state from outbox order by request_id limit 8"))?;
        let outbox = db(db(outbox.query_map([], |row| Ok(json!({"request_id":row.get::<_,String>(0)?,"payload":row.get::<_,String>(1)?,"state":row.get::<_,String>(2)?}))))?.collect::<rusqlite::Result<Vec<_>>>())?;
        let mut messages = db(self
            .connection
            .prepare("select id,revision,body,deleted from messages order by id limit 8"))?;
        let messages = db(db(messages.query_map([], |row| Ok(json!({"id":row.get::<_,String>(0)?,"revision":row.get::<_,i64>(1)?,"body":row.get::<_,String>(2)?,"deleted":row.get::<_,bool>(3)?}))))?.collect::<rusqlite::Result<Vec<_>>>())?;
        Ok(
            json!({"schema":4,"upload_drafts":self.upload_drafts,"draft_revisions":self.draft_revisions,"thread_targets":self.thread_targets,"account":account,"generation":generation,"active":active,"drafts":drafts,"outbox":outbox,"messages":messages}),
        )
    }

    #[cfg(test)]
    pub fn execute_test_sql(&self, sql: &str) {
        self.connection.execute_batch(sql).unwrap();
    }
}
