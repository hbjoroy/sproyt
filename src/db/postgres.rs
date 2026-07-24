use std::collections::HashSet;

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
    ChannelSlug, ChannelSummary, ChatEvent, ChatMessage, ChatRepository, Circle, CircleId,
    CircleInvitation, CircleMembership, CircleRole, CreateChannel, CreateCircle,
    CreateCircleInvitation, DeleteCircle, DisplayName, ExportedChannel, ExportedCircle,
    InboxMention, InvitationId, IssuedInvitation, JoinChannel, LeaveChannel, LoadRecentMessages,
    MarkRead, Membership, MembershipRole, MessageBody, MessageId, PORTABLE_USER_EXPORT_FORMAT,
    Policy, PortableUserExport, PresenceLease, RepositoryError, RepositoryFuture, SendMessage,
    User, UserId, UserTask,
};
use crate::process::{
    EnqueueCorrelation, EnqueueInspection, EnqueueProcessStart, OutboxId, OutboxJob,
    OutboxOperation, ProcessError, ProcessEvent, ProcessLink, ProcessLinkId, ProcessRepository,
    ProcessRepositoryFuture, ProcessView, SetCircleFeature, StartProcess, StartedProcess,
};

use super::{sql_error, storage};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/postgres");

#[derive(Clone)]
pub struct PostgresChatRepository {
    pool: PgPool,
    messages: broadcast::Sender<MessageId>,
    presence: broadcast::Sender<ChatEvent>,
}

