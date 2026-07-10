use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};

use crate::domain::{
    Channel, ChannelId, ChannelKind, ChannelRef, ChannelSequence, ChannelSlug, ChannelSummary,
    ChatMessage, ChatRepository, CreateChannel, DisplayName, JoinChannel, LeaveChannel,
    LoadRecentMessages, MarkRead, Membership, MembershipRole, MessageBody, MessageId,
    RepositoryError, RepositoryFuture, SendMessage, User, UserId,
};

use super::{sql_error, storage};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/sqlite");

#[derive(Clone)]
pub struct SqliteChatRepository {
    pool: SqlitePool,
}

impl SqliteChatRepository {
    pub async fn connect(url: &str) -> Result<Self, RepositoryError> {
        let options = SqliteConnectOptions::from_str(url)
            .map_err(storage)?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await.map_err(sql_error)?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self) -> Result<(), RepositoryError> {
        MIGRATOR.run(&self.pool).await.map_err(storage)
    }
}

impl ChatRepository for SqliteChatRepository {
    fn upsert_user<'a>(&'a self, user: User) -> RepositoryFuture<'a, User> {
        Box::pin(async move {
            sqlx::query(
                "insert into users (id, kind, display_name, external_provider, external_subject, created_at) values (?, ?, ?, ?, ?, ?) on conflict(id) do update set kind = excluded.kind, display_name = excluded.display_name, external_provider = excluded.external_provider, external_subject = excluded.external_subject",
            )
            .bind(user.id.to_string())
            .bind(user.kind.as_str())
            .bind(user.display_name.as_str())
            .bind(&user.external_provider)
            .bind(&user.external_subject)
            .bind(user.created_at)
            .execute(&self.pool)
            .await
            .map_err(sql_error)?;
            Ok(user)
        })
    }

    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel> {
        Box::pin(async move {
            let channel = Channel {
                id: ChannelId::generate(),
                slug: command.slug,
                name: command.name,
                kind: command.kind,
                created_by: command.actor,
            };
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query(
                "insert into channels (id, slug, name, kind, created_by) values (?, ?, ?, ?, ?)",
            )
            .bind(channel.id.to_string())
            .bind(channel.slug.as_str())
            .bind(channel.name.as_str())
            .bind(channel.kind.as_str())
            .bind(channel.created_by.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(sql_error)?;
            sqlx::query("insert into channel_sequences (channel_id) values (?)")
                .bind(channel.id.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) values (?, ?, 'owner')")
                .bind(channel.id.to_string())
                .bind(channel.created_by.to_string())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            Ok(channel)
        })
    }

    fn join_channel<'a>(&'a self, command: JoinChannel) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let channel_id = match command.channel {
                ChannelRef::Id(id) => id,
                ChannelRef::Slug(slug) => {
                    let value: Option<String> =
                        sqlx::query_scalar("select id from channels where slug = ?")
                            .bind(slug.as_str())
                            .fetch_optional(&self.pool)
                            .await
                            .map_err(sql_error)?;
                    ChannelId::new(value.ok_or(RepositoryError::NotFound)?).map_err(storage)?
                }
            };
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) values (?, ?, 'member') on conflict(channel_id, user_id) do nothing")
                .bind(channel_id.to_string())
                .bind(command.actor.to_string())
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            load_membership(&self.pool, channel_id, command.actor).await
        })
    }

    fn leave_channel<'a>(&'a self, command: LeaveChannel) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let result =
                sqlx::query("delete from channel_memberships where channel_id = ? and user_id = ?")
                    .bind(command.channel_id.to_string())
                    .bind(command.actor.to_string())
                    .execute(&self.pool)
                    .await
                    .map_err(sql_error)?;
            if result.rows_affected() == 0 {
                return Err(RepositoryError::NotFound);
            }
            Ok(())
        })
    }

    fn list_channels_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<ChannelSummary>> {
        Box::pin(async move {
            let rows = sqlx::query("select c.id, c.slug, c.name, c.kind, m.role from channels c join channel_memberships m on m.channel_id = c.id where m.user_id = ? order by c.slug")
                .bind(actor.to_string())
                .fetch_all(&self.pool)
                .await
                .map_err(sql_error)?;
            rows.into_iter().map(channel_summary).collect()
        })
    }

    fn load_recent_messages<'a>(
        &'a self,
        query: LoadRecentMessages,
    ) -> RepositoryFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            ensure_membership(&self.pool, &query.channel_id, &query.actor).await?;
            let after = query.after.map_or(0, u64::from);
            let after = i64::try_from(after).map_err(storage)?;
            let limit = i64::try_from(usize::from(query.limit)).map_err(storage)?;
            let rows = sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where channel_id = ? and sequence > ? order by sequence desc limit ?")
                .bind(query.channel_id.to_string())
                .bind(after)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
                .map_err(sql_error)?;
            let mut messages = rows
                .into_iter()
                .map(chat_message)
                .collect::<Result<Vec<_>, _>>()?;
            messages.reverse();
            Ok(messages)
        })
    }

    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            let membership: Option<i64> = sqlx::query_scalar(
                "select 1 from channel_memberships where channel_id = ? and user_id = ?",
            )
            .bind(command.channel_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sql_error)?;
            if membership.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
            let sequence: i64 = sqlx::query_scalar("update channel_sequences set next_sequence = next_sequence + 1 where channel_id = ? returning next_sequence - 1")
                .bind(command.channel_id.to_string())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(sql_error)?
                .ok_or(RepositoryError::NotFound)?;
            let message = ChatMessage {
                id: MessageId::generate(),
                channel_id: command.channel_id,
                sender_id: command.actor,
                body: command.body,
                sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
                sent_at: Utc::now(),
            };
            sqlx::query("insert into messages (id, channel_id, sender_id, sequence, body, created_at) values (?, ?, ?, ?, ?, ?)")
                .bind(message.id.as_uuid().to_string())
                .bind(message.channel_id.to_string())
                .bind(message.sender_id.to_string())
                .bind(sequence)
                .bind(message.body.as_str())
                .bind(message.sent_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            Ok(message)
        })
    }

    fn load_message<'a>(&'a self, id: MessageId) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let row = sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where id = ?")
                .bind(id.as_uuid().to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?
                .ok_or(RepositoryError::NotFound)?;
            chat_message(row)
        })
    }

    fn latest_sequence<'a>(
        &'a self,
        channel_id: ChannelId,
    ) -> RepositoryFuture<'a, ChannelSequence> {
        Box::pin(async move {
            let latest: Option<i64> =
                sqlx::query_scalar("select max(sequence) from messages where channel_id = ?")
                    .bind(channel_id.to_string())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(sql_error)?;
            ChannelSequence::try_from(latest.unwrap_or(0)).map_err(storage)
        })
    }

    fn mark_read<'a>(&'a self, command: MarkRead) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let latest: Option<i64> =
                sqlx::query_scalar("select max(sequence) from messages where channel_id = ?")
                    .bind(command.channel_id.to_string())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(sql_error)?;
            let requested = i64::try_from(u64::from(command.sequence)).map_err(storage)?;
            if requested > latest.unwrap_or(0) {
                return Err(RepositoryError::NotFound);
            }
            let result = sqlx::query("update channel_memberships set last_read_sequence = max(last_read_sequence, ?) where channel_id = ? and user_id = ?")
                .bind(requested)
                .bind(command.channel_id.to_string())
                .bind(command.actor.to_string())
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            if result.rows_affected() == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            load_membership(&self.pool, command.channel_id, command.actor).await
        })
    }
}

