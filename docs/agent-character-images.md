# Character images from circle agents

The circle editor has two separate opt-ins: generating a character image when
asked, and occasionally contributing one during an active conversation. Neither
option follows from an agent's name. A manager explicitly selects an identity
from the server catalogue; the initial catalogue contains Maria, a fictional
adult aged 30–40, using the existing versioned reference asset.

The API field image_generation is either null or an object with identity_id,
enabled and occasional. Omitting it on PATCH preserves the current choice;
null removes the configuration. List responses advertise image_identities and
image_generation_available. Older API replies default safely off. Configuration
failure keeps the form's edits, and an existing option can be disabled while
generation is unavailable. Unchanged settings are omitted when saving unrelated
agent edits.

The fixed limits for this first implementation are two reservations per agent
per rolling 24 hours, with at most one unsolicited image in that interval.
Occasional contributions remain anchored to a human message in an authorized
channel; this is not a scheduled broadcaster. There are no per-agent quota
controls in the editor. A generated scene is an illustration, not an eyewitness
observation or evidence that a vessel is currently at a particular location.

SPROYT_CHAT_AGENT_IMAGES_ENABLED, exposed as Helm config.chatAgentImagesEnabled,
defaults off. Keep it off until schema 57 and compatible workers/publishers are
running in both environments sharing the database. Enable only explicitly
configured agents after the durable image-publication checks and real model
acceptance. Gate-off alone does not authorize rolling back to a worker that
cannot understand admitted image jobs. Follow the implementation's reservation,
identity-hash, publication-receipt and ambiguous-submission contracts.

The editor contracts cover old replies, explicit identity selection, disabled
defaults, failed-save recovery, unchanged PATCH omission and disabling an
existing setting during an outage, in Chromium and iPhone WebKit. These UI
checks do not establish Comfy execution, published media or visual identity;
those require the separate backend and actual image acceptance evidence.
