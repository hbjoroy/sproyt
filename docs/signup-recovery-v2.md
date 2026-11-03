# Versioned signup and password recovery

`deploy/authentik/z-sproyt-signup-recovery-v2.yaml` installs
`sproyt-account-recovery-v2` (`9b763222-baf4-5a03-a3b8-26f0bc1f71c1`).
It leaves `sproyt-account-recovery` intact, so existing email links keep their
original flow. The new blueprint exclusively owns the public recovery link and
points the shared authentication IdentificationStage at V2. Import it after all
three invitation blueprints and gate deployment on executor acceptance.

After identification the new flow pins one of two authorized branches. Active
users may reset their password; inactive users require a current, signed grant
for the exact account snapshot. Unmarked inactive users, historical V2 markers,
ambiguous addresses and changed accounts are denied. EmailStage never activates
the user. Its fresh restored token must match the versioned flow, original email
stage, recipient, branch, snapshot, marker and fixed redirect. A signed mailbox
claim carries that proof across the password prompt. The existing authentication
MFA validation remains at order 25.

The finalizer locks the user, rechecks the original token and signed claim,
expiry, snapshot, branch and grant, then writes the chosen password. It activates
only the authorized pending branch and consumes both pending marker versions.
No UserWrite follows this transaction. The redirect is fixed to
`https://sproyt.bjoroy.me/auth/login`; no `next` or invitation continuation is
recovered by this flow. Sprøyt authenticates the user again through its ordinary
login and applies its existing membership rules.

## Provenance for future signups

Additional order-43 deny-stage bindings run only in production, canary and
canary-v2 invitation flows. They stamp actual newly created inactive accounts,
after the existing V2 placeholder stamp at order 42. Existing-account branches
do not acquire grants. The policy compares the just-created pending-user
snapshot with the locked database user and requires an unusable password,
exactly the `sproyt-users` group, external type, `users/sproyt` path, no login,
no roles, sources or superuser status.

`attributes.sproyt_pending_signup_v1` is a Django timestamped signature with
salt `sproyt.pending-signup.v1`. Its payload is exactly `{v, kind, flow, nonce,
snapshot}`. A signup grant lasts 30 days; a reviewed operator grant lasts 48
hours. The snapshot binds primary key, UUID, normalized email, last_updated,
password hash fingerprint, groups, roles, sources, active/superuser state,
type/path, last_login and confirmed MFA device identities. Mutable MFA usage
counters are excluded so completing MFA does not revoke recovery.

Normal administrative saves advance last_updated and invalidate a grant even
when the account remains inactive. Relation changes are checked independently.
Mailbox verification's attribute-only saves preserve last_updated. A crash
between creation and stamping leaves an unmarked account which fails closed.
Normal invitation completion advances the snapshot and thereby invalidates any
remaining generic grant; recovery completion also removes it.

## Historical account: operator review

`tools/issue-signup-recovery-grant.py` runs inside the deployed Authentik Django
environment and defaults to dry run. It requires an exact username, expected
UUID/email, production user-write creation event, corresponding model-created
event, old recovery password-set event and a specific operator review reason.
It verifies the event account identifiers and executor paths and rejects
administrative account-change events after creation. It requires the same
restricted inactive account state, except a historical usable password is
allowed only by this explicit operator grant. It never activates the account or
sends email.

Call `main([...])` after importing the helper into an Authentik shell. The dry
run prints the account snapshot and its SHA-256 fingerprint. Review both the
account and audit history, then repeat the identical arguments with
`--apply --expected-snapshot-sha256 <reviewed-fingerprint>`. Changed state fails
closed. Keep the printed review report in the operator's deployment evidence;
it contains no password or signed grant. The account holder must still obtain
a new recovery email, pass any configured MFA and choose their own password.

## Verification and rollout gates

Run `python tools/test-signup-recovery.py` with Django installed, plus the three
existing invitation contract scripts. These model-double tests use real Django
signatures and cover stale/forged grants, original-token and branch mismatches,
expiry, administrative and relation changes, MFA changes, inactive-account
denial, replay, rate limiting and operator evidence. They do not establish real
database locking or executor behavior.

Before deploying the public recovery link change, run the deployed Authentik runtime
probe for new/interrupted registration, ordinary active reset, MFA, revoked
accounts, mailbox token expiry, administrative edits and concurrent recovery.
Keep the old blueprint installed for outstanding links. Its shared
IdentificationStage entry omits recovery_flow, so reimporting it does not
overwrite the V2 link. The final entry of the new blueprint is the sole owner of
that attribute. On a fresh bootstrap import the baseline authentication and
invitation blueprints first; the new blueprint can then resolve existing MFA
and invitation stages and set the public recovery link.

The older production/canary invitation flows retain their separate final guard
and UserWrite stages. Resuming an original flow after recovery completion must
deny, but this release does not claim atomicity if an old executor has already
passed its final guard concurrently. The new recovery flow and existing V2
invitation completion each have transactional finalizers. Group/role/source/MFA
changes are compared before completion; user-row locking alone does not serialize
all administrative relation changes. Direct database edits must explicitly revoke
pending grants. This is not a general reactivation mechanism for disabled users.
