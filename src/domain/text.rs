use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextValidationError {
    Empty { field: &'static str },
    InvalidSlug,
    TooLarge { field: &'static str, max: usize },
}

impl fmt::Display for TextValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { field } => write!(formatter, "{field} cannot be empty"),
            Self::InvalidSlug => write!(
                formatter,
                "channel slug can only contain lowercase letters, numbers, '-' and '_'"
            ),
            Self::TooLarge { field, max } => write!(formatter, "{field} cannot exceed {max} bytes"),
        }
    }
}

impl std::error::Error for TextValidationError {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageBody(String);

impl MessageBody {
    const MAX_BYTES: usize = 64 * 1024;

    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        bounded_non_empty(value, "message body", Self::MAX_BYTES).map(Self)
    }
}

impl fmt::Display for MessageBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChannelSlug(String);

impl ChannelSlug {
    const MAX_BYTES: usize = 80;

    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        let value = bounded_non_empty(value, "channel slug", Self::MAX_BYTES)?;
        let normalized = value.to_lowercase();
        let is_valid = normalized.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        });
        if !is_valid {
            return Err(TextValidationError::InvalidSlug);
        }
        Ok(Self(normalized))
    }
}

impl fmt::Display for ChannelSlug {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DisplayName(String);

impl DisplayName {
    const MAX_BYTES: usize = 120;

    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        bounded_non_empty(value, "display name", Self::MAX_BYTES).map(Self)
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn bounded_non_empty(
    value: impl Into<String>,
    field: &'static str,
    max: usize,
) -> Result<String, TextValidationError> {
    let value = value.into();
    let value = value.trim();
    if value.is_empty() {
        return Err(TextValidationError::Empty { field });
    }
    if value.len() > max {
        return Err(TextValidationError::TooLarge { field, max });
    }
    Ok(value.to_owned())
}
