use std::{sync::Arc, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Row, Sqlite, SqlitePool, Transaction};
use tokio::sync::watch;
use tracing::{info, warn};
use uuid::Uuid;
use web_push_native::{
    Auth, WebPushBuilder, jwt_simple::algorithms::ES256KeyPair, p256::PublicKey,
};

use crate::{
    config::{DatabaseConfig, DatabaseKind},
    domain::{ChatMessage, RepositoryError, UserId},
};

#[derive(Clone)]
enum NotificationStore {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

#[derive(Clone)]
pub struct NotificationService {
    store: NotificationStore,
    sender: Option<Arc<PushSender>>,
    public_key: Option<String>,
}

#[derive(Clone)]
struct PushSender {
    private_key: Vec<u8>,
    subject: String,
    client: reqwest::Client,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NotificationPreferences {
    pub mode: NotificationMode,
    pub direct_messages: bool,
    pub mentions: bool,
    pub weekly_weekday: i16,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    Instant,
    Weekly,
    Muted,
}

impl NotificationMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Instant => "instant",
            Self::Weekly => "weekly",
            Self::Muted => "muted",
        }
    }

    fn from_str(value: &str) -> Result<Self, RepositoryError> {
        match value {
            "instant" => Ok(Self::Instant),
            "weekly" => Ok(Self::Weekly),
            "muted" => Ok(Self::Muted),
            _ => Err(RepositoryError::Storage("invalid notification mode".into())),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PushSubscriptionInput {
    pub endpoint: String,
    pub keys: PushSubscriptionKeys,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PushSubscriptionKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Serialize)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub public_key: Option<String>,
    pub preferences: NotificationPreferences,
    pub subscriptions: i64,
}

#[derive(Debug)]
struct PushJob {
    subscription_id: Uuid,
    message_id: Uuid,
    endpoint: String,
    p256dh: String,
    auth: String,
    sender: String,
    channel: String,
    body: String,
    kind: String,
}

enum DeliveryResult {
    Delivered,
    Expired,
    Retry(String),
}

impl NotificationService {
    #[cfg(test)]
    pub fn test() -> Self {
        Self {
            store: NotificationStore::Sqlite(
                SqlitePool::connect_lazy("sqlite::memory:").expect("test SQLite URL is valid"),
            ),
            sender: None,
            public_key: None,
        }
    }

    pub async fn connect(config: &DatabaseConfig) -> Result<Self, RepositoryError> {
        let store = match config.kind() {
            DatabaseKind::Postgres => {
                NotificationStore::Postgres(PgPool::connect(config.url()).await.map_err(storage)?)
            }
            DatabaseKind::Sqlite => {
                NotificationStore::Sqlite(SqlitePool::connect(config.url()).await.map_err(storage)?)
            }
        };
        let public_key = std::env::var("SPROYT_VAPID_PUBLIC_KEY").ok();
        let private_key = std::env::var("SPROYT_VAPID_PRIVATE_KEY").ok();
        let subject = std::env::var("SPROYT_VAPID_SUBJECT").ok();
        let sender = match (private_key, subject) {
            (Some(private_key), Some(subject)) => {
                let private_key = URL_SAFE_NO_PAD.decode(private_key).map_err(|_| {
                    RepositoryError::Storage("SPROYT_VAPID_PRIVATE_KEY is not base64url".into())
                })?;
                if private_key.len() != 32
                    || !(subject.starts_with("mailto:") || subject.starts_with("https://"))
                {
                    return Err(RepositoryError::Storage(
                        "invalid VAPID configuration".into(),
                    ));
                }
                Some(Arc::new(PushSender {
                    private_key,
                    subject,
                    client: reqwest::Client::builder()
                        .timeout(Duration::from_secs(15))
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                        .map_err(storage)?,
                }))
            }
            (None, None) => None,
            _ => {
                return Err(RepositoryError::Storage(
                    "partial VAPID configuration".into(),
                ));
            }
        };
        if sender.is_some() && public_key.is_none() {
            return Err(RepositoryError::Storage(
                "SPROYT_VAPID_PUBLIC_KEY is required".into(),
            ));
        }
        Ok(Self {
            store,
            sender,
            public_key,
        })
    }

    pub fn start_worker(&self, mut shutdown: watch::Receiver<bool>) {
        let service = self.clone();
        tokio::spawn(async move {
            if service.sender.is_none() {
                info!("web push is disabled because VAPID is not configured");
                return;
            }
            // A rolling deployment can leave a very small hand-off interval
            // where the previous binary persisted a message just before its
            // next periodic scan. Reconcile that interval once; every row is
            // still constrained by the per-subscription cursor, and the
            // outbox primary key makes replica start-up races harmless.
            if let Err(error) = service.backfill_current_cursors_once().await {
                warn!(error_kind = %error.kind(), "web push start-up backfill failed");
            }
            loop {
                tokio::select! {
                    _ = shutdown.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {
                        if let Err(error) = service.run_once().await {
                            warn!(error_kind = %error.kind(), "web push worker iteration failed");
                        }
                    }
                }
            }
        });
    }

    pub async fn settings(&self, user_id: UserId) -> Result<NotificationSettings, RepositoryError> {
        let preferences = self.preferences(user_id.clone()).await?;
        let subscriptions = match &self.store {
            NotificationStore::Postgres(pool) => sqlx::query_scalar::<_, i64>(
                "select count(*) from push_subscriptions where user_id=$1",
            )
            .bind(user_id.as_uuid())
            .fetch_one(pool)
            .await
            .map_err(storage)?,
            NotificationStore::Sqlite(pool) => sqlx::query_scalar::<_, i64>(
                "select count(*) from push_subscriptions where user_id=?",
            )
            .bind(user_id.to_string())
            .fetch_one(pool)
            .await
            .map_err(storage)?,
        };
        Ok(NotificationSettings {
            enabled: self.sender.is_some(),
            public_key: self.public_key.clone(),
            preferences,
            subscriptions,
        })
    }

    pub async fn save_preferences(
        &self,
        user_id: UserId,
        value: NotificationPreferences,
    ) -> Result<NotificationPreferences, RepositoryError> {
        if !(1..=7).contains(&value.weekly_weekday) {
            return Err(RepositoryError::Conflict);
        }
        match &self.store {
            NotificationStore::Postgres(pool) => {
                sqlx::query("insert into notification_preferences(user_id,mode,direct_messages,mentions,weekly_weekday) values($1,$2,$3,$4,$5) on conflict(user_id) do update set mode=excluded.mode,direct_messages=excluded.direct_messages,mentions=excluded.mentions,weekly_weekday=excluded.weekly_weekday,updated_at=now()").bind(user_id.as_uuid()).bind(value.mode.as_str()).bind(value.direct_messages).bind(value.mentions).bind(value.weekly_weekday).execute(pool).await.map_err(storage)?;
            }
            NotificationStore::Sqlite(pool) => {
                sqlx::query("insert into notification_preferences(user_id,mode,direct_messages,mentions,weekly_weekday) values(?,?,?,?,?) on conflict(user_id) do update set mode=excluded.mode,direct_messages=excluded.direct_messages,mentions=excluded.mentions,weekly_weekday=excluded.weekly_weekday,updated_at=current_timestamp").bind(user_id.to_string()).bind(value.mode.as_str()).bind(value.direct_messages).bind(value.mentions).bind(value.weekly_weekday).execute(pool).await.map_err(storage)?;
            }
        }
        Ok(value)
    }

    pub async fn subscribe(
        &self,
        user_id: UserId,
        input: PushSubscriptionInput,
        user_agent: Option<String>,
    ) -> Result<(), RepositoryError> {
        validate_subscription(&input)?;
        // The database supplies the initial high-water mark. This is shared
        // across pods and avoids timestamp precision gaps (SQLite's database
        // clock is second-granular).
        let id = Uuid::now_v7();
        match &self.store {
            NotificationStore::Postgres(pool) => {
                sqlx::query("insert into push_subscriptions(id,user_id,endpoint,p256dh,auth,user_agent,notification_after_message_id) values($1,$2,$3,$4,$5,$6,coalesce((select id from messages order by id desc limit 1),'00000000-0000-7000-8000-000000000000'::uuid)) on conflict(endpoint) do update set p256dh=excluded.p256dh,auth=excluded.auth,user_agent=excluded.user_agent where push_subscriptions.user_id=excluded.user_id").bind(id).bind(user_id.as_uuid()).bind(input.endpoint).bind(input.keys.p256dh).bind(input.keys.auth).bind(user_agent).execute(pool).await.map_err(storage)?;
            }
            NotificationStore::Sqlite(pool) => {
                sqlx::query("insert into push_subscriptions(id,user_id,endpoint,p256dh,auth,user_agent,notification_after_message_id) values(?,?,?,?,?,?,coalesce((select id from messages order by id desc limit 1),'00000000-0000-7000-8000-000000000000')) on conflict(endpoint) do update set p256dh=excluded.p256dh,auth=excluded.auth,user_agent=excluded.user_agent where push_subscriptions.user_id=excluded.user_id").bind(id.to_string()).bind(user_id.to_string()).bind(input.endpoint).bind(input.keys.p256dh).bind(input.keys.auth).bind(user_agent).execute(pool).await.map_err(storage)?;
            }
        }
        Ok(())
    }

    pub async fn unsubscribe(
        &self,
        user_id: UserId,
        endpoint: String,
    ) -> Result<(), RepositoryError> {
        match &self.store {
            NotificationStore::Postgres(pool) => {
                sqlx::query("delete from push_subscriptions where user_id=$1 and endpoint=$2")
                    .bind(user_id.as_uuid())
                    .bind(endpoint)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
            NotificationStore::Sqlite(pool) => {
                sqlx::query("delete from push_subscriptions where user_id=? and endpoint=?")
                    .bind(user_id.to_string())
                    .bind(endpoint)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
        }
        Ok(())
    }

    async fn preferences(
        &self,
        user_id: UserId,
    ) -> Result<NotificationPreferences, RepositoryError> {
        let row = match &self.store {
            NotificationStore::Postgres(pool) => sqlx::query("select mode,direct_messages,mentions,weekly_weekday from notification_preferences where user_id=$1").bind(user_id.as_uuid()).fetch_optional(pool).await.map_err(storage)?.map(|row| (row.get::<String,_>(0),row.get::<bool,_>(1),row.get::<bool,_>(2),row.get::<i16,_>(3))),
            NotificationStore::Sqlite(pool) => sqlx::query("select mode,direct_messages,mentions,weekly_weekday from notification_preferences where user_id=?").bind(user_id.to_string()).fetch_optional(pool).await.map_err(storage)?.map(|row| (row.get::<String,_>(0),row.get::<bool,_>(1),row.get::<bool,_>(2),row.get::<i16,_>(3))),
        };
        let Some((mode, direct_messages, mentions, weekly_weekday)) = row else {
            return Ok(NotificationPreferences {
                mode: NotificationMode::Instant,
                direct_messages: true,
                mentions: true,
                weekly_weekday: 1,
            });
        };
        Ok(NotificationPreferences {
            mode: NotificationMode::from_str(&mode)?,
            direct_messages,
            mentions,
            weekly_weekday,
        })
    }

    async fn run_once(&self) -> Result<(), RepositoryError> {
        let Some(job) = self.claim().await? else {
            return Ok(());
        };
        let result = self
            .sender
            .as_ref()
            .expect("worker requires sender")
            .deliver(&job)
            .await;
        self.complete(job, result).await
    }

    async fn backfill_current_cursors_once(&self) -> Result<(), RepositoryError> {
        // The cursor is set once, when a device first opts in. Re-registering
        // the same endpoint keeps it intact, so outstanding notifications are
        // not discarded. This is intentionally called only during worker
        // start-up to bridge a mixed-version rolling deployment; it is not the
        // delivery loop's old periodic global scan.
        let mention_pg = "insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,mm.mentioned_user_id,mm.message_id,'mention' from message_mentions mm join messages m on m.id=mm.message_id join push_subscriptions s on s.user_id=mm.mentioned_user_id left join notification_preferences p on p.user_id=mm.mentioned_user_id where mm.mentioned_user_id<>m.sender_id and m.id>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.mentions,true) on conflict(subscription_id,message_id) do nothing";
        let direct_pg = "insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,case when d.user_a_id=m.sender_id then d.user_b_id else d.user_a_id end,m.id,'direct_message' from messages m join direct_conversations d on d.channel_id=m.channel_id join push_subscriptions s on s.user_id=case when d.user_a_id=m.sender_id then d.user_b_id else d.user_a_id end left join notification_preferences p on p.user_id=s.user_id where m.id>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,true) on conflict(subscription_id,message_id) do nothing";
        let group_direct_pg = "insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,cm.user_id,m.id,'direct_message' from messages m join direct_group_conversations g on g.channel_id=m.channel_id join channel_memberships cm on cm.channel_id=m.channel_id join users u on u.id=cm.user_id and u.kind='human' join push_subscriptions s on s.user_id=cm.user_id left join notification_preferences p on p.user_id=s.user_id where cm.user_id<>m.sender_id and m.id>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,true) on conflict(subscription_id,message_id) do nothing";
        match &self.store {
            NotificationStore::Postgres(pool) => {
                sqlx::query(mention_pg)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
                sqlx::query(direct_pg)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
                sqlx::query(group_direct_pg)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
            NotificationStore::Sqlite(pool) => {
                let mention =
                    mention_pg.replace("coalesce(p.mentions,true)", "coalesce(p.mentions,1)");
                let direct = direct_pg.replace(
                    "coalesce(p.direct_messages,true)",
                    "coalesce(p.direct_messages,1)",
                );
                let group_direct = group_direct_pg.replace(
                    "coalesce(p.direct_messages,true)",
                    "coalesce(p.direct_messages,1)",
                );
                sqlx::query(&mention).execute(pool).await.map_err(storage)?;
                sqlx::query(&direct).execute(pool).await.map_err(storage)?;
                sqlx::query(&group_direct)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
        }
        Ok(())
    }

    async fn claim(&self) -> Result<Option<PushJob>, RepositoryError> {
        let select_pg = "select o.subscription_id,o.message_id,s.endpoint,s.p256dh,s.auth,m.sender_display_name,c.id,m.body,o.kind from notification_outbox o join push_subscriptions s on s.id=o.subscription_id join messages m on m.id=o.message_id join channels c on c.id=m.channel_id where o.delivered_at is null and o.available_at<=now() and (o.leased_until is null or o.leased_until<now()) order by o.created_at limit 1";
        let row = match &self.store {
            NotificationStore::Postgres(pool) => sqlx::query(select_pg)
                .fetch_optional(pool)
                .await
                .map_err(storage)?
                .map(push_job_pg),
            NotificationStore::Sqlite(pool) => {
                sqlx::query(&select_pg.replace("now()", "current_timestamp"))
                    .fetch_optional(pool)
                    .await
                    .map_err(storage)?
                    .map(push_job_sqlite)
            }
        }
        .transpose()?;
        let Some(job) = row else { return Ok(None) };
        let claimed = match &self.store {
            NotificationStore::Postgres(pool) => sqlx::query("update notification_outbox set leased_until=now()+interval '30 seconds',attempts=attempts+1 where subscription_id=$1 and message_id=$2 and delivered_at is null and (leased_until is null or leased_until<now())").bind(job.subscription_id).bind(job.message_id).execute(pool).await.map_err(storage)?.rows_affected(),
            NotificationStore::Sqlite(pool) => sqlx::query("update notification_outbox set leased_until=datetime('now','+30 seconds'),attempts=attempts+1 where subscription_id=? and message_id=? and delivered_at is null and (leased_until is null or leased_until<current_timestamp)").bind(job.subscription_id.to_string()).bind(job.message_id.to_string()).execute(pool).await.map_err(storage)?.rows_affected(),
        };
        if claimed == 0 {
            return Ok(None);
        }
        Ok(Some(job))
    }

    async fn complete(&self, job: PushJob, result: DeliveryResult) -> Result<(), RepositoryError> {
        match (&self.store, result) {
            (NotificationStore::Postgres(pool), DeliveryResult::Delivered) => {
                sqlx::query("update notification_outbox set delivered_at=now(),leased_until=null,last_error=null where subscription_id=$1 and message_id=$2").bind(job.subscription_id).bind(job.message_id).execute(pool).await.map_err(storage)?;
                sqlx::query("update push_subscriptions set last_success_at=now(),failure_count=0 where id=$1").bind(job.subscription_id).execute(pool).await.map_err(storage)?;
            }
            (NotificationStore::Sqlite(pool), DeliveryResult::Delivered) => {
                sqlx::query("update notification_outbox set delivered_at=current_timestamp,leased_until=null,last_error=null where subscription_id=? and message_id=?").bind(job.subscription_id.to_string()).bind(job.message_id.to_string()).execute(pool).await.map_err(storage)?;
                sqlx::query("update push_subscriptions set last_success_at=current_timestamp,failure_count=0 where id=?").bind(job.subscription_id.to_string()).execute(pool).await.map_err(storage)?;
            }
            (NotificationStore::Postgres(pool), DeliveryResult::Expired) => {
                sqlx::query("delete from push_subscriptions where id=$1")
                    .bind(job.subscription_id)
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
            (NotificationStore::Sqlite(pool), DeliveryResult::Expired) => {
                sqlx::query("delete from push_subscriptions where id=?")
                    .bind(job.subscription_id.to_string())
                    .execute(pool)
                    .await
                    .map_err(storage)?;
            }
            (NotificationStore::Postgres(pool), DeliveryResult::Retry(error)) => {
                sqlx::query("update notification_outbox set leased_until=null,available_at=now()+interval '30 seconds',last_error=$3 where subscription_id=$1 and message_id=$2").bind(job.subscription_id).bind(job.message_id).bind(redact_error(error)).execute(pool).await.map_err(storage)?;
            }
            (NotificationStore::Sqlite(pool), DeliveryResult::Retry(error)) => {
                sqlx::query("update notification_outbox set leased_until=null,available_at=datetime('now','+30 seconds'),last_error=? where subscription_id=? and message_id=?").bind(redact_error(error)).bind(job.subscription_id.to_string()).bind(job.message_id.to_string()).execute(pool).await.map_err(storage)?;
            }
        }
        Ok(())
    }
}

/// Adds web-push work while the message transaction is still open.  A fresh
/// subscription's database UUIDv7 high-water cursor is the boundary: historical messages never
/// enter the outbox. Mentions deliberately go first; the outbox primary key
/// then gives a message that is both a DM and an @mention one, more specific
/// notification instead of two.
pub(crate) async fn enqueue_message_postgres(
    transaction: &mut Transaction<'_, Postgres>,
    message: &ChatMessage,
) -> Result<(), RepositoryError> {
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,mm.mentioned_user_id,mm.message_id,'mention' from message_mentions mm join push_subscriptions s on s.user_id=mm.mentioned_user_id left join notification_preferences p on p.user_id=mm.mentioned_user_id where mm.message_id=$1 and mm.mentioned_user_id<>$2 and mm.message_id>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.mentions,true) on conflict(subscription_id,message_id) do nothing")
        .bind(*message.id.as_uuid())
        .bind(*message.sender_id.as_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,case when d.user_a_id=$2 then d.user_b_id else d.user_a_id end,$3,'direct_message' from direct_conversations d join push_subscriptions s on s.user_id=case when d.user_a_id=$2 then d.user_b_id else d.user_a_id end left join notification_preferences p on p.user_id=s.user_id where d.channel_id=$1 and $3>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,true) on conflict(subscription_id,message_id) do nothing")
        .bind(*message.channel_id.as_uuid())
        .bind(*message.sender_id.as_uuid())
        .bind(*message.id.as_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,cm.user_id,$2,'direct_message' from direct_group_conversations g join channel_memberships cm on cm.channel_id=g.channel_id join users u on u.id=cm.user_id and u.kind='human' join push_subscriptions s on s.user_id=cm.user_id left join notification_preferences p on p.user_id=s.user_id where g.channel_id=$1 and cm.user_id<>$3 and $2>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,true) on conflict(subscription_id,message_id) do nothing")
        .bind(*message.channel_id.as_uuid())
        .bind(*message.id.as_uuid())
        .bind(*message.sender_id.as_uuid())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    Ok(())
}

pub(crate) async fn enqueue_message_sqlite(
    transaction: &mut Transaction<'_, Sqlite>,
    message: &ChatMessage,
) -> Result<(), RepositoryError> {
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,mm.mentioned_user_id,mm.message_id,'mention' from message_mentions mm join push_subscriptions s on s.user_id=mm.mentioned_user_id left join notification_preferences p on p.user_id=mm.mentioned_user_id where mm.message_id=? and mm.mentioned_user_id<>? and mm.message_id>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.mentions,1) on conflict(subscription_id,message_id) do nothing")
        .bind(message.id.as_uuid().to_string())
        .bind(message.sender_id.to_string())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,case when d.user_a_id=? then d.user_b_id else d.user_a_id end,?,'direct_message' from direct_conversations d join push_subscriptions s on s.user_id=case when d.user_a_id=? then d.user_b_id else d.user_a_id end left join notification_preferences p on p.user_id=s.user_id where d.channel_id=? and ?>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,1) on conflict(subscription_id,message_id) do nothing")
        .bind(message.sender_id.to_string())
        .bind(message.id.as_uuid().to_string())
        .bind(message.sender_id.to_string())
        .bind(message.channel_id.to_string())
        .bind(message.id.as_uuid().to_string())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) select s.id,cm.user_id,?,'direct_message' from direct_group_conversations g join channel_memberships cm on cm.channel_id=g.channel_id join users u on u.id=cm.user_id and u.kind='human' join push_subscriptions s on s.user_id=cm.user_id left join notification_preferences p on p.user_id=s.user_id where g.channel_id=? and cm.user_id<>? and ?>s.notification_after_message_id and coalesce(p.mode,'instant')='instant' and coalesce(p.direct_messages,1) on conflict(subscription_id,message_id) do nothing")
        .bind(message.id.as_uuid().to_string())
        .bind(message.channel_id.to_string())
        .bind(message.sender_id.to_string())
        .bind(message.id.as_uuid().to_string())
        .execute(&mut **transaction)
        .await
        .map_err(storage)?;
    Ok(())
}

impl PushSender {
    async fn deliver(&self, job: &PushJob) -> DeliveryResult {
        let result = (|| {
            let key_pair =
                ES256KeyPair::from_bytes(&self.private_key).map_err(|error| error.to_string())?;
            let peer = PublicKey::from_sec1_bytes(
                &URL_SAFE_NO_PAD
                    .decode(&job.p256dh)
                    .map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
            let auth = URL_SAFE_NO_PAD
                .decode(&job.auth)
                .map_err(|error| error.to_string())?;
            let builder = WebPushBuilder::new(
                job.endpoint
                    .parse()
                    .map_err(|error: http::uri::InvalidUri| error.to_string())?,
                peer,
                Auth::clone_from_slice(&auth),
            )
            .with_vapid(&key_pair, &self.subject);
            let title = if job.kind == "mention" {
                format!("{} omtalte deg", job.sender)
            } else {
                format!("Melding frå {}", job.sender)
            };
            let payload = serde_json::to_vec(&serde_json::json!({"web_push":8030,"notification":{"title":title,"body":notification_body(&job.body),"navigate":format!("/?channel={}&message={}",job.channel,job.message_id),"tag":format!("message-{}",job.message_id)}})).map_err(|error| error.to_string())?;
            builder.build(payload).map_err(|error| error.to_string())
        })();
        let request = match result {
            Ok(request) => request,
            Err(error) => return DeliveryResult::Retry(error),
        };
        let (parts, body) = request.into_parts();
        match self
            .client
            .request(parts.method, parts.uri.to_string())
            .headers(parts.headers)
            .body(body)
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => DeliveryResult::Delivered,
            Ok(response) if matches!(response.status().as_u16(), 404 | 410) => {
                DeliveryResult::Expired
            }
            Ok(response) => {
                DeliveryResult::Retry(format!("push service returned {}", response.status()))
            }
            Err(error) => DeliveryResult::Retry(error.to_string()),
        }
    }
}

fn validate_subscription(input: &PushSubscriptionInput) -> Result<(), RepositoryError> {
    let endpoint = reqwest::Url::parse(&input.endpoint).map_err(|_| RepositoryError::Conflict)?;
    let host = endpoint.host_str().ok_or(RepositoryError::Conflict)?;
    let allowed_host = [
        "googleapis.com",
        "google.com",
        "push.services.mozilla.com",
        "push.apple.com",
        "notify.windows.com",
        "push.samsungosp.com",
    ]
    .iter()
    .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")));
    let public_key = URL_SAFE_NO_PAD
        .decode(&input.keys.p256dh)
        .map_err(|_| RepositoryError::Conflict)?;
    let auth = URL_SAFE_NO_PAD
        .decode(&input.keys.auth)
        .map_err(|_| RepositoryError::Conflict)?;
    if endpoint.scheme() != "https"
        || endpoint.port_or_known_default() != Some(443)
        || !allowed_host
        || input.endpoint.len() > 4096
        || public_key.len() != 65
        || PublicKey::from_sec1_bytes(&public_key).is_err()
        || auth.len() != 16
    {
        return Err(RepositoryError::Conflict);
    }
    Ok(())
}

fn push_job_pg(row: sqlx::postgres::PgRow) -> Result<PushJob, RepositoryError> {
    Ok(PushJob {
        subscription_id: row.try_get(0).map_err(storage)?,
        message_id: row.try_get(1).map_err(storage)?,
        endpoint: row.try_get(2).map_err(storage)?,
        p256dh: row.try_get(3).map_err(storage)?,
        auth: row.try_get(4).map_err(storage)?,
        sender: row.try_get(5).map_err(storage)?,
        channel: row.try_get::<Uuid, _>(6).map_err(storage)?.to_string(),
        body: row.try_get(7).map_err(storage)?,
        kind: row.try_get(8).map_err(storage)?,
    })
}
fn push_job_sqlite(row: sqlx::sqlite::SqliteRow) -> Result<PushJob, RepositoryError> {
    Ok(PushJob {
        subscription_id: Uuid::parse_str(row.try_get::<String, _>(0).map_err(storage)?.as_str())
            .map_err(storage)?,
        message_id: Uuid::parse_str(row.try_get::<String, _>(1).map_err(storage)?.as_str())
            .map_err(storage)?,
        endpoint: row.try_get(2).map_err(storage)?,
        p256dh: row.try_get(3).map_err(storage)?,
        auth: row.try_get(4).map_err(storage)?,
        sender: row.try_get(5).map_err(storage)?,
        channel: row.try_get(6).map_err(storage)?,
        body: row.try_get(7).map_err(storage)?,
        kind: row.try_get(8).map_err(storage)?,
    })
}
fn redact_error(error: String) -> String {
    error.chars().take(240).collect()
}
fn notification_body(body: &str) -> String {
    let visible = body.split("[[media:").next().unwrap_or(body).trim();
    let mut value: String = visible.chars().take(180).collect();
    if visible.chars().count() > 180 {
        value.push('…');
    }
    if value.is_empty() {
        "Vedlegg".to_owned()
    } else {
        value
    }
}
fn storage(error: impl std::fmt::Display) -> RepositoryError {
    RepositoryError::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    async fn service() -> NotificationService {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(":memory:")
                    .create_if_missing(true)
                    .foreign_keys(true),
            )
            .await
            .expect("SQLite starts");
        sqlx::migrate!("./migrations/sqlite")
            .run(&pool)
            .await
            .expect("notification migrations apply");
        NotificationService {
            store: NotificationStore::Sqlite(pool),
            sender: None,
            public_key: None,
        }
    }

    #[test]
    fn notification_text_hides_media_tokens_and_is_bounded() {
        assert_eq!(notification_body("[[media:abc]]"), "Vedlegg");
        assert_eq!(notification_body("Hei! [[media:abc]]"), "Hei!");
        let long = "x".repeat(181);
        assert_eq!(notification_body(&long).chars().count(), 181);
        assert!(notification_body(&long).ends_with('…'));
    }

    #[test]
    fn subscriptions_require_https_and_plausible_keys() {
        let input = PushSubscriptionInput {
            endpoint: "http://push.example/subscription".into(),
            keys: PushSubscriptionKeys {
                p256dh: "p".repeat(65),
                auth: "a".repeat(22),
            },
        };
        assert!(validate_subscription(&input).is_err());
    }

    #[test]
    fn subscriptions_reject_non_push_hosts() {
        let input = PushSubscriptionInput {
            endpoint: "https://postgres-postgresql.database.svc.cluster.local/subscription".into(),
            keys: PushSubscriptionKeys {
                p256dh: "p".repeat(65),
                auth: "a".repeat(22),
            },
        };
        assert!(validate_subscription(&input).is_err());
    }

    #[tokio::test]
    async fn group_direct_notifies_all_other_human_members_and_keeps_mention_priority() {
        let service = service().await;
        let NotificationStore::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        let sender = Uuid::now_v7();
        let mentioned = Uuid::now_v7();
        let other = Uuid::now_v7();
        let agent = Uuid::now_v7();
        let channel = Uuid::now_v7();
        let message = Uuid::now_v7();
        for (id, kind, name) in [
            (sender, "human", "Sender"),
            (mentioned, "human", "Mentioned"),
            (other, "human", "Other"),
            (agent, "agent", "Agent"),
        ] {
            sqlx::query("insert into users(id,kind,display_name) values(?,?,?)")
                .bind(id.to_string())
                .bind(kind)
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query("insert into channels(id,slug,name,kind,created_by) values(?,?,?,'private',?)")
            .bind(channel.to_string())
            .bind(format!("dm-group-{channel}"))
            .bind("Group DM")
            .bind(sender.to_string())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query(
            "insert into direct_group_conversations(channel_id,source_channel_id) values(?,?)",
        )
        .bind(channel.to_string())
        .bind(channel.to_string())
        .execute(pool)
        .await
        .unwrap();
        for user in [sender, mentioned, other, agent] {
            sqlx::query(
                "insert into channel_memberships(channel_id,user_id,role) values(?,?,'member')",
            )
            .bind(channel.to_string())
            .bind(user.to_string())
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,?,1,'Hei @Mentioned')")
            .bind(message.to_string()).bind(channel.to_string()).bind(sender.to_string()).bind("Sender").execute(pool).await.unwrap();
        sqlx::query("insert into message_mentions(message_id,mentioned_user_id) values(?,?)")
            .bind(message.to_string())
            .bind(mentioned.to_string())
            .execute(pool)
            .await
            .unwrap();
        for user in [mentioned, other, agent] {
            sqlx::query("insert into push_subscriptions(id,user_id,endpoint,p256dh,auth,notification_after_message_id) values(?,?,?,?,?,?)")
                .bind(Uuid::now_v7().to_string()).bind(user.to_string()).bind(format!("https://push.example/{user}")).bind("p".repeat(65)).bind("a".repeat(22)).bind("00000000-0000-7000-8000-000000000000").execute(pool).await.unwrap();
        }
        let chat_message = ChatMessage {
            id: crate::domain::MessageId::from_uuid(message),
            channel_id: crate::domain::ChannelId::from_uuid(channel),
            parent_message_id: None,
            sender_id: UserId::from_uuid(sender),
            sender_display_name: crate::domain::DisplayName::new("Sender").unwrap(),
            body: crate::domain::MessageBody::new("Hei @Mentioned").unwrap(),
            sequence: crate::domain::ChannelSequence::first(),
            sent_at: chrono::Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        let mut tx = pool.begin().await.unwrap();
        enqueue_message_sqlite(&mut tx, &chat_message)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        let rows: Vec<(String, String)> = sqlx::query_as(
            "select recipient_id,kind from notification_outbox order by recipient_id",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(
            rows,
            vec![
                (mentioned.to_string(), "mention".into()),
                (other.to_string(), "direct_message".into())
            ]
        );
    }

    #[tokio::test]
    async fn outbox_is_deduplicated_and_respects_preferences() {
        let service = service().await;
        let NotificationStore::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        let sender = UserId::named("sender");
        let recipient = UserId::named("recipient");
        let muted = UserId::named("muted");
        let channel = Uuid::now_v7();
        let message = Uuid::now_v7();

        for (id, name) in [
            (&sender, "Sender"),
            (&recipient, "Recipient"),
            (&muted, "Muted"),
        ] {
            sqlx::query("insert into users(id,kind,display_name) values(?, 'human', ?)")
                .bind(id.to_string())
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "insert into channels(id,slug,name,kind,created_by) values(?,?,'DM','private',?)",
        )
        .bind(channel.to_string())
        .bind(format!("dm-{channel}"))
        .bind(sender.to_string())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "insert into direct_conversations(channel_id,user_a_id,user_b_id) values(?,?,?)",
        )
        .bind(channel.to_string())
        .bind(sender.to_string().min(recipient.to_string()))
        .bind(sender.to_string().max(recipient.to_string()))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,?,1,'Hei @recipient')")
            .bind(message.to_string())
            .bind(channel.to_string())
            .bind(sender.to_string())
            .bind("Sender")
            .execute(pool)
            .await
            .unwrap();
        for user in [&recipient, &muted] {
            sqlx::query(
                "insert into push_subscriptions(id,user_id,endpoint,p256dh,auth,notification_after_message_id) values(?,?,?,?,?,?)",
            )
            .bind(Uuid::now_v7().to_string())
            .bind(user.to_string())
            .bind(format!("https://push.example/{user}"))
            .bind("p".repeat(65))
            .bind("a".repeat(22))
            .bind("00000000-0000-7000-8000-000000000000")
            .execute(pool)
            .await
            .unwrap();
        }
        sqlx::query("insert into message_mentions(message_id,mentioned_user_id) values(?,?)")
            .bind(message.to_string())
            .bind(recipient.to_string())
            .execute(pool)
            .await
            .unwrap();
        service
            .save_preferences(
                muted.clone(),
                NotificationPreferences {
                    mode: NotificationMode::Muted,
                    direct_messages: true,
                    mentions: true,
                    weekly_weekday: 1,
                },
            )
            .await
            .unwrap();

        let chat_message = ChatMessage {
            id: crate::domain::MessageId::from_uuid(message),
            channel_id: crate::domain::ChannelId::from_uuid(channel),
            parent_message_id: None,
            sender_id: sender.clone(),
            sender_display_name: crate::domain::DisplayName::new("Sender").unwrap(),
            body: crate::domain::MessageBody::new("Hei @recipient").unwrap(),
            sequence: crate::domain::ChannelSequence::first(),
            sent_at: chrono::Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        let mut transaction = pool.begin().await.unwrap();
        enqueue_message_sqlite(&mut transaction, &chat_message)
            .await
            .unwrap();
        enqueue_message_sqlite(&mut transaction, &chat_message)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        let rows: Vec<(String, String)> = sqlx::query_as(
            "select recipient_id,kind from notification_outbox order by recipient_id",
        )
        .fetch_all(pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![(recipient.to_string(), "mention".into())]);
    }

    #[tokio::test]
    async fn a_new_device_does_not_enqueue_historical_notifications() {
        let service = service().await;
        let NotificationStore::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        let sender = UserId::named("sender");
        let recipient = UserId::named("recipient");
        let channel = Uuid::now_v7();
        let historical = Uuid::now_v7();
        for (id, name) in [(&sender, "Sender"), (&recipient, "Recipient")] {
            sqlx::query("insert into users(id,kind,display_name) values(?, 'human', ?)")
                .bind(id.to_string())
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "insert into channels(id,slug,name,kind,created_by) values(?,?,'DM','private',?)",
        )
        .bind(channel.to_string())
        .bind(format!("dm-{channel}"))
        .bind(sender.to_string())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query(
            "insert into direct_conversations(channel_id,user_a_id,user_b_id) values(?,?,?)",
        )
        .bind(channel.to_string())
        .bind(sender.to_string().min(recipient.to_string()))
        .bind(sender.to_string().max(recipient.to_string()))
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?,?,?,?,1,'before opt-in',?)")
            .bind(historical.to_string())
            .bind(channel.to_string())
            .bind(sender.to_string())
            .bind("Sender")
            .bind("2000-01-01T00:00:00.000000Z")
            .execute(pool)
            .await
            .unwrap();

        service.subscribe(recipient.clone(), PushSubscriptionInput {
            endpoint: "https://fcm.googleapis.com/new-device".into(),
            keys: PushSubscriptionKeys { p256dh: "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU".into(), auth: "AAECAwQFBgcICQoLDA0ODw".into() },
        }, None).await.unwrap();
        let cursor: String = sqlx::query_scalar(
            "select notification_after_message_id from push_subscriptions where endpoint=?",
        )
        .bind("https://fcm.googleapis.com/new-device")
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(cursor, historical.to_string());
        // Start-up reconciliation is cursor-bounded too: opting in now does
        // not resurrect the old DM.
        service.backfill_current_cursors_once().await.unwrap();
        let queued: i64 = sqlx::query_scalar("select count(*) from notification_outbox")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(queued, 0);

        // `current_timestamp` intentionally has the same second-level
        // precision as the subscription on SQLite. The UUIDv7 cursor still
        // includes this message because it was generated after opt-in.
        let future = Uuid::now_v7();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body,created_at) values(?,?,?,?,2,'after opt-in',current_timestamp)")
            .bind(future.to_string())
            .bind(channel.to_string())
            .bind(sender.to_string())
            .bind("Sender")
            .execute(pool)
            .await
            .unwrap();
        let chat_message = ChatMessage {
            id: crate::domain::MessageId::from_uuid(future),
            channel_id: crate::domain::ChannelId::from_uuid(channel),
            parent_message_id: None,
            sender_id: sender.clone(),
            sender_display_name: crate::domain::DisplayName::new("Sender").unwrap(),
            body: crate::domain::MessageBody::new("after opt-in").unwrap(),
            sequence: crate::domain::ChannelSequence::new(2),
            sent_at: chrono::Utc::now(),
            edited_at: None,
            deleted_at: None,
        };
        let mut transaction = pool.begin().await.unwrap();
        enqueue_message_sqlite(&mut transaction, &chat_message)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let queued: Vec<String> = sqlx::query_scalar("select message_id from notification_outbox")
            .fetch_all(pool)
            .await
            .unwrap();
        assert_eq!(queued, vec![future.to_string()]);
    }

    #[tokio::test]
    async fn editing_a_message_to_add_a_mention_enqueues_it_once() {
        let service = service().await;
        let NotificationStore::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        let sender = UserId::named("sender");
        let recipient = UserId::named("recipient");
        let channel = Uuid::now_v7();
        let message_id = Uuid::now_v7();
        for (id, name) in [(&sender, "Sender"), (&recipient, "Recipient")] {
            sqlx::query("insert into users(id,kind,display_name) values(?, 'human', ?)")
                .bind(id.to_string())
                .bind(name)
                .execute(pool)
                .await
                .unwrap();
        }
        sqlx::query(
            "insert into channels(id,slug,name,kind,created_by) values(?,?,'Open','public',?)",
        )
        .bind(channel.to_string())
        .bind(format!("open-{channel}"))
        .bind(sender.to_string())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,?,1,'first draft')")
            .bind(message_id.to_string())
            .bind(channel.to_string())
            .bind(sender.to_string())
            .bind("Sender")
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("insert into push_subscriptions(id,user_id,endpoint,p256dh,auth,notification_after_message_id) values(?,?,?,?,?,?)")
            .bind(Uuid::now_v7().to_string())
            .bind(recipient.to_string())
            .bind("https://push.example/recipient")
            .bind("p".repeat(65))
            .bind("a".repeat(22))
            .bind("00000000-0000-7000-8000-000000000000")
            .execute(pool)
            .await
            .unwrap();
        let edited = ChatMessage {
            id: crate::domain::MessageId::from_uuid(message_id),
            channel_id: crate::domain::ChannelId::from_uuid(channel),
            parent_message_id: None,
            sender_id: sender.clone(),
            sender_display_name: crate::domain::DisplayName::new("Sender").unwrap(),
            body: crate::domain::MessageBody::new("Hei @recipient").unwrap(),
            sequence: crate::domain::ChannelSequence::first(),
            sent_at: chrono::Utc::now(),
            edited_at: Some(chrono::Utc::now()),
            deleted_at: None,
        };
        let mut transaction = pool.begin().await.unwrap();
        enqueue_message_sqlite(&mut transaction, &edited)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let before: i64 = sqlx::query_scalar("select count(*) from notification_outbox")
            .fetch_one(pool)
            .await
            .unwrap();
        assert_eq!(before, 0);

        sqlx::query("insert into message_mentions(message_id,mentioned_user_id) values(?,?)")
            .bind(message_id.to_string())
            .bind(recipient.to_string())
            .execute(pool)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        enqueue_message_sqlite(&mut transaction, &edited)
            .await
            .unwrap();
        enqueue_message_sqlite(&mut transaction, &edited)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        let rows: Vec<String> = sqlx::query_scalar("select kind from notification_outbox")
            .fetch_all(pool)
            .await
            .unwrap();
        assert_eq!(rows, vec!["mention"]);
    }

    #[tokio::test]
    async fn renewing_a_device_keeps_its_notification_cursor_and_pending_work() {
        let service = service().await;
        let NotificationStore::Sqlite(pool) = &service.store else {
            unreachable!()
        };
        let user = UserId::named("recipient");
        let sender = UserId::named("sender");
        sqlx::query("insert into users(id,kind,display_name) values(?, 'human', 'Recipient')")
            .bind(user.to_string())
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("insert into users(id,kind,display_name) values(?, 'human', 'Sender')")
            .bind(sender.to_string())
            .execute(pool)
            .await
            .unwrap();
        let input = PushSubscriptionInput {
            endpoint: "https://fcm.googleapis.com/existing-device".into(),
            keys: PushSubscriptionKeys { p256dh: "BGsX0fLhLEJH-Lzm5WOkQPJ3A32BLeszoPShOUXYmMKWT-NC4v4af5uO5-tKfA-eFivOM1drMV7Oy7ZAaDe_UfU".into(), auth: "AAECAwQFBgcICQoLDA0ODw".into() },
        };
        service
            .subscribe(user.clone(), input.clone(), None)
            .await
            .unwrap();
        let before: String = sqlx::query_scalar(
            "select notification_after_message_id from push_subscriptions where endpoint=?",
        )
        .bind(&input.endpoint)
        .fetch_one(pool)
        .await
        .unwrap();
        let subscription_id: String =
            sqlx::query_scalar("select id from push_subscriptions where endpoint=?")
                .bind(&input.endpoint)
                .fetch_one(pool)
                .await
                .unwrap();
        let channel = Uuid::now_v7();
        let pending_message = Uuid::now_v7();
        sqlx::query(
            "insert into channels(id,slug,name,kind,created_by) values(?,?,'Channel','private',?)",
        )
        .bind(channel.to_string())
        .bind(format!("channel-{channel}"))
        .bind(sender.to_string())
        .execute(pool)
        .await
        .unwrap();
        sqlx::query("insert into messages(id,channel_id,sender_id,sender_display_name,sequence,body) values(?,?,?,?,1,'pending')")
            .bind(pending_message.to_string()).bind(channel.to_string()).bind(sender.to_string()).bind("Sender")
            .execute(pool).await.unwrap();
        sqlx::query("insert into notification_outbox(subscription_id,recipient_id,message_id,kind) values(?,?,?,'mention')")
            .bind(&subscription_id).bind(user.to_string()).bind(pending_message.to_string())
            .execute(pool).await.unwrap();

        service
            .subscribe(user, input.clone(), Some("renewed".into()))
            .await
            .unwrap();
        let after: String = sqlx::query_scalar(
            "select notification_after_message_id from push_subscriptions where endpoint=?",
        )
        .bind(&input.endpoint)
        .fetch_one(pool)
        .await
        .unwrap();
        let pending: i64 = sqlx::query_scalar(
            "select count(*) from notification_outbox where subscription_id=? and message_id=?",
        )
        .bind(subscription_id)
        .bind(pending_message.to_string())
        .fetch_one(pool)
        .await
        .unwrap();
        assert_eq!(after, before);
        assert_eq!(pending, 1);
    }
}
