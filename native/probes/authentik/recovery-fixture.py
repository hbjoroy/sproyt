"""N01f email-proof laboratory configuration; never a production recovery signal."""
import json
import os
import traceback
from pathlib import Path

from django.db import transaction
from authentik.core.models import Application, User
from authentik.flows.models import Flow, FlowStageBinding
from authentik.policies.models import PolicyBinding
from authentik.policies.expression.models import ExpressionPolicy
from authentik.stages.deny.models import DenyStage
from authentik.stages.email.models import EmailStage
from authentik.stages.identification.models import IdentificationStage
from authentik.stages.authenticator_webauthn.models import AuthenticatorWebAuthnStage
from authentik.stages.user_login.models import UserLoginStage


def configure():
    configuration = json.loads(Path("/lab/bootstrap.json").read_text())
    assert configuration["environment"] == "sproyt-n01c-local"
    assert configuration["passkey_lab"] is True and configuration["recovery_lab"] is True
    application = Application.objects.get(slug="n01-lab")
    assert application.name == "Sprøyt N01c local laboratory"
    assert application.get_provider().client_secret == configuration["client_secret"]
    alice = User.objects.get(username="alice", name="N01c Alice", email="alice@example.invalid", is_active=True)
    # An admin-provisioned allowlist is authoritative ONLY inside this lab.
    # A writable profile attribute is deliberately not the verified-address source.
    verified = repr({str(alice.uuid): alice.email})
    for slug, atomic in (("n01f-email-baseline", False), ("n01f-recovery", True)):
        flow, _ = Flow.objects.get_or_create(slug=slug, defaults={"name": slug,
            "title": "Sprøyt N01f · recover synthetic Alice", "designation": "recovery",
            "authentication": "require_unauthenticated"})
        identification, _ = IdentificationStage.objects.get_or_create(name=slug + "-identify", defaults={
            "user_fields": ["username", "email"], "case_insensitive_matching": True,
            "show_matched_user": False, "pretend_user_exists": True})
        email, _ = EmailStage.objects.get_or_create(name=slug + "-email", defaults={
            "use_global_settings": True, "token_expiry": "minutes=5", "subject": "N01f synthetic recovery",
            "template": "email/account_confirmation.html", "activate_user_on_success": False,
            "recovery_max_attempts": 3, "recovery_cache_timeout": "minutes=5"})
        login, _ = UserLoginStage.objects.get_or_create(name=slug + "-login", defaults={
            "session_duration": "minutes=10", "remember_device": "seconds=0"})
        setup = AuthenticatorWebAuthnStage.objects.get(name="n01e-platform-setup")
        gate, _ = DenyStage.objects.get_or_create(name=slug + "-deny", defaults={"deny_message": "Recovery cannot continue. Request a new email."})

        eligibility = f'''
from hashlib import sha256
from django.core.cache import cache
from authentik.core.models import User
plan = request.context["flow_plan"]
user = request.context.get("pending_user")
fresh = User.objects.filter(pk=getattr(user, "pk", None)).first()
trusted = {verified}
address = trusted.get(str(getattr(fresh, "uuid", "")), "")
eligible = bool(fresh and fresh.is_active and address and fresh.email.strip().casefold() == address.casefold())
if eligible and not plan.context.get("n01f_initial_send_counted"):
    key = "n01f:{slug}:send:" + str(fresh.uuid)
    try:
        count = 1 if cache.add(key, 1, timeout=300) else cache.incr(key)
    except Exception:
        return True
    plan.context["n01f_initial_send_counted"] = True
    eligible = count <= 3
if not eligible:
    # Same email-sent challenge, no real recipient/token. Never reactivate a user.
    placeholder = User(username="n01f-placeholder", email="")
    plan.context["pending_user"] = placeholder
    request.context["pending_user"] = placeholder
    plan.context["email"] = ""
else:
    plan.context["email"] = address
    plan.context["n01f_subject"] = str(fresh.uuid)
    plan.context["n01f_recipient"] = address
return False
'''
        restored = f'''
from authentik.core.models import User
plan = request.context["flow_plan"]
token = request.context.get("is_restored")
user = request.context.get("pending_user")
trusted = {verified}
try:
    original = token.plan.context
    fresh = User.objects.get(pk=token.user_id)
    address = trusted.get(str(fresh.uuid), "")
    valid = (token.flow.slug == "{slug}" and getattr(user, "pk", None) == fresh.pk
             and fresh.is_active and address and fresh.email.strip().casefold() == address.casefold()
             and original.get("n01f_subject") == str(fresh.uuid)
             and original.get("n01f_recipient") == address and original.get("email") == address
             and plan.context.get("n01f_subject") == str(fresh.uuid)
             and plan.context.get("n01f_recipient") == address and plan.context.get("email") == address)
    return not valid
except Exception:
    return True
'''
        admission = '''
from hashlib import sha256
from uuid import uuid4
import json
from django.db import transaction
from authentik.core.models import Token
plan = request.context["flow_plan"]
token = request.context.get("is_restored")
try:
    # Created only AFTER email restoration/consent, never copied into the email
    # token's original plan. Distinct forks get distinct server attempt IDs.
    attempt = plan.context.setdefault("n01f_admission_attempt", uuid4().hex)
    session = request.http_request.session.session_key
    if not session or not token or not token.key:
        return True
    owner = json.dumps({"attempt": attempt, "session": sha256(session.encode()).hexdigest()}, sort_keys=True)
    identifier = "n01f-admission-" + sha256(token.key.encode()).hexdigest()
    with transaction.atomic():
        claim, _ = Token.objects.get_or_create(identifier=identifier, defaults={
            "user_id": token.user_id, "intent": "verification", "expiring": False,
            "description": owner})
        # Persistent tombstone: TTL cleanup cannot reopen a restored plan.
        return claim.user_id != token.user_id or claim.description != owner
except Exception:
    return True
'''
        stages = [(10, identification), (20, email), (40, setup), (100, login)]
        for order, stage in stages:
            FlowStageBinding.objects.get_or_create(target=flow, stage=stage, order=order,
                defaults={"evaluate_on_plan": False, "re_evaluate_policies": True})
        for order, name, expression in ((15, "eligibility", eligibility), (25, "restored", restored)):
            bind_gate(flow, gate, order, slug + "-" + name, expression)
        if atomic:
            bind_gate(flow, gate, 30, slug + "-admission", admission)
        assert list(FlowStageBinding.objects.filter(target=flow).order_by("order").values_list("order", flat=True)) == (
            [10, 15, 20, 25, 30, 40, 100] if atomic else [10, 15, 20, 25, 40, 100])
    return {"configured": True, "flow": "n01f-recovery", "baselineFlow": "n01f-email-baseline",
            "url": "http://localhost:19090/if/flow/n01f-recovery/", "smtp": "internal_mailpit_only",
            "verifiedAddressSource": "lab_admin_provisioned_policy_allowlist", "emailTokenExpiryConfigured": "minutes=5",
            "activateOnSuccess": False, "lostKeyRequired": False, "newKeyUserVerification": "required",
            "atomicAdmission": "unique non-expiring verification-token tombstone; lab extension",
            "recoveryGateApproved": False, "recoveryCompletionReceipt": False, "productionReady": False,
            "blockedBy": "outer policy execution failure can skip a Deny stage"}


