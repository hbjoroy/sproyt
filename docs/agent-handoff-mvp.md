# Agent delegation MVP

`SPROYT_AGENT_HANDOFFS_ENABLED=true` enables one-hop delegation admission in
canary. Default is disabled. Production ingress and workers can continue using
the ordinary queue while sharing the database. Apply migration 0067 first.

Owners/moderators configure `GET/PUT
/api/v1/circles/{circle}/chat-agents/{agent}/handoff`. GET returns
`{available, revision, target_agent_ids, guidance, agent_revision}`. Revision starts
at zero. PUT accepts `{revision, target_agent_ids, guidance}` and requires the
current revision, at most four distinct enabled same-circle recipients excluding
self, and at most 1000 guidance characters. Saving increments both routing and
source agent revisions atomically. Empty recipients disable future routing.
Unavailable PUT returns 503; stale revision/invalid choices return 409; manager
authority is required. GET reports availability without enabling anything.

Only explicitly mentioned/triggered human requests enter the isolated routing
queue. Followups remain ordinary jobs. The router sends the original human text,
approved recipient IDs/names, and owner guidance to configured vLLM under the
shared model permit. It has no external tools, agent memory, location, or facts.
The strict JSON choice is an approved recipient or normal source fallback.
Invalid/unavailable model output falls back to the source. Model contention is
retried within queue limits.

This MVP forwards the **original human request**, in the **same channel**. It does
not create an invented subtask, let an agent transfer memory/location permissions,
or recursively delegate. The recipient independently builds its existing context
and capabilities through the ordinary admission and publication path, including
its own attachment snapshot and location consent checks. Explicitly triggering
multiple agents retains each independent trigger's behavior.

The dedicated queue stores IDs and revisions only; it never copies human text or
display names. Routing and admission reconstruct the currently authorized original
message, so soft deletion leaves no duplicate text in the routing queue.

The dedicated queue is invisible to old ordinary workers. A narrow database
trigger suppresses old ingress's duplicate ordinary source INSERT only when a
matching routing row exists. Unrelated INSERTs retain their existing behavior.
Routing enqueue locks the original message before checking for an existing
ordinary job, preventing a routing/source race on PostgreSQL. SQLite serializes
writers. A dedicated PostgreSQL transaction advisory lock serializes only decision admission,
preventing reciprocal source/recipient locks across different channels; it never
covers model IO. The decision and exactly one ordinary recipient/source admission commit
in the same transaction; unique admission keys and lease fencing protect retries.
The existing image planner selects an ordinary text job and cannot create a
source image plan from a suppressed INSERT.

Admission revalidates source message edit/deletion, human provenance, source
configuration/profile, channel access revision, routing revision, recipient circle
and channel eligibility. Source/config revocation before admission cancels it.
Disabling routing after an ordinary recipient job was admitted does not revoke
that job: the recipient's independent configuration/profile/channel/source and
publication checks govern it. This is an admission policy, not a new capability
grant. There is no source acknowledgement carrying foreign facts.

Queue jobs expire after five minutes, have three attempts and 90-second renewable
by retry leases, and at most 256 pending/leased admissions. Capacity exhaustion
uses ordinary source admission. PostgreSQL serializes routing capacity checks
with a transaction advisory lock. Restarted workers recover expired leases;
expired/exhausted work fails closed. Terminal rows preserve decision history and
suppression of stale duplicate ingress. Existing source message deletion cascades
these records.

Validation contracts: `sqlite_handoff_admission_contract` and optional actual
`postgres_handoff_admission_contract` (`SPROYT_POSTGRES_TEST_URL`) cover isolated
claims, legacy INSERT suppression, target/fallback admission, retries, old lease
fencing, edits, config/source/channel revocation, expiry, and independent grants.
