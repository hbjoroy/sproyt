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

    pub fn can_leave_channel(role: Option<&MembershipRole>) -> bool {
        role.is_some()
    }

    pub fn can_moderate_channel(role: Option<&MembershipRole>) -> bool {
        matches!(
            role,
            Some(MembershipRole::Owner | MembershipRole::Moderator)
        )
    }

    pub fn can_start_process(role: Option<&MembershipRole>) -> bool {
        Self::can_complete_process_work(role)
    }

    pub fn can_complete_process_work(role: Option<&MembershipRole>) -> bool {
        Self::can_send_to_channel(role)
    }

    pub fn can_invite_agent_to_channel(role: Option<&MembershipRole>) -> bool {
        Self::can_moderate_channel(role)
    }

    pub fn can_invite_agent_to_circle(role: Option<&CircleRole>) -> bool {
        Self::can_invite_to_circle(role)
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
        for role in [
            MembershipRole::Owner,
            MembershipRole::Moderator,
            MembershipRole::Member,
        ] {
            assert!(Policy::can_start_process(Some(&role)));
            assert!(Policy::can_complete_process_work(Some(&role)));
        }
        assert!(!Policy::can_start_process(Some(&MembershipRole::Observer)));
        assert!(Policy::can_invite_agent_to_channel(Some(
            &MembershipRole::Moderator
        )));
        assert!(!Policy::can_invite_agent_to_channel(Some(
            &MembershipRole::Member
        )));
        assert!(Policy::can_leave_channel(Some(&MembershipRole::Observer)));
        assert!(!Policy::can_leave_channel(None));
    }
}
