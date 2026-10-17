#!/usr/bin/env python3
"""Check production binding and parity with the reviewed invitation flow."""
import importlib.util
from pathlib import Path
from types import ModuleType, SimpleNamespace
from unittest.mock import patch
import unittest

ROOT = Path(__file__).resolve().parents[1]
CANARY_ID = "5eb6dcb6-29a8-5b7d-b424-584c59b7409c"
PRODUCTION_ID = "645a51d1-1d8a-5369-867d-b2a93ac7504e"
PRODUCTION = ROOT / "deploy/authentik/sproyt-invitation-production.yaml"
spec = importlib.util.spec_from_file_location("invitation_contract", ROOT / "tools/test-invitation-canary.py")
contract = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contract)
contract.BLUEPRINT = PRODUCTION
contract.FLOW_ID = PRODUCTION_ID


class ProductionInvitationContract(unittest.TestCase):
    def test_only_environment_bindings_differ_from_reviewed_canary(self):
        expected = (ROOT / "deploy/authentik/sproyt-invitation-canary.yaml").read_text(encoding="utf-8")
        for before, after in [
            (CANARY_ID, PRODUCTION_ID),
            ("https://sproyt-canary.bjoroy.me", "https://sproyt.bjoroy.me"),
            ("sproyt-invitation-canary", "sproyt-invitation-production"),
            ("sproyt-canary-", "sproyt-production-"),
            ("sproyt_canary_", "sproyt_production_"),
            ("sproyt:canary:", "sproyt:production:"),
            ("canary-", "production-"),
            ("Sprøyt Canary", "Sprøyt production"),
            ("Canary", "production"),
        ]:
            expected = expected.replace(before, after)
        self.assertEqual(PRODUCTION.read_text(encoding="utf-8"), expected)

    def capture(self, request, existing=False):
        models = ModuleType("authentik.core.models")
        matches = [SimpleNamespace(attributes={"email_verified": True,
                   "email_verified_address": contract.EMAIL})] if existing else []
        models.User = SimpleNamespace(objects=SimpleNamespace(filter=lambda **kwargs: matches))
        with patch.dict("sys.modules", {"authentik.core.models": models}):
            return contract.expression("production-context")(request)

    def test_new_and_existing_accounts_continue_only_to_production(self):
        for existing in [False, True]:
            with self.subTest(existing=existing):
                request = contract.context()
                self.assertFalse(self.capture(request, existing))
                context = request.context["flow_plan"].context
                self.assertIs(context["sproyt_production_new_user"], not existing)
                self.assertEqual(context["redirect"],
                                 "https://sproyt.bjoroy.me/auth/login?enrollment=" + contract.TOKEN)
                self.assertNotIn("pending_user", context)
                self.assertEqual(request.user.email, "unrelated@example.test")

    def test_canary_invitation_cannot_enter_production_flow(self):
        request = contract.context()
        request.context["invitation"].flow_id = CANARY_ID
        self.assertTrue(self.capture(request))
        self.assertNotIn("sproyt_production_new_user", request.context["flow_plan"].context)


if __name__ == "__main__":
    unittest.main()
