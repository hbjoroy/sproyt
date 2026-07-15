use std::str::FromStr;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use uuid::Uuid;

use crate::agent::{
    ActivityProvenance, AgentFuture, AgentPrincipal, AgentRepository, AgentScope, CreateAgent,
    CreatedAgent, GrantAgent, MessageProvenance,
};
use crate::domain::{
    AcceptCircleInvitation, Channel, ChannelId, ChannelKind, ChannelRef, ChannelSequence,
    ChannelSlug, ChannelSummary, ChatMessage, ChatRepository, Circle, CircleId, CircleInvitation,
    CircleMembership, CircleRole, CreateChannel, CreateCircle, CreateCircleInvitation, DisplayName,
    InvitationId, IssuedInvitation, JoinChannel, LeaveChannel, LoadRecentMessages, MarkRead,
    Membership, MembershipRole, MessageBody, MessageId, Policy, RepositoryError, RepositoryFuture,
    SendMessage, User, UserId,
};
use crate::process::{
    EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, OutboxId, OutboxJob,
    OutboxOperation, ProcessError, ProcessEvent, ProcessLink, ProcessLinkId, ProcessRepository,
    ProcessRepositoryFuture, ProcessView, SetCircleFeature, StartProcess, StartedProcess,
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
    fn health_check(&self) -> RepositoryFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query_scalar::<_, i64>("select 1")
                .fetch_one(&self.pool)
                .await
                .map_err(sql_error)
                .map(|_| ())
        })
    }
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

    fn create_circle<'a>(&'a self, command: CreateCircle) -> RepositoryFuture<'a, Circle> {
        Box::pin(async move {
            let circle = Circle {
                id: CircleId::generate(),
                slug: command.slug,
                name: command.name,
                created_by: command.actor,
                created_at: Utc::now(),
            };
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("insert into circles (id, slug, name, created_by, created_at) values (?, ?, ?, ?, ?)")
                .bind(circle.id.to_string())
                .bind(circle.slug.as_str())
                .bind(circle.name.as_str())
                .bind(circle.created_by.to_string())
                .bind(circle.created_at)
                .execute(&mut *transaction).await.map_err(sql_error)?;
            sqlx::query("insert into circle_memberships (circle_id, user_id, role, joined_at) values (?, ?, 'owner', ?)")
                .bind(circle.id.to_string())
                .bind(circle.created_by.to_string())
                .bind(circle.created_at)
                .execute(&mut *transaction).await.map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            Ok(circle)
        })
    }

    fn list_circles_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<(Circle, CircleRole)>> {
        Box::pin(async move {
            let rows = sqlx::query("select c.id, c.slug, c.name, c.created_by, c.created_at, m.role from circles c join circle_memberships m on m.circle_id = c.id where m.user_id = ? order by c.slug")
                .bind(actor.to_string()).fetch_all(&self.pool).await.map_err(sql_error)?;
            rows.into_iter().map(circle_with_role).collect()
        })
    }

    fn create_circle_invitation<'a>(
        &'a self,
        command: CreateCircleInvitation,
    ) -> RepositoryFuture<'a, IssuedInvitation> {
        Box::pin(async move {
            let role: Option<String> = sqlx::query_scalar(
                "select role from circle_memberships where circle_id = ? and user_id = ?",
            )
            .bind(command.circle_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(CircleRole::parse);
            if !Policy::can_invite_to_circle(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let mut secret = [0_u8; 32];
            getrandom::fill(&mut secret).map_err(storage)?;
            let token = URL_SAFE_NO_PAD.encode(secret);
            let token_hash = Sha256::digest(token.as_bytes()).to_vec();
            let invitation = CircleInvitation {
                id: InvitationId::generate(),
                circle_id: command.circle_id,
                invited_by: command.actor,
                expires_at: Utc::now() + Duration::days(7),
            };
            sqlx::query("insert into circle_invitations (id, circle_id, invited_by, token_hash, expires_at) values (?, ?, ?, ?, ?)")
                .bind(invitation.id.as_uuid().to_string()).bind(invitation.circle_id.to_string())
                .bind(invitation.invited_by.to_string()).bind(token_hash).bind(invitation.expires_at)
                .execute(&self.pool).await.map_err(sql_error)?;
            Ok(IssuedInvitation { invitation, token })
        })
    }

    fn accept_circle_invitation<'a>(
        &'a self,
        command: AcceptCircleInvitation,
    ) -> RepositoryFuture<'a, CircleMembership> {
        Box::pin(async move {
            let token_hash = Sha256::digest(command.token.as_bytes()).to_vec();
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            let row = sqlx::query("select id, circle_id, expires_at from circle_invitations where token_hash = ? and accepted_at is null")
                .bind(token_hash).fetch_optional(&mut *transaction).await.map_err(sql_error)?
                .ok_or(RepositoryError::NotFound)?;
            let invitation_id: String = row.try_get("id").map_err(storage)?;
            let circle_id = CircleId::from_uuid(
                uuid::Uuid::parse_str(&row.try_get::<String, _>("circle_id").map_err(storage)?)
                    .map_err(storage)?,
            );
            let expires_at: DateTime<Utc> = row.try_get("expires_at").map_err(storage)?;
            if expires_at < Utc::now() {
                return Err(RepositoryError::NotFound);
            }
            let joined_at = Utc::now();
            sqlx::query("update circle_invitations set accepted_by = ?, accepted_at = ? where id = ? and accepted_at is null")
                .bind(command.actor.to_string()).bind(joined_at).bind(invitation_id)
                .execute(&mut *transaction).await.map_err(sql_error)?;
            sqlx::query("insert into circle_memberships (circle_id, user_id, role, joined_at) values (?, ?, 'member', ?) on conflict(circle_id, user_id) do nothing")
                .bind(circle_id.to_string()).bind(command.actor.to_string()).bind(joined_at)
                .execute(&mut *transaction).await.map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            Ok(CircleMembership {
                circle_id,
                user_id: command.actor,
                role: CircleRole::Member,
                joined_at,
            })
        })
    }

    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel> {
        Box::pin(async move {
            let channel = Channel {
                id: ChannelId::generate(),
                slug: command.slug,
                name: command.name,
                kind: command.kind,
                circle_id: command.circle_id,
                created_by: command.actor,
            };
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            if let Some(circle_id) = &channel.circle_id {
                let allowed: Option<String> = sqlx::query_scalar(
                    "select role from circle_memberships where circle_id = ? and user_id = ?",
                )
                .bind(circle_id.to_string())
                .bind(channel.created_by.to_string())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(sql_error)?;
                let role = allowed.as_deref().and_then(CircleRole::parse);
                if !Policy::can_create_channel_in_circle(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            sqlx::query(
                "insert into channels (id, slug, name, kind, circle_id, created_by) values (?, ?, ?, ?, ?, ?)",
            )
            .bind(channel.id.to_string())
            .bind(channel.slug.as_str())
            .bind(channel.name.as_str())
            .bind(channel.kind.as_str())
            .bind(channel.circle_id.as_ref().map(ToString::to_string))
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
            let allowed: Option<i64> = sqlx::query_scalar("select 1 from channels c where c.id=? and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=?))")
                .bind(channel_id.to_string()).bind(command.actor.to_string())
                .fetch_optional(&self.pool).await.map_err(sql_error)?;
            if allowed.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
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
            let role: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = ? and user_id = ?",
            )
            .bind(command.channel_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_leave_channel(role.as_ref()) {
                return Err(RepositoryError::NotFound);
            }
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
            let rows = sqlx::query("select c.id, c.slug, c.name, c.kind, c.circle_id, m.role, m.last_read_sequence, coalesce((select max(sequence) from messages where channel_id=c.id),0) as latest_sequence from channels c join channel_memberships m on m.channel_id = c.id where m.user_id = ? order by c.slug")
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
            let limit = i64::try_from(usize::from(query.limit)).map_err(storage)?;
            let (rows, reverse) = if let Some(after) = query.after {
                let after = i64::try_from(u64::from(after)).map_err(storage)?;
                (sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where channel_id = ? and sequence > ? order by sequence asc limit ?")
                    .bind(query.channel_id.to_string()).bind(after).bind(limit)
                    .fetch_all(&self.pool).await.map_err(sql_error)?, false)
            } else {
                (sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where channel_id = ? order by sequence desc limit ?")
                    .bind(query.channel_id.to_string()).bind(limit)
                    .fetch_all(&self.pool).await.map_err(sql_error)?, true)
            };
            let mut messages = rows
                .into_iter()
                .map(chat_message)
                .collect::<Result<Vec<_>, _>>()?;
            if reverse {
                messages.reverse();
            }
            Ok(messages)
        })
    }

    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            let membership: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = ? and user_id = ?",
            )
            .bind(command.channel_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sql_error)?;
            let role = membership.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_send_to_channel(role.as_ref()) {
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
                sent_at: persisted_now(),
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

    fn append_message_idempotent<'a>(
        &'a self,
        command: SendMessage,
        request_id: String,
    ) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            let reservation = sqlx::query("insert into command_receipts (principal_id, request_id) values (?, ?) on conflict(principal_id, request_id) do nothing")
                .bind(command.actor.to_string())
                .bind(&request_id)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            if reservation.rows_affected() == 0 {
                let row = sqlx::query("select m.id, m.channel_id, m.sender_id, m.sequence, m.body, m.created_at from command_receipts r join messages m on m.id = r.message_id where r.principal_id = ? and r.request_id = ?")
                    .bind(command.actor.to_string())
                    .bind(&request_id)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(sql_error)?
                    .ok_or_else(|| storage("idempotency receipt has no message"))?;
                return chat_message(row);
            }
            let membership: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = ? and user_id = ?",
            )
            .bind(command.channel_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sql_error)?;
            let role = membership.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_send_to_channel(role.as_ref()) {
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
                sent_at: persisted_now(),
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
            sqlx::query("update command_receipts set message_id = ? where principal_id = ? and request_id = ?")
                .bind(message.id.as_uuid().to_string())
                .bind(message.sender_id.to_string())
                .bind(&request_id)
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

impl ProcessRepository for SqliteChatRepository {
    fn enqueue_start<'a>(
        &'a self,
        command: EnqueueProcessStart,
    ) -> ProcessRepositoryFuture<'a, ProcessLink> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let role: Option<String> = sqlx::query_scalar(
                "select m.role from channel_memberships m \
                 join channels c on c.id=m.channel_id \
                 join circle_features f on f.circle_id=c.circle_id \
                 and f.feature='heart.event-planning' and f.enabled=1 \
                 where m.channel_id=? and m.user_id=?",
            )
            .bind(command.channel_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_start_process(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            if let Some(row) = sqlx::query("select id, heart_instance_id, namespace, definition_name, definition_version, status from process_links where initiated_by = ? and request_id = ?")
                .bind(command.actor.to_string()).bind(&command.request_id).fetch_optional(&mut *tx).await.map_err(sql_error)? {
                tx.commit().await.map_err(sql_error)?;
                return process_link_from_sqlite(row, command.channel_id, command.actor);
            }
            let link_id = ProcessLinkId::generate();
            let outbox_id = OutboxId::generate();
            let now = Utc::now();
            let operation = OutboxOperation::Start {
                command: StartProcess {
                    namespace: command.namespace.clone(),
                    definition_name: command.definition_name.clone(),
                    version: command.definition_version.clone(),
                    metadata: command.metadata,
                },
            };
            sqlx::query("insert into process_links (id, channel_id, namespace, definition_name, definition_version, initiated_by, request_id, created_at, updated_at) values (?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(link_id.as_uuid().to_string()).bind(command.channel_id.to_string()).bind(&command.namespace)
                .bind(&command.definition_name).bind(&command.definition_version).bind(command.actor.to_string())
                .bind(&command.request_id).bind(now).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_outbox (id, process_link_id, operation, payload, available_at, created_at) values (?, ?, 'start', ?, ?, ?)")
                .bind(outbox_id.as_uuid().to_string()).bind(link_id.as_uuid().to_string())
                .bind(serde_json::to_string(&operation).map_err(storage)?).bind(now).bind(now)
                .execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(ProcessLink {
                id: link_id,
                channel_id: command.channel_id,
                heart_instance_id: None,
                namespace: command.namespace,
                definition_name: command.definition_name,
                definition_version: command.definition_version,
                initiated_by: command.actor,
                status: "starting".into(),
            })
        })
    }

    fn enqueue_correlation<'a>(
        &'a self,
        command: EnqueueCorrelation,
    ) -> ProcessRepositoryFuture<'a, OutboxId> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            if let Some(existing) = sqlx::query_scalar::<_, String>(
                "select outbox_id from process_command_receipts where actor_id=? and request_id=?",
            )
            .bind(command.actor.to_string())
            .bind(&command.request_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?
            {
                tx.commit().await.map_err(sql_error)?;
                return Uuid::parse_str(&existing)
                    .map(OutboxId::from_uuid)
                    .map_err(storage);
            }
            let access: Option<(String, String)> = sqlx::query_as("select p.namespace,m.role from process_links p join channels c on c.id=p.channel_id join channel_memberships m on m.channel_id=c.id and m.user_id=? join circle_features f on f.circle_id=c.circle_id and f.feature='heart.event-planning' and f.enabled=1 where p.id=?")
                .bind(command.actor.to_string()).bind(command.process_link_id.as_uuid().to_string())
                .fetch_optional(&mut *tx).await.map_err(sql_error)?;
            let (namespace, role) = access.ok_or(RepositoryError::PermissionDenied)?;
            let role = MembershipRole::parse(&role);
            if !Policy::can_complete_process_work(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let outbox_id = OutboxId::generate();
            let now = Utc::now();
            let operation = OutboxOperation::Correlate {
                command: crate::process::CorrelateMessage {
                    namespace,
                    correlation_key: "process_link_id".into(),
                    correlation_value: command.process_link_id.as_uuid().to_string(),
                    payload: command.payload,
                },
            };
            sqlx::query("insert into process_outbox(id,process_link_id,operation,payload,available_at,created_at) values(?,?,'correlate',?,?,?)")
                .bind(outbox_id.as_uuid().to_string()).bind(command.process_link_id.as_uuid().to_string())
                .bind(serde_json::to_string(&operation).map_err(storage)?).bind(now).bind(now)
                .execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_command_receipts(actor_id,request_id,process_link_id,outbox_id,command_type,created_at) values(?,?,?,?, 'correlate', ?)")
                .bind(command.actor.to_string()).bind(command.request_id).bind(command.process_link_id.as_uuid().to_string())
                .bind(outbox_id.as_uuid().to_string()).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(outbox_id)
        })
    }

    fn get_process<'a>(
        &'a self,
        actor: UserId,
        process_link_id: ProcessLinkId,
    ) -> ProcessRepositoryFuture<'a, ProcessView> {
        Box::pin(async move {
            let row = sqlx::query("select p.id,p.channel_id,p.heart_instance_id,p.namespace,p.definition_name,p.definition_version,p.initiated_by,p.status,m.role from process_links p join channel_memberships m on m.channel_id=p.channel_id and m.user_id=? where p.id=?")
                .bind(actor.to_string()).bind(process_link_id.as_uuid().to_string())
                .fetch_optional(&self.pool).await.map_err(sql_error)?
                .ok_or(RepositoryError::PermissionDenied)?;
            let role: String = row.try_get("role").map_err(storage)?;
            if !Policy::can_read_channel(MembershipRole::parse(&role).as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let process = ProcessLink {
                id: ProcessLinkId::from_uuid(
                    Uuid::parse_str(&row.try_get::<String, _>("id").map_err(storage)?)
                        .map_err(storage)?,
                ),
                channel_id: ChannelId::new(
                    row.try_get::<String, _>("channel_id").map_err(storage)?,
                )
                .map_err(storage)?,
                heart_instance_id: row
                    .try_get::<Option<String>, _>("heart_instance_id")
                    .map_err(storage)?
                    .map(|id| Uuid::parse_str(&id))
                    .transpose()
                    .map_err(storage)?,
                namespace: row.try_get("namespace").map_err(storage)?,
                definition_name: row.try_get("definition_name").map_err(storage)?,
                definition_version: row.try_get("definition_version").map_err(storage)?,
                initiated_by: UserId::new(
                    row.try_get::<String, _>("initiated_by").map_err(storage)?,
                )
                .map_err(storage)?,
                status: row.try_get("status").map_err(storage)?,
            };
            let rows = sqlx::query("select id,event_type,payload,actor_id,occurred_at from process_events where process_link_id=? order by occurred_at,id")
                .bind(process_link_id.as_uuid().to_string()).fetch_all(&self.pool).await.map_err(sql_error)?;
            let events = rows
                .into_iter()
                .map(|row| {
                    Ok(ProcessEvent {
                        id: Uuid::parse_str(&row.try_get::<String, _>("id").map_err(storage)?)
                            .map_err(storage)?,
                        event_type: row.try_get("event_type").map_err(storage)?,
                        payload: serde_json::from_str(
                            &row.try_get::<String, _>("payload").map_err(storage)?,
                        )
                        .map_err(storage)?,
                        actor_id: UserId::new(
                            row.try_get::<String, _>("actor_id").map_err(storage)?,
                        )
                        .map_err(storage)?,
                        occurred_at: row.try_get("occurred_at").map_err(storage)?,
                    })
                })
                .collect::<Result<Vec<_>, RepositoryError>>()?;
            Ok(ProcessView { process, events })
        })
    }

    fn enqueue_inspection<'a>(
        &'a self,
        command: EnqueueInspection,
    ) -> ProcessRepositoryFuture<'a, OutboxId> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            if let Some(existing) = sqlx::query_scalar::<_, String>(
                "select outbox_id from process_command_receipts where actor_id=? and request_id=?",
            )
            .bind(command.actor.to_string())
            .bind(&command.request_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?
            {
                tx.commit().await.map_err(sql_error)?;
                return Uuid::parse_str(&existing)
                    .map(OutboxId::from_uuid)
                    .map_err(storage);
            }
            let access: Option<(String, String)> = sqlx::query_as("select p.heart_instance_id,m.role from process_links p join channels c on c.id=p.channel_id join channel_memberships m on m.channel_id=c.id and m.user_id=? join circle_features f on f.circle_id=c.circle_id and f.feature='heart.event-planning' and f.enabled=1 where p.id=?")
                .bind(command.actor.to_string()).bind(command.process_link_id.as_uuid().to_string()).fetch_optional(&mut *tx).await.map_err(sql_error)?;
            let (instance_id, role) = access.ok_or(RepositoryError::PermissionDenied)?;
            if !Policy::can_read_channel(MembershipRole::parse(&role).as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let instance_id = Uuid::parse_str(&instance_id).map_err(storage)?;
            let outbox_id = OutboxId::generate();
            let now = Utc::now();
            let operation = OutboxOperation::Inspect { instance_id };
            sqlx::query("insert into process_outbox(id,process_link_id,operation,payload,available_at,created_at) values(?,?,'inspect',?,?,?)")
                .bind(outbox_id.as_uuid().to_string()).bind(command.process_link_id.as_uuid().to_string())
                .bind(serde_json::to_string(&operation).map_err(storage)?).bind(now).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_command_receipts(actor_id,request_id,process_link_id,outbox_id,command_type,created_at) values(?,?,?,?, 'inspect', ?)")
                .bind(command.actor.to_string()).bind(command.request_id).bind(command.process_link_id.as_uuid().to_string())
                .bind(outbox_id.as_uuid().to_string()).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(outbox_id)
        })
    }

    fn set_circle_feature<'a>(
        &'a self,
        command: SetCircleFeature,
    ) -> ProcessRepositoryFuture<'a, ()> {
        Box::pin(async move {
            let role: Option<String> = sqlx::query_scalar(
                "select role from circle_memberships where circle_id=? and user_id=?",
            )
            .bind(command.circle_id.to_string())
            .bind(command.actor.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(CircleRole::parse);
            if !Policy::can_invite_to_circle(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            sqlx::query("insert into circle_features(circle_id,feature,enabled,updated_by,updated_at) values(?,?,?,?,?) on conflict(circle_id,feature) do update set enabled=excluded.enabled,updated_by=excluded.updated_by,updated_at=excluded.updated_at")
                .bind(command.circle_id.to_string()).bind(command.feature).bind(command.enabled)
                .bind(command.actor.to_string()).bind(Utc::now()).execute(&self.pool).await.map_err(sql_error)?;
            Ok(())
        })
    }

    fn lease_next<'a>(
        &'a self,
        lease_for: std::time::Duration,
    ) -> ProcessRepositoryFuture<'a, Option<OutboxJob>> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let now = Utc::now();
            let row = sqlx::query("select id, process_link_id, payload, attempts from process_outbox where (status = 'pending' or (status = 'leased' and lease_until < ?)) and available_at <= ? order by created_at limit 1")
                .bind(now).bind(now).fetch_optional(&mut *tx).await.map_err(sql_error)?;
            let Some(row) = row else {
                tx.commit().await.map_err(sql_error)?;
                return Ok(None);
            };
            let id: String = row.try_get("id").map_err(storage)?;
            let lease_until = now + chrono::Duration::from_std(lease_for).map_err(storage)?;
            let updated = sqlx::query("update process_outbox set status = 'leased', lease_until = ?, attempts = attempts + 1 where id = ? and (status = 'pending' or lease_until < ?)")
                .bind(lease_until).bind(&id).bind(now).execute(&mut *tx).await.map_err(sql_error)?.rows_affected();
            if updated == 0 {
                tx.commit().await.map_err(sql_error)?;
                return Ok(None);
            }
            let job = outbox_from_sqlite(&row)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(Some(OutboxJob {
                attempts: job.attempts + 1,
                ..job
            }))
        })
    }

    fn complete_start<'a>(
        &'a self,
        job: OutboxJob,
        result: StartedProcess,
    ) -> ProcessRepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let now = Utc::now();
            sqlx::query("update process_links set heart_instance_id = ?, status = 'active', updated_at = ? where id = ? and (heart_instance_id is null or heart_instance_id = ?)")
                .bind(result.instance_id.to_string()).bind(now).bind(job.process_link_id.as_uuid().to_string())
                .bind(result.instance_id.to_string()).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_events (id, process_link_id, event_key, event_type, payload, actor_id, occurred_at) values (?, ?, 'started', 'process.started', ?, (select initiated_by from process_links where id=?), ?) on conflict(process_link_id, event_key) do nothing")
                .bind(Uuid::now_v7().to_string()).bind(job.process_link_id.as_uuid().to_string())
                .bind(serde_json::json!({"instance_id": result.instance_id}).to_string())
                .bind(job.process_link_id.as_uuid().to_string()).bind(now)
                .execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status = 'completed', completed_at = ?, lease_until = null where id = ?")
                .bind(now).bind(job.id.as_uuid().to_string()).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)
        })
    }

    fn complete_operation<'a>(
        &'a self,
        job: OutboxJob,
        event_type: &'a str,
        payload: serde_json::Value,
    ) -> ProcessRepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let now = Utc::now();
            sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values(?,?,?,?,?,coalesce((select actor_id from process_command_receipts where outbox_id=?),(select initiated_by from process_links where id=?)),?) on conflict(process_link_id,event_key) do nothing")
                .bind(Uuid::now_v7().to_string()).bind(job.process_link_id.as_uuid().to_string())
                .bind(job.id.as_uuid().to_string()).bind(event_type).bind(payload.to_string())
                .bind(job.id.as_uuid().to_string()).bind(job.process_link_id.as_uuid().to_string()).bind(now)
                .execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status='completed',completed_at=?,lease_until=null where id=?")
                .bind(now).bind(job.id.as_uuid().to_string()).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)
        })
    }

    fn reschedule<'a>(
        &'a self,
        job: OutboxJob,
        error: ProcessError,
        delay: std::time::Duration,
    ) -> ProcessRepositoryFuture<'a, ()> {
        Box::pin(async move {
            let terminal = !error.retryable || job.attempts >= 8;
            let now = Utc::now();
            let available = now + chrono::Duration::from_std(delay).map_err(storage)?;
            let error_text = error.to_string();
            let payload = serde_json::json!({
                "kind":error.kind,
                "message":error.message,
                "retryable":error.retryable,
                "attempts":job.attempts
            })
            .to_string();
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status = ?, available_at = ?, lease_until = null, last_error = ? where id = ?")
                .bind(if terminal { "failed" } else { "pending" }).bind(available)
                .bind(error_text).bind(job.id.as_uuid().to_string()).execute(&mut *tx).await.map_err(sql_error)?;
            if terminal {
                sqlx::query("update process_links set status='failed',updated_at=? where id=?")
                    .bind(now)
                    .bind(job.process_link_id.as_uuid().to_string())
                    .execute(&mut *tx)
                    .await
                    .map_err(sql_error)?;
                sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values(?,?,?,?,?,coalesce((select actor_id from process_command_receipts where outbox_id=?),(select initiated_by from process_links where id=?)),?) on conflict(process_link_id,event_key) do nothing")
                    .bind(Uuid::now_v7().to_string()).bind(job.process_link_id.as_uuid().to_string())
                    .bind(format!("failed:{}",job.id.as_uuid())).bind("process.failed").bind(payload)
                    .bind(job.id.as_uuid().to_string()).bind(job.process_link_id.as_uuid().to_string()).bind(now)
                    .execute(&mut *tx).await.map_err(sql_error)?;
            }
            tx.commit().await.map_err(sql_error)
        })
    }
}

