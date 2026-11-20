#!/usr/bin/env bash
set -euo pipefail
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
export TMPDIR="$tmp" SERVICEACCOUNT_DIR="$tmp"
touch "$tmp/token" "$tmp/ca.crt"
cat >"$tmp/curl" <<'MOCK'
#!/bin/sh
cp "$TMPDIR/table" "$TMPDIR/deployment"
printf %s "$HTTP_CODE"
MOCK
cat >"$tmp/psql" <<'MOCK'
#!/bin/sh
echo "$*" >> "$TMPDIR/calls"
echo "$DECISION"
MOCK
printf '#!/bin/sh\nexit 0\n' >"$tmp/sleep"
chmod +x "$tmp/curl" "$tmp/psql" "$tmp/sleep"
export PATH="$tmp:$PATH" WEB_NAME=web WEB_IMAGE=local/app@sha256:fixture WEB_REPLICAS=2 WEB_OVERLAP=13
export NAMESPACE=sproyt DATABASE_URL=unused WEB_REVISION=fixture HTTP_CODE=200 DECISION='15|1'
printf '{"rows":[{"cells":["web","2/2",2,2,"1d","sproyt","%s","selector"],"object":{"metadata":{"labels":{"sproyt.me/web-capacity-revision":"fixture"}}}}]}' "$WEB_IMAGE" > "$tmp/table"
run() {
  : > "$tmp/calls"
  PHASE="$1" sh helm/sproyt/files/capacity-admission.sh
  test "$(grep -Fc -- "-v extra=$2 " "$tmp/calls")" -eq 3
}
run maintenance 2
run web 2
WEB_REVISION=changed run web 13
WEB_IMAGE=other run web 13
WEB_REPLICAS=1 run web 13
HTTP_CODE=404 run web 13
if HTTP_CODE=403 run web 2; then exit 1; fi
if DECISION='1|0' run maintenance 2; then exit 1; fi
render() {
  helm template sproyt helm/sproyt -n sproyt --set capacityAdmission.enabled=true \
    --set 'capacityAdmission.apiServerCidrs[0]=127.0.0.1/32' --set heart.enabled=true \
    --set config.oidcClientId=fixture --set config.environment=test --set mediaWorker.enabled=true "$@"
}
revision() { sed -n 's/^    sproyt.me\/web-capacity-revision: "\([^"]*\)"/\1/p'; }
base=$(render | revision)
test "${#base}" = 40
test "$base" = "$(render --set heart.image.tag=unrelated | revision)"
test "$base" != "$(render --set image.tag=new | revision)"
if render --set config.dbMaxConnections=5 >/dev/null 2>&1; then exit 1; fi
if render --set heart.dbMaxConnections=3 >/dev/null 2>&1; then exit 1; fi
