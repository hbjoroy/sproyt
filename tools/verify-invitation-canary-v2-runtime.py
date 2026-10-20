"""Run via the deployed Authentik shell, with `blueprint` supplied privately.

Importer/executor checks roll back. The lock test uses one committed, uniquely
named probe account so two database connections can see it, then deletes only
that account in finally. Delivery is mocked; no application messages are sent.
Run only as the documented pre-release operator gate, never on real users.
"""
import copy
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
from datetime import timedelta
import json
import logging
import secrets
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
from authentik.core.models import Group, Source, User, UserSourceConnection
from authentik.flows.models import Flow, FlowToken
from authentik.stages.email.stage import QS_KEY_TOKEN
from authentik.stages.invitation.models import Invitation

if "blueprint" not in globals():
    raise SystemExit("Supply the reviewed blueprint in the Authentik operator shell.")

logging.disable(logging.CRITICAL)
FLOW = "a1b4b2b7-80e3-591d-b52f-bed0ae3e5a35"
SLUG = "sproyt-invitation-canary-v2"
PREFIX, MARKER = "sproyt_canary_v2_", "sproyt_canary_v2_pending"
BASE = "/api/v3/flows/executor/" + SLUG + "/"
checks = []
probe_id = uuid.uuid4().hex
email_suffix = "-" + probe_id + "@example.invalid"


def record(name):
    checks.append(name)


def settle(client, response):
    for _ in range(16):
        if response.status_code not in (302, 303):
            return response
        location = response.headers["Location"]
        assert not location.startswith("https://"), "unexpected external redirect"
        response = client.get(location, secure=True)
    raise AssertionError("executor exceeded bounded stage traversal")


def start(inviter, flow, email, char, authenticated=False):
    inv = Invitation.objects.create(name="sproyt-v2-probe-" + probe_id,
        created_by=inviter, flow=flow, single_use=True,
        expires=now() + timedelta(minutes=5),
        fixed_data={"email": email, "sproyt_enrollment_token": char * 43})
    client = Client()
    if authenticated:
        client.force_login(inviter)
    entry = BASE + "?" + urlencode({"query": urlencode({"itoken": str(inv.pk)})})
    response = settle(client, client.get(entry, secure=True))
    return client, entry, response


def prove(user, flow, excluded=()):
    tokens = FlowToken.objects.filter(user=user, flow=flow).exclude(pk__in=excluded)
    assert tokens.count() == 1, "expected a fresh independent mailbox token"
    token = tokens.get()
    link = BASE + "?" + urlencode({"query": urlencode({QS_KEY_TOKEN: token.key})})
    client = Client()
    data = settle(client, client.get(link, secure=True)).json()
    assert data.get("component") == "ak-stage-consent", "mail consent missing"
    response = client.post(link, data=json.dumps({"component": "ak-stage-consent",
        "consent": True, "token": data.get("token")}), content_type="application/json", secure=True)
    response = settle(client, response)
    user.refresh_from_db()
    return client, link, response, token


def policy(policy_id):
    import textwrap
    block = blueprint.split("    id: " + policy_id + "\n", 1)[1].split("      expression: |\n", 1)[1]
    lines = []
    for line in block.splitlines():
        if line and not line.startswith("        "):
            break
        lines.append(line[8:] if line else "")
    namespace = {}
    exec("def run(request):\n" + textwrap.indent("\n".join(lines), "    "), namespace)
    return namespace["run"]


