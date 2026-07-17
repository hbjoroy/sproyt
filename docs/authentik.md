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
   a rotated session cookie without a second login.
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
