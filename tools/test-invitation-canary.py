#!/usr/bin/env python3
"""Execute Canary invitation policies with controlled identity-provider context.

These tests do not replace a running Authentik importer or browser acceptance.
"""
from pathlib import Path
from types import ModuleType, SimpleNamespace
from unittest.mock import patch
import copy
from contextlib import nullcontext
import textwrap
import unittest

BLUEPRINT = Path(__file__).resolve().parents[1] / "deploy/authentik/sproyt-invitation-canary.yaml"
FLOW_ID = "5eb6dcb6-29a8-5b7d-b424-584c59b7409c"
TOKEN = "abcdefghijklmnopqrstuvwxyz0123456789_ABCDEF"
EMAIL = "Invitee@Example.test"


def expression(policy_id):
    block = BLUEPRINT.read_text(encoding="utf-8").split(f"    id: {policy_id}\n", 1)[1]
    block = block.split("      expression: |\n", 1)[1]
    lines = []
    for line in block.splitlines():
        if line and not line.startswith("        "):
            break
        lines.append(line[8:] if line else "")
    namespace = {}
    exec("def policy(request):\n" + textwrap.indent("\n".join(lines), "    "), namespace)
    return namespace["policy"]


def context():
    prompt = {"email": EMAIL, "username": "new-person", "name": "New Person",
              "password": "test-only", "password_repeat": "test-only",
              "sproyt_enrollment_token": "attacker-token", "groups": ["administrators"],
              "attributes": {"is_superuser": True}}
    plan = SimpleNamespace(context={"prompt_data": copy.deepcopy(prompt),
                                    "redirect": "https://attacker.invalid/"})
    return SimpleNamespace(context={
        "invitation": SimpleNamespace(flow_id=FLOW_ID, fixed_data={
            "email": EMAIL, "sproyt_enrollment_token": TOKEN}),
        "invitation_in_effect": True, "prompt_data": prompt, "flow_plan": plan,
    }, user=SimpleNamespace(email="unrelated@example.test", is_authenticated=True))


