//! Ephemeral, author-scoped location shares for circle chat agents.
//!
//! A job binds the random share id that existed at enqueue time. Replacing a
//! share therefore invalidates older jobs instead of letting them acquire a
//! newer position. Coordinates are rounded before they cross the storage
//! boundary and are never included in logs or durable chat messages.
use std::sync::atomic::{AtomicI64, Ordering};

use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Postgres, Row, Sqlite, Transaction};
use uuid::Uuid;

use super::{CircleChatAgents, Job, Result, Store, WeatherConfig, sql, storage};
use crate::domain::{ChatMessage, RepositoryError, UserId};

const MAX_AGE_SECONDS: i64 = 5 * 60;
const MAX_FUTURE_SECONDS: i64 = 30;
static LAST_CLEANUP: AtomicI64 = AtomicI64::new(0);
#[cfg(test)]
tokio::task_local! {
    static TEST_ENABLED: bool;
}

pub(crate) fn enabled() -> bool {
    #[cfg(test)]
    if let Ok(value) = TEST_ENABLED.try_with(|value| *value) {
        return value;
    }
    std::env::var("SPROYT_AGENT_LOCATION_SHARING_ENABLED").as_deref() == Ok("true")
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationInput {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f64,
    pub observed_at: DateTime<Utc>,
}

#[derive(Clone, Serialize, PartialEq)]
pub(crate) struct SharedLocation {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy_m: f64,
    pub observed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize)]
pub(crate) struct AgentLocationView {
    pub id: String,
    pub name: String,
    pub location: Option<SharedLocation>,
}

#[derive(Serialize)]
pub(crate) struct AgentLocationsView {
    pub agents: Vec<AgentLocationView>,
}

#[derive(Clone)]
pub(crate) struct JobLocation {
    location: SharedLocation,
}

impl JobLocation {
    pub(crate) fn context(&self) -> Value {
        json!({
            "status": "shared",
            "latitude": self.location.latitude,
            "longitude": self.location.longitude,
            "accuracy_m": self.location.accuracy_m,
            "observed_at": self.location.observed_at,
            "expires_at": self.location.expires_at,
            "privacy": "Coordinates are approximate and temporary. Do not retain them or infer a street address."
        })
    }

    pub(crate) fn weather_config(&self) -> WeatherConfig {
        WeatherConfig {
            location: "Delt, omtrentleg posisjon".into(),
            latitude: self.location.latitude,
            longitude: self.location.longitude,
        }
    }
}

