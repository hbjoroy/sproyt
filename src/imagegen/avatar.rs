use super::{Error, ImageGeneration, Job, Store};
use crate::domain::UserId;

// Workshop references are media UUIDs. This marker also survives older workers.
pub(crate) const REFERENCE_PREFIX: &str = "profile-avatar:";

impl Job {
    pub(crate) fn is_avatar(&self) -> bool {
        matches!(self.reference_ids.as_slice(), [id] if id.starts_with(REFERENCE_PREFIX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::ChannelId;
    use crate::imagegen::PromptMode;
    use uuid::Uuid;

    #[tokio::test]
    async fn approval_publishes_only_the_ready_revision_and_retires_its_preview() {
        let service = ImageGeneration::test("http://unused").await;
        let Store::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        sqlx::query("create table users(id text primary key)")
            .execute(pool)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/sqlite/0084_profile_avatars.sql"
        ))
        .execute(pool)
        .await
        .unwrap();
        let owner = UserId::from_uuid(Uuid::now_v7());
        sqlx::query("insert into users(id) values(?)")
            .bind(owner.to_string())
            .execute(pool)
            .await
            .unwrap();
        let mut job = service
            .enqueue_with_mode(
                owner.clone(),
                ChannelId::generate(),
                Uuid::now_v7(),
                "portrait".into(),
                vec![(format!("{REFERENCE_PREFIX}test"), "private source".into())],
                PromptMode::Literal,
            )
            .await
            .unwrap();
        assert!(!service.approve_avatar(job.clone(), vec![1]).await.unwrap());
        assert!(
            service
                .avatar_job(UserId::from_uuid(Uuid::now_v7()))
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            service
                .avatar_job(owner.clone())
                .await
                .unwrap()
                .unwrap()
                .reference_images
                .is_empty()
        );
        job.image = Some("private preview".into());
        job.transition("ready");
        assert!(service.save(&mut job).await.unwrap());
        assert!(
            service
                .approve_avatar(job.clone(), vec![1, 2])
                .await
                .unwrap()
        );
        assert!(!service.approve_avatar(job.clone(), vec![3]).await.unwrap());
        assert!(service.avatar_job(owner).await.unwrap().is_none());
        let saved = service.get(&job.id).await.unwrap().unwrap();
        assert!(saved.image.is_none() && saved.reference_images.is_empty());
        assert_eq!(
            sqlx::query_scalar::<_, Vec<u8>>("select jpeg from profile_avatars")
                .fetch_one(pool)
                .await
                .unwrap(),
            vec![1, 2]
        );
    }
}

impl ImageGeneration {
    pub(crate) async fn avatar_job(&self, owner: UserId) -> Result<Option<Job>, Error> {
        let projection = match &self.store {
            Store::Postgres(_) => "(data::jsonb - 'image' - 'reference_images')::text AS data",
            Store::Sqlite(_) => "json_remove(data, '$.image', '$.reference_images') AS data",
        };
        let marker = match &self.store {
            Store::Postgres(_) => "data::jsonb->'reference_ids'->>0",
            Store::Sqlite(_) => "json_extract(data, '$.reference_ids[0]')",
        };
        let sql = format!(
            "SELECT {projection} FROM image_generation_jobs WHERE owner_id=$1 AND state IN ('queued','submitting','running','ready','failed') AND {marker} LIKE 'profile-avatar:%' ORDER BY updated_at DESC LIMIT 1"
        );
        Ok(self.query_jobs(&sql, &owner.to_string()).await?.pop())
    }

    pub(crate) async fn approve_avatar(&self, mut job: Job, jpeg: Vec<u8>) -> Result<bool, Error> {
        if !job.is_avatar() || job.state != "ready" {
            return Ok(false);
        }
        let revision = job.revision;
        job.revision += 1;
        job.transition("dismissed");
        job.image = None;
        job.reference_images.clear();
        let data = serde_json::to_string(&job)?;
        let retire = "update image_generation_jobs set state='dismissed', revision=$1, updated_at=$2, data=$3 where id=$4 and revision=$5 and state='ready' and owner_id=$6";
        let publish = "insert into profile_avatars(owner_id,job_id,jpeg) values($1,$2,$3) on conflict(owner_id) do update set job_id=excluded.job_id, jpeg=excluded.jpeg";
        macro_rules! approve {
            ($pool:expr, $owner:expr) => {{
                let mut tx = $pool.begin().await?;
                let changed = sqlx::query(retire)
                    .bind(job.revision)
                    .bind(job.updated_at)
                    .bind(&data)
                    .bind(&job.id)
                    .bind(revision)
                    .bind(job.owner_id.to_string())
                    .execute(&mut *tx)
                    .await?
                    .rows_affected();
                if changed == 1 {
                    sqlx::query(publish)
                        .bind($owner)
                        .bind(&job.id)
                        .bind(jpeg)
                        .execute(&mut *tx)
                        .await?;
                }
                tx.commit().await?;
                Ok(changed == 1)
            }};
        }
        match &self.store {
            Store::Postgres(p) => approve!(p, *job.owner_id.as_uuid()),
            Store::Sqlite(p) => approve!(p, job.owner_id.to_string()),
        }
    }
}
