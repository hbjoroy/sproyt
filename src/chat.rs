use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::Arc,
};

use tokio::sync::{broadcast, mpsc, oneshot};
use uuid::Uuid;

#[cfg(test)]
use crate::domain::PrincipalKind;
use crate::domain::{
    AcceptCircleInvitation, ChannelId, ChannelKind, ChannelRef, ChannelSequence, ChannelSlug,
    ChannelSummary, ChatEvent, ChatMessage, ChatRepository, Circle, CircleMembership, CircleRole,
    CreateChannel, CreateCircle, CreateCircleInvitation, DisplayName, IssuedInvitation,
    JoinChannel, LeaveChannel, LoadRecentMessages, MarkRead, Membership, MessageBody, MessageId,
    MessageLimit, RepositoryError, SendMessage, TextValidationError, User, UserId,
};

const MAILBOX_CAPACITY: usize = 1024;
const CHANNEL_EVENT_CAPACITY: usize = 256;
const PUBLISHED_MESSAGE_CACHE: usize = 4096;

#[derive(Clone)]
pub struct ChatEngine {
    mailbox: mpsc::Sender<Command>,
    repository: Arc<dyn ChatRepository>,
}

impl fmt::Debug for ChatEngine {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("ChatEngine").finish_non_exhaustive()
    }
}

