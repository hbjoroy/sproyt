#!/usr/bin/env bash
# Exercise the published psql files against fresh synthetic data on the CI PostgreSQL service.
set -euo pipefail
[[ ${PGHOST:?} == 127.0.0.1 && ${PGUSER:?} == sproyt ]] || exit 1
export PGDATABASE=sproyt_operator_fixture
createdb --template=template0 "$PGDATABASE"
trap 'dropdb "$PGDATABASE"' EXIT
psql=(psql -X -qAt -v ON_ERROR_STOP=1)
for migration in migrations/postgres/*.sql; do
  "${psql[@]}" -1 -f "$migration" >/dev/null
done
alice=00000000-0000-4000-8000-000000000001
bob=00000000-0000-4000-8000-000000000002
absent=00000000-0000-4000-8000-000000000003
"${psql[@]}" -v alice="$alice" -v bob="$bob" <<'SQL'
INSERT INTO users (id, kind, display_name)
VALUES (:'alice', 'human', 'Operator fixture A'), (:'bob', 'human', 'Operator fixture B');
INSERT INTO circles (id, slug, name, created_by)
VALUES (:'alice', 'operator-fixture', 'Operator fixture', :'alice');
INSERT INTO circle_memberships (circle_id, user_id, role) VALUES (:'alice', :'alice', 'owner');
SQL

for account in "$alice" "$bob" "$absent"; do
  rows=$("${psql[@]}" -v account_id="$account" -f tools/account-privacy-preview.sql)
  profile=0
  membership=0
  if [[ "$account" != "$absent" ]]; then profile=1; fi
  if [[ "$account" == "$alice" ]]; then membership=1; fi
  awk -F'|' -v profile="$profile" -v membership="$membership" '
    NF != 2 || $1 !~ /^[a-z_]+$/ || $2 !~ /^[0-9]+$/ || seen[$1]++ { exit 1 }
    $2 != ($1 == "profile" ? profile : $1 == "circle_memberships" ? membership : 0) { exit 1 }
    END { if (NR != 17 || !("profile" in seen) || !("circle_memberships" in seen)) exit 1 }
  ' <<<"$rows"
done

# Keep one synthetic client connected while the separate observer executes the report.
# psql's shell escape waits for the child, so no sleep or connection timing race is needed.
capacity=$("${psql[@]}" <<'SQL'
\! psql -X -qAt -v ON_ERROR_STOP=1 -f tools/capacity-postgres-budget.sql || echo capacity-query-failed
SQL
)
awk -F'|' '
  NR == 1 {
    if (NF != 7 || $2 !~ /^[0-9]+$/ || $2 <= 0 || $5 < 1 || $6 > $5) exit 1
    clients = $5
    slots = $2 - $3 - $4 - clients
    if ($7 != (slots < 0 ? 0 : slots)) exit 1
    next
  }
  NF != 2 { exit 1 }
  $1 ~ /^(identity|monitoring|other|sproyt|sproyt-heart)$/ {
    if (role_summary || $2 !~ /^[0-9]+$/ || seen[$1]++) exit 1
    grouped += $2
    next
  }
  $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ && $2 <= $1 && !role_summary { role_summary = 1; next }
  { exit 1 }
  END { if (grouped != clients || !seen["other"] || !role_summary) exit 1 }
' <<<"$capacity"

# Replay aggregate load in temporary rows on the isolated CI fixture. PostgreSQL
# supplies the actual reserved-slot settings. The published admission query is unmodified.
for case in '40 10 1' '41 10 0' '37 13 1' '38 13 0' '48 2 1' '49 2 0'; do
  read -r other extra expect <<<"$case"
  actual=$("${psql[@]}" -v other="$other" -v extra="$extra" <<'SQL'
SET search_path = pg_temp, pg_catalog;
CREATE TEMP TABLE pg_stat_activity (pid int, datname text, backend_type text, datid oid, usesysid oid);
INSERT INTO pg_stat_activity
SELECT 0, name, NULL, 1, 1
FROM (VALUES ('sproyt',3),('heart_sproyt_canary',1),('authentik',4),('grafana',10),('other',:other)) s(name,n)
CROSS JOIN LATERAL generate_series(1,n);
INSERT INTO pg_stat_activity VALUES (pg_backend_pid(),'observer','client backend',1,1), (0,'ignored',NULL,NULL,NULL);
\i helm/sproyt/files/capacity-admission.sql
SQL
  )
  [[ ${actual##*|} == "$expect" ]]
done
