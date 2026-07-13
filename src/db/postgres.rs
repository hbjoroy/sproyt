use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Duration;
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::{
    PgPool, Row,
    postgres::{PgListener, PgRow},
};
use tokio::sync::broadcast;
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
    EnqueueCorrelation, EnqueueProcessStart, OutboxId, OutboxJob, OutboxOperation, ProcessError,
    ProcessLink, ProcessLinkId, ProcessRepository, ProcessRepositoryFuture, SetCircleFeature,
    StartProcess, StartedProcess,
};

use super::{sql_error, storage};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/postgres");

#[derive(Clone)]
pub struct PostgresChatRepository {
    pool: PgPool,
    messages: broadcast::Sender<MessageId>,
}

impl PostgresChatRepository {
    pub async fn connect(url: &str) -> Result<Self, RepositoryError> {
        let pool = PgPool::connect(url).await.map_err(sql_error)?;
        let mut listener = PgListener::connect(url).await.map_err(sql_error)?;
        listener
            .listen("sproyt_messages")
            .await
            .map_err(sql_error)?;
        let (messages, _) = broadcast::channel(1024);
        let publisher = messages.clone();
        tokio::spawn(async move {
            loop {
                match listener.recv().await {
                    Ok(notification) => match uuid::Uuid::parse_str(notification.payload()) {
                        Ok(id) => {
                            let _ = publisher.send(MessageId::from_uuid(id));
                        }
                        Err(error) => {
                            tracing::warn!(%error, "ignored invalid sproyt_messages notification")
                        }
                    },
                    Err(error) => {
                        tracing::error!(%error, "PostgreSQL message listener stopped");
                        break;
                    }
                }
            }
        });
        Ok(Self { pool, messages })
    }

    pub async fn migrate(&self) -> Result<(), RepositoryError> {
        MIGRATOR.run(&self.pool).await.map_err(storage)
    }
}

