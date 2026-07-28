# Media pipeline

Sprøyt stores an immutable original and, for supported raster images larger than
720 pixels, a bounded `preview` variant. Both are written in the same database
transaction. A message can only attach the media object after that transaction
has completed, so an interrupted upload cannot expose a partially written blob.

Binary content is never embedded in the WebSocket payload. The browser uploads
first, receives an immutable media ID, and only then includes that ID in a
message. `media_objects` keeps ownership, channel scope, checksums, dimensions,
alt text and analysis metadata; `media_blobs` is the initial shared MediaStore;
and `message_attachments` normalizes the relation to messages. Attachment
creation verifies uploader ownership and channel scope, and a media object can
only be attached once.

The conversation timeline requests `/api/v1/media/{id}/preview`. The endpoint
falls back to the original for small images and formats without a server-side
decoder. Opening the lightbox requests the immutable original from
`/api/v1/media/{id}`. Both endpoints repeat channel-membership authorization.
Uploads are capped at 35 MiB and file signatures, rather than browser-declared
types, determine accepted content. Object-store URLs must not be exposed
directly if storage later moves to S3 or R2; Sprøyt must retain authorization at
the media API boundary or issue short-lived URLs after checking membership.

JPEG, PNG, GIF and WebP uploads are decoded before they are accepted. This both
rejects incomplete image containers and produces a preview whose longest edge
is at most 720 pixels. HEIC and AVIF originals remain supported, but preview
generation for those formats requires a future decoder/worker.

## Heart boundary

Heart should orchestrate optional, asynchronous work; it should not carry image
bytes or sit on the synchronous upload path. A future media worker can consume
an immutable media ID and idempotently add variants or metadata, while Heart
tracks steps such as preview generation, AI tagging and gallery indexing. The
`media_variants` table deliberately supports that incremental path. Chat and
original downloads must keep working while Heart or a media worker is offline.
The stable media ID and API paths allow storage to move away from PostgreSQL
without changing message history, gallery queries or clients.
