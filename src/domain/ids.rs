use std::fmt;

use serde::{Deserialize, Serialize};

use super::TextValidationError;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(String);

impl UserId {
    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        non_empty(value, "user id").map(Self)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelId(String);

impl ChannelId {
    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        non_empty(value, "channel id").map(Self)
    }
}

impl fmt::Display for ChannelId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(u64);

impl MessageId {
    pub const fn new(value: u64) -> Self {
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

fn non_empty(value: impl Into<String>, field: &'static str) -> Result<String, TextValidationError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(TextValidationError::Empty { field });
    }
    Ok(value.to_owned())
}
