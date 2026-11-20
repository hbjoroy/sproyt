#!/usr/bin/env bash
# Run only against the disposable CI PostgreSQL service.
set -euo pipefail
[[ ${CI:-} == true && ${PGHOST:?} == 127.0.0.1 && ${PGUSER:?} == sproyt ]] || exit 1
source_db="sproyt_synthetic_privacy_$$"
restore_db="sproyt_restore_privacy_$$"
journal_db="sproyt_journal_fixture_$$"
scratch=$(mktemp -d)
psql=(psql -X -qAt -v ON_ERROR_STOP=1)
trap 'dropdb --if-exists "$restore_db"; dropdb --if-exists "$source_db"; dropdb --if-exists "$journal_db"; rm -r -- "$scratch"' EXIT

createdb --template=template0 "$source_db"
createdb --template=template0 "$restore_db"
createdb --template=template0 "$journal_db"
"${psql[@]}" -d "$source_db" -c 'create schema sproyt'
(
    export PGOPTIONS='-c search_path=sproyt,public'
    for migration in migrations/postgres/*.sql; do
        "${psql[@]}" -d "$source_db" -1 -f "$migration" >/dev/null
    done
    "${psql[@]}" -d "$source_db" -f tools/seed-privacy-restore.sql
)
lineage=01900000-0000-7000-8000-000000004150
erased=01900000-0000-7000-8000-000000004151
revoked=01900000-0000-7000-8000-000000004152
"${psql[@]}" -d "$journal_db" -f tools/privacy-restore-journal.sql
"${psql[@]}" -d "$journal_db" -v lineage="$lineage" <<'SQL'
insert into privacy_journal.coverage(lineage,covered_from)
values (:'lineage',clock_timestamp());
SQL
backup_start=$("${psql[@]}" -d "$journal_db" -c 'select clock_timestamp()')
pg_client=(docker run --rm --network host -e PGHOST -e PGUSER -e PGPASSWORD)
client_image='postgres:17-alpine@sha256:742f40ea20b9ff2ff31db5458d127452988a2164df9e17441e191f3b72252193'
"${pg_client[@]}" "$client_image" pg_dump -Fc --no-owner --no-acl "$source_db" >"$scratch/source.dump"
"${psql[@]}" -d "$journal_db" -v erased="$erased" -v revoked="$revoked" <<'SQL'
set role privacy_journal_writer;
insert into privacy_journal.obligations(account_id,action,native_family) values
    (:'erased','erase_account',null), (:'revoked','revoke_native_family','synthetic-revoked');
SQL
"${psql[@]}" -d "$source_db" -v app_schema=sproyt -v account_id="$erased" \
    -v confirmed_account_id="$erased" -f tools/account-privacy-erase.sql
"${psql[@]}" -d "$source_db" -c 'set search_path=sproyt,pg_catalog,pg_temp' \
    -v account_id="$revoked" -v native_family=synthetic-revoked -f tools/privacy-restore-revoke-native.sql
frozen_at=$("${psql[@]}" -d "$journal_db" -c 'select clock_timestamp()')
"${psql[@]}" -d "$journal_db" -c 'set role privacy_journal_reader' -f tools/privacy-restore-manifest.sql >"$scratch/journal.json"
"${pg_client[@]}" -i "$client_image" pg_restore --exit-on-error --no-owner --no-acl -d "$restore_db" <"$scratch/source.dump"

printf '%s\n' "$revoked" >"$scratch/required"
cp "$scratch/required" "$scratch/production"
replay=(python3 tools/replay-privacy-restore.py --lineage "$lineage" --frozen-at "$frozen_at"
    --backup-start "$backup_start" --expected-count 2 --expected-last-id 2
    --restore-schema sproyt --restore-db "$restore_db" --restore-target "$restore_db"
    --required-blocked "$scratch/required" --prod-blocked "$scratch/production"
    --canary-blocked "$scratch/canary" --journal-json "$scratch/journal.json")
if "${replay[@]}"; then exit 1; fi
printf '%s\n' "$erased" >"$scratch/canary"
if "${replay[@]}"; then exit 1; fi
cp "$scratch/required" "$scratch/canary"
if "${replay[@]}" --expected-count 1; then exit 1; fi
if "${replay[@]}" --backup-start 2000-01-01T00:00:00Z; then exit 1; fi
[[ $("${psql[@]}" -d "$restore_db" -c "select count(*) from sproyt.native_families where phase='active'") == 3 ]]
"${replay[@]}"
"${replay[@]}"
"${psql[@]}" -d "$restore_db" -c 'set search_path=sproyt,pg_catalog,pg_temp' -f tools/verify-privacy-restore.sql
