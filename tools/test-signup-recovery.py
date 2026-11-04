#!/usr/bin/env python3
"""Real Django signature contracts; actual executor/locking is a separate gate."""
import copy
from contextlib import nullcontext
from datetime import timedelta
import importlib.util
import io
from pathlib import Path
from types import ModuleType, SimpleNamespace
import unittest
from unittest.mock import patch

from django.core import signing
from django.utils.timezone import now

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("v2_contract", ROOT / "tools/test-invitation-canary-v2.py")
v2 = importlib.util.module_from_spec(spec)
spec.loader.exec_module(v2)
BLUEPRINT = ROOT / "deploy/authentik/z-sproyt-signup-recovery-v2.yaml"
FLOW = "9b763222-baf4-5a03-a3b8-26f0bc1f71c1"
PRODUCTION = "645a51d1-1d8a-5369-867d-b2a93ac7504e"
MARKER = "sproyt_pending_signup_v1"
SALT = "sproyt.pending-signup.v1"
PREFIX = "sproyt_recovery_v2_"


def expression(name):
    original = v2.legacy.BLUEPRINT
    try:
        v2.legacy.BLUEPRINT = BLUEPRINT
        return v2.legacy.expression(name)
    finally:
        v2.legacy.BLUEPRINT = original


def request(user):
    plan = SimpleNamespace(context={"pending_user": copy.deepcopy(user), "redirect": "https://invalid.example/"})
    return SimpleNamespace(context={"flow_plan": plan, "pending_user": copy.deepcopy(user)},
                           http_request=SimpleNamespace())


