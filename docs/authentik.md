# Activate Authentik OIDC

Sproyt uses a confidential OpenID Connect client with Authorization Code +
PKCE S256. The application discovers every provider endpoint from the issuer;
do not copy individual Authentik endpoint URLs into Sproyt configuration.

This guide deliberately separates provider administration, secret handling,
deployment and browser acceptance. No command below prints or commits the
client secret.

## Create the application and provider

In the Authentik admin interface for the `sproyt` provider:

1. Create an application/provider pair under **Applications** using provider
   type **OAuth2/OpenID Connect**.
2. Use the stable provider slug `sproyt`. Changing the slug changes the issuer
   and therefore the identity namespace.
3. Select a signing key and client type **Confidential**.
4. Enable Authorization Code and refresh-token grants if the installed
   Authentik version exposes grant selection. Do not enable the implicit or
   password grant for Sproyt.
5. Add one **Strict** authorization redirect URI:
   `https://sproyt.bjoroy.me/auth/callback`. Never leave redirect URIs empty and
   do not use a wildcard or regular expression.
6. If the installed version distinguishes logout redirect URIs, add
   `https://sproyt.bjoroy.me/` as a strict logout URI. Otherwise configure it as
   the provider's permitted post-logout redirect.
7. Include the standard `openid`, `profile`, `email` and `offline_access`
   scopes/property mappings. Sproyt uses `sub` as the stable external identity
   and uses `name` or `preferred_username` only as display text.
8. Bind only the users or groups that should be allowed into the private beta.

Record the provider slug, client ID and client secret in the deployment secret
store. The expected issuer is:

```text
https://sproyt-security.bjoroy.me/application/o/sproyt/
```

Authentik recommends Authorization Code with PKCE and strict redirect URIs.
See its [OAuth2/OpenID provider documentation](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/).

## Verify discovery before deployment

The verifier checks the exact issuer, trusted HTTPS endpoints, authorization
code, refresh tokens, PKCE S256, core scopes, userinfo, JWKS, a supported
confidential-client token authentication method and RP-initiated logout. It
does not accept or transmit the client secret.

Authentik can support the requested `offline_access` behaviour without listing
that value in `scopes_supported`. The verifier warns rather than fails in that
case; actual refresh-token issuance remains a mandatory live acceptance check.

From this checkout or the cluster SSH host (requires `bash`, `curl` and `jq`):

```sh
OIDC_ISSUER='https://sproyt-security.bjoroy.me/application/o/sproyt/'
bash tools/verify-oidc-provider.sh "$OIDC_ISSUER"
```

Do not deploy if the command fails. Compare Authentik's displayed issuer with
the discovery document rather than guessing the slug or removing the trailing
slash.

On 2026-07-17, the Cloudflare-exposed provider slug `sproyt` passed this
contract for the
exact issuer, trusted endpoints, Authorization Code, refresh-token grant, PKCE
S256, core scopes, confidential-client authentication, userinfo, JWKS and
RP-initiated logout. It did not advertise `offline_access`; refresh-token
issuance must therefore be demonstrated during the browser acceptance below.

## Supply secrets and deploy

Follow [running.md](running.md) to create the namespaced Kubernetes Secret and
Helm values. Keep these values out of shell history where the SSH environment
records commands; entering them through the platform's protected secret UI or
an interactive `read -s` is preferable. At minimum, unset the transient shell
variables after the Secret has been applied:

```sh
unset OIDC_CLIENT_SECRET SPROYT_SESSION_KEY SPROYT_DB_PASSWORD
```

The Sproyt Secret must contain `SPROYT_OIDC_CLIENT_SECRET` and the shared
32-byte `SPROYT_SESSION_KEY`. The non-secret issuer, client ID, callback and
post-logout URL belong in Helm values. A Secret rotation must also change
`secret.rolloutChecksum` so both replicas roll.

## Live acceptance

Record timestamps, the application commit/image digest, Helm revision,
provider slug (not its secret), test-user identity and results for each check:

