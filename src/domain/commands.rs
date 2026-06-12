use serde::{Deserialize, Serialize};

use super::{
    ChannelId, ChannelKind, ChannelSequence, ChannelSlug, DisplayName, MembershipRole, MessageBody,
    MessageId, UserId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatCommand {
    CreateChannel(CreateChannel),
    JoinChannel(JoinChannel),
    LeaveChannel(LeaveChannel),
    ListMyChannels(ListMyChannels),
    LoadRecentMessages(LoadRecentMessages),
    SendMessage(SendMessage),
    MarkRead(MarkRead),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateChannel {
    pub actor: UserId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JoinChannel {
    pub actor: UserId,
    pub channel: ChannelRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LeaveChannel {
    pub actor: UserId,
    pub channel_id: ChannelId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ListMyChannels {
    pub actor: UserId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadRecentMessages {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub limit: MessageLimit,
    pub after: Option<ChannelSequence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SendMessage {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub body: MessageBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MarkRead {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum ChannelRef {
    Id(ChannelId),
    Slug(ChannelSlug),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageLimit(u16);

impl MessageLimit {
    pub const DEFAULT: Self = Self(50);
    pub const MAX: u16 = 200;

    pub const fn new(value: u16) -> Self {
        if value > Self::MAX {
            Self(Self::MAX)
        } else {
            Self(value)
        }
    }
}

impl From<MessageLimit> for usize {
    fn from(value: MessageLimit) -> Self {
        usize::from(value.0)
    }
}

impl From<MembershipRole> for &'static str {
    fn from(value: MembershipRole) -> Self {
        match value {
            MembershipRole::Owner => "owner",
            MembershipRole::Moderator => "moderator",
            MembershipRole::Member => "member",
            MembershipRole::Observer => "observer",
        }
    }
}
