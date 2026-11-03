"""Provider-only release gate. Run inside deployed Authentik with `blueprint` set.

Imports and executor scenarios roll back. One uniquely named account is committed
only for the two-connection race and removed in finally. Email is mocked, and all
probe addresses use example.invalid. Never supply a real user's identity here.
"""
import copy
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
from datetime import timedelta
import json
import hashlib
import hmac
import logging
import secrets
import struct
import time
from threading import Barrier
from types import SimpleNamespace
import uuid
from urllib.parse import urlencode
from unittest.mock import patch

from django.core import signing
from django.db import close_old_connections, connection, transaction
from django.test import Client, override_settings
from django.utils.timezone import now
from authentik.blueprints.v1.importer import Importer
from authentik.core.api.users import UserSerializer
from authentik.core.models import Group, User
from authentik.events.models import Event
from authentik.flows.models import Flow, FlowToken
from authentik.policies.engine import PolicyEngine
from authentik.policies.expression.models import ExpressionPolicy
from authentik.policies.models import PolicyBinding
from authentik.stages.email.stage import QS_KEY_TOKEN
from authentik.stages.invitation.models import Invitation
from authentik.stages.authenticator_totp.models import TOTPDevice
from authentik.stages.identification.models import IdentificationStage

if "blueprint" not in globals():
    raise SystemExit("Supply the reviewed recovery blueprint in the operator shell.")
logging.disable(logging.CRITICAL)
SLUG = "sproyt-account-recovery-v2"
BASE = "/api/v3/flows/executor/" + SLUG + "/"
MARKER = "sproyt_pending_signup_v1"
PREFIX = "sproyt_recovery_v2_"
probe_id = uuid.uuid4().hex
suffix = "-" + probe_id + "@example.invalid"
checks = []


def record(name):
    checks.append(name)


def settle(client, response):
    for _ in range(16):
        if response.status_code not in (302, 303):
            return response
        location = response.headers["Location"]
        assert not location.startswith("https://"), "unexpected external redirect"
        response = client.get(location, secure=True)
    raise AssertionError("unbounded flow traversal")


def policy(name):
    expression = ExpressionPolicy.objects.get(name=name).expression
    namespace = {}
    exec("def run(request):\n" + "\n".join("    " + line for line in expression.splitlines()), namespace)
    return namespace["run"]


def snapshot(user):
    expression = ExpressionPolicy.objects.get(name="sproyt-recovery-v2-classify").expression
    namespace = {}
    exec(expression.split('plan = request.context.get("flow_plan")', 1)[0], namespace)
    return namespace["snapshot"](user)


def stamp_fixture(user, kind="signup"):
    origin = {"v": 1, "kind": kind, "flow": "645a51d1-1d8a-5369-867d-b2a93ac7504e",
              "nonce": secrets.token_urlsafe(32), "snapshot": snapshot(user)}
    user.attributes[MARKER] = signing.dumps(origin, salt="sproyt.pending-signup.v1")
    user.save(update_fields=["attributes"])


def fixture(name, active=False, grant=True, usable=False):
    user = User.objects.create(username="kåre-" + name + "-" + probe_id,
        email=name + suffix, type="external", path="users/sproyt", is_active=active,
        attributes={"preserve": "value"})
    user.groups.add(Group.objects.get(name="sproyt-users"))
    user.set_password(secrets.token_urlsafe(40) if usable else None)
    user.save()
    if grant:
        stamp_fixture(user, "operator" if usable else "signup")
    return user


def start(user):
    client = Client()
    first = settle(client, client.get(BASE, secure=True))
    assert first.json().get("component") == "ak-stage-identification"
    response = client.post(BASE, data=json.dumps({"component": "ak-stage-identification",
        "uid_field": user.username}), content_type="application/json", secure=True)
    return client, settle(client, response)


def prove(user, flow, base=BASE):
    token = FlowToken.objects.filter(user=user, flow=flow).latest("expires")
    link = base + "?" + urlencode({"query": urlencode({QS_KEY_TOKEN: token.key})})
    client = Client()
    challenge = settle(client, client.get(link, secure=True)).json()
    assert challenge.get("component") == "ak-stage-consent", "fresh mailbox consent missing"
    response = client.post(link, data=json.dumps({"component": "ak-stage-consent",
        "consent": True, "token": challenge.get("token")}), content_type="application/json", secure=True)
    return client, link, settle(client, response), token


def finish(client, link, password):
    response = client.post(link, data=json.dumps({"component": "ak-stage-prompt",
        "password": password, "password_repeat": password}), content_type="application/json", secure=True)
    return settle(client, response)


