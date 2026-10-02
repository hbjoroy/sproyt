# Manual work-item status lifecycle

Step 5 adds `sproyt/work-item-status-change/1.0.0`: start → `change-status`
user task → end. Each change has its own Heart instance and collapsed channel
message. Existing review instances, information rounds and GitHub publication
receipts remain unchanged. Sprøyt owns business status; Heart owns activation
and completion of the user task.

## Admission and decision

Start from an authentic completed task message for a reconciled, completed
review. Require the current application processor's review permission,
`product-handler` role, writable channel membership, enabled application and
source binding, allowed application, and explicit `change-status` task route.
Pin actor, route, channel, source task/message, request ID and case revision.
Recheck current rights and the same route/channel at submission and replay.
Only one pending/waiting operation may exist for a case.

Transitions: planned → in_development/resolved/rejected;
in_development → planned/resolved/rejected; resolved/rejected → planned.
Unchanged status is rejected. **Avslutt utan endring** completes the operation
without changing case revision or recording a status transition.
`in_development` is a business status only: it starts no developer agent and
does not alter a GitHub issue. GitHub status synchronization is future work.

Accept decisions transactionally with a case-revision compare-and-swap,
immutable command receipt and history record. Exact retries return the saved
decision; changed payloads conflict. Reconciliation only completes Heart and
never rewrites business status. Lost completion responses are verified against
the exact accepted result on a later poll. Failed/cancelled Heart instances
release the operation slot without reverting an accepted business decision;
their cards show failed delivery. A subsequent change uses a new operation.

## Visibility and storage

Internal note and feedback are separately limited to 2,000 characters. Neither
is placed in Heart metadata, chat message bodies or notifications. Heart gets
only operation/case/handler/channel identifiers and accepted status/revision.
Internal notes are visible only to a currently qualified product handler.
The lifecycle projection is available only to the requester or such a handler.

A single access-controlled status marker is published in the source channel
after the first accepted change. It reads the current status/history rather
than copying text into chat. Exact case/message binding and current channel
membership are required. Other members receive only `{ "visible": false }`.
This initial policy deliberately does not publish feedback to everyone in the
source channel. Copied markers do not grant access.

Additive migration 0048 creates status operations, history and publication
receipts in both PostgreSQL and SQLite. It does not rebuild legacy review
tables or rewrite messages. Back up and fully restore-verify the shared chat
database before rollout. Roll back the app image/pins while retaining the new
tables and recorded decisions; do not downgrade or delete shared data.

## Verification and activation

SQLite/PostgreSQL contracts cover authorization, stale revisions, exact replay,
one active operation, no-change, source-message proof, third-party privacy,
lost Heart responses and terminal Heart failure/cancellation. Frontend decoder
and mobile browser contracts cover controlled forms, retry and visibility.
CI explicitly runs the PostgreSQL cases and the WebKit mobile contract.

Deploy to canary through CI/CD first. Enable only the explicit `change-status`
route in the feedback **Testkanal** with operator SQL and current policy guards.
Acceptance: use **Endre status** on a completed case, submit the new task,
inspect history and requester feedback, then reopen it using a new task.
Production promotion requires a separate approval.
