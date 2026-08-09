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
    ListCircleUsers {
        circle_id: crate::domain::CircleId,
    },
    SetStatus {
        text: String,
        emoji: String,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    },
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
    ListChannelUsers {
        channel_id: ChannelId,
    },
    UpdateChannelDescription {
        channel_id: ChannelId,
        description: String,
    },
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
    LoadThread {
        root_message_id: crate::domain::MessageId,
    },
    ListThreadSummaries {
        channel_id: ChannelId,
    },
    MarkThreadRead {
        root_message_id: crate::domain::MessageId,
        sequence: crate::domain::ChannelSequence,
    },
    SubscribeChannel {
        channel_id: ChannelId,
    },
    UnsubscribeChannel {
        channel_id: ChannelId,
    },
    SendMessage {
        channel_id: ChannelId,
        #[serde(default)]
        parent_message_id: Option<crate::domain::MessageId>,
        body: String,
    },
    EditMessage {
        message_id: crate::domain::MessageId,
        body: String,
    },
    DeleteMessage {
        message_id: crate::domain::MessageId,
    },
    ListChannelReactions {
        channel_id: ChannelId,
    },
    ToggleMessageReaction {
        message_id: crate::domain::MessageId,
        emoji: String,
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
    LeaveCircle {
        circle_id: crate::domain::CircleId,
    },
    CreateCircleInvitation {
        circle_id: crate::domain::CircleId,
    },
    AcceptCircleInvitation {
        token: String,
    },
    CreateInvitation {
        target: crate::domain::InvitationTarget,
    },
    InspectInvitation {
        token: String,
    },
    DeclineInvitation {
        token: String,
    },
    AcceptInvitation {
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
        users: Vec<crate::domain::UserProfile>,
    },
    CircleUsersListed {
        circle_id: crate::domain::CircleId,
        users: Vec<crate::domain::UserProfile>,
    },
    StatusUpdated {
        profile: crate::domain::UserProfile,
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
    ChannelUsersListed {
        channel_id: ChannelId,
        users: Vec<crate::domain::UserProfile>,
    },
    ChannelDescriptionUpdated {
        channel_id: ChannelId,
        description: String,
    },
    JoinableChannelsListed {
        channels: Vec<crate::domain::DiscoverableChannel>,
    },
    ChannelMemberAdded {
        membership: Membership,
    },
    MessagesLoaded {
        channel_id: ChannelId,
        messages: Vec<ChatMessage>,
    },
    ThreadLoaded {
        root_message_id: crate::domain::MessageId,
        messages: Vec<ChatMessage>,
    },
    ThreadSummariesListed {
        channel_id: ChannelId,
        summaries: Vec<crate::domain::ThreadSummary>,
    },
    ThreadReadUpdated {
        summary: crate::domain::ThreadSummary,
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
    MessageEdited {
        message: ChatMessage,
    },
    MessageDeleted {
        message: ChatMessage,
    },
    ChannelReactionsListed {
        channel_id: ChannelId,
        reactions: Vec<crate::domain::MessageReactionSummary>,
    },
    MessageReactionChanged {
        change: crate::domain::MessageReactionChange,
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
    CircleLeft {
        circle_id: crate::domain::CircleId,
    },
    CircleInvitationCreated {
        invitation: crate::domain::IssuedInvitation,
    },
    CircleInvitationAccepted {
        membership: crate::domain::CircleMembership,
    },
    InvitationCreated {
        invitation: crate::domain::IssuedChatInvitation,
    },
    InvitationInspected {
        token: String,
        invitation: crate::domain::InvitationPreview,
    },
    InvitationDeclined {
        token: String,
        invitation: crate::domain::InvitationPreview,
    },
    InvitationAccepted {
        token: String,
        invitation: crate::domain::AcceptedChatInvitation,
    },
    Error {
        code: &'static str,
        message: String,
    },
}
