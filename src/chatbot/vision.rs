//! Only committed target attachments are visual input. URLs/tokens in chat are data.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

pub(super) const MAX_IMAGES: usize = 2;
const MAX_ORIGINAL_BYTES: i64 = 8 * 1024 * 1024;
const MAX_INPUT_BYTES: i64 = 1024 * 1024;
const LONG_EDGE: u32 = 1024;

pub(super) fn enabled() -> bool {
    std::env::var("SPROYT_CHAT_AGENT_VISION_ENABLED").as_deref() == Ok("true")
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct SelectedImage {
    id: String,
    position: i64,
    original_sha256: String,
    original_content_type: String,
    content_type: String,
    variant: String,
    sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Snapshot {
    version: u8,
    images: Vec<SelectedImage>,
    omitted: usize,
}

#[derive(Default)]
pub(super) struct Input {
    pub images: Vec<String>,
    pub omitted: usize,
}

impl Input {
    pub(super) fn content(&self, text: String) -> Value {
        let mut parts = vec![
            json!({"type":"text", "text":format!("{text}\nVisual input: {} target image(s) supplied; {} image(s) unavailable or outside the inspection limit. Do not claim to have inspected unavailable images.",self.images.len(),self.omitted)}),
        ];
        for image in &self.images {
            parts.push(json!({"type":"image_url","image_url":{"url":image}}));
        }
        Value::Array(parts)
    }
}

fn supported(kind: &str) -> bool {
    matches!(kind, "image/jpeg" | "image/png" | "image/webp")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn parse(raw: &str) -> Result<Snapshot> {
    if raw.len() > 4096 {
        return Err(RepositoryError::Conflict);
    }
    let snapshot: Snapshot = serde_json::from_str(raw).map_err(storage)?;
    if snapshot.version != 1
        || snapshot.images.len() > MAX_IMAGES
        || snapshot.images.iter().any(|image| {
            Uuid::parse_str(&image.id).is_err()
                || !supported(&image.content_type)
                || !supported(&image.original_content_type)
                || !matches!(image.variant.as_str(), "original" | "preview")
                || image.position < 0
                || [&image.sha256, &image.original_sha256].iter().any(|hash| {
                    hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
        })
    {
        return Err(RepositoryError::Conflict);
    }
    Ok(snapshot)
}

fn selection_query(pg: bool) -> String {
    // Metadata first; fetch only the at-most-two selected bounded previews.
    // Original blobs are never fetched in the human send transaction.
    sql(
        &format!(
            "select cast(media.id as text) id,cast(attachment.position as bigint) position,media.content_type,media.size_bytes,media.sha256,count(*) over() total,variant.content_type preview_type,cast(case when variant.size_bytes<={MAX_INPUT_BYTES} and length(variant.content)<={MAX_INPUT_BYTES} and variant.content_type in ('image/jpeg','image/png','image/webp') then 1 else 0 end as bigint) preview_eligible from message_attachments attachment join media_objects media on media.id=attachment.media_id join messages source on source.id=attachment.message_id left join media_variants variant on variant.media_id=media.id and variant.variant='preview' where source.id=?uuid and media.channel_id=source.channel_id and media.owner_id=source.sender_id and media.content_type like 'image/%' order by attachment.position limit 10"
        ),
        pg,
    )
}

macro_rules! select_snapshot {
    ($tx:expr, $message:expr, $pg:expr) => {{
        let rows = sqlx::query(&selection_query($pg))
            .bind($message.id.as_uuid().to_string())
            .fetch_all(&mut **$tx)
            .await
            .map_err(storage)?;
        let mut selected = Vec::new();
        let total: i64 = rows
            .first()
            .map(|row| row.try_get("total"))
            .transpose()
            .map_err(storage)?
            .unwrap_or(0);
        for row in rows {
            if selected.len() == MAX_IMAGES {
                break;
            }
            let original_type: String = row.try_get("content_type").map_err(storage)?;
            if !supported(&original_type) {
                continue;
            }
            let original_sha256: String = row.try_get("sha256").map_err(storage)?;
            let media_id:String=row.try_get("id").map_err(storage)?;
            let preview=if row.try_get::<i64,_>("preview_eligible").map_err(storage)?==1 {
                let query=sql(&format!("select content from media_variants where media_id=?uuid and variant='preview' and size_bytes<={MAX_INPUT_BYTES} and length(content)<={MAX_INPUT_BYTES}"),$pg);
                sqlx::query_scalar::<_,Vec<u8>>(&query).bind(&media_id).fetch_optional(&mut **$tx).await.map_err(storage)?
            }else{None};
            let (variant, content_type, sha256) = if let Some(preview) = preview {
                let kind: String = row.try_get("preview_type").map_err(storage)?;
                ("preview", kind, digest(&preview))
            } else {
                let size: i64 = row.try_get("size_bytes").map_err(storage)?;
                if !(1..=MAX_ORIGINAL_BYTES).contains(&size) {
                    continue;
                }
                ("original", original_type.clone(), original_sha256.clone())
            };
            selected.push(SelectedImage {
                id: media_id,
                position: row.try_get("position").map_err(storage)?,
                original_sha256,
                original_content_type: original_type,
                content_type,
                variant: variant.into(),
                sha256,
            });
        }
        let snapshot = Snapshot {
            version: 1,
            omitted: usize::try_from(total)
                .map_err(storage)?
                .saturating_sub(selected.len()),
            images: selected,
        };
        serde_json::to_string(&snapshot).map_err(storage)
    }};
}

pub(super) async fn snapshot_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    message: &crate::domain::ChatMessage,
) -> Result<String> {
    select_snapshot!(tx, message, true)
}

pub(super) async fn snapshot_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    message: &crate::domain::ChatMessage,
) -> Result<String> {
    select_snapshot!(tx, message, false)
}

pub(super) fn source_clause(pg: bool) -> String {
    let (table, id, sha, kind, position) = if pg {
        (
            "jsonb_array_elements(cast(j.vision_snapshot as jsonb)->'images') selected(item)",
            "selected.item->>'id'",
            "selected.item->>'original_sha256'",
            "selected.item->>'original_content_type'",
            "selected.item->>'position'",
        )
    } else {
        (
            "json_each(j.vision_snapshot,'$.images') selected",
            "json_extract(selected.value,'$.id')",
            "json_extract(selected.value,'$.original_sha256')",
            "json_extract(selected.value,'$.original_content_type')",
            "cast(json_extract(selected.value,'$.position') as text)",
        )
    };
    format!(
        " and (j.vision_snapshot is null or not exists(select 1 from {table} where not exists(select 1 from message_attachments attachment join media_objects media on media.id=attachment.media_id where attachment.message_id=j.source_message_id and cast(media.id as text)={id} and media.channel_id=j.channel_id and media.sha256={sha} and media.content_type={kind} and cast(attachment.position as text)={position})))"
    )
}

fn blob_query(image: &SelectedImage, pg: bool, lock: bool) -> String {
    let (table, extra, max) = if image.variant == "preview" {
        (
            "media_variants",
            " and blob.variant='preview'",
            MAX_INPUT_BYTES,
        )
    } else {
        ("media_blobs", "", MAX_ORIGINAL_BYTES)
    };
    let query = format!(
        "select blob.content from circle_chat_agent_jobs j join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles profile on profile.agent_id=j.agent_id join channels c on c.id=j.channel_id join messages source on source.id=j.source_message_id join message_attachments attachment on attachment.message_id=source.id join media_objects media on media.id=attachment.media_id join {table} blob on blob.media_id=media.id where j.id=?uuid and cast(j.lease_token as text)=? and j.status='leased' and j.leased_until>?int and a.enabled=true and a.vision_enabled=true and a.revision=j.config_revision and c.chat_agent_access_revision=j.access_revision and profile.revoked_at is null and (profile.expires_at is null or profile.expires_at>current_timestamp) and c.circle_id=a.circle_id and coalesce((select settings.enabled from channel_chat_agent_settings settings where settings.channel_id=c.id and settings.agent_id=a.agent_id),c.kind!='private') and source.channel_id=c.id and source.edited_at is null and source.deleted_at is null and source.created_at>? and media.id=?uuid and media.channel_id=source.channel_id and media.owner_id=source.sender_id and media.sha256=? and attachment.position=?int and length(blob.content)<={max}{extra}"
    );
    let type_clause = if image.variant == "preview" {
        " and media.content_type=? and blob.content_type=?"
    } else {
        " and media.content_type=? and media.content_type=?"
    };
    let locks = if pg && lock {
        " for share of media,attachment,blob"
    } else {
        ""
    };
    sql(&(query + type_clause + locks), pg)
}

macro_rules! fetch_blob {
    ($executor:expr,$job:expr,$image:expr,$pg:expr,$lock:expr) => {{
        sqlx::query_scalar::<_, Vec<u8>>(&blob_query($image, $pg, $lock))
            .bind(&$job.id)
            .bind(&$job.lease_token)
            .bind(Utc::now().timestamp().to_string())
            .bind(Utc::now() - chrono::Duration::seconds(WINDOW_SECONDS))
            .bind(&$image.id)
            .bind(&$image.original_sha256)
            .bind($image.position.to_string())
            .bind(&$image.original_content_type)
            .bind(&$image.content_type)
            .fetch_optional($executor)
            .await
            .map_err(storage)?
            .ok_or(RepositoryError::PermissionDenied)?
    }};
}

fn normalized_image(bytes: Vec<u8>, kind: String) -> std::result::Result<String, ()> {
    let format = match kind.as_str() {
        "image/jpeg" => image::ImageFormat::Jpeg,
        "image/png" => image::ImageFormat::Png,
        "image/webp" => image::ImageFormat::WebP,
        _ => return Err(()),
    };
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| ())?
        .thumbnail(LONG_EDGE, LONG_EDGE)
        .to_rgb8();
    let mut encoded = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 82)
        .encode_image(&image)
        .map_err(|_| ())?;
    if encoded.len() > MAX_INPUT_BYTES as usize {
        return Err(());
    }
    Ok(format!(
        "data:image/jpeg;base64,{}",
        STANDARD.encode(encoded)
    ))
}

impl CircleChatAgents {
    pub(super) async fn vision_input(
        &self,
        job: &Job,
        source: &JobSource,
    ) -> Result<Option<Input>> {
        let Some(raw) = source.vision_snapshot.as_deref() else {
            return Ok(None);
        };
        let snapshot = parse(raw)?;
        let mut input = Input {
            images: Vec::new(),
            omitted: snapshot.omitted,
        };
        for image in snapshot.images {
            let bytes = match &self.store {
                Store::Pg(pool) => fetch_blob!(pool, job, &image, true, false),
                Store::Sqlite(pool) => fetch_blob!(pool, job, &image, false, false),
            };
            if digest(&bytes) != image.sha256 {
                return Err(RepositoryError::PermissionDenied);
            }
            match tokio::task::spawn_blocking(move || normalized_image(bytes, image.content_type))
                .await
                .map_err(storage)?
            {
                Ok(encoded) => input.images.push(encoded),
                Err(()) => input.omitted += 1,
            }
        }
        Ok(Some(input))
    }
}

macro_rules! authorize_images {
    ($tx:expr,$id:expr,$pg:expr)=>{{
        let locks=if $pg {" for share of j"}else{""};
        let query=sql(&("select j.vision_snapshot,cast(j.lease_token as text) lease_token from circle_chat_agent_jobs j where j.id=?uuid".to_owned()+locks),$pg);
        let row=sqlx::query(&query).bind($id).fetch_one(&mut **$tx).await.map_err(storage)?;
        let raw:Option<String>=row.try_get("vision_snapshot").map_err(storage)?;
        if let Some(raw)=raw {
            let snapshot=parse(&raw)?;
            let job=Job {id:$id.to_string(),agent_id:String::new(),source_message_id:String::new(),channel_id:String::new(),attempts:0,reply_body:None,lease_token:row.try_get("lease_token").map_err(storage)?};
            for image in snapshot.images {
                let bytes=fetch_blob!(&mut **$tx,&job,&image,$pg,true);
                if digest(&bytes)!=image.sha256{return Err(RepositoryError::PermissionDenied)}
            }
        }
        Ok(())
    }};
}

pub(super) async fn authorize_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: &str,
) -> Result<()> {
    authorize_images!(tx, id, true)
}
pub(super) async fn authorize_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    id: &str,
) -> Result<()> {
    authorize_images!(tx, id, false)
}

