use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    ChannelId, ChannelSequence, ChannelSlug, CircleId, DisplayName, InvitationId, MessageBody,
    MessageId, UserId,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
}

impl PrincipalKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub kind: PrincipalKind,
    pub display_name: DisplayName,
    pub external_provider: Option<String>,
    pub external_subject: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Public,
    Local,
    Private,
}

impl ChannelKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Local => "local",
            Self::Private => "private",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "public" => Some(Self::Public),
            "local" => Some(Self::Local),
            "private" => Some(Self::Private),
            _ => None,
        }
    }
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
#[serde(rename_all = "snake_case")]
pub enum CircleRole {
    Owner,
    Member,
}

impl CircleRole {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "member" => Some(Self::Member),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub id: CircleId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub created_by: UserId,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CircleMembership {
    pub circle_id: CircleId,
    pub user_id: UserId,
    pub role: CircleRole,
    pub joined_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CircleInvitation {
    pub id: InvitationId,
    pub circle_id: CircleId,
    pub invited_by: UserId,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IssuedInvitation {
    pub invitation: CircleInvitation,
    pub token: String,
}

impl MembershipRole {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Moderator => "moderator",
            Self::Member => "member",
            Self::Observer => "observer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "owner" => Some(Self::Owner),
            "moderator" => Some(Self::Moderator),
            "member" => Some(Self::Member),
            "observer" => Some(Self::Observer),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
    pub circle_id: Option<CircleId>,
    pub created_by: UserId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelSummary {
    pub id: ChannelId,
    pub slug: ChannelSlug,
    pub name: DisplayName,
    pub kind: ChannelKind,
    pub circle_id: Option<CircleId>,
    pub role: MembershipRole,
    #[serde(default)]
    pub last_read_sequence: ChannelSequence,
    #[serde(default)]
    pub latest_sequence: ChannelSequence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Membership {
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub role: MembershipRole,
    pub last_read_sequence: ChannelSequence,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub sender_id: UserId,
    pub body: MessageBody,
    pub sequence: ChannelSequence,
    pub sent_at: DateTime<Utc>,
}