impl PostgresChatRepository {
    pub async fn connect(url: &str) -> Result<Self, RepositoryError> {
        let pool = PgPool::connect(url).await.map_err(sql_error)?;
        let mut listener = PgListener::connect(url).await.map_err(sql_error)?;
        listener
            .listen("sproyt_messages")
            .await
            .map_err(sql_error)?;
        listener
            .listen("sproyt_presence")
            .await
            .map_err(sql_error)?;
        let (messages, _) = broadcast::channel(1024);
        let (presence, _) = broadcast::channel(1024);
        let message_publisher = messages.clone();
        let presence_publisher = presence.clone();
        let listener_url = url.to_owned();
        tokio::spawn(async move {
            loop {
                match listener.recv().await {
                    Ok(notification) if notification.channel() == "sproyt_messages" => {
                        match uuid::Uuid::parse_str(notification.payload()) {
                            Ok(id) => {
                                let _ = message_publisher.send(MessageId::from_uuid(id));
                            }
                            Err(_) => tracing::warn!(
                                error_kind = "invalid_uuid",
                                "ignored invalid sproyt_messages notification"
                            ),
                        }
                    }
                    Ok(notification) if notification.channel() == "sproyt_presence" => {
                        match serde_json::from_str::<ChatEvent>(notification.payload()) {
                            Ok(event @ ChatEvent::ParticipantJoined { .. })
                            | Ok(event @ ChatEvent::ParticipantLeft { .. }) => {
                                let _ = presence_publisher.send(event);
                            }
                            _ => tracing::warn!(
                                error_kind = "invalid_presence",
                                "ignored invalid sproyt_presence notification"
                            ),
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        tracing::warn!(
                            error = %error,
                            error_kind = "database_listener",
                            "PostgreSQL realtime listener disconnected; reconnecting"
                        );
                        loop {
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            match PgListener::connect(&listener_url).await {
                                Ok(mut replacement) => {
                                    let messages_ready =
                                        replacement.listen("sproyt_messages").await;
                                    let presence_ready =
                                        replacement.listen("sproyt_presence").await;
                                    if messages_ready.is_ok() && presence_ready.is_ok() {
                                        listener = replacement;
                                        tracing::info!(
                                            error_kind = "database_listener_recovered",
                                            "PostgreSQL realtime listener reconnected"
                                        );
                                        break;
                                    }
                                }
                                Err(error) => tracing::warn!(
                                    error = %error,
                                    error_kind = "database_listener_reconnect",
                                    "PostgreSQL realtime listener reconnect failed"
                                ),
                            }
                        }
                    }
                }
            }
        });
        Ok(Self {
            pool,
            messages,
            presence,
        })
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
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("insert into users (id, kind, display_name, external_provider, external_subject, created_at) values ($1, $2, $3, $4, $5, $6) on conflict(id) do update set kind = excluded.kind, display_name = excluded.display_name, external_provider = excluded.external_provider, external_subject = excluded.external_subject")
                .bind(*user.id.as_uuid())
                .bind(user.kind.as_str())
                .bind(user.display_name.as_str())
                .bind(&user.external_provider)
                .bind(&user.external_subject)
                .bind(user.created_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) select id, $1, 'member' from channels where slug = 'general' and circle_id is null on conflict(channel_id, user_id) do nothing")
                .bind(*user.id.as_uuid())
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            Ok(user)
        })
    }

    fn list_human_users<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, Vec<User>> {
        Box::pin(async move {
            let exists = sqlx::query_scalar::<_, i32>("select 1 from users where id=$1")
                .bind(*actor.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
            if exists.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
            let rows = sqlx::query("select id, kind, display_name, external_provider, external_subject, created_at from users where kind = 'human' order by lower(display_name), id")
                .fetch_all(&self.pool)
                .await
                .map_err(sql_error)?;
            rows.into_iter().map(user_from_row).collect()
        })
    }

    fn open_direct_channel<'a>(
        &'a self,
        actor: UserId,
        other: UserId,
    ) -> RepositoryFuture<'a, Channel> {
        Box::pin(async move {
            if actor == other {
                return Err(RepositoryError::Conflict);
            }
            let (user_a, user_b) = if actor < other {
                (actor.clone(), other.clone())
            } else {
                (other.clone(), actor.clone())
            };
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            if let Some(channel_id) = sqlx::query_scalar::<_, uuid::Uuid>(
                "select channel_id from direct_conversations where user_a_id=$1 and user_b_id=$2",
            )
            .bind(*user_a.as_uuid())
            .bind(*user_b.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?
            {
                let row = sqlx::query(
                    "select id,slug,name,kind,circle_id,created_by from channels where id=$1",
                )
                .bind(channel_id)
                .fetch_one(&mut *tx)
                .await
                .map_err(sql_error)?;
                return channel_from_row(row);
            }
            let names = sqlx::query("select display_name from users where id=any($1) and kind='human' order by lower(display_name)")
                .bind(vec![*actor.as_uuid(), *other.as_uuid()])
                .fetch_all(&mut *tx).await.map_err(sql_error)?;
            if names.len() != 2 {
                return Err(RepositoryError::NotFound);
            }
            let channel = Channel {
                id: ChannelId::generate(),
                slug: ChannelSlug::new(format!("dm-{}", uuid::Uuid::new_v4().simple()))
                    .map_err(storage)?,
                name: DisplayName::new(format!(
                    "{} ↔ {}",
                    names[0]
                        .try_get::<String, _>("display_name")
                        .map_err(storage)?,
                    names[1]
                        .try_get::<String, _>("display_name")
                        .map_err(storage)?
                ))
                .map_err(storage)?,
                kind: ChannelKind::Private,
                circle_id: None,
                created_by: actor.clone(),
            };
            sqlx::query("insert into channels(id,slug,name,kind,circle_id,created_by) values($1,$2,$3,'private',null,$4)")
                .bind(*channel.id.as_uuid()).bind(channel.slug.as_str()).bind(channel.name.as_str()).bind(*actor.as_uuid())
                .execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into channel_sequences(channel_id) values($1)")
                .bind(*channel.id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
            sqlx::query(
                "insert into direct_conversations(channel_id,user_a_id,user_b_id) values($1,$2,$3)",
            )
            .bind(*channel.id.as_uuid())
            .bind(*user_a.as_uuid())
            .bind(*user_b.as_uuid())
            .execute(&mut *tx)
            .await
            .map_err(sql_error)?;
            sqlx::query("insert into channel_memberships(channel_id,user_id,role) values($1,$2,'member'),($1,$3,'member')")
                .bind(*channel.id.as_uuid()).bind(*actor.as_uuid()).bind(*other.as_uuid())
                .execute(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            Ok(channel)
        })
    }

    fn export_user_data<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, PortableUserExport> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("set transaction isolation level repeatable read read only")
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
            let row = sqlx::query("select id, kind, display_name, external_provider, external_subject, created_at from users where id=$1")
                .bind(*actor.as_uuid()).fetch_optional(&mut *tx).await.map_err(sql_error)?
                .ok_or(RepositoryError::NotFound)?;
            let kind: String = row.try_get("kind").map_err(storage)?;
            let user = User {
                id: UserId::from_uuid(row.try_get("id").map_err(storage)?),
                kind: crate::domain::PrincipalKind::parse(&kind)
                    .ok_or_else(|| storage("invalid principal kind"))?,
                display_name: DisplayName::new(
                    row.try_get::<String, _>("display_name").map_err(storage)?,
                )
                .map_err(storage)?,
                external_provider: row.try_get("external_provider").map_err(storage)?,
                external_subject: row.try_get("external_subject").map_err(storage)?,
                created_at: row.try_get("created_at").map_err(storage)?,
            };
            let circle_rows = sqlx::query("select c.id,c.slug,c.name,c.created_by,c.created_at,m.role from circles c join circle_memberships m on m.circle_id=c.id where m.user_id=$1 order by c.slug")
                .bind(*actor.as_uuid()).fetch_all(&mut *tx).await.map_err(sql_error)?;
            let circles = circle_rows
                .into_iter()
                .map(circle_with_role)
                .map(|result| result.map(|(circle, role)| ExportedCircle { circle, role }))
                .collect::<Result<Vec<_>, _>>()?;
            let channel_rows = sqlx::query("select c.id,c.slug,c.name,c.kind,c.circle_id,m.role,m.last_read_sequence,coalesce((select max(sequence) from messages where channel_id=c.id),0) as latest_sequence from channels c join channel_memberships m on m.channel_id=c.id where m.user_id=$1 order by c.slug")
                .bind(*actor.as_uuid()).fetch_all(&mut *tx).await.map_err(sql_error)?;
            let summaries = channel_rows
                .into_iter()
                .map(channel_summary)
                .collect::<Result<Vec<_>, _>>()?;
            let mut channels = Vec::with_capacity(summaries.len());
            for channel in summaries {
                let rows = sqlx::query("select id,channel_id,sender_id,sender_display_name,sequence,body,created_at from messages where channel_id=$1 order by sequence")
                    .bind(*channel.id.as_uuid()).fetch_all(&mut *tx).await.map_err(sql_error)?;
                channels.push(ExportedChannel {
                    channel,
                    messages: rows
                        .into_iter()
                        .map(chat_message)
                        .collect::<Result<Vec<_>, _>>()?,
                });
            }
            tx.commit().await.map_err(sql_error)?;
            Ok(PortableUserExport {
                format: PORTABLE_USER_EXPORT_FORMAT.to_owned(),
                exported_at: Utc::now(),
                user,
                circles,
                channels,
            })
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

    fn delete_circle<'a>(&'a self, command: DeleteCircle) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let role: Option<String> = sqlx::query_scalar(
                "select m.role from circles c join circle_memberships m on m.circle_id=c.id where c.id=$1 and m.user_id=$2 for update of c",
            )
            .bind(*command.circle_id.as_uuid())
            .bind(*command.actor.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(sql_error)?;
            let role = role.as_deref().and_then(CircleRole::parse);
            if !Policy::can_delete_circle(role.as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            sqlx::query(
                "insert into audit_events(actor_id,action,target_kind,target_id,payload) values ($1,'circle.deleted','circle',$2,jsonb_build_object('deletion','owner_requested'))",
            )
            .bind(*command.actor.as_uuid())
            .bind(command.circle_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(sql_error)?;
            let result = sqlx::query("delete from circles where id=$1")
                .bind(*command.circle_id.as_uuid())
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
            if result.rows_affected() != 1 {
                return Err(RepositoryError::NotFound);
            }
            tx.commit().await.map_err(sql_error)
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
            sqlx::query("insert into channel_memberships (channel_id,user_id,role) select id,$1,'member' from channels where circle_id=$2 on conflict(channel_id,user_id) do nothing")
                .bind(*command.actor.as_uuid())
                .bind(circle_uuid)
                .execute(&mut *tx)
                .await
                .map_err(sql_error)?;
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
            if let Some(circle_id) = &channel.circle_id {
                sqlx::query("insert into channel_memberships (channel_id,user_id,role) select $1,user_id,case role when 'owner' then 'owner' else 'member' end from circle_memberships where circle_id=$2 on conflict(channel_id,user_id) do nothing")
                    .bind(*channel.id.as_uuid())
                    .bind(*circle_id.as_uuid())
                    .execute(&mut *transaction)
                    .await
                    .map_err(sql_error)?;
            } else if channel.slug.as_str() == "general" {
                sqlx::query("insert into channel_memberships (channel_id,user_id,role) select $1,id,case when id=$2 then 'owner' else 'member' end from users on conflict(channel_id,user_id) do nothing")
                    .bind(*channel.id.as_uuid())
                    .bind(*channel.created_by.as_uuid())
                    .execute(&mut *transaction)
                    .await
                    .map_err(sql_error)?;
            }
            sqlx::query("insert into channel_memberships (channel_id, user_id, role) values ($1, $2, 'owner') on conflict(channel_id,user_id) do update set role='owner'")
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
            let exists: Option<i32> = sqlx::query_scalar("select 1 from channels where id=$1")
                .bind(*channel_id.as_uuid())
                .fetch_optional(&self.pool)
                .await
                .map_err(sql_error)?;
            if exists.is_none() {
                return Err(RepositoryError::NotFound);
            }
            let allowed: Option<i32> = sqlx::query_scalar("select 1 from channels c where c.id=$1 and (c.circle_id is null or exists(select 1 from circle_memberships cm where cm.circle_id=c.circle_id and cm.user_id=$2))")
                .bind(*channel_id.as_uuid()).bind(*command.actor.as_uuid())
                .fetch_optional(&self.pool).await.map_err(sql_error)?;
            if allowed.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
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
            let rows = sqlx::query("select c.id, c.slug, c.name, c.kind, c.circle_id, m.role, m.last_read_sequence, coalesce((select max(sequence) from messages where channel_id=c.id),0) as latest_sequence from channels c join channel_memberships m on m.channel_id = c.id where m.user_id = $1 order by c.slug")
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
            let limit = i64::try_from(usize::from(query.limit)).map_err(storage)?;
            let (rows, reverse) = if let Some(after) = query.after {
                let after = i64::try_from(u64::from(after)).map_err(storage)?;
                (sqlx::query("select id, channel_id, sender_id, sender_display_name, sequence, body, created_at from messages where channel_id = $1 and sequence > $2 order by sequence asc limit $3")
                    .bind(*query.channel_id.as_uuid()).bind(after).bind(limit)
                    .fetch_all(&self.pool).await.map_err(sql_error)?, false)
            } else {
                (sqlx::query("select id, channel_id, sender_id, sender_display_name, sequence, body, created_at from messages where channel_id = $1 order by sequence desc limit $2")
                    .bind(*query.channel_id.as_uuid()).bind(limit)
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
            let sender_display_name = DisplayName::new(
                sqlx::query_scalar::<_, String>("select display_name from users where id = $1")
                    .bind(*command.actor.as_uuid())
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(sql_error)?,
            )
            .map_err(storage)?;
            let message = ChatMessage {
                id: MessageId::generate(),
                channel_id: command.channel_id,
                sender_id: command.actor,
                sender_display_name,
                body: command.body,
                sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
                sent_at: persisted_now(),
            };
            sqlx::query("insert into messages (id, channel_id, sender_id, sender_display_name, sequence, body, created_at) values ($1, $2, $3, $4, $5, $6, $7)")
                .bind(*message.id.as_uuid())
                .bind(*message.channel_id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(message.sender_display_name.as_str())
                .bind(sequence)
                .bind(message.body.as_str())
                .bind(message.sent_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            persist_mentions_postgres(&mut transaction, &message).await?;
            transaction.commit().await.map_err(sql_error)?;
            if sqlx::query("select pg_notify('sproyt_messages', $1)")
                .bind(message.id.as_uuid().to_string())
                .execute(&self.pool)
                .await
                .is_err()
            {
                tracing::warn!(error_kind = "database_notify", message_id = %message.id.as_uuid(), "message persisted but realtime notification failed");
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
                let row = sqlx::query("select m.id, m.channel_id, m.sender_id, m.sender_display_name, m.sequence, m.body, m.created_at from command_receipts r join messages m on m.id = r.message_id where r.principal_id = $1 and r.request_id = $2")
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
            let sender_display_name = DisplayName::new(
                sqlx::query_scalar::<_, String>("select display_name from users where id = $1")
                    .bind(*command.actor.as_uuid())
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(sql_error)?,
            )
            .map_err(storage)?;
            let message = ChatMessage {
                id: MessageId::generate(),
                channel_id: command.channel_id,
                sender_id: command.actor,
                sender_display_name,
                body: command.body,
                sequence: ChannelSequence::try_from(sequence).map_err(storage)?,
                sent_at: persisted_now(),
            };
            sqlx::query("insert into messages (id, channel_id, sender_id, sender_display_name, sequence, body, created_at) values ($1, $2, $3, $4, $5, $6, $7)")
                .bind(*message.id.as_uuid())
                .bind(*message.channel_id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(message.sender_display_name.as_str())
                .bind(sequence)
                .bind(message.body.as_str())
                .bind(message.sent_at)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            persist_mentions_postgres(&mut transaction, &message).await?;
            sqlx::query("update command_receipts set message_id = $1 where principal_id = $2 and request_id = $3")
                .bind(*message.id.as_uuid())
                .bind(*message.sender_id.as_uuid())
                .bind(&request_id)
                .execute(&mut *transaction)
                .await
                .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)?;
            if sqlx::query("select pg_notify('sproyt_messages', $1)")
                .bind(message.id.as_uuid().to_string())
                .execute(&self.pool)
                .await
                .is_err()
            {
                tracing::warn!(error_kind = "database_notify", message_id = %message.id.as_uuid(), "message persisted but realtime notification failed");
            }
            Ok(message)
        })
    }

    fn load_message<'a>(&'a self, id: MessageId) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let row = sqlx::query("select id, channel_id, sender_id, sender_display_name, sequence, body, created_at from messages where id = $1")
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

    fn list_mentions<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, Vec<InboxMention>> {
        Box::pin(async move {
            let rows = sqlx::query("select m.id,m.channel_id,m.sender_id,m.sender_display_name,m.sequence,m.body,m.created_at,c.name as channel_name,mm.read_at from message_mentions mm join messages m on m.id=mm.message_id join channels c on c.id=m.channel_id join channel_memberships cm on cm.channel_id=m.channel_id and cm.user_id=mm.mentioned_user_id where mm.mentioned_user_id=$1 order by m.created_at desc limit 200")
                .bind(*actor.as_uuid()).fetch_all(&self.pool).await.map_err(sql_error)?;
            rows.into_iter().map(inbox_mention).collect()
        })
    }

    fn mark_mention_read<'a>(
        &'a self,
        actor: UserId,
        message_id: MessageId,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let result = sqlx::query("update message_mentions set read_at=coalesce(read_at,now()) where message_id=$1 and mentioned_user_id=$2")
                .bind(*message_id.as_uuid()).bind(*actor.as_uuid())
                .execute(&self.pool).await.map_err(sql_error)?;
            if result.rows_affected() == 0 {
                return Err(RepositoryError::NotFound);
            }
            Ok(())
        })
    }

    fn create_task<'a>(
        &'a self,
        actor: UserId,
        source_message_id: MessageId,
        assignee_id: UserId,
        title: String,
        process_link_id: Option<uuid::Uuid>,
    ) -> RepositoryFuture<'a, UserTask> {
        Box::pin(async move {
            let id = uuid::Uuid::now_v7();
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            let allowed: Option<i32> = sqlx::query_scalar("select 1 from messages m join channel_memberships cm on cm.channel_id=m.channel_id and cm.user_id=$1 join message_mentions mm on mm.message_id=m.id and mm.mentioned_user_id=$2 where m.id=$3")
                .bind(*actor.as_uuid()).bind(*assignee_id.as_uuid()).bind(*source_message_id.as_uuid())
                .fetch_optional(&mut *tx).await.map_err(sql_error)?;
            if allowed.is_none() {
                return Err(RepositoryError::PermissionDenied);
            }
            if let Some(process_id) = process_link_id {
                let linked: Option<i32> = sqlx::query_scalar("select 1 from process_links p join messages m on m.channel_id=p.channel_id where p.id=$1 and m.id=$2")
                    .bind(process_id).bind(*source_message_id.as_uuid())
                    .fetch_optional(&mut *tx).await.map_err(sql_error)?;
                if linked.is_none() {
                    return Err(RepositoryError::PermissionDenied);
                }
            }
            sqlx::query("insert into user_tasks(id,source_message_id,assignee_id,created_by,process_link_id,title) values($1,$2,$3,$4,$5,$6)")
                .bind(id).bind(*source_message_id.as_uuid()).bind(*assignee_id.as_uuid())
                .bind(*actor.as_uuid()).bind(process_link_id).bind(title)
                .execute(&mut *tx).await.map_err(sql_error)?;
            let row = sqlx::query("select t.*,m.channel_id,c.name as channel_name from user_tasks t join messages m on m.id=t.source_message_id join channels c on c.id=m.channel_id where t.id=$1")
                .bind(id).fetch_one(&mut *tx).await.map_err(sql_error)?;
            tx.commit().await.map_err(sql_error)?;
            user_task(row)
        })
    }

    fn list_tasks<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, Vec<UserTask>> {
        Box::pin(async move {
            let rows = sqlx::query("select t.*,m.channel_id,c.name as channel_name from user_tasks t join messages m on m.id=t.source_message_id join channels c on c.id=m.channel_id where t.assignee_id=$1 order by case t.status when 'open' then 0 else 1 end,t.created_at desc")
                .bind(*actor.as_uuid()).fetch_all(&self.pool).await.map_err(sql_error)?;
            rows.into_iter().map(user_task).collect()
        })
    }

    fn set_task_done<'a>(
        &'a self,
        actor: UserId,
        task_id: uuid::Uuid,
        done: bool,
    ) -> RepositoryFuture<'a, UserTask> {
        Box::pin(async move {
            let row = sqlx::query("update user_tasks set status=$1,completed_at=case when $2 then now() else null end where id=$3 and assignee_id=$4 returning id")
                .bind(if done { "done" } else { "open" }).bind(done).bind(task_id).bind(*actor.as_uuid())
                .fetch_optional(&self.pool).await.map_err(sql_error)?
                .ok_or(RepositoryError::NotFound)?;
            let row = sqlx::query("select t.*,m.channel_id,c.name as channel_name from user_tasks t join messages m on m.id=t.source_message_id join channels c on c.id=m.channel_id where t.id=$1")
                .bind(row.try_get::<uuid::Uuid,_>("id").map_err(storage)?).fetch_one(&self.pool).await.map_err(sql_error)?;
            user_task(row)
        })
    }

    fn subscribe_messages(&self) -> Option<broadcast::Receiver<MessageId>> {
        Some(self.messages.subscribe())
    }

    fn subscribe_presence(&self) -> Option<broadcast::Receiver<ChatEvent>> {
        Some(self.presence.subscribe())
    }

    fn register_presence<'a>(
        &'a self,
        lease: PresenceLease,
        ttl: std::time::Duration,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let expires_at = Utc::now() + Duration::from_std(ttl).map_err(storage)?;
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            lock_presence(&mut tx, &lease.channel_id, &lease.participant_id).await?;
            sqlx::query("delete from presence_leases where channel_id=$1 and user_id=$2 and expires_at <= now()")
                .bind(*lease.channel_id.as_uuid()).bind(*lease.participant_id.as_uuid())
                .execute(&mut *tx).await.map_err(sql_error)?;
            let was_present: bool = sqlx::query_scalar("select exists(select 1 from presence_leases where channel_id=$1 and user_id=$2 and expires_at > now())")
                .bind(*lease.channel_id.as_uuid()).bind(*lease.participant_id.as_uuid())
                .fetch_one(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into presence_leases(channel_id,user_id,connection_id,expires_at) values($1,$2,$3,$4) on conflict(connection_id) do update set expires_at=excluded.expires_at")
                .bind(*lease.channel_id.as_uuid()).bind(*lease.participant_id.as_uuid())
                .bind(lease.connection_id).bind(expires_at)
                .execute(&mut *tx).await.map_err(sql_error)?;
            if !was_present {
                notify_presence(
                    &mut tx,
                    ChatEvent::ParticipantJoined {
                        channel_id: lease.channel_id,
                        participant_id: lease.participant_id,
                    },
                )
                .await?;
            }
            tx.commit().await.map_err(sql_error)
        })
    }

    fn renew_presence<'a>(
        &'a self,
        leases: Vec<PresenceLease>,
        ttl: std::time::Duration,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let expires_at = Utc::now() + Duration::from_std(ttl).map_err(storage)?;
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            for lease in leases {
                sqlx::query("update presence_leases set expires_at=$1 where connection_id=$2 and channel_id=$3 and user_id=$4")
                    .bind(expires_at).bind(lease.connection_id)
                    .bind(*lease.channel_id.as_uuid()).bind(*lease.participant_id.as_uuid())
                    .execute(&mut *tx).await.map_err(sql_error)?;
            }
            tx.commit().await.map_err(sql_error)
        })
    }

    fn unregister_presence<'a>(&'a self, lease: PresenceLease) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            lock_presence(&mut tx, &lease.channel_id, &lease.participant_id).await?;
            let removed = sqlx::query("delete from presence_leases where connection_id=$1 and channel_id=$2 and user_id=$3")
                .bind(lease.connection_id).bind(*lease.channel_id.as_uuid())
                .bind(*lease.participant_id.as_uuid())
                .execute(&mut *tx).await.map_err(sql_error)?.rows_affected() > 0;
            let remains: bool = sqlx::query_scalar("select exists(select 1 from presence_leases where channel_id=$1 and user_id=$2 and expires_at > now())")
                .bind(*lease.channel_id.as_uuid()).bind(*lease.participant_id.as_uuid())
                .fetch_one(&mut *tx).await.map_err(sql_error)?;
            if removed && !remains {
                notify_presence(
                    &mut tx,
                    ChatEvent::ParticipantLeft {
                        channel_id: lease.channel_id,
                        participant_id: lease.participant_id,
                    },
                )
                .await?;
            }
            tx.commit().await.map_err(sql_error)
        })
    }

    fn expire_presence(&self) -> RepositoryFuture<'_, ()> {
        Box::pin(async move {
            let expired: Vec<(Uuid, Uuid)> = sqlx::query_as(
                "select distinct channel_id,user_id from presence_leases where expires_at <= now() limit 1000"
            ).fetch_all(&self.pool).await.map_err(sql_error)?;
            let mut seen = HashSet::new();
            for (channel_uuid, user_uuid) in expired {
                if !seen.insert((channel_uuid, user_uuid)) {
                    continue;
                }
                let channel_id = ChannelId::from_uuid(channel_uuid);
                let participant_id = UserId::from_uuid(user_uuid);
                let mut tx = self.pool.begin().await.map_err(sql_error)?;
                lock_presence(&mut tx, &channel_id, &participant_id).await?;
                let removed = sqlx::query("delete from presence_leases where channel_id=$1 and user_id=$2 and expires_at <= now()")
                    .bind(channel_uuid).bind(user_uuid).execute(&mut *tx).await.map_err(sql_error)?.rows_affected() > 0;
                let remains: bool = sqlx::query_scalar("select exists(select 1 from presence_leases where channel_id=$1 and user_id=$2 and expires_at > now())")
                    .bind(channel_uuid).bind(user_uuid).fetch_one(&mut *tx).await.map_err(sql_error)?;
                if removed && !remains {
                    notify_presence(
                        &mut tx,
                        ChatEvent::ParticipantLeft {
                            channel_id,
                            participant_id,
                        },
                    )
                    .await?;
                }
                tx.commit().await.map_err(sql_error)?;
            }
            Ok(())
        })
    }
}

