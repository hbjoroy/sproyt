//! M0 contracts only. Collection, storage and model workers are introduced in
//! later stages, after the user's inspection and forgetting controls exist.
#![allow(dead_code)]

pub(crate) mod repository;

use std::collections::HashSet;

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{agent::ActivityProvenance, domain::PrincipalKind};

pub(super) const MAX_BATCH_MESSAGES: usize = 20;
pub(super) const MAX_NEIGHBOUR_MESSAGES: usize = 10;
pub(crate) const MAX_PROFILE_NOTES: usize = 24;
pub(super) const MAX_NOTE_BYTES: usize = 1_024;
pub(super) const MAX_PROFILE_NOTE_BYTES: usize = 16 * 1_024;
pub(super) const MAX_PROFILE_SOURCE_REFERENCES: usize = 720;
pub(super) const MAX_PROFILE_EXCLUSIONS: usize = 1_024;
pub(super) const MAX_PROFILE_SCOPES: usize = 64;
// Bounds the serialized contract; this is not a PostgreSQL disk-size promise.
pub(super) const MAX_PROFILE_SERIALIZED_BYTES: usize = 256 * 1_024;
pub(super) const MAX_MODEL_INPUT_BYTES: usize = 24 * 1_024;
pub(super) const MAX_MODEL_OUTPUT_TOKENS: u32 = 600;
pub(super) const MAX_CONCURRENT_MODEL_LEASES: usize = 1;
pub(super) const MAX_NEW_MODEL_CALLS_PER_MINUTE: usize = 2;

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentMemoryConfig {
    pub enabled: bool,
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum UserMemoryChoice {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub(super) struct MemoryGates {
    pub collect: bool,
    pub build: bool,
    pub use_in_replies: bool,
}

impl MemoryGates {
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        Self {
            collect: lookup("SPROYT_CHAT_AGENT_MEMORY_COLLECT_ENABLED").as_deref() == Some("true"),
            build: lookup("SPROYT_CHAT_AGENT_MEMORY_BUILD_ENABLED").as_deref() == Some("true"),
            use_in_replies: lookup("SPROYT_CHAT_AGENT_MEMORY_USE_ENABLED").as_deref()
                == Some("true"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryOwner {
    pub circle_id: Uuid,
    pub agent_id: Uuid,
    pub user_id: Uuid,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryScope {
    pub owner: MemoryOwner,
    pub channel_id: Uuid,
}

/// A privacy fence, distinct from routine learning/content revisions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "i64", into = "i64")]
pub(super) struct MemoryEpoch(i64);

impl TryFrom<i64> for MemoryEpoch {
    type Error = &'static str;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value > 0 {
            Ok(Self(value))
        } else {
            Err("invalid_memory_epoch")
        }
    }
}

impl From<MemoryEpoch> for i64 {
    fn from(value: MemoryEpoch) -> Self {
        value.0
    }
}

impl MemoryEpoch {
    pub fn next(self) -> Result<Self, &'static str> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or("memory_epoch_exhausted")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct SourceVersion(String);

impl TryFrom<String> for SourceVersion {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            Ok(Self(value))
        } else {
            Err("invalid_memory_source_version")
        }
    }
}

impl From<SourceVersion> for String {
    fn from(value: SourceVersion) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceReference {
    pub message_id: Uuid,
    pub version: SourceVersion,
}

/// Database-authored attribution. Display names are deliberately excluded.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceMetadata {
    pub message_id: Uuid,
    pub circle_id: Option<Uuid>,
    pub channel_id: Uuid,
    pub sender_id: Uuid,
    pub sender_kind: Option<PrincipalKind>,
    pub provenance: Option<ActivityProvenance>,
    pub parent_message_id: Option<Uuid>,
    pub sequence: u64,
    #[serde(deserialize_with = "database_timestamp")]
    pub created_at: DateTime<Utc>,
    #[serde(default, deserialize_with = "optional_database_timestamp")]
    pub edited_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "optional_database_timestamp")]
    pub deleted_at: Option<DateTime<Utc>>,
    // Never accept a caller's claimed digest. Rust seals the actual raw body.
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub version: Option<SourceVersion>,
}

fn parse_timestamp(value: &str) -> Result<DateTime<Utc>, &'static str> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .or_else(|_| {
            // SQLite CURRENT_TIMESTAMP is UTC but has no offset. Preserve its
            // precision instead of rounding through strftime in the query.
            NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f").map(|time| time.and_utc())
        })
        .map_err(|_| "invalid_memory_source_timestamp")
}