#[cfg(test)]
mod tests {
    use super::super::weather_followup_tests::{execute, message};
    use super::*;
    use crate::domain::SendMessage;

    async fn blob(store: &Store, id: &str, bytes: &[u8]) {
        match store {
            Store::Pg(pool) => {
                sqlx::query("insert into media_blobs(media_id,content) values($1,$2) on conflict(media_id) do update set content=excluded.content").bind(Uuid::parse_str(id).unwrap()).bind(bytes).execute(pool).await.unwrap();
            }
            Store::Sqlite(pool) => {
                sqlx::query("insert into media_blobs(media_id,content) values(?,?) on conflict(media_id) do update set content=excluded.content").bind(id).bind(bytes).execute(pool).await.unwrap();
            }
        }
    }

    async fn publication(store: &Store, job: &Job) -> Result<()> {
        let command = SendMessage {
            actor: UserId::new(&job.agent_id).unwrap(),
            channel_id: ChannelId::new(&job.channel_id).unwrap(),
            parent_message_id: None,
            body: MessageBody::new("A visible red image.").unwrap(),
        };
        let request = format!("circle-chat-agent:{}", job.id);
        match store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.unwrap();
                let result = authorize_reply_postgres(&mut tx, &command, &request).await;
                tx.rollback().await.unwrap();
                result
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin().await.unwrap();
                let result = authorize_reply_sqlite(&mut tx, &command, &request).await;
                tx.rollback().await.unwrap();
                result
            }
        }
    }

    async fn locked_publication_deadline(pool: &PgPool, job: &Job, media: &str) {
        let now: i64 =
            sqlx::query_scalar("select floor(extract(epoch from clock_timestamp()))::bigint")
                .fetch_one(pool)
                .await
                .unwrap();
        let original_lease: i64 =
            sqlx::query_scalar("select leased_until from circle_chat_agent_jobs where id=$1")
                .bind(Uuid::parse_str(&job.id).unwrap())
                .fetch_one(pool)
                .await
                .unwrap();
        let deadline = now + 3;
        let snapshot = json!({"version":1,"port":"paros","status":"warming","fetched_at_epoch":now,"valid_until_epoch":deadline,"vessels":[]});
        sqlx::query("update circle_chat_agent_jobs set leased_until=$1,observation_snapshot=$2,observation_valid_until=$1 where id=$3").bind(deadline).bind(snapshot.to_string()).bind(Uuid::parse_str(&job.id).unwrap()).execute(pool).await.unwrap();
        let mut blocker = pool.begin().await.unwrap();
        sqlx::query("select media_id from media_blobs where media_id=$1 for update")
            .bind(Uuid::parse_str(media).unwrap())
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let (pid_tx, pid_rx) = tokio::sync::oneshot::channel();
        let pool_copy = pool.clone();
        let job_copy = job.clone();
        let publication = tokio::spawn(async move {
            let mut tx = pool_copy.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("select pg_backend_pid()")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            pid_tx.send(pid).unwrap();
            let command = SendMessage {
                actor: UserId::new(&job_copy.agent_id).unwrap(),
                channel_id: ChannelId::new(&job_copy.channel_id).unwrap(),
                parent_message_id: None,
                body: MessageBody::new("A visible red image.").unwrap(),
            };
            let result = authorize_reply_postgres(
                &mut tx,
                &command,
                &format!("circle-chat-agent:{}", job_copy.id),
            )
            .await;
            tx.rollback().await.unwrap();
            result
        });
        let pid = pid_rx.await.unwrap();
        tokio::time::timeout(Duration::from_secs(2),async{
            loop {
                let blocked:bool=sqlx::query_scalar("select coalesce(wait_event_type='Lock',false) from pg_stat_activity where pid=$1").bind(pid).fetch_one(pool).await.unwrap();
                if blocked{break;}tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("publisher must actually reach the held media lock before expiry");
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let expired: bool =
                    sqlx::query_scalar("select extract(epoch from clock_timestamp()) >= $1")
                        .bind(deadline)
                        .fetch_one(pool)
                        .await
                        .unwrap();
                if expired {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("bounded real DB deadline");
        blocker.rollback().await.unwrap();
        let result = tokio::time::timeout(Duration::from_secs(2), publication)
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(result, Err(RepositoryError::PermissionDenied)),
            "a valid initial check cannot survive deadline expiry while waiting for media locks"
        );
        sqlx::query("update circle_chat_agent_jobs set leased_until=$1,observation_snapshot=null,observation_valid_until=null where id=$2").bind(original_lease).bind(Uuid::parse_str(&job.id).unwrap()).execute(pool).await.unwrap();
    }

    async fn admission(store: &Store, message: &crate::domain::ChatMessage, enabled: bool) {
        match store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.unwrap();
                enqueue_postgres_with_capabilities(&mut tx, message, false, enabled, false)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin().await.unwrap();
                enqueue_sqlite_with_capabilities(&mut tx, message, false, enabled, false)
                    .await
                    .unwrap();
                tx.commit().await.unwrap();
            }
        }
    }

    fn input(revision: Option<i64>, vision: Option<bool>) -> AgentInput {
        AgentInput {
            display_name: "Biletevennen".into(),
            trigger_words: vec!["hjelp".into()],
            response_phrases: vec!["Be natural and describe only visible evidence".into()],
            memory_enabled: None,
            enabled: false,
            revision,
            weather: None,
            ferry_port: None,
            vision_enabled: vision,
            image_generation: None,
        }
    }

    async fn contract(store: Store) {
        let service = CircleChatAgents {
            store: store.clone(),
            model: None,
            worker_enabled: false,
            weather: None,
            ferry: None,
            observations: None,
            imagegen: None,
        };
        let owner = UserId::new(Uuid::now_v7().to_string()).unwrap();
        let member = UserId::new(Uuid::now_v7().to_string()).unwrap();
        for actor in [&owner, &member] {
            execute(
                &store,
                "insert into users(id,kind,display_name) values(?uuid,'human','Vision human')",
                &[&actor.to_string()],
            )
            .await;
        }
        let circle = Uuid::now_v7().to_string();
        execute(
            &store,
            "insert into circles(id,slug,name,created_by) values(?uuid,?,'Vision circle',?uuid)",
            &[&circle, &format!("vision-{circle}"), &owner.to_string()],
        )
        .await;
        for (actor, role) in [(&owner, "owner"), (&member, "member")] {
            execute(
                &store,
                "insert into circle_memberships(circle_id,user_id,role) values(?uuid,?uuid,?)",
                &[&circle, &actor.to_string(), role],
            )
            .await;
        }
        let channel = Uuid::now_v7().to_string();
        let other_channel = Uuid::now_v7().to_string();
        for id in [&channel, &other_channel] {
            execute(&store,"insert into channels(id,slug,name,kind,circle_id,created_by) values(?uuid,?,'Vision channel','public',?uuid,?uuid)",&[id,&format!("vision-{id}"),&circle,&owner.to_string()]).await;
            execute(&store,"insert into channel_memberships(channel_id,user_id,role) values(?uuid,?uuid,'member')",&[id,&owner.to_string()]).await;
        }
        assert!(
            service
                .create(&member, &circle, input(None, Some(true)))
                .await
                .is_err()
        );
        let created = service
            .create(&owner, &circle, input(None, Some(true)))
            .await
            .unwrap();
        assert!(created.vision_enabled);
        assert!(!created.vision_available);
        let preserved = service
            .update(&owner, &circle, &created.agent_id, input(Some(1), None))
            .await
            .unwrap();
        assert!(
            preserved.vision_enabled,
            "omitted PATCH must preserve the opt-in while service is unavailable"
        );
        let mut second = input(None, Some(true));
        second.display_name = "Another visual agent".into();
        let second = service.create(&owner, &circle, second).await.unwrap();
        assert_ne!(second.agent_id, created.agent_id);
        execute(
            &store,
            "update circle_chat_agents set enabled=true where circle_id=?uuid",
            &[&circle],
        )
        .await;

        let image = image::RgbImage::from_pixel(12, 12, image::Rgb([255, 0, 0]));
        let mut buffer = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut buffer, image::ImageFormat::Png)
            .unwrap();
        let bytes = buffer.into_inner();
        let hash = digest(&bytes);
        let media = Uuid::now_v7().to_string();
        execute(&store,"insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,width,height,created_at) values(?uuid,?uuid,?uuid,?,'vision.png','image/png',?int,?,12,12,current_timestamp)",&[&media,&owner.to_string(),&channel,&format!("vision:{media}"),&bytes.len().to_string(),&hash]).await;
        blob(&store, &media, &bytes).await;
        let source = message(
            &store,
            &channel,
            &owner,
            1,
            "Hjelp: kva ser du på dette biletet?",
            None,
        )
        .await;
        execute(
            &store,
            "insert into message_attachments(message_id,media_id,position) values(?uuid,?uuid,0)",
            &[&source.id.as_uuid().to_string(), &media],
        )
        .await;
        admission(&store, &source, true).await;
        let job = service.claim().await.unwrap().unwrap();
        let source_data = service.source(&job).await.unwrap().unwrap();
        assert_ne!(source_data.agent_name, "Maria");
        let visual = service
            .vision_input(&job, &source_data)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(visual.images.len(), 1);
        assert_eq!(visual.omitted, 0);
        let snapshot = parse(source_data.vision_snapshot.as_deref().unwrap()).unwrap();
        assert_eq!(snapshot.images[0].id, media);
        assert_eq!(snapshot.images[0].sha256, hash);
        execute(
            &store,
            "update circle_chat_agent_jobs set reply_body='A visible red image.' where id=?uuid",
            &[&job.id],
        )
        .await;
        publication(&store, &job).await.unwrap();
        if let Store::Pg(pool) = &store {
            locked_publication_deadline(pool, &job, &media).await;
        }

        execute(&store,"update media_objects set sha256='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' where id=?uuid",&[&media]).await;
        assert!(service.source(&job).await.unwrap().is_none());
        assert!(publication(&store, &job).await.is_err());
        execute(
            &store,
            "update media_objects set sha256=? where id=?uuid",
            &[&hash, &media],
        )
        .await;
        execute(
            &store,
            "delete from message_attachments where media_id=?uuid",
            &[&media],
        )
        .await;
        assert!(service.source(&job).await.unwrap().is_none());
        assert!(publication(&store, &job).await.is_err());
        execute(
            &store,
            "insert into message_attachments(message_id,media_id,position) values(?uuid,?uuid,0)",
            &[&source.id.as_uuid().to_string(), &media],
        )
        .await;
        blob(&store, &media, b"tampered bytes").await;
        assert!(service.vision_input(&job, &source_data).await.is_err());
        assert!(publication(&store, &job).await.is_err());
        blob(&store, &media, &bytes).await;
        let source_id = source.id.as_uuid().to_string();
        for (change, restore, target) in [
            (
                "update circle_chat_agents set revision=revision+1 where agent_id=?uuid",
                "update circle_chat_agents set revision=revision-1 where agent_id=?uuid",
                job.agent_id.as_str(),
            ),
            (
                "update channels set chat_agent_access_revision=chat_agent_access_revision+1 where id=?uuid",
                "update channels set chat_agent_access_revision=chat_agent_access_revision-1 where id=?uuid",
                channel.as_str(),
            ),
            (
                "update messages set deleted_at=current_timestamp where id=?uuid",
                "update messages set deleted_at=null where id=?uuid",
                source_id.as_str(),
            ),
            (
                "update messages set edited_at=current_timestamp where id=?uuid",
                "update messages set edited_at=null where id=?uuid",
                source_id.as_str(),
            ),
            (
                "update circle_chat_agents set vision_enabled=false where agent_id=?uuid",
                "update circle_chat_agents set vision_enabled=true where agent_id=?uuid",
                job.agent_id.as_str(),
            ),
        ] {
            execute(&store, change, &[target]).await;
            assert!(service.source(&job).await.unwrap().is_none());
            assert!(publication(&store, &job).await.is_err());
            execute(&store, restore, &[target]).await;
        }
        execute(
            &store,
            "update media_objects set channel_id=?uuid where id=?uuid",
            &[&other_channel, &media],
        )
        .await;
        assert!(service.source(&job).await.unwrap().is_none());
        assert!(publication(&store, &job).await.is_err());
        execute(
            &store,
            "update media_objects set channel_id=?uuid where id=?uuid",
            &[&channel, &media],
        )
        .await;
        execute(
            &store,
            "update agent_profiles set revoked_at=current_timestamp where agent_id=?uuid",
            &[&job.agent_id],
        )
        .await;
        assert!(service.source(&job).await.unwrap().is_none());
        assert!(publication(&store, &job).await.is_err());
        execute(
            &store,
            "update agent_profiles set revoked_at=null where agent_id=?uuid",
            &[&job.agent_id],
        )
        .await;
        execute(
            &store,
            "update channels set kind='private' where id=?uuid",
            &[&channel],
        )
        .await;
        assert!(service.source(&job).await.unwrap().is_none());
        assert!(publication(&store, &job).await.is_err());
        execute(&store,"insert into channel_chat_agent_settings(channel_id,agent_id,enabled,updated_by,updated_at) values(?uuid,?uuid,true,?uuid,?int)",&[&channel,&job.agent_id,&owner.to_string(),&Utc::now().timestamp().to_string()]).await;
        assert!(service.source(&job).await.unwrap().is_some());
        publication(&store, &job).await.unwrap();

        // Pending and expired AIS snapshots cannot authorize a cached reply.
        execute(
            &store,
            "update circle_chat_agent_jobs set observation_valid_until=0 where id=?uuid",
            &[&job.id],
        )
        .await;
        assert!(publication(&store, &job).await.is_err());
        let now = Utc::now().timestamp();
        let observation = json!({"version":1,"port":"paros","status":"warming","fetched_at_epoch":now,"valid_until_epoch":now+60,"vessels":[]});
        execute(&store,"update circle_chat_agent_jobs set observation_snapshot=?,observation_valid_until=?int where id=?uuid",&[&observation.to_string(),&(now+60).to_string(),&job.id]).await;
        publication(&store, &job).await.unwrap();
        execute(
            &store,
            "update circle_chat_agent_jobs set observation_valid_until=?int where id=?uuid",
            &[&(now - 1).to_string(), &job.id],
        )
        .await;
        assert!(publication(&store, &job).await.is_err());
        let mut stale = observation.clone();
        stale["vessels"] = json!([{"position_received_at_epoch":now-601}]);
        execute(&store,"update circle_chat_agent_jobs set observation_snapshot=?,observation_valid_until=?int where id=?uuid",&[&stale.to_string(),&(now+60).to_string(),&job.id]).await;
        assert!(
            publication(&store, &job).await.is_err(),
            "fresh SQL TTL cannot rescue stale position JSON"
        );
        execute(&store,"update circle_chat_agent_jobs set observation_snapshot=null,observation_valid_until=null where id=?uuid",&[&job.id]).await;

        let disabled = message(&store, &channel, &owner, 2, "Hjelp med noko anna", None).await;
        admission(&store, &disabled, false).await;
        let values=store.values("select coalesce(vision_snapshot,'disabled') from circle_chat_agent_jobs where source_message_id=?uuid",&[disabled.id.as_uuid().to_string()]).await.unwrap();
        assert!(!values.is_empty());
        assert!(values.iter().all(|value| value == "disabled"));
        let preview_target =
            message(&store, &channel, &owner, 4, "Hjelp med mobilbiletet", None).await;
        let mut preview_id = String::new();
        for (position, kind, size) in [
            (0, "image/png", 35 * 1024 * 1024),
            (1, "image/svg+xml", 500),
            (2, "image/png", bytes.len()),
            (3, "image/png", bytes.len()),
        ] {
            let id = Uuid::now_v7().to_string();
            execute(&store,"insert into media_objects(id,owner_id,channel_id,storage_key,original_filename,content_type,size_bytes,sha256,width,height,created_at) values(?uuid,?uuid,?uuid,?,'target.png',?,?int,?,12,12,current_timestamp)",&[&id,&owner.to_string(),&channel,&format!("vision:{id}"),kind,&size.to_string(),&hash]).await;
            if position == 0 {
                preview_id = id.clone();
                match &store {
                    Store::Pg(pool) => {
                        sqlx::query("insert into media_variants(media_id,variant,content_type,size_bytes,width,height,content,created_at) values($1,'preview','image/png',$2,12,12,$3,current_timestamp)").bind(Uuid::parse_str(&id).unwrap()).bind(bytes.len() as i64).bind(&bytes).execute(pool).await.unwrap();
                    }
                    Store::Sqlite(pool) => {
                        sqlx::query("insert into media_variants(media_id,variant,content_type,size_bytes,width,height,content,created_at) values(?,'preview','image/png',?,12,12,?,current_timestamp)").bind(&id).bind(bytes.len() as i64).bind(&bytes).execute(pool).await.unwrap();
                    }
                }
            } else if supported(kind) {
                blob(&store, &id, &bytes).await;
            }
            execute(&store,"insert into message_attachments(message_id,media_id,position) values(?uuid,?uuid,?int)",&[&preview_target.id.as_uuid().to_string(),&id,&position.to_string()]).await;
        }
        let raw = match &store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.unwrap();
                let raw = snapshot_postgres(&mut tx, &preview_target).await.unwrap();
                tx.commit().await.unwrap();
                raw
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin().await.unwrap();
                let raw = snapshot_sqlite(&mut tx, &preview_target).await.unwrap();
                tx.commit().await.unwrap();
                raw
            }
        };
        let selected = parse(&raw).unwrap();
        assert_eq!(selected.images.len(), 2);
        assert_eq!(selected.omitted, 2);
        assert_eq!(
            selected.images[0].variant, "preview",
            "large mobile originals use the bounded existing preview"
        );
        assert_eq!(selected.images[0].id, preview_id);
        execute(&store,"update circle_chat_agent_jobs set source_message_id=?uuid,vision_snapshot=? where id=?uuid",&[&preview_target.id.as_uuid().to_string(),&raw,&job.id]).await;
        let preview_source = service.source(&job).await.unwrap().unwrap();
        let visual = service
            .vision_input(&job, &preview_source)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(visual.images.len(), 2);
        assert_eq!(visual.omitted, 2);
        publication(&store, &job).await.unwrap();
        execute(
            &store,
            "update media_variants set content_type='image/jpeg' where media_id=?uuid",
            &[&preview_id],
        )
        .await;
        assert!(
            publication(&store, &job).await.is_err(),
            "a changed preview identity cannot authorize the frozen answer"
        );

        // A body URL or media token without a committed attachment is never authority.
        let token = message(
            &store,
            &channel,
            &owner,
            3,
            &format!("Hjelp med https://external.invalid/image.png [[media:{media}]]"),
            None,
        )
        .await;
        admission(&store, &token, true).await;
        let values = store
            .values(
                "select vision_snapshot from circle_chat_agent_jobs where source_message_id=?uuid",
                &[token.id.as_uuid().to_string()],
            )
            .await
            .unwrap();
        assert!(
            values
                .iter()
                .all(|value| parse(value).unwrap().images.is_empty())
        );
        // Leave no runnable jobs for later shared-database worker contracts.
        execute(&store,"update circle_chat_agent_jobs set status='skipped' where agent_id in (select agent_id from circle_chat_agents where circle_id=?uuid)",&[&circle]).await;
    }

    #[tokio::test]
    async fn model_http_receives_bounded_images_and_unavailable_sources_without_blocking_conversation()
     {
        use axum::{
            Json, Router,
            routing::{get, post},
        };
        let captured = Arc::new(tokio::sync::Mutex::new(None::<Value>));
        let record = captured.clone();
        let app=Router::new().route("/v1/models",get(||async{Json(json!({"data":[{"id":"visual-model"}]}))})).route("/v1/chat/completions",post(move |Json(request):Json<Value>|{
            let record=record.clone();async move{*record.lock().await=Some(request);Json(json!({"choices":[{"message":{"content":"I can see a red picture; your eyewitness report is separate from the timetable."}}]}))}
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let model = VllmChat {
            base: format!("http://{address}/v1"),
            key: None,
            http: reqwest::Client::new(),
        };
        let target = "actual-human-target";
        let messages = [
            ContextMessage {
                source: None,
                id: "historical".into(),
                author: "Other human".into(),
                body: "https://untrusted.invalid/old-picture.png".into(),
            },
            ContextMessage {
                source: None,
                id: target.into(),
                author: "Kari".into(),
                body: "I saw Blue Star Delos. What can you see in my photo?".into(),
            },
        ];
        let weather = unavailable_snapshot("weather", "Parikia, Paros", None);
        let ferry = unavailable_snapshot("GTP timetable", "paros", Some("Europe/Athens"));
        let observations = json!({"status":"warming","coverage":"partial","vessels":[]});
        let visual = Input {
            images: vec!["data:image/jpeg;base64,eA==".into()],
            omitted: 1,
        };
        model
            .reply_with_vision(
                "Another visual agent",
                &[],
                &["Friendly".into()],
                target,
                &messages,
                Some(&weather),
                None,
                Some(&ferry),
                Some(&visual),
                Some(&observations),
            )
            .await
            .unwrap();
        let request = captured.lock().await.clone().unwrap();
        let content = request["messages"][1]["content"].as_array().unwrap();
        assert_eq!(
            content.len(),
            2,
            "only the admitted target image is a visual part"
        );
        assert_eq!(content[1]["image_url"]["url"], visual.images[0]);
        let text = content[0]["text"].as_str().unwrap();
        for expected in [
            target,
            "Blue Star Delos",
            "unavailable",
            "warming",
            "1 image(s) unavailable",
        ] {
            assert!(
                text.contains(expected),
                "missing factual boundary: {expected}"
            );
        }
        let system = request["messages"][0]["content"].as_str().unwrap();
        assert!(
            system.contains("Acknowledge what the human says they observed without requiring AIS")
        );
        assert!(system.contains("Only inspect the actual target image parts"));
        assert!(request.get("tools").is_none());
        server.abort();
    }

    #[tokio::test]
    async fn sqlite_vision_attachment_contract() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();
        contract(Store::Sqlite(pool)).await;
    }

    #[tokio::test]
    async fn postgres_vision_attachment_contract() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let pool = PgPool::connect(&url).await.unwrap();
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .unwrap();
        contract(Store::Pg(pool)).await;
    }
    #[test]
    fn vision_normalizes_real_pixels_and_rejects_unsupported_or_invalid_input() {
        let image = image::RgbImage::from_pixel(1500, 20, image::Rgb([255, 0, 0]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let uri = normalized_image(png.into_inner(), "image/png".into()).unwrap();
        let bytes = STANDARD
            .decode(uri.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.width(), LONG_EDGE);
        assert!(bytes.len() <= MAX_INPUT_BYTES as usize);
        assert!(normalized_image(vec![1, 2, 3], "image/jpeg".into()).is_err());
        assert!(normalized_image(vec![1, 2, 3], "image/svg+xml".into()).is_err());
    }

    #[test]
    fn visual_model_parts_are_target_only_data_uris_with_truthful_omission() {
        let input = Input {
            images: vec!["data:image/jpeg;base64,eA==".into()],
            omitted: 2,
        };
        let parts = input.content("Target asks about this picture".into());
        assert_eq!(parts.as_array().unwrap().len(), 2);
        assert!(
            parts[0]["text"]
                .as_str()
                .unwrap()
                .contains("2 image(s) unavailable")
        );
        assert_eq!(parts[1]["image_url"]["url"], "data:image/jpeg;base64,eA==");
    }
}
