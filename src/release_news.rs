use std::{io::Read, time::Duration};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    config::{AppConfig, DatabaseKind},
    db::{self, PostgresChatRepository},
    domain::{ChannelId, ChatRepository, MessageBody, SendMessage, UserId},
    web::assets::BUILD_REVISION,
};

const PRODUCTION_ORIGIN: &str = "https://sproyt.bjoroy.me";
const SYSTEM_ID: Uuid = Uuid::from_u128(1);
const SYSTEM_NAME: &str = "Sprøyt";
const CHANNEL_NAME: &str = "Nyhende i Sprøyt";
const MAX_SUMMARY_CHARS: usize = 280;
const MAX_SUMMARY_BYTES: u64 = 4 * MAX_SUMMARY_CHARS as u64 + 2;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(crate) async fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let revision = args
        .next()
        .ok_or("usage: sproyt release-news <revision> < summary.txt")?;
    if args.next().is_some() || !valid_revision(&revision) || revision != BUILD_REVISION {
        return Err("release-news requires the full revision of this application binary".into());
    }
    if std::env::var("SPROYT_ENV").as_deref() != Ok("production")
        || !std::env::var("SPROYT_PUBLIC_URL")
            .is_ok_and(|url| url.trim_end_matches('/') == PRODUCTION_ORIGIN)
        || std::env::var("SPROYT_OIDC_REDIRECT_URL").as_deref()
            != Ok(concat!("https://sproyt.bjoroy.me", "/auth/callback"))
    {
        return Err("release-news must run in the production application workload".into());
    }
    let body = read_summary(std::io::stdin().lock())?;
    let database = AppConfig::database_from_env()?;
    if database.kind() != DatabaseKind::Postgres {
        return Err("release-news requires the production PostgreSQL repository".into());
    }
    let pool = db::connect_postgres_pool(&database, 2)
        .await?
        .ok_or("release-news PostgreSQL pool unavailable")?;
    let repository =
        PostgresChatRepository::connect_with_pool(database.url(), pool.clone()).await?;
    verify_public_revision(&revision).await?;
    let publication = publish(&pool, &repository, &revision, body).await?;
    println!(
        "{}",
        serde_json::json!({
            "status": "published",
            "revision": revision,
            "channelId": publication.channel_id,
            "messageId": publication.message_id,
            "sequence": publication.sequence,
        })
    );
    Ok(())
}

fn valid_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn read_summary(reader: impl Read) -> Result<MessageBody> {
    let mut raw = String::new();
    reader
        .take(MAX_SUMMARY_BYTES + 1)
        .read_to_string(&mut raw)?;
    let summary = raw.trim_end_matches(['\r', '\n']);
    if raw.len() as u64 > MAX_SUMMARY_BYTES
        || summary.trim().is_empty()
        || summary.chars().count() > MAX_SUMMARY_CHARS
        || summary.chars().any(char::is_control)
        || summary.contains("[[")
    {
        return Err(
            "release news must be one line of 1–280 characters without chat attachment tokens"
                .into(),
        );
    }
    Ok(MessageBody::new(summary)?)
}

#[derive(Deserialize)]
struct Version {
    service: String,
    revision: String,
}

