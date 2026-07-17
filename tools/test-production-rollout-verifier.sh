#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
mock_dir=$(mktemp -d)
trap 'rm -rf "$mock_dir"' EXIT

cat >"$mock_dir/kubectl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$*" in
  *"get application rocket-applications -o json")
    printf '%s\n' '{"status":{"sync":{"status":"Synced","revision":"9cb3db8ce5545c69026b5cf39c239ad286879dbf"},"health":{"status":"Healthy"}}}'
    ;;
  *"get application sproyt -o json")
    printf '%s\n' '{"spec":{"sources":[{"repoURL":"git@github.com:hbjoroy/rocket-applications.git","targetRevision":"main"},{"repoURL":"git@github.com:hbjoroy/sproyt.git","targetRevision":"6355c13720c9bce94d3a55b9c485876c10b915d7"}]},"status":{"sync":{"status":"Synced"},"health":{"status":"Healthy"}}}'
    ;;
  *"get deployment sproyt-sproyt -o json")
    image=${MOCK_IMAGE:-oci.bjoroy.me/sproyt/sproyt@sha256:e8a6a49cbe85c7b2b9578261b8ec565742601b5288fe81d8d57046adf0c858ce}
    printf '{"metadata":{"generation":4},"spec":{"replicas":2,"template":{"spec":{"containers":[{"name":"sproyt","image":"%s"}]}}},"status":{"observedGeneration":4,"updatedReplicas":2,"availableReplicas":2}}\n' "$image"
    ;;
  *"get endpointslice"*)
    printf '%s\n' '{"items":[{"endpoints":[{"conditions":{"ready":true}},{"conditions":{"ready":true}}]}]}'
    ;;
  *) echo "unexpected kubectl call: $*" >&2; exit 1 ;;
esac
EOF

cat >"$mock_dir/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
url=${!#}
case "$url" in
  https://sproyt.bjoroy.me/healthz) printf 'ok\n' ;;
  https://sproyt.bjoroy.me/readyz) printf 'ready\n' ;;
  https://sproyt.bjoroy.me/)
    printf '%s\n' '303 https://sproyt.bjoroy.me/auth/login'
    ;;
  https://sproyt.bjoroy.me/auth/login)
    printf '%s\n' '303 https://sproyt-security.bjoroy.me/application/o/authorize/?client_id=test'
    ;;
  https://sproyt-security.bjoroy.me/application/o/sproyt/.well-known/openid-configuration)
    cat <<'JSON'
{"issuer":"https://sproyt-security.bjoroy.me/application/o/sproyt/","authorization_endpoint":"https://sproyt-security.bjoroy.me/application/o/authorize/","token_endpoint":"https://sproyt-security.bjoroy.me/application/o/token/","jwks_uri":"https://sproyt-security.bjoroy.me/application/o/sproyt/jwks/","userinfo_endpoint":"https://sproyt-security.bjoroy.me/application/o/userinfo/","end_session_endpoint":"https://sproyt-security.bjoroy.me/application/o/sproyt/end-session/","response_types_supported":["code"],"code_challenge_methods_supported":["S256"],"scopes_supported":["openid","profile","email","offline_access"],"grant_types_supported":["authorization_code","refresh_token"],"token_endpoint_auth_methods_supported":["client_secret_basic"],"id_token_signing_alg_values_supported":["RS256"]}
JSON
    ;;
  *) echo "unexpected curl call: $*" >&2; exit 1 ;;
esac
EOF

chmod +x "$mock_dir/kubectl" "$mock_dir/curl"
export PATH="$mock_dir:$PATH"

revision=6355c13720c9bce94d3a55b9c485876c10b915d7
digest=sha256:e8a6a49cbe85c7b2b9578261b8ec565742601b5288fe81d8d57046adf0c858ce
gitops=9cb3db8ce5545c69026b5cf39c239ad286879dbf

result=$(bash "$repo_root/tools/verify-production-rollout.sh" "$revision" "$digest" "$gitops")
jq -e '.status == "verified" and .replicas == 2 and .readyEndpoints == 2' \
  >/dev/null <<<"$result"

export MOCK_IMAGE=oci.bjoroy.me/sproyt/sproyt@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
if bash "$repo_root/tools/verify-production-rollout.sh" "$revision" "$digest" "$gitops" \
  >/dev/null 2>&1; then
  echo "verifier accepted an unexpected deployment image" >&2
  exit 1
fi

echo "production rollout verifier contract passed"
