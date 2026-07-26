#[cfg(test)]
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::{fmt, future::Future, pin::Pin, time::Duration as StdDuration};

use super::{
    AcceptCircleInvitation, Channel, ChannelId, ChannelSequence, ChannelSummary, ChatEvent,
    ChatMessage, Circle, CircleMembership, CircleRole, CreateChannel, CreateCircle,
    CreateCircleInvitation, DeleteCircle, InboxMention, IssuedInvitation, JoinChannel,
    LeaveChannel, LoadRecentMessages, MarkRead, Membership, MessageId, PortableUserExport,
    SendMessage, User, UserId, UserTask,
};
#[cfg(test)]
use super::{
    ChannelRef, ChannelSlug, CircleId, CircleInvitation, ExportedChannel, ExportedCircle,
    InvitationId, MembershipRole, PORTABLE_USER_EXPORT_FORMAT, Policy, RepositoryError::NotFound,
};
#[cfg(test)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
#[cfg(test)]
use chrono::{Duration, Utc};
#[cfg(test)]
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PresenceLease {
    pub channel_id: ChannelId,
    pub participant_id: UserId,
    pub connection_id: Uuid,
}

pub type RepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

pub trait ChatRepository: Send + Sync + 'static {
    fn health_check(&self) -> RepositoryFuture<'_, ()>;
    fn upsert_user<'a>(&'a self, user: User) -> RepositoryFuture<'a, User>;
    fn list_human_users<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, Vec<User>>;
    fn open_direct_channel<'a>(
        &'a self,
        actor: UserId,
        other: UserId,
    ) -> RepositoryFuture<'a, Channel>;
    fn export_user_data<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, PortableUserExport>;
    fn create_circle<'a>(&'a self, command: CreateCircle) -> RepositoryFuture<'a, Circle>;
    fn list_circles_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<(Circle, CircleRole)>>;
    fn delete_circle<'a>(&'a self, command: DeleteCircle) -> RepositoryFuture<'a, ()>;
    fn create_circle_invitation<'a>(
        &'a self,
        command: CreateCircleInvitation,
    ) -> RepositoryFuture<'a, IssuedInvitation>;
    fn accept_circle_invitation<'a>(
        &'a self,
        command: AcceptCircleInvitation,
    ) -> RepositoryFuture<'a, CircleMembership>;
    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel>;
    fn join_channel<'a>(&'a self, command: JoinChannel) -> RepositoryFuture<'a, Membership>;
    fn leave_channel<'a>(&'a self, command: LeaveChannel) -> RepositoryFuture<'a, ()>;
    fn list_channels_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<ChannelSummary>>;
    fn load_recent_messages<'a>(
        &'a self,
        query: LoadRecentMessages,
    ) -> RepositoryFuture<'a, Vec<ChatMessage>>;
    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage>;
    fn append_message_idempotent<'a>(
        &'a self,
        command: SendMessage,
        request_id: String,
    ) -> RepositoryFuture<'a, ChatMessage>;
    fn load_message<'a>(&'a self, id: MessageId) -> RepositoryFuture<'a, ChatMessage>;
    fn latest_sequence<'a>(
        &'a self,
        channel_id: ChannelId,
    ) -> RepositoryFuture<'a, ChannelSequence>;
    fn mark_read<'a>(&'a self, command: MarkRead) -> RepositoryFuture<'a, Membership>;
    fn list_mentions<'a>(&'a self, _actor: UserId) -> RepositoryFuture<'a, Vec<InboxMention>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn mark_mention_read<'a>(
        &'a self,
        _actor: UserId,
        _message_id: MessageId,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn create_task<'a>(
        &'a self,
        _actor: UserId,
        _source_message_id: MessageId,
        _assignee_id: UserId,
        _title: String,
        _process_link_id: Option<uuid::Uuid>,
    ) -> RepositoryFuture<'a, UserTask> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn list_tasks<'a>(&'a self, _actor: UserId) -> RepositoryFuture<'a, Vec<UserTask>> {
        Box::pin(async { Ok(Vec::new()) })
    }
    fn set_task_done<'a>(
        &'a self,
        _actor: UserId,
        _task_id: uuid::Uuid,
        _done: bool,
    ) -> RepositoryFuture<'a, UserTask> {
        Box::pin(async { Err(RepositoryError::NotFound) })
    }
    fn subscribe_messages(&self) -> Option<broadcast::Receiver<MessageId>> {
        None
    }
    fn subscribe_presence(&self) -> Option<broadcast::Receiver<ChatEvent>> {
        None
    }
    fn register_presence<'a>(
        &'a self,
        _lease: PresenceLease,
        _ttl: StdDuration,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn renew_presence<'a>(
        &'a self,
        _leases: Vec<PresenceLease>,
        _ttl: StdDuration,
    ) -> RepositoryFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn unregister_presence<'a>(&'a self, _lease: PresenceLease) -> RepositoryFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
    fn expire_presence(&self) -> RepositoryFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    Conflict,
    NotFound,
    PermissionDenied,
    Storage(String),
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => formatter.write_str("repository conflict"),
            Self::NotFound => formatter.write_str("repository resource not found"),
            Self::PermissionDenied => formatter.write_str("repository permission denied"),
            Self::Storage(message) => write!(formatter, "repository storage error: {message}"),
        }
    }
}

