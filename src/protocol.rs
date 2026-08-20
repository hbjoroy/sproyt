//! Server-side compatibility surface for the portable WebSocket contract.
//!
//! The concrete wire types live in `sproyt-protocol`; this module preserves
//! the original internal import path while the server domain is disentangled.
pub use sproyt_protocol::{
    ClientCommand, PROTOCOL_ID, ProtocolVersion, ServerEvent, check_protocol,
};

pub type ClientEnvelope = sproyt_protocol::ClientEnvelope<ClientCommand>;
pub type ServerEnvelope = sproyt_protocol::ServerEnvelope<ServerEvent>;
