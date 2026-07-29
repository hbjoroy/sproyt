use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::Arc,
};

use tokio::sync::{broadcast, mpsc, oneshot};
use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

#[cfg(test)]
use crate::domain::PrincipalKind;
use crate::domain::{
    AcceptCircleInvitation, ChannelId, ChannelKind, ChannelRef, ChannelSequence, ChannelSlug,
    ChannelSummary, ChatEvent, ChatMessage, ChatRepository, Circle, CircleMembership, CircleRole,
    CreateChannel, CreateCircle, CreateCircleInvitation, DeleteCircle, DisplayName, EditMessage,
    IssuedInvitation, JoinChannel, LeaveChannel, LoadRecentMessages, MarkRead, MediaId,
    MediaObject, MediaUpload, MediaVariant, Membership, MessageBody, MessageId, MessageLimit,
    MessageReactionChange, MessageReactionSummary, PresenceLease, RepositoryError, SendMessage,
    TextValidationError, User, UserId, UserProfile,
};

const MAILBOX_CAPACITY: usize = 1024;
const CHANNEL_EVENT_CAPACITY: usize = 256;
const PUBLISHED_MESSAGE_CACHE: usize = 4096;
const PRESENCE_LEASE_TTL: std::time::Duration = std::time::Duration::from_secs(75);
fn is_reaction_emoji(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 32 || value.graphemes(true).count() != 1 {
        return false;
    }
    value.chars().any(|character| {
        matches!(
            character,
            '\u{00a9}'
                | '\u{00ae}'
                | '\u{203c}'
                | '\u{2049}'
                | '\u{2122}'
                | '\u{2139}'
                | '\u{3030}'
                | '\u{303d}'
                | '\u{3297}'
                | '\u{3299}'
        ) || ('\u{2190}'..='\u{21ff}').contains(&character)
            || ('\u{2300}'..='\u{23ff}').contains(&character)
            || ('\u{2460}'..='\u{24ff}').contains(&character)
            || ('\u{25a0}'..='\u{27bf}').contains(&character)
            || ('\u{1f000}'..='\u{1faff}').contains(&character)
            || ('\u{1fc00}'..='\u{1ffff}').contains(&character)
            || character == '\u{20e3}'
    })
}

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
        if let Some(mut notifications) = repository.subscribe_reactions() {
            let notification_mailbox = mailbox.clone();
            tokio::spawn(async move {
                loop {
                    match notifications.recv().await {
                        Ok(event) => {
                            if notification_mailbox
                                .send(Command::ExternalReaction { event })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "reaction notification listener lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        if let Some(mut notifications) = repository.subscribe_message_updates() {
            let notification_mailbox = mailbox.clone();
            tokio::spawn(async move {
                loop {
                    match notifications.recv().await {
                        Ok(event) => {
                            if notification_mailbox
                                .send(Command::ExternalUpdate { event })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "message update notification listener lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
        }
        let distributed_presence = if let Some(mut notifications) = repository.subscribe_presence()
        {
            let notification_mailbox = mailbox.clone();
            tokio::spawn(async move {
                loop {
                    match notifications.recv().await {
                        Ok(event) => {
                            if notification_mailbox
                                .send(Command::ExternalPresence { event })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(skipped, "database presence listener lagged");
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
            });
            true
        } else {
            false
        };
        let sweep_mailbox = mailbox.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            interval.tick().await;
            loop {
                interval.tick().await;
                if sweep_mailbox.send(Command::ExpirePresence).await.is_err() {
                    break;
                }
            }
        });
        tokio::spawn(ChatActor::new(repository.clone(), distributed_presence).run(receiver));
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

    pub async fn list_users(&self, actor: UserId) -> Result<Vec<UserProfile>, ChatError> {
        self.repository
            .list_user_profiles(actor)
            .await
            .map_err(ChatError::from)
    }

    pub async fn list_circle_users(
        &self,
        actor: UserId,
        circle_id: crate::domain::CircleId,
    ) -> Result<Vec<UserProfile>, ChatError> {
        self.repository
            .list_circle_user_profiles(actor, circle_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn set_status(
        &self,
        actor: UserId,
        text: String,
        emoji: String,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<UserProfile, ChatError> {
        let text = text.trim().to_owned();
        let emoji = emoji.trim().to_owned();
        if text.chars().count() > 100
            || emoji.chars().count() > 16
            || text.chars().any(char::is_control)
            || emoji.chars().any(char::is_control)
        {
            return Err(ChatError::Validation(TextValidationError::TooLarge {
                field: "status",
                max: 100,
            }));
        }
        self.repository
            .set_user_status(actor, text, emoji, expires_at)
            .await
            .map_err(ChatError::from)
    }

    pub async fn store_media(&self, upload: MediaUpload) -> Result<MediaObject, ChatError> {
        use sha2::{Digest, Sha256};
        let sha256 = format!("{:x}", Sha256::digest(&upload.content));
        self.repository
            .store_media(upload, sha256)
            .await
            .map_err(ChatError::from)
    }

    pub async fn load_media(
        &self,
        actor: UserId,
        media_id: MediaId,
    ) -> Result<(MediaObject, Vec<u8>), ChatError> {
        self.repository
            .load_media(actor, media_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn load_media_preview(
        &self,
        actor: UserId,
        media_id: MediaId,
    ) -> Result<Option<MediaVariant>, ChatError> {
        self.repository
            .load_media_preview(actor, media_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn open_direct_channel(
        &self,
        actor: UserId,
        other: UserId,
    ) -> Result<crate::domain::Channel, ChatError> {
        self.repository
            .open_direct_channel(actor, other)
            .await
            .map_err(ChatError::from)
    }

    pub async fn export_user_data(
        &self,
        actor: UserId,
    ) -> Result<crate::domain::PortableUserExport, ChatError> {
        self.repository
            .export_user_data(actor)
            .await
            .map_err(ChatError::from)
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

    pub async fn delete_circle(
        &self,
        actor: UserId,
        circle_id: crate::domain::CircleId,
    ) -> Result<(), ChatError> {
        self.repository
            .delete_circle(DeleteCircle { actor, circle_id })
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

    pub async fn list_joinable_channels(
        &self,
        actor: UserId,
        circle_id: crate::domain::CircleId,
    ) -> Result<Vec<crate::domain::Channel>, ChatError> {
        self.repository
            .list_joinable_channels(actor, circle_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn add_channel_member(
        &self,
        actor: UserId,
        channel_id: ChannelId,
        user_id: UserId,
    ) -> Result<Membership, ChatError> {
        self.repository
            .add_channel_member(crate::domain::AddChannelMember {
                actor,
                channel_id,
                user_id,
            })
            .await
            .map_err(ChatError::from)
    }

    pub async fn load_messages(
        &self,
        actor: UserId,
        channel_id: ChannelId,
        limit: MessageLimit,
        after: Option<ChannelSequence>,
        before: Option<ChannelSequence>,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        self.repository
            .load_recent_messages(LoadRecentMessages {
                actor,
                channel_id,
                limit,
                after,
                before,
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

    pub async fn list_mentions(
        &self,
        actor: UserId,
    ) -> Result<Vec<crate::domain::InboxMention>, ChatError> {
        self.repository
            .list_mentions(actor)
            .await
            .map_err(ChatError::from)
    }

    pub async fn mark_mention_read(
        &self,
        actor: UserId,
        message_id: MessageId,
    ) -> Result<(), ChatError> {
        self.repository
            .mark_mention_read(actor, message_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn create_task(
        &self,
        actor: UserId,
        source_message_id: MessageId,
        assignee_id: UserId,
        title: String,
        process_link_id: Option<uuid::Uuid>,
    ) -> Result<crate::domain::UserTask, ChatError> {
        if title.trim().is_empty() {
            return Err(ChatError::Validation(TextValidationError::Empty {
                field: "task title",
            }));
        }
        if title.len() > 240 {
            return Err(ChatError::Validation(TextValidationError::TooLarge {
                field: "task title",
                max: 240,
            }));
        }
        self.repository
            .create_task(
                actor,
                source_message_id,
                assignee_id,
                title.trim().to_owned(),
                process_link_id,
            )
            .await
            .map_err(ChatError::from)
    }

    pub async fn list_tasks(
        &self,
        actor: UserId,
    ) -> Result<Vec<crate::domain::UserTask>, ChatError> {
        self.repository
            .list_tasks(actor)
            .await
            .map_err(ChatError::from)
    }

    pub async fn set_task_done(
        &self,
        actor: UserId,
        task_id: uuid::Uuid,
        done: bool,
    ) -> Result<crate::domain::UserTask, ChatError> {
        self.repository
            .set_task_done(actor, task_id, done)
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

    pub async fn renew_presence(
        &self,
        participant_id: UserId,
        subscriptions: Vec<(ChannelId, ConnectionId)>,
    ) -> Result<(), ChatError> {
        let leases = subscriptions
            .into_iter()
            .map(|(channel_id, connection_id)| PresenceLease {
                channel_id,
                participant_id: participant_id.clone(),
                connection_id: connection_id.0,
            })
            .collect();
        self.repository
            .renew_presence(leases, PRESENCE_LEASE_TTL)
            .await
            .map_err(ChatError::from)
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

    pub async fn list_channel_reactions(
        &self,
        actor: UserId,
        channel_id: ChannelId,
    ) -> Result<Vec<MessageReactionSummary>, ChatError> {
        self.repository
            .list_channel_reactions(actor, channel_id)
            .await
            .map_err(ChatError::from)
    }

    pub async fn toggle_reaction(
        &self,
        actor: UserId,
        message_id: MessageId,
        emoji: String,
    ) -> Result<MessageReactionChange, ChatError> {
        let emoji = emoji.trim().to_owned();
        if !is_reaction_emoji(&emoji) {
            return Err(TextValidationError::InvalidReaction.into());
        }
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::ToggleReaction {
                actor,
                message_id,
                emoji,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
    }

    pub async fn edit_message(
        &self,
        actor: UserId,
        message_id: MessageId,
        body: MessageBody,
    ) -> Result<ChatMessage, ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::EditMessage {
                actor,
                message_id,
                body,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
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
    ToggleReaction {
        actor: UserId,
        message_id: MessageId,
        emoji: String,
        reply: oneshot::Sender<Result<MessageReactionChange, ChatError>>,
    },
    EditMessage {
        actor: UserId,
        message_id: MessageId,
        body: MessageBody,
        reply: oneshot::Sender<Result<ChatMessage, ChatError>>,
    },
    ExternalMessage {
        message_id: MessageId,
    },
    ExternalReaction {
        event: ChatEvent,
    },
    ExternalUpdate {
        event: ChatEvent,
    },
    ExternalPresence {
        event: ChatEvent,
    },
    ExpirePresence,
}

struct ChatActor {
    channels: HashMap<ChannelId, ChannelState>,
    published_messages: HashSet<MessageId>,
    published_order: VecDeque<MessageId>,
    repository: Arc<dyn ChatRepository>,
    distributed_presence: bool,
}

impl ChatActor {
    fn new(repository: Arc<dyn ChatRepository>, distributed_presence: bool) -> Self {
        Self {
            channels: HashMap::new(),
            published_messages: HashSet::new(),
            published_order: VecDeque::with_capacity(PUBLISHED_MESSAGE_CACHE),
            repository,
            distributed_presence,
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
                    self.leave(channel_id, participant_id, connection_id).await;
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
                Command::ToggleReaction {
                    actor,
                    message_id,
                    emoji,
                    reply,
                } => {
                    let response = self
                        .repository
                        .toggle_message_reaction(actor, message_id, emoji)
                        .await
                        .map_err(ChatError::from);
                    if let Ok(change) = &response {
                        self.channel_mut(change.channel_id.clone()).publish(
                            ChatEvent::MessageReactionChanged {
                                change: change.clone(),
                            },
                        );
                    }
                    let _ = reply.send(response);
                }
                Command::EditMessage {
                    actor,
                    message_id,
                    body,
                    reply,
                } => {
                    let response = self
                        .repository
                        .edit_message(EditMessage {
                            actor,
                            message_id,
                            body,
                        })
                        .await
                        .map_err(ChatError::from);
                    if let Ok(message) = &response {
                        self.channel_mut(message.channel_id.clone()).publish(
                            ChatEvent::MessageEdited {
                                message: message.clone(),
                            },
                        );
                    }
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
                Command::ExternalReaction { event } => {
                    if let ChatEvent::MessageReactionChanged { change } = &event {
                        self.channel_mut(change.channel_id.clone()).publish(event);
                    }
                }
                Command::ExternalUpdate { event } => {
                    if let ChatEvent::MessageEdited { message } = &event {
                        self.channel_mut(message.channel_id.clone()).publish(event);
                    }
                }
                Command::ExternalPresence { event } => self.publish_presence(event),
                Command::ExpirePresence => {
                    if let Err(error) = self.repository.expire_presence().await {
                        tracing::warn!(
                            error_kind = error.kind(),
                            "failed to expire presence leases"
                        );
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
                limit: MessageLimit::new(50),
                after: None,
                before: None,
            })
            .await?;
        let connection_id = ConnectionId::generate();
        let (is_new_participant, receiver) = {
            let channel = self.channel_mut(channel_id.clone());
            let connections = channel
                .participants
                .entry(participant_id.clone())
                .or_default();
            let is_new_participant = connections.is_empty();
            connections.insert(connection_id);
            (is_new_participant, channel.events.subscribe())
        };

        if self.distributed_presence {
            if let Err(error) = self
                .repository
                .register_presence(
                    PresenceLease {
                        channel_id: channel_id.clone(),
                        participant_id: participant_id.clone(),
                        connection_id: connection_id.0,
                    },
                    PRESENCE_LEASE_TTL,
                )
                .await
            {
                if let Some(channel) = self.channels.get_mut(&channel_id)
                    && let Some(connections) = channel.participants.get_mut(&participant_id)
                {
                    connections.remove(&connection_id);
                    if connections.is_empty() {
                        channel.participants.remove(&participant_id);
                    }
                }
                return Err(error.into());
            }
        } else if is_new_participant {
            self.channel_mut(channel_id.clone())
                .publish(ChatEvent::ParticipantJoined {
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

    async fn leave(
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
        if self.distributed_presence {
            let _ = self
                .repository
                .unregister_presence(PresenceLease {
                    channel_id,
                    participant_id,
                    connection_id: connection_id.0,
                })
                .await;
        } else if should_publish_left {
            channel.participants.remove(&participant_id);
            channel.publish(ChatEvent::ParticipantLeft {
                channel_id,
                participant_id,
            });
        }
    }

    fn publish_presence(&mut self, event: ChatEvent) {
        let channel_id = match &event {
            ChatEvent::ParticipantJoined { channel_id, .. }
            | ChatEvent::ParticipantLeft { channel_id, .. } => channel_id.clone(),
            _ => return,
        };
        self.channel_mut(channel_id).publish(event);
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

    #[test]
    fn reaction_validation_accepts_one_unicode_emoji_grapheme() {
        for emoji in ["❤️", "👍🏽", "👨‍👩‍👧‍👦", "🇳🇴", "🌊"] {
            assert!(is_reaction_emoji(emoji), "rejected {emoji}");
        }
        for invalid in ["", "ja", "👍👍", "<script>", "a"] {
            assert!(!is_reaction_emoji(invalid), "accepted {invalid}");
        }
    }

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
    async fn edits_are_authorized_persisted_and_broadcast() {
        let (chat, channel, alice, bob) = chat_fixture().await;
        let mut subscription = chat
            .subscribe(channel.clone(), alice.clone())
            .await
            .unwrap();
        let _joined = subscription.receiver.recv().await.unwrap();
        let message = chat
            .send_message(channel, alice.clone(), MessageBody::new("før").unwrap())
            .await
            .unwrap();
        let _accepted = next_message_event(&mut subscription.receiver).await;
        let denied = chat
            .edit_message(bob, message.id, MessageBody::new("uautorisert").unwrap())
            .await;
        assert!(matches!(
            denied,
            Err(ChatError::Repository(RepositoryError::PermissionDenied))
        ));
        let edited = chat
            .edit_message(alice, message.id, MessageBody::new("etter").unwrap())
            .await
            .unwrap();
        assert_eq!(edited.body.as_str(), "etter");
        assert!(edited.edited_at.is_some());
        assert_eq!(edited.sequence, message.sequence);
        assert_eq!(
            subscription.receiver.recv().await.unwrap(),
            ChatEvent::MessageEdited { message: edited }
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
                    None,
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
                ChatEvent::MessageEdited { .. }
                | ChatEvent::ChannelCreated { .. }
                | ChatEvent::ParticipantJoined { .. }
                | ChatEvent::ParticipantLeft { .. }
                | ChatEvent::MessageReactionChanged { .. }
                | ChatEvent::ReadMarkerUpdated { .. } => {}
            }
        }
    }
}