impl AgentRepository for SqliteChatRepository {
    fn create_agent<'a>(&'a self, command: CreateAgent) -> AgentFuture<'a, CreatedAgent> {
        Box::pin(async move {
            if command.actor != command.owner_id
                || !(1..=600).contains(&command.rate_limit_per_minute)
            {
                return Err(RepositoryError::PermissionDenied);
            }
            let agent_id = UserId::from_uuid(Uuid::now_v7());
            let now = Utc::now();
            let credential_expires_at = now + Duration::days(90);
            let mut secret = [0_u8; 32];
            getrandom::fill(&mut secret).map_err(storage)?;
            let credential = URL_SAFE_NO_PAD.encode(secret);
            let hash = Sha256::digest(credential.as_bytes()).to_vec();
            let display_name = DisplayName::new(command.display_name).map_err(storage)?;
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("insert into users(id,kind,display_name,external_provider,external_subject,created_at) values(?,'agent',?,?,?,?)")
                .bind(agent_id.to_string()).bind(display_name.as_str()).bind(&command.provider)
                .bind(&command.service_identity).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,expires_at,created_at) values(?,?,?,?,?,?,?,?,?)")
                .bind(agent_id.to_string()).bind(command.owner_id.to_string()).bind(command.actor.to_string())
                .bind(command.provider).bind(command.service_identity).bind(command.purpose)
                .bind(i64::from(command.rate_limit_per_minute)).bind(command.expires_at).bind(now)
                .execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into agent_credentials(id,agent_id,token_hash,expires_at,created_at) values(?,?,?,?,?)")
                .bind(Uuid::now_v7().to_string()).bind(agent_id.to_string()).bind(hash)
                .bind(credential_expires_at).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(CreatedAgent {
                agent_id,
                credential,
                credential_expires_at,
            })
        })
    }

    fn grant_agent<'a>(&'a self, command: GrantAgent) -> AgentFuture<'a, Uuid> {
        Box::pin(async move {
            if command.circle_id.is_none() && command.channel_id.is_none() {
                return Err(RepositoryError::Conflict);
            }
            let owner: Option<i64> = sqlx::query_scalar("select 1 from agent_profiles where agent_id=? and owner_id=? and revoked_at is null and (expires_at is null or expires_at>?)")
                .bind(command.agent_id.to_string()).bind(command.actor.to_string()).bind(Utc::now())
                .fetch_optional(&self.pool).await.map_err(sql_error)?;
            if owner.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
            if let Some(circle_id) = &command.circle_id {
                let role: Option<String> = sqlx::query_scalar(
                    "select role from circle_memberships where circle_id=? and user_id=?",
                )
                .bind(circle_id.to_string())
                .bind(command.actor.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
                let role = role.as_deref().and_then(CircleRole::parse);
                if !Policy::can_invite_agent_to_circle(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            if let Some(channel_id) = &command.channel_id {
                let role: Option<String> = sqlx::query_scalar(
                    "select role from channel_memberships where channel_id=? and user_id=?",
                )
                .bind(channel_id.to_string())
                .bind(command.actor.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
                let role = role.as_deref().and_then(MembershipRole::parse);
                if !Policy::can_invite_agent_to_channel(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            let proposed_id = Uuid::now_v7();
            let id: String = sqlx::query_scalar("insert into agent_grants(id,agent_id,circle_id,channel_id,scope,granted_by,expires_at,created_at) values(?,?,?,?,?,?,?,?) on conflict do update set revoked_at=null,revoked_by=null,expires_at=excluded.expires_at,granted_by=excluded.granted_by returning id")
                .bind(proposed_id.to_string()).bind(command.agent_id.to_string()).bind(command.circle_id.map(|v| v.to_string()))
                .bind(command.channel_id.as_ref().map(ToString::to_string)).bind(command.scope.as_str()).bind(command.actor.to_string())
                .bind(command.expires_at).bind(Utc::now()).fetch_one(&self.pool).await.map_err(sql_error)?;
            if let Some(channel_id) = command.channel_id {
                let role = if matches!(command.scope, AgentScope::ReadHistory) {
                    "observer"
                } else {
                    "member"
                };
                sqlx::query("insert into channel_memberships(channel_id,user_id,role,last_read_sequence,joined_at) values(?,?,?,0,?) on conflict(channel_id,user_id) do update set role=case when channel_memberships.role='observer' and excluded.role='member' then 'member' else channel_memberships.role end")
                    .bind(channel_id.to_string()).bind(command.agent_id.to_string()).bind(role).bind(Utc::now()).execute(&self.pool).await.map_err(sql_error)?;
            }
            Uuid::parse_str(&id).map_err(storage)
        })
    }

    fn revoke_grant<'a>(&'a self, actor: UserId, grant_id: Uuid) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let changed = sqlx::query("update agent_grants set revoked_at=?,revoked_by=? where id=? and revoked_at is null and agent_id in (select agent_id from agent_profiles where owner_id=?)")
                .bind(Utc::now()).bind(actor.to_string()).bind(grant_id.to_string()).bind(actor.to_string()).execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            Ok(())
        })
    }

    fn authenticate_agent<'a>(&'a self, credential: &'a str) -> AgentFuture<'a, AgentPrincipal> {
        Box::pin(async move {
            let hash = Sha256::digest(credential.as_bytes()).to_vec();
            let now = Utc::now();
            let row = sqlx::query("select p.agent_id,p.owner_id,p.purpose,p.rate_limit_per_minute from agent_credentials c join agent_profiles p on p.agent_id=c.agent_id where c.token_hash=? and c.revoked_at is null and c.expires_at>? and p.revoked_at is null and (p.expires_at is null or p.expires_at>?)")
                .bind(hash).bind(now).bind(now).fetch_optional(&self.pool).await.map_err(sql_error)?
                .ok_or(RepositoryError::PermissionDenied)?;
            sqlx::query("update agent_credentials set last_used_at=? where token_hash=?")
                .bind(now)
                .bind(Sha256::digest(credential.as_bytes()).to_vec())
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            Ok(AgentPrincipal {
                agent_id: UserId::new(row.try_get::<String, _>("agent_id").map_err(storage)?)
                    .map_err(storage)?,
                owner_id: UserId::new(row.try_get::<String, _>("owner_id").map_err(storage)?)
                    .map_err(storage)?,
                purpose: row.try_get("purpose").map_err(storage)?,
                rate_limit_per_minute: u16::try_from(
                    row.try_get::<i64, _>("rate_limit_per_minute")
                        .map_err(storage)?,
                )
                .map_err(storage)?,
            })
        })
    }

    fn consume_rate_limit<'a>(
        &'a self,
        agent_id: UserId,
        limit_per_minute: u16,
    ) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let now = Utc::now();
            let cutoff = now - Duration::seconds(60);
            let consumed: Option<i64> = sqlx::query_scalar(
                "insert into agent_rate_limits(agent_id,window_started_at,request_count) values(?,?,1) \
                 on conflict(agent_id) do update set \
                 window_started_at=case when agent_rate_limits.window_started_at<=? then excluded.window_started_at else agent_rate_limits.window_started_at end, \
                 request_count=case when agent_rate_limits.window_started_at<=? then 1 else agent_rate_limits.request_count+1 end \
                 where agent_rate_limits.window_started_at<=? or agent_rate_limits.request_count<? \
                 returning request_count",
            )
            .bind(agent_id.to_string())
            .bind(now)
            .bind(cutoff)
            .bind(cutoff)
            .bind(cutoff)
            .bind(i64::from(limit_per_minute))
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            consumed.map(|_| ()).ok_or(RepositoryError::Conflict)
        })
    }

    fn has_scope<'a>(
        &'a self,
        agent_id: UserId,
        circle_id: Option<CircleId>,
        channel_id: Option<ChannelId>,
        scope: AgentScope,
    ) -> AgentFuture<'a, bool> {
        Box::pin(async move {
            let found: Option<i64> = sqlx::query_scalar("select 1 from agent_grants where agent_id=? and scope=? and revoked_at is null and (expires_at is null or expires_at>?) and ((channel_id is not null and channel_id=?) or (circle_id is not null and circle_id=?)) limit 1")
                .bind(agent_id.to_string()).bind(scope.as_str()).bind(Utc::now()).bind(channel_id.map(|v| v.to_string()))
                .bind(circle_id.map(|v| v.to_string())).fetch_optional(&self.pool).await.map_err(sql_error)?;
            Ok(found.is_some())
        })
    }

    fn mark_delegated<'a>(
        &'a self,
        agent_id: UserId,
        message_id: MessageId,
    ) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let changed=sqlx::query("update message_provenance set provenance='delegated',delegated_by=owner_id where message_id=? and agent_id=? and provenance in ('generated','delegated')")
                .bind(message_id.as_uuid().to_string()).bind(agent_id.to_string()).execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            Ok(())
        })
    }

    fn approve_message<'a>(&'a self, actor: UserId, message_id: MessageId) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let changed=sqlx::query("update message_provenance set provenance='human_approved',approved_by=?,approved_at=? where message_id=? and owner_id=? and agent_id is not null")
                .bind(actor.to_string()).bind(Utc::now()).bind(message_id.as_uuid().to_string()).bind(actor.to_string())
                .execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            Ok(())
        })
    }

    fn message_provenance<'a>(
        &'a self,
        message_id: MessageId,
    ) -> AgentFuture<'a, MessageProvenance> {
        Box::pin(async move {
            let row=sqlx::query("select provenance,agent_id,owner_id,approved_by from message_provenance where message_id=?").bind(message_id.as_uuid().to_string()).fetch_optional(&self.pool).await.map_err(sql_error)?.ok_or(RepositoryError::NotFound)?;
            let parse_user =
                |value: Option<String>| value.map(UserId::new).transpose().map_err(storage);
            let raw: String = row.try_get("provenance").map_err(storage)?;
            Ok(MessageProvenance {
                message_id,
                provenance: ActivityProvenance::parse(&raw)
                    .ok_or_else(|| storage("invalid provenance"))?,
                agent_id: parse_user(row.try_get("agent_id").map_err(storage)?)?,
                owner_id: parse_user(row.try_get("owner_id").map_err(storage)?)?,
                approved_by: parse_user(row.try_get("approved_by").map_err(storage)?)?,
            })
        })
    }
}