async fn verify_public_revision(revision: &str) -> Result<()> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()?;
    let mut response = client
        .get(format!("{PRODUCTION_ORIGIN}/versionz"))
        .header(reqwest::header::CACHE_CONTROL, "no-cache")
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("the public production revision could not be verified".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len() + chunk.len() > 4096 {
            return Err("the public production version response was too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let version: Version = serde_json::from_slice(&bytes)?;
    if version.service != "sproyt" || version.revision != revision {
        return Err("the public production revision does not match the announcement".into());
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct Publication {
    summary_digest: String,
    channel_id: Uuid,
    message_id: Uuid,
    sequence: i64,
}

async fn previous_publication(
    pool: &PgPool,
    revision: &str,
    summary_digest: &str,
) -> Result<Option<Publication>> {
    let previous = sqlx::query_as::<_, Publication>(
        "select summary_digest,channel_id,message_id,sequence from release_news_publications where revision=$1",
    )
    .bind(revision)
    .fetch_optional(pool)
    .await?;
    if previous
        .as_ref()
        .is_some_and(|publication| publication.summary_digest != summary_digest)
    {
        return Err("this production revision already has a different release announcement".into());
    }
    Ok(previous)
}

async fn publish(
    pool: &PgPool,
    repository: &impl ChatRepository,
    revision: &str,
    body: MessageBody,
) -> Result<Publication> {
    let summary_digest = hex::encode(Sha256::digest(body.as_str()));
    if let Some(previous) = previous_publication(pool, revision, &summary_digest).await? {
        return Ok(previous);
    }
    let system = sqlx::query_scalar::<_, Uuid>(
        "select u.id from users u join agent_profiles profile on profile.agent_id=u.id
         where u.id=$1 and u.kind='agent' and u.display_name=$2
         and u.external_provider is null and u.external_subject is null
         and profile.provider='sproyt-system' and profile.service_identity='release-news'
         and profile.owner_id=u.id and profile.revoked_at is not null
         and not exists(select 1 from agent_credentials where agent_id=$1)",
    )
    .bind(SYSTEM_ID)
    .bind(SYSTEM_NAME)
    .fetch_optional(pool)
    .await?;
    let actor = UserId::from_uuid(system.ok_or("reserved Sprøyt identity unavailable")?);
    let channel = sqlx::query_scalar::<_, Uuid>(
        "select c.id from release_news_channel destination join channels c on c.id=destination.channel_id
         where c.circle_id is null and c.kind='public' and c.name=$1",
    )
    .bind(CHANNEL_NAME)
    .fetch_optional(pool)
    .await?
    .ok_or("the public Felles release channel Nyhende i Sprøyt is not configured")?;
    let appended = repository
        .append_message_idempotent(
            SendMessage {
                actor,
                channel_id: ChannelId::from_uuid(channel),
                parent_message_id: None,
                body,
            },
            format!("release-news:{revision}:{summary_digest}"),
        )
        .await;
    // The ledger is written in the normal message transaction. A competing
    // operator can win the revision's unique key between the two reads.
    if let Some(publication) = previous_publication(pool, revision, &summary_digest).await? {
        return Ok(publication);
    }
    appended?;
    Err("release news was stored without a publication receipt".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_summary_is_short_plain_single_line() {
        assert_eq!(
            read_summary("Kanallenkjer er klare.\r\n".as_bytes())
                .unwrap()
                .as_str(),
            "Kanallenkjer er klare."
        );
        for rejected in [
            "",
            "   ",
            "to\nlinjer",
            "[[media:attachment]]",
            &"ø".repeat(281),
        ] {
            assert!(
                read_summary(rejected.as_bytes()).is_err(),
                "accepted {rejected}"
            );
        }
        assert!(valid_revision(&"a".repeat(40)));
        assert!(!valid_revision(&"A".repeat(40)));
        assert!(!valid_revision(&"a".repeat(39)));
    }

    #[tokio::test]
    async fn postgres_release_news_publishes_once_and_keeps_receipt_after_deletion() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let admin = PgPool::connect(&url).await.unwrap();
        let schema = format!("release_news_{}", Uuid::now_v7().simple());
        sqlx::query(&format!("create schema {schema}"))
            .execute(&admin)
            .await
            .unwrap();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .after_connect({
                let search_path = format!("set search_path to {schema}");
                move |connection, _| {
                    let search_path = search_path.clone();
                    Box::pin(async move {
                        sqlx::query(&search_path).execute(connection).await?;
                        Ok(())
                    })
                }
            })
            .connect(&url)
            .await
            .unwrap();
        sqlx::migrate!("./migrations/postgres")
            .run(&pool)
            .await
            .unwrap();
        let repository = PostgresChatRepository::connect_with_pool(&url, pool.clone())
            .await
            .unwrap();
        let mut published = repository.subscribe_messages().unwrap();

        let revision = "a".repeat(40);
        let summary = MessageBody::new("Kanallenkjer er klare.").unwrap();
        let (first, retry) = tokio::join!(
            publish(&pool, &repository, &revision, summary.clone()),
            publish(&pool, &repository, &revision, summary.clone()),
        );
        let first = first.unwrap();
        assert_eq!(retry.unwrap().message_id, first.message_id);
        let message = repository
            .load_message(crate::domain::MessageId::from_uuid(first.message_id))
            .await
            .unwrap();
        assert_eq!(*message.sender_id.as_uuid(), SYSTEM_ID);
        assert_eq!(message.sender_display_name.as_str(), SYSTEM_NAME);
        assert_eq!(message.body, summary);
        assert_eq!(*message.channel_id.as_uuid(), first.channel_id);
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let message_id = published.recv().await.unwrap();
                if *message_id.as_uuid() == first.message_id {
                    break message_id;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(event, message.id);

        let changed = MessageBody::new("Eit anna samandrag.").unwrap();
        assert!(
            publish(&pool, &repository, &revision, changed.clone())
                .await
                .is_err()
        );
        sqlx::query("delete from messages where id=$1")
            .bind(first.message_id)
            .execute(&pool)
            .await
            .unwrap();
        let retry = publish(&pool, &repository, &revision, summary)
            .await
            .unwrap();
        assert_eq!(retry.message_id, first.message_id);
        assert!(
            publish(&pool, &repository, &revision, changed)
                .await
                .is_err()
        );
        let messages: i64 = sqlx::query_scalar("select count(*) from messages where channel_id=$1")
            .bind(first.channel_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(messages, 0);

        pool.close().await;
        sqlx::query(&format!("drop schema {schema} cascade"))
            .execute(&admin)
            .await
            .unwrap();
    }
}
