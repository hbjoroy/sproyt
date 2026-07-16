#!/usr/bin/env bash
set -euo pipefail

chart=${1:-helm/sproyt}
digest=${2:-sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa}
issuer=https://identity.limani-parou.com/application/o/ci-only/
helm_command=${HELM:-helm}

common=(
  --namespace sproyt
  --set "config.oidcIssuer=$issuer"
  --set config.oidcClientId=ci-client
)

if "$helm_command" template sproyt "$chart" "${common[@]}" >/dev/null 2>&1; then
  echo "production rendering unexpectedly accepted an empty image digest" >&2
  exit 1
fi

rendered=$(mktemp)
trap 'rm -f "$rendered"' EXIT
"$helm_command" template sproyt "$chart" "${common[@]}" \
  --set "image.digest=$digest" \
  --set imagePullSecrets[0].name=oci-pull-secret \
  --set config.heartUrl=http://heart.heart.svc.cluster.local:3000 \
  --set 'networkPolicy.heartNamespaceSelector.matchLabels.kubernetes\.io/metadata\.name=heart' \
  --set networkPolicy.heartPodSelector.matchLabels.app=heart \
  --set networkPolicy.heartPort=3000 >"$rendered"

image="oci.bjoroy.me/sproyt/sproyt@$digest"
test "$(grep -F -c "image: \"$image\"" "$rendered")" -eq 2
grep -F -q "checksum/config:" "$rendered"
grep -F -q "kubernetes.io/metadata.name: kube-system" "$rendered"
grep -F -q "kubernetes.io/metadata.name: heart" "$rendered"
grep -F -q "port: 3000" "$rendered"
grep -F -q "app: heart" "$rendered"
grep -F -q "podSelector: {}" "$rendered"
test "$(grep -F -c -- "- name: oci-pull-secret" "$rendered")" -eq 2
test "$(grep -F -c "serviceAccountName: default" "$rendered")" -eq 1

echo "Helm delivery contract verified for $image"
