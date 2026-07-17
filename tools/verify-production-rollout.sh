#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
usage: verify-production-rollout.sh <app-revision> <image-digest> [gitops-revision]

Read-only verification of the Argo CD and Kubernetes rollout plus the public
anonymous OIDC boundary. The current kubectl context must point at the target
cluster. gitops-revision is optional.
EOF
  exit 2
}

app_revision=${1:-}
image_digest=${2:-}
gitops_revision=${3:-}
[[ -n "$app_revision" && "$app_revision" =~ ^[0-9a-f]{40}$ ]] || usage
[[ "$image_digest" =~ ^sha256:[0-9a-f]{64}$ ]] || usage
if [[ -n "$gitops_revision" && ! "$gitops_revision" =~ ^[0-9a-f]{7,40}$ ]]; then
  usage
fi

for command in kubectl jq curl; do
  command -v "$command" >/dev/null || {
    echo "required command is missing: $command" >&2
    exit 1
  }
done

argocd_namespace=${ARGOCD_NAMESPACE:-argocd}
namespace=${SPROYT_NAMESPACE:-sproyt}
root_application=${SPROYT_ROOT_APPLICATION:-rocket-applications}
application=${SPROYT_APPLICATION:-sproyt}
deployment=${SPROYT_DEPLOYMENT:-sproyt-sproyt}
service=${SPROYT_SERVICE:-sproyt-sproyt}
public_url=${SPROYT_PUBLIC_URL:-https://sproyt.bjoroy.me}
issuer=${SPROYT_OIDC_ISSUER:-https://sproyt-security.bjoroy.me/application/o/sproyt/}
authorization_prefix=${SPROYT_OIDC_AUTHORIZATION_PREFIX:-https://sproyt-security.bjoroy.me/application/o/authorize/}
expected_image="oci.bjoroy.me/sproyt/sproyt@$image_digest"

root=$(kubectl -n "$argocd_namespace" get application "$root_application" -o json)
child=$(kubectl -n "$argocd_namespace" get application "$application" -o json)
workload=$(kubectl -n "$namespace" get deployment "$deployment" -o json)

jq -e '.status.sync.status == "Synced" and .status.health.status == "Healthy"' \
  >/dev/null <<<"$root"
jq -e '.status.sync.status == "Synced" and .status.health.status == "Healthy"' \
  >/dev/null <<<"$child"

actual_app_revision=$(jq -r \
  '.spec.sources[] | select(.repoURL | endswith("/sproyt.git")) | .targetRevision' \
  <<<"$child")
[[ "$actual_app_revision" == "$app_revision" ]] || {
  echo "Argo CD child revision mismatch: expected $app_revision, got $actual_app_revision" >&2
  exit 1
}

if [[ -n "$gitops_revision" ]]; then
  actual_gitops_revision=$(jq -r '.status.sync.revision' <<<"$root")
  [[ "$actual_gitops_revision" == "$gitops_revision"* ]] || {
    echo "Argo CD root revision mismatch: expected prefix $gitops_revision, got $actual_gitops_revision" >&2
    exit 1
  }
fi

actual_image=$(jq -r '.spec.template.spec.containers[] | select(.name == "sproyt") | .image' \
  <<<"$workload")
[[ "$actual_image" == "$expected_image" ]] || {
  echo "deployment image mismatch: expected $expected_image, got $actual_image" >&2
  exit 1
}

jq -e '
  (.spec.replicas >= 2) and
  (.status.observedGeneration == .metadata.generation) and
  (.status.updatedReplicas == .spec.replicas) and
  (.status.availableReplicas == .spec.replicas) and
  (.status.unavailableReplicas // 0 == 0)
' >/dev/null <<<"$workload"

ready_endpoints=$(kubectl -n "$namespace" get endpointslice \
  -l "kubernetes.io/service-name=$service" -o json | jq \
  '[.items[].endpoints[] | select(.conditions.ready != false)] | length')
[[ "$ready_endpoints" -ge 2 ]] || {
  echo "expected at least two ready service endpoints, got $ready_endpoints" >&2
  exit 1
}

health=$(curl --fail --silent --show-error --connect-timeout 10 --max-time 30 \
  "$public_url/healthz")
readiness=$(curl --fail --silent --show-error --connect-timeout 10 --max-time 30 \
  "$public_url/readyz")
[[ "$health" == "ok" ]] || { echo "unexpected health response: $health" >&2; exit 1; }
[[ "$readiness" == "ready" ]] || {
  echo "unexpected readiness response: $readiness" >&2
  exit 1
}

read -r root_status root_redirect < <(curl --silent --show-error --output /dev/null \
  --connect-timeout 10 --max-time 30 --write-out '%{http_code} %{redirect_url}\n' \
  "$public_url/")
[[ "$root_status" == "303" && "$root_redirect" == "$public_url/auth/login" ]] || {
  echo "anonymous root did not redirect to login: $root_status $root_redirect" >&2
  exit 1
}

read -r login_status login_redirect < <(curl --silent --show-error --output /dev/null \
  --connect-timeout 10 --max-time 30 --write-out '%{http_code} %{redirect_url}\n' \
  "$public_url/auth/login")
[[ "$login_status" == "303" && "$login_redirect" == "${authorization_prefix}"* ]] || {
  echo "login did not redirect to the configured issuer: $login_status $login_redirect" >&2
  exit 1
}

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
"$script_dir/verify-oidc-provider.sh" "$issuer" >/dev/null

jq -n \
  --arg appRevision "$app_revision" \
  --arg image "$expected_image" \
  --argjson replicas "$(jq '.spec.replicas' <<<"$workload")" \
  --argjson readyEndpoints "$ready_endpoints" \
  --arg publicUrl "$public_url" \
  --arg issuer "$issuer" \
  '{status:"verified", appRevision:$appRevision, image:$image, replicas:$replicas,
    readyEndpoints:$readyEndpoints, publicUrl:$publicUrl, issuer:$issuer}'