impl ChatRepository for PostgresChatRepository {
    fn health_check(&self) -> RepositoryFuture<'_, ()> {
        Box::pin(async move {
            sqlx::query_scalar::<_, i32>("select 1")
                .fetch_one(&self.pool)
                .await
                .map_err(sql_error)
                .map(|_| ())
        })
    }
    fn upsert_user<'a>(&'a self, user: User) -> RepositoryFuture<'a, User> {
        Box::pin(async move {
            sqlx::query("insert into users (id, kind, display_name, external_provider, external_subject, created_at) values ($1, $2, $3, $4, $5, $6) on conflict(id) do update set kind = excluded.kind, display_name = excluded.display_name, external_provider = excluded.external_provider, external_subject = excluded.external_subject")
                .bind(*user.id.as_uuid())
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
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("insert into circles (id, slug, name, created_by, created_at) values ($1,$2,$3,$4,$5)")
                .bind(*circle.id.as_uuid()).bind(circle.slug.as_str()).bind(circle.name.as_str())
                .bind(*circle.created_by.as_uuid()).bind(circle.created_at).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into circle_memberships (circle_id,user_id,role,joined_at) values ($1,$2,'owner',$3)")
                .bind(*circle.id.as_uuid()).bind(*circle.created_by.as_uuid()).bind(circle.created_at)
                .execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(circle)
        })
    }

    fn list_circles_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<(Circle, CircleRole)>> {
        Box::pin(async move {
            let rows = sqlx::query("select c.id,c.slug,c.name,c.created_by,c.created_at,m.role from circles c join circle_memberships m on m.circle_id=c.id where m.user_id=$1 order by c.slug")
                .bind(*actor.as_uuid()).fetch_all(&self.pool).await.map_err(sql_error)?;
            rows.into_iter().map(circle_with_role).collect()
        })
    }

    fn create_circle_invitation<'a>(
        &'a self,
        command: CreateCircleInvitation,
    ) -> RepositoryFuture<'a, IssuedInvitation> {
        Box::pin(async move {
            let role: Option<String> = sqlx::query_scalar(
                "select role from circle_memberships where circle_id=$1 and user_id=$2",
            )
            .bind(*command.circle_id.as_uuid())
            .bind(*command.actor.as_uuid())
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
            sqlx::query("insert into circle_invitations (id,circle_id,invited_by,token_hash,expires_at) values ($1,$2,$3,$4,$5)")
                .bind(*invitation.id.as_uuid()).bind(*invitation.circle_id.as_uuid()).bind(*invitation.invited_by.as_uuid())
                .bind(token_hash).bind(invitation.expires_at).execute(&self.pool).await.map_err(sql_error)?;
            Ok(IssuedInvitation { invitation, token })
        })
    }

    fn accept_circle_invitation<'a>(
        &'a self,
        command: AcceptCircleInvitation,
    ) -> RepositoryFuture<'a, CircleMembership> {
        Box::pin(async move {
            let hash = Sha256::digest(command.token.as_bytes()).to_vec();
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let row=sqlx::query("select id,circle_id,expires_at from circle_invitations where token_hash=$1 and accepted_at is null for update")
                .bind(hash).fetch_optional(&mut *tx).await.map_err(sql_error)?.ok_or(RepositoryError::NotFound)?;
            let id: uuid::Uuid = row.try_get("id").map_err(storage)?;
            let circle_uuid: uuid::Uuid = row.try_get("circle_id").map_err(storage)?;
            let expires_at: chrono::DateTime<Utc> = row.try_get("expires_at").map_err(storage)?;
            if expires_at < Utc::now() {
                return Err(RepositoryError::NotFound);
            }
            let joined_at = Utc::now();
            sqlx::query("update circle_invitations set accepted_by=$1,accepted_at=$2 where id=$3 and accepted_at is null")
                .bind(*command.actor.as_uuid()).bind(joined_at).bind(id).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into circle_memberships (circle_id,user_id,role,joined_at) values ($1,$2,'member',$3) on conflict(circle_id,user_id) do nothing")
                .bind(circle_uuid).bind(*command.actor.as_uuid()).bind(joined_at).execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(CircleMembership {
                circle_id: CircleId::from_uuid(circle_uuid),
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
                    "select role from circle_memberships where circle_id = $1 and user_id = $2",
                )
                .bind(*circle_id.as_uuid())
                .bind(*channel.created_by.as_uuid())
                .fetch_optional(&mut *transaction)
                .await
                .map_err(sql_error)?;
                let role = allowed.as_deref().and_then(CircleRole::parse);
                if !Policy::can_create_channel_in_circle(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            sqlx::query("insert into channels (id, slug, name, kind, circle_id, created_by) values ($1, $2, $3, $4, $5, $6)")
                .bind(*channel.id.as_uuid())
                .bind(channel.slug.as_str())
                .bind(channel.name.as_str())
                .bind(channel.kind.as_str())
                .bind(channel.circle_id.as_ref().map(|id| *id.as_uuid()))
                .bind(*channel.created_by.as_uuid())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            sqlx::query("insert into channel_sequences (channel_id) values ($1)")
                .bind(*channel.id.as_uuid())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) values ($1, $2, 'owner')")
                .bind(*channel.id.as_uuid())
                .bind(*channel.created_by.as_uuid())
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
                    let value: Option<uuid::Uuid> =
                        sqlx::query_scalar("select id from channels where slug = $1")
                            .bind(slug.as_str())
                            .fetch_optional(&self.pool)
                            .await
                            .map_err(sql_error)?;
                    ChannelId::from_uuid(value.ok_or(RepositoryError::NotFound)?)
                }
            };
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) values ($1, $2, 'member') on conflict(channel_id, user_id) do nothing")
                .bind(*channel_id.as_uuid())
                .bind(*command.actor.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            load_membership(&self.pool, channel_id, command.actor).await
        })
    }

    fn leave_channel<'a>(&'a self, command: LeaveChannel) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let role: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = $1 and user_id = $2",
            )
            .bind(*command.channel_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_leave_channel(role.as_ref()) {
                return Err(RepositoryError::NotFound);
            }
            let result = sqlx::query(
                "delete from channel_memberships where channel_id = $1 and user_id = $2",
            )
            .bind(*command.channel_id.as_uuid())
            .bind(*command.actor.as_uuid())
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
            let rows = sqlx::query("select c.id, c.slug, c.name, c.kind, c.circle_id, m.role from channels c join channel_memberships m on m.channel_id = c.id where m.user_id = $1 order by c.slug")
                .bind(*actor.as_uuid())
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
            let after = i64::try_from(query.after.map_or(0, u64::from)).map_err(storage)?;
            let limit = i64::try_from(usize::from(query.limit)).map_err(storage)?;
            let rows = sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where channel_id = $1 and sequence > $2 order by sequence desc limit $3")
                .bind(*query.channel_id.as_uuid())
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
            let membership: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = $1 and user_id = $2",
            )
            .bind(*command.channel_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sql_error)?;
            let role = membership.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_send_to_channel(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let sequence: i64 = sqlx::query_scalar("update channel_sequences set next_sequence = next_sequence + 1 where channel_id = $1 returning next_sequence - 1")
                .bind(*command.channel_id.as_uuid())
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
            sqlx::query("insert into messages (id, channel_id, sender_id, sequence, body, created_at) values ($1, $2, $3, $4, $5, $6)")
                .bind(*message.id.as_uuid())
                .bind(*message.channel_id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(sequence)
                .bind(message.body.as_str())
                .bind(message.sent_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            if let Err(error) = sqlx::query("select pg_notify('sproyt_messages', $1)")
                .bind(message.id.as_uuid().to_string())
                .execute(&self.pool)
                .await
            {
                tracing::warn!(%error, message_id = %message.id.as_uuid(), "message persisted but realtime notification failed");
            }
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
            let reservation = sqlx::query("insert into command_receipts (principal_id, request_id) values ($1, $2) on conflict(principal_id, request_id) do nothing")
                .bind(*command.actor.as_uuid())
                .bind(&request_id)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            if reservation.rows_affected() == 0 {
                let row = sqlx::query("select m.id, m.channel_id, m.sender_id, m.sequence, m.body, m.created_at from command_receipts r join messages m on m.id = r.message_id where r.principal_id = $1 and r.request_id = $2")
                    .bind(*command.actor.as_uuid())
                    .bind(&request_id)
                    .fetch_optional(&mut *transaction)
                    .await
                    .map_err(sql_error)?
                    .ok_or_else(|| storage("idempotency receipt has no message"))?;
                return chat_message(row);
            }
            let membership: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id = $1 and user_id = $2",
            )
            .bind(*command.channel_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&mut *transaction)
            .await
            .map_err(sql_error)?;
            let role = membership.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_send_to_channel(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let sequence: i64 = sqlx::query_scalar("update channel_sequences set next_sequence = next_sequence + 1 where channel_id = $1 returning next_sequence - 1")
                .bind(*command.channel_id.as_uuid())
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
            sqlx::query("insert into messages (id, channel_id, sender_id, sequence, body, created_at) values ($1, $2, $3, $4, $5, $6)")
                .bind(*message.id.as_uuid())
                .bind(*message.channel_id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(sequence)
                .bind(message.body.as_str())
                .bind(message.sent_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            sqlx::query("update command_receipts set message_id = $1 where principal_id = $2 and request_id = $3")
                .bind(*message.id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(&request_id)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            if let Err(error) = sqlx::query("select pg_notify('sproyt_messages', $1)")
                .bind(message.id.as_uuid().to_string())
                .execute(&self.pool)
                .await
            {
                tracing::warn!(%error, message_id = %message.id.as_uuid(), "message persisted but realtime notification failed");
            }
            Ok(message)
        })
    }

    fn load_message<'a>(&'a self, id: MessageId) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let row = sqlx::query("select id, channel_id, sender_id, sequence, body, created_at from messages where id = $1")
                .bind(*id.as_uuid())
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
                sqlx::query_scalar("select max(sequence) from messages where channel_id = $1")
                    .bind(*channel_id.as_uuid())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(sql_error)?;
            ChannelSequence::try_from(latest.unwrap_or(0)).map_err(storage)
        })
    }

    fn mark_read<'a>(&'a self, command: MarkRead) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let latest: Option<i64> =
                sqlx::query_scalar("select max(sequence) from messages where channel_id = $1")
                    .bind(*command.channel_id.as_uuid())
                    .fetch_one(&self.pool)
                    .await
                    .map_err(sql_error)?;
            let requested = i64::try_from(u64::from(command.sequence)).map_err(storage)?;
            if requested > latest.unwrap_or(0) {
                return Err(RepositoryError::NotFound);
            }
            let result = sqlx::query("update channel_memberships set last_read_sequence = greatest(last_read_sequence, $1) where channel_id = $2 and user_id = $3")
                .bind(requested)
                .bind(*command.channel_id.as_uuid())
                .bind(*command.actor.as_uuid())
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            if result.rows_affected() == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            load_membership(&self.pool, command.channel_id, command.actor).await
        })
    }

    fn subscribe_messages(&self) -> Option<broadcast::Receiver<MessageId>> {
        Some(self.messages.subscribe())
    }
}

