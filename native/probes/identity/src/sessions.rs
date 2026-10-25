//! Synthetic N03a sessions only. These tokens are never production API credentials.
use crate::store::{self, Result, digest, now, random, valid_challenge};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

pub const SCOPE: &str = "n03a_session_lab";
#[derive(Clone)]
pub struct Context {
    pub instance_id: String,
    pub origin: String,
    pub issuer: String,
}
// No Debug: even fixture credentials must not reach logs.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Grant {
    pub scope: String,
    pub environment: String,
    pub instance_id: String,
    pub user_id: String,
    pub family: String,
    pub generation: u64,
    pub epoch: u64,
    pub access_token: String,
    pub refresh_token: String,
    pub revoke_token: String,
    pub expires_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refresh {
    pub operation_id: String,
    pub generation: u64,
}
#[derive(Serialize)]
pub struct Claims {
    pub user_id: String,
    pub family: String,
    pub generation: u64,
    pub epoch: u64,
}

pub fn initialize(connection: &Connection, context: &Context) -> Result<()> {
    connection.execute_batch("CREATE TABLE session_metadata(schema_version INTEGER NOT NULL,scope TEXT NOT NULL,instance_id TEXT NOT NULL,origin TEXT NOT NULL,issuer TEXT NOT NULL);
        CREATE TABLE session_epochs(user_id TEXT PRIMARY KEY,epoch INTEGER NOT NULL);
        CREATE TABLE session_families(family TEXT PRIMARY KEY,user_id TEXT NOT NULL,epoch INTEGER NOT NULL,generation INTEGER NOT NULL,revoked INTEGER NOT NULL DEFAULT 0,access_hash TEXT UNIQUE NOT NULL,access_expires INTEGER NOT NULL,refresh_hash TEXT UNIQUE NOT NULL,revoke_hash TEXT UNIQUE NOT NULL,grant_json TEXT NOT NULL);
        CREATE TABLE session_receipts(old_hash TEXT NOT NULL,operation_id TEXT NOT NULL,generation INTEGER NOT NULL,family TEXT NOT NULL,result TEXT NOT NULL,expires INTEGER NOT NULL,PRIMARY KEY(old_hash,operation_id,generation));
        PRAGMA user_version=3;") .map_err(|_| ())?;
    connection
        .execute(
            "INSERT INTO session_metadata VALUES(1,?1,?2,?3,?4)",
            params![SCOPE, context.instance_id, context.origin, context.issuer],
        )
        .map_err(|_| ())?;
    Ok(())
}
pub fn reopen(connection: &Connection, origin: &str, issuer: &str) -> Result<Context> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|_| ())?;
    if version != 3 {
        return Err(());
    }
    let rows: i64 = connection
        .query_row("SELECT count(*) FROM session_metadata", [], |row| {
            row.get(0)
        })
        .map_err(|_| ())?;
    let (schema, scope, instance, stored_origin, stored_issuer): (
        i64,
        String,
        String,
        String,
        String,
    ) = connection
        .query_row(
            "SELECT schema_version,scope,instance_id,origin,issuer FROM session_metadata",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .map_err(|_| ())?;
    if rows != 1
        || schema != 1
        || scope != SCOPE
        || !valid_challenge(&instance)
        || stored_origin != origin
        || stored_issuer != issuer
    {
        return Err(());
    }
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; UPDATE attempts SET phase='authorizing' WHERE phase='verifying';").map_err(|_| ())?;
    Ok(Context {
        instance_id: instance,
        origin: origin.into(),
        issuer: issuer.into(),
    })
}
pub fn epoch(connection: &Connection, user: &str) -> Result<u64> {
    let value: Option<i64> = connection
        .query_row(
            "SELECT epoch FROM session_epochs WHERE user_id=?1",
            [user],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ())?;
    u64::try_from(value.unwrap_or(0)).map_err(|_| ())
}
pub fn valid_family(connection: &Connection, grant: &Grant) -> Result<()> {
    let valid: bool = connection
        .query_row(
            "SELECT user_id=?2 AND epoch=?3 AND revoked=0 FROM session_families WHERE family=?1",
            params![grant.family, grant.user_id, grant.epoch as i64],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ())?
        .unwrap_or(false);
    if !valid || epoch(connection, &grant.user_id)? != grant.epoch {
        return Err(());
    }
    Ok(())
}
pub fn issue(tx: &Transaction<'_>, context: &Context, user: &str, family: String) -> Result<Grant> {
    let count: i64 = tx
        .query_row("SELECT count(*) FROM session_families", [], |row| {
            row.get(0)
        })
        .map_err(|_| ())?;
    if count >= 32 {
        return Err(());
    }
    let grant = Grant {
        scope: SCOPE.into(),
        environment: store::ENVIRONMENT.into(),
        instance_id: context.instance_id.clone(),
        user_id: user.into(),
        family,
        generation: 0,
        epoch: epoch(tx, user)?,
        access_token: format!("n03a-access.{}", random()?),
        refresh_token: format!("n03a-refresh.{}", random()?),
        revoke_token: format!("n03a-revoke.{}", random()?),
        expires_at: now() + 60,
    };
    // Exact retry grants contain only synthetic tokens. Production requires an
    // encrypted credential/receipt store and key lifecycle, not this fixture JSON.
    tx.execute("INSERT INTO session_families(family,user_id,epoch,generation,access_hash,access_expires,refresh_hash,revoke_hash,grant_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![grant.family,grant.user_id,grant.epoch as i64,grant.generation as i64,digest(&grant.access_token),grant.expires_at,digest(&grant.refresh_token),digest(&grant.revoke_token),serde_json::to_string(&grant).map_err(|_| ())?]).map_err(|_| ())?;
    Ok(grant)
}
fn valid_token(token: &str, prefix: &str) -> bool {
    token.strip_prefix(prefix).is_some_and(valid_challenge)
}
pub fn probe(tx: &Transaction<'_>, token: &str) -> Result<Claims> {
    if !valid_token(token, "n03a-access.") {
        return Err(());
    }
    let json: String = tx
        .query_row(
            "SELECT grant_json FROM session_families WHERE access_hash=?1 AND access_expires>?2",
            params![digest(token), now()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ())?
        .ok_or(())?;
    let grant: Grant = serde_json::from_str(&json).map_err(|_| ())?;
    valid_family(tx, &grant)?;
    Ok(Claims {
        user_id: grant.user_id,
        family: grant.family,
        generation: grant.generation,
        epoch: grant.epoch,
    })
}
pub fn refresh(tx: &Transaction<'_>, token: &str, input: &Refresh) -> Result<Grant> {
    if !valid_token(token, "n03a-refresh.")
        || input.operation_id.len() != 32
        || !input
            .operation_id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || input.generation >= i64::MAX as u64
    {
        return Err(());
    }
    let hash = digest(token);
    let receipt:Option<(String,i64)>=tx.query_row("SELECT result,expires FROM session_receipts WHERE old_hash=?1 AND operation_id=?2 AND generation=?3",params![hash,input.operation_id,input.generation as i64],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|_| ())?;
    if let Some((json, expires)) = receipt {
        let grant: Grant = serde_json::from_str(&json).map_err(|_| ())?;
        valid_family(tx, &grant)?; // Revoke/epoch fences run BEFORE any saved result.
        if expires <= now() {
            return Err(());
        }
        return Ok(grant);
    }
    let json: String = tx
        .query_row(
            "SELECT grant_json FROM session_families WHERE refresh_hash=?1",
            [&hash],
            |row| row.get(0),
        )
        .optional()
        .map_err(|_| ())?
        .ok_or(())?;
    let mut grant: Grant = serde_json::from_str(&json).map_err(|_| ())?;
    valid_family(tx, &grant)?;
    if grant.generation != input.generation {
        return Err(());
    }
    tx.execute("DELETE FROM session_receipts WHERE expires<=?1", [now()])
        .map_err(|_| ())?;
    let count: i64 = tx
        .query_row("SELECT count(*) FROM session_receipts", [], |row| {
            row.get(0)
        })
        .map_err(|_| ())?;
    if count >= 1024 {
        return Err(());
    }
    grant.generation += 1;
    grant.access_token = format!("n03a-access.{}", random()?);
    grant.refresh_token = format!("n03a-refresh.{}", random()?);
    grant.expires_at = now() + 60;
    let json = serde_json::to_string(&grant).map_err(|_| ())?;
    tx.execute("UPDATE session_families SET generation=?1,access_hash=?2,access_expires=?3,refresh_hash=?4,grant_json=?5 WHERE family=?6",params![grant.generation as i64,digest(&grant.access_token),grant.expires_at,digest(&grant.refresh_token),json,grant.family]).map_err(|_| ())?;
    tx.execute(
        "INSERT INTO session_receipts VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            hash,
            input.operation_id,
            input.generation as i64,
            grant.family,
            json,
            now() + 60
        ],
    )
    .map_err(|_| ())?;
    Ok(grant)
}
pub fn revoke(tx: &Transaction<'_>, token: &str) -> Result<()> {
    if !valid_token(token, "n03a-revoke.") {
        return Err(());
    }
    if tx
        .execute(
            "UPDATE session_families SET revoked=1 WHERE revoke_hash=?1",
            [digest(token)],
        )
        .map_err(|_| ())?
        != 1
    {
        return Err(());
    }
    Ok(())
}
pub fn bump_epoch(tx: &Transaction<'_>, user: &str) -> Result<u64> {
    if sproyt_protocol::UserId::new(user).is_err() {
        return Err(());
    }
    tx.execute("INSERT INTO session_epochs(user_id,epoch) VALUES(?1,1) ON CONFLICT(user_id) DO UPDATE SET epoch=epoch+1",[user]).map_err(|_| ())?;
    epoch(tx, user)
}
pub fn expire(tx: &Transaction<'_>, family: &str, scope: &str) -> Result<()> {
    if !valid_challenge(family) {
        return Err(());
    }
    let sql = match scope {
        "access" => "UPDATE session_families SET access_expires=0 WHERE family=?1",
        "receipt" => "UPDATE session_receipts SET expires=0 WHERE family=?1",
        _ => return Err(()),
    };
    tx.execute(sql, [family]).map_err(|_| ())?;
    Ok(())
}
