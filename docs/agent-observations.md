# Agent image context and vessel observations

This extends circle-agent capabilities independently of Maria's name or
personality. Existing agent/channel grants and configuration revisions remain
the authority. Vision is an explicit per-agent choice; own AIS is available to
agents configured for Paros ferries when the server's AIS gate is enabled.

## Vision

SPROYT_CHAT_AGENT_VISION_ENABLED is an admission gate, initially off. Once
available, a circle manager can enable image interpretation per agent. PATCH
omission preserves the existing choice, and the UI can disable an existing
choice even when the server gate is unavailable.

Only committed attachments belonging to the triggering message are eligible;
URLs and attachment-like text in a message grant no access. At most two JPEG,
PNG or WebP images are supplied. A bounded preview is preferred, including
for a large original; original fallback is limited to 8 MiB. Decode limits
and normalization constrain each model image to 1024 pixels on its longest
edge and at most 1 MiB. Unsupported, omitted and undecodable images are
reported explicitly rather than silently treated as inspected.

The durable job freezes selected media IDs, positions, variants, MIME types
and hashes. Fetch and publication recheck the source message, ownership,
channel grant, agent/profile state and configuration/access revisions.
Publication locks the selected attachments and media bytes, verifies their
hashes, then rechecks lease and data deadlines after any lock wait. Revoked,
edited, deleted or changed sources cannot publish a stale image response.

The existing Santorini model receives text plus inline image data; no remote
image URL is fetched by the model. Agent skills are capabilities selected by
configuration, independent of the display name. This slice interprets
images; generating a character image is a separate implementation stage.

## Own AIS

SPROYT_AIS_URL is an operator-configured ship-tracker frontend base URL.
Only /api/stations and /api/events are read, with no redirects, credentials
in the URL or model-selected addresses. SPROYT_AIS_AGENTS_ENABLED defaults
off. Helm permits only the existing ship-tracker frontend on TCP3000.
The receiver and its MarineTraffic forwarding remain unchanged.

The existing stations endpoint updates last_seen for static AIS name
messages too. Its stored position cannot be claimed fresh on that basis.
Sprøyt therefore uses stations only for vessel names/types and builds its own
bounded position cache from actual SSE position frames. It keeps at most 512
positions around Paros, supplies at most ten observations and excludes
positions older than nine minutes when composing new context, reserving a
full minute for inference and publication. No published observation may be
ten minutes old. An absolute transmitter observation time
is not available: the exposed time is when Sprøyt received the position frame.
The AIS second-of-minute field is never treated as an epoch timestamp.

The snapshot is valid for at most sixty seconds and no longer than the oldest
selected position's remaining freshness. A restarted replica warms its cache
from the live stream; partial coverage, warming and unavailable data are
explicit. An empty selection does not mean no vessels are present.
Positions, movement and AIS-reported names do not establish docking, delay,
cancellation, route or a live ETA. A person's eyewitness report is separately
attributed evidence and does not require AIS corroboration to be acknowledged.

## Operator facts and planned calls

The small reviewed catalogue in src/chatbot/operators.rs includes Blue Star
Delos/Naxos, Artemis, Champion Jet 3, Superjet 2 and Worldchampion Jet.
It records official source links and a verification date. Length, capacity
and published speed are specifications, never current activity or actual
speed. Blue Star and Hellenic are distinct Attica brands; Seajets is separate.
Complete normalized names permit spacing aliases; partial names and corporate
ownership do not identify a vessel. An image silhouette alone is insufficient.

The sources are the official [Blue Star fleet](https://www.bluestarferries.com/en-gb/ferries),
[Hellenic Seaways fleet](https://www.hellenicseaways.gr/en-gb/ferries),
[Seajets fleet](https://www.seajets.com/learn-about-seajets/fleet), and
[Attica profile](https://www.attica-group.com/en/group-profile).
Review the linked vessel pages when extending or refreshing the catalogue;
do not infer MMSIs from similar names. GTP calls remain planned timetable data,
separate from both observations and operator specifications.

## Delivery

Schema 0056 is additive. Deploy compatible workers to production and canary,
which share a database, before enabling new admissions and agent vision.
Use the existing full backup/restore and GitOps gates. Gate-off alone is not
permission to run older workers against queued jobs with newer capabilities;
retain compatible code and roll forward after activation.