impl ProcessRepository for PostgresChatRepository {
    fn enqueue_start<'a>(
        &'a self,
        command: EnqueueProcessStart,
    ) -> ProcessRepositoryFuture<'a, ProcessLink> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let role: Option<String> = sqlx::query_scalar(
                "select role from channel_memberships where channel_id=$1 and user_id=$2",
            )
            .bind(*command.channel_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(MembershipRole::parse);
            if !Policy::can_start_process(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            if let Some(row) = sqlx::query("select id,heart_instance_id,namespace,definition_name,definition_version,status from process_links where initiated_by=$1 and request_id=$2")
                .bind(*command.actor.as_uuid()).bind(&command.request_id).fetch_optional(&mut *tx).await.map_err(sql_error)? {
                tx.commit().await.map_err(sql_error)?;
                return process_link_from_postgres(row, command.channel_id, command.actor);
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
            sqlx::query("insert into process_links(id,channel_id,namespace,definition_name,definition_version,initiated_by,request_id,created_at,updated_at) values($1,$2,$3,$4,$5,$6,$7,$8,$8)")
                .bind(link_id.as_uuid()).bind(*command.channel_id.as_uuid()).bind(&command.namespace)
                .bind(&command.definition_name).bind(&command.definition_version).bind(*command.actor.as_uuid())
                .bind(&command.request_id).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_outbox(id,process_link_id,operation,payload,available_at,created_at) values($1,$2,'start',$3,$4,$4)")
                .bind(outbox_id.as_uuid()).bind(link_id.as_uuid()).bind(serde_json::to_value(&operation).map_err(storage)?)
                .bind(now).execute(&mut *tx).await.map_err(sql_error)?;
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
            if let Some(existing) = sqlx::query_scalar::<_, Uuid>("select outbox_id from process_command_receipts where actor_id=$1 and request_id=$2")
                .bind(*command.actor.as_uuid()).bind(&command.request_id).fetch_optional(&mut *tx).await.map_err(sql_error)? {
                tx.commit().await.map_err(sql_error)?;
                return Ok(OutboxId::from_uuid(existing));
            }
            let access: Option<(String, String)> = sqlx::query_as("select p.namespace,m.role from process_links p join channels c on c.id=p.channel_id join channel_memberships m on m.channel_id=c.id and m.user_id=$1 join circle_features f on f.circle_id=c.circle_id and f.feature='heart.event-planning' and f.enabled where p.id=$2")
                .bind(*command.actor.as_uuid()).bind(command.process_link_id.as_uuid())
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
            sqlx::query("insert into process_outbox(id,process_link_id,operation,payload,available_at,created_at) values($1,$2,'correlate',$3,$4,$4)")
                .bind(outbox_id.as_uuid()).bind(command.process_link_id.as_uuid()).bind(serde_json::to_value(&operation).map_err(storage)?)
                .bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_command_receipts(actor_id,request_id,process_link_id,outbox_id,command_type,created_at) values($1,$2,$3,$4,'correlate',$5)")
                .bind(*command.actor.as_uuid()).bind(command.request_id).bind(command.process_link_id.as_uuid())
                .bind(outbox_id.as_uuid()).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
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
                "select role from circle_memberships where circle_id=$1 and user_id=$2",
            )
            .bind(*command.circle_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&self.pool)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(CircleRole::parse);
            if !Policy::can_invite_to_circle(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            sqlx::query("insert into circle_features(circle_id,feature,enabled,updated_by,updated_at) values($1,$2,$3,$4,$5) on conflict(circle_id,feature) do update set enabled=excluded.enabled,updated_by=excluded.updated_by,updated_at=excluded.updated_at")
                .bind(*command.circle_id.as_uuid()).bind(command.feature).bind(command.enabled)
                .bind(*command.actor.as_uuid()).bind(Utc::now()).execute(&self.pool).await.map_err(sql_error)?;
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
            let row = sqlx::query("select id,process_link_id,payload,attempts from process_outbox where (status='pending' or (status='leased' and lease_until<$1)) and available_at<=$1 order by created_at for update skip locked limit 1")
                .bind(now).fetch_optional(&mut *tx).await.map_err(sql_error)?;
            let Some(row) = row else {
                tx.commit().await.map_err(sql_error)?;
                return Ok(None);
            };
            let id: Uuid = row.try_get("id").map_err(storage)?;
            let lease_until = now + chrono::Duration::from_std(lease_for).map_err(storage)?;
            sqlx::query("update process_outbox set status='leased',lease_until=$1,attempts=attempts+1 where id=$2")
                .bind(lease_until).bind(id).execute(&mut *tx).await.map_err(sql_error)?;
            let mut job = outbox_from_postgres(&row)?;
            job.attempts += 1;
            tx.commit().await.map_err(sql_error)?;
            Ok(Some(job))
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
            sqlx::query("update process_links set heart_instance_id=$1,status='active',updated_at=$2 where id=$3 and (heart_instance_id is null or heart_instance_id=$1)")
                .bind(result.instance_id).bind(now).bind(job.process_link_id.as_uuid()).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,occurred_at) values($1,$2,'started','process.started',$3,$4) on conflict(process_link_id,event_key) do nothing")
                .bind(Uuid::now_v7()).bind(job.process_link_id.as_uuid()).bind(serde_json::json!({"instance_id": result.instance_id}))
                .bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status='completed',completed_at=$1,lease_until=null where id=$2")
                .bind(now).bind(job.id.as_uuid()).execute(&mut *tx).await.map_err(sql_error)?;
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
            sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,occurred_at) values($1,$2,$3,$4,$5,$6) on conflict(process_link_id,event_key) do nothing")
                .bind(Uuid::now_v7()).bind(job.process_link_id.as_uuid()).bind(job.id.as_uuid().to_string())
                .bind(event_type).bind(payload).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status='completed',completed_at=$1,lease_until=null where id=$2")
                .bind(now).bind(job.id.as_uuid()).execute(&mut *tx).await.map_err(sql_error)?;
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
            let available = Utc::now() + chrono::Duration::from_std(delay).map_err(storage)?;
            sqlx::query("update process_outbox set status=$1,available_at=$2,lease_until=null,last_error=$3 where id=$4")
                .bind(if terminal { "failed" } else { "pending" }).bind(available).bind(error.to_string())
                .bind(job.id.as_uuid()).execute(&self.pool).await.map_err(sql_error)?;
            Ok(())
        })
    }
}

