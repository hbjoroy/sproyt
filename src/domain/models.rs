use serde::{Deserialize, Serialize};

use super::{ChannelId, ChannelSequence, ChannelSlug, DisplayName, MessageBody, MessageId, UserId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Public,
    Local,
    Private,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipRole {
    Owner,
    Moderator,
    Member,
    Observer,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
    pub created_by: UserId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelSummary {
    pub id: ChannelId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
    pub role: MembershipRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub role: MembershipRole,
    pub last_read_message_id: Option<MessageId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub sender_id: UserId,
    pub body: MessageBody,
    pub sequence: ChannelSequence,
    pub sent_at_unix_ms: u128,
}