fn process_link_from_sqlite(
    row: sqlx::sqlite::SqliteRow,
    channel_id: ChannelId,
    actor: UserId,
) -> Result<ProcessLink, RepositoryError> {
    let id = Uuid::parse_str(&row.try_get::<String, _>("id").map_err(storage)?).map_err(storage)?;
    let heart = row
        .try_get::<Option<String>, _>("heart_instance_id")
        .map_err(storage)?
        .map(|value| Uuid::parse_str(&value))
        .transpose()
        .map_err(storage)?;
    Ok(ProcessLink {
        id: ProcessLinkId::from_uuid(id),
        channel_id,
        heart_instance_id: heart,
        namespace: row.try_get("namespace").map_err(storage)?,
        definition_name: row.try_get("definition_name").map_err(storage)?,
        definition_version: row.try_get("definition_version").map_err(storage)?,
        initiated_by: actor,
        status: row.try_get("status").map_err(storage)?,
    })
}

fn outbox_from_sqlite(row: &sqlx::sqlite::SqliteRow) -> Result<OutboxJob, RepositoryError> {
    let id = Uuid::parse_str(&row.try_get::<String, _>("id").map_err(storage)?).map_err(storage)?;
    let link = Uuid::parse_str(
        &row.try_get::<String, _>("process_link_id")
            .map_err(storage)?,
    )
    .map_err(storage)?;
    let payload: String = row.try_get("payload").map_err(storage)?;
    Ok(OutboxJob {
        id: OutboxId::from_uuid(id),
        process_link_id: ProcessLinkId::from_uuid(link),
        operation: serde_json::from_str(&payload).map_err(storage)?,
        attempts: row.try_get::<i64, _>("attempts").map_err(storage)? as u32,
    })
}

