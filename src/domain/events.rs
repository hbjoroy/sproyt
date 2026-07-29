use serde::{Deserialize, Serialize};

use super::{ChannelId, ChannelSequence, ChatMessage, MessageReactionChange, UserId};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEvent {
    ChannelCreated {
        channel_id: ChannelId,
        created_by: UserId,
    },
    ParticipantJoined {
        channel_id: ChannelId,
        participant_id: UserId,
    },
    ParticipantLeft {
        channel_id: ChannelId,
        participant_id: UserId,
    },
    MessageAccepted {
        message: ChatMessage,
    },
    MessageEdited {
        message: ChatMessage,
    },
    MessageReactionChanged {
        change: MessageReactionChange,
    },
    ReadMarkerUpdated {
        channel_id: ChannelId,
        user_id: UserId,
        sequence: ChannelSequence,
    },
}