def bind_gate(flow, gate, order, name, expression):
    # Expression evaluation errors return False. Return ALLOWED here and negate
    # at the Deny binding, so both evaluator errors and process timeouts deny.
    allowed = "def n01f_deny():\n" + "\n".join("    " + line for line in expression.strip().splitlines()) + "\nreturn not n01f_deny()\n"
    policy, _ = ExpressionPolicy.objects.get_or_create(name=name, defaults={"expression": allowed})
    assert policy.expression == allowed
    binding, _ = FlowStageBinding.objects.get_or_create(target=flow, stage=gate, order=order,
        defaults={"evaluate_on_plan": False, "re_evaluate_policies": True})
    assert binding.evaluate_on_plan is False and binding.re_evaluate_policies is True
    attached, _ = PolicyBinding.objects.get_or_create(target=binding, policy=policy, order=0,
        defaults={"enabled": True, "timeout": 10, "negate": True, "failure_result": True})
    assert attached.enabled and attached.negate and attached.failure_result


try:
    assert os.environ["N01F_ACTION"] == "configure"
    with transaction.atomic():
        result = configure()
    print("N01F_RESULT " + json.dumps(result))
except Exception as error:
    location = next((frame.lineno for frame in reversed(traceback.extract_tb(error.__traceback__))
                     if frame.filename == "/lab-code/recovery-fixture.py"), 0)
    print("N01F_FAILED configure " + type(error).__name__ + " line=" + str(location))
    raise SystemExit(1)