try:
    with ExitStack() as stack:
        stack.enter_context(patch("django.core.cache.backends.locmem.LocMemCache.keys", create=True, return_value=[]))
        stack.enter_context(override_settings(ALLOWED_HOSTS=["testserver"], CACHES={"default": {
            "BACKEND": "django.core.cache.backends.locmem.LocMemCache", "LOCATION": "recovery-" + probe_id}}))
        stack.enter_context(patch("authentik.stages.email.stage.send_mails"))
        with transaction.atomic():
            assert Importer.from_string(blueprint).apply(), "recovery blueprint import failed"
            flow = Flow.objects.get(slug=SLUG)
            for name in ("classify", "rate-limit", "proof", "finalize"):
                binding = PolicyBinding.objects.get(policy__name="sproyt-recovery-v2-" + name)
                assert binding.failure_result, "policy error fallback must deny"
                original = binding.policy.expression
                try:
                    binding.policy.expression = "raise RuntimeError('isolated-recovery-error')"
                    binding.policy.save()
                    engine = PolicyEngine(binding.target, User.objects.first())
                    engine.use_cache = False
                    assert engine.build().passing is True, "engine error bypassed deny"
                finally:
                    binding.policy.expression = original
                    binding.policy.save()
            record("importer-and-real-policy-error-fallbacks")

            for name, active, grant, usable in (("pending", False, True, False),
                    ("legacy-grant", False, True, True), ("active", True, False, True)):
                user = fixture(name, active, grant, usable)
                _, response = start(user)
                assert response.json().get("component") == "ak-stage-email", response.json().get("component")
                client, link, response, token = prove(user, flow)
                assert response.json().get("component") == "ak-stage-prompt", response.json().get("component")
                user.refresh_from_db()
                assert user.is_active == active, "mailbox proof activated before password completion"
                password = secrets.token_urlsafe(40)
                response = finish(client, link, password)
                user.refresh_from_db()
                assert response.json().get("component") == "xak-flow-redirect", response.json().get("component")
                assert user.is_active and user.check_password(password)
                assert MARKER not in user.attributes and user.attributes["preserve"] == "value"
                # Authentik 2026.8 uses its own authenticated-session storage,
                # rather than Django's conventional _auth_user_id session key.
                assert user.last_login is not None, "UserLogin did not complete"
                before = (user.password, user.last_updated)
                finish(client, link, secrets.token_urlsafe(40))
                user.refresh_from_db()
                assert before == (user.password, user.last_updated), "replay rewrote password"
                record("executor-unicode-" + name + "-mailbox-password-login-replay")

            for name in ("unmarked", "revoked", "admin-edit"):
                user = fixture(name, grant=name != "unmarked")
                if name == "revoked":
                    user.attributes.pop(MARKER)
                    user.save(update_fields=["attributes"])
                if name == "admin-edit":
                    serializer = UserSerializer(user, data={"is_active": False}, partial=True)
                    assert serializer.is_valid(), "admin serializer failed"
                    serializer.save()
                before = (user.password, user.is_active)
                client, response = start(user)
                assert response.json().get("component") == "ak-stage-access-denied"
                user.refresh_from_db()
                assert before == (user.password, user.is_active) and user.last_login is None
            record("unmarked-revoked-and-administratively-disabled-accounts-denied")

            user = fixture("disabled-midway", active=True, grant=False, usable=True)
            start(user)
            client, link, response, _ = prove(user, flow)
            user.is_active = False
            user.save()
            before = user.password
            response = finish(client, link, secrets.token_urlsafe(40))
            user.refresh_from_db()
            assert response.json().get("component") == "ak-stage-access-denied"
            assert not user.is_active and user.password == before and user.last_login is None
            record("administrative-disable-after-mail-proof-denies-before-login")

            user = fixture("mfa", active=True, grant=False, usable=True)
            device = TOTPDevice.objects.create(user=user, name="isolated-recovery-probe", confirmed=True)
            start(user)
            client, link, response, _ = prove(user, flow)
            assert response.json().get("component") == "ak-stage-authenticator-validate", "MFA was bypassed"
            assert user.last_login is None
            response = settle(client, client.post(link, data=json.dumps({"component":"ak-stage-authenticator-validate"}),
                content_type="application/json", secure=True))
            assert response.json().get("component") == "ak-stage-authenticator-validate", "missing OTP was accepted"
            counter = int((time.time()-device.t0)/device.step)+device.drift
            digest = hmac.new(device.bin_key, struct.pack(">Q", counter), hashlib.sha1).digest()
            offset = digest[-1] & 15
            number = (struct.unpack(">I", digest[offset:offset+4])[0] & 0x7fffffff) % (10**device.digits)
            code = str(number).zfill(device.digits)
            response = settle(client, client.post(link, data=json.dumps({"component":"ak-stage-authenticator-validate","code":code}),
                content_type="application/json", secure=True))
            assert response.json().get("component") == "ak-stage-prompt", "valid OTP did not reach password prompt"
            password = secrets.token_urlsafe(40)
            finish(client, link, password)
            user.refresh_from_db()
            assert user.check_password(password) and user.last_login is not None
            record("configured-totp-required-missing-code-denied-valid-code-completes")

            user = fixture("added-mfa", active=True, grant=False, usable=True)
            start(user)
            client, link, response, _ = prove(user, flow)
            assert response.json().get("component") == "ak-stage-prompt"
            TOTPDevice.objects.create(user=user, name="isolated-recovery-probe", confirmed=True)
            before = user.password
            response = finish(client, link, secrets.token_urlsafe(40))
            user.refresh_from_db()
            assert response.json().get("component") == "ak-stage-access-denied"
            assert user.password == before and user.last_login is None
            record("mfa-added-after-proof-requires-restart")

            inviter = User.objects.create(username="recovery-inviter-" + probe_id)
            invitation_flow = Flow.objects.get(slug="sproyt-invitation-canary-v2")
            inv = Invitation.objects.create(name="recovery-probe-" + probe_id, created_by=inviter,
                flow=invitation_flow, single_use=True, expires=now() + timedelta(minutes=5),
                fixed_data={"email": "new" + suffix, "sproyt_enrollment_token": "a" * 43})
            entry = "/api/v3/flows/executor/" + invitation_flow.slug + "/?" + urlencode({"query": urlencode({"itoken": str(inv.pk)})})
            client = Client()
            response = settle(client, client.get(entry, secure=True))
            assert response.json().get("component") == "ak-stage-prompt"
            response = settle(client, client.post(entry, data=json.dumps({"component": "ak-stage-prompt",
                "username": "kåre-new-" + probe_id, "name": "Isolated recovery probe"}),
                content_type="application/json", secure=True))
            assert response.json().get("component") == "ak-stage-email"
            user = User.objects.get(email="new" + suffix)
            assert MARKER in user.attributes and not user.is_active
            start(user)
            client, link, response, _ = prove(user, flow)
            assert response.json().get("component") == "ak-stage-prompt"
            password = secrets.token_urlsafe(40)
            finish(client, link, password)
            user.refresh_from_db()
            assert user.is_active and user.check_password(password)
            record("actual-invitation-interrupted-before-mailbox-rescued-through-forgot-password")

            for slug in ("sproyt-invitation-production", "sproyt-invitation-canary", "sproyt-invitation-canary-v2"):
                origin = Flow.objects.get(slug=slug)
                address = slug + suffix
                inv = Invitation.objects.create(name="recovery-direct-"+probe_id, created_by=inviter,
                    flow=origin, single_use=True, expires=now()+timedelta(minutes=5),
                    fixed_data={"email":address,"sproyt_enrollment_token":"b"*43})
                origin_base = "/api/v3/flows/executor/"+slug+"/"
                entry = origin_base+"?"+urlencode({"query":urlencode({"itoken":str(inv.pk)})})
                client = Client()
                settle(client, client.get(entry, secure=True))
                response = settle(client, client.post(entry, data=json.dumps({"component":"ak-stage-prompt",
                    "username":"direct-"+slug+"-"+probe_id,"name":"Isolated direct signup"}),
                    content_type="application/json", secure=True))
                assert response.json().get("component") == "ak-stage-email"
                user = User.objects.get(email=address)
                assert MARKER in user.attributes
                client, link, response, _ = prove(user, origin, origin_base)
                assert response.json().get("component") == "ak-stage-prompt"
                password = secrets.token_urlsafe(40)
                finish(client, link, password)
                user.refresh_from_db()
                assert user.is_active and user.check_password(password) and user.last_login is not None
            record("all-three-original-invitation-flows-still-complete")

            if "legacy_blueprint" in globals():
                assert Importer.from_string(legacy_blueprint).apply()
                assert IdentificationStage.objects.get(name="sproyt-authentication-identification").recovery_flow_id == flow.pk
                record("legacy-blueprint-reimport-does-not-revert-public-recovery-link")
            transaction.set_rollback(True)

        # Import only within an outer transaction for policy objects; the unique
        # committed account below is removed regardless of race-test outcome.
        with transaction.atomic():
            assert Importer.from_string(blueprint).apply()
            # Use existing reviewed policy objects in a temporary savepoint only
            # for setup. Their changes must not leak from this verification run.
            expression = ExpressionPolicy.objects.get(name="sproyt-recovery-v2-finalize").expression
            classify_expression = ExpressionPolicy.objects.get(name="sproyt-recovery-v2-classify").expression
            proof_expression = ExpressionPolicy.objects.get(name="sproyt-recovery-v2-proof").expression
            transaction.set_rollback(True)
        # A separate committed test needs no persisted flow or imported policies.
        namespace = {}
        exec(classify_expression.split('plan = request.context.get("flow_plan")', 1)[0], namespace)
        real_snapshot = namespace["snapshot"]
        def expression_function(source):
            ns = {}
            exec("def run(request):\n" + "\n".join("    " + line for line in source.splitlines()), ns)
            return ns["run"]
        finalize, proof = expression_function(expression), expression_function(proof_expression)
        fixture_pk = None
        try:
            user = User.objects.create(username="recovery-race-" + probe_id, email="race" + suffix,
                is_active=False, type="external", path="users/sproyt", attributes={})
            fixture_pk = user.pk
            user.groups.add(Group.objects.get(name="sproyt-users"))
            user.set_password(None); user.save()
            state = real_snapshot(user)
            marker = signing.dumps({"v": 1, "kind": "signup", "flow": "645a51d1-1d8a-5369-867d-b2a93ac7504e",
                "nonce": secrets.token_urlsafe(32), "snapshot": state}, salt="sproyt.pending-signup.v1")
            user.attributes[MARKER] = marker; user.save(update_fields=["attributes"])
            context = {"pending_user": user, "email": user.email, "redirect": "https://sproyt.bjoroy.me/auth/login",
                PREFIX + "branch": "pending", PREFIX + "snapshot": state, PREFIX + "marker": marker}
            original = SimpleNamespace(context=copy.deepcopy(context), bindings=[
                SimpleNamespace(stage=SimpleNamespace(name="consent")),
                SimpleNamespace(stage=SimpleNamespace(name="sproyt-recovery-v2-email"))])
            token = SimpleNamespace(flow_id="9b763222-baf4-5a03-a3b8-26f0bc1f71c1", user=user,
                plan=original, expires=now()+timedelta(minutes=30))
            request = SimpleNamespace(context={"pending_user": user, "flow_plan": SimpleNamespace(context=context),
                "is_restored": token}, http_request=None)
            assert proof(request) is False
            expired = copy.deepcopy(request)
            expired.context["is_restored"].expires = now()-timedelta(seconds=1)
            expired.context["prompt_data"] = {"password":"isolated-never-written","password_repeat":"isolated-never-written"}
            assert finalize(expired) is True
            forged = copy.deepcopy(request)
            forged.context["flow_plan"].context[PREFIX+"mailbox"] = "forged"
            forged.context["prompt_data"] = expired.context["prompt_data"]
            assert finalize(forged) is True
            record("expired-mailbox-and-forged-proof-deny-before-write")
            attempts = [copy.deepcopy(request), copy.deepcopy(request)]
            passwords = [secrets.token_urlsafe(40), secrets.token_urlsafe(40)]
            barrier = Barrier(2)
            def complete(index):
                close_old_connections()
                try:
                    attempts[index].context["prompt_data"] = {"password": passwords[index], "password_repeat": passwords[index]}
                    barrier.wait(timeout=10)
                    return finalize(attempts[index])
                finally:
                    connection.close()
            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(complete, [0, 1]))
            assert sorted(results) == [False, True], "completion must have one winner"
            user.refresh_from_db()
            assert user.is_active and user.check_password(passwords[results.index(False)])
            assert not attempts[results.index(True)].context["flow_plan"].context["pending_user"].is_active
            record("postgres-two-connections-single-winner-without-stale-login")
        finally:
            if fixture_pk is not None:
                User.objects.filter(pk=fixture_pk, username="recovery-race-"+probe_id).delete()
            Event.objects.filter(user__username="recovery-race-"+probe_id).delete()
        assert not User.objects.filter(email__endswith=suffix).exists()
        record("isolated-fixtures-removed-no-real-mail-delivery")
    print("SPROYT_RECOVERY_RUNTIME " + json.dumps({"ok": True, "checks": checks}))
except Exception as exc:
    import traceback
    print("SPROYT_RECOVERY_RUNTIME " + json.dumps({"ok": False, "checks": checks,
        "error_type": type(exc).__name__, "error": str(exc)[:200],
        "where": [(f.name, f.lineno) for f in traceback.extract_tb(exc.__traceback__)]}))
    raise SystemExit(1)