fn database_timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<DateTime<Utc>, D::Error> {
    let value = String::deserialize(deserializer)?;
    parse_timestamp(&value).map_err(serde::de::Error::custom)
}

fn optional_database_timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<DateTime<Utc>>, D::Error> {
    Option::<String>::deserialize(deserializer)?
        .map(|value| parse_timestamp(&value).map_err(serde::de::Error::custom))
        .transpose()
}

impl SourceMetadata {
    pub fn seal(&mut self, raw_body: &str) -> Result<(), serde_json::Error> {
        let bytes = serde_json::to_vec(&(
            self.message_id,
            self.circle_id,
            self.channel_id,
            self.sender_id,
            &self.sender_kind,
            self.provenance,
            self.parent_message_id,
            self.sequence,
            self.created_at,
            self.edited_at,
            self.deleted_at,
            raw_body,
        ))?;
        self.version = Some(SourceVersion(format!("{:x}", Sha256::digest(bytes))));
        Ok(())
    }

    pub fn is_human_evidence(&self) -> bool {
        self.sender_kind == Some(PrincipalKind::Human)
            && self.provenance == Some(ActivityProvenance::Human)
            && self.deleted_at.is_none()
            && self.sequence > 0
            && self.version.is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoteKind {
    Preference,
    TemporaryContext,
    Interaction,
}

impl NoteKind {
    /// Provisional pilot retention; only the server assigns the deadline.
    pub fn default_lifetime_days(self) -> Option<u32> {
        match self {
            Self::Preference => None,
            Self::TemporaryContext => Some(7),
            Self::Interaction => Some(90),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoteOrigin {
    Automatic,
    User,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EvidenceKind {
    UserStated,
    ConversationEvent,
    UserConfirmed,
}

// Avoid Debug for free text: these values must not enter routine logs.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub(crate) struct NoteText(String);

impl TryFrom<String> for NoteText {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let value = value.trim();
        if value.is_empty()
            || value.len() > MAX_NOTE_BYTES
            || value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            Err("invalid_memory_note_text")
        } else {
            Ok(Self(value.to_owned()))
        }
    }
}

impl From<NoteText> for String {
    fn from(value: NoteText) -> Self {
        value.0
    }
}

/// Proposed model output. Ownership, confirmation, expiry and participants are
/// supplied by the server, not fields the model is permitted to choose.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MemoryCandidate {
    pub kind: NoteKind,
    pub text: NoteText,
    pub source_message_ids: Vec<Uuid>,
}

impl MemoryCandidate {
    pub fn validate_sources(
        &self,
        scope: &MemoryScope,
        supplied: &[SourceMetadata],
    ) -> Result<(), &'static str> {
        if self.source_message_ids.is_empty()
            || self.source_message_ids.len() > MAX_BATCH_MESSAGES + MAX_NEIGHBOUR_MESSAGES
        {
            return Err("invalid_memory_sources");
        }
        let mut unique = HashSet::new();
        let mut has_owner = false;
        for id in &self.source_message_ids {
            if !unique.insert(id) {
                return Err("duplicate_memory_source");
            }
            let source = supplied
                .iter()
                .find(|source| source.message_id == *id)
                .ok_or("unknown_memory_source")?;
            if source.circle_id != Some(scope.owner.circle_id)
                || source.channel_id != scope.channel_id
                || !source.is_human_evidence()
            {
                return Err("invalid_memory_source_scope");
            }
            has_owner |= source.sender_id == scope.owner.user_id;
        }
        if !has_owner {
            return Err("missing_memory_owner_evidence");
        }
        // This validates references, not semantic truth. Later admission must
        // conservatively retain all relevant supplied context dependencies.
        Ok(())
    }
}

#[derive(Clone, Copy, Default, Debug, Eq, PartialEq)]
pub(super) struct MemoryUsage {
    pub notes: usize,
    pub note_bytes: usize,
    pub source_references: usize,
    pub exclusions: usize,
    pub scopes: usize,
    pub serialized_bytes: usize,
}

impl MemoryUsage {
    pub fn within_limits(&self) -> bool {
        self.notes <= MAX_PROFILE_NOTES
            && self.note_bytes <= MAX_PROFILE_NOTE_BYTES
            && self.source_references <= MAX_PROFILE_SOURCE_REFERENCES
            && self.exclusions <= MAX_PROFILE_EXCLUSIONS
            && self.scopes <= MAX_PROFILE_SCOPES
            && self.serialized_bytes <= MAX_PROFILE_SERIALIZED_BYTES
    }
}

#[cfg(test)]
mod tests;