1. Open a private browser window at `https://sproyt.bjoroy.me/auth/login`.
   Confirm the redirect stays on `sproyt-security.bjoroy.me`, uses
   `response_type=code`, requests `openid profile email offline_access`, and
   contains `state`, `nonce`, `code_challenge` and
   `code_challenge_method=S256`.
2. Sign in as an allowed beta user. Confirm the callback returns to `/`, the
   page and WebSocket work, and the `sproyt_session` cookie is `Secure`,
   `HttpOnly` and `SameSite=Lax`. Never copy the cookie value into evidence.
3. Leave the page open through a renewal or invoke `POST /auth/refresh` from
   the same authenticated browser session. Confirm success, continued chat and
   a rotated session cookie without a second login. The browser schedules the
   renewal about 60 seconds before token expiry and silently replaces the
   WebSocket so it uses the rotated cookie without showing a reconnect state.
4. Visit `/auth/logout`. Confirm the local cookie is cleared, Authentik accepts
   the registered post-logout URL, and protected HTTP/WebSocket access now
   requires login.
5. Start a second login and alter or reuse the returned `state`; it must fail.
   The automated provider contract already covers invalid nonce, expired login
   transaction, expired session and rotated signing keys without exposing
   those attacks to the production provider.
6. Disable or unbind the test user in Authentik. Within the periodic
   revalidation window (30 seconds), the open WebSocket must close with policy
   code 1008 and new protected requests must fail. Restore access only after
   recording the result.
7. Rotate the Authentik signing key, then perform a new login. Sproyt must
   refresh discovery/JWKS and accept the new valid token while rejecting an
   invalid signature.
8. Inspect a structured log sample. It may contain request IDs and internal
   entity IDs, but no authorization code, access/refresh/ID token, cookie,
   client secret or private message body.

Attach this evidence to S-11 and the production release checklist. A green
offline CI contract is not a substitute for these live checks.

## Invite a new Sprøyt user

Sprøyt can create a combined, single-use enrollment link for a circle owner.
The link opens the Authentik enrollment flow, creates an external user in the
`sproyt-users` group, signs the user in, and returns through `/auth/login`.
Sprøyt then consumes the e-mail-bound invitation once and adds the user to the
intended circle automatically. Sprøyt does not create passwords or receive
them.

The production flow must have slug `sproyt-invitation-enrollment`, designation
`enrollment`, authentication `require_unauthenticated`, and an Invitation stage
with **Continue flow without invitation** disabled. The current production
flow UUID is `dcde5ce9-ca43-4003-8d0c-762e8554650c`; treat a changed UUID as a
reviewed configuration change. Its User Write stage must create external users
in `sproyt-users` so the OIDC application binding admits them.

