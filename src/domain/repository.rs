use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    Channel, ChannelId, ChannelRef, ChannelSequence, ChannelSummary, ChatMessage, CreateChannel,
    JoinChannel, LoadRecentMessages, Membership, MembershipRole, MessageId,
    RepositoryError::NotFound, SendMessage, UserId,
};

pub type RepositoryFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RepositoryError>> + Send + 'a>>;

pub trait ChatRepository: Send + Sync + 'static {
    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel>;
    fn join_channel<'a>(&'a self, command: JoinChannel) -> RepositoryFuture<'a, Membership>;
    fn list_channels_for_user<'a>(
        &'a self,
        actor: UserId,
    ) -> RepositoryFuture<'a, Vec<ChannelSummary>>;
    fn load_recent_messages<'a>(
        &'a self,
        query: LoadRecentMessages,
    ) -> RepositoryFuture<'a, Vec<ChatMessage>>;
    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepositoryError {
    Conflict,
    NotFound,
    PermissionDenied,
    Storage(String),
}

#[derive(Clone, Default)]
pub struct InMemoryChatRepository {
    state: Arc<Mutex<RepositoryState>>,
}

impl ChatRepository for InMemoryChatRepository {
    fn create_channel<'a>(&'a self, command: CreateChannel) -> RepositoryFuture<'a, Channel> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            if state.channels_by_slug.contains_key(&command.slug) {
                return Err(RepositoryError::Conflict);
            }

            state.next_channel_id += 1;
            let channel = Channel {
                id: ChannelId::new(format!("channel-{}", state.next_channel_id))
                    .map_err(|error| RepositoryError::Storage(error.to_string()))?,
                slug: command.slug,
                name: command.name,
                kind: command.kind,
                created_by: command.actor.clone(),
            };

            state
                .channels_by_slug
                .insert(channel.slug.clone(), channel.id.clone());
            state.channels.insert(channel.id.clone(), channel.clone());
            state.memberships.insert(
                (channel.id.clone(), command.actor.clone()),
                Membership {
                    channel_id: channel.id.clone(),
                    user_id: command.actor,
                    role: MembershipRole::Owner,
                    last_read_message_id: None,
                },
            );

            Ok(channel)
        })
    }

    fn join_channel<'a>(&'a self, command: JoinChannel) -> RepositoryFuture<'a, Membership> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            let channel_id = state.resolve_channel_ref(&command.channel)?;
            let membership = Membership {
                channel_id: channel_id.clone(),
                user_id: command.actor.clone(),
                role: MembershipRole::Member,
                last_read_message_id: None,
            };
            state
                .memberships
                .insert((channel_id, command.actor), membership.clone());
            Ok(membership)
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
                        role: membership.role.clone(),
                    })
                })
                .collect::<Vec<_>>();
            channels.sort_by(|left, right| left.slug.to_string().cmp(&right.slug.to_string()));
            Ok(channels)
        })
    }

    fn load_recent_messages<'a>(
        &'a self,
        query: LoadRecentMessages,
    ) -> RepositoryFuture<'a, Vec<ChatMessage>> {
        Box::pin(async move {
            let state = self.lock_state()?;
            if !state
                .memberships
                .contains_key(&(query.channel_id.clone(), query.actor))
            {
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
            let keep_from = messages.len().saturating_sub(limit);
            Ok(messages.split_off(keep_from))
        })
    }

    fn append_message<'a>(&'a self, command: SendMessage) -> RepositoryFuture<'a, ChatMessage> {
        Box::pin(async move {
            let mut state = self.lock_state()?;
            if !state
                .memberships
                .contains_key(&(command.channel_id.clone(), command.actor.clone()))
            {
                return Err(RepositoryError::PermissionDenied);
            }

            state.next_message_id += 1;
            let next_sequence = state.next_sequence(&command.channel_id);
            let message = ChatMessage {
                id: MessageId::new(state.next_message_id),
                channel_id: command.channel_id.clone(),
                sender_id: command.actor,
                body: command.body,
                sequence: next_sequence,
                sent_at_unix_ms: unix_time_ms(),
            };

            state
                .messages
                .entry(command.channel_id)
                .or_default()
                .push(message.clone());
            Ok(message)
        })
    }
}

impl InMemoryChatRepository {
    fn lock_state(&self) -> Result<std::sync::MutexGuard<'_, RepositoryState>, RepositoryError> {
        self.state
            .lock()
            .map_err(|error| RepositoryError::Storage(error.to_string()))
    }
}

#[derive(Default)]
struct RepositoryState {
    channels: HashMap<ChannelId, Channel>,
    channels_by_slug: HashMap<super::ChannelSlug, ChannelId>,
    memberships: HashMap<(ChannelId, UserId), Membership>,
    messages: HashMap<ChannelId, Vec<ChatMessage>>,
    next_channel_id: u64,
    next_message_id: u64,
    next_sequences: HashMap<ChannelId, ChannelSequence>,
    _known_users: HashSet<UserId>,
}

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

    fn next_sequence(&mut self, channel_id: &ChannelId) -> ChannelSequence {
        let sequence = self
            .next_sequences
            .get(channel_id)
            .map_or(ChannelSequence::first(), |sequence| sequence.next());
        self.next_sequences.insert(channel_id.clone(), sequence);
        sequence
    }
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ChannelKind, ChannelSlug, DisplayName, MessageBody};

    #[tokio::test]
    async fn in_memory_repository_lists_joined_channels_and_appends_messages() {
        let repository = InMemoryChatRepository::default();
        let alice = UserId::new("alice").unwrap();
        let channel = repository
            .create_channel(CreateChannel {
                actor: alice.clone(),
                slug: ChannelSlug::new("general").unwrap(),
                name: DisplayName::new("General").unwrap(),
                kind: ChannelKind::Public,
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
}