impl RepositoryError {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Conflict => "conflict",
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::Storage(_) => "storage",
        }
    }

    pub const fn public_message(&self) -> &'static str {
        match self {
            Self::Conflict => "resource conflict",
            Self::NotFound => "resource not found",
            Self::PermissionDenied => "permission denied",
            Self::Storage(_) => "internal storage error",
        }
    }
}

impl std::error::Error for RepositoryError {}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct InMemoryChatRepository {
    state: Arc<Mutex<RepositoryState>>,
}

#[cfg(test)]
impl ChatRepository for InMemoryChatRepository {
    fn health_check(&self) -> RepositoryFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn upsert_user<'a>(&'a self, user: User) -> RepositoryFuture<'a, User> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            state.users.insert(user.id.clone(), user.clone());
            let general_channel = state
                .channels
                .values()
                .find(|channel| channel.slug.as_str() == "general")
                .map(|channel| channel.id.clone());
            if let Some(channel_id) = general_channel {
                state
                    .memberships
                    .entry((channel_id.clone(), user.id.clone()))
                    .or_insert(Membership {
                        channel_id,
                        user_id: user.id.clone(),
                        role: MembershipRole::Member,
                        last_read_sequence: ChannelSequence::new(0),
                    });
            }
            Ok(user)
        })
    }

    fn list_human_users<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, Vec<User>> {
        Box::pin(async move {
            let state = self.lock_state()?;
            if !state.users.contains_key(&actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            let mut users = state
                .users
                .values()
                .filter(|user| user.kind == crate::domain::PrincipalKind::Human)
                .cloned()
                .collect::<Vec<_>>();
            users
                .sort_by(|left, right| left.display_name.as_str().cmp(right.display_name.as_str()));
            Ok(users)
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
            let mut state = self.lock_state()?;
            let other_user = state
                .users
                .get(&other)
                .cloned()
                .ok_or(RepositoryError::NotFound)?;
            if !state.users.contains_key(&actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            let pair = if actor < other {
                (actor.clone(), other.clone())
            } else {
                (other.clone(), actor.clone())
            };
            if let Some(channel_id) = state.direct_conversations.get(&pair) {
                return state
                    .channels
                    .get(channel_id)
                    .cloned()
                    .ok_or_else(|| RepositoryError::Storage("direct channel is missing".into()));
            }
            let channel = Channel {
                id: ChannelId::generate(),
                slug: ChannelSlug::new(format!("dm-{}", uuid::Uuid::new_v4().simple()))
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
                name: other_user.display_name,
                kind: crate::domain::ChannelKind::Private,
                circle_id: None,
                created_by: actor.clone(),
            };
            state
                .channels_by_slug
                .insert(channel.slug.clone(), channel.id.clone());
            state.channels.insert(channel.id.clone(), channel.clone());
            state.direct_conversations.insert(pair, channel.id.clone());
            for (user_id, role) in [
                (actor, MembershipRole::Owner),
                (other, MembershipRole::Member),
            ] {
                state.memberships.insert(
                    (channel.id.clone(), user_id.clone()),
                    Membership {
                        channel_id: channel.id.clone(),
                        user_id,
                        role,
                        last_read_sequence: ChannelSequence::new(0),
                    },
                );
            }
            Ok(channel)
        })
    }

    fn export_user_data<'a>(&'a self, actor: UserId) -> RepositoryFuture<'a, PortableUserExport> {
        Box::pin(async move {
            let state = self.lock_state()?;
            let user = state
                .users
                .get(&actor)
                .cloned()
                .ok_or(RepositoryError::NotFound)?;
            let mut circles = state
                .circle_memberships
                .values()
                .filter(|membership| membership.user_id == actor)
                .filter_map(|membership| {
                    state
                        .circles
                        .get(&membership.circle_id)
                        .cloned()
                        .map(|circle| ExportedCircle {
                            circle,
                            role: membership.role.clone(),
                        })
                })
                .collect::<Vec<_>>();
            circles.sort_by(|left, right| left.circle.slug.cmp(&right.circle.slug));

            let mut channels = state
                .memberships
                .values()
                .filter(|membership| membership.user_id == actor)
                .filter_map(|membership| {
                    let channel = state.channels.get(&membership.channel_id)?;
                    let messages = state.messages.get(&channel.id).cloned().unwrap_or_default();
                    Some(ExportedChannel {
                        channel: ChannelSummary {
                            id: channel.id.clone(),
                            slug: channel.slug.clone(),
                            name: channel.name.clone(),
                            kind: channel.kind.clone(),
                            circle_id: channel.circle_id.clone(),
                            role: membership.role.clone(),
                            last_read_sequence: membership.last_read_sequence,
                            latest_sequence: messages
                                .last()
                                .map_or(ChannelSequence::new(0), |message| message.sequence),
                        },
                        messages,
                    })
                })
                .collect::<Vec<_>>();
            channels.sort_by(|left, right| left.channel.slug.cmp(&right.channel.slug));
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
            let mut state = self.lock_state()?;
            if !state.users.contains_key(&command.actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            if state.circles_by_slug.contains_key(&command.slug) {
                return Err(RepositoryError::Conflict);
            }
            let circle = Circle {
                id: CircleId::generate(),
                slug: command.slug,
                name: command.name,
                created_by: command.actor.clone(),
                created_at: Utc::now(),
            };
            state
                .circles_by_slug
                .insert(circle.slug.clone(), circle.id.clone());
            state.circles.insert(circle.id.clone(), circle.clone());
            state.circle_memberships.insert(
                (circle.id.clone(), command.actor.clone()),
                CircleMembership {
                    circle_id: circle.id.clone(),
                    user_id: command.actor,
                    role: CircleRole::Owner,
                    joined_at: circle.created_at,
                },
            );
            Ok(circle)
        })
    }

    fn list_circles_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<(Circle, CircleRole)>> {
        Box::pin(async move {
            let state = self.lock_state()?;
            let mut circles = state
                .circle_memberships
                .values()
                .filter(|membership| membership.user_id == actor)
                .filter_map(|membership| {
                    state
                        .circles
                        .get(&membership.circle_id)
                        .cloned()
                        .map(|circle| (circle, membership.role.clone()))
                })
                .collect::<Vec<_>>();
            circles.sort_by(|left, right| left.0.slug.cmp(&right.0.slug));
            Ok(circles)
        })
    }

    fn delete_circle<'a>(&'a self, command: DeleteCircle) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let role = state
                .circle_memberships
                .get(&(command.circle_id.clone(), command.actor))
                .map(|membership| &membership.role);
            if !Policy::can_delete_circle(role) {
                return Err(RepositoryError::PermissionDenied);
            }
            let circle = state
                .circles
                .remove(&command.circle_id)
                .ok_or(RepositoryError::NotFound)?;
            state.circles_by_slug.remove(&circle.slug);
            state
                .circle_memberships
                .retain(|(circle_id, _), _| circle_id != &command.circle_id);
            state
                .circle_invitations
                .retain(|_, invitation| invitation.circle_id != command.circle_id);

            let channel_ids = state
                .channels
                .values()
                .filter(|channel| channel.circle_id.as_ref() == Some(&command.circle_id))
                .map(|channel| channel.id.clone())
                .collect::<Vec<_>>();
            let message_ids = channel_ids
                .iter()
                .flat_map(|channel_id| state.messages.get(channel_id).into_iter().flatten())
                .map(|message| message.id)
                .collect::<std::collections::HashSet<_>>();
            for channel_id in &channel_ids {
                if let Some(channel) = state.channels.remove(channel_id) {
                    state.channels_by_slug.remove(&channel.slug);
                }
                state
                    .memberships
                    .retain(|(membership_channel_id, _), _| membership_channel_id != channel_id);
                state.messages.remove(channel_id);
                state.next_sequences.remove(channel_id);
            }
            state
                .command_receipts
                .retain(|_, message_id| !message_ids.contains(message_id));
            Ok(())
        })
    }

    fn create_circle_invitation<'a>(
        &'a self,
        command: CreateCircleInvitation,
    ) -> RepositoryFuture<'a, IssuedInvitation> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let role = state
                .circle_memberships
                .get(&(command.circle_id.clone(), command.actor.clone()))
                .map(|membership| &membership.role);
            if !Policy::can_invite_to_circle(role) {
                return Err(RepositoryError::PermissionDenied);
            }
            let mut secret = [0_u8; 32];
            getrandom::fill(&mut secret)
                .map_err(|error| RepositoryError::Storage(error.to_string()))?;
            let token = URL_SAFE_NO_PAD.encode(secret);
            let token_hash = Sha256::digest(token.as_bytes()).to_vec();
            let invitation = CircleInvitation {
                id: InvitationId::generate(),
                circle_id: command.circle_id,
                invited_by: command.actor,
                expires_at: Utc::now() + Duration::days(7),
            };
            state
                .circle_invitations
                .insert(token_hash, invitation.clone());
            Ok(IssuedInvitation { invitation, token })
        })
    }

    fn accept_circle_invitation<'a>(
        &'a self,
        command: AcceptCircleInvitation,
    ) -> RepositoryFuture<'a, CircleMembership> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            if !state.users.contains_key(&command.actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            let token_hash = Sha256::digest(command.token.as_bytes()).to_vec();
            let invitation = state
                .circle_invitations
                .remove(&token_hash)
                .ok_or(RepositoryError::NotFound)?;
            if invitation.expires_at < Utc::now() {
                return Err(RepositoryError::NotFound);
            }
            let membership = CircleMembership {
                circle_id: invitation.circle_id.clone(),
                user_id: command.actor.clone(),
                role: CircleRole::Member,
                joined_at: Utc::now(),
            };
            state.circle_memberships.insert(
                (invitation.circle_id.clone(), command.actor.clone()),
                membership.clone(),
            );
            let channel_ids = state
                .channels
                .values()
                .filter(|channel| channel.circle_id.as_ref() == Some(&invitation.circle_id))
                .map(|channel| channel.id.clone())
                .collect::<Vec<_>>();
            for channel_id in channel_ids {
                state
                    .memberships
                    .entry((channel_id.clone(), command.actor.clone()))
                    .or_insert(Membership {
                        channel_id,
                        user_id: command.actor.clone(),
                        role: MembershipRole::Member,
                        last_read_sequence: ChannelSequence::new(0),
                    });
            }
            Ok(membership)
        })
    }

    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            if !state.users.contains_key(&command.actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            if state.channels_by_slug.contains_key(&command.slug) {
                return Err(RepositoryError::Conflict);
            }
            if let Some(circle_id) = &command.circle_id {
                let role = state
                    .circle_memberships
                    .get(&(circle_id.clone(), command.actor.clone()))
                    .map(|membership| &membership.role);
                if !Policy::can_create_channel_in_circle(role) {
                    return Err(RepositoryError::PermissionDenied);
                }
            }

            let circle_members = command
                .circle_id
                .as_ref()
                .map_or_else(Vec::new, |circle_id| {
                    state
                        .circle_memberships
                        .values()
                        .filter(|membership| &membership.circle_id == circle_id)
                        .map(|membership| {
                            (
                                membership.user_id.clone(),
                                if membership.role == CircleRole::Owner {
                                    MembershipRole::Owner
                                } else {
                                    MembershipRole::Member
                                },
                            )
                        })
                        .collect::<Vec<_>>()
                });
            let is_general = command.slug.as_str() == "general" && command.circle_id.is_none();
            let general_users = if is_general {
                state.users.keys().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let channel = Channel {
                id: ChannelId::generate(),
                slug: command.slug,
                name: command.name,
                kind: command.kind,
                circle_id: command.circle_id,
                created_by: command.actor.clone(),
            };

            state
                .channels_by_slug
                .insert(channel.slug.clone(), channel.id.clone());
            state.channels.insert(channel.id.clone(), channel.clone());
            let mut members = circle_members;
            members.extend(
                general_users
                    .into_iter()
                    .map(|user_id| (user_id, MembershipRole::Member)),
            );
            members.push((command.actor.clone(), MembershipRole::Owner));
            for (user_id, role) in members {
                let key = (channel.id.clone(), user_id.clone());
                let membership = Membership {
                    channel_id: channel.id.clone(),
                    user_id,
                    role,
                    last_read_sequence: ChannelSequence::new(0),
                };
                match state.memberships.entry(key) {
                    std::collections::hash_map::Entry::Occupied(mut entry)
                        if membership.role == MembershipRole::Owner =>
                    {
                        entry.insert(membership);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {}
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(membership);
                    }
                }
            }

            Ok(channel)
        })
    }

    fn join_channel<'a>(&'a self, command: JoinChannel) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            if !state.users.contains_key(&command.actor) {
                return Err(RepositoryError::PermissionDenied);
            }
            let channel_id = state.resolve_channel_ref(&command.channel)?;
            let channel = state
                .channels
                .get(&channel_id)
                .ok_or(RepositoryError::NotFound)?;
            if channel.circle_id.as_ref().is_some_and(|circle_id| {
                !state
                    .circle_memberships
                    .contains_key(&(circle_id.clone(), command.actor.clone()))
            }) {
                return Err(RepositoryError::PermissionDenied);
            }
            let membership = Membership {
                channel_id: channel_id.clone(),
                user_id: command.actor.clone(),
                role: MembershipRole::Member,
                last_read_sequence: ChannelSequence::new(0),
            };
            state
                .memberships
                .insert((channel_id, command.actor), membership.clone());
            Ok(membership)
        })
    }

    fn leave_channel<'a>(&'a self, command: LeaveChannel) -> RepositoryFuture<'a, ()> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let key = (command.channel_id, command.actor);
            let role = state
                .memberships
                .get(&key)
                .map(|membership| &membership.role);
            if !Policy::can_leave_channel(role) {
                return Err(RepositoryError::NotFound);
            }
            if state.memberships.remove(&key).is_none() {
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
            let state = self.lock_state()?;
            let mut channels = state
                .memberships
                .values()
                .filter(|membership| membership.user_id == actor)
                .filter_map(|membership| {
                    let channel = state.channels.get(&membership.channel_id)?;
                    Some(ChannelSummary {
                        id: channel.id.clone(),
                        slug: channel.slug.clone(),
                        name: channel.name.clone(),
                        kind: channel.kind.clone(),
                        circle_id: channel.circle_id.clone(),
                        role: membership.role.clone(),
                        last_read_sequence: membership.last_read_sequence,
                        latest_sequence: state
                            .messages
                            .get(&channel.id)
                            .and_then(|messages| messages.last())
                            .map_or(ChannelSequence::new(0), |message| message.sequence),
                    })
                })
                .collect::<Vec<_>>();
            channels.sort_by(|left, right| left.slug.cmp(&right.slug));
            Ok(channels)
        })
    }

    fn load_recent_messages<'a>(
        &'a self,
        query: LoadRecentMessages,
    ) -> RepositoryFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            let state = self.lock_state()?;
            let role = state
                .memberships
                .get(&(query.channel_id.clone(), query.actor))
                .map(|membership| &membership.role);
            if !Policy::can_read_channel(role) {
                return Err(RepositoryError::PermissionDenied);
            }

            let limit = usize::from(query.limit);
            let mut messages = state
                .messages
                .get(&query.channel_id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|message| query.after.is_none_or(|after| message.sequence > after))
                .collect::<Vec<_>>();
            if query.after.is_some() {
                messages.truncate(limit);
                Ok(messages)
            } else {
                let keep_from = messages.len().saturating_sub(limit);
                Ok(messages.split_off(keep_from))
            }
        })
    }

    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let role = state
                .memberships
                .get(&(command.channel_id.clone(), command.actor.clone()))
                .map(|membership| &membership.role);
            if !Policy::can_send_to_channel(role) {
                return Err(RepositoryError::PermissionDenied);
            }

            let next_sequence = state.next_sequence(&command.channel_id)?;
            let sender_display_name = state
                .users
                .get(&command.actor)
                .map(|user| user.display_name.clone())
                .ok_or(RepositoryError::NotFound)?;
            let message = ChatMessage {
                id: MessageId::generate(),
                channel_id: command.channel_id.clone(),
                sender_id: command.actor,
                sender_display_name,
                body: command.body,
                sequence: next_sequence,
                sent_at: persisted_now(),
            };

            state
                .messages
                .entry(command.channel_id)
                .or_default()
                .push(message.clone());
            Ok(message)
        })
    }

    fn append_message_idempotent<'a>(
        &'a self,
        command: SendMessage,
        request_id: String,
    ) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let key = (command.actor.clone(), request_id);
            if let Some(message_id) = state.command_receipts.get(&key) {
                let message = state
                    .messages
                    .values()
                    .flatten()
                    .find(|message| &message.id == message_id)
                    .cloned()
                    .ok_or(RepositoryError::NotFound)?;
                if message.channel_id != command.channel_id || message.body != command.body {
                    return Err(RepositoryError::Conflict);
                }
                return Ok(message);
            }
            let role = state
                .memberships
                .get(&(command.channel_id.clone(), command.actor.clone()))
                .map(|membership| &membership.role);
            if !Policy::can_send_to_channel(role) {
                return Err(RepositoryError::PermissionDenied);
            }
            let next_sequence = state.next_sequence(&command.channel_id)?;
            let sender_display_name = state
                .users
                .get(&command.actor)
                .map(|user| user.display_name.clone())
                .ok_or(RepositoryError::NotFound)?;
            let message = ChatMessage {
                id: MessageId::generate(),
                channel_id: command.channel_id.clone(),
                sender_id: command.actor,
                sender_display_name,
                body: command.body,
                sequence: next_sequence,
                sent_at: persisted_now(),
            };
            state.command_receipts.insert(key, message.id);
            state
                .messages
                .entry(command.channel_id)
                .or_default()
                .push(message.clone());
            Ok(message)
        })
    }

    fn load_message<'a>(&'a self, id: MessageId) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let state = self.lock_state()?;
            state
                .messages
                .values()
                .flatten()
                .find(|message| message.id == id)
                .cloned()
                .ok_or(RepositoryError::NotFound)
        })
    }

    fn latest_sequence<'a>(
        &'a self,
        channel_id: ChannelId,
    ) -> RepositoryFuture<'a, ChannelSequence> {
        Box::pin(async move {
            let state = self.lock_state()?;
            Ok(state
                .messages
                .get(&channel_id)
                .and_then(|messages| messages.last())
                .map_or(ChannelSequence::new(0), |message| message.sequence))
        })
    }

    fn mark_read<'a>(&'a self, command: MarkRead) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let latest_sequence = state
                .messages
                .get(&command.channel_id)
                .and_then(|messages| messages.last())
                .map_or(ChannelSequence::new(0), |message| message.sequence);
            if command.sequence > latest_sequence {
                return Err(RepositoryError::NotFound);
            }

            let membership = state
                .memberships
                .get_mut(&(command.channel_id, command.actor))
                .ok_or(RepositoryError::PermissionDenied)?;
            if command.sequence > membership.last_read_sequence {
                membership.last_read_sequence = command.sequence;
            }
            Ok(membership.clone())
        })
    }
}