async fn ensure_membership(
    pool: &SqlitePool,
    channel_id: &ChannelId,
    actor: &UserId,
) -> Result<(), RepositoryError> {
    let found: Option<i64> = sqlx::query_scalar(
        "select 1 from channel_memberships where channel_id = ? and user_id = ?",
    )
    .bind(channel_id.to_string())
    .bind(actor.to_string())
    .fetch_optional(pool)
    .await
    .map_err(sql_error)?;
    found.map(|_| ()).ok_or(RepositoryError::PermissionDenied)
}

async fn load_membership(
    pool: &SqlitePool,
    channel_id: ChannelId,
    actor: UserId,
) -> Result<Membership, RepositoryError> {
    let row = sqlx::query("select role, last_read_sequence from channel_memberships where channel_id = ? and user_id = ?")
        .bind(channel_id.to_string())
        .bind(actor.to_string())
        .fetch_optional(pool)
        .await
        .map_err(sql_error)?
        .ok_or(RepositoryError::NotFound)?;
    let role: String = row.try_get("role").map_err(storage)?;
    let sequence: i64 = row.try_get("last_read_sequence").map_err(storage)?;
    Ok(Membership {
        channel_id,
        user_id: actor,
        role: MembershipRole::parse(&role).ok_or_else(|| storage("invalid membership role"))?,
        last_read_sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
    })
}

