use serde::{Deserialize, Serialize};

use crate::{
    Channel, ChannelId, ChannelKind, ChannelRef, ChannelSequence, ChannelSummary, ChatEvent,
    ChatMessage, Membership,
};

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ClientCommand {
    Hello,
    ListUsers,
    ListCircleUsers {
        circle_id: crate::CircleId,
    },
    SetStatus {
        text: String,
        emoji: String,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    },
    OpenDirectChannel {
        user_id: crate::UserId,
    },
    CreateChannel {
        slug: String,
        name: String,
        kind: ChannelKind,
        circle_id: Option<crate::CircleId>,
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
        circle_id: crate::CircleId,
    },
    AddChannelMember {
        channel_id: ChannelId,
        user_id: crate::UserId,
    },
    LoadRecentMessages {
        channel_id: ChannelId,
        limit: Option<u16>,
        after: Option<ChannelSequence>,
        before: Option<ChannelSequence>,
    },
    LoadThread {
        root_message_id: crate::MessageId,
    },
    ListThreadSummaries {
        channel_id: ChannelId,
    },
    MarkThreadRead {
        root_message_id: crate::MessageId,
        sequence: crate::ChannelSequence,
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
        parent_message_id: Option<crate::MessageId>,
        body: String,
    },
    EditMessage {
        message_id: crate::MessageId,
        body: String,
    },
    DeleteMessage {
        message_id: crate::MessageId,
    },
    ListChannelReactions {
        channel_id: ChannelId,
    },
    ToggleMessageReaction {
        message_id: crate::MessageId,
        emoji: String,
    },
    MarkRead {
        channel_id: ChannelId,
        sequence: ChannelSequence,
    },
    ListMentions,
    MarkMentionRead {
        message_id: crate::MessageId,
    },
    CreateTask {
        source_message_id: crate::MessageId,
        assignee_id: crate::UserId,
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
        circle_id: crate::CircleId,
    },
    LeaveCircle {
        circle_id: crate::CircleId,
    },
    CreateCircleInvitation {
        circle_id: crate::CircleId,
    },
    AcceptCircleInvitation {
        token: String,
    },
    CreateInvitation {
        target: crate::InvitationTarget,
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum ServerEvent {
    Hello {
        participant_id: crate::UserId,
        /// Private to the authenticated participant; never included in profiles.
        signup_ordinal: Option<u64>,
    },
    UsersListed {
        users: Vec<crate::UserProfile>,
    },
    CircleUsersListed {
        circle_id: crate::CircleId,
        users: Vec<crate::UserProfile>,
    },
    StatusUpdated {
        profile: crate::UserProfile,
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
        users: Vec<crate::UserProfile>,
    },
    ChannelDescriptionUpdated {
        channel_id: ChannelId,
        description: String,
    },
    JoinableChannelsListed {
        channels: Vec<crate::DiscoverableChannel>,
    },
    ChannelMemberAdded {
        membership: Membership,
    },
    MessagesLoaded {
        channel_id: ChannelId,
        messages: Vec<ChatMessage>,
    },
    ThreadLoaded {
        root_message_id: crate::MessageId,
        messages: Vec<ChatMessage>,
    },
    ThreadSummariesListed {
        channel_id: ChannelId,
        summaries: Vec<crate::ThreadSummary>,
    },
    ThreadReadUpdated {
        summary: crate::ThreadSummary,
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
        reactions: Vec<crate::MessageReactionSummary>,
    },
    MessageReactionChanged {
        change: crate::MessageReactionChange,
    },
    ReadMarkerUpdated {
        membership: Membership,
    },
    MentionsListed {
        mentions: Vec<crate::InboxMention>,
    },
    MentionRead {
        message_id: crate::MessageId,
    },
    TaskCreated {
        task: crate::UserTask,
    },
    TasksListed {
        tasks: Vec<crate::UserTask>,
    },
    TaskUpdated {
        task: crate::UserTask,
    },
    Chat {
        event: ChatEvent,
    },
    Lagged {
        channel_id: ChannelId,
        last_seen_sequence: ChannelSequence,
        latest_known_sequence: ChannelSequence,
        skipped: u64,
        hint: String,
    },
    Pong,
    CircleCreated {
        circle: crate::Circle,
    },
    CirclesListed {
        circles: Vec<(crate::Circle, crate::CircleRole)>,
    },
    CircleDeleted {
        circle_id: crate::CircleId,
    },
    CircleLeft {
        circle_id: crate::CircleId,
    },
    CircleInvitationCreated {
        invitation: crate::IssuedInvitation,
    },
    CircleInvitationAccepted {
        membership: crate::CircleMembership,
    },
    InvitationCreated {
        invitation: crate::IssuedChatInvitation,
    },
    InvitationInspected {
        token: String,
        invitation: crate::InvitationPreview,
    },
    InvitationDeclined {
        token: String,
        invitation: crate::InvitationPreview,
    },
    InvitationAccepted {
        token: String,
        invitation: crate::AcceptedChatInvitation,
    },
    Error {
        code: String,
        message: String,
    },
}