#[cfg(test)]
impl InMemoryChatRepository {
    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, RepositoryState>, RepositoryError> {
        self.state
            .lock()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}

#[cfg(test)]
#[derive(Default)]
struct RepositoryState {
    circles: HashMap<CircleId, Circle>,
    circles_by_slug: HashMap<super::ChannelSlug, CircleId>,
    circle_memberships: HashMap<(CircleId, UserId), CircleMembership>,
    circle_invitations: HashMap<Vec<u8>, CircleInvitation>,
    channels: HashMap<ChannelId, Channel>,
    channels_by_slug: HashMap<super::ChannelSlug, ChannelId>,
    direct_conversations: HashMap<(UserId, UserId), ChannelId>,
    memberships: HashMap<(ChannelId, UserId), Membership>,
    messages: HashMap<ChannelId, Vec<ChatMessage>>,
    next_sequences: HashMap<ChannelId, ChannelSequence>,
    users: HashMap<UserId, User>,
    command_receipts: HashMap<(UserId, String), MessageId>,
}

#[cfg(test)]
impl RepositoryState {
    fn resolve_channel_ref(&self, channel: &ChannelRef) -> Result<ChannelId, RepositoryError> {
        match channel {
            ChannelRef::Id(channel_id) if self.channels.contains_key(channel_id) => {
                Ok(channel_id.clone())
            }
            ChannelRef::Slug(slug) => self.channels_by_slug.get(slug).cloned().ok_or(NotFound),
            ChannelRef::Id(_) => Err(NotFound),
        }
    }

