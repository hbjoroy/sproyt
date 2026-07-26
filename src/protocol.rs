use serde::{Deserialize, Serialize};

use crate::domain::{
    Channel, ChannelId, ChannelKind, ChannelRef, ChannelSequence, ChannelSummary, ChatEvent,
    ChatMessage, Membership,
};

pub const PROTOCOL_ID: &str = "sproyt.chat.v1";

#[derive(Debug, Deserialize)]
pub struct ClientEnvelope {
    pub protocol: String,
    pub request_id: String,
    #[serde(flatten)]
    pub command: ClientCommand,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ClientCommand {
    Hello,
    ListUsers,
    OpenDirectChannel {
        user_id: crate::domain::UserId,
    },
    CreateChannel {
        slug: String,
        name: String,
        kind: ChannelKind,
        circle_id: Option<crate::domain::CircleId>,
    },
    JoinChannel {
        channel: ChannelRef,
    },
    LeaveChannel {
        channel_id: ChannelId,
    },
    ListMyChannels,
    ListJoinableChannels {
        circle_id: crate::domain::CircleId,
    },
    AddChannelMember {
        channel_id: ChannelId,
        user_id: crate::domain::UserId,
    },
    LoadRecentMessages {
        channel_id: ChannelId,
        limit: Option<u16>,
        after: Option<ChannelSequence>,
        before: Option<ChannelSequence>,
    },
    SubscribeChannel {
        channel_id: ChannelId,
    },
    UnsubscribeChannel {
        channel_id: ChannelId,
    },
    SendMessage {
        channel_id: ChannelId,
        body: String,
    },
    MarkRead {
        channel_id: ChannelId,
        sequence: ChannelSequence,
    },
    ListMentions,
    MarkMentionRead {
        message_id: crate::domain::MessageId,
    },
    CreateTask {
        source_message_id: crate::domain::MessageId,
        assignee_id: crate::domain::UserId,
        title: String,
        process_link_id: Option<uuid::Uuid>,
    },
    ListTasks,
    SetTaskDone {
        task_id: uuid::Uuid,
        done: bool,
    },
    Ping,
    CreateCircle {
        slug: String,
        name: String,
    },
    ListMyCircles,
    DeleteCircle {
        circle_id: crate::domain::CircleId,
    },
    CreateCircleInvitation {
        circle_id: crate::domain::CircleId,
    },
    AcceptCircleInvitation {
        token: String,
    },
}

#[derive(Debug, Serialize)]
pub struct ServerEnvelope {
    pub protocol: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub event: ServerEvent,
}

impl ServerEnvelope {
    pub fn response(request_id: String, event: ServerEvent) -> Self {
        Self {
            protocol: PROTOCOL_ID,
            request_id: Some(request_id),
            event,
        }
    }

    pub fn event(event: ServerEvent) -> Self {
        Self {
            protocol: PROTOCOL_ID,
            request_id: None,
            event,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ServerEvent {
    Hello {
        participant_id: crate::domain::UserId,
    },
    UsersListed {
        users: Vec<crate::domain::User>,
    },
    DirectChannelOpened {
        channel: Channel,
    },
    ChannelCreated {
        channel: Channel,
    },
    MembershipJoined {
        membership: Membership,
    },
    MembershipLeft {
        channel_id: ChannelId,
    },
    ChannelsListed {
        channels: Vec<ChannelSummary>,
    },
    JoinableChannelsListed {
        channels: Vec<Channel>,
    },
    ChannelMemberAdded {
        membership: Membership,
    },
    MessagesLoaded {
        channel_id: ChannelId,
        messages: Vec<ChatMessage>,
    },
    SubscriptionStarted {
        channel_id: ChannelId,
        history: Vec<ChatMessage>,
    },
    SubscriptionEnded {
        channel_id: ChannelId,
    },
    MessageAccepted {
        message: ChatMessage,
    },
    ReadMarkerUpdated {
        membership: Membership,
    },
    MentionsListed {
        mentions: Vec<crate::domain::InboxMention>,
    },
    MentionRead {
        message_id: crate::domain::MessageId,
    },
    TaskCreated {
        task: crate::domain::UserTask,
    },
    TasksListed {
        tasks: Vec<crate::domain::UserTask>,
    },
    TaskUpdated {
        task: crate::domain::UserTask,
    },
    Chat {
        event: ChatEvent,
    },
    Lagged {
        channel_id: ChannelId,
        last_seen_sequence: ChannelSequence,
        latest_known_sequence: ChannelSequence,
        skipped: u64,
        hint: &'static str,
    },
    Pong,
    CircleCreated {
        circle: crate::domain::Circle,
    },
    CirclesListed {
        circles: Vec<(crate::domain::Circle, crate::domain::CircleRole)>,
    },
    CircleDeleted {
        circle_id: crate::domain::CircleId,
    },
    CircleInvitationCreated {
        invitation: crate::domain::IssuedInvitation,
    },
    CircleInvitationAccepted {
        membership: crate::domain::CircleMembership,
    },
    Error {
        code: &'static str,
        message: String,
    },
}
