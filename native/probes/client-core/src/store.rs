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
    #[cfg(any(test, feature = "fault-injection"))]
    fault: Option<crate::fault::Fault>,
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

impl Store {
    pub fn open(path: &str, account: &str) -> Result<Self> {
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
        if !(0..=1).contains(&version) || (version != 0 && application != APPLICATION_ID) {
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
        // The pure core does not yet define a process-recovery event. This
        // adapter explicitly makes an interrupted dispatch uncertain.
        db(tx.execute(
            "update outbox set state='uncertain' where state='dispatching'",
            [],
        ))?;
        db(tx.commit())?;
        Ok(Self {
            connection,
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

    pub fn save_draft(&mut self, generation: u64, channel: &str, body: &str) -> Result<()> {
        identifier(channel)?;
        text(body)?;
        let tx = self.transaction(generation)?;
        let count: i64 = db(tx.query_row(
            "select count(*) from drafts where channel<>?1",
            [channel],
            |row| row.get(0),
        ))?;
        if count >= ITEM_LIMIT {
            return Err("capacity");
        }
        db(tx.execute("insert into drafts values(?1,?2) on conflict(channel) do update set body=excluded.body", params![channel, body]))?;
        db(tx.commit())
    }

    pub fn clear_draft(&mut self, generation: u64, channel: &str) -> Result<()> {
        identifier(channel)?;
        let tx = self.transaction(generation)?;
        db(tx.execute("delete from drafts where channel=?1", [channel]))?;
        db(tx.commit())
    }

    pub fn enqueue(&mut self, generation: u64, id: &str, channel: &str, body: &str) -> Result<()> {
        // One-shot means the next decoded enqueue attempt, even if validation
        // or duplicate-ID checks return before the chosen site is reached.
        #[cfg(any(test, feature = "fault-injection"))]
        let fault = self.fault.take();
        identifier(id)?;
        identifier(channel)?;
        text(body)?;
        if body.trim().is_empty() {
            return Err("empty_body");
        }
        let payload = serde_json::to_string(&json!({"protocol":"sproyt.chat.v1","request_id":id,
            "type":"send_message","payload":{"channel":{"type":"id","value":channel},"body":body}}))
        .map_err(|_| "serialization_error")?;
        let tx = self.transaction(generation)?;
        let existing: Option<String> = db(tx
            .query_row(
                "select payload from outbox where request_id=?1",
                [id],
                |row| row.get(0),
            )
            .optional())?;
        if let Some(existing) = existing {
            return if existing == payload {
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
            "insert into outbox values(?1,?2,?3,'queued')",
            params![id, channel, payload],
        ))?;
        #[cfg(any(test, feature = "fault-injection"))]
        crate::fault::hit(fault.as_ref(), crate::fault::Site::AfterInsert, &payload)?;
        // Journal first; clearing the matching draft belongs to the same commit.
        db(tx.execute(
            "delete from drafts where channel=?1 and body=?2",
            params![channel, body],
        ))?;
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
        if action == "forget" {
            if state != "acknowledged" {
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
        db(tx.execute_batch("update identity set generation=generation+1,active=0 where id=1; delete from drafts; delete from outbox; delete from messages;"))?;
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
            .prepare("select channel,body from drafts order by channel limit 8"))?;
        let drafts = db(db(drafts.query_map([], |row| {
            Ok(json!({"channel":row.get::<_,String>(0)?,"body":row.get::<_,String>(1)?}))
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
            json!({"account":account,"generation":generation,"active":active,"drafts":drafts,"outbox":outbox,"messages":messages}),
        )
    }

    #[cfg(test)]
    pub fn execute_test_sql(&self, sql: &str) {
        self.connection.execute_batch(sql).unwrap();
    }
}