    fn next_sequence(
        &mut self,
        channel_id: &ChannelId,
    ) -> Result<ChannelSequence, RepositoryError> {
        let sequence = self
            .next_sequences
            .get(channel_id)
            .map_or(Ok(ChannelSequence::first()), |sequence| {
                sequence.checked_next()
            })
            .map_err(|error| RepositoryError::Storage(error.to_string()))?;
        self.next_sequences.insert(channel_id.clone(), sequence);
        Ok(sequence)
    }
}

#[cfg(test)]
fn persisted_now() -> chrono::DateTime<Utc> {
    chrono::DateTime::from_timestamp_micros(Utc::now().timestamp_micros())
        .expect("current UTC timestamp is representable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_passes_shared_chat_repository_contract() {
        crate::db::verify_chat_repository_contract(&InMemoryChatRepository::default(), "in-memory")
            .await;
    }
    use crate::domain::{ChannelKind, ChannelSlug, DisplayName, MessageBody, PrincipalKind};
    use chrono::Utc;

    async fn add_human(repository: &InMemoryChatRepository, name: &str) -> UserId {
        let id = UserId::named(name);
        repository
            .upsert_user(User {
                id: id.clone(),
                kind: PrincipalKind::Human,
                display_name: DisplayName::new(name).unwrap(),
                external_provider: None,
                external_subject: None,
                created_at: Utc::now(),
            })
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn in_memory_repository_lists_joined_channels_and_appends_messages() {
        let repository = InMemoryChatRepository::default();
        let alice = add_human(&repository, "alice").await;
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("general").unwrap(),
                name: DisplayName::new("General").unwrap(),
                kind: ChannelKind::Public,
                circle_id: None,
            })
            .await
            .unwrap();

        let channels = repository
            .list_channels_for_user(alice.clone())
            .await
            .unwrap();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0].id, channel.id);

        let message = repository
            .append_message(SendMessage {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("Persistent nok snart").unwrap(),
            })
            .await
            .unwrap();

        assert_eq!(u64::from(message.sequence), 1);

        let messages = repository
            .load_recent_messages(LoadRecentMessages {
                actor: alice,
                channel_id: channel.id,
                limit: super::super::MessageLimit::DEFAULT,
                after: None,
            })
            .await
            .unwrap();
        assert_eq!(messages, vec![message]);
    }

    #[tokio::test]
    async fn read_markers_are_monotonic_and_cannot_pass_latest_message() {
        let repository = InMemoryChatRepository::default();
        let alice = add_human(&repository, "alice").await;
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("general").unwrap(),
                name: DisplayName::new("General").unwrap(),
                kind: ChannelKind::Private,
                circle_id: None,
            })
            .await
            .unwrap();
        let message = repository
            .append_message(SendMessage {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                body: MessageBody::new("Hei").unwrap(),
            })
            .await
            .unwrap();

        let membership = repository
            .mark_read(MarkRead {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                sequence: message.sequence,
            })
            .await
            .unwrap();
        assert_eq!(membership.last_read_sequence, message.sequence);

        let behind = repository
            .mark_read(MarkRead {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
                sequence: ChannelSequence::new(0),
            })
            .await
            .unwrap();
        assert_eq!(behind.last_read_sequence, message.sequence);

        let ahead = repository
            .mark_read(MarkRead {
                actor: alice,
                channel_id: channel.id,
                sequence: message.sequence.checked_next().unwrap(),
            })
            .await;
        assert_eq!(ahead, Err(RepositoryError::NotFound));
    }

    #[tokio::test]
    async fn leaving_removes_access() {
        let repository = InMemoryChatRepository::default();
        let alice = add_human(&repository, "alice").await;
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("private").unwrap(),
                name: DisplayName::new("Private").unwrap(),
                kind: ChannelKind::Private,
                circle_id: None,
            })
            .await
            .unwrap();

        repository
            .leave_channel(LeaveChannel {
                actor: alice.clone(),
                channel_id: channel.id.clone(),
            })
            .await
            .unwrap();

        let result = repository
            .append_message(SendMessage {
                actor: alice,
                channel_id: channel.id,
                body: MessageBody::new("should fail").unwrap(),
            })
            .await;
        assert_eq!(result, Err(RepositoryError::PermissionDenied));
    }

    #[tokio::test]
    async fn repeated_request_id_returns_original_message() {
        let repository = InMemoryChatRepository::default();
        let alice = add_human(&repository, "idempotent-alice").await;
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("idempotent").unwrap(),
                name: DisplayName::new("Idempotent").unwrap(),
                kind: ChannelKind::Private,
                circle_id: None,
            })
            .await
            .unwrap();
        let first = repository
            .append_message_idempotent(
                SendMessage {
                    actor: alice.clone(),
                    channel_id: channel.id.clone(),
                    body: MessageBody::new("first").unwrap(),
                },
                "request-1".to_owned(),
            )
            .await
            .unwrap();
        let repeated = repository
            .append_message_idempotent(
                SendMessage {
                    actor: alice.clone(),
                    channel_id: channel.id.clone(),
                    body: MessageBody::new("first").unwrap(),
                },
                "request-1".to_owned(),
            )
            .await
            .unwrap();
        assert_eq!(first, repeated);
        let mismatch = repository
            .append_message_idempotent(
                SendMessage {
                    actor: alice,
                    channel_id: channel.id.clone(),
                    body: MessageBody::new("must not replace").unwrap(),
                },
                "request-1".to_owned(),
            )
            .await;
        assert_eq!(mismatch, Err(RepositoryError::Conflict));
        assert_eq!(
            repository.latest_sequence(channel.id).await.unwrap(),
            ChannelSequence::first()
        );
    }

    #[tokio::test]
    async fn circle_invitation_is_single_use_and_grants_membership() {
        let repository = InMemoryChatRepository::default();
        let alice = add_human(&repository, "circle-alice").await;
        let bob = add_human(&repository, "circle-bob").await;
        let circle = repository
            .create_circle(CreateCircle {
                actor: alice.clone(),
                slug: ChannelSlug::new("friends").unwrap(),
                name: DisplayName::new("Friends").unwrap(),
            })
            .await
            .unwrap();
        let issued = repository
            .create_circle_invitation(CreateCircleInvitation {
                actor: alice,
                circle_id: circle.id.clone(),
            })
            .await
            .unwrap();
        let membership = repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: bob.clone(),
                token: issued.token.clone(),
            })
            .await
            .unwrap();
        assert_eq!(membership.role, CircleRole::Member);
        assert_eq!(membership.circle_id, circle.id);
        let reused = repository
            .accept_circle_invitation(AcceptCircleInvitation {
                actor: bob.clone(),
                token: issued.token,
            })
            .await;
        assert_eq!(reused, Err(RepositoryError::NotFound));
        let channel = repository
            .create_channel(CreateChannel {
                actor: bob,
                slug: ChannelSlug::new("friends-general").unwrap(),
                name: DisplayName::new("General").unwrap(),
                kind: ChannelKind::Private,
                circle_id: Some(circle.id.clone()),
            })
            .await
            .unwrap();
        assert_eq!(channel.circle_id, Some(circle.id));
    }
}
