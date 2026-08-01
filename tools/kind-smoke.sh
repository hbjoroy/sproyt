#!/usr/bin/env bash
set -euo pipefail

: "${SPROYT_IMAGE:?set SPROYT_IMAGE to the image loaded into kind}"

namespace=sproyt-smoke
release=sproyt
database_url='postgres://sproyt:sproyt-smoke@postgres:5432/sproyt'
session_key='AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA'

kubectl create namespace "$namespace"
cat <<'YAML' | kubectl -n "$namespace" apply -f -
apiVersion: apps/v1
kind: Deployment
metadata:
  name: postgres
spec:
  replicas: 1
  selector:
    matchLabels:
      app: postgres
  template:
    metadata:
      labels:
        app: postgres
    spec:
      containers:
        - name: postgres
          image: postgres:17-alpine
          env:
            - name: POSTGRES_USER
              value: sproyt
            - name: POSTGRES_PASSWORD
              value: sproyt-smoke
            - name: POSTGRES_DB
              value: sproyt
YAML
kubectl -n "$namespace" expose deployment postgres --port=5432 --target-port=5432
kubectl -n "$namespace" rollout status deployment/postgres --timeout=120s
for _ in {1..30}; do
  # The image briefly starts an init-only PostgreSQL server on the Unix
  # socket before replacing it with the final TCP-listening server. Waiting
  # on TCP avoids treating that transient bootstrap process as application
  # readiness.
  if kubectl -n "$namespace" exec deployment/postgres -- \
    pg_isready -h 127.0.0.1 -p 5432 -U sproyt -d sproyt; then
    break
  fi
  sleep 1
done
kubectl -n "$namespace" exec deployment/postgres -- \
  pg_isready -h 127.0.0.1 -p 5432 -U sproyt -d sproyt

kubectl -n "$namespace" create secret generic sproyt-ci \
  --from-literal=DATABASE_URL="$database_url" \
  --from-literal=SPROYT_OIDC_CLIENT_SECRET=ci-only \
  --from-literal=SPROYT_SESSION_KEY="$session_key"

image_repository=${SPROYT_IMAGE%:*}
image_tag=${SPROYT_IMAGE##*:}
common_values=(
  --namespace "$namespace"
  --set "image.repository=$image_repository"
  --set "image.tag=$image_tag"
  --set image.pullPolicy=Never
  --set secret.existingSecret=sproyt-ci
  --set config.environment=test
  --set config.authMode=development
  --set config.oidcIssuer=https://sproyt-security.bjoroy.me/application/o/sproyt/
  --set config.oidcClientId=ci-only
  --set networkPolicy.enabled=true
)

helm upgrade --install "$release" helm/sproyt "${common_values[@]}" \
  --set replicaCount=2 --wait --timeout=3m
kubectl -n "$namespace" rollout status deployment/sproyt-sproyt --timeout=120s
test "$(kubectl -n "$namespace" get deployment sproyt-sproyt -o jsonpath='{.status.readyReplicas}')" = 2

kubectl -n "$namespace" port-forward service/sproyt-sproyt 19010:80 > /tmp/sproyt-port-forward.log 2>&1 &
port_forward_pid=$!
trap 'kill "$port_forward_pid" 2>/dev/null || true' EXIT
for _ in {1..30}; do
  if curl --fail --silent http://127.0.0.1:19010/readyz >/dev/null; then
    break
  fi
  sleep 1
done
curl --fail --silent http://127.0.0.1:19010/healthz | grep --quiet '^ok$'
curl --fail --silent http://127.0.0.1:19010/readyz | grep --quiet '^ready$'

helm upgrade "$release" helm/sproyt "${common_values[@]}" \
  --set replicaCount=1 --wait --timeout=3m
test "$(kubectl -n "$namespace" get deployment sproyt-sproyt -o jsonpath='{.status.readyReplicas}')" = 1

helm rollback "$release" 1 --namespace "$namespace" --wait --timeout=3m
kubectl -n "$namespace" rollout status deployment/sproyt-sproyt --timeout=120s
test "$(kubectl -n "$namespace" get deployment sproyt-sproyt -o jsonpath='{.status.readyReplicas}')" = 2
test "$(helm history "$release" --namespace "$namespace" --output json | jq -r '.[-1].status')" = deployed
