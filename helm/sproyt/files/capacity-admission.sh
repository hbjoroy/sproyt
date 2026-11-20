set -eu
tmp=${TMPDIR:-/tmp}
extra=2
if [ "$PHASE" = web ]; then
  sa=${SERVICEACCOUNT_DIR:-/var/run/secrets/kubernetes.io/serviceaccount}
  # The Table response contains readiness, image and metadata. No pod spec is requested.
  code=$(curl --silent --show-error --max-time 8 --cacert "$sa/ca.crt" \
    -H "Authorization: Bearer $(cat "$sa/token")" \
    -H 'Accept: application/json;as=Table;g=meta.k8s.io;v=v1' \
    -o "$tmp/deployment" -w '%{http_code}' \
    "https://kubernetes.default.svc/apis/apps/v1/namespaces/$NAMESPACE/deployments/$WEB_NAME?includeObject=Metadata")
  case "$code" in
    200)
      compact=$(tr -d '[:space:]' <"$tmp/deployment")
      # Missing columns, old images and partially applied/rolling Deployments all cost full overlap.
      if ! printf '%s' "$compact" | grep -Fq "\"sproyt.me/web-capacity-revision\":\"$WEB_REVISION\"" ||
         ! printf '%s' "$compact" | grep -Fq "\"cells\":[\"$WEB_NAME\",\"$WEB_REPLICAS/$WEB_REPLICAS\",$WEB_REPLICAS,$WEB_REPLICAS," ||
         ! printf '%s' "$compact" | grep -Fq "\"sproyt\",\"$WEB_IMAGE\","; then
        extra=$WEB_OVERLAP
      fi
      ;;
    404) extra=$WEB_OVERLAP ;;
    *) echo "Capacity check could not read Deployment (HTTP $code)." >&2; exit 1 ;;
  esac
fi
for sample in 1 2 3; do
  result=$(psql "$DATABASE_URL" -XqAt -v ON_ERROR_STOP=1 -v extra="$extra" -f "$tmp/admission.sql")
  case "$result" in
    *'|1') echo "Capacity sample $sample: adjusted headroom ${result%|*}, phase $PHASE, increment $extra, reserve 2." ;;
    *) echo "Capacity held: sample $sample, adjusted headroom ${result%|*}, phase $PHASE, increment $extra, reserve 2." >&2; exit 1 ;;
  esac
  if [ "$sample" -lt 3 ]; then sleep 5; fi
done
