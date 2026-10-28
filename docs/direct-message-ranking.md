# Direct-message order

Direct conversations (including groups) use message history rather than local
navigation history. All devices receive the same per-user activity metadata in
`ChannelSummary.direct_activity`.

1. Conversations with a non-deleted message in the last 36 hours come first,
   newest message first. The boundary is inclusive.
2. Remaining conversations use a decayed activity score, then latest message.
3. Channel ID breaks exact ties, including empty conversations, consistently
   across clients. Unread counts do not affect ranking.

Each UTC calendar day with at least one non-deleted human message sent by the
requesting user earns one point. Another human participating that day adds one
point. Message volume within the day does not increase the score. Contributions
decay exponentially with a 30-day half-life, measured from the UTC day start.
Incoming-only days and agent messages earn no points. Last-message recency still
includes non-deleted agent messages. Threads and attachments count as messages.

The server aggregates all existing history for authorized direct memberships in
one additional batched query, returning channel/day metadata, never bodies.
PostgreSQL, SQLite and the memory repository share the scoring function. No new
database migration or local usage storage is required. If history volume makes
this query expensive, a transactional per-channel/day rollup can replace the
aggregate without changing the wire contract or client algorithm.

The browser normalizes scores to one comparison time and rechecks the 36-hour
boundary once a minute. Accepted messages update recency immediately. A coalesced
channel-list refresh after 15 seconds obtains authoritative scores and deletion
effects. Bootstrap/reconnect obtains the same data; older server responses may
omit the optional field during a rolling upgrade.

Reordering waits until the user leaves the list and finishes composing, with a
250 ms settling delay. Existing rows retain their positions while hovered or
keyboard-focused; new conversations remain reachable. No extra headings, badges
or controls are added.

Validation covers active/older ordering, cutoff and decay, stable identity ties,
daily deduplication, authorization, deletion, group DMs, UTC offset handling,
protocol compatibility, and browser focus/compact navigation behavior.
