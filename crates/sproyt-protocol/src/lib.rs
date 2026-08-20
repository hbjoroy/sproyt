//! Portable, dependency-light framing for the Sprøyt WebSocket protocol.
//!
//! Application commands and events deliberately remain generic here.  The
//! server can keep its validated domain types while a WASM client can use its
//! own DTOs without importing server-only dependencies.

use serde::{Deserialize, Serialize};

mod commands;
mod events;
mod ids;
mod models;
mod text;
mod wire;

pub use commands::*;
pub use events::ChatEvent;
pub use ids::{ChannelId, ChannelSequence, CircleId, InvitationId, MediaId, MessageId, UserId};
pub use models::*;
pub use text::{ChannelSlug, DisplayName, MessageBody, TextValidationError};
pub use wire::{ClientCommand, ServerEvent};

/// The only protocol version accepted by this release.
pub const PROTOCOL_ID: &str = "sproyt.chat.v1";

/// A typed command DTO that may be put into a [`ClientEnvelope`].
///
/// This deliberately has a blanket implementation so the server's validated
/// domain command enum and a future WASM command enum can share framing
/// without depending on each other.
pub trait ClientCommandDto: Serialize + for<'de> Deserialize<'de> {}

impl<T> ClientCommandDto for T where T: Serialize + for<'de> Deserialize<'de> {}

/// A typed event DTO that may be put into a [`ServerEnvelope`].
///
/// Unknown variants remain a decoder error by default; an application that
/// wants forward-compatible extension events must model that explicitly.
pub trait ServerEventDto: Serialize + for<'de> Deserialize<'de> {}

impl<T> ServerEventDto for T where T: Serialize + for<'de> Deserialize<'de> {}

/// The result of checking a peer supplied protocol identifier.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolVersion {
    Supported,
    Unsupported,
}

/// Apply the protocol version policy before decoding or executing a command.
///
/// Unknown protocol versions are rejected. Unknown command and event variants
/// are intentionally left to the typed payload decoder: they must not be
/// silently treated as a known operation.
pub fn check_protocol(value: &str) -> ProtocolVersion {
    if value == PROTOCOL_ID {
        ProtocolVersion::Supported
    } else {
        ProtocolVersion::Unsupported
    }
}

/// A client request frame. `C` is normally a typed `ClientCommand` DTO.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ClientEnvelope<C> {
    pub protocol: String,
    pub request_id: String,
    #[serde(flatten)]
    pub command: C,
}

/// A response or unsolicited server frame. `E` is normally a typed
/// `ServerEvent` DTO. The omission of `request_id` distinguishes broadcasts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ServerEnvelope<E> {
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(flatten)]
    pub event: E,
}

impl<E> ServerEnvelope<E> {
    pub fn response(request_id: String, event: E) -> Self {
        Self {
            protocol: PROTOCOL_ID.to_owned(),
            request_id: Some(request_id),
            event,
        }
    }

    pub fn event(event: E) -> Self {
        Self {
            protocol: PROTOCOL_ID.to_owned(),
            request_id: None,
            event,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", content = "payload", rename_all = "snake_case")]
    enum ClientCommand {
        Ping,
    }

    #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "type", content = "payload", rename_all = "snake_case")]
    enum ServerEvent {
        Pong,
    }

    fn assert_client_command<T: super::ClientCommandDto>() {}
    fn assert_server_event<T: super::ServerEventDto>() {}

    #[test]
    fn golden_client_json_is_stable_and_round_trips() {
        assert_client_command::<ClientCommand>();
        let frame = ClientEnvelope {
            protocol: PROTOCOL_ID.to_owned(),
            request_id: "r1".to_owned(),
            command: ClientCommand::Ping,
        };
        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(
            json,
            r#"{"protocol":"sproyt.chat.v1","request_id":"r1","type":"ping"}"#
        );
        assert_eq!(
            serde_json::from_str::<ClientEnvelope<ClientCommand>>(&json).unwrap(),
            frame
        );
    }

    #[test]
    fn golden_server_json_is_stable_and_round_trips() {
        assert_server_event::<ServerEvent>();
        let frame = ServerEnvelope::response("r1".to_owned(), ServerEvent::Pong);
        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(
            json,
            r#"{"protocol":"sproyt.chat.v1","request_id":"r1","type":"pong"}"#
        );
        assert_eq!(
            serde_json::from_str::<ServerEnvelope<ServerEvent>>(&json).unwrap(),
            frame
        );
    }

    #[test]
    fn concrete_server_events_deserialize_dynamic_wire_strings() {
        fn assert_deserialize_owned<T: serde::de::DeserializeOwned>() {}
        assert_deserialize_owned::<crate::ServerEvent>();
        let json = r#"{"protocol":"sproyt.chat.v1","type":"error","payload":{"code":"future_error","message":"dynamic"}}"#.to_owned();
        let frame = serde_json::from_str::<ServerEnvelope<crate::ServerEvent>>(&json).unwrap();
        assert!(
            matches!(frame.event, crate::ServerEvent::Error { code, message } if code == "future_error" && message == "dynamic")
        );
    }

    #[test]
    fn unknown_versions_and_events_are_rejected() {
        assert_eq!(
            check_protocol("sproyt.chat.v2"),
            ProtocolVersion::Unsupported
        );
        let error = serde_json::from_str::<ClientEnvelope<ClientCommand>>(
            r#"{"protocol":"sproyt.chat.v1","request_id":"r1","type":"future_command"}"#,
        )
        .unwrap_err();
        assert!(error.is_data());
    }
}
