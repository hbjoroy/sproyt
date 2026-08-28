//! Deterministic client-side transport decisions.
//!
//! TypeScript owns WebSocket, IndexedDB, timers and the DOM; this core decides
//! whether an already durable send can safely be dispatched now. That keeps it
//! native-testable and WASM-compatible without retaining browser handles.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransportReadiness<'a> {
    pub connected: bool,
    pub subscribed_channel_id: Option<&'a str>,
    pub handoff_active: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SendAdmission {
    DispatchNow,
    QueueUntilSubscribed,
}

/// Decide after persistence, closing the race where the socket changes while
/// an IndexedDB write is resolving.
pub fn admit_persisted_send(channel_id: &str, readiness: TransportReadiness<'_>) -> SendAdmission {
    if readiness.connected
        && !readiness.handoff_active
        && readiness.subscribed_channel_id == Some(channel_id)
    {
        SendAdmission::DispatchNow
    } else {
        SendAdmission::QueueUntilSubscribed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSendState {
    Persisting,
    Queued,
    Dispatching,
    Uncertain,
    Acknowledged,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSendEvent {
    Persisted(SendAdmission),
    DispatchFailed,
    SocketLost,
    Acknowledged,
    Rejected,
}

/// Transport failure retains the idempotency key for a later replay.
pub fn transition(state: DurableSendState, event: DurableSendEvent) -> DurableSendState {
    match (state, event) {
        (DurableSendState::Persisting, DurableSendEvent::Persisted(SendAdmission::DispatchNow))
        | (DurableSendState::Queued, DurableSendEvent::Persisted(SendAdmission::DispatchNow)) => {
            DurableSendState::Dispatching
        }
        (
            DurableSendState::Persisting,
            DurableSendEvent::Persisted(SendAdmission::QueueUntilSubscribed),
        )
        | (
            DurableSendState::Dispatching,
            DurableSendEvent::DispatchFailed | DurableSendEvent::SocketLost,
        )
        | (
            DurableSendState::Uncertain,
            DurableSendEvent::DispatchFailed | DurableSendEvent::SocketLost,
        ) => DurableSendState::Queued,
        (
            DurableSendState::Dispatching | DurableSendState::Uncertain,
            DurableSendEvent::Acknowledged,
        ) => DurableSendState::Acknowledged,
        (
            DurableSendState::Dispatching | DurableSendState::Uncertain,
            DurableSendEvent::Rejected,
        ) => DurableSendState::Rejected,
        _ => state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Scenario {
        channel_id: String,
        connected: bool,
        subscribed_channel_id: Option<String>,
        handoff_active: bool,
        admission: String,
    }

    #[test]
    fn admission_requires_the_current_subscription() {
        let ready = TransportReadiness {
            connected: true,
            subscribed_channel_id: Some("circle-prat"),
            handoff_active: false,
        };
        assert_eq!(
            admit_persisted_send("circle-prat", ready),
            SendAdmission::DispatchNow
        );
        assert_eq!(
            admit_persisted_send("other", ready),
            SendAdmission::QueueUntilSubscribed
        );
        assert_eq!(
            admit_persisted_send(
                "circle-prat",
                TransportReadiness {
                    connected: false,
                    subscribed_channel_id: Some("circle-prat"),
                    handoff_active: false,
                }
            ),
            SendAdmission::QueueUntilSubscribed
        );
    }

    #[test]
    fn shared_admission_scenarios_stay_in_sync_with_the_browser_adapter() {
        let scenarios: Vec<Scenario> = serde_json::from_str(include_str!(
            "../../../frontend/tests/fixtures/durable-send-admission.json"
        ))
        .expect("shared send-admission fixture is valid JSON");
        for scenario in scenarios {
            let actual = admit_persisted_send(
                &scenario.channel_id,
                TransportReadiness {
                    connected: scenario.connected,
                    subscribed_channel_id: scenario.subscribed_channel_id.as_deref(),
                    handoff_active: scenario.handoff_active,
                },
            );
            let expected = match scenario.admission.as_str() {
                "dispatch_now" => SendAdmission::DispatchNow,
                "queue_until_subscribed" => SendAdmission::QueueUntilSubscribed,
                unexpected => panic!("unexpected fixture admission {unexpected}"),
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn socket_change_after_persistence_stays_replayable() {
        let state = transition(
            DurableSendState::Persisting,
            DurableSendEvent::Persisted(SendAdmission::DispatchNow),
        );
        assert_eq!(
            transition(state, DurableSendEvent::DispatchFailed),
            DurableSendState::Queued
        );
    }

    #[test]
    fn receipt_and_rejection_are_terminal() {
        assert_eq!(
            transition(
                DurableSendState::Dispatching,
                DurableSendEvent::Acknowledged
            ),
            DurableSendState::Acknowledged
        );
        assert_eq!(
            transition(DurableSendState::Dispatching, DurableSendEvent::Rejected),
            DurableSendState::Rejected
        );
        assert_eq!(
            transition(DurableSendState::Acknowledged, DurableSendEvent::SocketLost),
            DurableSendState::Acknowledged
        );
    }
}
