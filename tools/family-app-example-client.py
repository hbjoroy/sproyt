#!/usr/bin/env python3
"""Proposed Open Drinks polling consumer. No dependencies beyond Python 3.10+.

Run --check-fixtures offline. Real calls require --base-url, --binding-id and
SPROYT_APP_CREDENTIAL in the environment. --state-file is a server-private
0600 local cache; an actual application should use its own database transaction.
This example does not implement Open Drinks session approval or publish media.
"""
import argparse
import json
import os
from pathlib import Path
import random
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class ProtocolError(Exception):
    pass


def empty_state():
    return {"members": {}, "revisions": {}, "cursor": None}


def apply_change(state, event):
    subject = event["subject_id"]
    revision = event["revision"]
    kind = event["kind"]
    if kind == "response.revoked" and set(event) - {
        "sequence", "event_id", "kind", "subject_id", "revision"
    }:
        raise ProtocolError("revocation included a payload")
    if revision <= state["revisions"].get(subject, 0):
        return
    if kind == "response.revoked":
        state["members"].pop(subject, None)
    elif kind == "response.upserted":
        row = event["member"]
        if row["subject_id"] != subject or row["revision"] != revision:
            raise ProtocolError("event response mismatch")
        state["members"][subject] = row
    elif kind == "attributes.updated":
        # An app write can never create participation or reactivate sharing.
        if subject not in state["members"]:
            return
        row = event["member"]
        if row["subject_id"] != subject or row["revision"] != revision:
            raise ProtocolError("event response mismatch")
        current = dict(state["members"][subject])
        current["app_attributes"] = row["app_attributes"]
        current["revision"] = revision
        current["updated_at"] = row["updated_at"]
        state["members"][subject] = current
    else:
        raise ProtocolError("unknown change kind")
    state["revisions"][subject] = revision


def atomic_save(path, state):
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=path.name, dir=path.parent)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
            json.dump(state, handle, ensure_ascii=False)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


class Client:
    def __init__(self, origin, credential):
        parsed = urllib.parse.urlsplit(origin)
        if (parsed.scheme != "https" or not parsed.hostname or parsed.username
                or parsed.password or parsed.query or parsed.fragment
                or parsed.path not in ("", "/")):
            raise ProtocolError("base URL must be an operator-configured HTTPS origin")
        if not credential or "\r" in credential or "\n" in credential:
            raise ProtocolError("missing or invalid SPROYT_APP_CREDENTIAL")
        self.origin = origin.rstrip("/")
        self.credential = credential
        self.opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({}), NoRedirect())

    def get(self, path, params=None):
        query = urllib.parse.urlencode(params or {})
        request = urllib.request.Request(
            self.origin + path + ("?" + query if query else ""),
            headers={"Authorization": "Bearer " + self.credential,
                     "Accept": "application/json"})
        for attempt in range(5):
            try:
                with self.opener.open(request, timeout=15) as response:
                    data = response.read(2 * 1024 * 1024 + 1)
                    if len(data) > 2 * 1024 * 1024:
                        raise ProtocolError("oversized JSON response")
                    return json.loads(data)
            except urllib.error.HTTPError as error:
                if error.code not in (429, 503) or attempt == 4:
                    raise
                try:
                    delay = min(60, max(0, int(error.headers["Retry-After"])))
                except (TypeError, ValueError):
                    delay = min(30, 2 ** attempt) + random.random()
                time.sleep(delay)
        raise ProtocolError("retry exhausted")


def snapshot(client, path, state_file):
    # Discard the old cache before rebuilding after expiry; do not continue using
    # answers whose revocations may have expired from the change stream.
    save_state(client, path, state_file, empty_state())
    candidate = empty_state()
    page = client.get(path + "/responses")
    if len(page["items"]) > 1000:
        raise ProtocolError("oversized respondent snapshot")
    for row in page["items"]:
        candidate["members"][row["subject_id"]] = row
        candidate["revisions"][row["subject_id"]] = row["revision"]
    candidate["cursor"] = page["changes_cursor"]
    if not isinstance(candidate["cursor"], int) or candidate["cursor"] < 0:
        raise ProtocolError("invalid snapshot cursor")
    return changes(client, path, candidate, state_file)


def save_state(client, path, state_file, state):
    state["binding_id"] = urllib.parse.unquote(path.rsplit("/", 1)[-1])
    state["origin"] = client.origin
    atomic_save(state_file, state)


