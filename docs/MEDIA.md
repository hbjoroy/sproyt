# Media architecture

Sprøyt treats uploaded media as durable domain data. The browser uploads a file before sending the
message and receives an immutable media ID. The message carries that ID; binary content is never
embedded in the WebSocket payload.

## Current storage

- `media_objects` contains ownership, channel scope, MIME type, size, checksum, dimensions,
  duration, alt text and analysis state/metadata.
- `media_blobs` is the initial `MediaStore`. Keeping the bytes in PostgreSQL gives all application
  replicas the same view without adding a cluster component for the first production slice.
- `message_attachments` is the normalized message-to-media relation. It is written in the same
  transaction as the message, after validating that every media object belongs to the sender and
  the selected channel. A media object can only be attached once.
- The browser message format includes a stable compatibility token. The server parses it only to
  obtain media IDs; ownership and MIME metadata always come from the database.
- Downloads require authentication and current membership in the media object's channel.
- Uploads are limited to 25 MiB and accepted types are determined from file signatures, not only
  the browser-provided content type.

## Planned storage evolution

The metadata ID and API URL stay stable when blob storage moves to an S3-compatible service or
Cloudflare R2. A background worker can claim rows through `analysis_status`, extract dimensions,
duration, thumbnails and AI tags, and store provider-neutral results in `analysis_metadata`.
Gallery views query `(channel_id, created_at)` and search workers query the analysis index; neither
needs to parse message text or read the original blob.

Do not expose object-store URLs directly. Sprøyt must keep authorisation at the media API boundary
or issue short-lived signed URLs only after checking channel membership.
