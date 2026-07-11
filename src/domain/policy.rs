use super::{CircleRole, MembershipRole};

pub struct Policy;

impl Policy {
    pub fn can_invite_to_circle(role: Option<&CircleRole>) -> bool {
        matches!(role, Some(CircleRole::Owner))
    }

    pub fn can_create_channel_in_circle(role: Option<&CircleRole>) -> bool {
        matches!(role, Some(CircleRole::Owner | CircleRole::Member))
    }

    pub fn can_read_channel(role: Option<&MembershipRole>) -> bool {
        role.is_some()
    }

    pub fn can_send_to_channel(role: Option<&MembershipRole>) -> bool {
        matches!(
            role,
            Some(MembershipRole::Owner | MembershipRole::Moderator | MembershipRole::Member)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_matrix_is_explicit() {
        assert!(Policy::can_invite_to_circle(Some(&CircleRole::Owner)));
        assert!(!Policy::can_invite_to_circle(Some(&CircleRole::Member)));
        assert!(Policy::can_create_channel_in_circle(Some(
            &CircleRole::Member
        )));
        assert!(Policy::can_read_channel(Some(&MembershipRole::Observer)));
        assert!(!Policy::can_send_to_channel(Some(
            &MembershipRole::Observer
        )));
        assert!(!Policy::can_read_channel(None));
    }
}
