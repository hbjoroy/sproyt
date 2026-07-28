use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    ChannelId, ChannelSequence, ChannelSlug, CircleId, DisplayName, InvitationId, MessageBody,
    MessageId, UserId,
};

pub const PORTABLE_USER_EXPORT_FORMAT: &str = "sproyt.user-export.v1";

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

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "human" => Some(Self::Human),
            "agent" => Some(Self::Agent),
            _ => None,
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
pub struct UserProfile {
    #[serde(flatten)]
    pub user: User,
    pub status_text: String,
    pub status_emoji: String,
    pub status_expires_at: Option<DateTime<Utc>>,
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
    #[serde(default)]
    pub direct_user_id: Option<UserId>,
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
    pub sender_display_name: DisplayName,
    pub body: MessageBody,
    pub sequence: ChannelSequence,
    pub sent_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageReactionSummary {
    pub message_id: MessageId,
    pub emoji: String,
    pub count: u32,
    pub reacted_by_me: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MessageReactionChange {
    pub message_id: MessageId,
    pub channel_id: ChannelId,
    pub user_id: UserId,
    pub emoji: String,
    pub added: bool,
    pub count: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MediaObject {
    pub id: crate::domain::MediaId,
    pub owner_id: UserId,
    pub channel_id: ChannelId,
    pub original_filename: String,
    pub content_type: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub alt_text: String,
    pub analysis_status: String,
    pub analysis_metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaVariant {
    pub content_type: String,
    pub width: u32,
    pub height: u32,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaUpload {
    pub actor: UserId,
    pub channel_id: ChannelId,
    pub filename: String,
    pub content_type: String,
    pub content: Vec<u8>,
    pub dimensions: Option<(u32, u32)>,
    pub preview: Option<MediaVariant>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct InboxMention {
    pub message: ChatMessage,
    pub channel_name: DisplayName,
    pub read: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UserTask {
    pub id: uuid::Uuid,
    pub source_message_id: MessageId,
    pub channel_id: ChannelId,
    pub channel_name: DisplayName,
    pub assignee_id: UserId,
    pub created_by: UserId,
    pub process_link_id: Option<uuid::Uuid>,
    pub title: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PortableUserExport {
    pub format: String,
    pub exported_at: DateTime<Utc>,
    pub user: User,
    pub circles: Vec<ExportedCircle>,
    pub channels: Vec<ExportedChannel>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExportedCircle {
    pub circle: Circle,
    pub role: CircleRole,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ExportedChannel {
    pub channel: ChannelSummary,
    pub messages: Vec<ChatMessage>,
}
