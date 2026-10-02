# Manual GitHub publication task

Step 4 adds an actual Heart user task, displayed as a collapsed channel message.
The reviewed case remains in Sprøyt whether or not it is published. Publication
is explicitly approved by the assigned product handler after reviewing the
destination, title and text. No channel history, information-round notes or
attachments are added to the public issue automatically.

## Linked process

Review definitions 1.0.0 and 1.1.0 and their completed instances are unchanged.
Instead of rebuilding SQLite's existing review-version constraint, use the
immutable `sproyt/work-item-github-export/1.0.0` continuation:

`completed planned review → publish-github user task → end`

The scanner starts a continuation only for a completed, reconciled planned
case whose application has an explicit enabled GitHub binding, export-task
flag and enabled `publish-github`/`product-handler` channel route matching the
case's frozen review channel. Historical cases use this same path. One persisted
continuation per case pins the reviewer/channel and retains a separate Heart
instance ID, lease and status. It never overwrites the original review instance
or changes the review category, priority or disposition.

The task offers **Send til GitHub** or **Berre internt**. Both are durable
decisions; skip is terminal and cannot be recreated by the scanner. Other
channel members see the card read-only. Submission requires the assigned
reviewer, current writable channel membership and product-handler/review
rights. Publication additionally requires `can_export`, an enabled application,
the explicit export route and the operator-approved destination.

## Approval and delivery

The preview includes repository ID and binding revision. The command binds
these to its case revision, exact text, request ID and actor. A changed
destination cannot silently redirect an open preview or an accepted receipt.
Identical retries return the original receipt; changed text, target or revision
conflicts. Database uniqueness permits one export receipt per case/task.

The transactional receipt commits before network I/O. Workers recheck current
rights, route and exact pinned destination before a first create. They mint an
installation token restricted to that repository and verify its identity and
issue support. The approved body receives one invisible correlation marker;
the external response must match the exact repository, content and App author.

A durable `sending` state precedes POST. Timeout, response loss, malformed
success and ambiguous server responses enter `uncertain`; an expired sending
lease follows the same path. Reconciliation lists open and closed issues in
the pinned repository and verifies the unique marker and provenance. It can
record an already-created issue after local publish rights are revoked, but
does not issue another POST. Missing/incomplete/ambiguous results remain
uncertain for operator investigation. The scan is bounded to ten pages of
100 issues; this is recoverability, not an external exactly-once guarantee.

Explicit GitHub non-creation rejections (400, 401, 403, 404, 410, 415, 422,
429) are blocked and can retry the same immutable payload after backoff and
renewed authorization. In particular, repairing permissions does not leave a
known rejected request permanently uncertain. Persistent validation rejection
needs operator attention; approved payloads are not edited behind the user.

Issue ID/number/URL and the exact Heart completion result commit together.
Only durable GitHub success or skip completes the Heart task, using the
accepted request ID as its completion key. Heart outages after publication
show the saved issue link and never cause a second GitHub create.

## Configuration and canary

Migration 0047 is additive on SQLite and PostgreSQL. GitHub binding is
operator-approved in `work_github_bindings`; a circle owner's generic app
configuration cannot select arbitrary repositories from the App installation.
The owner's all-hbjoroy-repository installation is not an application grant.
Initial approved destination: application `sproyt`, `hbjoroy/sproyt`, repository
ID `1272543078`, installation `167150170`.

Helm `github.existingSecret` defaults empty. Canary uses the externally managed
`sproyt-github-app` Secret with App ID and a read-only PEM mount. Credentials
and installation tokens never appear in source, images or application logs.
The existing Heart image supports the new immutable definition unchanged.

Enable only the requested feedback Testkanal's route and Harald's export right
after CI/release and canary migration verification. The first real publication
is performed by the owner through the preview, not by a deployment smoke test.
Automatic development and GitHub-to-Sprøyt status synchronization remain off.

Disable the binding/export-task flag or route to stop new tasks/publications;
already attempted work retains its receipt for read-only reconciliation.
Rollback retains additive migrations, messages, reviewed cases, receipts and
links. Never restore an old database over later chat messages for this release.
Credential removal requires checking active workload references first.
