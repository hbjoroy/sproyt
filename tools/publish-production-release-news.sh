#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 || -z "$3" || ! -f "$4" ]]; then
  echo 'usage: publish-production-release-news.sh <app-revision> <image-digest> <gitops-revision> <summary-file>' >&2
  exit 2
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
rollout=$(ARGOCD_NAMESPACE=argocd \
  SPROYT_NAMESPACE=sproyt \
  SPROYT_ROOT_APPLICATION=rocket-applications \
  SPROYT_APPLICATION=sproyt \
  SPROYT_DEPLOYMENT=sproyt-sproyt \
  SPROYT_SERVICE=sproyt-sproyt \
  SPROYT_PUBLIC_URL=https://sproyt.bjoroy.me \
  SPROYT_OIDC_ISSUER=https://sproyt-security.bjoroy.me/application/o/sproyt/ \
  SPROYT_OIDC_AUTHORIZATION_PREFIX=https://sproyt-security.bjoroy.me/application/o/authorize/ \
  bash "$script_dir/verify-production-rollout.sh" "$1" "$2" "$3")

news=$(kubectl -n sproyt exec -i deployment/sproyt-sproyt -c sproyt -- \
  /app/sproyt release-news "$1" < "$4")
jq -n --argjson rollout "$rollout" --argjson news "$news" \
  '{rollout:$rollout, releaseNews:$news}'