impl ChatEngine {
    pub fn start(repository: Arc<dyn ChatRepository>) -> Self {
        let (mailbox, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        if let Some(mut notifications) = repository.subscribe_messages() {
            let notification_mailbox = mailbox.clone();
            tokio::spawn(async move {
                loop {
                    match notifications.recv().await {
                        Ok(message_id) => {
                            if notification_mailbox
                                .send(Command::ExternalMessage { message_id })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "database notification listener lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        tokio::spawn(ChatActor::new(repository.clone()).run(receiver));
        Self {
            mailbox,
            repository,
        }
    }

    pub async fn health_check(&self) -> Result<(), ChatError> {
        self.repository
            .health_check()
            .await
            .map_err(ChatError::from)
    }

    /// Persist the user resolved by the configured authentication provider.
    pub async fn ensure_user(&self, user: User) -> Result<(), ChatError> {
        self.repository.upsert_user(user).await?;
        Ok(())
    }

    /// Create a deterministic development principal for repository tests.
    #[cfg(test)]
    pub async fn prepare_development_user(
        &self,
        participant_id: UserId,
        participant_name: &str,
    ) -> Result<(), ChatError> {
        self.repository
            .upsert_user(User {
                id: participant_id,
                kind: PrincipalKind::Human,
                display_name: DisplayName::new(participant_name)?,
                external_provider: Some("development".to_owned()),
                external_subject: Some(participant_name.to_owned()),
                created_at: chrono::Utc::now(),
            })
            .await?;
        Ok(())
    }

    /// Create a deterministic development session fixture.
    #[cfg(test)]
    pub async fn prepare_development_session(
        &self,
        participant_id: UserId,
        participant_name: &str,
        channel_slug: &str,
    ) -> Result<ChannelId, ChatError> {
        self.prepare_development_user(participant_id.clone(), participant_name)
            .await?;

        let slug = ChannelSlug::new(channel_slug)?;
        match self
            .repository
            .create_channel(CreateChannel {
                actor: participant_id.clone(),
                slug: slug.clone(),
                name: DisplayName::new(channel_slug)?,
                kind: ChannelKind::Private,
                circle_id: None,
            })
            .await
        {
            Ok(channel) => Ok(channel.id),
            Err(RepositoryError::Conflict) => self
                .repository
                .join_channel(JoinChannel {
                    actor: participant_id,
                    channel: ChannelRef::Slug(slug),
                })
                .await
                .map(|membership| membership.channel_id)
                .map_err(ChatError::from),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn create_channel(
        &self,
        actor: UserId,
        slug: ChannelSlug,
        name: DisplayName,
        kind: ChannelKind,
        circle_id: Option<crate::domain::CircleId>,
    ) -> Result<crate::domain::Channel, ChatError> {
        self.repository
            .create_channel(CreateChannel {
                actor,
                slug,
                name,
                kind,
                circle_id,
            })
            .await
            .map_err(ChatError::from)
    }

    pub async fn create_circle(
        &self,
        actor: UserId,
        slug: ChannelSlug,
        name: DisplayName,
    ) -> Result<Circle, ChatError> {
        self.repository
            .create_circle(CreateCircle { actor, slug, name })
            .await
            .map_err(ChatError::from)
    }

    pub async fn list_circles(
        &self,
        actor: UserId,
    ) -> Result<Vec<(Circle, CircleRole)>, ChatError> {
        self.repository
            .list_circles_for_user(actor)
            .await
            .map_err(ChatError::from)
    }

    pub async fn create_circle_invitation(
        &self,
        actor: UserId,
        circle_id: crate::domain::CircleId,
    ) -> Result<IssuedInvitation, ChatError> {
        self.repository
            .create_circle_invitation(CreateCircleInvitation { actor, circle_id })
            .await
            .map_err(ChatError::from)
    }

    pub async fn accept_circle_invitation(
        &self,
        actor: UserId,
        token: String,
    ) -> Result<CircleMembership, ChatError> {
        self.repository
            .accept_circle_invitation(AcceptCircleInvitation { actor, token })
            .await
            .map_err(ChatError::from)
    }

    pub async fn join_channel(
        &self,
        actor: UserId,
        channel: ChannelRef,
    ) -> Result<Membership, ChatError> {
        self.repository
            .join_channel(JoinChannel { actor, channel })
            .await
            .map_err(ChatError::from)
    }

    pub async fn leave_channel(
        &self,
        actor: UserId,
        channel_id: ChannelId,
    ) -> Result<(), ChatError> {
        self.repository
            .leave_channel(LeaveChannel { actor, channel_id })
            .await
            .map_err(ChatError::from)
    }

    pub async fn list_channels(&self, actor: UserId) -> Result<Vec<ChannelSummary>, ChatError> {
        self.repository
            .list_channels_for_user(actor)
            .await
            .map_err(ChatError::from)
    }

    pub async fn load_messages(
        &self,
        actor: UserId,
        channel_id: ChannelId,
        limit: MessageLimit,
        after: Option<ChannelSequence>,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        self.repository
            .load_recent_messages(LoadRecentMessages {
                actor,
                channel_id,
                limit,
                after,
            })
            .await
            .map_err(ChatError::from)
    }

    pub async fn mark_read(
        &self,
        actor: UserId,
        channel_id: ChannelId,
        sequence: ChannelSequence,
    ) -> Result<Membership, ChatError> {
        self.repository
            .mark_read(MarkRead {
                actor,
                channel_id,
                sequence,
            })
            .await
            .map_err(ChatError::from)
    }

    pub async fn subscribe(
        &self,
        channel_id: ChannelId,
        participant_id: UserId,
    ) -> Result<ChannelSubscription, ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::Subscribe {
                channel_id,
                participant_id,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
    }

    pub async fn leave(
        &self,
        channel_id: ChannelId,
        participant_id: UserId,
        connection_id: ConnectionId,
    ) -> Result<(), ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::Leave {
                channel_id,
                participant_id,
                connection_id,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
    }

    #[cfg(test)]
    pub async fn send_message(
        &self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
    ) -> Result<ChatMessage, ChatError> {
        self.send_message_command(channel_id, sender_id, body, None)
            .await
    }

    pub async fn send_message_idempotent(
        &self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
        request_id: String,
    ) -> Result<ChatMessage, ChatError> {
        self.send_message_command(channel_id, sender_id, body, Some(request_id))
            .await
    }

    async fn send_message_command(
        &self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
        request_id: Option<String>,
    ) -> Result<ChatMessage, ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::SendMessage {
                channel_id,
                sender_id,
                body,
                request_id,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
    }

    pub async fn latest_sequence(
        &self,
        channel_id: ChannelId,
    ) -> Result<crate::domain::ChannelSequence, ChatError> {
        self.repository
            .latest_sequence(channel_id)
            .await
            .map_err(ChatError::from)
    }
}

#[derive(Debug)]
pub struct ChannelSubscription {
    pub connection_id: ConnectionId,
    pub receiver: broadcast::Receiver<ChatEvent>,
    pub history: Vec<ChatMessage>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ConnectionId(Uuid);

impl ConnectionId {
    fn generate() -> Self {
        Self(Uuid::now_v7())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatError {
    EngineStopped,
    Repository(RepositoryError),
    Validation(TextValidationError),
}

impl fmt::Display for ChatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EngineStopped => formatter.write_str("chat engine is not running"),
            Self::Repository(error) => error.fmt(formatter),
            Self::Validation(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ChatError {}

impl ChatError {
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::EngineStopped => "engine_stopped",
            Self::Repository(error) => error.kind(),
            Self::Validation(_) => "validation",
        }
    }

    pub fn public_message(&self) -> String {
        match self {
            Self::EngineStopped => "chat service unavailable".to_owned(),
            Self::Repository(error) => error.public_message().to_owned(),
            Self::Validation(error) => error.to_string(),
        }
    }
}

impl From<TextValidationError> for ChatError {
    fn from(value: TextValidationError) -> Self {
        Self::Validation(value)
    }
}

impl From<RepositoryError> for ChatError {
    fn from(value: RepositoryError) -> Self {
        Self::Repository(value)
    }
}

enum Command {
    Subscribe {
        channel_id: ChannelId,
        participant_id: UserId,
        reply: oneshot::Sender<Result<ChannelSubscription, ChatError>>,
    },
    Leave {
        channel_id: ChannelId,
        participant_id: UserId,
        connection_id: ConnectionId,
        reply: oneshot::Sender<Result<(), ChatError>>,
    },
    SendMessage {
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
        request_id: Option<String>,
        reply: oneshot::Sender<Result<ChatMessage, ChatError>>,
    },
    ExternalMessage {
        message_id: MessageId,
    },
}

struct ChatActor {
    channels: HashMap<ChannelId, ChannelState>,
    published_messages: HashSet<MessageId>,
    published_order: VecDeque<MessageId>,
    repository: Arc<dyn ChatRepository>,
}

impl ChatActor {
    fn new(repository: Arc<dyn ChatRepository>) -> Self {
        Self {
            channels: HashMap::new(),
            published_messages: HashSet::new(),
            published_order: VecDeque::with_capacity(PUBLISHED_MESSAGE_CACHE),
            repository,
        }
    }

    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        while let Some(command) = receiver.recv().await {
            match command {
                Command::Subscribe {
                    channel_id,
                    participant_id,
                    reply,
                } => {
                    let response = self.subscribe(channel_id, participant_id).await;
                    let _ = reply.send(response);
                }
                Command::Leave {
                    channel_id,
                    participant_id,
                    connection_id,
                    reply,
                } => {
                    self.leave(channel_id, participant_id, connection_id);
                    let _ = reply.send(Ok(()));
                }
                Command::SendMessage {
                    channel_id,
                    sender_id,
                    body,
                    request_id,
                    reply,
                } => {
                    let response = self
                        .send_message(channel_id, sender_id, body, request_id)
                        .await;
                    let _ = reply.send(response);
                }
                Command::ExternalMessage { message_id } => {
                    match self.repository.load_message(message_id).await {
                        Ok(message) => self.publish_message(message),
                        Err(error) => tracing::warn!(
                            error_kind = error.kind(),
                            "failed to load notified message"
                        ),
                    }
                }
            }
        }
    }

    async fn subscribe(
        &mut self,
        channel_id: ChannelId,
        participant_id: UserId,
    ) -> Result<ChannelSubscription, ChatError> {
        let history = self
            .repository
            .load_recent_messages(LoadRecentMessages {
                actor: participant_id.clone(),
                channel_id: channel_id.clone(),
                limit: MessageLimit::new(100),
                after: None,
            })
            .await?;
        let channel = self.channel_mut(channel_id.clone());
        let connection_id = ConnectionId::generate();
        let connections = channel
            .participants
            .entry(participant_id.clone())
            .or_default();
        let is_new_participant = connections.is_empty();
        connections.insert(connection_id);
        let receiver = channel.events.subscribe();

        if is_new_participant {
            channel.publish(ChatEvent::ParticipantJoined {
                channel_id,
                participant_id,
            });
        }
        Ok(ChannelSubscription {
            connection_id,
            receiver,
            history,
        })
    }

    fn leave(
        &mut self,
        channel_id: ChannelId,
        participant_id: UserId,
        connection_id: ConnectionId,
    ) {
        let Some(channel) = self.channels.get_mut(&channel_id) else {
            return;
        };
        let should_publish_left =
            channel
                .participants
                .get_mut(&participant_id)
                .is_some_and(|connections| {
                    connections.remove(&connection_id);
                    connections.is_empty()
                });
        if should_publish_left {
            channel.participants.remove(&participant_id);
            channel.publish(ChatEvent::ParticipantLeft {
                channel_id,
                participant_id,
            });
        }
    }

    async fn send_message(
        &mut self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
        request_id: Option<String>,
    ) -> Result<ChatMessage, ChatError> {
        let command = SendMessage {
            actor: sender_id,
            channel_id: channel_id.clone(),
            body,
        };
        let message = match request_id {
            Some(request_id) => {
                self.repository
                    .append_message_idempotent(command, request_id)
                    .await?
            }
            None => self.repository.append_message(command).await?,
        };
        self.publish_message(message.clone());
        Ok(message)
    }

    fn publish_message(&mut self, message: ChatMessage) {
        if !self.published_messages.insert(message.id) {
            return;
        }
        if self.published_order.len() == PUBLISHED_MESSAGE_CACHE
            && let Some(expired) = self.published_order.pop_front()
        {
            self.published_messages.remove(&expired);
        }
        self.published_order.push_back(message.id);
        self.channel_mut(message.channel_id.clone())
            .publish(ChatEvent::MessageAccepted { message });
    }

    fn channel_mut(&mut self, channel_id: ChannelId) -> &mut ChannelState {
        self.channels
            .entry(channel_id)
            .or_insert_with(ChannelState::new)
    }
}

struct ChannelState {
    participants: HashMap<UserId, HashSet<ConnectionId>>,
    events: broadcast::Sender<ChatEvent>,
}

impl ChannelState {
    fn new() -> Self {
        let (events, _) = broadcast::channel(CHANNEL_EVENT_CAPACITY);
        Self {
            participants: HashMap::new(),
            events,
        }
    }

    fn publish(&self, event: ChatEvent) {
        let _ = self.events.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::InMemoryChatRepository;

    async fn chat_fixture() -> (ChatEngine, ChannelId, UserId, UserId) {
        let chat = ChatEngine::start(Arc::new(InMemoryChatRepository::default()));
        let alice = UserId::named("alice");
        let bob = UserId::named("bob");
        let channel = chat
            .prepare_development_session(alice.clone(), "alice", "general")
            .await
            .unwrap();
        let bob_channel = chat
            .prepare_development_session(bob.clone(), "bob", "general")
            .await
            .unwrap();
        assert_eq!(channel, bob_channel);
        (chat, channel, alice, bob)
    }

    #[tokio::test]
    async fn persists_before_broadcasting_messages() {
        let (chat, channel, alice, bob) = chat_fixture().await;
        let mut alice_subscription = chat
            .subscribe(channel.clone(), alice.clone())
            .await
            .unwrap();
        let mut bob_subscription = chat.subscribe(channel.clone(), bob).await.unwrap();

        let message = chat
            .send_message(
                channel,
                alice,
                MessageBody::new("Hei frå mailboxen").unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(u64::from(message.sequence), 1);
        assert_eq!(
            next_message_event(&mut alice_subscription.receiver).await,
            message
        );
        assert_eq!(
            next_message_event(&mut bob_subscription.receiver).await,
            message
        );
    }

    #[tokio::test]
    async fn new_subscribers_load_history_from_repository() {
        let (chat, channel, alice, bob) = chat_fixture().await;
        chat.send_message(
            channel.clone(),
            alice,
            MessageBody::new("Melding før innlogging").unwrap(),
        )
        .await
        .unwrap();

        let subscription = chat.subscribe(channel, bob).await.unwrap();
        assert_eq!(subscription.history.len(), 1);
        assert_eq!(u64::from(subscription.history[0].sequence), 1);
    }

    #[tokio::test]
    async fn participant_leaves_only_after_last_connection_closes() {
        let (chat, channel, alice, _) = chat_fixture().await;
        let mut first = chat
            .subscribe(channel.clone(), alice.clone())
            .await
            .unwrap();
        let joined = first.receiver.recv().await.unwrap();
        assert!(matches!(joined, ChatEvent::ParticipantJoined { .. }));
        let second = chat
            .subscribe(channel.clone(), alice.clone())
            .await
            .unwrap();

        chat.leave(channel.clone(), alice.clone(), first.connection_id)
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), first.receiver.recv())
                .await
                .is_err()
        );

        chat.leave(channel, alice, second.connection_id)
            .await
            .unwrap();
        let left = first.receiver.recv().await.unwrap();
        assert!(matches!(left, ChatEvent::ParticipantLeft { .. }));
    }

    #[tokio::test]
    async fn lagged_subscriber_recovers_exactly_from_durable_sequence() {
        let (chat, channel, alice, bob) = chat_fixture().await;
        let mut subscription = chat.subscribe(channel.clone(), bob.clone()).await.unwrap();
        let _joined = subscription.receiver.recv().await.unwrap();
        let message_count = CHANNEL_EVENT_CAPACITY + 44;

        for index in 1..=message_count {
            chat.send_message_idempotent(
                channel.clone(),
                alice.clone(),
                MessageBody::new(format!("lag-{index}")).unwrap(),
                format!("lag-request-{index}"),
            )
            .await
            .unwrap();
        }

        let skipped = match subscription.receiver.recv().await {
            Err(broadcast::error::RecvError::Lagged(skipped)) => skipped,
            other => panic!("expected broadcast lag, received {other:?}"),
        };
        assert!(skipped > 0);
        let mut recovered = Vec::new();
        let mut cursor = ChannelSequence::new(0);
        while u64::from(cursor) < message_count as u64 {
            let page = chat
                .load_messages(
                    bob.clone(),
                    channel.clone(),
                    MessageLimit::new(200),
                    Some(cursor),
                )
                .await
                .unwrap();
            assert!(!page.is_empty(), "catch-up stopped before latest sequence");
            cursor = page.last().unwrap().sequence;
            recovered.extend(page);
        }
        assert_eq!(recovered.len(), message_count);
        assert_eq!(u64::from(recovered[0].sequence), 1);
        assert_eq!(
            u64::from(recovered.last().unwrap().sequence),
            message_count as u64
        );
    }

    async fn next_message_event(receiver: &mut broadcast::Receiver<ChatEvent>) -> ChatMessage {
        loop {
            match receiver.recv().await.unwrap() {
                ChatEvent::MessageAccepted { message } => return message,
                ChatEvent::ChannelCreated { .. }
                | ChatEvent::ParticipantJoined { .. }
                | ChatEvent::ParticipantLeft { .. }
                | ChatEvent::ReadMarkerUpdated { .. } => {}
            }
        }
    }
}
