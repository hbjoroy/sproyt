use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::{broadcast, mpsc, oneshot};

use crate::domain::{
    ChannelId, ChannelSequence, ChatEvent, ChatMessage, MessageBody, MessageId,
    TextValidationError, UserId,
};

const MAILBOX_CAPACITY: usize = 1024;
const CHANNEL_EVENT_CAPACITY: usize = 256;
const CHANNEL_HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug)]
pub struct ChatEngine {
    mailbox: mpsc::Sender<Command>,
}

impl ChatEngine {
    pub fn start() -> Self {
        let (mailbox, receiver) = mpsc::channel(MAILBOX_CAPACITY);
        tokio::spawn(ChatActor::default().run(receiver));
        Self { mailbox }
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
    ) -> Result<(), ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::Leave {
                channel_id,
                participant_id,
                reply,
            })
            .await
            .map_err(|_| ChatError::EngineStopped)?;
        response.await.map_err(|_| ChatError::EngineStopped)?
    }

    pub async fn send_message(
        &self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
    ) -> Result<ChatMessage, ChatError> {
        let (reply, response) = oneshot::channel();
        self.mailbox
            .send(Command::SendMessage {
                channel_id,
                sender_id,
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
    pub receiver: broadcast::Receiver<ChatEvent>,
    pub history: Vec<ChatMessage>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChatError {
    EngineStopped,
    Validation(TextValidationError),
}

impl fmt::Display for ChatError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EngineStopped => write!(formatter, "chat engine is not running"),
            Self::Validation(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ChatError {}

impl From<TextValidationError> for ChatError {
    fn from(value: TextValidationError) -> Self {
        Self::Validation(value)
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
        reply: oneshot::Sender<Result<(), ChatError>>,
    },
    SendMessage {
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
        reply: oneshot::Sender<Result<ChatMessage, ChatError>>,
    },
}

#[derive(Default)]
struct ChatActor {
    channels: HashMap<ChannelId, ChannelState>,
    next_message_id: u64,
}

impl ChatActor {
    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        while let Some(command) = receiver.recv().await {
            match command {
                Command::Subscribe {
                    channel_id,
                    participant_id,
                    reply,
                } => {
                    let response = self.subscribe(channel_id, participant_id);
                    let _ = reply.send(Ok(response));
                }
                Command::Leave {
                    channel_id,
                    participant_id,
                    reply,
                } => {
                    self.leave(channel_id, participant_id);
                    let _ = reply.send(Ok(()));
                }
                Command::SendMessage {
                    channel_id,
                    sender_id,
                    body,
                    reply,
                } => {
                    let message = self.send_message(channel_id, sender_id, body);
                    let _ = reply.send(Ok(message));
                }
            }
        }
    }

    fn subscribe(&mut self, channel_id: ChannelId, participant_id: UserId) -> ChannelSubscription {
        let channel = self.channel_mut(channel_id.clone());
        let is_new_participant = channel.participants.insert(participant_id.clone());
        let receiver = channel.events.subscribe();
        let history = channel.messages.iter().cloned().collect();

        if is_new_participant {
            channel.publish(ChatEvent::ParticipantJoined {
                channel_id,
                participant_id,
            });
        }

        ChannelSubscription { receiver, history }
    }

    fn leave(&mut self, channel_id: ChannelId, participant_id: UserId) {
        let Some(channel) = self.channels.get_mut(&channel_id) else {
            return;
        };

        if channel.participants.remove(&participant_id) {
            channel.publish(ChatEvent::ParticipantLeft {
                channel_id,
                participant_id,
            });
        }
    }

    fn send_message(
        &mut self,
        channel_id: ChannelId,
        sender_id: UserId,
        body: MessageBody,
    ) -> ChatMessage {
        self.next_message_id += 1;
        let message_id = self.next_message_id;
        let channel = self.channel_mut(channel_id.clone());
        channel.next_sequence = channel.next_sequence.next();

        let message = ChatMessage {
            id: MessageId::new(message_id),
            channel_id,
            sender_id,
            body,
            sequence: channel.next_sequence,
            sent_at_unix_ms: unix_time_ms(),
        };

        channel.remember(message.clone());
        channel.publish(ChatEvent::MessageAccepted {
            message: message.clone(),
        });
        message
    }

    fn channel_mut(&mut self, channel_id: ChannelId) -> &mut ChannelState {
        self.channels
            .entry(channel_id)
            .or_insert_with(ChannelState::new)
    }
}

struct ChannelState {
    participants: HashSet<UserId>,
    messages: VecDeque<ChatMessage>,
    events: broadcast::Sender<ChatEvent>,
    next_sequence: ChannelSequence,
}

impl ChannelState {
    fn new() -> Self {
        let (events, _) = broadcast::channel(CHANNEL_EVENT_CAPACITY);
        Self {
            participants: HashSet::new(),
            messages: VecDeque::with_capacity(CHANNEL_HISTORY_LIMIT),
            events,
            next_sequence: ChannelSequence::new(0),
        }
    }

    fn remember(&mut self, message: ChatMessage) {
        if self.messages.len() == CHANNEL_HISTORY_LIMIT {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
    }

    fn publish(&self, event: ChatEvent) {
        let _ = self.events.send(event);
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

    #[tokio::test]
    async fn broadcasts_messages_to_channel_subscribers() {
        let chat = ChatEngine::start();
        let channel = ChannelId::new("general").unwrap();
        let alice = UserId::new("alice").unwrap();
        let bob = UserId::new("bob").unwrap();

        let mut alice_subscription = chat
            .subscribe(channel.clone(), alice.clone())
            .await
            .unwrap();
        let mut bob_subscription = chat.subscribe(channel.clone(), bob).await.unwrap();

        let message = chat
            .send_message(
                channel.clone(),
                alice,
                MessageBody::new("Hei frå mailboxen").unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(u64::from(message.sequence), 1);

        let alice_event = next_message_event(&mut alice_subscription.receiver).await;
        let bob_event = next_message_event(&mut bob_subscription.receiver).await;

        assert_eq!(alice_event.id, message.id);
        assert_eq!(bob_event.id, message.id);
    }

    #[tokio::test]
    async fn new_subscribers_get_recent_history() {
        let chat = ChatEngine::start();
        let channel = ChannelId::new("general").unwrap();
        let alice = UserId::new("alice").unwrap();

        chat.send_message(
            channel.clone(),
            alice,
            MessageBody::new("Melding før innlogging").unwrap(),
        )
        .await
        .unwrap();

        let subscription = chat
            .subscribe(channel, UserId::new("bob").unwrap())
            .await
            .unwrap();

        assert_eq!(subscription.history.len(), 1);
        assert_eq!(u64::from(subscription.history[0].sequence), 1);
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
