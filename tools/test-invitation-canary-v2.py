#!/usr/bin/env python3
"""V2 policy contracts using real Django timestamped signatures.

Model doubles exercise denial cases; the separate Authentik runtime probe must
establish actual executor, administrative-save and database-lock behavior.
"""
import copy
from contextlib import nullcontext
from datetime import timedelta
import importlib.util
from pathlib import Path
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch
from uuid import uuid4

from django.conf import settings
from django.core import signing
from django.utils.timezone import now

if not settings.configured:
    settings.configure(SECRET_KEY="isolated-contract-key-not-a-deployment-secret", USE_TZ=True)

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("legacy", ROOT / "tools/test-invitation-canary.py")
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)
legacy.BLUEPRINT = ROOT / "deploy/authentik/sproyt-invitation-canary-v2.yaml"
FLOW = "a1b4b2b7-80e3-591d-b52f-bed0ae3e5a35"
SALT = "sproyt.canary.invitation-placeholder.v2"
MARKER = "sproyt_canary_v2_pending"
PREFIX = "sproyt_canary_v2_"


class Relation(list):
    def exists(self):
        return bool(self)

    def values_list(self, field, flat):
        return list(self)


class User:
    def __init__(self, pk=42):
        self.pk, self.uuid = pk, uuid4()
        self.email = legacy.EMAIL
        self.last_updated = now()
        self.last_login = None
        self.password = "!unusable"
        self.attributes = {"email_verified": False}
        self.groups, self.roles, self.sources = Relation([123]), Relation(), Relation()
        self.is_active = self.is_superuser = False
        self.type, self.path = "external", "users/sproyt"
        self.password_writes = 0

    def has_usable_password(self):
        return not self.password.startswith("!")

    def set_password(self, value, request):
        self.password = "test-hash:" + value
        self.password_writes += 1
        self.password_change_date = now()

    def save(self, update_fields):
        if "last_updated" in update_fields:
            self.last_updated = now()


def request():
    result = legacy.context()
    result.context["invitation"].flow_id = FLOW
    result.http_request = SimpleNamespace()
    return result