class CanaryInvitationContract(unittest.TestCase):
    def setUp(self):
        self.emails = set()
        self.lookups = []
        self.users = {}

        def users_filter(**kwargs):
            self.lookups.append(kwargs)
            self.assertEqual(set(kwargs), {"email__iexact"})
            address = kwargs["email__iexact"].casefold()
            matches = [user for user in self.users.values() if user.email.casefold() == address]
            if address in self.emails and not matches:
                matches = [SimpleNamespace(pk=1, email=address, attributes={
                    "email_verified": True, "email_verified_address": address})]

            class Matches(list):
                def exists(self):
                    return bool(self)
            return Matches(matches)

        models = ModuleType("authentik.core.models")
        manager = SimpleNamespace(filter=users_filter)
        manager.get = lambda pk: self.users[pk]
        manager.select_for_update = lambda: SimpleNamespace(get=lambda pk: self.users[pk])
        models.User = SimpleNamespace(objects=manager)
        db = ModuleType("django.db")
        db.transaction = SimpleNamespace(atomic=nullcontext)
        cache_module = ModuleType("django.core.cache")
        self.counts = {}

        def cache_add(key, value, timeout):
            self.assertEqual(timeout, 300)
            if key in self.counts:
                return False
            self.counts[key] = value
            return True

        def cache_incr(key):
            self.counts[key] += 1
            return self.counts[key]

        cache_module.cache = SimpleNamespace(add=cache_add, incr=cache_incr)
        self.module_patch = patch.dict("sys.modules", {
            "authentik.core.models": models, "django.db": db, "django.core.cache": cache_module})
        self.module_patch.start()
        self.addCleanup(self.module_patch.stop)
        self.capture = expression("canary-context")
        self.new_only = expression("canary-new-only")
        self.before_create = expression("canary-before-create")
        self.proof = expression("canary-email-proof")
        self.rate = expression("canary-email-rate-limit")
        self.required = expression("canary-email-required")
        self.password_guard = expression("canary-password-write-guard")

    def unverified_request(self):
        self.saves = []
        user = SimpleNamespace(pk=42, email=EMAIL, attributes={"keep": "value"},
                               save=lambda **kwargs: self.saves.append(kwargs))
        self.users[user.pk] = user
        request = context()
        self.assertFalse(self.capture(request))
        return request

    def add_email_proof(self, request):
        original = copy.deepcopy(request.context["flow_plan"])
        original.bindings = [SimpleNamespace(stage=SimpleNamespace(name="consent")),
                             SimpleNamespace(stage=SimpleNamespace(name="sproyt-canary-existing-email"))]
        token = SimpleNamespace(flow_id=FLOW_ID, plan=original, user=self.users[42])
        request.context["is_restored"] = token
        return token

    def test_existing_unverified_account_requires_a_fresh_mailbox_challenge(self):
        request = self.unverified_request()
        self.assertFalse(self.new_only(request))
        self.assertTrue(self.required(request))
        self.assertTrue(self.proof(request))
        self.assertEqual(self.saves, [])
        self.add_email_proof(request)
        self.assertFalse(self.proof(request))
        self.assertEqual(self.saves, [{"update_fields": ["attributes"]}])
        self.assertEqual(self.users[42].attributes, {"keep": "value", "email_verified": True,
                                                   "email_verified_address": EMAIL})
        self.assertFalse(self.new_only(request))

    def test_mailbox_proof_rejects_wrong_user_email_flow_stage_or_continuation(self):
        for tamper in ["user", "email", "flow", "stage", "continuation", "changed_address"]:
            request = self.unverified_request()
            token = self.add_email_proof(request)
            if tamper == "user":
                token.user = SimpleNamespace(pk=99)
            elif tamper == "email":
                token.plan.context["email"] = "other@example.test"
            elif tamper == "flow":
                token.flow_id = "other-flow"
            elif tamper == "stage":
                token.plan.bindings[1].stage.name = "another-email-stage"
            elif tamper == "continuation":
                request.context["flow_plan"].context["redirect"] += "other"
            else:
                self.users[42].email = "changed@example.test"
            self.assertTrue(self.proof(request), tamper)
            self.assertEqual(self.saves, [], tamper)

    def test_email_rate_limit_is_per_recipient_across_fresh_invitations(self):
        for attempt in range(1, 7):
            request = self.unverified_request()
            self.assertEqual(self.rate(request), attempt > 5)
        request = self.unverified_request()
        self.add_email_proof(request)
        self.assertFalse(self.rate(request))

    def test_ambiguous_existing_email_and_missing_control_flags_are_denied(self):
        self.users = {1: SimpleNamespace(email=EMAIL), 2: SimpleNamespace(email=EMAIL.lower())}
        self.assertTrue(self.capture(context()))
        self.assertTrue(self.proof(context()))
        self.assertTrue(self.rate(context()))

    def test_existing_identity_is_never_selected_written_or_logged_in(self):
        self.emails.add(EMAIL.casefold())
        request = context()
        before_user = copy.deepcopy(vars(request.user))
        self.assertFalse(self.capture(request))
        self.assertFalse(self.new_only(request))
        self.assertFalse(self.before_create(request))
        self.assertNotIn("pending_user", request.context)
        self.assertNotIn("pending_user", request.context["flow_plan"].context)
        self.assertEqual(vars(request.user), before_user)
        self.assertEqual(request.context["flow_plan"].context["redirect"],
                         "https://sproyt-canary.bjoroy.me/auth/login?enrollment=" + TOKEN)

    def test_new_account_is_verified_only_at_invited_address_and_prompt_is_allowlisted(self):
        request = context()
        self.assertFalse(self.capture(request))
        self.assertTrue(self.new_only(request))
        self.assertFalse(self.before_create(request))
        prompt = request.context["flow_plan"].context["prompt_data"]
        self.assertEqual(set(prompt), {"username", "name", "password", "email", "attributes"})
        self.assertIsNone(prompt["password"])
        self.assertEqual(prompt["email"], EMAIL)
        self.assertEqual(prompt["attributes"], {"email_verified": False})
        self.assertEqual(request.user.email, "unrelated@example.test")

    def test_token_and_redirect_never_come_from_prompt(self):
        request = context()
        request.context["prompt_data"]["next"] = "https://attacker.invalid/"
        self.assertFalse(self.capture(request))
        for prompt in [request.context["prompt_data"], request.context["flow_plan"].context["prompt_data"]]:
            self.assertNotIn("sproyt_enrollment_token", prompt)
        self.assertTrue(request.context["flow_plan"].context["redirect"].endswith(TOKEN))

    def test_missing_invalid_or_foreign_invitation_fails_closed(self):
        for token in [None, "", "a" * 42, "a" * 44, "a" * 42 + "&", "a" * 42 + "\n", 1, "é" * 43]:
            request = context()
            request.context["invitation"].fixed_data["sproyt_enrollment_token"] = token
            self.assertTrue(self.capture(request))
            self.assertFalse(self.new_only(request))
        for field, value in [("invitation", None), ("invitation_in_effect", False), ("flow_plan", None)]:
            request = context()
            request.context[field] = value
            self.assertTrue(self.capture(request))
        request = context()
        request.context["invitation"].flow_id = "dcde5ce9-ca43-4003-8d0c-762e8554650c"
        self.assertTrue(self.capture(request))
        request = context()
        request.context["invitation"].fixed_data["email"] = ""
        self.assertTrue(self.capture(request))
        self.assertEqual(self.lookups, [])

    def test_account_created_concurrently_and_tampered_email_are_denied(self):
        request = context()
        self.assertFalse(self.capture(request))
        self.emails.add(EMAIL.casefold())
        self.assertTrue(self.before_create(request))
        self.emails.clear()
        request.context["prompt_data"]["email"] = "other@example.test"
        self.assertTrue(self.before_create(request))

    def test_deleted_invitation_object_does_not_need_another_database_lookup(self):
        request = context()
        self.assertFalse(self.capture(request))
        request.context.pop("invitation")
        self.assertTrue(self.new_only(request))
        self.assertFalse(self.before_create(request))
        self.assertEqual(request.context["flow_plan"].context["redirect"],
                         "https://sproyt-canary.bjoroy.me/auth/login?enrollment=" + TOKEN)

    def test_new_account_stays_inactive_unverified_until_independent_mailbox_proof(self):
        request = context()
        self.assertFalse(self.capture(request))
        self.assertFalse(self.before_create(request))
        self.assertTrue(self.required(request))
        self.saves = []
        # Model the dedicated always-create UserWrite stage, prior to EmailStage.
        user = SimpleNamespace(pk=42, email=EMAIL, attributes={"email_verified": False},
                               is_active=False, save=lambda **kwargs: self.saves.append(kwargs))
        self.users[42] = user
        request.context["flow_plan"].context["pending_user"] = user
        self.assertTrue(self.proof(request))
        self.assertFalse(user.is_active)
        self.add_email_proof(request)
        self.assertFalse(self.proof(request))
        self.assertFalse(user.is_active)
        self.assertTrue(user.attributes["email_verified"])
        self.assertEqual(self.saves, [{"update_fields": ["attributes"]}])
        self.assertTrue(self.password_guard(request))
        request.context["prompt_data"].update(password="recipient-password", password_repeat="recipient-password")
        self.assertFalse(self.password_guard(request))
        self.assertEqual(request.context["flow_plan"].context["prompt_data"],
                         {"password": "recipient-password", "is_active": True})

    def test_password_write_cannot_fall_back_to_browser_user_or_borrow_another_identity(self):
        request = context()
        self.assertFalse(self.capture(request))
        self.assertTrue(self.password_guard(request))
        plan = request.context["flow_plan"]
        plan.context.update(sproyt_canary_mailbox_verified=True, sproyt_canary_verified_user_id=42)
        for user in [None, SimpleNamespace(pk=99, email=EMAIL, attributes={
            "email_verified": True, "email_verified_address": EMAIL})]:
            plan.context["pending_user"] = user
            self.assertTrue(self.password_guard(request))

    def test_blueprint_guards_every_new_account_stage_and_uses_create_only(self):
        source = BLUEPRINT.read_text(encoding="utf-8")
        self.assertIn("user_creation_mode: always_create", source)
        self.assertIn("create_users_as_inactive: true", source)
        self.assertIn("authentication: none", source)
        self.assertIn("continue_flow_without_invitation: false", source)
        self.assertNotIn("https://sproyt.bjoroy.me", source)
        self.assertNotIn("evaluate_on_plan: true", source)
        for binding in ["profile", "password", "write", "password-write", "login"]:
            self.assertIn(f"policy: !KeyOf canary-new-only, target: !KeyOf canary-{binding}-binding", source)
        self.assertIn("create_users_group: !Find [authentik_core.group, [name, sproyt-users]]", source)


if __name__ == "__main__":
    unittest.main()