impl AgentRepository for PostgresChatRepository {
    fn create_agent<'a>(&'a self, command: CreateAgent) -> AgentFuture<'a, CreatedAgent> {
        Box::pin(async move {
            if command.actor != command.owner_id
                || !(1..=600).contains(&command.rate_limit_per_minute)
            {
                return Err(RepositoryError::PermissionDenied);
            }
            let agent_id = UserId::from_uuid(Uuid::now_v7());
            let now = Utc::now();
            let credential_expires_at = now + chrono::Duration::days(90);
            let mut secret = [0_u8; 32];
            getrandom::fill(&mut secret).map_err(storage)?;
            let credential = URL_SAFE_NO_PAD.encode(secret);
            let hash = Sha256::digest(credential.as_bytes()).to_vec();
            let display = DisplayName::new(command.display_name).map_err(storage)?;
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("insert into users(id,kind,display_name,external_provider,external_subject,created_at) values($1,'agent',$2,$3,$4,$5)").bind(*agent_id.as_uuid()).bind(display.as_str()).bind(&command.provider).bind(&command.service_identity).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into agent_profiles(agent_id,owner_id,invited_by,provider,service_identity,purpose,rate_limit_per_minute,expires_at,created_at) values($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(*agent_id.as_uuid()).bind(*command.owner_id.as_uuid()).bind(*command.actor.as_uuid()).bind(command.provider).bind(command.service_identity).bind(command.purpose).bind(i32::from(command.rate_limit_per_minute)).bind(command.expires_at).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into agent_credentials(id,agent_id,token_hash,expires_at,created_at) values($1,$2,$3,$4,$5)").bind(Uuid::now_v7()).bind(*agent_id.as_uuid()).bind(hash).bind(credential_expires_at).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
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
            let owner:Option<i32>=sqlx::query_scalar("select 1 from agent_profiles where agent_id=$1 and owner_id=$2 and revoked_at is null and (expires_at is null or expires_at>$3)").bind(*command.agent_id.as_uuid()).bind(*command.actor.as_uuid()).bind(Utc::now()).fetch_optional(&self.pool).await.map_err(sql_error)?;
            if owner.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
            if let Some(id) = &command.circle_id {
                let role: Option<String> = sqlx::query_scalar(
                    "select role from circle_memberships where circle_id=$1 and user_id=$2",
                )
                .bind(*id.as_uuid())
                .bind(*command.actor.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
                let role = role.as_deref().and_then(CircleRole::parse);
                if !Policy::can_invite_agent_to_circle(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            if let Some(id) = &command.channel_id {
                let role: Option<String> = sqlx::query_scalar(
                    "select role from channel_memberships where channel_id=$1 and user_id=$2",
                )
                .bind(*id.as_uuid())
                .bind(*command.actor.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
                let role = role.as_deref().and_then(MembershipRole::parse);
                if !Policy::can_invite_agent_to_channel(role.as_ref()) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            let id = Uuid::now_v7();
            sqlx::query("insert into agent_grants(id,agent_id,circle_id,channel_id,scope,granted_by,expires_at,created_at) values($1,$2,$3,$4,$5,$6,$7,$8) on conflict(agent_id,circle_id,channel_id,scope) do update set revoked_at=null,revoked_by=null,expires_at=excluded.expires_at,granted_by=excluded.granted_by").bind(id).bind(*command.agent_id.as_uuid()).bind(command.circle_id.as_ref().map(|v|*v.as_uuid())).bind(command.channel_id.as_ref().map(|v|*v.as_uuid())).bind(command.scope.as_str()).bind(*command.actor.as_uuid()).bind(command.expires_at).bind(Utc::now()).execute(&self.pool).await.map_err(sql_error)?;
            if let Some(channel_id) = command.channel_id {
                let role = if matches!(command.scope, AgentScope::ReadHistory) {
                    "observer"
                } else {
                    "member"
                };
                sqlx::query("insert into channel_memberships(channel_id,user_id,role,last_read_sequence,joined_at) values($1,$2,$3,0,$4) on conflict(channel_id,user_id) do update set role=case when channel_memberships.role='observer' and excluded.role='member' then 'member' else channel_memberships.role end").bind(*channel_id.as_uuid()).bind(*command.agent_id.as_uuid()).bind(role).bind(Utc::now()).execute(&self.pool).await.map_err(sql_error)?;
            }
            Ok(id)
        })
    }
    fn revoke_grant<'a>(&'a self, actor: UserId, grant_id: Uuid) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let n=sqlx::query("update agent_grants set revoked_at=$1,revoked_by=$2 where id=$3 and revoked_at is null and agent_id in(select agent_id from agent_profiles where owner_id=$2)").bind(Utc::now()).bind(*actor.as_uuid()).bind(grant_id).execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if n == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            Ok(())
        })
    }
    fn authenticate_agent<'a>(&'a self, credential: &'a str) -> AgentFuture<'a, AgentPrincipal> {
        Box::pin(async move {
            let hash = Sha256::digest(credential.as_bytes()).to_vec();
            let now = Utc::now();
            let row=sqlx::query("select p.agent_id,p.owner_id,p.purpose,p.rate_limit_per_minute from agent_credentials c join agent_profiles p on p.agent_id=c.agent_id where c.token_hash=$1 and c.revoked_at is null and c.expires_at>$2 and p.revoked_at is null and(p.expires_at is null or p.expires_at>$2)").bind(&hash).bind(now).fetch_optional(&self.pool).await.map_err(sql_error)?.ok_or(RepositoryError::PermissionDenied)?;
            sqlx::query("update agent_credentials set last_used_at=$1 where token_hash=$2")
                .bind(now)
                .bind(hash)
                .execute(&self.pool)
                .await
                .map_err(sql_error)?;
            Ok(AgentPrincipal {
                agent_id: UserId::from_uuid(row.try_get("agent_id").map_err(storage)?),
                owner_id: UserId::from_uuid(row.try_get("owner_id").map_err(storage)?),
                purpose: row.try_get("purpose").map_err(storage)?,
                rate_limit_per_minute: u16::try_from(
                    row.try_get::<i32, _>("rate_limit_per_minute")
                        .map_err(storage)?,
                )
                .map_err(storage)?,
            })
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
            let found:Option<i32>=sqlx::query_scalar("select 1 from agent_grants where agent_id=$1 and scope=$2 and revoked_at is null and(expires_at is null or expires_at>$3)and((channel_id is not null and channel_id=$4)or(circle_id is not null and circle_id=$5))limit 1").bind(*agent_id.as_uuid()).bind(scope.as_str()).bind(Utc::now()).bind(channel_id.map(|v|*v.as_uuid())).bind(circle_id.map(|v|*v.as_uuid())).fetch_optional(&self.pool).await.map_err(sql_error)?;
            Ok(found.is_some())
        })
    }
    fn mark_delegated<'a>(
        &'a self,
        agent_id: UserId,
        message_id: MessageId,
    ) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let n=sqlx::query("update message_provenance set provenance='delegated',delegated_by=owner_id where message_id=$1 and agent_id=$2 and provenance in('generated','delegated')").bind(*message_id.as_uuid()).bind(*agent_id.as_uuid()).execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if n == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            Ok(())
        })
    }
    fn approve_message<'a>(&'a self, actor: UserId, message_id: MessageId) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let n=sqlx::query("update message_provenance set provenance='human_approved',approved_by=$1,approved_at=$2 where message_id=$3 and owner_id=$1 and agent_id is not null").bind(*actor.as_uuid()).bind(Utc::now()).bind(*message_id.as_uuid()).execute(&self.pool).await.map_err(sql_error)?.rows_affected();
            if n == 0 {
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
            let row=sqlx::query("select provenance,agent_id,owner_id,approved_by from message_provenance where message_id=$1").bind(*message_id.as_uuid()).fetch_optional(&self.pool).await.map_err(sql_error)?.ok_or(RepositoryError::NotFound)?;
            let raw: String = row.try_get("provenance").map_err(storage)?;
            Ok(MessageProvenance {
                message_id,
                provenance: ActivityProvenance::parse(&raw)
                    .ok_or_else(|| storage("invalid provenance"))?,
                agent_id: row
                    .try_get::<Option<Uuid>, _>("agent_id")
                    .map_err(storage)?
                    .map(UserId::from_uuid),
                owner_id: row
                    .try_get::<Option<Uuid>, _>("owner_id")
                    .map_err(storage)?
                    .map(UserId::from_uuid),
                approved_by: row
                    .try_get::<Option<Uuid>, _>("approved_by")
                    .map_err(storage)?
                    .map(UserId::from_uuid),
            })
        })
    }
}

