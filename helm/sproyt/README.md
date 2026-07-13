# Sproyt Helm chart

`config.oidcIssuer` and `config.oidcClientId` are required. The issuer must be
the exact discovery-capable Authentik provider URL under
`https://identity.limani-parou.com/application/o/<provider-slug>/`; the chart
deliberately has no guessed production slug.

Create the referenced Secret before installation. It must contain
`DATABASE_URL`, `SPROYT_OIDC_CLIENT_SECRET`, and `SPROYT_SESSION_KEY`.
During session-key rotation it may also contain the optional
`SPROYT_SESSION_PREVIOUS_KEYS` value.

```sh
helm upgrade --install sproyt ./helm/sproyt \
  --namespace sproyt --create-namespace \
  --set image.digest=sha256:<registry-digest> \
  --set ingress.enabled=true \
  --set ingress.host=chat.example.com \
  --set config.oidcClientId=<client-id> \
  --set config.oidcRedirectUrl=https://chat.example.com/auth/callback \
  --set config.oidcPostLogoutRedirectUrl=https://chat.example.com/
```

The pre-install/pre-upgrade Job applies additive SQLx migrations before the
Deployment rolls. Application pods never mutate schema at startup. Use an
registry digest in production; `image.tag` is retained for local and kind
workflows. The same rendered digest is used by both the migration Job and the
Deployment. The Job intentionally receives
only `DATABASE_URL` and the log format: it does not initialize OIDC and remains
available during provider outages, client-secret changes, and session-key
rotation.