class RecoveryContracts(unittest.TestCase):
    def setUp(self):
        self.users = {}
        models = ModuleType("authentik.core.models")
        manager = SimpleNamespace(get=lambda pk: self.users[pk], filter=lambda email__iexact: v2.Relation([
            u for u in self.users.values() if u.email.casefold() == email__iexact.casefold()]))
        manager.select_for_update = lambda: manager
        models.User = SimpleNamespace(objects=manager)
        models.Group = SimpleNamespace(objects=SimpleNamespace(get=lambda name: SimpleNamespace(pk=123)))
        devices = ModuleType("authentik.stages.authenticator")
        devices.devices_for_user = lambda user: user.devices
        self.modules = patch.dict("sys.modules", {"authentik.core.models": models,
                                                  "authentik.stages.authenticator": devices})
        self.modules.start()
        self.addCleanup(self.modules.stop)
        self.atomic = patch("django.db.transaction.atomic", nullcontext)
        self.atomic.start()
        self.addCleanup(self.atomic.stop)
        self.capture = expression("recovery-v2-classify")
        self.proof = expression("recovery-v2-proof")
        self.commit = expression("recovery-v2-finalize")
        self.stamp = expression("pending-signup-stamp")

    def user(self, pk=42, active=False):
        user = v2.User(pk)
        user.devices = []
        user.username = "signup-user"
        user.is_active = active
        if active:
            user.password = "active-password-hash"
        self.users[pk] = user
        return user

    def placeholder(self, flow=PRODUCTION):
        user = self.user()
        r = request(user)
        prefix, flag = {PRODUCTION: ("sproyt_production_", "new_user"),
                        "5eb6dcb6-29a8-5b7d-b424-584c59b7409c": ("sproyt_canary_", "new_user"),
                        v2.FLOW: ("sproyt_canary_v2_", "create_user")}[flow]
        r.context.update(invitation_in_effect=True, invitation=SimpleNamespace(
            flow_id=flow, fixed_data={"email": user.email}))
        r.context["flow_plan"].context.update({prefix + flag: True, prefix + "email": user.email})
        self.assertFalse(self.stamp(r))
        return user

    def recover(self, user):
        r = request(user)
        self.assertFalse(self.capture(r))
        return r

    def mailbox(self, r, user):
        original = copy.deepcopy(r.context["flow_plan"])
        original.bindings = [SimpleNamespace(stage=SimpleNamespace(name="consent")),
                             SimpleNamespace(stage=SimpleNamespace(name="sproyt-recovery-v2-email"))]
        token = SimpleNamespace(flow_id=FLOW, plan=original, user=copy.deepcopy(user),
                                expires=now() + timedelta(minutes=30))
        r.context["is_restored"] = token
        self.assertFalse(self.proof(r))
        r.context["prompt_data"] = {"password": "mailbox-recipient-choice", "password_repeat": "mailbox-recipient-choice",
                                    "is_active": False, "groups": ["admin"], "email": "attacker@example.test"}
        return token

    def test_bindings_fail_closed_mfa_and_no_later_userwrite(self):
        source = BLUEPRINT.read_text(encoding="utf-8")
        self.assertNotIn("authentik_stages_user_write", source)
        legacy_source = (ROOT / "deploy/authentik/sproyt-email-recovery.yaml").read_text(encoding="utf-8")
        self.assertNotIn("recovery_flow:", legacy_source)
        self.assertEqual(source.count("recovery_flow:"), 1)
        self.assertIn("recovery_flow: !KeyOf recovery-v2-flow", source)
        for block in source.split("  - identifiers:"):
            if "model: authentik_policies_expression.expressionpolicy" in block:
                self.assertNotIn("failure_result:", block)
            if "model: authentik_policies.policybinding" in block:
                self.assertIn("failure_result: true", block)
        self.assertIn("sproyt-authentication-mfa-validation", source)
        self.assertIn("order: 25}", source)
        self.assertIn("activate_user_on_success: false", source)

    def test_new_pending_signup_and_existing_active_reset_once(self):
        for active in [False, True]:
            with self.subTest(active=active):
                self.users.clear()
                user = self.user(active=True) if active else self.placeholder()
                email = user.email
                r = self.recover(user)
                self.assertEqual(r.context["flow_plan"].context[PREFIX + "branch"], "active" if active else "pending")
                self.assertTrue(self.commit(r))  # attributes/email_verified is never sufficient
                self.mailbox(r, user)
                stale = copy.deepcopy(r)
                self.assertFalse(self.commit(r))
                self.assertTrue(user.is_active)
                self.assertEqual(user.email, email)
                self.assertEqual(user.groups, [123])
                self.assertTrue(user.attributes["email_verified"])
                self.assertNotIn(MARKER, user.attributes)
                self.assertEqual(user.password_writes, 1)
                self.assertEqual(r.context["prompt_data"], {})
                self.assertEqual(r.context["flow_plan"].context["redirect"], "https://sproyt.bjoroy.me/auth/login")
                self.assertTrue(self.commit(stale))
                self.assertEqual(user.password_writes, 1)

    def test_all_three_creation_flows_stamp_and_preserve_mailbox_attribute_saves(self):
        for flow in [PRODUCTION, "5eb6dcb6-29a8-5b7d-b424-584c59b7409c", v2.FLOW]:
            with self.subTest(flow=flow):
                self.users.clear()
                user = self.placeholder(flow)
                user.attributes["email_verified"] = True
                user.attributes["email_verified_address"] = user.email
                user.save(update_fields=["attributes"])
                r = self.recover(user)
                self.assertEqual(r.context["flow_plan"].context[PREFIX + "branch"], "pending")
                self.assertTrue(self.commit(r))

    def test_unmarked_disabled_historical_old_v2_and_ambiguous_users_denied(self):
        for attrs, password in [({}, "!unusable"), ({}, "historical-usable"),
                                ({"sproyt_canary_v2_pending": "old-v2-grant"}, "!unusable"),
                                ({"email_verified": True}, "disabled-admin-account")]:
            user = self.user()
            user.attributes, user.password = attrs, password
            self.assertTrue(self.capture(request(user)))
        user = self.placeholder()
        self.user(43)
        self.assertTrue(self.capture(request(user)))

    def test_bad_copied_unsigned_wrong_origin_and_expired_grants(self):
        for case in ["unsigned", "signature", "copy", "flow", "nonce", "kind", "expired"]:
            with self.subTest(case=case):
                self.users.clear()
                user = self.placeholder()
                if case == "unsigned":
                    user.attributes[MARKER] = {"trusted": True}
                elif case == "signature":
                    user.attributes[MARKER] += "tamper"
                elif case == "copy":
                    user.uuid = v2.uuid4()
                elif case == "expired":
                    with patch("time.time", return_value=now().timestamp() + 2592001):
                        self.assertTrue(self.capture(request(user)))
                    continue
                else:
                    grant = signing.loads(user.attributes[MARKER], salt=SALT)
                    grant[case] = "wrong"
                    user.attributes[MARKER] = signing.dumps(grant, salt=SALT)
                self.assertTrue(self.capture(request(user)))

    def test_stamp_requires_actual_new_creation_and_unchanged_minimal_account(self):
        for case in ["existing", "missing-flag", "wrong-flow", "stale", "password", "role", "group", "active"]:
            with self.subTest(case=case):
                self.users.clear()
                user = self.user()
                r = request(user)
                r.context.update(invitation_in_effect=True, invitation=SimpleNamespace(
                    flow_id=PRODUCTION, fixed_data={"email": user.email}))
                state = r.context["flow_plan"].context
                state.update(sproyt_production_new_user=True, sproyt_production_email=user.email)
                if case == "existing":
                    state["sproyt_production_new_user"] = False
                elif case == "missing-flag":
                    state.pop("sproyt_production_new_user")
                elif case == "wrong-flow":
                    r.context["invitation"].flow_id = FLOW
                elif case == "stale":
                    user.last_updated += timedelta(seconds=1)
                elif case == "password":
                    user.password = "usable-hash"
                elif case == "role":
                    user.roles.append(8)
                elif case == "group":
                    user.groups.append(8)
                else:
                    user.is_active = True
                self.assertEqual(self.stamp(r), case != "existing")
                self.assertNotIn(MARKER, user.attributes)

    def test_mfa_usage_counters_do_not_invalidate_but_device_identity_does(self):
        user = self.user(active=True)
        device = SimpleNamespace(_meta=SimpleNamespace(label_lower="totp.totpdevice"), pk=7, sign_count=10)
        user.devices.append(device)
        r = self.recover(user)
        self.mailbox(r, user)
        device.sign_count += 1
        self.assertFalse(self.commit(r))
        self.assertEqual(user.password_writes, 1)

    def test_every_snapshot_change_after_mailbox_proof_denies_before_password_write(self):
        for field in ["updated", "password", "email", "active", "groups", "roles", "sources", "superuser",
                      "type", "path", "last_login", "mfa", "marker"]:
            with self.subTest(field=field):
                self.users.clear()
                user = self.placeholder()
                r = self.recover(user)
                self.mailbox(r, user)
                if field == "updated":
                    user.last_updated += timedelta(seconds=1)
                elif field == "password":
                    user.password = "administrator-change"
                elif field == "email":
                    user.email = "changed@example.test"
                elif field == "active":
                    user.is_active = True
                elif field in {"groups", "roles", "sources"}:
                    getattr(user, field).append(999)
                elif field == "superuser":
                    user.is_superuser = True
                elif field == "last_login":
                    user.last_login = now()
                elif field == "mfa":
                    user.devices.append(SimpleNamespace(_meta=SimpleNamespace(label_lower="totp.totpdevice"), pk=7))
                elif field == "marker":
                    user.attributes.pop(MARKER)
                else:
                    setattr(user, field, "changed")
                self.assertTrue(self.commit(r))
                self.assertEqual(user.password_writes, 0)

    def test_active_account_cannot_turn_into_activation_branch(self):
        user = self.user(active=True)
        r = self.recover(user)
        self.mailbox(r, user)
        user.is_active = False
        self.assertTrue(self.commit(r))
        self.assertFalse(user.is_active)
        self.assertEqual(user.password_writes, 0)

    def test_token_and_plan_mismatches_denied_at_proof_and_after_prompt(self):
        for case in ["flow", "stage", "user", "expires", "email", "branch", "snapshot", "marker", "redirect"]:
            for before in [True, False]:
                with self.subTest(case=case, before=before):
                    self.users.clear()
                    user = self.placeholder()
                    r = self.recover(user)
                    token = self.mailbox(r, user)
                    if case == "flow":
                        token.flow_id = PRODUCTION
                    elif case == "stage":
                        token.plan.bindings[1].stage.name = "sproyt-recovery-email"
                    elif case == "user":
                        token.user.pk = 999
                    elif case == "expires":
                        token.expires = now() - timedelta(seconds=1)
                    elif case in {"email", "redirect"}:
                        r.context["flow_plan"].context[case] = "https://attacker.invalid/"
                    else:
                        r.context["flow_plan"].context[PREFIX + case] = "wrong"
                    self.assertTrue(self.proof(r) if before else self.commit(r))
                    self.assertEqual(user.password_writes, 0)

    def test_signed_mailbox_claim_cannot_be_forged_or_replaced_by_boolean(self):
        for claim in [True, "unsigned", "", {"verified": True}]:
            self.users.clear()
            user = self.placeholder()
            r = self.recover(user)
            self.mailbox(r, user)
            r.context["flow_plan"].context[PREFIX + "mailbox"] = claim
            # Non-string claims are invalid too; the real policy engine's
            # failure_result=true is a second line of protection for exceptions.
            self.assertTrue(self.commit(r))

    def test_operator_grant_is_shorter_lived_and_allows_only_explicit_snapshot(self):
        user = self.placeholder()
        user.password = "historical-usable-password"
        self.assertTrue(self.capture(request(user)))
        grant = signing.loads(user.attributes[MARKER], salt=SALT)
        grant["kind"] = "operator"
        from hashlib import sha256
        grant["snapshot"]["password"] = sha256(user.password.encode()).hexdigest()
        user.attributes[MARKER] = signing.dumps(grant, salt=SALT)
        r = self.recover(user)
        self.mailbox(r, user)
        with patch("time.time", return_value=now().timestamp() + 172801):
            self.assertTrue(self.capture(request(user)))
        self.assertFalse(self.commit(r))

    def test_initial_send_limit_shared_with_legacy_and_cache_failure_denies(self):
        cache_module = ModuleType("django.core.cache")
        calls = []
        cache_module.cache = SimpleNamespace(add=lambda key, value, timeout: calls.append(key) or False,
                                             incr=lambda key: 6)
        with patch.dict("sys.modules", {"django.core.cache": cache_module}):
            r = self.recover(self.placeholder())
            self.assertTrue(expression("recovery-v2-rate-limit")(r))
            self.assertTrue(calls[0].startswith("sproyt:recovery:first-send:"))
            self.assertFalse(expression("recovery-v2-rate-limit")(r))
            cache_module.cache.add = lambda *a, **kw: (_ for _ in ()).throw(RuntimeError("cache down"))
            self.assertTrue(expression("recovery-v2-rate-limit")(self.recover(self.users[42])))

    def test_operator_dry_run_apply_and_reviewed_snapshot_change(self):
        import sys
        helper = OperatorEvidenceContracts().helper()
        user = self.user()
        user.password = "historical-usable-password"
        manager = sys.modules["authentik.core.models"].User.objects
        manager.get = lambda username: user
        created = now()
        creation = SimpleNamespace(action="user_write", created=created, context={"created": True,
            "email": user.email, "username": user.username, "http_request": {"path": helper.SIGNUP_PATH}})
        model = SimpleNamespace(action="model_created", created=created, context={
            "model": {"pk": user.pk, "app": "authentik_core", "model_name": "user"},
            "http_request": {"path": helper.SIGNUP_PATH}})
        password = SimpleNamespace(action="password_set", created=created, user={"pk": user.pk},
            context={"http_request": {"path": helper.RECOVERY_PATH}})
        events = ModuleType("authentik.events.models")
        evidence = {"create": creation, "model": model, "password": password}
        events.Event = SimpleNamespace(objects=SimpleNamespace(get=lambda pk: evidence[pk],
            filter=lambda *a, **kw: SimpleNamespace(order_by=lambda field: [])))
        args = ["--username", user.username, "--expected-email", user.email, "--expected-uuid", str(user.uuid),
                "--creation-event", "create", "--model-created-event", "model", "--password-set-event", "password",
                "--reason", "Reviewed exact historical interrupted signup"]
        with patch.dict("sys.modules", {"authentik.events.models": events}), patch("sys.stdout", new_callable=io.StringIO):
            report = helper.main(args)
            self.assertFalse(report["applied"])
            self.assertNotIn(MARKER, user.attributes)
            self.assertFalse(user.is_active)
            apply = args + ["--apply", "--expected-snapshot-sha256", report["snapshot_sha256"]]
            user.last_updated += timedelta(seconds=1)
            with self.assertRaises(ValueError):
                helper.main(apply)
            user.last_updated -= timedelta(seconds=1)
            helper.main(apply)
        self.assertFalse(user.is_active)
        self.assertEqual(user.password_writes, 0)
        self.assertEqual(signing.loads(user.attributes[MARKER], salt=SALT)["kind"], "operator")
        manager.get = lambda pk: self.users[pk]
        recovered = self.recover(user)
        self.assertEqual(recovered.context["flow_plan"].context[PREFIX + "snapshot"], helper.snapshot(user))