try:
    with ExitStack() as stack:
        stack.enter_context(patch("django.core.cache.backends.locmem.LocMemCache.keys", create=True, return_value=[]))
        stack.enter_context(override_settings(ALLOWED_HOSTS=["testserver"], CACHES={"default": {
            "BACKEND": "django.core.cache.backends.locmem.LocMemCache", "LOCATION": "sproyt-v2-" + probe_id}}))
        mail = stack.enter_context(patch("authentik.stages.email.stage.send_mails"))
        with transaction.atomic():
            assert Importer.from_string(blueprint).apply(), "import failed"
            flow = Flow.objects.get(slug=SLUG)
            inviter = User.objects.create(username="sproyt-v2-inviter-" + probe_id, email="inviter" + email_suffix)
            for scenario in ["direct", "before-proof", "after-proof"]:
                email = scenario + email_suffix
                client, entry, response = start(inviter, flow, email, "a", authenticated=scenario == "before-proof")
                assert response.json().get("component") == "ak-stage-prompt", "profile missing"
                response = settle(client, client.post(entry, data=json.dumps({"component": "ak-stage-prompt",
                    "username": "sproyt-v2-" + scenario + "-" + probe_id, "name": "Isolated V2 probe"}),
                    content_type="application/json", secure=True))
                assert response.json().get("component") == "ak-stage-email", "initial mailbox missing"
                user = User.objects.get(email=email)
                assert not user.is_active and not user.has_usable_password()
                assert MARKER in user.attributes, "creation provenance missing"
                old_tokens = list(FlowToken.objects.filter(user=user, flow=flow).values_list("pk", flat=True))
                if scenario == "after-proof":
                    _, _, prior, _ = prove(user, flow)
                    assert prior.json().get("component") == "ak-stage-prompt"
                    assert not user.is_active and not user.has_usable_password()
                if scenario != "direct":
                    _, _, resumed = start(inviter, flow, email, "b", authenticated=True)
                    assert resumed.json().get("component") == "ak-stage-email", "resume must skip profile and reprove mailbox"
                    client, entry, response, token = prove(user, flow, old_tokens)
                else:
                    client, entry, response, token = prove(user, flow)
                assert response.json().get("component") == "ak-stage-prompt", "password prompt missing"
                assert not user.is_active and not user.has_usable_password()
                password = secrets.token_urlsafe(40)
                response = settle(client, client.post(entry, data=json.dumps({"component": "ak-stage-prompt",
                    "password": password, "password_repeat": password}), content_type="application/json", secure=True))
                data = response.json()
                user.refresh_from_db()
                assert user.is_active and user.check_password(password), "completion did not activate once"
                assert MARKER not in user.attributes, "provenance not consumed"
                assert data.get("component") == "xak-flow-redirect", "completion failed"
                assert data.get("to", "").startswith("https://sproyt-canary.bjoroy.me/auth/login?enrollment=")
                assert not FlowToken.objects.filter(pk=token.pk).exists(), "mail token not consumed"
                before = (user.password, user.is_active)
                _, _, again = start(inviter, flow, email, "c")
                assert again.json().get("component") == "xak-flow-redirect", "active identity must use ordinary OIDC"
                user.refresh_from_db()
                assert before == (user.password, user.is_active)
                record("executor-" + scenario + "-single-activation-canary-only")
            for verified in (True, False):
                email = "existing-" + str(verified) + email_suffix
                user = User.objects.create(username="sproyt-v2-existing-" + str(verified) + "-" + probe_id,
                    email=email, name="Existing probe", is_active=verified, attributes={
                        "email_verified": verified, "email_verified_address": email, "preserve": True})
                user.set_password(secrets.token_urlsafe(40)); user.save()
                before = (user.password, user.is_active, list(user.groups.values_list("pk", flat=True)))
                client, _, response = start(inviter, flow, email, "d")
                if not verified:
                    assert response.json().get("component") == "ak-stage-email"
                    client, _, response, _ = prove(user, flow)
                assert response.json().get("component") == "xak-flow-redirect"
                user.refresh_from_db()
                assert before == (user.password, user.is_active, list(user.groups.values_list("pk", flat=True)))
                assert "_auth_user_id" not in client.session
                record("executor-existing-" + str(verified) + "-preserved-no-auto-login")
            transaction.set_rollback(True)
        assert not User.objects.filter(email__endswith=email_suffix).exists(), "rollback leaked users"
        assert not Flow.objects.filter(pk=FLOW).exists(), "rollback leaked V2 flow"
        record("importer-and-executor-rollback-confirmed")

        # Cross-connection concurrency needs one committed dedicated fixture.
        # No flow, group, existing account or provider configuration is changed.
        fixture = User.objects.create(username="sproyt-v2-lock-" + probe_id,
            email="lock" + email_suffix, type="external", path="users/sproyt", is_active=False,
            attributes={"email_verified": False})
        fixture.set_unusable_password(); fixture.save()
        fixture.groups.add(Group.objects.get(name="sproyt-users"))
        fixture.refresh_from_db()
        fixture_pk = fixture.pk
        try:
            plan = SimpleNamespace(context={PREFIX + "create_user": True, PREFIX + "new_user": True,
                PREFIX + "email": fixture.email, "email": fixture.email,
                PREFIX + "verify_email": True, "pending_user": copy.deepcopy(fixture),
                "redirect": "https://sproyt-canary.bjoroy.me/auth/login?enrollment=" + "r" * 43})
            request = SimpleNamespace(context={"flow_plan": plan}, http_request=None)
            stamp = policy("canary-v2-stamp-placeholder")
            proof_policy = policy("canary-v2-email-proof")
            commit = policy("canary-v2-password-write-guard")
            assert stamp(request) is False
            original = copy.deepcopy(plan)
            original.bindings = [SimpleNamespace(stage=SimpleNamespace(name="consent")),
                SimpleNamespace(stage=SimpleNamespace(name="sproyt-canary-v2-existing-email"))]
            token = SimpleNamespace(flow_id=FLOW, plan=original, user=fixture,
                expires=now() + timedelta(minutes=30))
            request.context["is_restored"] = token
            assert proof_policy(request) is False
            request.context["prompt_data"] = {"password": secrets.token_urlsafe(40)}
            request.context["prompt_data"]["password_repeat"] = request.context["prompt_data"]["password"]

            # Actual normal admin API save revokes even an already-inactive user.
            with transaction.atomic():
                admin = UserSerializer(fixture, data={"is_active": False}, partial=True)
                assert admin.is_valid(), "admin serializer rejected fixture"
                admin.save()
                assert commit(copy.deepcopy(request)) is True, "admin save failed to revoke"
                transaction.set_rollback(True)
            fixture.refresh_from_db()
            record("normal-admin-deactivation-revokes-pending-registration")

            with transaction.atomic():
                fixture.attributes.pop(MARKER); fixture.save(update_fields=["attributes"])
                fixture.is_active = False; fixture.save()
                assert stamp(copy.deepcopy(request)) is True, "administrative edit was blessed at stamp"
                transaction.set_rollback(True)
            fixture.refresh_from_db()
            record("creation-snapshot-interference-denied-before-stamp")

            with transaction.atomic():
                source = Source.objects.create(name="sproyt-v2-source-" + probe_id, slug="sproyt-v2-source-" + probe_id)
                UserSourceConnection.objects.create(user=fixture, source=source, identifier="isolated-v2-probe")
                assert commit(copy.deepcopy(request)) is True, "linked source bypassed"
                transaction.set_rollback(True)
            record("linked-external-source-denies-completion")

            with transaction.atomic():
                fixture.is_active = True; fixture.save(update_fields=["is_active"])
                assert commit(copy.deepcopy(request)) is True, "activation race reset account"
                transaction.set_rollback(True)
            fixture.refresh_from_db()
            record("activation-between-proof-and-completion-denied")

            with transaction.atomic():
                fixture.attributes.pop(MARKER); fixture.save(update_fields=["attributes"])
                assert commit(copy.deepcopy(request)) is True
                transaction.set_rollback(True)
            fixture.refresh_from_db()
            record("explicit-marker-revocation-denies")

            with transaction.atomic():
                token.expires = now() - timedelta(seconds=1)
                assert commit(copy.deepcopy(request)) is True
                token.expires = now() + timedelta(minutes=30)
                transaction.set_rollback(True)
            record("expired-proof-denies-completion")

            # A failure after save but before transaction commit rolls back every field.
            snapshot = (fixture.password, fixture.is_active, copy.deepcopy(fixture.attributes), fixture.last_updated)
            try:
                with transaction.atomic():
                    assert commit(copy.deepcopy(request)) is False
                    raise RuntimeError("injected-before-commit")
            except RuntimeError:
                pass
            fixture.refresh_from_db()
            assert snapshot == (fixture.password, fixture.is_active, fixture.attributes, fixture.last_updated)
            record("failure-before-commit-rolls-back-password-activation-and-marker")

            barrier = Barrier(2)
            attempts = [copy.deepcopy(request), copy.deepcopy(request)]
            passwords = [secrets.token_urlsafe(40), secrets.token_urlsafe(40)]

            def finish(index):
                close_old_connections()
                try:
                    attempts[index].context["prompt_data"] = {"password": passwords[index], "password_repeat": passwords[index]}
                    barrier.wait(timeout=10)
                    return commit(attempts[index])
                finally:
                    connection.close()

            with ThreadPoolExecutor(max_workers=2) as pool:
                results = list(pool.map(finish, [0, 1]))
            assert sorted(results) == [False, True], "concurrent completion was not single-winner"
            fixture.refresh_from_db()
            winner = results.index(False)
            assert fixture.is_active and fixture.check_password(passwords[winner])
            assert not fixture.check_password(passwords[1-winner]) and MARKER not in fixture.attributes
            # pending_user is the pre-existing email recipient, not a login.
            # A denied attempt must keep that stale recipient inactive; only
            # the successful policy may refresh it and reach User Login.
            assert not attempts[1-winner].context["flow_plan"].context["pending_user"].is_active
            assert commit(copy.deepcopy(request)) is True, "stale mailbox proof replay succeeded"
            record("postgres-two-connections-one-winner-no-password-reset-or-stale-login")
        finally:
            User.objects.filter(pk=fixture_pk, username="sproyt-v2-lock-" + probe_id).delete()
        assert not User.objects.filter(email__endswith=email_suffix).exists(), "fixture cleanup failed"
        record("probe-accounts-removed-no-real-mail-delivery")
    print("SPROYT_V2_RUNTIME " + json.dumps({"ok": True, "checks": checks}))
except Exception as exc:
    import traceback
    print("SPROYT_V2_RUNTIME " + json.dumps({"ok": False, "checks": checks,
        "error_type": type(exc).__name__, "where": [(f.name, f.lineno) for f in traceback.extract_tb(exc.__traceback__)]}))
    raise SystemExit(1)