fn rounded(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn normalize(input: LocationInput, now: DateTime<Utc>) -> Result<LocationInput> {
    if !input.latitude.is_finite()
        || !input.longitude.is_finite()
        || !input.accuracy_m.is_finite()
        || !(-90.0..=90.0).contains(&input.latitude)
        || !(-180.0..=180.0).contains(&input.longitude)
        || !(0.0..=100_000.0).contains(&input.accuracy_m)
        || input.observed_at < now - ChronoDuration::seconds(MAX_AGE_SECONDS)
        || input.observed_at > now + ChronoDuration::seconds(MAX_FUTURE_SECONDS)
    {
        return Err(RepositoryError::Conflict);
    }
    Ok(LocationInput {
        latitude: rounded(input.latitude),
        longitude: rounded(input.longitude),
        accuracy_m: input.accuracy_m,
        observed_at: input.observed_at,
    })
}

fn parse_iso(value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(storage)
}

fn sqlite_iso(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn location_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<SharedLocation> {
    Ok(SharedLocation {
        latitude: row.try_get("latitude").map_err(storage)?,
        longitude: row.try_get("longitude").map_err(storage)?,
        accuracy_m: row.try_get("accuracy_m").map_err(storage)?,
        observed_at: parse_iso(&row.try_get::<String, _>("observed_at").map_err(storage)?)?,
        expires_at: parse_iso(&row.try_get::<String, _>("expires_at").map_err(storage)?)?,
    })
}

fn pg_location_from_row(row: &sqlx::postgres::PgRow) -> Result<SharedLocation> {
    Ok(SharedLocation {
        latitude: row.try_get("latitude").map_err(storage)?,
        longitude: row.try_get("longitude").map_err(storage)?,
        accuracy_m: row.try_get("accuracy_m").map_err(storage)?,
        observed_at: row.try_get("observed_at").map_err(storage)?,
        expires_at: row.try_get("expires_at").map_err(storage)?,
    })
}

const ACTOR_CHANNEL: &str = "c.circle_id is not null and not exists(select 1 from direct_conversations d where d.channel_id=c.id) and not exists(select 1 from direct_group_conversations g where g.channel_id=c.id) and exists(select 1 from channel_memberships cm join users actor on actor.id=cm.user_id where cm.channel_id=c.id and cm.user_id=?uuid and actor.kind='human') and exists(select 1 from circle_memberships membership where membership.circle_id=c.circle_id and membership.user_id=?uuid)";
const AGENT_ACCESS: &str = "a.circle_id=c.circle_id and a.enabled=true and profile.revoked_at is null and (profile.expires_at is null or profile.expires_at>current_timestamp) and coalesce((select setting.enabled from channel_chat_agent_settings setting where setting.channel_id=c.id and setting.agent_id=a.agent_id),c.kind!='private')";

fn sqlite_times(query: &str) -> String {
    query
        .replace(
            "l.expires_at>current_timestamp",
            "julianday(l.expires_at)>julianday('now')",
        )
        .replace(
            "s.expires_at>current_timestamp",
            "julianday(s.expires_at)>julianday('now')",
        )
        .replace(
            "profile.expires_at>current_timestamp",
            "julianday(profile.expires_at)>julianday('now')",
        )
}

impl CircleChatAgents {
    pub(crate) async fn list_locations(
        &self,
        actor: &UserId,
        channel: &str,
    ) -> Result<AgentLocationsView> {
        let actor = actor.to_string();
        let query = format!(
            "select cast(a.agent_id as text) as agent_id,u.display_name,l.latitude,l.longitude,l.accuracy_m,l.observed_at,l.expires_at from channels c join circle_chat_agents a on a.circle_id=c.circle_id join agent_profiles profile on profile.agent_id=a.agent_id join users u on u.id=a.agent_id left join agent_location_shares l on l.user_id=?uuid and l.agent_id=a.agent_id and l.channel_id=c.id and l.expires_at>current_timestamp where c.id=?uuid and {ACTOR_CHANNEL} and {AGENT_ACCESS} order by lower(u.display_name),a.agent_id"
        );
        let authority = format!("select 1 from channels c where c.id=?uuid and {ACTOR_CHANNEL}");
        let agents = match &self.store {
            Store::Pg(pool) => {
                let pg_query = sql(&query, true).replace("::uuid", "::text::uuid");
                let pg_authority = sql(&authority, true).replace("::uuid", "::text::uuid");
                let rows = sqlx::query(&pg_query)
                    .bind(&actor)
                    .bind(channel)
                    .bind(&actor)
                    .bind(&actor)
                    .fetch_all(pool)
                    .await
                    .map_err(storage)?;
                if rows.is_empty() {
                    let allowed = sqlx::query_scalar::<_, i32>(&pg_authority)
                        .bind(channel)
                        .bind(&actor)
                        .bind(&actor)
                        .fetch_optional(pool)
                        .await
                        .map_err(storage)?;
                    if allowed.is_none() {
                        return Err(RepositoryError::PermissionDenied);
                    }
                }
                rows.into_iter()
                    .map(|row| {
                        let location = row
                            .try_get::<Option<f64>, _>("latitude")
                            .map_err(storage)?
                            .map(|_| pg_location_from_row(&row))
                            .transpose()?;
                        Ok(AgentLocationView {
                            id: row.try_get("agent_id").map_err(storage)?,
                            name: row.try_get("display_name").map_err(storage)?,
                            location,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            Store::Sqlite(pool) => {
                let query = sqlite_times(&sql(&query, false));
                let authority = sqlite_times(&sql(&authority, false));
                let rows = sqlx::query(&query)
                    .bind(&actor)
                    .bind(channel)
                    .bind(&actor)
                    .bind(&actor)
                    .fetch_all(pool)
                    .await
                    .map_err(storage)?;
                if rows.is_empty() {
                    let allowed = sqlx::query_scalar::<_, i32>(&authority)
                        .bind(channel)
                        .bind(&actor)
                        .bind(&actor)
                        .fetch_optional(pool)
                        .await
                        .map_err(storage)?;
                    if allowed.is_none() {
                        return Err(RepositoryError::PermissionDenied);
                    }
                }
                rows.into_iter()
                    .map(|row| {
                        let location = row
                            .try_get::<Option<f64>, _>("latitude")
                            .map_err(storage)?
                            .map(|_| location_from_row(&row))
                            .transpose()?;
                        Ok(AgentLocationView {
                            id: row.try_get("agent_id").map_err(storage)?,
                            name: row.try_get("display_name").map_err(storage)?,
                            location,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?
            }
        };
        Ok(AgentLocationsView { agents })
    }

    pub(crate) async fn put_location(
        &self,
        actor: &UserId,
        channel: &str,
        agent: &str,
        input: LocationInput,
    ) -> Result<SharedLocation> {
        let input = normalize(input, Utc::now())?;
        let actor = actor.to_string();
        let new_share = Uuid::now_v7().to_string();
        let location = match &self.store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.map_err(storage)?;
                let pg = format!(
                    "insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) select $1::text::uuid,a.agent_id,c.id,$2::text::uuid,$3,$4,$5,$6,clock_timestamp()+interval '30 minutes' from channels c join circle_chat_agents a on a.agent_id=$7::text::uuid join agent_profiles profile on profile.agent_id=a.agent_id where c.id=$8::text::uuid and {actor_channel} and {agent_access} and $6::timestamptz between clock_timestamp()-interval '5 minutes' and clock_timestamp()+interval '30 seconds' on conflict(user_id,agent_id,channel_id) do update set share_id=excluded.share_id,latitude=excluded.latitude,longitude=excluded.longitude,accuracy_m=excluded.accuracy_m,observed_at=excluded.observed_at,expires_at=excluded.expires_at returning latitude,longitude,accuracy_m,observed_at,expires_at",
                    actor_channel = ACTOR_CHANNEL
                        .replacen("?uuid", "$9::text::uuid", 1)
                        .replacen("?uuid", "$10::text::uuid", 1),
                    agent_access = AGENT_ACCESS.replace("current_timestamp", "clock_timestamp()")
                );
                let row = sqlx::query(&pg)
                    .bind(&actor)
                    .bind(&new_share)
                    .bind(input.latitude)
                    .bind(input.longitude)
                    .bind(input.accuracy_m)
                    .bind(input.observed_at)
                    .bind(agent)
                    .bind(channel)
                    .bind(&actor)
                    .bind(&actor)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage)?
                    .ok_or(RepositoryError::PermissionDenied)?;
                let result = pg_location_from_row(&row)?;
                tx.commit().await.map_err(storage)?;
                result
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(storage)?;
                let sqlite = sqlite_times(&sql(
                    &format!(
                        "insert into agent_location_shares(user_id,agent_id,channel_id,share_id,latitude,longitude,accuracy_m,observed_at,expires_at) select ?,a.agent_id,c.id,?,?,?,?,?,strftime('%Y-%m-%dT%H:%M:%fZ','now','+30 minutes') from channels c join circle_chat_agents a on a.agent_id=? join agent_profiles profile on profile.agent_id=a.agent_id where c.id=? and {ACTOR_CHANNEL} and {AGENT_ACCESS} and julianday(?) between julianday('now','-5 minutes') and julianday('now','+30 seconds') on conflict(user_id,agent_id,channel_id) do update set share_id=excluded.share_id,latitude=excluded.latitude,longitude=excluded.longitude,accuracy_m=excluded.accuracy_m,observed_at=excluded.observed_at,expires_at=excluded.expires_at returning latitude,longitude,accuracy_m,observed_at,expires_at"
                    ),
                    false,
                ));
                let observed = sqlite_iso(input.observed_at);
                let row = sqlx::query(&sqlite)
                    .bind(&actor)
                    .bind(&new_share)
                    .bind(input.latitude)
                    .bind(input.longitude)
                    .bind(input.accuracy_m)
                    .bind(&observed)
                    .bind(agent)
                    .bind(channel)
                    .bind(&actor)
                    .bind(&actor)
                    .bind(&observed)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(storage)?
                    .ok_or(RepositoryError::PermissionDenied)?;
                let result = location_from_row(&row)?;
                tx.commit().await.map_err(storage)?;
                result
            }
        };
        Ok(location)
    }

    pub(crate) async fn delete_location(
        &self,
        actor: &UserId,
        channel: &str,
        agent: &str,
    ) -> Result<()> {
        let actor = actor.to_string();
        match &self.store {
            Store::Pg(pool) => {
                let mut tx = pool.begin().await.map_err(storage)?;
                sqlx::query("delete from agent_location_shares where user_id=$1::text::uuid and agent_id=$2::text::uuid and channel_id=$3::text::uuid")
                    .bind(&actor).bind(agent).bind(channel).execute(&mut *tx).await.map_err(storage)?;
                tx.commit().await.map_err(storage)?;
            }
            Store::Sqlite(pool) => {
                let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(storage)?;
                sqlx::query("delete from agent_location_shares where user_id=? and agent_id=? and channel_id=?")
                    .bind(&actor).bind(agent).bind(channel).execute(&mut *tx).await.map_err(storage)?;
                tx.commit().await.map_err(storage)?;
            }
        }
        Ok(())
    }

    pub(super) async fn location_for_job(&self, job: &Job) -> Result<Option<JobLocation>> {
        let Some(share_id) = job.location_share_id.as_deref() else {
            return Ok(None);
        };
        if !enabled() {
            return Err(RepositoryError::PermissionDenied);
        }
        match &self.store {
            Store::Pg(pool) => load_job_location_postgres(pool, &job.id, share_id).await,
            Store::Sqlite(pool) => load_job_location_sqlite(pool, &job.id, share_id).await,
        }
    }

    pub(crate) async fn cleanup_locations(&self) -> Result<()> {
        let now = Utc::now().timestamp();
        let last = LAST_CLEANUP.load(Ordering::Relaxed);
        if now - last < 60
            || LAST_CLEANUP
                .compare_exchange(last, now, Ordering::AcqRel, Ordering::Relaxed)
                .is_err()
        {
            return Ok(());
        }
        match &self.store {
            Store::Pg(pool) => cleanup_postgres(pool).await,
            Store::Sqlite(pool) => cleanup_sqlite(pool).await,
        }
    }
}

pub(crate) async fn bind_postgres(
    tx: &mut Transaction<'_, Postgres>,
    message: &ChatMessage,
    agent: &str,
) -> Result<Option<Uuid>> {
    if !enabled() {
        return Ok(None);
    }
    sqlx::query_scalar("select share_id from agent_location_shares where user_id=$1::text::uuid and agent_id=$2::text::uuid and channel_id=$3::text::uuid and expires_at>clock_timestamp()")
        .bind(message.sender_id.to_string()).bind(agent).bind(message.channel_id.to_string())
        .fetch_optional(&mut **tx).await.map_err(storage)
}

pub(crate) async fn bind_sqlite(
    tx: &mut Transaction<'_, Sqlite>,
    message: &ChatMessage,
    agent: &str,
) -> Result<Option<String>> {
    if !enabled() {
        return Ok(None);
    }
    sqlx::query_scalar("select share_id from agent_location_shares where user_id=? and agent_id=? and channel_id=? and julianday(expires_at)>julianday('now')")
        .bind(message.sender_id.to_string()).bind(agent).bind(message.channel_id.to_string())
        .fetch_optional(&mut **tx).await.map_err(storage)
}

pub(crate) async fn authorize_postgres(
    tx: &mut Transaction<'_, Postgres>,
    job_id: &str,
) -> Result<()> {
    let share: Option<Uuid> = sqlx::query_scalar(
        "select location_share_id from circle_chat_agent_jobs where id=$1::text::uuid",
    )
    .bind(job_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .flatten();
    let Some(share) = share else {
        return Ok(());
    };
    if !enabled() {
        return Err(RepositoryError::PermissionDenied);
    }
    lock_share_postgres(tx, share).await?;
    validate_bound_postgres(&mut **tx, job_id, &share.to_string(), true).await
}

async fn lock_share_postgres(tx: &mut Transaction<'_, Postgres>, share: Uuid) -> Result<()> {
    sqlx::query_scalar::<_, Uuid>(
        "select share_id from agent_location_shares where share_id=$1 for share",
    )
    .bind(share)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage)?
    .ok_or(RepositoryError::PermissionDenied)?;
    Ok(())
}

pub(crate) async fn authorize_sqlite(tx: &mut Transaction<'_, Sqlite>, job_id: &str) -> Result<()> {
    let share: Option<String> =
        sqlx::query_scalar("select location_share_id from circle_chat_agent_jobs where id=?")
            .bind(job_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(storage)?
            .flatten();
    let Some(share) = share else {
        return Ok(());
    };
    if !enabled() {
        return Err(RepositoryError::PermissionDenied);
    }
    validate_bound_sqlite(&mut **tx, job_id, &share, true).await
}

const JOB_ACCESS: &str = "s.share_id=j.location_share_id and s.user_id=source.sender_id and s.agent_id=j.agent_id and s.channel_id=j.channel_id and s.expires_at>current_timestamp and source.channel_id=j.channel_id and author.kind='human' and exists(select 1 from channel_memberships cm where cm.channel_id=j.channel_id and cm.user_id=source.sender_id) and exists(select 1 from circle_memberships membership where membership.circle_id=c.circle_id and membership.user_id=source.sender_id) and c.circle_id is not null and not exists(select 1 from direct_conversations d where d.channel_id=c.id) and not exists(select 1 from direct_group_conversations g where g.channel_id=c.id) and a.enabled=true and a.circle_id=c.circle_id and profile.revoked_at is null and (profile.expires_at is null or profile.expires_at>current_timestamp) and coalesce((select setting.enabled from channel_chat_agent_settings setting where setting.channel_id=c.id and setting.agent_id=a.agent_id),c.kind!='private')";

async fn load_job_location_postgres(
    pool: &sqlx::PgPool,
    job: &str,
    share: &str,
) -> Result<Option<JobLocation>> {
    let query = format!(
        "select cast(s.share_id as text) share_id,s.latitude,s.longitude,s.accuracy_m,s.observed_at,s.expires_at from circle_chat_agent_jobs j join messages source on source.id=j.source_message_id join users author on author.id=source.sender_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles profile on profile.agent_id=j.agent_id join agent_location_shares s on {access} where j.id=$1::text::uuid and s.share_id=$2::text::uuid",
        access = JOB_ACCESS.replace("current_timestamp", "clock_timestamp()")
    );
    let row = sqlx::query(&query)
        .bind(job)
        .bind(share)
        .fetch_optional(pool)
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::PermissionDenied)?;
    Ok(Some(JobLocation {
        location: pg_location_from_row(&row)?,
    }))
}

async fn load_job_location_sqlite(
    pool: &sqlx::SqlitePool,
    job: &str,
    share: &str,
) -> Result<Option<JobLocation>> {
    let query = sqlite_times(&format!(
        "select s.share_id,s.latitude,s.longitude,s.accuracy_m,s.observed_at,s.expires_at from circle_chat_agent_jobs j join messages source on source.id=j.source_message_id join users author on author.id=source.sender_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles profile on profile.agent_id=j.agent_id join agent_location_shares s on {JOB_ACCESS} where j.id=? and s.share_id=?"
    ));
    let row = sqlx::query(&query)
        .bind(job)
        .bind(share)
        .fetch_optional(pool)
        .await
        .map_err(storage)?
        .ok_or(RepositoryError::PermissionDenied)?;
    Ok(Some(JobLocation {
        location: location_from_row(&row)?,
    }))
}

async fn validate_bound_postgres<'e, E>(
    executor: E,
    job: &str,
    share: &str,
    publication: bool,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Postgres>,
{
    let deadline = if publication {
        " and (j.weather_valid_until=0 or j.weather_valid_until>extract(epoch from clock_timestamp()))"
    } else {
        ""
    };
    let query = format!(
        "select 1 from circle_chat_agent_jobs j join messages source on source.id=j.source_message_id join users author on author.id=source.sender_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles profile on profile.agent_id=j.agent_id join agent_location_shares s on {access} where j.id=$1::text::uuid and s.share_id=$2::text::uuid{deadline}",
        access = JOB_ACCESS.replace("current_timestamp", "clock_timestamp()")
    );
    if sqlx::query_scalar::<_, i32>(&query)
        .bind(job)
        .bind(share)
        .fetch_optional(executor)
        .await
        .map_err(storage)?
        .is_none()
    {
        return Err(RepositoryError::PermissionDenied);
    }
    Ok(())
}

async fn validate_bound_sqlite<'e, E>(
    executor: E,
    job: &str,
    share: &str,
    publication: bool,
) -> Result<()>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let deadline = if publication {
        " and (j.weather_valid_until=0 or j.weather_valid_until>cast(strftime('%s','now') as integer))"
    } else {
        ""
    };
    let query = sqlite_times(&format!(
        "select 1 from circle_chat_agent_jobs j join messages source on source.id=j.source_message_id join users author on author.id=source.sender_id join channels c on c.id=j.channel_id join circle_chat_agents a on a.agent_id=j.agent_id join agent_profiles profile on profile.agent_id=j.agent_id join agent_location_shares s on {JOB_ACCESS} where j.id=? and s.share_id=?{deadline}"
    ));
    if sqlx::query_scalar::<_, i32>(&query)
        .bind(job)
        .bind(share)
        .fetch_optional(executor)
        .await
        .map_err(storage)?
        .is_none()
    {
        return Err(RepositoryError::PermissionDenied);
    }
    Ok(())
}

const PURGE_SET: &str = "reply_body=null,weather_snapshot=null,weather_valid_until=null,ferry_snapshot=null,ferry_valid_until=null,vision_snapshot=null,observation_snapshot=null,observation_valid_until=null";

async fn cleanup_postgres(pool: &sqlx::PgPool) -> Result<()> {
    let mut tx = pool.begin().await.map_err(storage)?;
    sqlx::query("delete from agent_location_shares where share_id in (select share_id from agent_location_shares where expires_at<=clock_timestamp() order by expires_at,share_id limit 100 for update skip locked)")
        .execute(&mut *tx).await.map_err(storage)?;
    tx.commit().await.map_err(storage)?;

    // Purging happens after the share locks are released. Publication already
    // fails closed as soon as a share is revoked or expires.
    let mut tx = pool.begin().await.map_err(storage)?;
    sqlx::query(&format!("update circle_chat_agent_jobs j set {PURGE_SET} where j.id in (select job.id from circle_chat_agent_jobs job left join agent_location_shares s on s.share_id=job.location_share_id where job.location_share_id is not null and s.share_id is null and job.reply_message_id is null and (job.reply_body is not null or job.weather_snapshot is not null or job.weather_valid_until is not null or job.ferry_snapshot is not null or job.ferry_valid_until is not null or job.vision_snapshot is not null or job.observation_snapshot is not null or job.observation_valid_until is not null) order by job.created_at,job.id limit 100 for update of job skip locked)"))
        .execute(&mut *tx).await.map_err(storage)?;
    tx.commit().await.map_err(storage)
}

async fn cleanup_sqlite(pool: &sqlx::SqlitePool) -> Result<()> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(storage)?;
    sqlx::query("delete from agent_location_shares where share_id in (select share_id from agent_location_shares where julianday(expires_at)<=julianday('now') order by expires_at,share_id limit 100)")
        .execute(&mut *tx).await.map_err(storage)?;
    tx.commit().await.map_err(storage)?;

    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.map_err(storage)?;
    sqlx::query(&format!("update circle_chat_agent_jobs set {PURGE_SET} where id in (select job.id from circle_chat_agent_jobs job left join agent_location_shares s on s.share_id=job.location_share_id where job.location_share_id is not null and s.share_id is null and job.reply_message_id is null and (job.reply_body is not null or job.weather_snapshot is not null or job.weather_valid_until is not null or job.ferry_snapshot is not null or job.ferry_valid_until is not null or job.vision_snapshot is not null or job.observation_snapshot is not null or job.observation_valid_until is not null) order by job.created_at,job.id limit 100)"))
        .execute(&mut *tx).await.map_err(storage)?;
    tx.commit().await.map_err(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChannelId, ChannelSequence, DisplayName, MessageBody, MessageId};

    fn service(pool: sqlx::SqlitePool) -> CircleChatAgents {
        CircleChatAgents {
            store: Store::Sqlite(pool),
            model: None,
            worker_enabled: false,
            weather: None,
            ferry: None,
            observations: None,
            imagegen: None,
        }
    }

    #[test]
    fn input_validation_rounds_and_rejects_stale_or_invalid_fixes() {
        let now = Utc::now();
        let fix = normalize(
            LocationInput {
                latitude: 60.123_456,
                longitude: 5.987_654,
                accuracy_m: 12.5,
                observed_at: now,
            },
            now,
        )
        .unwrap();
        assert_eq!(fix.latitude, 60.1235);
        assert_eq!(fix.longitude, 5.9877);
        for bad in [
            LocationInput {
                latitude: 91.0,
                ..fix.clone()
            },
            LocationInput {
                accuracy_m: f64::NAN,
                ..fix.clone()
            },
            LocationInput {
                observed_at: now - ChronoDuration::minutes(6),
                ..fix.clone()
            },
            LocationInput {
                observed_at: now + ChronoDuration::seconds(31),
                ..fix.clone()
            },
        ] {
            assert!(matches!(
                normalize(bad, now),
                Err(RepositoryError::Conflict)
            ));
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn sqlite_share_is_author_scoped_bound_once_and_revocable_after_access_loss() {
        TEST_ENABLED.scope(true, async {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .unwrap();

        let owner = Uuid::now_v7();
        let other = Uuid::now_v7();
        let agent = Uuid::now_v7();
        let circle = Uuid::now_v7();
        let channel = Uuid::now_v7();
        for (id, kind, name) in [
            (owner, "human", "Owner"),
            (other, "human", "Other"),
            (agent, "agent", "Agent"),
        ] {
            sqlx::query("insert into users(id,kind,display_name) values(?,?,?)")
                .bind(id.to_string())
                .bind(kind)
                .bind(name)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("insert into circles(id,slug,name,created_by) values(?,'location-test','Location test',?)")
            .bind(circle.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        for (id, role) in [(owner, "owner"), (other, "member")] {
            sqlx::query("insert into circle_memberships(circle_id,user_id,role) values(?,?,?)")
                .bind(circle.to_string())
                .bind(id.to_string())
                .bind(role)
                .execute(&pool)
                .await
                .unwrap();
        }
        sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values(?,'location-channel','Location','public',?,?)")
            .bind(channel.to_string()).bind(owner.to_string()).bind(circle.to_string()).execute(&pool).await.unwrap();
        for id in [owner, other] {
            sqlx::query(
                "insert into channel_memberships(channel_id,user_id,role) values(?,?,'member')",
            )
            .bind(channel.to_string())
            .bind(id.to_string())
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values(?,?,?,'test',?,'test',60,current_timestamp)")
            .bind(agent.to_string()).bind(owner.to_string()).bind(owner.to_string()).bind(format!("location-{agent}")).execute(&pool).await.unwrap();
        let now_epoch = Utc::now().timestamp();
        sqlx::query("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values(?,?,'[]','[]',1,?,?,?,?)")
            .bind(agent.to_string()).bind(circle.to_string()).bind(owner.to_string()).bind(owner.to_string()).bind(now_epoch).bind(now_epoch).execute(&pool).await.unwrap();

        let service = service(pool.clone());
        let actor = UserId::new(owner.to_string()).unwrap();
        let other_actor = UserId::new(other.to_string()).unwrap();
        let first = service
            .put_location(
                &actor,
                &channel.to_string(),
                &agent.to_string(),
                LocationInput {
                    latitude: 60.123_456,
                    longitude: 5.987_654,
                    accuracy_m: 8.0,
                    observed_at: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert_eq!((first.latitude, first.longitude), (60.1235, 5.9877));
        assert!(
            service
                .list_locations(&other_actor, &channel.to_string())
                .await
                .unwrap()
                .agents[0]
                .location
                .is_none()
        );

        let message_id = Uuid::now_v7();
        sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values(?,?,?,1,'where am I','Owner')")
            .bind(message_id.to_string()).bind(channel.to_string()).bind(owner.to_string()).execute(&pool).await.unwrap();
        let message = ChatMessage {
            id: MessageId::from_uuid(message_id),
            channel_id: ChannelId::new(channel.to_string()).unwrap(),
            parent_message_id: None,
            sender_id: actor.clone(),
            sender_display_name: DisplayName::new("Owner").unwrap(),
            body: MessageBody::new("where am I").unwrap(),
            sequence: ChannelSequence::try_from(1).unwrap(),
            sent_at: Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        let mut tx = pool.begin().await.unwrap();
        let bound = bind_sqlite(&mut tx, &message, &agent.to_string())
            .await
            .unwrap()
            .unwrap();
        tx.commit().await.unwrap();
        let job = Uuid::now_v7();
        sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,weather_valid_until,location_share_id) values(?,?,?,?,1,'leased',?,?, 'ready',0,?)")
            .bind(job.to_string()).bind(agent.to_string()).bind(message_id.to_string()).bind(channel.to_string()).bind(now_epoch).bind(now_epoch).bind(&bound).execute(&pool).await.unwrap();
        assert!(
            load_job_location_sqlite(&pool, &job.to_string(), &bound)
                .await
                .unwrap()
                .is_some()
        );
        let mut tx = pool.begin().await.unwrap();
        authorize_sqlite(&mut tx, &job.to_string()).await.unwrap();
        tx.rollback().await.unwrap();

        service
            .put_location(
                &actor,
                &channel.to_string(),
                &agent.to_string(),
                LocationInput {
                    latitude: 61.0,
                    longitude: 6.0,
                    accuracy_m: 4.0,
                    observed_at: Utc::now(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            load_job_location_sqlite(&pool, &job.to_string(), &bound).await,
            Err(RepositoryError::PermissionDenied)
        ));
        let replacement: String = sqlx::query_scalar("select share_id from agent_location_shares where user_id=? and agent_id=? and channel_id=?")
            .bind(owner.to_string()).bind(agent.to_string()).bind(channel.to_string()).fetch_one(&pool).await.unwrap();
        sqlx::query("update circle_chat_agent_jobs set location_share_id=? where id=?")
            .bind(&replacement).bind(job.to_string()).execute(&pool).await.unwrap();
        sqlx::query("update agent_location_shares set expires_at=strftime('%Y-%m-%dT%H:%M:%fZ','now','-1 second') where share_id=?")
            .bind(&replacement).execute(&pool).await.unwrap();
        assert!(matches!(
            load_job_location_sqlite(&pool, &job.to_string(), &replacement).await,
            Err(RepositoryError::PermissionDenied)
        ));
        service.put_location(&actor, &channel.to_string(), &agent.to_string(), LocationInput {
            latitude: 61.0, longitude: 6.0, accuracy_m: 4.0, observed_at: Utc::now(),
        }).await.unwrap();
        let unbound = Job {
            id: job.to_string(),
            agent_id: agent.to_string(),
            source_message_id: message_id.to_string(),
            channel_id: channel.to_string(),
            attempts: 0,
            reply_body: None,
            location_share_id: None,
            lease_token: String::new(),
        };
        assert!(service.location_for_job(&unbound).await.unwrap().is_none());

        sqlx::query("delete from channel_memberships where channel_id=? and user_id=?")
            .bind(channel.to_string())
            .bind(owner.to_string())
            .execute(&pool)
            .await
            .unwrap();
        service
            .delete_location(&actor, &channel.to_string(), &agent.to_string())
            .await
            .unwrap();
        let shares: i64 =
            sqlx::query_scalar("select count(*) from agent_location_shares where user_id=?")
                .bind(owner.to_string())
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(shares, 0);

        TEST_ENABLED.scope(false, async {
            let mut tx = pool.begin().await.unwrap();
            assert!(matches!(
                authorize_sqlite(&mut tx, &job.to_string()).await,
                Err(RepositoryError::PermissionDenied)
            ));
            tx.rollback().await.unwrap();
        }).await;
        }).await;
    }

    #[tokio::test]
    async fn postgres_share_binding_expiry_and_revoke_publication_fence() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        TEST_ENABLED.scope(true, async {
            let pool = sqlx::PgPool::connect(&url).await.unwrap();
            sqlx::migrate!("./migrations/postgres").run(&pool).await.unwrap();
            let owner = Uuid::now_v7();
            let agent = Uuid::now_v7();
            let circle = Uuid::now_v7();
            let channel = Uuid::now_v7();
            let suffix = Uuid::now_v7().simple().to_string();
            sqlx::query("insert into users(id,kind,display_name) values($1,'human','Owner'),($2,'agent','Agent')")
                .bind(owner).bind(agent).execute(&pool).await.unwrap();
            sqlx::query("insert into circles(id,slug,name,created_by) values($1,$2,'Location test',$3)")
                .bind(circle).bind(format!("location-{suffix}")).bind(owner).execute(&pool).await.unwrap();
            sqlx::query("insert into circle_memberships(circle_id,user_id,role) values($1,$2,'owner')")
                .bind(circle).bind(owner).execute(&pool).await.unwrap();
            sqlx::query("insert into channels(id,slug,name,kind,created_by,circle_id) values($1,$2,'Location','public',$3,$4)")
                .bind(channel).bind(format!("location-channel-{suffix}")).bind(owner).bind(circle).execute(&pool).await.unwrap();
            sqlx::query("insert into channel_memberships(channel_id,user_id,role) values($1,$2,'member')")
                .bind(channel).bind(owner).execute(&pool).await.unwrap();
            sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,created_at) values($1,$2,$2,'test',$3,'test',60,clock_timestamp())")
                .bind(agent).bind(owner).bind(format!("location-{agent}")).execute(&pool).await.unwrap();
            let epoch = Utc::now().timestamp();
            sqlx::query("insert into circle_chat_agents(agent_id,circle_id,trigger_words,response_phrases,enabled,created_by,updated_by,created_at,updated_at) values($1,$2,'[]','[]',true,$3,$3,$4,$4)")
                .bind(agent).bind(circle).bind(owner).bind(epoch).execute(&pool).await.unwrap();
            let service = CircleChatAgents {
                store: Store::Pg(pool.clone()), model: None, worker_enabled: false,
                weather: None, ferry: None, observations: None, imagegen: None,
            };
            let actor = UserId::new(owner.to_string()).unwrap();
            service.put_location(&actor, &channel.to_string(), &agent.to_string(), LocationInput {
                latitude: 60.123_456, longitude: 5.987_654, accuracy_m: 7.0, observed_at: Utc::now(),
            }).await.unwrap();
            let message_id = Uuid::now_v7();
            sqlx::query("insert into messages(id,channel_id,sender_id,sequence,body,sender_display_name) values($1,$2,$3,1,'where am I','Owner')")
                .bind(message_id).bind(channel).bind(owner).execute(&pool).await.unwrap();
            let message = ChatMessage {
                id: MessageId::from_uuid(message_id), channel_id: ChannelId::new(channel.to_string()).unwrap(),
                parent_message_id: None, sender_id: actor.clone(), sender_display_name: DisplayName::new("Owner").unwrap(),
                body: MessageBody::new("where am I").unwrap(), sequence: ChannelSequence::try_from(1).unwrap(),
                sent_at: Utc::now(), edited_at: None, deleted_at: None,
            };
            let mut tx = pool.begin().await.unwrap();
            let first = bind_postgres(&mut tx, &message, &agent.to_string()).await.unwrap().unwrap();
            tx.commit().await.unwrap();
            let job = Uuid::now_v7();
            sqlx::query("insert into circle_chat_agent_jobs(id,agent_id,source_message_id,channel_id,config_revision,status,available_at,created_at,reply_body,weather_valid_until,location_share_id) values($1,$2,$3,$4,1,'leased',$5,$5,'ready',0,$6)")
                .bind(job).bind(agent).bind(message_id).bind(channel).bind(epoch).bind(first).execute(&pool).await.unwrap();

            service.put_location(&actor, &channel.to_string(), &agent.to_string(), LocationInput {
                latitude: 61.0, longitude: 6.0, accuracy_m: 4.0, observed_at: Utc::now(),
            }).await.unwrap();
            assert!(matches!(load_job_location_postgres(&pool, &job.to_string(), &first.to_string()).await, Err(RepositoryError::PermissionDenied)));
            let replacement: Uuid = sqlx::query_scalar("select share_id from agent_location_shares where user_id=$1 and agent_id=$2 and channel_id=$3")
                .bind(owner).bind(agent).bind(channel).fetch_one(&pool).await.unwrap();
            sqlx::query("update circle_chat_agent_jobs set location_share_id=$1 where id=$2")
                .bind(replacement).bind(job).execute(&pool).await.unwrap();
            sqlx::query("update agent_location_shares set expires_at=clock_timestamp()-interval '1 second' where share_id=$1")
                .bind(replacement).execute(&pool).await.unwrap();
            assert!(matches!(load_job_location_postgres(&pool, &job.to_string(), &replacement.to_string()).await, Err(RepositoryError::PermissionDenied)));

            service.put_location(&actor, &channel.to_string(), &agent.to_string(), LocationInput {
                latitude: 61.0, longitude: 6.0, accuracy_m: 4.0, observed_at: Utc::now(),
            }).await.unwrap();
            let live: Uuid = sqlx::query_scalar("select share_id from agent_location_shares where user_id=$1 and agent_id=$2 and channel_id=$3")
                .bind(owner).bind(agent).bind(channel).fetch_one(&pool).await.unwrap();
            sqlx::query("update circle_chat_agent_jobs set location_share_id=$1 where id=$2")
                .bind(live).bind(job).execute(&pool).await.unwrap();
            let mut publication = pool.begin().await.unwrap();
            authorize_postgres(&mut publication, &job.to_string()).await.unwrap();
            let deleting = {
                let service = service.clone();
                let actor = actor.clone();
                tokio::spawn(async move { service.delete_location(&actor, &channel.to_string(), &agent.to_string()).await })
            };
            tokio::time::sleep(std::time::Duration::from_millis(75)).await;
            assert!(!deleting.is_finished(), "revocation bypassed the publication share lock");
            publication.rollback().await.unwrap();
            deleting.await.unwrap().unwrap();
            let mut denied = pool.begin().await.unwrap();
            assert!(matches!(authorize_postgres(&mut denied, &job.to_string()).await, Err(RepositoryError::PermissionDenied)));
            denied.rollback().await.unwrap();
        }).await;
    }
}