class RecoveryContract(unittest.TestCase):
    def test_deny_fallbacks_are_on_policy_bindings_not_expression_policies(self):
        source = legacy.BLUEPRINT.read_text(encoding="utf-8")
        deny = {"context", "rate", "proof", "before-create", "password-guard", "stamp"}
        seen = set()
        for block in source.split("  - identifiers:"):
            if "model: authentik_policies_expression.expressionpolicy" in block:
                self.assertNotIn("failure_result:", block)
            if "model: authentik_policies.policybinding" in block:
                target = next((name for name in deny if f"target: !KeyOf canary-v2-{name}-binding" in block), None)
                self.assertIn("failure_result: " + ("true" if target else "false"), block)
                if target:
                    seen.add(target)
        self.assertEqual(seen, deny)

    def setUp(self):
        self.users = {}
        models = ModuleType("authentik.core.models")
        manager = SimpleNamespace(
            filter=lambda email__iexact: Relation([
                u for u in self.users.values() if u.email.casefold() == email__iexact.casefold()]),
            get=lambda pk: self.users[pk],
        )
        manager.select_for_update = lambda: manager
        models.User = SimpleNamespace(objects=manager)
        models.Group = SimpleNamespace(objects=SimpleNamespace(get=lambda name: SimpleNamespace(pk=123)))
        self.modules = patch.dict("sys.modules", {"authentik.core.models": models})
        self.modules.start()
        self.addCleanup(self.modules.stop)
        self.atomic = patch("django.db.transaction.atomic", nullcontext)
        self.atomic.start()
        self.addCleanup(self.atomic.stop)
        self.capture = legacy.expression("canary-v2-context")
        self.before = legacy.expression("canary-v2-before-create")
        self.stamp = legacy.expression("canary-v2-stamp-placeholder")
        self.proof = legacy.expression("canary-v2-email-proof")
        self.commit = legacy.expression("canary-v2-password-write-guard")

    def placeholder(self):
        r = request()
        self.assertFalse(self.capture(r))
        self.assertFalse(self.before(r))
        user = User()
        self.users[user.pk] = user
        r.context["flow_plan"].context["pending_user"] = copy.deepcopy(user)
        self.assertFalse(self.stamp(r))
        self.assertIsInstance(user.attributes[MARKER], str)
        return r, user

    def mailbox(self, r, user):
        original = copy.deepcopy(r.context["flow_plan"])
        original.bindings = [SimpleNamespace(stage=SimpleNamespace(name="consent")),
                             SimpleNamespace(stage=SimpleNamespace(name="sproyt-canary-v2-existing-email"))]
        token = SimpleNamespace(flow_id=FLOW, plan=original, user=user, expires=now() + timedelta(minutes=30))
        r.context["is_restored"] = token
        self.assertFalse(self.proof(r))
        r.context["prompt_data"] = {"password": "recipient-choice", "password_repeat": "recipient-choice"}
        return token

    def test_new_and_interrupted_registration_commit_once(self):
        r, user = self.placeholder()
        marker = user.attributes[MARKER]
        resumed = request()
        self.assertFalse(self.capture(resumed))
        self.assertFalse(resumed.context["flow_plan"].context[PREFIX + "create_user"])
        self.assertTrue(resumed.context["flow_plan"].context[PREFIX + "new_user"])
        self.assertTrue(resumed.context["flow_plan"].context[PREFIX + "verify_email"])
        self.assertTrue(self.commit(resumed))
        self.mailbox(resumed, user)
        stale = copy.deepcopy(resumed)
        self.assertFalse(self.commit(resumed))
        self.assertTrue(user.is_active)
        self.assertNotIn(MARKER, user.attributes)
        self.assertEqual(user.password_writes, 1)
        self.assertTrue(self.commit(stale))
        self.assertEqual(user.password_writes, 1)
        active = request()
        self.assertFalse(self.capture(active))
        self.assertFalse(active.context["flow_plan"].context[PREFIX + "new_user"])
        self.assertFalse(self.commit(active))  # skip, never reset existing credentials
        self.assertEqual(user.password_writes, 1)
        self.assertIsInstance(marker, str)

    def test_new_flow_completes_without_interruption(self):
        r, user = self.placeholder()
        self.mailbox(r, user)
        self.assertFalse(self.commit(r))
        self.assertTrue(user.is_active)
        self.assertEqual(r.context["prompt_data"], {})

    def test_verification_before_interruption_still_requires_fresh_mailbox_proof(self):
        r, user = self.placeholder()
        self.mailbox(r, user)
        resumed = request()
        self.assertFalse(self.capture(resumed))
        self.assertTrue(resumed.context["flow_plan"].context[PREFIX + "verify_email"])
        self.assertTrue(self.commit(resumed))
        self.mailbox(resumed, user)
        self.assertFalse(self.commit(resumed))

    def test_historical_unmarked_and_ambiguous_accounts_are_not_resumed(self):
        user = User()
        self.users[user.pk] = user
        r = request()
        self.assertFalse(self.capture(r))
        self.assertFalse(r.context["flow_plan"].context[PREFIX + "new_user"])
        self.users[43] = User(43)
        self.assertTrue(self.capture(request()))

    def test_forged_copied_wrong_flow_address_and_expired_provenance(self):
        for tamper in ["unsigned", "signature", "copy", "flow", "email", "expired", "salt"]:
            with self.subTest(tamper=tamper):
                self.users.clear()
                _, user = self.placeholder()
                marker = user.attributes[MARKER]
                if tamper in {"flow", "email"}:
                    origin = signing.loads(marker, salt=SALT)
                    origin[tamper] = "wrong"
                    user.attributes[MARKER] = signing.dumps(origin, salt=SALT)
                elif tamper == "expired":
                    with patch("time.time", return_value=now().timestamp() + 86401):
                        r = request()
                        self.assertFalse(self.capture(r))
                        self.assertFalse(r.context["flow_plan"].context[PREFIX + "new_user"])
                    continue
                elif tamper == "copy":
                    user.uuid = uuid4()
                elif tamper == "salt":
                    user.attributes[MARKER] = signing.dumps(signing.loads(marker, salt=SALT), salt="other")
                else:
                    user.attributes[MARKER] = "unsigned" if tamper == "unsigned" else marker + "tampered"
                r = request()
                self.assertFalse(self.capture(r))
                self.assertFalse(r.context["flow_plan"].context[PREFIX + "new_user"])

    def test_administrative_edits_and_account_changes_revoke_at_completion(self):
        for field in ["updated", "marker", "active", "password", "source", "roles", "group", "path", "type", "login"]:
            with self.subTest(field=field):
                self.users.clear()
                r, user = self.placeholder()
                self.mailbox(r, user)
                if field == "updated":
                    user.last_updated += timedelta(seconds=1)
                elif field == "marker":
                    user.attributes.pop(MARKER)
                elif field == "active":
                    user.is_active = True
                elif field == "password":
                    user.password = "admin-chosen"
                elif field in {"source", "roles", "group"}:
                    getattr(user, {"source": "sources", "group": "groups"}.get(field, field)).append(999)
                elif field == "login":
                    user.last_login = now()
                else:
                    setattr(user, field, "changed")
                self.assertTrue(self.commit(r))
                self.assertEqual(user.password_writes, 0)

    def test_creation_to_stamp_interference_and_repeated_stamp_are_denied(self):
        r = request()
        self.assertFalse(self.capture(r))
        self.assertFalse(self.before(r))
        user = User()
        self.users[user.pk] = user
        r.context["flow_plan"].context["pending_user"] = copy.deepcopy(user)
        user.last_updated += timedelta(seconds=1)
        self.assertTrue(self.stamp(r))
        self.assertNotIn(MARKER, user.attributes)
        self.users.clear()
        r, user = self.placeholder()
        marker = user.attributes[MARKER]
        self.assertTrue(self.stamp(r))
        self.assertEqual(user.attributes[MARKER], marker)

    def test_mailbox_snapshot_tampering_and_expiry_deny_without_activation(self):
        for field in ["expiry", "flow", "user", "stage", "marker", "address", "redirect", "verified_id"]:
            with self.subTest(field=field):
                self.users.clear()
                r, user = self.placeholder()
                token = self.mailbox(r, user)
                if field == "expiry":
                    token.expires = now() - timedelta(seconds=1)
                elif field == "flow":
                    token.flow_id = "wrong"
                elif field == "user":
                    token.user = SimpleNamespace(pk=99)
                elif field == "stage":
                    token.plan.bindings[1].stage.name = "wrong"
                elif field == "verified_id":
                    r.context["flow_plan"].context[PREFIX + "verified_user_id"] = 99
                else:
                    key = {"marker": PREFIX + "marker", "address": "email", "redirect": "redirect"}[field]
                    token.plan.context[key] = "wrong"
                if field != "verified_id":
                    self.assertTrue(self.proof(r))
                self.assertTrue(self.commit(r))
                self.assertFalse(user.is_active)
                self.assertEqual(user.password_writes, 0)


if __name__ == "__main__":
    unittest.main()