async fn lock_presence(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    channel_id: &ChannelId,
    participant_id: &UserId,
) -> Result<(), RepositoryError> {
    sqlx::query("select pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2::text, 0))")
        .bind(*channel_id.as_uuid())
        .bind(*participant_id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(sql_error)?;
    Ok(())
}

async fn notify_presence(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: ChatEvent,
) -> Result<(), RepositoryError> {
    let payload = serde_json::to_string(&event).map_err(storage)?;
    sqlx::query("select pg_notify('sproyt_presence', $1)")
        .bind(payload)
        .execute(&mut **tx)
        .await
        .map_err(sql_error)?;
    Ok(())
}

impl ProcessRepository for PostgresChatRepository {
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
                 and f.feature='heart.event-planning' and f.enabled \
                 where m.channel_id=$1 and m.user_id=$2",
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

    fn get_process<'a>(
        &'a self,
        actor: UserId,
        process_link_id: ProcessLinkId,
    ) -> ProcessRepositoryFuture<'a, ProcessView> {
        Box::pin(async move {
            let row = sqlx::query("select p.id,p.channel_id,p.heart_instance_id,p.namespace,p.definition_name,p.definition_version,p.initiated_by,p.status,m.role from process_links p join channel_memberships m on m.channel_id=p.channel_id and m.user_id=$1 where p.id=$2")
                .bind(*actor.as_uuid()).bind(process_link_id.as_uuid())
                .fetch_optional(&self.pool).await.map_err(sql_error)?
                .ok_or(RepositoryError::PermissionDenied)?;
            let role: String = row.try_get("role").map_err(storage)?;
            if !Policy::can_read_channel(MembershipRole::parse(&role).as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let process = ProcessLink {
                id: ProcessLinkId::from_uuid(row.try_get("id").map_err(storage)?),
                channel_id: ChannelId::from_uuid(row.try_get("channel_id").map_err(storage)?),
                heart_instance_id: row.try_get("heart_instance_id").map_err(storage)?,
                namespace: row.try_get("namespace").map_err(storage)?,
                definition_name: row.try_get("definition_name").map_err(storage)?,
                definition_version: row.try_get("definition_version").map_err(storage)?,
                initiated_by: UserId::from_uuid(row.try_get("initiated_by").map_err(storage)?),
                status: row.try_get("status").map_err(storage)?,
            };
            let rows = sqlx::query("select id,event_type,payload,actor_id,occurred_at from process_events where process_link_id=$1 order by occurred_at,id")
                .bind(process_link_id.as_uuid()).fetch_all(&self.pool).await.map_err(sql_error)?;
            let events = rows
                .into_iter()
                .map(|row| {
                    Ok(ProcessEvent {
                        id: row.try_get("id").map_err(storage)?,
                        event_type: row.try_get("event_type").map_err(storage)?,
                        payload: row.try_get("payload").map_err(storage)?,
                        actor_id: UserId::from_uuid(row.try_get("actor_id").map_err(storage)?),
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
            if let Some(existing) = sqlx::query_scalar::<_, Uuid>("select outbox_id from process_command_receipts where actor_id=$1 and request_id=$2")
                .bind(*command.actor.as_uuid()).bind(&command.request_id).fetch_optional(&mut *tx).await.map_err(sql_error)? {
                tx.commit().await.map_err(sql_error)?;
                return Ok(OutboxId::from_uuid(existing));
            }
            let access: Option<(Uuid, String)> = sqlx::query_as("select p.heart_instance_id,m.role from process_links p join channels c on c.id=p.channel_id join channel_memberships m on m.channel_id=c.id and m.user_id=$1 join circle_features f on f.circle_id=c.circle_id and f.feature='heart.event-planning' and f.enabled where p.id=$2")
                .bind(*command.actor.as_uuid()).bind(command.process_link_id.as_uuid()).fetch_optional(&mut *tx).await.map_err(sql_error)?;
            let (instance_id, role) = access.ok_or(RepositoryError::PermissionDenied)?;
            if !Policy::can_read_channel(MembershipRole::parse(&role).as_ref()) {
                return Err(RepositoryError::PermissionDenied);
            }
            let outbox_id = OutboxId::generate();
            let now = Utc::now();
            let operation = OutboxOperation::Inspect { instance_id };
            sqlx::query("insert into process_outbox(id,process_link_id,operation,payload,available_at,created_at) values($1,$2,'inspect',$3,$4,$4)")
                .bind(outbox_id.as_uuid()).bind(command.process_link_id.as_uuid()).bind(serde_json::to_value(&operation).map_err(storage)?)
                .bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            sqlx::query("insert into process_command_receipts(actor_id,request_id,process_link_id,outbox_id,command_type,created_at) values($1,$2,$3,$4,'inspect',$5)")
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
            sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values($1,$2,'started','process.started',$3,(select initiated_by from process_links where id=$2),$4) on conflict(process_link_id,event_key) do nothing")
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
            sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values($1,$2,$3::text,$4,$5,coalesce((select actor_id from process_command_receipts where outbox_id=$3),(select initiated_by from process_links where id=$2)),$6) on conflict(process_link_id,event_key) do nothing")
                .bind(Uuid::now_v7()).bind(job.process_link_id.as_uuid()).bind(job.id.as_uuid())
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
            let now = Utc::now();
            let available = now + chrono::Duration::from_std(delay).map_err(storage)?;
            let error_text = error.to_string();
            let payload = serde_json::json!({
                "kind":error.kind,
                "message":error.message,
                "retryable":error.retryable,
                "attempts":job.attempts
            });
            let mut tx = self.pool.begin().await.map_err(sql_error)?;
            sqlx::query("update process_outbox set status=$1,available_at=$2,lease_until=null,last_error=$3 where id=$4")
                .bind(if terminal { "failed" } else { "pending" }).bind(available).bind(error_text)
                .bind(job.id.as_uuid()).execute(&mut *tx).await.map_err(sql_error)?;
            if terminal {
                sqlx::query("update process_links set status='failed',updated_at=$1 where id=$2")
                    .bind(now)
                    .bind(job.process_link_id.as_uuid())
                    .execute(&mut *tx)
                    .await
                    .map_err(sql_error)?;
                sqlx::query("insert into process_events(id,process_link_id,event_key,event_type,payload,actor_id,occurred_at) values($1,$2,$3,'process.failed',$4,coalesce((select actor_id from process_command_receipts where outbox_id=$5),(select initiated_by from process_links where id=$2)),$6) on conflict(process_link_id,event_key) do nothing")
                    .bind(Uuid::now_v7()).bind(job.process_link_id.as_uuid()).bind(format!("failed:{}",job.id.as_uuid()))
                    .bind(payload).bind(job.id.as_uuid()).bind(now).execute(&mut *tx).await.map_err(sql_error)?;
            }
            tx.commit().await.map_err(sql_error)
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
            let proposed_id = Uuid::now_v7();
            let id: Uuid = sqlx::query_scalar("insert into agent_grants(id,agent_id,circle_id,channel_id,scope,granted_by,expires_at,created_at) values($1,$2,$3,$4,$5,$6,$7,$8) on conflict(agent_id,circle_id,channel_id,scope) do update set revoked_at=null,revoked_by=null,expires_at=excluded.expires_at,granted_by=excluded.granted_by returning id").bind(proposed_id).bind(*command.agent_id.as_uuid()).bind(command.circle_id.as_ref().map(|v|*v.as_uuid())).bind(command.channel_id.as_ref().map(|v|*v.as_uuid())).bind(command.scope.as_str()).bind(*command.actor.as_uuid()).bind(command.expires_at).bind(Utc::now()).fetch_one(&self.pool).await.map_err(sql_error)?;
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
    fn revoke_agent<'a>(&'a self, actor: UserId, agent_id: UserId) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let now = Utc::now();
            let actor = *actor.as_uuid();
            let agent_id = *agent_id.as_uuid();
            let mut transaction = self.pool.begin().await.map_err(sql_error)?;
            let changed = sqlx::query(
                "update agent_profiles set revoked_at=$1 where agent_id=$2 and owner_id=$3 and revoked_at is null",
            )
            .bind(now)
            .bind(agent_id)
            .bind(actor)
            .execute(&mut *transaction)
            .await
            .map_err(sql_error)?
            .rows_affected();
            if changed == 0 {
                return Err(RepositoryError::PermissionDenied);
            }
            sqlx::query(
                "update agent_credentials set revoked_at=$1 where agent_id=$2 and revoked_at is null",
            )
            .bind(now)
            .bind(agent_id)
            .execute(&mut *transaction)
            .await
            .map_err(sql_error)?;
            sqlx::query(
                "update agent_grants set revoked_at=$1,revoked_by=$2 where agent_id=$3 and revoked_at is null",
            )
            .bind(now)
            .bind(actor)
            .bind(agent_id)
            .execute(&mut *transaction)
            .await
            .map_err(sql_error)?;
            transaction.commit().await.map_err(sql_error)
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
    fn consume_rate_limit<'a>(
        &'a self,
        agent_id: UserId,
        limit_per_minute: u16,
    ) -> AgentFuture<'a, ()> {
        Box::pin(async move {
            let now = Utc::now();
            let cutoff = now - chrono::Duration::seconds(60);
            let consumed: Option<i32> = sqlx::query_scalar(
                "insert into agent_rate_limits(agent_id,window_started_at,request_count) values($1,$2,1) \
                 on conflict(agent_id) do update set \
                 window_started_at=case when agent_rate_limits.window_started_at<=$3 then excluded.window_started_at else agent_rate_limits.window_started_at end, \
                 request_count=case when agent_rate_limits.window_started_at<=$3 then 1 else agent_rate_limits.request_count+1 end \
                 where agent_rate_limits.window_started_at<=$3 or agent_rate_limits.request_count<$4 \
                 returning request_count",
            )
            .bind(*agent_id.as_uuid())
            .bind(now)
            .bind(cutoff)
            .bind(i32::from(limit_per_minute))
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
    let last_read_sequence: i64 = row.try_get("last_read_sequence").map_err(storage)?;
    let latest_sequence: i64 = row.try_get("latest_sequence").map_err(storage)?;
    Ok(ChannelSummary {
        id: ChannelId::from_uuid(id),
        slug: ChannelSlug::new(slug).map_err(storage)?,
        name: DisplayName::new(name).map_err(storage)?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| storage("invalid channel kind"))?,
        circle_id: circle_id.map(CircleId::from_uuid),
        role: MembershipRole::parse(&role).ok_or_else(|| storage("invalid membership role"))?,
        last_read_sequence: ChannelSequence::try_from(last_read_sequence).map_err(storage)?,
        latest_sequence: ChannelSequence::try_from(latest_sequence).map_err(storage)?,
    })
}

fn user_from_row(row: PgRow) -> Result<User, RepositoryError> {
    let kind: String = row.try_get("kind").map_err(storage)?;
    Ok(User {
        id: UserId::from_uuid(row.try_get("id").map_err(storage)?),
        kind: crate::domain::PrincipalKind::parse(&kind)
            .ok_or_else(|| storage("invalid principal kind"))?,
        display_name: DisplayName::new(row.try_get::<String, _>("display_name").map_err(storage)?)
            .map_err(storage)?,
        external_provider: row.try_get("external_provider").map_err(storage)?,
        external_subject: row.try_get("external_subject").map_err(storage)?,
        created_at: row.try_get("created_at").map_err(storage)?,
    })
}

fn channel_from_row(row: PgRow) -> Result<Channel, RepositoryError> {
    let kind: String = row.try_get("kind").map_err(storage)?;
    Ok(Channel {
        id: ChannelId::from_uuid(row.try_get("id").map_err(storage)?),
        slug: ChannelSlug::new(row.try_get::<String, _>("slug").map_err(storage)?)
            .map_err(storage)?,
        name: DisplayName::new(row.try_get::<String, _>("name").map_err(storage)?)
            .map_err(storage)?,
        kind: ChannelKind::parse(&kind).ok_or_else(|| storage("invalid channel kind"))?,
        circle_id: row
            .try_get::<Option<uuid::Uuid>, _>("circle_id")
            .map_err(storage)?
            .map(CircleId::from_uuid),
        created_by: UserId::from_uuid(row.try_get("created_by").map_err(storage)?),
    })
}

async fn persist_mentions_postgres(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    message: &ChatMessage,
) -> Result<(), RepositoryError> {
    let requested = mention_handles(message.body.as_str());
    if requested.is_empty() {
        return Ok(());
    }
    let rows = sqlx::query(
        "select u.id,u.display_name from users u join channel_memberships m on m.user_id=u.id where m.channel_id=$1 and u.kind='human'",
    )
    .bind(*message.channel_id.as_uuid())
    .fetch_all(&mut **transaction)
    .await
    .map_err(sql_error)?;
    for row in rows {
        let display_name: String = row.try_get("display_name").map_err(storage)?;
        if requested.contains(&mention_handle(&display_name)) {
            sqlx::query("insert into message_mentions(message_id,mentioned_user_id) values($1,$2) on conflict(message_id,mentioned_user_id) do nothing")
                .bind(*message.id.as_uuid())
                .bind(row.try_get::<uuid::Uuid, _>("id").map_err(storage)?)
                .execute(&mut **transaction)
                .await
                .map_err(sql_error)?;
        }
    }
    Ok(())
}

fn mention_handles(body: &str) -> std::collections::HashSet<String> {
    body.split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
        .map(mention_handle)
        .filter(|handle| !handle.is_empty())
        .collect()
}

fn mention_handle(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '_' || *character == '-')
        .flat_map(char::to_lowercase)
        .collect()
}

fn inbox_mention(row: PgRow) -> Result<InboxMention, RepositoryError> {
    let read = row
        .try_get::<Option<chrono::DateTime<Utc>>, _>("read_at")
        .map_err(storage)?
        .is_some();
    let channel_name = DisplayName::new(row.try_get::<String, _>("channel_name").map_err(storage)?)
        .map_err(storage)?;
    Ok(InboxMention {
        message: chat_message(row)?,
        channel_name,
        read,
    })
}

fn user_task(row: PgRow) -> Result<UserTask, RepositoryError> {
    Ok(UserTask {
        id: row.try_get("id").map_err(storage)?,
        source_message_id: MessageId::from_uuid(row.try_get("source_message_id").map_err(storage)?),
        channel_id: ChannelId::from_uuid(row.try_get("channel_id").map_err(storage)?),
        channel_name: DisplayName::new(row.try_get::<String, _>("channel_name").map_err(storage)?)
            .map_err(storage)?,
        assignee_id: UserId::from_uuid(row.try_get("assignee_id").map_err(storage)?),
        created_by: UserId::from_uuid(row.try_get("created_by").map_err(storage)?),
        process_link_id: row.try_get("process_link_id").map_err(storage)?,
        title: row.try_get("title").map_err(storage)?,
        status: row.try_get("status").map_err(storage)?,
        created_at: row.try_get("created_at").map_err(storage)?,
        completed_at: row.try_get("completed_at").map_err(storage)?,
    })
}

fn chat_message(row: PgRow) -> Result<ChatMessage, RepositoryError> {
    let id: uuid::Uuid = row.try_get("id").map_err(storage)?;
    let channel_id: uuid::Uuid = row.try_get("channel_id").map_err(storage)?;
    let sender_id: uuid::Uuid = row.try_get("sender_id").map_err(storage)?;
    let sender_display_name: String = row.try_get("sender_display_name").map_err(storage)?;
    let sequence: i64 = row.try_get("sequence").map_err(storage)?;
    let body: String = row.try_get("body").map_err(storage)?;
    let sent_at = row.try_get("created_at").map_err(storage)?;
    Ok(ChatMessage {
        id: MessageId::from_uuid(id),
        channel_id: ChannelId::from_uuid(channel_id),
        sender_id: UserId::from_uuid(sender_id),
        sender_display_name: DisplayName::new(sender_display_name).map_err(storage)?,
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

    async fn receive_presence_for(
        events: &mut broadcast::Receiver<ChatEvent>,
        channel_id: &ChannelId,
        participant_id: &UserId,
    ) -> ChatEvent {
        loop {
            let event = events.recv().await.unwrap();
            let matches = match &event {
                ChatEvent::ParticipantJoined {
                    channel_id: event_channel,
                    participant_id: event_participant,
                }
                | ChatEvent::ParticipantLeft {
                    channel_id: event_channel,
                    participant_id: event_participant,
                } => event_channel == channel_id && event_participant == participant_id,
                _ => false,
            };
            if matches {
                return event;
            }
        }
    }

    #[tokio::test]
    async fn presence_handoff_is_atomic_across_postgres_replicas() {
        let Ok(url) = std::env::var("SPROYT_POSTGRES_TEST_URL") else {
            return;
        };
        let first = PostgresChatRepository::connect(&url).await.unwrap();
        first.migrate().await.unwrap();
        let second = PostgresChatRepository::connect(&url).await.unwrap();
        let mut first_events = first.subscribe_presence().unwrap();
        let mut second_events = second.subscribe_presence().unwrap();
        let suffix = Uuid::now_v7().simple().to_string();
        let user = UserId::named(format!("presence-{suffix}"));
        first
            .upsert_user(User {
                id: user.clone(),
                kind: PrincipalKind::Human,
                display_name: DisplayName::new("Presence user").unwrap(),
                external_provider: None,
                external_subject: None,
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        let channel = first
            .create_channel(CreateChannel {
                actor: user.clone(),
                slug: ChannelSlug::new(format!("presence-{suffix}")).unwrap(),
                name: DisplayName::new("Presence").unwrap(),
                kind: ChannelKind::Private,
                circle_id: None,
            })
            .await
            .unwrap();
        let lease_one = PresenceLease {
            channel_id: channel.id.clone(),
            participant_id: user.clone(),
            connection_id: Uuid::now_v7(),
        };
        let lease_two = PresenceLease {
            connection_id: Uuid::now_v7(),
            ..lease_one.clone()
        };

        first
            .register_presence(lease_one.clone(), std::time::Duration::from_secs(75))
            .await
            .unwrap();
        for events in [&mut first_events, &mut second_events] {
            let event = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                receive_presence_for(events, &channel.id, &user),
            )
            .await
            .expect("replica missed participant_joined");
            assert_eq!(
                event,
                ChatEvent::ParticipantJoined {
                    channel_id: channel.id.clone(),
                    participant_id: user.clone()
                }
            );
        }

        second
            .register_presence(lease_two.clone(), std::time::Duration::from_secs(75))
            .await
            .unwrap();
        first.unregister_presence(lease_one).await.unwrap();
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(200),
                receive_presence_for(&mut first_events, &channel.id, &user),
            )
            .await
            .is_err(),
            "overlapping handoff emitted a false presence transition"
        );

        second.unregister_presence(lease_two).await.unwrap();
        for events in [&mut first_events, &mut second_events] {
            let event = tokio::time::timeout(
                std::time::Duration::from_secs(2),
                receive_presence_for(events, &channel.id, &user),
            )
            .await
            .expect("replica missed participant_left");
            assert_eq!(
                event,
                ChatEvent::ParticipantLeft {
                    channel_id: channel.id.clone(),
                    participant_id: user.clone()
                }
            );
        }
    }

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
        let rows = sqlx::query_as::<
            _,
            (
                String,
                Option<uuid::Uuid>,
                String,
                String,
                serde_json::Value,
            ),
        >(
            "select action, actor_id, target_kind, target_id, payload from audit_events \
             where action in ('agent.created', 'agent.grant_created', \
             'agent.grant_changed', 'agent.grant_revoked', 'agent.revoked', 'process.started', \
             'process.correlation_requested', 'process.inspection_requested', \
             'circle.feature_changed', 'circle.deleted')",
        )
        .fetch_all(&repository.pool)
        .await
        .unwrap();
        for expected in [
            "agent.created",
            "agent.grant_created",
            "agent.grant_changed",
            "agent.grant_revoked",
            "agent.revoked",
            "process.started",
            "process.correlation_requested",
            "process.inspection_requested",
            "circle.feature_changed",
            "circle.deleted",
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
            assert_ne!(
                payload,
                serde_json::json!({}),
                "{action} has no cause payload"
            );
        }
        let feature_changes: i64 = sqlx::query_scalar(
            "select count(*) from audit_events where action='circle.feature_changed'",
        )
        .fetch_one(&repository.pool)
        .await
        .unwrap();
        assert!(feature_changes >= 2, "feature update was not audited");
        let membership_left = sqlx::query_as::<
            _,
            (Option<uuid::Uuid>, String, String, serde_json::Value),
        >(
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
                    actor.is_some()
                        && kind == "channel"
                        && !target.is_empty()
                        && payload != &serde_json::json!({})
                })
        );
        let process_events = sqlx::query_as::<_, (String, Option<uuid::Uuid>)>(
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
        let replica = PostgresChatRepository::connect(&url).await.unwrap();
        let mut replica_events = replica.subscribe_messages().unwrap();
        let message = repository
            .append_message(SendMessage {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("durable").unwrap(),
            })
            .await
            .unwrap();
        let notified =
            tokio::time::timeout(std::time::Duration::from_secs(2), replica_events.recv())
                .await
                .expect("second replica did not receive PostgreSQL notification")
                .unwrap();
        assert_eq!(notified, message.id);
        let loaded = replica
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
