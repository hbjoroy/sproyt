//! Pure session refresh and recovery policy.
//!
//! The browser adapter performs HTTP, locks, storage, timers and navigation.
//! This module only classifies their outcomes so that a suspended PWA cannot
//! accidentally turn a transient failure into a destructive reload or login.

/// Result of refreshing the browser session and subsequently verifying it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshOutcome {
    /// Refresh and verification succeeded.
    Rotated,
    /// Network, timeout, malformed payload, or a non-authentication response.
    RetryableFailure,
    /// Refresh or verification explicitly rejected the current credentials.
    AuthenticationRejected,
}

/// Result of asking `/auth/session` whether another browser context rotated it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurrentSession {
    /// The probe has not run yet.
    NotChecked,
    /// A current session exists, so no interactive reauthentication is needed.
    Valid,
    /// The server explicitly rejected the current session.
    AuthenticationRejected,
    /// Network, timeout, malformed payload, or another transient response.
    RetryableFailure,
}

/// What the browser should do directly after a refresh attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshDisposition {
    /// Use the supplied server refresh deadline.
    ScheduleServerDeadline,
    /// Keep the app usable and retry using its short backoff.
    RetryShortly,
    /// Retry shortly, remembering that authentication was explicitly rejected.
    RetryAfterAuthenticationRejection,
}

/// Classify a refresh attempt without deciding how the browser schedules time.
pub const fn refresh_disposition(outcome: RefreshOutcome) -> RefreshDisposition {
    match outcome {
        RefreshOutcome::Rotated => RefreshDisposition::ScheduleServerDeadline,
        RefreshOutcome::RetryableFailure => RefreshDisposition::RetryShortly,
        RefreshOutcome::AuthenticationRejected => {
            RefreshDisposition::RetryAfterAuthenticationRejection
        }
    }
}

/// Next action when foreground activity needs an authenticated session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryDecision {
    /// The refresh already restored the session.
    SessionRotated,
    /// Preserve the view and wait for network recovery before trying again.
    WaitForNetwork,
    /// A rejected refresh must be followed by a current-session probe.
    ProbeCurrentSession,
    /// Another browser context already has a valid session.
    UseCurrentSession,
    /// The user is active: explain the state and defer interactive login.
    WaitForReauthentication,
    /// It is safe to send the user to the login flow.
    RequireLogin,
}

/// Decide recovery after a refresh, including the drowsy-PWA case.
///
/// `foreground` and `recently_active` are owned by the adapter. A rejected
/// refresh must never log out an active foreground user before it has probed
/// the current session and offered a short recovery window.
pub const fn recovery_decision(
    refresh: RefreshOutcome,
    current_session: CurrentSession,
    foreground: bool,
    recently_active: bool,
) -> RecoveryDecision {
    match refresh {
        RefreshOutcome::Rotated => RecoveryDecision::SessionRotated,
        RefreshOutcome::RetryableFailure => RecoveryDecision::WaitForNetwork,
        RefreshOutcome::AuthenticationRejected => match current_session {
            CurrentSession::NotChecked => RecoveryDecision::ProbeCurrentSession,
            CurrentSession::Valid => RecoveryDecision::UseCurrentSession,
            CurrentSession::RetryableFailure => RecoveryDecision::WaitForNetwork,
            CurrentSession::AuthenticationRejected if foreground && recently_active => {
                RecoveryDecision::WaitForReauthentication
            }
            CurrentSession::AuthenticationRejected => RecoveryDecision::RequireLogin,
        },
    }
}

/// Result of the initial `/auth/session` probe when a page is opened or woken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionProbe {
    Valid,
    AuthenticationRejected,
    RetryableFailure,
}

/// Initial action after checking the existing session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartDecision {
    /// Schedule using the server's refresh deadline.
    ScheduleServerDeadline,
    /// Try a refresh while retaining the current UI.
    RefreshNow,
    /// Keep the UI and retry after the adapter's short backoff.
    RetryShortly,
}

