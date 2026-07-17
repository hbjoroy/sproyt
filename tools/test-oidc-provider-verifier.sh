#!/usr/bin/env bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
issuer=https://identity.limani-parou.com/application/o/sproyt-ci/
fixture="$root/tools/fixtures/authentik-discovery.json"
verifier="$root/tools/verify-oidc-provider.sh"

bash "$verifier" "$issuer" "$fixture" >/dev/null

invalid=$(mktemp)
trap 'rm -f "$invalid"' EXIT
jq '.issuer = "https://attacker.example/application/o/sproyt-ci/"' "$fixture" >"$invalid"

if bash "$verifier" "$issuer" "$invalid" >/dev/null 2>&1; then
  echo "OIDC verifier accepted a discovery document with a mismatched issuer" >&2
  exit 1
fi

jq 'del(.code_challenge_methods_supported)' "$fixture" >"$invalid"
if bash "$verifier" "$issuer" "$invalid" >/dev/null 2>&1; then
  echo "OIDC verifier accepted a provider without PKCE S256" >&2
  exit 1
fi

jq '.scopes_supported -= ["offline_access"]' "$fixture" >"$invalid"
bash "$verifier" "$issuer" "$invalid" >/dev/null 2>&1

echo "OIDC provider verifier contract passed"
