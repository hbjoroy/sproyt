use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextValidationError {
    Empty { field: &'static str },
    InvalidSlug,
    InvalidHandle,
    InvalidSequence,
    InvalidReaction,
    InvalidCircleName,
    SequenceOverflow,
    InvalidUuid { field: &'static str },
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
            Self::InvalidHandle => {
                formatter.write_str("handle can only contain letters, numbers, '.', '-' and '_'")
            }
            Self::InvalidSequence => formatter.write_str("channel sequence cannot be negative"),
            Self::InvalidCircleName => {
                formatter.write_str("circle name must contain 1 to 120 characters")
            }
            Self::InvalidReaction => formatter.write_str("reaction emoji is not supported"),
            Self::SequenceOverflow => formatter.write_str("channel sequence is exhausted"),
            Self::InvalidUuid { field } => write!(formatter, "{field} must be a UUID"),
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
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

    pub fn as_str(&self) -> &str {
        &self.0
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
    /// Circle labels allow 120 Unicode characters, independently of user-name byte limits.
    pub fn circle_name(value: impl Into<String>) -> Result<Self, TextValidationError> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() || value.chars().count() > 120 {
            return Err(TextValidationError::InvalidCircleName);
        }
        Ok(Self(value.to_owned()))
    }

    const MAX_BYTES: usize = 120;

    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        bounded_non_empty(value, "display name", Self::MAX_BYTES).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// A stable public address. The leading `@` is presentation, not storage.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Handle(String);

impl Handle {
    const MAX_BYTES: usize = 80;

    pub fn with_suffix(&self, ordinal: u32) -> Self {
        if ordinal <= 1 {
            return self.clone();
        }
        let suffix = format!("-{ordinal}");
        let limit = Self::MAX_BYTES.saturating_sub(suffix.len());
        let mut base = String::new();
        for character in self.0.chars() {
            if base.len() + character.len_utf8() > limit {
                break;
            }
            base.push(character);
        }
        Self(format!("{base}{suffix}"))
    }

    pub fn new(value: impl Into<String>) -> Result<Self, TextValidationError> {
        let value = bounded_non_empty(value, "handle", Self::MAX_BYTES)?;
        let normalized = value.to_lowercase();
        if !normalized
            .chars()
            .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-' | '.'))
        {
            return Err(TextValidationError::InvalidHandle);
        }
        Ok(Self(normalized))
    }

    /// Canonicalize a verified external username without making authentication
    /// depend on the provider's punctuation policy.
    pub fn from_external(value: &str) -> Self {
        let mut normalized = String::new();
        for character in value.trim().to_lowercase().chars() {
            if !(character.is_alphanumeric() || matches!(character, '_' | '-' | '.')) {
                continue;
            }
            if normalized.len() + character.len_utf8() > Self::MAX_BYTES {
                break;
            }
            normalized.push(character);
        }
        Self(if normalized.is_empty() {
            "user".to_owned()
        } else {
            normalized
        })
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Handle {
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

#[cfg(test)]
mod circle_name_tests {
    use super::*;
    #[test]
    fn circle_names_are_trimmed_unicode_characters_without_changing_user_limits() {
        assert_eq!(DisplayName::circle_name("  Ω  ").unwrap().as_str(), "Ω");
        assert!(DisplayName::circle_name("Ω".repeat(120)).is_ok());
        for invalid in [
            "".to_owned(),
            " \t\n".to_owned(),
            "a".repeat(121),
            "Ω".repeat(121),
        ] {
            assert_eq!(
                DisplayName::circle_name(invalid),
                Err(TextValidationError::InvalidCircleName)
            );
        }
        assert!(DisplayName::new("Ω".repeat(120)).is_err());
    }
}
