# Interrupted invitation signup: Canary V2

This is the forward-only recovery slice of #169. New and existing recipients
still need real delivered-email/OIDC acceptance. Historical inactive accounts
are not backfilled, activated or reset by this change.

## Eligibility and completion

The separate `sproyt-invitation-canary-v2` flow has UUID
`a1b4b2b7-80e3-591d-b52f-bed0ae3e5a35`. Existing Canary and production flow
files and objects remain available for outstanding invitations.

Immediately after dedicated inactive account creation, a row-locked policy
checks the original creation snapshot, then stores a timestamped Django-signed
attribute `sproyt_canary_v2_pending`. The V2-specific salt binds the account
primary key/UUID, normalized email, exact flow, creation `last_updated`, unusable
password fingerprint and stable initial `sproyt-users` group identifier.
The signature expires after 24 hours and is never renewed. Eligible accounts
must remain inactive external users in `users/sproyt`, with an unusable password,
no previous login, no direct roles, no linked sources and no extra groups or
superuser status.

A fresh owner/moderator-issued invitation can resume an eligible account. It
skips profile/creation and always sends a fresh independent mailbox challenge,
including if an earlier attempt already verified the address. Knowing the
original invitation does not grant mailbox proof or allow password selection
before that proof. The actual email token snapshot binds recipient, original
marker, flow, email stage, continuation and expiry. Those bindings are checked
again after the password prompt.

One transaction and `select_for_update` cover eligibility, password hashing,
activation, marker removal and saving the password-change timestamp. There is
no later UserWrite stage. Two concurrent attempts have one winner; a stale
attempt hits the deny stage before User Login. A failure before commit restores
all account fields. If the browser disappears after commit, the now-active
account uses ordinary OIDC login and cannot be reset by replaying this flow.

All six Deny guards explicitly store `failure_result: true` on the actual
Authentik PolicyBinding. Conditional-stage bindings store false. ExpressionPolicy
has no such field; placing it there would not enforce the exception fallback.
The runtime gate injects real engine/executor errors to prove rejection before
mailbox verification and password completion, without an authenticated session.

Unmarked, expired, ambiguous, active or otherwise ineligible identities retain
the existing-account branch: no password change, activation, profile overwrite
or automatic login. They must use ordinary authentication and existing operator
recovery. An old historical placeholder may therefore still require private
administrative handling; do not infer its origin from its disabled state.

## Administrative revocation

Removing `sproyt_canary_v2_pending` explicitly revokes eligibility. In the deployed
Authentik 2026.8.2, the normal administrative UserSerializer update calls a full
model save, advancing `last_updated`; even saving `is_active=false` for an
already-inactive placeholder revokes recovery. A changed creation snapshot also
prevents initial stamping. No policy silently signs that edited state.

Direct database updates or custom code using `QuerySet.update` or restrictive
`update_fields` can bypass `auto_now`. Such operators must remove the marker
explicitly when revoking a pending registration. This is not a general account
reactivation or password-recovery flow.

## Verification and delivery

Run the original invitation/production/continuation contracts and
`tools/test-invitation-canary-v2.py`, with the pinned test-only Django dependency
from `tools/requirements-authentik-tests.txt`. CI runs these checks. The new
signature tests cover forged/copied/wrong-origin/expired markers; policy tests
cover identity, email, proof, group, role, source, activation and admin changes.

Before enabling V2, run `tools/verify-invitation-canary-v2-runtime.py` through the
running Authentik shell, supplying the exact reviewed blueprint as a `blueprint`
string. Preserve its sanitized `SPROYT_V2_RUNTIME` JSON receipt. It validates the
real importer, direct and interrupted executor journeys, an unrelated signed-in
browser, existing-account preservation, administrative revocation, failure
rollback and actual PostgreSQL two-connection single-winner completion.

The importer/executor portion rolls back. The concurrency portion creates one
uniquely named committed probe account so both connections can see it; `finally`
removes only that fixture, and a final query verifies cleanup. Mail delivery is
mocked and addresses use `example.invalid`. These checks do not prove delivered
email, OIDC callback or circle membership acceptance.

Merge source only after independent Astra diff/test review and required CI.
The GitOps blueprint Application must pin the final reviewed source; manually
sync its existing single ConfigMap, preserving all three old blueprint keys and
all production flow bytes. Read back imported V2 policies before switching only
Canary's enrollment UUID/slug in a separate GitOps values change. Retain the
already verified application image/chart: application code, SQL migrations and
OCI runtime assets are unchanged, so this provider-flow release needs no new
application image or database backup/restore cycle. Its own importer/executor
and authentication gates remain mandatory. No production values are changed.

Verify Argo Synced/Healthy, the exact running image/revision and public Canary
readiness after the GitOps change. Try a delivered fresh invitation, abandon
before or after mailbox confirmation, request another invitation within 24
hours, verify the fresh mailbox challenge and complete exactly one intended
membership. Also check an existing account, a mismatched signed-in identity and
replay. Keep #169 open until real email/OIDC acceptance is recorded.

Rollback selects the previous Canary enrollment UUID/slug in GitOps. Keep V2
objects for outstanding links; do not delete the ConfigMap, alter production's
flow, backfill old accounts or run the broad email-recovery rollback.
