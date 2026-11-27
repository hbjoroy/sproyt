pub(crate) mod direct_activity;
mod onboarding;
mod policy;
mod repository;
pub use onboarding::{
    AcceptEnrollmentInvitation, ActivateEnrollmentInvitation, EnrollmentInvitation,
    EnrollmentInvitationState, IssuedEnrollmentInvitation, PrepareEnrollmentInvitation,
};
pub(crate) use onboarding::{
    enrollment_email_hash, enrollment_token_hash, generate_enrollment_token,
};
pub use policy::Policy;
#[cfg(test)]
pub use repository::InMemoryChatRepository;
pub use repository::MEDIA_READ_BLOCK_BYTES;
pub use repository::{
    CHANNEL_REFERENCE_BATCH_MAX, ResolvedChannelReference, THREAD_ROOT_PREVIEW_BATCH_MAX,
    ThreadRootPreview,
};
pub use repository::{
    ChatRepository, PresenceLease, ProfileRepository, RepositoryError, RepositoryFuture,
};
pub(crate) use repository::{personal_channel, personal_link_body};
pub use sproyt_protocol::*;

/// Profile details available on demand to an authenticated viewer.
#[derive(serde::Serialize)]
pub struct PersonCard {
    pub id: UserId,
    pub display_name: DisplayName,
    pub handle: Option<Handle>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub signup_ordinal: Option<u64>,
    pub status_text: String,
    pub status_emoji: String,
    pub messages: Vec<ChatMessage>,
}

impl PersonCard {
    pub fn new(
        profile: UserProfile,
        signup_ordinal: Option<u64>,
        messages: Vec<ChatMessage>,
    ) -> Self {
        Self {
            id: profile.user.id,
            display_name: profile.user.display_name,
            handle: profile.user.handle,
            created_at: profile.user.created_at,
            signup_ordinal,
            status_text: profile.status_text,
            status_emoji: profile.status_emoji,
            messages,
        }
    }
}