pub const fn start_decision(probe: SessionProbe) -> StartDecision {
    match probe {
        SessionProbe::Valid => StartDecision::ScheduleServerDeadline,
        SessionProbe::AuthenticationRejected => StartDecision::RefreshNow,
        SessionProbe::RetryableFailure => StartDecision::RetryShortly,
    }
}

/// Stable integer ABI for [`RefreshOutcome`].
///
/// `0 = rotated`, `1 = retryable failure`, `2 = authentication rejected`.
pub const fn refresh_outcome_from_abi(value: u32) -> RefreshOutcome {
    match value {
        0 => RefreshOutcome::Rotated,
        2 => RefreshOutcome::AuthenticationRejected,
        _ => RefreshOutcome::RetryableFailure,
    }
}

const fn refresh_disposition_to_abi(value: RefreshDisposition) -> u32 {
    match value {
        RefreshDisposition::ScheduleServerDeadline => 0,
        RefreshDisposition::RetryShortly => 1,
        RefreshDisposition::RetryAfterAuthenticationRejection => 2,
    }
}

const fn current_session_from_abi(value: u32) -> CurrentSession {
    match value {
        1 => CurrentSession::Valid,
        2 => CurrentSession::AuthenticationRejected,
        3 => CurrentSession::RetryableFailure,
        _ => CurrentSession::NotChecked,
    }
}

const fn recovery_decision_to_abi(value: RecoveryDecision) -> u32 {
    match value {
        RecoveryDecision::SessionRotated => 0,
        RecoveryDecision::WaitForNetwork => 1,
        RecoveryDecision::ProbeCurrentSession => 2,
        RecoveryDecision::UseCurrentSession => 3,
        RecoveryDecision::WaitForReauthentication => 4,
        RecoveryDecision::RequireLogin => 5,
    }
}

const fn start_decision_to_abi(value: StartDecision) -> u32 {
    match value {
        StartDecision::ScheduleServerDeadline => 0,
        StartDecision::RefreshNow => 1,
        StartDecision::RetryShortly => 2,
    }
}

/// Raw WASM ABI for [`refresh_disposition`].
///
/// Outputs `0 = server deadline`, `1 = short retry`, `2 = rejected short retry`.
#[unsafe(no_mangle)]
pub extern "C" fn sproyt_refresh_disposition(refresh_outcome: u32) -> u32 {
    refresh_disposition_to_abi(refresh_disposition(refresh_outcome_from_abi(
        refresh_outcome,
    )))
}

/// Raw WASM ABI for [`recovery_decision`].
///
/// `current_session` is `0 = not checked`, `1 = valid`, `2 = rejected`,
/// `3 = retryable failure`. Outputs `0 = rotated`, `1 = wait for network`,
/// `2 = probe current session`, `3 = use current session`,
/// `4 = wait for reauthentication`, `5 = require login`.
#[unsafe(no_mangle)]
pub extern "C" fn sproyt_recovery_decision(
    refresh_outcome: u32,
    current_session: u32,
    foreground: u32,
    recently_active: u32,
) -> u32 {
    recovery_decision_to_abi(recovery_decision(
        refresh_outcome_from_abi(refresh_outcome),
        current_session_from_abi(current_session),
        foreground != 0,
        recently_active != 0,
    ))
}