class OperatorEvidenceContracts(unittest.TestCase):
    def helper(self):
        spec = importlib.util.spec_from_file_location("grant_helper", ROOT / "tools/issue-signup-recovery-grant.py")
        helper = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(helper)
        return helper

    def test_evidence_checks_exact_account_creation_and_reject_admin_edits(self):
        helper = self.helper()
        user = SimpleNamespace(pk=42, username="signup-user", email="Recipient@example.test")
        creation = SimpleNamespace(action="user_write", created=now(), context={"created": True,
            "email": user.email, "username": user.username, "http_request": {"path": helper.SIGNUP_PATH}})
        model = SimpleNamespace(action="model_created", created=creation.created, context={
            "model": {"pk": 42, "app": "authentik_core", "model_name": "user"},
            "http_request": {"path": helper.SIGNUP_PATH}})
        password = SimpleNamespace(action="password_set", created=creation.created, user={"pk": 42},
            context={"http_request": {"path": helper.RECOVERY_PATH}})
        helper.check_evidence(user, creation, model, password, [])
        admin = SimpleNamespace(pk="edit", action="model_updated", context={"http_request": {"path": "/api/v3/core/users/42/"}})
        with self.assertRaises(ValueError):
            helper.check_evidence(user, creation, model, password, [admin])
        model.context["model"]["pk"] = 43
        with self.assertRaises(ValueError):
            helper.check_evidence(user, creation, model, password, [])


if __name__ == "__main__":
    unittest.main()
