use serde::{Deserialize, Serialize};

use super::{
    ChannelId, ChannelKind, ChannelSequence, ChannelSlug, CircleId, DisplayName, MembershipRole,
    MessageBody, UserId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateChannel {
    pub actor: UserId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
    pub circle_id: Option<CircleId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateCircle {
    pub actor: UserId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DeleteCircle {
    pub actor: UserId,
    pub circle_id: CircleId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CreateCircleInvitation {
    pub actor: UserId,
    pub circle_id: CircleId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AcceptCircleInvitation {
    pub actor: UserId,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct JoinChannel {
    pub actor: UserId,
    pub channel: ChannelRef,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AddChannelMember {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LeaveChannel {
    pub actor: UserId,
    pub channel_id: ChannelId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LoadRecentMessages {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub limit: MessageLimit,
    pub after: Option<ChannelSequence>,
    pub before: Option<ChannelSequence>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SendMessage {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub body: MessageBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EditMessage {
    pub actor: UserId,
    pub message_id: super::MessageId,
    pub body: MessageBody,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MarkRead {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub sequence: ChannelSequence,
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