/// Raw WASM ABI for [`start_decision`].
///
/// Probe input matches [`RefreshOutcome`]'s integer mapping. Output is
/// `0 = server deadline`, `1 = refresh now`, `2 = short retry`.
#[unsafe(no_mangle)]
pub extern "C" fn sproyt_start_session_decision(probe: u32) -> u32 {
    let probe = match refresh_outcome_from_abi(probe) {
        RefreshOutcome::Rotated => SessionProbe::Valid,
        RefreshOutcome::AuthenticationRejected => SessionProbe::AuthenticationRejected,
        RefreshOutcome::RetryableFailure => SessionProbe::RetryableFailure,
    };
    start_decision_to_abi(start_decision(probe))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_outcomes_preserve_the_existing_backoff_policy() {
        assert_eq!(
            refresh_disposition(RefreshOutcome::Rotated),
            RefreshDisposition::ScheduleServerDeadline
        );
        assert_eq!(
            refresh_disposition(RefreshOutcome::RetryableFailure),
            RefreshDisposition::RetryShortly
        );
        assert_eq!(
            refresh_disposition(RefreshOutcome::AuthenticationRejected),
            RefreshDisposition::RetryAfterAuthenticationRejection
        );
    }

    #[test]
    fn network_failures_never_force_a_login() {
        assert_eq!(
            recovery_decision(
                RefreshOutcome::RetryableFailure,
                CurrentSession::NotChecked,
                true,
                true
            ),
            RecoveryDecision::WaitForNetwork
        );
    }

    #[test]
    fn rejected_refresh_can_reuse_a_rotated_session_from_another_context() {
        assert_eq!(
            recovery_decision(
                RefreshOutcome::AuthenticationRejected,
                CurrentSession::Valid,
                true,
                true
            ),
            RecoveryDecision::UseCurrentSession
        );
    }

    #[test]
    fn active_foreground_pwa_gets_a_reauthentication_grace_window() {
        assert_eq!(
            recovery_decision(
                RefreshOutcome::AuthenticationRejected,
                CurrentSession::AuthenticationRejected,
                true,
                true
            ),
            RecoveryDecision::WaitForReauthentication
        );
        assert_eq!(
            recovery_decision(
                RefreshOutcome::AuthenticationRejected,
                CurrentSession::AuthenticationRejected,
                false,
                true
            ),
            RecoveryDecision::RequireLogin
        );
    }

    #[test]
    fn current_session_network_failure_never_becomes_login() {
        assert_eq!(
            recovery_decision(
                RefreshOutcome::AuthenticationRejected,
                CurrentSession::RetryableFailure,
                false,
                false
            ),
            RecoveryDecision::WaitForNetwork
        );
        assert_eq!(
            recovery_decision(
                RefreshOutcome::AuthenticationRejected,
                CurrentSession::NotChecked,
                false,
                false
            ),
            RecoveryDecision::ProbeCurrentSession
        );
    }

    #[test]
    fn startup_keeps_the_view_and_recovers_in_place() {
        assert_eq!(
            start_decision(SessionProbe::Valid),
            StartDecision::ScheduleServerDeadline
        );
        assert_eq!(
            start_decision(SessionProbe::AuthenticationRejected),
            StartDecision::RefreshNow
        );
        assert_eq!(
            start_decision(SessionProbe::RetryableFailure),
            StartDecision::RetryShortly
        );
    }

    #[test]
    fn raw_wasm_session_abi_has_a_stable_integer_contract() {
        assert_eq!(sproyt_refresh_disposition(0), 0);
        assert_eq!(sproyt_refresh_disposition(1), 1);
        assert_eq!(sproyt_refresh_disposition(2), 2);
        assert_eq!(sproyt_recovery_decision(2, 0, 1, 1), 2);
        assert_eq!(sproyt_recovery_decision(2, 1, 1, 1), 3);
        assert_eq!(sproyt_recovery_decision(2, 2, 1, 1), 4);
        assert_eq!(sproyt_recovery_decision(2, 2, 0, 1), 5);
        assert_eq!(sproyt_recovery_decision(2, 3, 0, 0), 1);
        assert_eq!(sproyt_recovery_decision(1, 0, 1, 1), 1);
        assert_eq!(sproyt_start_session_decision(0), 0);
        assert_eq!(sproyt_start_session_decision(2), 1);
        assert_eq!(sproyt_start_session_decision(1), 2);
    }
}
