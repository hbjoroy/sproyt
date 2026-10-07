# Agent memory: M3–M6 operation

Memory belongs to one human and one circle agent. Notes keep their evidence
channel, including notes corrected or confirmed by their owner. Notes from open
(`public`/`local`) channels may be used across that circle, including in private
channels. Private-channel notes may only be used in their own source channel;
they cannot move into open channels or another private channel. Direct chats
remain excluded. No shared vector database or external memory service is used.

## Capture and processing

Both durable human send paths record pending work in the message transaction,
independently of triggers. Both agent and human must explicitly allow memory;
the global collection flag must also be enabled. First capture sets a new
channel floor. Opt-out, reset and revoked access cannot reopen earlier history.
The original messages are never removed by memory controls.

The builder reads at most twenty new own messages in ascending order and ten
human context items from the same channel/thread after its start floor. A
thread change ends the batch rather than skipping intervening own messages.
The full raw evidence is sealed before bounded prompt text is prepared. Old
notes are not recursively summarized. All supplied context becomes a source
dependency, not just the citations returned by the model.

Low traffic becomes eligible after five minutes of inactivity; five pending
own messages can be processed earlier. Busy scopes return behind other waiting
scopes. Normal replies have priority over new memory calls, so backlog may grow
under sustained load. There is no unconditional freshness guarantee.

An edit/delete directly deletes dependent notes and invalidates in-flight work.
It schedules one bounded repair batch without rewinding the normal cursor.
Several nearby edits coalesce. Widely separated edits can lose automatic
reconstruction of later notes; their invalid notes are nevertheless deleted.
Ordinary trigger/phrase changes and unrelated departures preserve notes.

## Budgets and model admission

Each profile has at most 24 notes, 16 KiB note text, 720 source references,
1,024 forgotten source exclusions and 64 channel scopes. One note is at most
1,024 UTF-8 bytes. Model input is capped at 24 KiB and output at 600 tokens.
Preference notes have no automatic expiry; temporary context expires after
seven days and interaction notes after ninety. The builder only removes
expired automatic notes; confirmed/corrected notes are never silently evicted
to make room. Saturation and pending work are visible in the owner's panel.

The database singleton admits one valid model lease across prod/canary and
replicas. Memory is capped at two starts in a rolling sixty-second window;
normal replies use the same lease without consuming the learning quota. No
transaction or connection is retained across the model call. Completed HTTP
responses release the slot even if their JSON/candidates are rejected.
Unknown transport outcomes retain a 180-second quarantine. This limits valid
database leases; it does not prove a remote model request physically stopped.

Failed memory work retries after five minutes, with a maximum of five attempts.
Source changes or an explicit owner control reset attempts. Inspect pending
work and `last_error` rather than increasing the quota when the queue stalls.
Metrics expose only aggregate counts, model duration/tokens and pending age;
no user/channel/agent identity or private content is a metric label.

## Publication and database locks

A reply uses at most six relevant, current notes about its target human, from
eligible channels in its circle, in a separate 4 KiB untrusted-data budget. Stored reply
jobs record the epoch, exact note revisions/content and source versions before
caching model text. Publication validates these in the message transaction,
even when a worker has memory USE disabled or resumes a cached reply.

The current source-channel visibility, owner membership and agent access are
checked both before generation and before publication. A source made private,
removed membership, disabled source agent, or forgotten note fences even a
cached reply. The model is told to describe available notes when asked what it
remembers; an empty selection means no available notes here, not no memory ability.

PostgreSQL takes source message locks before memory profile locks, matching
source-edit triggers. Membership/agent authority locks also precede the memory
profile. Source/destination channel rows are locked together in ID order between
circle membership and channel membership, matching channel access updates.
Publication rechecks its full source/configuration/lease/deadline
predicate after waiting for memory/media locks. Stale cached text is discarded;
only an otherwise still-valid original job can retry.

## Release, activation and rollback

Use the normal reviewed main → full release CI → immutable digest → GitOps
path. Production and canary share a database: deploy compatible publication
fences to **every** reply worker with all three memory flags off before any
learning/use is enabled. Independently pinned migration hooks must include
migrations 0058–0061 before the new application runs.

Activate collection, building and reply use in that order for the Maria pilot.
Agent-level approval does not grant user consent. Users opt in themselves via
`Mitt minne`; do not write enabled profiles for other people. Preserve Maria's
triggers, personality and tools. Canary/production workers enforce saved
dependencies regardless of local flags. Flags govern new admission; operational
collection off/on is a pause, not a deletion or new consent boundary.

Roll back only to binaries that understand stored publication dependencies.
Turning flags off must not bypass privacy fences. Keep canary for further work.

## Restoring an older backup

1. Stop **all** production/canary reply and memory workers using this database.
2. Restore/migrate the database using the established verified recovery process.
3. Run `psql --set ON_ERROR_STOP=1 --file tools/reset-agent-memory-after-restore.sql`.
4. Verify profiles/notes are empty, memory-based pending/cached replies stopped,
   and original messages retained. The CI restore drill checks this distinction.
5. Drain the remote model service or wait the fresh 180-second quarantine.
6. Restart compatible workers. Users must opt in again; new learning starts at
   fresh channel boundaries. Do not restore old consent or confirmed notes.

Retaining memory across an older restore needs an independent deletion journal;
that is outside this MVP. A raw backup contains private memory until retention
deletes that backup, even though the application has forgotten it.
