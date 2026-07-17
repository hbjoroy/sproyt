#!/usr/bin/env bash
set -euo pipefail

issuer=${1:?usage: verify-oidc-provider.sh <issuer> [discovery-json]}
discovery_file=${2:-}

case "$issuer" in
  https://identity.limani-parou.com/application/o/*/)
    ;;
  *)
    echo "issuer must match https://identity.limani-parou.com/application/o/<provider-slug>/" >&2
    exit 1
    ;;
esac

if [[ -n "$discovery_file" ]]; then
  discovery=$(cat "$discovery_file")
else
  discovery_url="${issuer}.well-known/openid-configuration"
  discovery=$(curl --fail --silent --show-error \
    --connect-timeout 10 --max-time 30 \
    --header 'Accept: application/json' \
    "$discovery_url")
fi

jq --exit-status --arg issuer "$issuer" '
  def has_value($array; $value): ($array // []) | index($value) != null;
  def trusted_endpoint:
    type == "string" and startswith("https://identity.limani-parou.com/");

  .issuer == $issuer and
  (.authorization_endpoint | trusted_endpoint) and
  (.token_endpoint | trusted_endpoint) and
  (.jwks_uri | trusted_endpoint) and
  (.userinfo_endpoint | trusted_endpoint) and
  (.end_session_endpoint | trusted_endpoint) and
  has_value(.response_types_supported; "code") and
  has_value(.code_challenge_methods_supported; "S256") and
  has_value(.scopes_supported; "openid") and
  has_value(.scopes_supported; "profile") and
  has_value(.scopes_supported; "email") and
  has_value((.grant_types_supported // ["authorization_code", "refresh_token"]); "authorization_code") and
  has_value((.grant_types_supported // ["authorization_code", "refresh_token"]); "refresh_token") and
  ((.token_endpoint_auth_methods_supported // ["client_secret_basic"]) |
    (index("client_secret_basic") != null or index("client_secret_post") != null)) and
  ((.id_token_signing_alg_values_supported // []) | length > 0)
' >/dev/null <<<"$discovery"

if ! jq --exit-status '(.scopes_supported // []) | index("offline_access") != null' \
  >/dev/null <<<"$discovery"; then
  echo "warning: provider does not advertise offline_access; verify refresh-token issuance in the live acceptance" >&2
fi

echo "OIDC provider contract verified for $issuer"
jq --raw-output '
  "authorization_endpoint=" + .authorization_endpoint,
  "token_endpoint=" + .token_endpoint,
  "userinfo_endpoint=" + .userinfo_endpoint,
  "end_session_endpoint=" + .end_session_endpoint
' <<<"$discovery"
