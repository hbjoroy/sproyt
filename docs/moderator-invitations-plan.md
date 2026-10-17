# Moderator invitations and one email invitation

The Canary change lets human circle owners and moderators invite people through
the existing circle invitation controls. Members remain unable to invite. Circle
rename, deletion, role administration and agent invitation permissions retain
their separate existing rules. In-memory, SQLite and PostgreSQL repositories
must enforce the same policy, including the direct enrollment preparation query.

One email contains one Authentik `itoken` link. It works with the same local
email-bound enrollment token for an existing account or a new account. Sprøyt
continues to create an inactive local invitation, provision and email the
Authentik invitation, and activate locally only after successful API responses.
Failed delivery or activation revokes the external invitation where possible.

## Isolated Canary flow

`deploy/authentik/sproyt-invitation-canary.yaml` creates the separate flow
`sproyt-invitation-canary`, UUID `5eb6dcb6-29a8-5b7d-b424-584c59b7409c`.
Only Canary configuration selects this flow. Its stages, prompts, bindings and
expression policies have new names; the production enrollment and shared
authentication flow are unchanged. The existing password validation policy and
`sproyt-users` group are referenced without modification.

The flow permits both authenticated and anonymous browsers. Its mandatory
Invitation stage validates and consumes the single-use Authentik invitation.
The next runtime policy reads the real invitation object retained in flow
context, validates its exact flow UUID and 43-character local token, and pins
the continuation to `https://sproyt-canary.bjoroy.me/auth/login?enrollment=…`.
Prompt data and URL `next` cannot supply the token or destination. No second
lookup of the consumed Authentik invitation is needed.

A case-insensitive Authentik email lookup determines the branch. An existing
address skips profile, password, User Write and User Login stages. Verified
addresses proceed directly; an unverified address receives a separate mailbox
confirmation challenge. Only this EmailStage selects the existing recipient as
`pending_user`; it never authenticates that identity. Successful proof writes
only address-verification attributes, preserving account activation and roles.
The continuation requires
the recipient's normal OIDC authentication. Sprøyt verifies issuer, audience,
nonce, signature, a verified email claim and the local expected-email hash.
A different signed-in account cannot consume the local invitation.

A new address gets a dedicated profile prompt. Immediately before creation the
email is checked again and prompt fields are allowlisted, excluding all password
data and starting with an unverified address.
Dedicated User Write uses `always_create`: the Authentik default would otherwise
reuse and overwrite a browser's already authenticated user. It creates an
inactive external user in `users/sproyt`, adds `sproyt-users`, and sends the same
independent mailbox confirmation challenge. No usable password is set yet.
Only after confirmation does the recipient choose a password under the existing
password policy. A new-user-only `never_create` write sets that password and
activates exactly the proof-verified pending user, then User Login runs. This
ordering prevents an inviter who knows the original invitation URL from choosing
a password that the recipient unknowingly activates. The final OIDC continuation
is the same as for an existing recipient.

The mailbox proof policy checks the original EmailStage FlowToken snapshot:
exact Canary flow, email stage following Authentik's consent stage, recipient,
address and continuation. It rejects context merged from another browser flow,
and reloads the recipient under a database lock before changing only verification
attributes. Existing inactive accounts are never activated. Challenge initial
sends are limited per email address to five in five minutes through shared cache;
the EmailStage also limits resends. There is one invitation email, with a separate
confirmation email only when independent mailbox verification is needed.

Local acceptance remains atomic and single-use, with expiry and email checks.
Adding an existing circle member must preserve their owner/moderator/member role
and original join time. No arbitrary pending invitation is accepted by email.

## Limits to verify before claiming end-to-end completion

Existing legacy accounts without an address-bound verified email must complete
the fresh confirmation email; OIDC verification remains mandatory. Existing Authentik accounts outside the provider's
allowed groups can also be denied by the provider. This change does not silently
mark existing addresses verified from the invitation alone or grant provider access. Check live aggregate
configuration and demonstrate acceptance with eligible existing users. A denied
recipient needs the existing verification/access process and may need a fresh
invitation; never describe such a case as successful registration.

Authentik consumes its invitation at its first stage, so an interrupted
registration or wrong-account authentication may require a fresh invitation.
An abandoned new registration can leave an inactive placeholder account; a new
invitation does not silently activate or reset that existing account.
The local token is not consumed on a failed email match. The new-account email
recheck reduces the race with another registration but does not add a new global
unique-email database constraint. Account matching uses signed verified email,
not a username or a browser-supplied account identifier.

## Checks and delivery

Run Rust policy/repository/OIDC tests, including moderator success, ordinary
member and agent denial, wrong email, inactive/expired/reused invitation, and
existing membership role preservation. Run frontend checks for both supported
invitation surfaces. `python tools/test-invitation-canary.py` executes the actual
blueprint policy expressions against controlled contexts, including malicious
prompt data, existing accounts, concurrent account creation and wrong flow IDs.
These tests are not an Authentik importer or browser acceptance test.

Dry-run this exact blueprint with the running Authentik importer before enabling
its Canary flow ID. Preserve the original production blueprint bytes when adding
the separate file to the existing mounted ConfigMap. Publish the reviewed app
through CI/CD, pin the image digest and change only Canary Helm configuration.
Verify its public URL and OIDC callback remain on `sproyt-canary.bjoroy.me` and
its API token is supplied through the existing secret store, never source or logs.

Check a delivered invitation for an existing eligible account and a genuinely
new account, with both matching and unrelated existing browser sessions. Verify
exactly one intended membership, role preservation, replay rejection and the
Canary origin at every Sprøyt continuation. Record revisions and redacted paths,
never real invitation URLs, credentials, cookies or capabilities. Authentik and
proxy logging must not expose query capabilities; DEBUG flow logging is unsafe.
If human credentials are unavailable, record exactly which browser checks remain
unperformed; do not replace them with an assertion based only on unit tests.

Rollback disables Canary enrollment or restores its previous configuration and
application digest. Leave the isolated flow unselected until outstanding links
expire; do not alter the production flow or run the broad email-recovery rollback.

Upstream implementation contracts:
[invitation stage](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/invitation/stage.py),
[User Write](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/stages/user_write/stage.py),
[flow completion](https://github.com/goauthentik/authentik/blob/version/2026.8.2/authentik/flows/views/executor.py).
