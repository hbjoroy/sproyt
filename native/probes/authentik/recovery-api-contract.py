"""N01f2 public recovery API/restore observation, not a recovery coordinator."""
import hmac
import json
import threading
import traceback
from concurrent.futures import ThreadPoolExecutor
from datetime import timedelta
from pathlib import Path
from unittest.mock import patch
from urllib.parse import parse_qs, urlencode, urlsplit
from uuid import uuid4

from django.db import close_old_connections
from django.test import Client, RequestFactory
from django.utils.timezone import now

from authentik.brands.models import Brand
from authentik.brands.utils import get_brand_for_request
from authentik.core.models import Application, AuthenticatedSession, Token, User, UserTypes
from authentik.flows.models import Flow, FlowStageBinding, FlowToken
from authentik.flows.views.executor import QS_KEY_TOKEN, QS_QUERY
from authentik.providers.oauth2.models import AccessToken, RefreshToken
from authentik.rbac.models import Role
from authentik.stages.authenticator_webauthn.models import AuthenticatorWebAuthnStage, WebAuthnDevice
from authentik.stages.user_login.models import UserLoginStage

HOST = "localhost:19090"
SLUG = "n01f2-api-recovery"
phase, observed_status = "guard", 0
service = role = bearer = flow = login = brand = None
original_recovery = None
owned_identifier = private_key = None
brand_changed = False
recovery_token_owned = False


def require(condition):
    if not condition:
        raise AssertionError("N01f2 contract assertion failed")


def response_json(response):
    global observed_status
    observed_status = response.status_code
    require(len(response.content) <= 131072)
    return response.json() if response.status_code == 200 else {}


def mint(client, user):
    response = client.post("/api/v3/core/users/" + str(user.pk) + "/recovery/", data=json.dumps({"token_duration": "minutes=2"}), content_type="application/json", HTTP_HOST=HOST)
    body = response_json(response)
    return response.status_code, body


def link_key(body):
    target = urlsplit(body["link"])
    require(target.scheme == "http" and target.netloc == HOST and target.path == "/if/flow/" + SLUG + "/")
    values = parse_qs(target.query)[QS_KEY_TOKEN]
    require(len(values) == 1 and 0 < len(values[0]) <= 512)
    return values[0], "/api/v3/flows/executor/" + SLUG + "/?" + urlencode({QS_QUERY: target.query})


def restore(client, path):
    response = client.get(path, HTTP_HOST=HOST)
    for _ in range(8):
        if response.status_code not in (302, 303):
            break
        location = urlsplit(response.headers.get("Location", ""))
        require(location.scheme in ("", "http") and location.netloc in ("", HOST))
        if location.path != "/api/v3/flows/executor/" + SLUG + "/":
            return False
        response = client.get(location.path + ("?" + location.query if location.query else ""), HTTP_HOST=HOST)
    body = response_json(response)
    return body.get("component") == "ak-stage-authenticator-webauthn" and not body.get("response_errors")


