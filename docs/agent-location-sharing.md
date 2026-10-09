# Temporary position sharing with circle agents (#241)

A human can share one browser position with one active circle agent in one
channel. The channel menu and compact composer open the same dialog. Sharing
requires a deliberate click and the browser's permission. No background GPS
watch, browser storage or long-term agent-memory ingestion is used.

The dialog warns that replies are visible to the channel: other participants
may learn where the person is from the reply. Revocation prevents future use;
it does not erase already published messages or undo a request already sent
to the configured model service.

## Storage and authority

Migration 0063 adds `agent_location_shares`, keyed by human, agent and channel,
and the nullable `circle_chat_agent_jobs.location_share_id`. Each replacement
gets a new random share ID. A job binds to the current grant when enqueued;
earlier jobs cannot pick up a later grant. Coordinates are rounded to four
decimal places and kept with the browser observation time and accuracy.
The server expiry is 30 minutes after the share request. Observations older
than five minutes or more than 30 seconds in the future are rejected.

Only the authenticated human can read, replace or remove their shares. Creating
and using a share require current human membership in the channel and circle,
an enabled circle agent and that agent's current channel access. Direct
messages and global channels are excluded. Revocation remains available after
membership loss and when the feature flag is off.

`SPROYT_AGENT_LOCATION_SHARING_ENABLED` defaults to false. GET/PUT fail closed
when disabled. DELETE remains available. The API is under
`/api/v1/channels/{channel_id}/agent-locations`, with PUT/DELETE on
`/{agent_id}`. Responses are private and uncached; writes require same-origin
requests. Raw coordinates are not broadcast as chat messages.

## Reply processing

The worker loads only the bound grant and checks its current revision, expiry
and memberships before generation, after generation and in the publication
transaction. Replacement, expiry, revocation and flag disable invalidate the
bound job, including retries with a cached reply. PostgreSQL publication locks
the source/job using the existing memory authority flow, then the exact grant
for sharing; validation uses the current clock after waiting. Share mutations
commit before job-cache cleanup to avoid opposing lock orders.

The model receives explicit temporary `shared_location` context. It must not
apply it to another person/channel or invent a precise venue. When weather is
configured, initial weather is fetched at the shared coordinates. An explicitly
requested different place takes precedence through the existing weather tool.
Location-derived weather snapshots are not persisted in the job; their validity
deadline still gates publication. Existing unbound jobs retain their behavior.

Expired shares and unpublished caches belonging to deleted/replaced grants are
removed in bounded worker cleanup batches, throttled to once per minute per
process. Long-term memory extraction continues to use human chat messages;
temporary GPS context is not included. Standard channel exports do not include
the private share table. Database backups may retain a share until backup
retention removes it: restoration must run
`tools/reset-agent-memory-after-restore.sql` before starting any agent worker.
That reset also deletes all restored position grants and skips their unpublished
jobs. Original messages and published replies are preserved.

## Release and validation

Ship schema and code with `config.agentLocationSharingEnabled: false`. Confirm
every production and canary worker runs the new immutable image, then enable
the flag through GitOps in both environments. Old workers cannot enforce the
new grant fence, so the flag must not be enabled during a mixed-version rollout.
Rollback requires disabling the flag first; never roll back to an older worker
while bound jobs are active.

Tests cover private API authority, input validation, scoped binding,
replacement/revocation/expiry and publication fencing, plus browser permission,
one-shot geolocation, consent, removal, draft preservation and late callbacks.
Real PostgreSQL contract tests run in CI; Chromium and iPhone WebKit exercise
the dialog. Final real-device permission and local-weather acceptance require
a human using their own browser position.