async fn ensure_membership(
    pool: &SqlitePool,
    channel_id: &ChannelId,
    actor: &UserId,
) -> Result<(), RepositoryError> {
    let role: Option<String> = sqlx::query_scalar(
        "select role from channel_memberships where channel_id = ? and user_id = ?",
    )
    .bind(channel_id.to_string())
    .bind(actor.to_string())
    .fetch_optional(pool)
    .await
    .map_err(sql_error)?;
    let role = role.as_deref().and_then(MembershipRole::parse);
    Policy::can_read_channel(role.as_ref())
        .then_some(())
        .ok_or(RepositoryError::PermissionDenied)
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
    let circle_id: Option<String> = row.try_get("circle_id").map_err(storage)?;
    let last_read_sequence: i64 = row.try_get("last_read_sequence").map_err(storage)?;
    let latest_sequence: i64 = row.try_get("latest_sequence").map_err(storage)?;
    Ok(ChannelSummary {
        id: ChannelId::new(id).map_err(storage)?,
        slug: ChannelSlug::new(slug).map_err(storage)?,
        name: DisplayName::new(name).map_err(storage)?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| storage("invalid channel kind"))?,
        circle_id: circle_id
            .map(|id| uuid::Uuid::parse_str(&id).map(CircleId::from_uuid))
            .transpose()
            .map_err(storage)?,
        role: MembershipRole::parse(&role).ok_or_else(|| storage("invalid membership role"))?,
        last_read_sequence: ChannelSequence::try_from(last_read_sequence).map_err(storage)?,
        latest_sequence: ChannelSequence::try_from(latest_sequence).map_err(storage)?,
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

fn persisted_now() -> DateTime<Utc> {
    DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("current UTC timestamp is representable")
}

fn circle_with_role(row: sqlx::sqlite::SqliteRow) -> Result<(Circle, CircleRole), RepositoryError> {
    let id = uuid::Uuid::parse_str(&row.try_get::<String, _>("id").map_err(storage)?)
        .map_err(storage)?;
    let created_by =
        UserId::new(row.try_get::<String, _>("created_by").map_err(storage)?).map_err(storage)?;
    let role: String = row.try_get("role").map_err(storage)?;
    Ok((
        Circle {
            id: CircleId::from_uuid(id),
            slug: ChannelSlug::new(row.try_get::<String, _>("slug").map_err(storage)?)
                .map_err(storage)?,
            name: DisplayName::new(row.try_get::<String, _>("name").map_err(storage)?)
                .map_err(storage)?,
            created_by,
            created_at: row.try_get("created_at").map_err(storage)?,
        },
        CircleRole::parse(&role).ok_or_else(|| storage("invalid circle role"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{PrincipalKind, User};

    #[tokio::test]
    async fn sqlite_passes_shared_repository_contract() {
        let repository = SqliteChatRepository::connect("sqlite::memory:")
            .await
            .unwrap();
        repository.migrate().await.unwrap();
        super::super::verify_repository_contract(&repository, "sqlite-shared").await;

        let rows = sqlx::query_as::<_, (String, Option<String>, String, String, String)>(
            "select action, actor_id, target_kind, target_id, payload from audit_events \
             where action in ('agent.created', 'agent.grant_created', \
             'agent.grant_changed', 'agent.grant_revoked', 'process.started', \
             'process.correlation_requested', 'process.inspection_requested', \
             'circle.feature_changed')",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap();
        for expected in [
            "agent.created",
            "agent.grant_created",
            "agent.grant_changed",
            "agent.grant_revoked",
            "process.started",
            "process.correlation_requested",
            "process.inspection_requested",
            "circle.feature_changed",
        ] {
            assert!(
                rows.iter().any(|row| row.0 == expected),
                "missing {expected}"
            );
        }
        for (action, actor_id, target_kind, target_id, payload) in rows {
            assert!(actor_id.is_some(), "{action} has no actor");
            assert!(!target_kind.is_empty(), "{action} has no target kind");
            assert!(!target_id.is_empty(), "{action} has no target id");
            assert_ne!(payload, "{}", "{action} has no cause payload");
        }
        let feature_changes: i64 = sqlx::query_scalar(
            "select count(*) from audit_events where action='circle.feature_changed'",
        )
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        assert_eq!(feature_changes, 2, "feature update was not audited");
        let membership_left = sqlx::query_as::<_, (Option<String>, String, String, String)>(
            "select actor_id,target_kind,target_id,payload from audit_events where action='channel.membership_left'",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap();
        assert!(!membership_left.is_empty());
        assert!(
            membership_left
                .iter()
                .all(|(actor, kind, target, payload)| {
                    actor.is_some() && kind == "channel" && !target.is_empty() && payload != "{}"
                })
        );
        let process_events = sqlx::query_as::<_, (String, Option<String>)>(
            "select event_type, actor_id from process_events",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap();
        assert_eq!(
            process_events
                .iter()
                .filter(|event| event.0 == "process.started")
                .count(),
            1,
            "idempotent completion duplicated the start event"
        );
        assert!(
            process_events
                .iter()
                .any(|event| event.0 == "process.correlated")
        );
        assert!(process_events.iter().all(|event| event.1.is_some()));
    }

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
        let circle = repository
            .create_circle(CreateCircle {
                actor: alice.clone(),
                slug: ChannelSlug::new("sqlite-circle").unwrap(),
                name: DisplayName::new("SQLite circle").unwrap(),
            })
            .await
            .unwrap();
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("sqlite").unwrap(),
                name: DisplayName::new("SQLite").unwrap(),
                kind: ChannelKind::Private,
                circle_id: Some(circle.id.clone()),
            })
            .await
            .unwrap();
        repository
            .set_circle_feature(SetCircleFeature {
                circle_id: circle.id,
                actor: alice.clone(),
                feature: "heart.event-planning".to_owned(),
                enabled: true,
            })
            .await
            .unwrap();
        let agent = repository
            .create_agent(CreateAgent {
                actor: alice.clone(),
                owner_id: alice.clone(),
                display_name: "Planner agent".to_owned(),
                provider: "contract-test".to_owned(),
                service_identity: "planner-1".to_owned(),
                purpose: "Help plan events".to_owned(),
                rate_limit_per_minute: 60,
                expires_at: None,
            })
            .await
            .unwrap();
        assert!(repository.authenticate_agent("wrong-token").await.is_err());
        let authenticated = repository
            .authenticate_agent(&agent.credential)
            .await
            .unwrap();
        assert_eq!(authenticated.agent_id, agent.agent_id);
        let grant_id = repository
            .grant_agent(GrantAgent {
                actor: alice.clone(),
                agent_id: agent.agent_id.clone(),
                circle_id: None,
                channel_id: Some(channel.id.clone()),
                scope: AgentScope::ReadHistory,
                expires_at: None,
            })
            .await
            .unwrap();
        assert!(
            repository
                .has_scope(
                    agent.agent_id.clone(),
                    None,
                    Some(channel.id.clone()),
                    AgentScope::ReadHistory
                )
                .await
                .unwrap()
        );
        repository
            .revoke_grant(alice.clone(), grant_id)
            .await
            .unwrap();
        assert!(
            !repository
                .has_scope(
                    agent.agent_id,
                    None,
                    Some(channel.id.clone()),
                    AgentScope::ReadHistory
                )
                .await
                .unwrap()
        );
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
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                limit: crate::domain::MessageLimit::DEFAULT,
                after: None,
            })
            .await
            .unwrap();
        assert_eq!(loaded, vec![message]);

        let first = repository
            .append_message_idempotent(
                SendMessage {
                    actor: UserId::named("sqlite-alice"),
                    channel_id: channel.id.clone(),
                    body: MessageBody::new("once").unwrap(),
                },
                "request-1".to_owned(),
            )
            .await
            .unwrap();
        let repeated = repository
            .append_message_idempotent(
                SendMessage {
                    actor: UserId::named("sqlite-alice"),
                    channel_id: channel.id.clone(),
                    body: MessageBody::new("twice").unwrap(),
                },
                "request-1".to_owned(),
            )
            .await
            .unwrap();
        assert_eq!(first, repeated);

        let process = repository
            .enqueue_start(EnqueueProcessStart {
                channel_id: channel.id.clone(),
                actor: alice.clone(),
                request_id: "process-request-1".to_owned(),
                namespace: "sproyt".to_owned(),
                definition_name: "event-plan".to_owned(),
                definition_version: Some("1".to_owned()),
                metadata: serde_json::json!({"title": "Dinner"}),
            })
            .await
            .unwrap();
        let repeated_process = repository
            .enqueue_start(EnqueueProcessStart {
                channel_id: channel.id.clone(),
                actor: alice.clone(),
                request_id: "process-request-1".to_owned(),
                namespace: "sproyt".to_owned(),
                definition_name: "ignored-on-replay".to_owned(),
                definition_version: None,
                metadata: serde_json::Value::Null,
            })
            .await
            .unwrap();
        assert_eq!(process.id, repeated_process.id);
        let job = repository
            .lease_next(std::time::Duration::from_secs(30))
            .await
            .unwrap()
            .unwrap();
        let instance_id = Uuid::now_v7();
        repository
            .complete_start(job, StartedProcess { instance_id })
            .await
            .unwrap();
        let completed: (String, i64) = sqlx::query_as(
            "select status, (select count(*) from process_events where process_link_id = ?) from process_links where id = ?",
        )
        .bind(process.id.as_uuid().to_string())
        .bind(process.id.as_uuid().to_string())
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        assert_eq!(completed, ("active".to_owned(), 1));

        let bob = UserId::named("sqlite-bob");
        repository
            .upsert_user(User {
                id: bob.clone(),
                kind: PrincipalKind::Human,
                display_name: DisplayName::new("Bob").unwrap(),
                external_provider: None,
                external_subject: None,
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        let circle = repository
            .create_circle(CreateCircle {
                actor: UserId::named("sqlite-alice"),
                slug: ChannelSlug::new("sqlite-friends").unwrap(),
                name: DisplayName::new("SQLite Friends").unwrap(),
            })
            .await
            .unwrap();
        repository
            .set_circle_feature(SetCircleFeature {
                circle_id: circle.id.clone(),
                actor: alice.clone(),
                feature: "heart.event-planning".to_owned(),
                enabled: true,
            })
            .await
            .unwrap();
        let pilot_channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("event-pilot").unwrap(),
                name: DisplayName::new("Event pilot").unwrap(),
                kind: ChannelKind::Private,
                circle_id: Some(circle.id.clone()),
            })
            .await
            .unwrap();
        let pilot_link = repository
            .enqueue_start(EnqueueProcessStart {
                channel_id: pilot_channel.id,
                actor: alice.clone(),
                request_id: "pilot-start".to_owned(),
                namespace: "sproyt".to_owned(),
                definition_name: "sproyt-event-planning".to_owned(),
                definition_version: Some("1.0.0".to_owned()),
                metadata: serde_json::json!({"title": "Dinner"}),
            })
            .await
            .unwrap();
        let first_correlation = repository
            .enqueue_correlation(EnqueueCorrelation {
                process_link_id: pilot_link.id,
                actor: alice.clone(),
                request_id: "pilot-answer".to_owned(),
                payload: serde_json::json!({"decision": "yes"}),
            })
            .await
            .unwrap();
        let repeated_correlation = repository
            .enqueue_correlation(EnqueueCorrelation {
                process_link_id: pilot_link.id,
                actor: alice.clone(),
                request_id: "pilot-answer".to_owned(),
                payload: serde_json::json!({"decision": "no"}),
            })
            .await
            .unwrap();
        assert_eq!(first_correlation, repeated_correlation);
        let invitation = repository
            .create_circle_invitation(CreateCircleInvitation {
                actor: UserId::named("sqlite-alice"),
                circle_id: circle.id.clone(),
            })
            .await
            .unwrap();
        repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: bob.clone(),
                token: invitation.token.clone(),
            })
            .await
            .unwrap();
        let reused = repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: bob,
                token: invitation.token,
            })
            .await;
        assert_eq!(reused, Err(RepositoryError::NotFound));
        let audit_count: i64 = sqlx::query_scalar("select count(*) from audit_events")
            .fetch_one(&repository.pool)
            .await
            .unwrap();
        assert!(audit_count >= 6);
    }
}