def changes(client, path, state, state_file):
    while True:
        previous_cursor = state["cursor"]
        page = client.get(path + "/changes", {"cursor": previous_cursor})
        sequence = 0
        for event in page["items"]:
            if event["sequence"] < sequence:
                raise ProtocolError("unordered change page")
            sequence = event["sequence"]
            apply_change(state, event)
        state["cursor"] = page["next_cursor"]
        if page["has_more"] and state["cursor"] == previous_cursor:
            raise ProtocolError("change cursor did not advance")
        # Effect and cursor are persisted together; retry repeats safe revisions.
        save_state(client, path, state_file, state)
        if not page["has_more"]:
            return state


def check_fixtures():
    root = Path(__file__).resolve().parents[1]
    fixture = json.loads((root / "docs/family-app-integration-fixtures.json").read_text(encoding="utf-8"))
    contract = json.loads((root / "docs/family-app-integration-openapi.json").read_text(encoding="utf-8"))
    assert contract["openapi"] == "3.1.0"

    def check_references(value):
        if isinstance(value, dict):
            if "$ref" in value:
                target = value["$ref"].split("/")
                assert target[:3] == ["#", "components", "schemas"]
                assert target[3] in contract["components"]["schemas"]
            for child in value.values():
                check_references(child)
        elif isinstance(value, list):
            for child in value:
                check_references(child)

    check_references(contract)
    state = empty_state()
    for row in fixture["snapshot"]["items"]:
        state["members"][row["subject_id"]] = row
        state["revisions"][row["subject_id"]] = row["revision"]
    assert len(state["members"]) == 2
    assert {row["response"]["participation"] for row in state["members"].values()} == {"yes", "no"}
    for event in fixture["changes"]["items"]:
        apply_change(state, event)
    state["cursor"] = fixture["changes"]["next_cursor"]
    assert state == fixture["expected_after_changes"]
    apply_change(state, fixture["delayed_upsert_after_revocation"])
    assert state == fixture["expected_after_changes"]
    private_keys = {"email", "verified_email", "sproyt_user_id", "oidc_subject"}
    assert all(not private_keys.intersection(row) for row in fixture["snapshot"]["items"])

    class FixtureClient:
        origin = "https://sprout.example.invalid"

        def get(self, path, params=None):
            if path.endswith("/responses"):
                return fixture["snapshot"]
            assert path.endswith("/changes")
            assert params == {"cursor": 0}
            return fixture["changes"]

    with tempfile.TemporaryDirectory() as directory:
        state_file = Path(directory) / "cache.json"
        committed = snapshot(FixtureClient(), "/api/v1/app-integrations/bindings/binding-a", state_file)
        assert committed["cursor"] == 5 and committed["members"] == {}
        assert committed["binding_id"] == "binding-a"
        assert json.loads(state_file.read_text(encoding="utf-8")) == committed
    print("Consumer fixtures passed: yes/no, changes, revocation, duplicate and stale replay.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-fixtures", action="store_true")
    parser.add_argument("--base-url")
    parser.add_argument("--binding-id")
    parser.add_argument("--state-file", type=Path)
    args = parser.parse_args()
    if args.check_fixtures:
        check_fixtures()
        return
    if not args.base_url or not args.binding_id or not args.state_file:
        parser.error("real polling requires --base-url, --binding-id and --state-file")
    client = Client(args.base_url, os.environ.get("SPROYT_APP_CREDENTIAL", ""))
    path = "/api/v1/app-integrations/bindings/" + urllib.parse.quote(args.binding_id, safe="")
    state = json.loads(args.state_file.read_text(encoding="utf-8")) if args.state_file.exists() else empty_state()
    if args.state_file.exists() and (state.get("binding_id") != args.binding_id or state.get("origin") != client.origin):
        raise ProtocolError("state file belongs to another binding/origin")
    try:
        try:
            state = changes(client, path, state, args.state_file) if state["cursor"] is not None else snapshot(client, path, args.state_file)
        except urllib.error.HTTPError as error:
            if error.code != 410:
                raise
            state = snapshot(client, path, args.state_file)
    except urllib.error.HTTPError as error:
        if error.code in (401, 403, 404):
            save_state(client, path, args.state_file, empty_state())
        raise ProtocolError("sync stopped with HTTP " + str(error.code)) from None
    state["binding_id"] = args.binding_id
    state["origin"] = client.origin
    atomic_save(args.state_file, state)
    print("Committed shared respondents:", len(state["members"]))


if __name__ == "__main__":
    try:
        main()
    except (ProtocolError, urllib.error.URLError, OSError, ValueError, KeyError) as error:
        # Do not log payloads, credentials, emails, names, codes or cursors.
        raise SystemExit("Integration sync failed: " + type(error).__name__) from None