fn process_link_from_postgres(
    row: PgRow,
    channel_id: ChannelId,
    actor: UserId,
) -> Result<ProcessLink, RepositoryError> {
    Ok(ProcessLink {
        id: ProcessLinkId::from_uuid(row.try_get("id").map_err(storage)?),
        channel_id,
        heart_instance_id: row.try_get("heart_instance_id").map_err(storage)?,
        namespace: row.try_get("namespace").map_err(storage)?,
        definition_name: row.try_get("definition_name").map_err(storage)?,
        definition_version: row.try_get("definition_version").map_err(storage)?,
        initiated_by: actor,
        status: row.try_get("status").map_err(storage)?,
    })
}

fn outbox_from_postgres(row: &PgRow) -> Result<OutboxJob, RepositoryError> {
    let payload: serde_json::Value = row.try_get("payload").map_err(storage)?;
    Ok(OutboxJob {
        id: OutboxId::from_uuid(row.try_get("id").map_err(storage)?),
        process_link_id: ProcessLinkId::from_uuid(row.try_get("process_link_id").map_err(storage)?),
        operation: serde_json::from_value(payload).map_err(storage)?,
        attempts: u32::try_from(row.try_get::<i32, _>("attempts").map_err(storage)?)
            .map_err(storage)?,
    })
}

