use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::TextValidationError;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(Uuid);

impl UserId {
    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        parse_uuid(value, "user id").map(Self)
    }

    pub fn named(value: impl AsRef<str>) -> Self {
        Self(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            value.as_ref().as_bytes(),
        ))
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(Uuid);

impl ChannelId {
    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        parse_uuid(value, "channel id").map(Self)
    }

    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn named(value: impl AsRef<str>) -> Self {
        Self(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            value.as_ref().as_bytes(),
        ))
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(Uuid);

impl MessageId {
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }

    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }

    pub const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelSequence(u64);

impl ChannelSequence {
    pub const fn first() -> Self {
        Self(1)
    }

    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

impl From<ChannelSequence> for u64 {
    fn from(value: ChannelSequence) -> Self {
        value.0
    }
}

impl TryFrom<i64> for ChannelSequence {
    type Error = TextValidationError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        u64::try_from(value)
            .map(Self)
            .map_err(|_| TextValidationError::InvalidSequence)
    }
}

fn non_empty(value: impl Into<String>, field: &'static str) -> Result<String, TextValidationError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(TextValidationError::Empty { field });
    }
    Ok(value.to_owned())
}

fn parse_uuid(value: impl Into<String>, field: &'static str) -> Result<Uuid, TextValidationError> {
    let value = non_empty(value, field)?;
    Uuid::parse_str(&value).map_err(|_| TextValidationError::InvalidUuid { field })
}
