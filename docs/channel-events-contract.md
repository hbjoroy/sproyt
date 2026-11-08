# Channel event HTTP contract

All routes are under `/api/v1/channels/{channel}/events`; require current human `x-sproyt-account`, current channel (and circle) membership. Responses private/no-store. IDs UUID strings. Times RFC3339 UTC strings; timezone an IANA name.

Channel availability is exposed as `events_enabled` in channel summaries, governance and event responses. Owners and channel moderators can `PUT /api/v1/channels/{channel}/events-enabled` with `{enabled:boolean}`; the response is the current governance document. The setting uses the existing channel-change invalidation.

An unset policy enables existing events and leaves empty channels disabled. An explicit choice always wins, including after older application instances create or edit events. Turning the feature off preserves events, responses and images. Normal chat shows no event navigation, pills or autocomplete; authorized existing links and history remain readable as an archive. New application instances reject event mutations while disabled with HTTP 409, distinct from revoked membership (403). Membership-removal cleanup remains active.

The availability predicate is checked atomically with writes and serialized against setting changes. During rolling upgrades, older 4172333 application instances do not enforce this new availability policy; their existing authorization still applies and their writes do not overwrite an explicit choice. Availability is not a new access-control boundary. Retain a schema-77-capable migrator when rolling back the application.

- GET collection: `{events: Event[]}` upcoming/ongoing first by start ascending, ended next by start descending, cancelled last by start descending; UUID ascending breaks ties. `?after=<opaque cursor>` and `next_after`, at most 100 per page; never interpret the cursor as an event ID. `attention_count` is the current actor's global material-change notice count across every page. List summaries include cover but `images:[]`; open detail for library.
- POST collection / PUT `/{id}` body `{title,code,starts_at,timezone,ends_at:null|string,location:null|string,description}`. Create returns Event. Update full replacement; creator/owner/moderator only.
- GET `/{id}` returns Event.
- POST `/{id}/cancel` returns Event, cancellation retained in history.
- PUT `/{id}/rsvp` body `{status:"attending"|"declined"|"following"|"unanswered"}` returns Event. Unanswered deletes own answer.
- GET `/{id}/responses` returns `{responses:[{user_id,display_name,status}],counts:{attending,declined,following,unanswered}}` at most 200 per page; `?after=<user_id>` and `next_after`.
- POST `/{id}/images` body `{media_id}` attaches an existing image uploaded using normal channel media upload. GET detail includes images. DELETE `/{id}/images/{media_id}` removes own contribution or manager's image.
- GET `/{id}/images?after=<media_id>` returns `{images,next_after}`, at most 200; detail carries first page and `images_next_after`.
- POST `/{id}/acknowledge` returns Event and atomically acknowledges the current material revision for own RSVP without changing it.
- PUT `/{id}/cover` body `{media_id:null|string}` chooses a library image.
- GET `/{id}/messages?before=<sequence>` returns `{messages:ChatMessage[],next_before:number|null}`, bounded 30, complete ordinary message objects.
- POST `/references` body `{codes:string[]}` max 50 returns `{events:Event[]}`. Codes case normalized; reference identity is event UUID.

Event: `{id,channel_id,creator_id,title,code,starts_at,timezone,ends_at,location,description,cancelled,revision,updated_at,cover_media_id,my_rsvp,can_manage,images:[{media_id,contributor_id}]}`. `my_rsvp` always one of the four values above. `revision` increments on changes to start/end/timezone/location and cancellation. `needs_attention` compares own RSVP baseline to that revision for attending/following; this is a durable in-app notice, explicitly acknowledged via POST acknowledge. No automatic push is sent. `aliases:string[]` preserves previous codes (up to 200 code identities per event; another new code then returns conflict). All mutation endpoints return Event.

Codes: 1–48 lowercase ASCII letters/digits/hyphens; unique per channel. Title 1–160, description at most 20000, location at most 300. Eligible channels exclude personal spaces and direct/group conversations. All channel members may create and upload; managers are creator/owner/moderator. Existing normal media quota, validation, processing and channel read controls apply; only own uploads from the same channel may be attached, without copying bytes. Removal from library unlinks it and clears cover but does not remove a shared media object.

Past answers preserved on membership removal; future answers (COALESCE(ends_at,starts_at) later than current UTC time) cleared authoritatively in database triggers (also old production writers). Rejoining never restores RSVP.

References are parsed from CommonMark text nodes, skipping links/images/code/HTML. The case-insensitive prefix must begin a text node or follow whitespace/`([{`; the ASCII code may not be followed by a Unicode letter/number, underscore or hyphen. Escaped references remain literal. `docs/fixtures/event-references.json` is the shared Rust/TypeScript conformance corpus. Retired aliases are reserved and cannot be reassigned. Message filters parse current live bodies (including old production writes), so edits and deletions are reflected without a worker. Each request scans at most 200 ordinary complete messages and returns up to 30 exact matches. `next_before` may be present on an empty page; clients must continue until null. This cursor is the last scanned sequence when fewer than 30 matches are found.

The responses roster includes every current human channel/circle member (default unanswered), plus retained former members with an explicit ended-event answer. Aggregate counts cover the entire roster and are repeated on each page. A former member of a future event is absent, rather than being counted unanswered.

`GET` collection with `?prefix=<code prefix>` is bounded autocomplete: up to 12 authorized summaries selected from the indexed current/reserved code range, including events beyond the first listing page. Prefix is case normalized ASCII code characters (empty allowed, max 48); SQL wildcards are rejected. `next_after` is null.

List cursors are versioned base64url JSON carrying fixed UTC `as_of`, bucket, start and event UUID. The original as-of is preserved across pages so time passing cannot move an event between buckets during pagination. Unchanged records paginate without gaps or duplicates; changing an event's schedule/cancellation concurrently can change its ordering and callers should refresh after mutations. Cursor fields are strictly validated and bound; authorization is independent of the cursor.