async fn ensure_membership(
    pool: &PgPool,
    channel_id: &ChannelId,
    actor: &UserId,
) -> Result<(), RepositoryError> {
    let role: Option<String> = sqlx::query_scalar(
        "select role from channel_memberships where channel_id = $1 and user_id = $2",
    )
    .bind(*channel_id.as_uuid())
    .bind(*actor.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(sql_error)?;
    let role = role.as_deref().and_then(MembershipRole::parse);
    Policy::can_read_channel(role.as_ref())
        .then_some(())
        .ok_or(RepositoryError::PermissionDenied)
}

async fn load_membership(
    pool: &PgPool,
    channel_id: ChannelId,
    actor: UserId,
) -> Result<Membership, RepositoryError> {
    let row = sqlx::query("select role, last_read_sequence from channel_memberships where channel_id = $1 and user_id = $2")
        .bind(*channel_id.as_uuid())
        .bind(*actor.as_uuid())
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

fn channel_summary(row: PgRow) -> Result<ChannelSummary, RepositoryError> {
    let id: uuid::Uuid = row.try_get("id").map_err(storage)?;
    let slug: String = row.try_get("slug").map_err(storage)?;
    let name: String = row.try_get("name").map_err(storage)?;
    let kind: String = row.try_get("kind").map_err(storage)?;
    let role: String = row.try_get("role").map_err(storage)?;
    let circle_id: Option<uuid::Uuid> = row.try_get("circle_id").map_err(storage)?;
    Ok(ChannelSummary {
        id: ChannelId::from_uuid(id),
        slug: ChannelSlug::new(slug).map_err(storage)?,
        name: DisplayName::new(name).map_err(storage)?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| storage("invalid channel kind"))?,
        circle_id: circle_id.map(CircleId::from_uuid),
        role: MembershipRole::parse(&role).ok_or_else(|| storage("invalid membership role"))?,
    })
}

fn chat_message(row: PgRow) -> Result<ChatMessage, RepositoryError> {
    let id: uuid::Uuid = row.try_get("id").map_err(storage)?;
    let channel_id: uuid::Uuid = row.try_get("channel_id").map_err(storage)?;
    let sender_id: uuid::Uuid = row.try_get("sender_id").map_err(storage)?;
    let sequence: i64 = row.try_get("sequence").map_err(storage)?;
    let body: String = row.try_get("body").map_err(storage)?;
    let sent_at = row.try_get("created_at").map_err(storage)?;
    Ok(ChatMessage {
        id: MessageId::from_uuid(id),
        channel_id: ChannelId::from_uuid(channel_id),
        sender_id: UserId::from_uuid(sender_id),
        body: MessageBody::new(body).map_err(storage)?,
        sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
        sent_at,
    })
}

fn persisted_now() -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("current UTC timestamp is representable")
}