Create a dedicated Authentik service account such as `sproyt-enrollment`, then
create an expiring API token for it. Grant only the invitation permissions
needed by Sprøyt: `authentik_stages_invitation.add_invitation`,
`authentik_stages_invitation.view_invitation`, and
`authentik_stages_invitation.delete_invitation`. These respectively create an
invitation, queue its enrollment email, and clean it up when Sprøyt cannot
activate it locally. Do not make the account a superuser and do not reuse the
OIDC client secret. Authentik's
[service-account guidance](https://docs.goauthentik.io/users-sources/user/account-types/service-accounts/)
recommends a separate least-privilege account and API token for automation.

Store the token only as `SPROYT_AUTHENTIK_API_TOKEN` in the existing namespaced
Sprøyt Secret. Configure the non-secret values in Helm:

```yaml
config:
  authentikApiUrl: http://authentik-server.authentik.svc.cluster.local
  authentikPublicUrl: https://sproyt-security.bjoroy.me
  authentikEnrollmentFlowId: dcde5ce9-ca43-4003-8d0c-762e8554650c
  authentikEnrollmentFlowSlug: sproyt-invitation-enrollment
  publicUrl: https://sproyt.bjoroy.me
```

The chart admits port 80 only to pods labelled as the Authentik server in the
`authentik` namespace. The public enrollment link remains HTTPS. If the token
is absent the feature fails closed with a visible unavailable message; if the
token is present but the flow ID is missing or invalid, Sprøyt refuses to
start. Created invitations expire after 48 hours and are single-use.

Acceptance requires all of the following:

1. A circle owner can enter an email and Authentik queues the enrollment email
   through Brevo. The owner receives no secret or API response details in logs.
2. A non-owner gets HTTP 403 and no Authentik invitation is created.
3. Following the link creates one external `sproyt-users` user, signs them in,
   returns to Sprøyt, and adds them to the intended circle without another
   confirmation step.
4. Reusing the Authentik invitation fails, and an expired link cannot enroll.
5. Revoking the service token disables only new-user invitations; existing
   login and chat remain available.

## Email verification and account recovery

The versioned blueprint is `deploy/authentik/sproyt-email-recovery.yaml`. It
adds a recovery link to the Sprøyt identification stage, configures a dedicated
Sprøyt user-settings flow, and marks email received through a single-use Sprøyt
invitation as verified.

Recovery is deliberately transitional: an account with a registered email can
recover even when its legacy `email_verified` attribute was absent. The email
token expires after 30 minutes. A dedicated, fail-closed policy limits initial
email sends to five per recipient per five minutes; Authentik's Email Stage
also limits resends in an existing flow. Existing MFA is required; users
without configured MFA are not blocked. Inactive users are not automatically
reactivated.

The user-settings flow does not persist a changed email until the recipient
opens the confirmation link. An unchanged address already marked verified does
not send another email. The Sprøyt brand points to this dedicated settings flow
so the default settings flow for other Authentik brands remains unchanged.
A guard before the write stage reads the verified address and user from the
original email token plan. It denies the flow if a newer, unverified address in
the same browser session no longer matches the link that was opened.
Verification is bound to the exact address in
`attributes.email_verified_address`; the Sprøyt-specific OIDC email mapping
only emits `email_verified: true` while that address still matches the user's
current email. The built-in settings flow clears this state when it changes an
address, so a direct settings URL cannot leave a stale verification claim.

At initial activation, mark only existing human accounts with a non-empty
email as verified, store the same address in `email_verified_address`, and add
`email_verified_migrated_2026_09_15: true`. The marker
makes the one-time trust decision auditable and lets rollback remove only the
migrated attributes. Do not include service accounts or accounts without email.
Successful later verification removes the migration marker, so rollback never
undoes verification completed by the user after migration.
Also store the exact previous presence and values in
`email_verified_migration_previous_2026_09_15`; this makes rollback reversible
even if a pre-existing account already carried one of the attributes.

Before applying, dry-run the blueprint with the running Authentik version and
record the current Sprøyt identification and brand settings. After applying,
verify the stage order, the linked recovery/settings flows, the migrated user
counts, SMTP delivery, and a complete recovery with a dedicated test account.
Never test password recovery against an owner's production account.

### Production activation 2026-09-15

The blueprint passed Authentik 2026.8.2's own dry-run before activation. Helm
revision 38 is the pre-change deployment and revision 39 mounts the blueprint.
Fifteen human accounts with email were migrated; eighteen human accounts
without email were left unverified. Anonymous inspection confirmed that the
Sprøyt login advertises the recovery flow and that the recovery flow starts
with email/username identification.

Isolated live-code policy tests (without sending mail) confirmed that initial
sends 1-5 are allowed and send 6 is denied within the window. They also
confirmed that a matching email token passes, an A-to-B address change followed
by the old A token is denied, and the Sprøyt OIDC mapping returns true only for
an address-bound verified account.

For rollback, first change the blueprint instantiate label to `false` and
update its ConfigMap. Then run
`deploy/authentik/rollback-email-recovery.py` through `ak shell`; it restores
the previous identification, brand and OIDC mappings, removes bindings added to
shared flows, expires outstanding recovery/settings tokens, and restores the
exact pre-migration attributes only for accounts still carrying the migration
marker. Finally, Helm revision 38 can be restored to remove the blueprint mount.
