mod worker;

pub(crate) use worker::spawn;

use std::{
    collections::BTreeMap,
    str::FromStr,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{
    PgPool, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::{
    chatbot::sql,
    config::{DatabaseConfig, DatabaseKind},
    db::{sql_error, storage},
    domain::{
        ChannelId, ChannelSequence, ChatMessage, DisplayName, MediaId, MembershipRole, MessageBody,
        MessageId, Policy, RepositoryError, UserId,
    },
};

const PAGE_SIZE: usize = 30;
const ANALYZER_VERSION: i32 = 1;
static PREFER_BACKFILL: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
pub(crate) enum TopicStore {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

#[derive(Debug)]
pub(crate) struct TopicJob {
    message_id: String,
    lease: String,
}

pub(crate) struct TopicImage {
    pub id: MediaId,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

pub(crate) struct TopicInput {
    pub body: String,
    pub existing_labels: Vec<String>,
    pub images: Vec<TopicImage>,
}

#[derive(Serialize)]
pub(crate) struct TopicSummary {
    enabled: bool,
    can_manage: bool,
    topics: Vec<TopicPill>,
}

#[derive(Serialize, sqlx::FromRow)]
struct TopicPill {
    key: String,
    label: String,
    count: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct TopicMatchPreview {
    message_id: String,
    parent_message_id: Option<String>,
    sequence: i64,
    sender_display_name: String,
    body: String,
    created_at: DateTime<Utc>,
}

#[derive(Serialize)]
struct TopicMatch {
    #[serde(flatten)]
    preview: TopicMatchPreview,
    message: ChatMessage,
}

#[derive(sqlx::FromRow)]
struct TopicMatchRow {
    #[sqlx(flatten)]
    preview: TopicMatchPreview,
    channel_id: String,
    sender_id: String,
    message_sender_display_name: String,
    message_body: String,
    sent_at: DateTime<Utc>,
    edited_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
}

impl TopicMatchRow {
    fn into_match(self) -> Result<TopicMatch, RepositoryError> {
        let message = ChatMessage {
            id: MessageId::from_uuid(Uuid::parse_str(&self.preview.message_id).map_err(storage)?),
            channel_id: ChannelId::new(self.channel_id).map_err(storage)?,
            parent_message_id: self
                .preview
                .parent_message_id
                .as_deref()
                .map(Uuid::parse_str)
                .transpose()
                .map_err(storage)?
                .map(MessageId::from_uuid),
            sender_id: UserId::new(self.sender_id).map_err(storage)?,
            sender_display_name: DisplayName::new(self.message_sender_display_name)
                .map_err(storage)?,
            body: MessageBody::new(self.message_body).map_err(storage)?,
            sequence: ChannelSequence::try_from(self.preview.sequence).map_err(storage)?,
            sent_at: self.sent_at,
            edited_at: self.edited_at,
            deleted_at: self.deleted_at,
        };
        Ok(TopicMatch {
            preview: self.preview,
            message,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct TopicPage {
    matches: Vec<TopicMatch>,
    next_before: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Access {
    role: String,
    enabled: bool,
    is_direct: bool,
}

impl Access {
    fn can_manage(&self) -> bool {
        let role = MembershipRole::parse(&self.role);
        Policy::can_moderate_channel(role.as_ref())
            || self.is_direct && Policy::can_send_to_channel(role.as_ref())
    }
}

const CHANNEL_ACCESS: &str = "
    select m.role,coalesce(s.enabled,false) as enabled,
           (exists(select 1 from direct_conversations d where d.channel_id=m.channel_id)
            or exists(select 1 from direct_group_conversations d where d.channel_id=m.channel_id)) as is_direct
    from channel_memberships m
    left join channel_topic_settings s on s.channel_id=m.channel_id
    where m.channel_id=?uuid and m.user_id=?uuid
";

const TOPIC_PILLS: &str = "
    select e.topic_key as key,min(e.label) as label,count(distinct e.message_id) as count
    from channel_topic_evidence e
    join channel_topic_settings setting on setting.channel_id=e.channel_id and setting.enabled
    join channel_topic_sources source on source.message_id=e.message_id
    join channel_memberships viewer on viewer.channel_id=e.channel_id and viewer.user_id=?uuid
    where e.channel_id=?uuid and e.source_revision=source.source_revision
      and e.analyzer_version={version} and (source.saved_owner_id is null or source.saved_owner_id=viewer.user_id)
    group by e.topic_key
    order by count desc,min(e.sequence),e.topic_key
";

const TOPIC_MATCHES: &str = "
    select cast(m.id as text) as message_id,cast(m.parent_message_id as text) as parent_message_id,
           m.sequence,source.sender_display_name,substr(source.body,1,1200) as body,source.created_at,
           cast(m.channel_id as text) as channel_id,cast(m.sender_id as text) as sender_id,
           m.sender_display_name as message_sender_display_name,m.body as message_body,
           m.created_at as sent_at,m.edited_at,m.deleted_at
    from (
        select distinct e.message_id,e.sequence from channel_topic_evidence e
        join channel_topic_settings setting on setting.channel_id=e.channel_id and setting.enabled
        join channel_topic_sources source on source.message_id=e.message_id
        join channel_memberships viewer on viewer.channel_id=e.channel_id and viewer.user_id=?uuid
        where e.channel_id=?uuid and e.topic_key=? and e.sequence<?int
          and e.source_revision=source.source_revision and e.analyzer_version={version}
          and (source.saved_owner_id is null or source.saved_owner_id=viewer.user_id)
        order by e.sequence desc
        limit 31
    ) page
    join messages m on m.id=page.message_id
    join channel_topic_sources source on source.message_id=m.id
    order by m.sequence desc
";

const JOB_INPUT: &str = "
    select source.body
    from channel_topic_jobs j
    join channel_topic_settings s on s.channel_id=j.channel_id and s.enabled
    join channel_topic_sources source on source.message_id=j.message_id
    where j.message_id=?uuid and j.lease_token=? and j.due_at>{now}
      and j.source_revision=source.source_revision
";

const JOB_IMAGES: &str = "
    select cast(v.media_id as text),v.content_type,v.content from channel_topic_jobs j
    join channel_topic_settings s on s.channel_id=j.channel_id and s.enabled
    join channel_topic_sources source on source.message_id=j.message_id
    join message_attachments a on a.message_id=source.source_message_id
    join media_objects media on media.id=a.media_id and media.channel_id=source.source_channel_id
    join media_variants v on v.media_id=a.media_id and v.variant='preview'
    where j.message_id=?uuid and j.lease_token=? and j.due_at>{now}
      and j.source_revision=source.source_revision
      and v.content_type in ('image/jpeg','image/png','image/webp')
      and v.size_bytes between 1 and 4194304 and length(v.content) between 1 and 4194304
    order by a.position limit 10
";

const JOB_LABELS: &str = "
    with recent as (
        select e.message_id,e.topic_key,e.label,e.source_revision,e.analyzer_version,e.sequence
        from channel_topic_jobs job
        join channel_topic_evidence e on e.channel_id=job.channel_id
        where job.message_id=?uuid and job.lease_token=?
        order by e.sequence desc,e.topic_key,e.kind,e.origin_id limit 200
    )
    select min(recent.label) from recent
    join channel_topic_sources source on source.message_id=recent.message_id
    where recent.source_revision=source.source_revision and recent.analyzer_version={version}
      and source.saved_owner_id is null
    group by recent.topic_key
    order by count(distinct recent.message_id) desc,max(recent.sequence) desc,recent.topic_key limit 40
";

const CLAIM_JOB: &str = "
    with candidate as (
        select j.message_id,source.source_revision from channel_topic_jobs j
        join channel_topic_settings s on s.channel_id=j.channel_id and s.enabled
        join channel_topic_sources source on source.message_id=j.message_id
        where j.is_backfill=? and j.due_at<={now} and j.attempts<3
        order by j.due_at,j.message_id limit 1 {lock}
    )
    update channel_topic_jobs set lease_token=?,due_at={now}+360,attempts=attempts+1,
        source_revision=(select source_revision from candidate)
    where message_id=(select message_id from candidate)
    returning cast(message_id as text)
";

const JOB_COMPLETE: &str = "
    update channel_topic_jobs
    set due_at=null,lease_token=null,attempts=0,analyzer_version=?
    where message_id=?uuid and lease_token=? and due_at>{now}
      and exists(select 1 from channel_topic_settings s where s.channel_id=channel_topic_jobs.channel_id and s.enabled)
      and exists(select 1 from channel_topic_sources source where source.message_id=channel_topic_jobs.message_id
                 and source.source_revision=channel_topic_jobs.source_revision)
    returning cast(channel_id as text),source_revision
";

const JOB_FAIL: &str = "
    update channel_topic_jobs
    set lease_token=null,due_at=case when attempts<3 then {now}+60*attempts end
    where message_id=?uuid and lease_token=?
";

macro_rules! with_pool {
    ($store:expr, |$pool:ident,$pg:ident| $body:block) => {
        match $store {
            TopicStore::Postgres($pool) => {
                let $pg = true;
                $body
            }
            TopicStore::Sqlite($pool) => {
                let $pg = false;
                $body
            }
        }
    };
}

fn statement(query: &str, pg: bool) -> String {
    let now = if pg {
        "cast(extract(epoch from statement_timestamp()) as bigint)"
    } else {
        "unixepoch()"
    };
    let lock = if pg {
        "for update of j skip locked"
    } else {
        ""
    };
    sql(
        &query
            .replace("{now}", now)
            .replace("{lock}", lock)
            .replace("{version}", &ANALYZER_VERSION.to_string()),
        pg,
    )
}

impl TopicStore {
    pub(crate) async fn connect(
        config: &DatabaseConfig,
        postgres: Option<&PgPool>,
    ) -> Result<Self, RepositoryError> {
        match config.kind() {
            DatabaseKind::Postgres => Ok(Self::Postgres(
                postgres
                    .ok_or_else(|| storage("missing PostgreSQL pool"))?
                    .clone(),
            )),
            DatabaseKind::Sqlite => {
                let options = SqliteConnectOptions::from_str(config.url())
                    .map_err(storage)?
                    .foreign_keys(true)
                    .busy_timeout(Duration::from_secs(5));
                Ok(Self::Sqlite(
                    SqlitePoolOptions::new()
                        .max_connections(2)
                        .connect_with(options)
                        .await
                        .map_err(sql_error)?,
                ))
            }
        }
    }

    async fn access(&self, actor: &UserId, channel: &ChannelId) -> Result<Access, RepositoryError> {
        let row = with_pool!(self, |pool, pg| {
            sqlx::query_as::<_, Access>(&sql(CHANNEL_ACCESS, pg))
                .bind(channel.to_string())
                .bind(actor.to_string())
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
        });
        row.filter(|access| Policy::can_read_channel(MembershipRole::parse(&access.role).as_ref()))
            .ok_or(RepositoryError::PermissionDenied)
    }

    pub(crate) async fn summary(
        &self,
        actor: &UserId,
        channel: &ChannelId,
    ) -> Result<TopicSummary, RepositoryError> {
        let access = self.access(actor, channel).await?;
        let topics = if access.enabled {
            with_pool!(self, |pool, pg| {
                sqlx::query_as::<_, TopicPill>(&statement(&format!("{TOPIC_PILLS} limit 6"), pg))
                    .bind(actor.to_string())
                    .bind(channel.to_string())
                    .fetch_all(pool)
                    .await
                    .map_err(sql_error)?
            })
        } else {
            Vec::new()
        };
        Ok(TopicSummary {
            enabled: access.enabled,
            can_manage: access.can_manage(),
            topics,
        })
    }

    pub(crate) async fn set_enabled(
        &self,
        actor: &UserId,
        channel: &ChannelId,
        enabled: bool,
    ) -> Result<TopicSummary, RepositoryError> {
        let access = self.access(actor, channel).await?;
        if !access.can_manage() {
            return Err(RepositoryError::PermissionDenied);
        }
        with_pool!(self, |pool, pg| {
            let mut tx = pool
                .begin_with(if pg { "BEGIN" } else { "BEGIN IMMEDIATE" })
                .await
                .map_err(sql_error)?;
            // Coordinate enablement with message sequence allocation. A message
            // inserted before enablement will be found by the backfill.
            if pg {
                sqlx::query(&sql(
                    "select channel_id from channel_sequences where channel_id=?uuid for update",
                    pg,
                ))
                .bind(channel.to_string())
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
                sqlx::query(&sql("select user_id from channel_memberships where channel_id=?uuid and user_id=?uuid for share", pg))
                    .bind(channel.to_string()).bind(actor.to_string()).execute(&mut *tx).await.map_err(sql_error)?;
            }
            let access = sqlx::query_as::<_, Access>(&sql(CHANNEL_ACCESS, pg))
                .bind(channel.to_string())
                .bind(actor.to_string())
                .fetch_optional(&mut *tx)
                .await
                .map_err(sql_error)?
                .filter(Access::can_manage)
                .ok_or(RepositoryError::PermissionDenied)?;
            if access.enabled != enabled {
                sqlx::query(&sql("insert into channel_topic_settings(channel_id,enabled,backfill_after) values (?uuid,?,?int)
                    on conflict(channel_id) do update set enabled=excluded.enabled,backfill_after=excluded.backfill_after", pg))
                    .bind(channel.to_string()).bind(enabled).bind(enabled.then_some(0_i64))
                    .execute(&mut *tx).await.map_err(sql_error)?;
            }
            tx.commit().await.map_err(sql_error)?;
        });
        self.summary(actor, channel).await
    }

    pub(crate) async fn matches(
        &self,
        actor: &UserId,
        channel: &ChannelId,
        key: &str,
        before: i64,
    ) -> Result<TopicPage, RepositoryError> {
        self.access(actor, channel).await?;
        let mut rows = with_pool!(self, |pool, pg| {
            sqlx::query_as::<_, TopicMatchRow>(&statement(TOPIC_MATCHES, pg))
                .bind(actor.to_string())
                .bind(channel.to_string())
                .bind(key)
                .bind(before)
                .fetch_all(pool)
                .await
                .map_err(sql_error)?
        });
        let next_before = if rows.len() > PAGE_SIZE {
            rows.truncate(PAGE_SIZE);
            rows.last().map(|row| row.preview.sequence)
        } else {
            None
        };
        let matches = rows
            .into_iter()
            .map(TopicMatchRow::into_match)
            .collect::<Result<_, _>>()?;
        Ok(TopicPage {
            matches,
            next_before,
        })
    }

    async fn claim_due(&self, backfill: bool) -> Result<Option<TopicJob>, RepositoryError> {
        let lease = Uuid::now_v7().to_string();
        let message_id = with_pool!(self, |pool, pg| {
            sqlx::query_scalar::<_, String>(&statement(CLAIM_JOB, pg))
                .bind(backfill)
                .bind(&lease)
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
        });
        Ok(message_id.map(|message_id| TopicJob { message_id, lease }))
    }

    pub(crate) async fn claim(&self) -> Result<Option<TopicJob>, RepositoryError> {
        self.refresh_media().await?;
        self.refresh_saved_sources().await?;
        // Reserve alternate opportunities for history while keeping live
        // messages moving during a long backfill.
        let prefer_backfill = PREFER_BACKFILL.fetch_xor(true, Ordering::Relaxed);
        if !prefer_backfill && let Some(job) = self.claim_due(false).await? {
            return Ok(Some(job));
        }
        if let Some(job) = self.claim_due(true).await? {
            return Ok(Some(job));
        }
        self.backfill().await?;
        if let Some(job) = self.claim_due(true).await? {
            return Ok(Some(job));
        }
        if prefer_backfill {
            return self.claim_due(false).await;
        }
        Ok(None)
    }

    async fn refresh_media(&self) -> Result<(), RepositoryError> {
        let Self::Postgres(pool) = self else {
            return Ok(());
        };
        let mut tx = pool.begin().await.map_err(sql_error)?;
        let media = sqlx::query_scalar::<_, Uuid>(
            "select media_id from channel_topic_dirty_media order by media_id limit 1 for update skip locked",
        )
        .fetch_optional(&mut *tx).await.map_err(sql_error)?;
        if let Some(media) = media {
            sqlx::query(
                "select channel_topics_refresh(message_id) from message_attachments where media_id=$1",
            )
            .bind(media).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("delete from channel_topic_dirty_media where media_id=$1")
                .bind(media)
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
        }
        tx.commit().await.map_err(sql_error)?;
        Ok(())
    }

    async fn refresh_saved_sources(&self) -> Result<(), RepositoryError> {
        with_pool!(self, |pool, pg| {
            let mut tx = pool
                .begin_with(if pg { "BEGIN" } else { "BEGIN IMMEDIATE" })
                .await
                .map_err(sql_error)?;
            let lock = if pg { "for update skip locked" } else { "" };
            let dirty = sqlx::query_as::<_, (String, Option<String>)>(&format!(
                "select cast(source_message_id as text),cast(after_log_message_id as text)
                 from channel_topic_dirty_sources order by picked_at,source_message_id limit 1 {lock}"
            ))
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?;
            if let Some((source, after)) = dirty {
                let after = after.unwrap_or_else(|| Uuid::nil().to_string());
                let end = sqlx::query_scalar::<_, Option<String>>(&sql(
                    "select max(cast(log_message_id as text)) from (
                         select log_message_id from personal_saved_links
                         where source_message_id=?uuid and log_message_id>?uuid
                         order by log_message_id limit 64
                     ) batch",
                    pg,
                ))
                .bind(&source)
                .bind(&after)
                .fetch_one(&mut *tx)
                .await
                .map_err(sql_error)?;
                if let Some(end) = end {
                    sqlx::query(&sql(
                        "insert into channel_topic_jobs(message_id,channel_id,due_at,attempts)
                         select m.id,m.channel_id,case when current_source.message_id is not null then 0 end,
                                case when current_source.message_id is null then 3 else 0 end
                         from personal_saved_links link
                         join messages m on m.id=link.log_message_id and m.deleted_at is null
                         left join channel_topic_sources current_source on current_source.message_id=m.id
                         left join channel_topic_settings s on s.channel_id=m.channel_id
                         where link.source_message_id=?uuid and link.log_message_id>?uuid and link.log_message_id<=?uuid
                           and (s.enabled or exists(select 1 from channel_topic_jobs j where j.message_id=m.id))
                         order by m.id
                         on conflict(message_id) do update set due_at=excluded.due_at,lease_token=null,attempts=excluded.attempts,is_backfill=false,
                            analyzer_version=null,source_revision=null
                         where not exists(select 1 from channel_topic_sources current_source
                            where current_source.message_id=channel_topic_jobs.message_id
                              and current_source.source_revision=channel_topic_jobs.source_revision)", pg))
                        .bind(&source).bind(&after).bind(&end)
                        .execute(&mut *tx).await.map_err(sql_error)?;
                    sqlx::query(&sql(
                        "delete from channel_topic_evidence where message_id in (
                            select log_message_id from personal_saved_links
                            where source_message_id=?uuid and log_message_id>?uuid and log_message_id<=?uuid
                         ) and not exists(select 1 from channel_topic_sources current_source
                           where current_source.message_id=channel_topic_evidence.message_id
                             and current_source.source_revision=channel_topic_evidence.source_revision)", pg))
                        .bind(&source).bind(&after).bind(&end)
                        .execute(&mut *tx).await.map_err(sql_error)?;
                    sqlx::query(&statement("update channel_topic_dirty_sources set after_log_message_id=?uuid,picked_at={now} where source_message_id=?uuid", pg))
                        .bind(end).bind(&source).execute(&mut *tx).await.map_err(sql_error)?;
                } else {
                    sqlx::query(&sql(
                        "delete from channel_topic_dirty_sources where source_message_id=?uuid",
                        pg,
                    ))
                    .bind(&source)
                    .execute(&mut *tx)
                    .await
                    .map_err(sql_error)?;
                }
            }
            tx.commit().await.map_err(sql_error)?;
        });
        Ok(())
    }

    async fn backfill(&self) -> Result<(), RepositoryError> {
        with_pool!(self, |pool, pg| {
            let mut tx = pool
                .begin_with(if pg { "BEGIN" } else { "BEGIN IMMEDIATE" })
                .await
                .map_err(sql_error)?;
            let lock = if pg { "for update skip locked" } else { "" };
            let pending = sqlx::query_as::<_, (String, i64)>(&format!(
                "select cast(channel_id as text),backfill_after from channel_topic_settings
                 where enabled and backfill_after is not null order by backfill_picked_at,channel_id limit 1 {lock}"
            )).fetch_optional(&mut *tx).await.map_err(sql_error)?;
            if let Some((channel, after)) = pending {
                let end = sqlx::query_scalar::<_, Option<i64>>(&sql(
                    "select max(sequence) from (select sequence from messages where channel_id=?uuid and sequence>?int
                     order by sequence limit 64) batch", pg))
                    .bind(&channel).bind(after).fetch_one(&mut *tx).await.map_err(sql_error)?;
                if let Some(end) = end {
                    sqlx::query(&statement(
                        "insert into channel_topic_jobs(message_id,channel_id,due_at,is_backfill)
                         select id,channel_id,0,true from messages
                         where channel_id=?uuid and sequence>?int and sequence<=?int and deleted_at is null
                         order by id on conflict(message_id) do update set due_at=0,lease_token=null,attempts=0,is_backfill=true,analyzer_version=null,source_revision=null
                         where (channel_topic_jobs.attempts>=3 and (channel_topic_jobs.due_at is null or channel_topic_jobs.due_at<={now}))
                            or channel_topic_jobs.analyzer_version<>{version}",
                        pg,
                    )).bind(&channel).bind(after).bind(end).execute(&mut *tx).await.map_err(sql_error)?;
                }
                sqlx::query(&statement(
                    "update channel_topic_settings set backfill_after=?int,backfill_picked_at={now} where channel_id=?uuid",
                    pg,
                ))
                .bind(end)
                .bind(&channel)
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
            }
            tx.commit().await.map_err(sql_error)?;
        });
        Ok(())
    }

    pub(crate) async fn job_current(&self, job: &TopicJob) -> Result<bool, RepositoryError> {
        with_pool!(self, |pool, pg| {
            let current = JOB_INPUT.replace("select source.body", "select 1");
            Ok(sqlx::query_scalar::<_, i32>(&statement(&current, pg))
                .bind(&job.message_id)
                .bind(&job.lease)
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?
                .is_some())
        })
    }

    pub(crate) async fn load_input(
        &self,
        job: &TopicJob,
    ) -> Result<Option<TopicInput>, RepositoryError> {
        with_pool!(self, |pool, pg| {
            let body = sqlx::query_scalar::<_, String>(&statement(JOB_INPUT, pg))
                .bind(&job.message_id)
                .bind(&job.lease)
                .fetch_optional(pool)
                .await
                .map_err(sql_error)?;
            let Some(body) = body else { return Ok(None) };
            let images = sqlx::query_as::<_, (String, String, Vec<u8>)>(&statement(JOB_IMAGES, pg))
                .bind(&job.message_id)
                .bind(&job.lease)
                .fetch_all(pool)
                .await
                .map_err(sql_error)?
                .into_iter()
                .map(|(id, content_type, bytes)| {
                    Ok(TopicImage {
                        id: MediaId::new(&id).map_err(storage)?,
                        content_type,
                        bytes,
                    })
                })
                .collect::<Result<_, RepositoryError>>()?;
            let existing_labels = sqlx::query_scalar::<_, String>(&statement(JOB_LABELS, pg))
                .bind(&job.message_id)
                .bind(&job.lease)
                .fetch_all(pool)
                .await
                .map_err(sql_error)?;
            if !self.job_current(job).await? {
                return Ok(None);
            }
            Ok(Some(TopicInput {
                body,
                images,
                existing_labels,
            }))
        })
    }

    pub(crate) async fn complete(
        &self,
        job: &TopicJob,
        text: &[String],
        images: &[(MediaId, Vec<String>)],
    ) -> Result<(), RepositoryError> {
        with_pool!(self, |pool, pg| {
            let mut tx = pool.begin().await.map_err(sql_error)?;
            let completed = sqlx::query_as::<_, (String, i64)>(&statement(JOB_COMPLETE, pg))
                .bind(ANALYZER_VERSION)
                .bind(&job.message_id)
                .bind(&job.lease)
                .fetch_optional(&mut *tx)
                .await
                .map_err(sql_error)?;
            if let Some((channel, revision)) = completed {
                sqlx::query(&sql(
                    "delete from channel_topic_evidence where message_id=?uuid",
                    pg,
                ))
                .bind(&job.message_id)
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
                let evidence = std::iter::once(("text", String::new(), text)).chain(
                    images
                        .iter()
                        .take(10)
                        .map(|(id, labels)| ("image", id.to_string(), labels.as_slice())),
                );
                for (kind, origin_id, keywords) in evidence {
                    for (key, label) in normalize(keywords) {
                        sqlx::query(&sql(
                            "insert into channel_topic_evidence(message_id,channel_id,topic_key,label,kind,origin_id,sequence,source_revision,analyzer_version)
                             select id,?uuid,?,?,?,?,sequence,?int,? from messages where id=?uuid
                             on conflict(message_id,topic_key,kind,origin_id) do nothing", pg))
                            .bind(&channel).bind(key).bind(label).bind(kind).bind(&origin_id)
                            .bind(revision).bind(ANALYZER_VERSION).bind(&job.message_id)
                            .execute(&mut *tx).await.map_err(sql_error)?;
                    }
                }
            }
            tx.commit().await.map_err(sql_error)?;
        });
        Ok(())
    }

    pub(crate) async fn fail(&self, job: &TopicJob) -> Result<(), RepositoryError> {
        with_pool!(self, |pool, pg| {
            sqlx::query(&statement(JOB_FAIL, pg))
                .bind(&job.message_id)
                .bind(&job.lease)
                .execute(pool)
                .await
                .map_err(sql_error)?;
        });
        Ok(())
    }
}

fn normalize(keywords: &[String]) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    for keyword in keywords.iter().take(5) {
        let normalized: String = keyword.nfkc().collect();
        let label = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty()
            || label.chars().count() > 40
            || !label.chars().next().is_some_and(char::is_alphanumeric)
            || !label
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '\u{27}' | '’'))
        {
            continue;
        }
        let key: String = label.to_lowercase().nfkc().collect();
        labels.entry(key).or_insert(label);
    }
    labels
}

pub(crate) fn valid_key(key: &str) -> bool {
    key.len() <= 160 && normalize(&[key.to_owned()]).contains_key(key)
}

#[cfg(test)]
mod tests;
