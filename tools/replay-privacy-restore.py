#!/usr/bin/env python3
"""Reapply checked privacy obligations to an isolated PostgreSQL restore."""
import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import subprocess
import uuid

TOOLS = Path(__file__).resolve().parent


def timestamp(value):
    try:
        result = datetime.fromisoformat(value)
    except (TypeError, ValueError) as error:
        raise ValueError("invalid journal timestamp") from error
    if result.tzinfo is None:
        raise ValueError("timestamps must include a time zone")
    return result


def manifest(path, lineage, backup_start, frozen_at, expected_count, expected_last_id):
    data = json.loads(path.read_text(encoding="utf-8"))
    captured = timestamp(data["captured_at"])
    covered_from = timestamp(data["covered_from"])
    if (
        data["version"] != 1
        or uuid.UUID(data["lineage"]) != lineage
        or covered_from > backup_start
        or not backup_start <= frozen_at <= captured <= datetime.now(timezone.utc)
    ):
        raise ValueError("journal lineage or time coverage did not match the restore")
    rows = data["obligations"]
    if not isinstance(rows, list):
        raise ValueError("journal obligations must be an array")
    last_id = 0
    for row in rows:
        row_id = row["id"]
        recorded_at = timestamp(row["recorded_at"])
        if type(row_id) is not int or row_id <= last_id or not covered_from <= recorded_at <= captured:
            raise ValueError("journal order or recorded timestamp is invalid")
        last_id = row_id
        row["account_id"] = str(uuid.UUID(row["account_id"]))
        family = row["native_family"]
        if row["action"] == "erase_account" and family is None:
            continue
        if row["action"] == "revoke_native_family" and isinstance(family, str) and re.fullmatch(r"[A-Za-z0-9_-]{1,128}", family):
            continue
        raise ValueError("unsupported or malformed obligation")
    if len(rows) != expected_count or last_id != expected_last_id:
        raise ValueError("journal row count or final id differs from the independent check")
    return rows



def verify_restrictions(authoritative, production, canary):
    def accounts(path):
        text = path.read_text(encoding="utf-8").strip()
        return {uuid.UUID(entry.strip()) for entry in text.split(",")} if text else set()

    try:
        expected = accounts(authoritative)
        if accounts(production) != expected or accounts(canary) != expected:
            raise ValueError("suspension lists differ from the authoritative set")
    except (OSError, ValueError) as error:
        raise ValueError("readable, valid and matching suspension lists are required") from error


def replay(args, rows):
    def apply(name, sql, **variables):
        command = [
            "psql", "-X", "-qAt", "-w", "-v", "ON_ERROR_STOP=1",
            "-v", f"app_schema={args.restore_schema}", "-d", args.restore_target,
        ]
        for key, value in variables.items():
            command.extend(["-v", f"{key}={value}"])
        script = 'set search_path to pg_catalog, :"app_schema", pg_temp;\n' + sql
        result = subprocess.run(
            command + ["-f", "-"], input=script, text=True, encoding="utf-8", capture_output=True
        )
        if result.returncode:
            raise ValueError(
                f"{name} failed. Inspect restricted database logs or repeat this step "
                "in an isolated operator session."
            )
        return result.stdout.strip()

    # Names are an extra guard. The restore network and database connection
    # privileges must already exclude all applications and workers.
    check = (
        "select current_database() || ':' || count(*) from pg_stat_activity "
        "where datname=current_database() and pid<>pg_backend_pid();"
    )
    if apply("isolation check", check) != f"{args.restore_db}:0":
        raise ValueError("database name or exclusive operator access did not match")
    if apply("schema check", "select to_regclass(format('%I.users', :'app_schema')) is not null;") != "t":
        raise ValueError("restored application schema was not found")
    erase = None
    if any(row["action"] == "erase_account" for row in rows):
        erase = (TOOLS / "account-privacy-erase.sql").read_text(encoding="utf-8")
    revoke = (TOOLS / "privacy-restore-revoke-native.sql").read_text(encoding="utf-8")
    for row in rows:
        account_id = row["account_id"]
        if row["action"] == "erase_account":
            apply(f"obligation {row['id']}", erase, account_id=account_id, confirmed_account_id=account_id)
        else:
            apply(f"obligation {row['id']}", revoke, account_id=account_id, native_family=row["native_family"])
    apply("memory and position reset", (TOOLS / "reset-agent-memory-after-restore.sql").read_text(encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--journal-json", type=Path, required=True)
    parser.add_argument("--lineage", type=uuid.UUID, required=True)
    parser.add_argument("--backup-start", type=timestamp, required=True)
    parser.add_argument("--frozen-at", type=timestamp, required=True)
    parser.add_argument("--expected-count", type=int, required=True)
    parser.add_argument("--expected-last-id", type=int, required=True)
    parser.add_argument("--required-blocked", type=Path, required=True, help="authoritative suspension set")
    parser.add_argument("--prod-blocked", type=Path, required=True, help="production suspension setting")
    parser.add_argument("--canary-blocked", type=Path, required=True, help="canary suspension setting")
    parser.add_argument("--restore-target", required=True, help="passwordless libpq service or database name")
    parser.add_argument("--restore-schema", choices=("sproyt", "public"), required=True)
    parser.add_argument("--restore-db", required=True, help="expected actual database name, beginning sproyt_restore_")
    args = parser.parse_args()
    if not re.fullmatch(r"(?:service=)?[A-Za-z0-9_-]+", args.restore_target):
        parser.error("--restore-target requires a libpq service or database name")
    if not re.fullmatch(r"sproyt_restore_[a-z0-9_]+", args.restore_db):
        parser.error("--restore-db must begin with sproyt_restore_")
    try:
        verify_restrictions(args.required_blocked, args.prod_blocked, args.canary_blocked)
        rows = manifest(
            args.journal_json, args.lineage, args.backup_start, args.frozen_at,
            args.expected_count, args.expected_last_id,
        )
        replay(args, rows)
    except (TypeError, KeyError, OSError, ValueError, AttributeError) as error:
        parser.exit(1, f"Restore remains isolated. {type(error).__name__}: {error}\n")
    print(f"Replayed {args.expected_count} obligations through id {args.expected_last_id}. Keep restore isolated until sign-off.")


if __name__ == "__main__":
    main()
