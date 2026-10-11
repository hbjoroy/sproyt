//! Durable upload staging. Originals are never concatenated or decoded here.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, SqlitePool, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use crate::{
    config::{DatabaseConfig, DatabaseKind},
    domain::{MediaId, RepositoryError, UserId},
};

pub(crate) const CHUNK_BYTES: usize = 1024 * 1024;
const UPLOAD_TTL: i64 = 24 * 60 * 60;

#[derive(Clone)]
enum Store {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

#[derive(Clone)]
pub(crate) struct MediaStore {
    store: Store,
    limits: UploadLimits,
}

#[derive(Clone, Copy)]
pub(crate) struct UploadLimits {
    pub max_file_bytes: u64,
    pub owner_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewUpload {
    pub request_id: Uuid,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct UploadStatus {
    pub id: Uuid,
    pub channel_id: Uuid,
    pub original_filename: String,
    pub declared_type: String,
    pub size_bytes: u64,
    pub received_bytes: u64,
    pub next_chunk: u32,
    pub state: String,
    pub expires_at: i64,
    pub media_id: Option<Uuid>,
    pub chunk_bytes: usize,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum UploadError {
    #[error("upload not found or expired")]
    NotFound,
    #[error("upload conflicts with stored state")]
    Conflict,
    #[error("media storage budget is full")]
    Quota,
    #[error("invalid upload")]
    Invalid,
    #[error("unsupported media type")]
    Unsupported,
    #[error("media storage unavailable")]
    Storage(#[source] sqlx::Error),
}

impl UploadError {
    fn from_storage(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(db) if db.message() == "SPROYT_MEDIA_QUOTA" => Self::Quota,
            sqlx::Error::Database(db) if db.message() == "SPROYT_MEDIA_IMMUTABLE" => Self::Conflict,
            _ => Self::Storage(error),
        }
    }
}

fn sql(query: &str, pg: bool) -> String {
    let mut output = String::new();
    let mut rest = query;
    let mut index = 0;
    while let Some(at) = rest.find('?') {
        output.push_str(&rest[..at]);
        index += 1;
        rest = &rest[at + 1..];
        let marker = if rest.starts_with("uuid") {
            rest = &rest[4..];
            Some("uuid")
        } else if rest.starts_with("json") {
            rest = &rest[4..];
            Some("jsonb")
        } else {
            None
        };
        if pg {
            let parameter = format!("${index}");
            if let Some(kind) = marker {
                output.push_str(&format!("cast({parameter} as {kind})"));
            } else {
                output.push_str(&parameter);
            }
        } else {
            output.push('?');
        }
    }
    output.push_str(rest);
    output
}

// Both adapters execute the same state transitions; SQL parameter types are
// explicit casts, and SQLite obtains a write lock before read/modify/write.
macro_rules! with_store {
    ($self:expr, |$pool:ident, $pg:ident| $body:block) => {
        match &$self.store {
            Store::Postgres($pool) => {
                let $pg = true;
                $body
            }
            Store::Sqlite($pool) => {
                let $pg = false;
                $body
            }
        }
    };
}
macro_rules! write_transaction {
    ($pool:expr, $pg:expr) => {{
        let mut tx = $pool.begin().await.map_err(UploadError::from_storage)?;
        if !$pg {
            sqlx::query("update media_upload_budget set revision=revision where id=1")
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
        }
        tx
    }};
}

const UPLOAD_QUERY: &str = "select cast(u.id as text) as id,cast(u.channel_id as text) as channel_id,u.original_filename,u.declared_type,u.expected_bytes,u.received_bytes,cast(u.next_chunk as bigint) as next_chunk,u.state,u.expires_at,cast(u.media_id as text) as media_id from media_uploads u join channel_memberships cm on cm.channel_id=u.channel_id and cm.user_id=u.owner_id where u.id=?uuid and u.owner_id=?uuid";

macro_rules! read_upload {
    ($connection:expr, $pg:expr, $actor:expr, $id:expr, $lock:expr) => {{
        let mut query = sql(UPLOAD_QUERY, $pg);
        if $pg && $lock {
            query.push_str(" for update of u for key share of cm");
        }
        let row = sqlx::query(&query)
            .bind($id.to_string())
            .bind($actor.to_string())
            .fetch_optional($connection)
            .await
            .map_err(UploadError::from_storage)?
            .ok_or(UploadError::NotFound)?;
        let number = |key| {
            row.try_get::<i64, _>(key)
                .map_err(UploadError::from_storage)
        };
        let status = UploadStatus {
            id: Uuid::parse_str(
                &row.try_get::<String, _>("id")
                    .map_err(UploadError::from_storage)?,
            )
            .map_err(|_| UploadError::Invalid)?,
            channel_id: Uuid::parse_str(
                &row.try_get::<String, _>("channel_id")
                    .map_err(UploadError::from_storage)?,
            )
            .map_err(|_| UploadError::Invalid)?,
            original_filename: row
                .try_get("original_filename")
                .map_err(UploadError::from_storage)?,
            declared_type: row
                .try_get("declared_type")
                .map_err(UploadError::from_storage)?,
            size_bytes: u64::try_from(number("expected_bytes")?)
                .map_err(|_| UploadError::Invalid)?,
            received_bytes: u64::try_from(number("received_bytes")?)
                .map_err(|_| UploadError::Invalid)?,
            next_chunk: u32::try_from(number("next_chunk")?).map_err(|_| UploadError::Invalid)?,
            state: row.try_get("state").map_err(UploadError::from_storage)?,
            expires_at: number("expires_at")?,
            media_id: row
                .try_get::<Option<String>, _>("media_id")
                .map_err(UploadError::from_storage)?
                .map(|id| Uuid::parse_str(&id).map_err(|_| UploadError::Invalid))
                .transpose()?,
            chunk_bytes: CHUNK_BYTES,
        };
        if status.state != "complete" && status.expires_at <= chrono::Utc::now().timestamp() {
            return Err(UploadError::NotFound);
        }
        status
    }};
}

impl MediaStore {
    pub(crate) async fn initialize_policy_from_env(
        config: &DatabaseConfig,
        postgres: Option<&PgPool>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let limits = UploadLimits {
            max_file_bytes: std::env::var("SPROYT_MEDIA_MAX_FILE_BYTES")?.parse()?,
            owner_bytes: std::env::var("SPROYT_MEDIA_OWNER_BUDGET_BYTES")?.parse()?,
            total_bytes: std::env::var("SPROYT_MEDIA_TOTAL_BUDGET_BYTES")?.parse()?,
        };
        Self::connect(config, postgres, limits)
            .await?
            .check_policy(true)
            .await?;
        Ok(())
    }

    pub(crate) async fn check_policy(&self, initialize: bool) -> Result<(), UploadError> {
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            sqlx::query("update media_upload_budget set revision=revision where id=1")
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            let row = sqlx::query(
                "select max_file_bytes,owner_bytes,total_bytes from media_upload_budget where id=1",
            )
            .fetch_one(&mut *tx)
            .await
            .map_err(UploadError::from_storage)?;
            let total: Option<i64> = row
                .try_get("total_bytes")
                .map_err(UploadError::from_storage)?;
            if total.is_none() && initialize {
                sqlx::query(&sql("update media_upload_budget set max_file_bytes=?,owner_bytes=?,total_bytes=? where id=1",pg))
                    .bind(self.limits.max_file_bytes as i64).bind(self.limits.owner_bytes as i64).bind(self.limits.total_bytes as i64)
                    .execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            } else if total != Some(self.limits.total_bytes as i64)
                || row
                    .try_get::<Option<i64>, _>("max_file_bytes")
                    .map_err(UploadError::from_storage)?
                    != Some(self.limits.max_file_bytes as i64)
                || row
                    .try_get::<Option<i64>, _>("owner_bytes")
                    .map_err(UploadError::from_storage)?
                    != Some(self.limits.owner_bytes as i64)
            {
                return Err(UploadError::Conflict);
            }
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(())
        })
    }
    pub(crate) fn max_file_bytes(&self) -> u64 {
        self.limits.max_file_bytes
    }
    #[cfg(test)]
    pub(crate) fn test_sqlite(pool: SqlitePool, limits: UploadLimits) -> Self {
        Self {
            store: Store::Sqlite(pool),
            limits,
        }
    }
    #[cfg(test)]
    pub(crate) fn test_postgres(pool: PgPool, limits: UploadLimits) -> Self {
        Self {
            store: Store::Postgres(pool),
            limits,
        }
    }
    pub(crate) async fn from_env(
        config: &DatabaseConfig,
        postgres: Option<&PgPool>,
    ) -> Result<Option<Self>, Box<dyn std::error::Error + Send + Sync>> {
        if std::env::var("SPROYT_MEDIA_CHUNK_UPLOADS_ENABLED").as_deref() != Ok("true") {
            return Ok(None);
        }
        let limits = UploadLimits {
            max_file_bytes: std::env::var("SPROYT_MEDIA_MAX_FILE_BYTES")?.parse()?,
            owner_bytes: std::env::var("SPROYT_MEDIA_OWNER_BUDGET_BYTES")?.parse()?,
            total_bytes: std::env::var("SPROYT_MEDIA_TOTAL_BUDGET_BYTES")?.parse()?,
        };
        let store = Self::connect(config, postgres, limits).await?;
        store.check_policy(false).await?;
        Ok(Some(store))
    }

    pub(crate) fn start_cleanup(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        let store = self.clone();
        tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(60));
            loop {
                tokio::select! {
                    _ = timer.tick() => {
                        if let Err(error) = store.cleanup_expired().await {
                            tracing::warn!(%error,"media upload cleanup failed");
                        }
                    }
                    _ = shutdown.changed() => break,
                }
            }
        });
    }

    pub(crate) async fn connect(
        config: &DatabaseConfig,
        postgres: Option<&PgPool>,
        limits: UploadLimits,
    ) -> Result<Self, RepositoryError> {
        if limits.max_file_bytes == 0
            || limits.max_file_bytes > 1024 * 1024 * 1024
            || limits.owner_bytes < limits.max_file_bytes
            || limits.total_bytes < limits.owner_bytes
            || limits.total_bytes > i64::MAX as u64
        {
            return Err(RepositoryError::Storage(
                "invalid media upload budgets".into(),
            ));
        }
        let store = match config.kind() {
            DatabaseKind::Postgres => Store::Postgres(
                postgres
                    .ok_or_else(|| RepositoryError::Storage("missing PostgreSQL pool".into()))?
                    .clone(),
            ),
            DatabaseKind::Sqlite => {
                use std::str::FromStr;
                let options = SqliteConnectOptions::from_str(config.url())
                    .map_err(crate::db::storage)?
                    .foreign_keys(true)
                    .busy_timeout(Duration::from_secs(5));
                Store::Sqlite(
                    SqlitePool::connect_with(options)
                        .await
                        .map_err(crate::db::storage)?,
                )
            }
        };
        Ok(Self { store, limits })
    }

    pub(crate) async fn create(
        &self,
        actor: &UserId,
        channel: Uuid,
        input: &NewUpload,
    ) -> Result<UploadStatus, UploadError> {
        if input.size_bytes == 0
            || input.size_bytes > self.limits.max_file_bytes
            || input.filename.is_empty()
            || input.filename.chars().count() > 255
            || input.filename.chars().any(char::is_control)
            || input.content_type.is_empty()
            || input.content_type.len() > 127
            || input.content_type.chars().any(char::is_control)
        {
            return Err(UploadError::Invalid);
        }
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            // Global admission lock covers lookup, quota calculation and insert.
            sqlx::query("update media_upload_budget set revision=revision where id=1")
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            let mut access_query = sql(
                "select cast(1 as integer) from channel_memberships where channel_id=?uuid and user_id=?uuid",
                pg,
            );
            if pg {
                access_query.push_str(" for key share");
            }
            let allowed: Option<i32> = sqlx::query_scalar(&access_query)
                .bind(channel.to_string())
                .bind(actor.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            if allowed.is_none() {
                return Err(UploadError::NotFound);
            }
            let previous: Option<String> = sqlx::query_scalar(&sql("select cast(id as text) from media_uploads where owner_id=?uuid and request_id=?uuid",pg))
                .bind(actor.to_string()).bind(input.request_id.to_string()).fetch_optional(&mut *tx).await.map_err(UploadError::from_storage)?;
            let id = if let Some(id) = previous {
                let id = Uuid::parse_str(&id).map_err(|_| UploadError::Invalid)?;
                let previous = read_upload!(&mut *tx, pg, actor, id, true);
                if previous.channel_id != channel
                    || previous.size_bytes != input.size_bytes
                    || previous.original_filename != input.filename
                    || previous.declared_type != input.content_type
                {
                    return Err(UploadError::Conflict);
                }
                id
            } else {
                let active: i64 = sqlx::query_scalar(
                    "select count(*) from media_uploads where state!='complete'",
                )
                .fetch_one(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
                let owner_active: i64 = sqlx::query_scalar(&sql(
                    "select count(*) from media_uploads where owner_id=?uuid and state!='complete'",
                    pg,
                ))
                .bind(actor.to_string())
                .fetch_one(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
                if active >= 64 || owner_active >= 8 {
                    return Err(UploadError::Quota);
                }
                // Minimum charge covers row/index/preview overhead for tiny files.
                let total: i64 = sqlx::query_scalar("select cast(coalesce((select sum(case when size_bytes<1048576 then 1048576 else size_bytes end) from media_objects),0)+coalesce((select sum(case when expected_bytes<1048576 then 1048576 else expected_bytes end) from media_uploads where state!='complete'),0) as bigint)")
                    .fetch_one(&mut *tx).await.map_err(UploadError::from_storage)?;
                let owner: i64 = sqlx::query_scalar(&sql("select cast(coalesce((select sum(case when size_bytes<1048576 then 1048576 else size_bytes end) from media_objects where owner_id=?uuid),0)+coalesce((select sum(case when expected_bytes<1048576 then 1048576 else expected_bytes end) from media_uploads where owner_id=?uuid and state!='complete'),0) as bigint)",pg))
                    .bind(actor.to_string()).bind(actor.to_string()).fetch_one(&mut *tx).await.map_err(UploadError::from_storage)?;
                let reservation = input.size_bytes.max(CHUNK_BYTES as u64);
                if (total as u64).saturating_add(reservation) > self.limits.total_bytes
                    || (owner as u64).saturating_add(reservation) > self.limits.owner_bytes
                {
                    return Err(UploadError::Quota);
                }
                let id = Uuid::now_v7();
                let now = chrono::Utc::now().timestamp();
                sqlx::query(&sql("insert into media_uploads(id,owner_id,channel_id,request_id,original_filename,declared_type,expected_bytes,created_at,expires_at) values(?uuid,?uuid,?uuid,?uuid,?,?,?,?,?)",pg))
                    .bind(id.to_string()).bind(actor.to_string()).bind(channel.to_string()).bind(input.request_id.to_string())
                    .bind(&input.filename).bind(&input.content_type).bind(input.size_bytes as i64).bind(now).bind(now+UPLOAD_TTL)
                    .execute(&mut *tx).await.map_err(UploadError::from_storage)?;
                id
            };
            let status = read_upload!(&mut *tx, pg, actor, id, false);
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(status)
        })
    }

    pub(crate) async fn status(
        &self,
        actor: &UserId,
        id: Uuid,
    ) -> Result<UploadStatus, UploadError> {
        with_store!(self, |pool, pg| {
            Ok(read_upload!(pool, pg, actor, id, false))
        })
    }

    pub(crate) async fn put_chunk(
        &self,
        actor: &UserId,
        id: Uuid,
        index: u32,
        bytes: &[u8],
        claimed_hash: &str,
    ) -> Result<UploadStatus, UploadError> {
        if bytes.is_empty() || bytes.len() > CHUNK_BYTES {
            return Err(UploadError::Invalid);
        }
        let hash = format!("{:x}", Sha256::digest(bytes));
        if hash != claimed_hash {
            return Err(UploadError::Invalid);
        }
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            let status = read_upload!(&mut *tx, pg, actor, id, true);
            if status.state != "open" || index > status.next_chunk {
                return Err(UploadError::Conflict);
            }
            let offset = u64::from(index) * CHUNK_BYTES as u64;
            if offset >= status.size_bytes
                || bytes.len() as u64 != (status.size_bytes - offset).min(CHUNK_BYTES as u64)
            {
                return Err(UploadError::Invalid);
            }
            if index < status.next_chunk {
                let previous: Option<Vec<u8>> = sqlx::query_scalar(&sql("select content from media_upload_chunks where upload_id=?uuid and chunk_index=?",pg))
                    .bind(id.to_string()).bind(index as i32).fetch_optional(&mut *tx).await.map_err(UploadError::from_storage)?;
                if previous.as_deref() != Some(bytes) {
                    return Err(UploadError::Conflict);
                }
            } else {
                sqlx::query(&sql("insert into media_upload_chunks(upload_id,chunk_index,content,sha256) values(?uuid,?,?,?)",pg))
                    .bind(id.to_string()).bind(index as i32).bind(bytes).bind(&hash).execute(&mut *tx).await.map_err(UploadError::from_storage)?;
                sqlx::query(&sql("update media_uploads set next_chunk=next_chunk+1,received_bytes=received_bytes+? where id=?uuid",pg))
                    .bind(bytes.len() as i64).bind(id.to_string()).execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            }
            let status = read_upload!(&mut *tx, pg, actor, id, false);
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(status)
        })
    }

    pub(crate) async fn seal(&self, actor: &UserId, id: Uuid) -> Result<UploadStatus, UploadError> {
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            let status = read_upload!(&mut *tx, pg, actor, id, true);
            if status.received_bytes != status.size_bytes {
                return Err(UploadError::Conflict);
            }
            if status.state == "open" {
                sqlx::query(&sql(
                    "update media_uploads set state='sealed' where id=?uuid",
                    pg,
                ))
                .bind(id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            }
            let status = read_upload!(&mut *tx, pg, actor, id, false);
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(status)
        })
    }

    async fn read_sealed_chunk(
        &self,
        actor: &UserId,
        id: Uuid,
        index: u32,
    ) -> Result<Vec<u8>, UploadError> {
        with_store!(self, |pool, pg| {
            sqlx::query_scalar(&sql("select b.content from media_upload_chunks b join media_uploads u on u.id=b.upload_id join channel_memberships cm on cm.channel_id=u.channel_id and cm.user_id=u.owner_id where u.id=?uuid and u.owner_id=?uuid and u.state in ('sealed','complete') and (u.state='complete' or u.expires_at>?) and b.chunk_index=?",pg))
                .bind(id.to_string()).bind(actor.to_string()).bind(chrono::Utc::now().timestamp()).bind(index as i32)
                .fetch_optional(pool).await.map_err(UploadError::from_storage)?.ok_or(UploadError::NotFound)
        })
    }

    pub(crate) async fn complete(&self, actor: &UserId, id: Uuid) -> Result<MediaId, UploadError> {
        let status = self.seal(actor, id).await?;
        if let Some(id) = status.media_id {
            return Ok(MediaId::from_uuid(id));
        }
        let mut hash = Sha256::new();
        let mut detected_type = None;
        let mut length = 0u64;
        for index in 0..status.next_chunk {
            let bytes = self.read_sealed_chunk(actor, id, index).await?;
            if index == 0 {
                detected_type =
                    crate::web::media::detected_media_type(&bytes, &status.declared_type);
            }
            length += bytes.len() as u64;
            hash.update(&bytes);
        }
        if length != status.size_bytes {
            return Err(UploadError::Conflict);
        }
        let content_type = detected_type.ok_or(UploadError::Unsupported)?;
        let sha256 = format!("{:x}", hash.finalize());
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            // The all-writer trigger uses this same lock. Always budget ->
            // upload -> membership, including quota-neutral publication.
            sqlx::query("update media_upload_budget set revision=revision where id=1")
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            let status = read_upload!(&mut *tx, pg, actor, id, true);
            if let Some(id) = status.media_id {
                tx.commit().await.map_err(UploadError::from_storage)?;
                return Ok(MediaId::from_uuid(id));
            }
            if status.state != "sealed" {
                return Err(UploadError::Conflict);
            }
            let media_id = Uuid::now_v7();
            sqlx::query(&sql("insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,analysis_metadata,created_at) values(?uuid,?uuid,?uuid,?,?,?,?,?,?json,?)",pg))
                .bind(media_id.to_string()).bind(actor.to_string()).bind(status.channel_id.to_string()).bind(format!("chunks:v1:{id}"))
                .bind(&status.original_filename).bind(&content_type).bind(status.size_bytes as i64).bind(&sha256).bind("{}")
                .bind(chrono::Utc::now()).execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            sqlx::query(&sql(
                "update media_uploads set state='complete',media_id=?uuid where id=?uuid",
                pg,
            ))
            .bind(media_id.to_string())
            .bind(id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(UploadError::from_storage)?;
            if content_type.starts_with("image/") {
                sqlx::query(&sql(
                    "insert into media_preview_jobs(media_id,available_at) values(?uuid,?)",
                    pg,
                ))
                .bind(media_id.to_string())
                .bind(chrono::Utc::now().timestamp())
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            }
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(MediaId::from_uuid(media_id))
        })
    }

    pub(crate) async fn abort(&self, actor: &UserId, id: Uuid) -> Result<(), UploadError> {
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            let status = read_upload!(&mut *tx, pg, actor, id, true);
            if status.state == "complete" {
                return Err(UploadError::Conflict);
            }
            sqlx::query(&sql("delete from media_uploads where id=?uuid", pg))
                .bind(id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(())
        })
    }

    pub(crate) async fn cleanup_expired(&self) -> Result<u64, UploadError> {
        with_store!(self, |pool, pg| {
            Ok(sqlx::query(&sql(
                "delete from media_uploads where state!='complete' and expires_at<=?",
                pg,
            ))
            .bind(chrono::Utc::now().timestamp())
            .execute(pool)
            .await
            .map_err(UploadError::from_storage)?
            .rows_affected())
        })
    }
}

#[cfg(test)]
mod tests;

pub(crate) mod preview;