fn channel_summary(row: sqlx::sqlite::SqliteRow) -> Result<ChannelSummary, RepositoryError> {
    let id: String = row.try_get("id").map_err(storage)?;
    let slug: String = row.try_get("slug").map_err(storage)?;
    let name: String = row.try_get("name").map_err(storage)?;
    let kind: String = row.try_get("kind").map_err(storage)?;
    let role: String = row.try_get("role").map_err(storage)?;
    Ok(ChannelSummary {
        id: ChannelId::new(id).map_err(storage)?,
        slug: ChannelSlug::new(slug).map_err(storage)?,
        name: DisplayName::new(name).map_err(storage)?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| storage("invalid channel kind"))?,
        role: MembershipRole::parse(&role).ok_or_else(|| storage("invalid membership role"))?,
    })
}

fn chat_message(row: sqlx::sqlite::SqliteRow) -> Result<ChatMessage, RepositoryError> {
    let id: String = row.try_get("id").map_err(storage)?;
    let channel_id: String = row.try_get("channel_id").map_err(storage)?;
    let sender_id: String = row.try_get("sender_id").map_err(storage)?;
    let sequence: i64 = row.try_get("sequence").map_err(storage)?;
    let body: String = row.try_get("body").map_err(storage)?;
    let sent_at: DateTime<Utc> = row.try_get("created_at").map_err(storage)?;
    let id = uuid::Uuid::parse_str(&id).map_err(storage)?;
    Ok(ChatMessage {
        id: MessageId::from_uuid(id),
        channel_id: ChannelId::new(channel_id).map_err(storage)?,
        sender_id: UserId::new(sender_id).map_err(storage)?,
        body: MessageBody::new(body).map_err(storage)?,
        sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
        sent_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PrincipalKind, User};

    #[tokio::test]
    async fn sqlite_repository_persists_and_reads_messages() {
        let repository = SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        repository.migrate().await.unwrap();
        let alice = UserId::named("sqlite-alice");
        repository
            .upsert_user(User {
                id: alice.clone(),
                kind: PrincipalKind::Human,
                display_name: DisplayName::new("Alice").unwrap(),
                external_provider: None,
                external_subject: None,
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("sqlite").unwrap(),
                name: DisplayName::new("SQLite").unwrap(),
                kind: ChannelKind::Private,
            })
            .await
            .unwrap();
        let message = repository
            .append_message(SendMessage {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("durable").unwrap(),
            })
            .await
            .unwrap();
        let loaded = repository
            .load_recent_messages(LoadRecentMessages {
                actor: alice,
                channel_id: channel.id,
                limit: crate::domain::MessageLimit::DEFAULT,
                after: None,
            })
            .await
            .unwrap();
        assert_eq!(loaded, vec![message]);
    }
}
