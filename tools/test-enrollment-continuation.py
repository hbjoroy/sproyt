#!/usr/bin/env python3
"""Execute the versioned enrollment policy expressions, without Authentik writes.

This checks policy behavior, not Authentik's blueprint schema or a real user's
registration. The running Authentik version must still dry-run the blueprint.
"""
from pathlib import Path
from types import SimpleNamespace
import copy
import textwrap
import unittest


BLUEPRINT = Path(__file__).resolve().parents[1] / "deploy/authentik/sproyt-email-recovery.yaml"
TOKEN = "abcdefghijklmnopqrstuvwxyz0123456789_ABCDEF"
KEY = "sproyt_enrollment_token"


def expression(policy_id):
    # Extract only the existing literal expression block; do not implement a
    # general YAML loader or execute unrelated recovery policies in this test.
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


def context(token=TOKEN, flow="sproyt-invitation-enrollment"):
    invitation = SimpleNamespace(flow=SimpleNamespace(slug=flow), fixed_data={
        "email": "invitee@example.test", KEY: token,
    })
    prompt = {"email": "invitee@example.test", KEY: "attacker-prompt-value", "attributes": {"keep": "yes"}}
    plan = SimpleNamespace(context={"prompt_data": copy.deepcopy(prompt), "redirect": "https://attacker.invalid/"})
    return SimpleNamespace(context={"invitation": invitation, "invitation_in_effect": True,
                                    "prompt_data": prompt, "flow_plan": plan})


class ContinuationContract(unittest.TestCase):
    def setUp(self):
        self.deny = expression("invalid-invitation-context")
        self.mark = expression("mark-invitation-email")

    def test_email_link_needs_only_itoken_and_server_invitation_context(self):
        request = context()
        self.assertFalse(self.deny(request))
        self.assertTrue(self.mark(request))
        plan = request.context["flow_plan"]
        self.assertEqual(plan.context["redirect"], "https://sproyt.bjoroy.me/auth/login?enrollment=" + TOKEN)
        for prompt in [request.context["prompt_data"], plan.context["prompt_data"]]:
            self.assertNotIn(KEY, prompt)
            self.assertEqual(prompt["attributes"], {"keep": "yes", "email_verified": True,
                                                    "email_verified_address": "invitee@example.test"})
        self.assertEqual(request.context["invitation"].fixed_data[KEY], TOKEN)

    def test_present_invalid_token_cannot_continue(self):
        for token in [None, "", "a" * 42, "a" * 44, "a" * 42 + "&", "a" * 42 + "\n", 123, "é" * 43]:
            with self.subTest(token=token):
                request = context(token)
                request.context["prompt_data"][KEY] = TOKEN
                self.assertTrue(self.deny(request))
                self.assertFalse(self.mark(request))
                self.assertEqual(request.context["flow_plan"].context["redirect"], "https://attacker.invalid/")

    def test_absent_legacy_key_preserves_registration_without_prompt_based_continuation(self):
        request = context()
        del request.context["invitation"].fixed_data[KEY]
        request.context["prompt_data"][KEY] = TOKEN
        plan = request.context["flow_plan"]
        plan.context.pop("redirect")
        self.assertFalse(self.deny(request))
        self.assertTrue(self.mark(request))
        self.assertNotIn("redirect", plan.context)
        self.assertNotIn(KEY, request.context["prompt_data"])
        self.assertNotIn(KEY, plan.context["prompt_data"])
        self.assertTrue(request.context["prompt_data"]["attributes"]["email_verified"])

    def test_wrong_invitation_flow_email_or_inactive_context_is_denied(self):
        request = context(flow="other-flow")
        self.assertTrue(self.deny(request))
        self.assertFalse(self.mark(request))
        request = context()
        request.context["prompt_data"]["email"] = "other@example.test"
        self.assertTrue(self.deny(request))
        request = context()
        request.context["invitation_in_effect"] = False
        self.assertTrue(self.deny(request))
        self.assertFalse(self.mark(request))

    def test_missing_invitation_is_denied(self):
        request = context()
        request.context["invitation"] = None
        self.assertTrue(self.deny(request))
        self.assertFalse(self.mark(request))


if __name__ == "__main__":
    unittest.main()