fn circle_with_role(row: PgRow) -> Result<(Circle, CircleRole), RepositoryError> {
    let role: String = row.try_get("role").map_err(storage)?;
    Ok((
        Circle {
            id: CircleId::from_uuid(row.try_get("id").map_err(storage)?),
            slug: ChannelSlug::new(row.try_get::<String, _>("slug").map_err(storage)?)
                .map_err(storage)?,
            name: DisplayName::new(row.try_get::<String, _>("name").map_err(storage)?)
                .map_err(storage)?,
            created_by: UserId::from_uuid(row.try_get("created_by").map_err(storage)?),
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
    async fn postgres_repository_persists_and_reads_messages() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let repository = std::sync::Arc::new(PostgresChatRepository::connect(&url).await.unwrap());
        repository.migrate().await.unwrap();
        let suffix = uuid::Uuid::now_v7().simple().to_string();
        super::super::verify_repository_contract(repository.as_ref(), &format!("pg-{suffix}"))
            .await;
        let alice = UserId::named(format!("postgres-alice-{suffix}"));
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
                slug: ChannelSlug::new(format!("pg-{suffix}")).unwrap(),
                name: DisplayName::new("PostgreSQL").unwrap(),
                kind: ChannelKind::Private,
                circle_id: None,
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
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                limit: crate::domain::MessageLimit::DEFAULT,
                after: None,
            })
            .await
            .unwrap();
        assert_eq!(loaded, vec![message]);

        let mut writers = tokio::task::JoinSet::new();
        for index in 0..32 {
            let repository = repository.clone();
            let actor = alice.clone();
            let channel_id = channel.id.clone();
            writers.spawn(async move {
                repository
                    .append_message_idempotent(
                        SendMessage {
                            actor,
                            channel_id,
                            body: MessageBody::new(format!("concurrent-{index}")).unwrap(),
                        },
                        format!("concurrent-request-{index}"),
                    )
                    .await
                    .unwrap()
                    .sequence
            });
        }
        let mut sequences = Vec::new();
        while let Some(result) = writers.join_next().await {
            sequences.push(u64::from(result.unwrap()));
        }
        sequences.sort_unstable();
        assert_eq!(sequences, (2_u64..=33).collect::<Vec<_>>());
    }
}
