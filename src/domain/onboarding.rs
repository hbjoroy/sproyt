use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{CircleId, UserId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EnrollmentInvitationState {
    Inactive,
    Active,
    // Production repositories consume this state atomically in SQL and return
    // the resulting membership. The in-memory contract model materialises it.
    #[cfg_attr(not(test), allow(dead_code))]
    Consumed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnrollmentInvitation {
    pub id: Uuid,
    pub circle_id: CircleId,
    pub invited_by: UserId,
    pub expires_at: DateTime<Utc>,
    pub state: EnrollmentInvitationState,
    pub authentik_invitation_id: Option<Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareEnrollmentInvitation {
    pub actor: UserId,
    pub circle_id: CircleId,
    pub email: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedEnrollmentInvitation {
    pub invitation: EnrollmentInvitation,
    pub token: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivateEnrollmentInvitation {
    pub token: String,
    pub authentik_invitation_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AcceptEnrollmentInvitation {
    pub actor: UserId,
    pub email: String,
    pub token: String,
}

pub(crate) fn generate_enrollment_token() -> Result<String, getrandom::Error> {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret)?;
    Ok(URL_SAFE_NO_PAD.encode(secret))
}

pub(crate) fn enrollment_token_hash(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub(crate) fn enrollment_email_hash(email: &str) -> Vec<u8> {
    let normalized = email.trim().to_lowercase();
    Sha256::digest(normalized.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_hash_is_trimmed_and_lowercase() {
        assert_eq!(
            enrollment_email_hash("  Person@Example.COM\t"),
            enrollment_email_hash("person@example.com")
        );
        assert_ne!(
            enrollment_email_hash("person@example.com"),
            enrollment_email_hash("other@example.com")
        );
    }

    #[test]
    fn generated_token_has_32_bytes_of_url_safe_entropy() {
        let token = generate_enrollment_token().unwrap();
        let decoded = URL_SAFE_NO_PAD.decode(token.as_bytes()).unwrap();
        assert_eq!(decoded.len(), 32);
        assert!(
            !token
                .chars()
                .any(|character| matches!(character, '+' | '/' | '='))
        );
    }
}
