//! Worker-only operations. No HTTP route exposes privileged original reads.
use super::*;

pub(crate) const PREVIEW_MAX_BYTES: usize = 4 * CHUNK_BYTES;
const LEASE_SECONDS: i64 = 180;
const MAX_ATTEMPTS: i64 = 3;

macro_rules! lock_preview {
    ($tx:expr, $pg:expr, $id:expr) => {{
        let mut query = sql(
            "select media_id from media_preview_jobs where media_id=?uuid",
            $pg,
        );
        if $pg {
            query.push_str(" for update");
        }
        sqlx::query(&query)
            .bind($id.to_string())
            .fetch_optional(&mut *$tx)
            .await
            .map_err(UploadError::from_storage)?;
    }};
}

#[derive(Clone, Debug)]
pub(crate) struct PreviewJob {
    pub media_id: Uuid,
    pub token: Uuid,
    pub size_bytes: u64,
    pub sha256: String,
}

impl MediaStore {
    pub(crate) async fn claim_preview(&self) -> Result<Option<PreviewJob>, UploadError> {
        let now = chrono::Utc::now().timestamp();
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            sqlx::query(&sql("update media_preview_jobs set state='failed',lease_token=null,lease_until=null,last_error='preview attempts exhausted' where attempts>=? and ((state='running' and lease_until<=?) or state='pending')",pg))
                .bind(MAX_ATTEMPTS).bind(now).execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            let mut query = sql(
                "select cast(j.media_id as text) as media_id,m.size_bytes,m.sha256 from media_preview_jobs j join media_objects m on m.id=j.media_id where j.attempts<? and ((j.state='pending' and j.available_at<=?) or (j.state='running' and j.lease_until<=?)) order by j.available_at,j.media_id limit 1",
                pg,
            );
            if pg {
                query.push_str(" for update of j skip locked");
            }
            let row = sqlx::query(&query)
                .bind(MAX_ATTEMPTS)
                .bind(now)
                .bind(now)
                .fetch_optional(&mut *tx)
                .await
                .map_err(UploadError::from_storage)?;
            let Some(row) = row else {
                tx.commit().await.map_err(UploadError::from_storage)?;
                return Ok(None);
            };
            let media_id = Uuid::parse_str(
                &row.try_get::<String, _>("media_id")
                    .map_err(UploadError::from_storage)?,
            )
            .map_err(|_| UploadError::Invalid)?;
            let job = PreviewJob {
                media_id,
                token: Uuid::now_v7(),
                size_bytes: row
                    .try_get::<i64, _>("size_bytes")
                    .map_err(UploadError::from_storage)? as u64,
                sha256: row.try_get("sha256").map_err(UploadError::from_storage)?,
            };
            sqlx::query(&sql("update media_preview_jobs set state='running',attempts=attempts+1,lease_token=?uuid,lease_until=? where media_id=?uuid",pg))
                .bind(job.token.to_string()).bind(now+LEASE_SECONDS).bind(media_id.to_string())
                .execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(Some(job))
        })
    }

    // A stolen/expired lease cannot read chunks, publish, or change the new job.
    pub(crate) async fn preview_chunk(
        &self,
        job: &PreviewJob,
        index: u32,
    ) -> Result<Vec<u8>, UploadError> {
        with_store!(self, |pool, pg| {
            let row = sqlx::query(&sql("select c.content from media_upload_chunks c join media_uploads u on u.id=c.upload_id join media_preview_jobs j on j.media_id=u.media_id where u.media_id=?uuid and u.state='complete' and c.chunk_index=? and j.state='running' and j.lease_token=?uuid and j.lease_until>?",pg))
                .bind(job.media_id.to_string()).bind(i64::from(index)).bind(job.token.to_string()).bind(chrono::Utc::now().timestamp())
                .fetch_optional(pool).await.map_err(UploadError::from_storage)?.ok_or(UploadError::Conflict)?;
            row.try_get("content").map_err(UploadError::from_storage)
        })
    }

    pub(crate) async fn publish_preview(
        &self,
        job: &PreviewJob,
        bytes: Vec<u8>,
        width: u32,
        height: u32,
    ) -> Result<(), UploadError> {
        if bytes.is_empty()
            || bytes.len() > PREVIEW_MAX_BYTES
            || width == 0
            || height == 0
            || width > 720
            || height > 720
        {
            return Err(UploadError::Invalid);
        }
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            lock_preview!(tx, pg, job.media_id);
            // Lock and check before inserting. Updating first also fences the
            // PostgreSQL claimant; insertion and completion commit together.
            let changed = sqlx::query(&sql("update media_preview_jobs set state='ready',lease_token=null,lease_until=null,last_error='' where media_id=?uuid and state='running' and lease_token=?uuid and lease_until>?",pg))
                .bind(job.media_id.to_string()).bind(job.token.to_string()).bind(chrono::Utc::now().timestamp())
                .execute(&mut *tx).await.map_err(UploadError::from_storage)?.rows_affected();
            if changed != 1 {
                return Err(UploadError::Conflict);
            }
            sqlx::query(&sql("insert into media_variants(media_id,variant,content_type,size_bytes,width,height,content,created_at) values(?uuid,'preview','image/jpeg',?,?,?,?,?) on conflict(media_id,variant) do update set content_type=excluded.content_type,size_bytes=excluded.size_bytes,width=excluded.width,height=excluded.height,content=excluded.content,created_at=excluded.created_at",pg))
                .bind(job.media_id.to_string()).bind(bytes.len() as i64).bind(width as i32).bind(height as i32).bind(bytes).bind(chrono::Utc::now())
                .execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(())
        })
    }

    pub(crate) async fn fail_preview(&self, job: &PreviewJob) -> Result<(), UploadError> {
        with_store!(self, |pool, pg| {
            let mut tx = write_transaction!(pool, pg);
            lock_preview!(tx, pg, job.media_id);
            sqlx::query(&sql("update media_preview_jobs set state=case when attempts>=? then 'failed' else 'pending' end,available_at=?,lease_token=null,lease_until=null,last_error='preview processing failed' where media_id=?uuid and state='running' and lease_token=?uuid and lease_until>?",pg))
                .bind(MAX_ATTEMPTS).bind(chrono::Utc::now().timestamp()+30).bind(job.media_id.to_string()).bind(job.token.to_string()).bind(chrono::Utc::now().timestamp())
                .execute(&mut *tx).await.map_err(UploadError::from_storage)?;
            tx.commit().await.map_err(UploadError::from_storage)?;
            Ok(())
        })
    }
}