try:
    config = json.loads(Path("/lab/bootstrap.json").read_text())
    require(config["environment"] == "sproyt-n01c-local" and config["recovery_lab"] is True and config["passkey_lab"] is True)
    app = Application.objects.get(slug="n01-lab", name="Sprøyt N01c local laboratory")
    require(app.get_provider().client_secret == config["client_secret"])
    alice = User.objects.get(username="alice", name="N01c Alice", email="alice@example.invalid", is_active=True)
    bob = User.objects.get(username="bob", name="N01c Bob", email="bob@example.invalid")
    alice_uuid, users_before = alice.uuid, User.objects.count()
    devices_before = WebAuthnDevice.objects.count()
    owned_identifier = str(alice.uid) + "-password-reset"
    require(not FlowToken.objects.filter(identifier=owned_identifier).exists())
    recovery_token_owned = True
    require(not Flow.objects.filter(slug=SLUG).exists())

    phase = "temporary-fixed-flow"
    flow = Flow.objects.create(slug=SLUG, name="N01f2 temporary API recovery observation", title="N01f2 local observation", designation="recovery", authentication="require_unauthenticated")
    setup = AuthenticatorWebAuthnStage.objects.get(name="n01e-platform-setup")
    login = UserLoginStage.objects.create(name="n01f2-api-login-" + uuid4().hex, session_duration="minutes=10", remember_device="seconds=0")
    for order, stage in ((10, setup), (20, login)):
        FlowStageBinding.objects.create(target=flow, stage=stage, order=order, evaluate_on_plan=False, re_evaluate_policies=True)
    brand = get_brand_for_request(RequestFactory().get("/", HTTP_HOST=HOST))
    require(brand.pk is not None)
    original_recovery = brand.flow_recovery_id
    Brand.objects.filter(pk=brand.pk).update(flow_recovery=flow)
    brand_changed = True

    phase = "scoped-bearer-fixture"
    suffix = uuid4().hex
    service = User.objects.create(username="n01f2-api-" + suffix, name="N01f2 temporary scoped recovery service", type=UserTypes.SERVICE_ACCOUNT, path="service-accounts/n01f2", is_active=True)
    service.set_unusable_password()
    service.save(update_fields=["password"])
    role = Role.objects.create(name="n01f2-alice-reset-" + suffix)
    # Pinned 2026.8.2 Role.assign_perms uses Guardian object permissions.
    # Only the two action prerequisites on Alice; no global/create scope or fallback.
    role.assign_perms(["authentik_core.reset_user_password", "authentik_core.add_user"], alice)
    service.roles.add(role)
    require(not service.is_superuser and not service.groups.exists())
    require(service.has_perm("authentik_core.reset_user_password", alice))
    for permission in ("authentik_core.reset_user_password", "authentik_core.add_user"):
        require(service.has_perm(permission, alice) and not service.has_perm(permission, bob) and not service.has_perm(permission))
    for model in (AuthenticatedSession, AccessToken, RefreshToken):
        for action in ("view", "add", "change", "delete"):
            require(not service.has_perm(model._meta.app_label + "." + action + "_" + model._meta.model_name))
    bearer = Token.objects.create(identifier="n01f2-api-" + suffix, user=service, intent="api", expiring=True, expires=now() + timedelta(minutes=2))
    api = Client(HTTP_AUTHORIZATION="Bearer " + bearer.key)
    phase = "global-create-user-denied"
    denied_username = "n01f2-denied-create-" + uuid4().hex
    creation = api.post("/api/v3/core/users/", data=json.dumps({"username": denied_username, "name": "N01f2 denied synthetic create", "email": "n01f2-denied@example.invalid", "is_active": False}), content_type="application/json", HTTP_HOST=HOST)
    observed_status = creation.status_code
    require(observed_status in (403, 404) and not User.objects.filter(username=denied_username).exists())
    phase = "scoped-api-alice-mint"
    status, first = mint(api, alice)
    require(status == 200)  # Stop/report on unsupported least-privilege API access.
    private_key, path = link_key(first)
    phase = "scoped-api-bob-denied"
    status, _ = mint(api, bob)
    require(status in (403, 404))
    phase = "repeat-mint-key-observation"
    status, again = mint(api, alice)
    require(status == 200)
    again_key, path = link_key(again)
    same_key = hmac.compare_digest(private_key, again_key)
    require(same_key)
    token = FlowToken.objects.get(identifier=owned_identifier, key=private_key)
    require(token.revoke_on_execution and token.user_id == alice.pk and token.flow_id == flow.pk)

    phase = "concurrent-real-plan-restore"
    original_plan = FlowToken.plan
    barrier = threading.Barrier(2, timeout=5)
    hits = []
    def overlapping_plan(token):
        if hmac.compare_digest(token.key, private_key):
            hits.append(True)
            barrier.wait()
        return original_plan.fget(token)
    def request_restore():
        close_old_connections()
        try:
            return restore(Client(), path)
        finally:
            close_old_connections()
    with patch.object(FlowToken, "plan", property(overlapping_plan, original_plan.fset, original_plan.fdel, original_plan.__doc__)):
        with ThreadPoolExecutor(max_workers=2) as pool:
            pending = [pool.submit(request_restore) for _ in range(2)]
            admitted = sum(int(future.result(timeout=10)) for future in pending)
    require(len(hits) == 2 and admitted == 2)
    phase = "new-browser-replay"
    require(not restore(Client(), path))
    alice.refresh_from_db()
    require(alice.uuid == alice_uuid and User.objects.exclude(pk=service.pk).count() == users_before and WebAuthnDevice.objects.count() == devices_before)
    result = {"passed": True, "scope": "n01f2_public_api_restore_observation", "aliceScopedRecoveryAllowed": True, "globalCreateUserDenied": True, "bobRecoveryDenied": True, "sessionGrantManagementNotGranted": True, "repeatMintSameKey": same_key, "concurrentRestoreAdmissions": admitted, "newBrowserReplayDenied": True, "sameSubject": True, "noUsersCreated": True, "noWebAuthnDevicesCreated": True, "publicRecoveryApiApproved": False, "fullRecoveryVerified": False, "hardwarePasskeyVerified": False}
except Exception as error:
    location = next((frame.lineno for frame in reversed(traceback.extract_tb(error.__traceback__)) if frame.filename.endswith("recovery-api-contract.py") and frame.name != "require"), 0)
    print("N01F2_FAILED " + phase + " " + type(error).__name__ + " line=" + str(location) + " status=" + str(observed_status))
    raise SystemExit(1)
finally:
    if brand_changed:
        Brand.objects.filter(pk=brand.pk).update(flow_recovery_id=original_recovery)
    if recovery_token_owned:
        FlowToken.objects.filter(identifier=owned_identifier).delete()
    if bearer is not None:
        bearer.delete()
    if service is not None:
        service.delete()
    if role is not None:
        role.delete()
    if flow is not None:
        flow.delete()
    if login is not None:
        login.delete()
require(Brand.objects.get(pk=brand.pk).flow_recovery_id == original_recovery)
require(not Flow.objects.filter(slug=SLUG).exists())
require(not User.objects.filter(username="n01f2-api-" + suffix).exists())
require(not User.objects.filter(username=denied_username).exists())
require(not Role.objects.filter(name="n01f2-alice-reset-" + suffix).exists())
require(not Token.objects.filter(identifier="n01f2-api-" + suffix).exists())
require(not FlowToken.objects.filter(identifier=owned_identifier).exists())
result["temporaryStateRestored"] = True
print("N01F2_RESULT " + json.dumps(result))
