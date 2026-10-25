"""N01e config and read-only WebAuthn evidence in a dedicated localhost lab."""
import json
import os
import traceback
import hashlib
import hmac
import uuid
from pathlib import Path

from django.db import transaction
from django.conf import settings
from django.test import Client
from django.utils.dateparse import parse_datetime
from django.utils.timezone import now
from rest_framework.test import APIClient

from authentik.core.models import Application, AuthenticatedSession, User
from authentik.events.signals import get_login_event
from authentik.providers.oauth2.models import AccessToken
from authentik.flows.models import Flow, FlowStageBinding
from authentik.stages.authenticator_validate.models import AuthenticatorValidateStage
from authentik.stages.authenticator_webauthn.models import AuthenticatorWebAuthnStage, WebAuthnDevice
from authentik.stages.user_login.models import UserLoginStage
from authentik.root.middleware import SessionMiddleware

phase = "guard"


class ConsumedProof(Exception):
    """Only a consumed server-owned proof produces the replay rejection marker."""


def policy():
    setup = AuthenticatorWebAuthnStage.objects.get(name="n01e-platform-setup")
    validation = AuthenticatorValidateStage.objects.get(name="n01e-passkey-validation")
    login = Flow.objects.get(slug="n01e-passkey-login")
    assert setup.user_verification == "required"
    assert setup.resident_key_requirement == "required"
    assert setup.authenticator_attachment == "platform"
    assert list(validation.device_classes) == ["webauthn"]
    assert validation.not_configured_action == "deny"
    assert validation.last_auth_threshold == "seconds=0"
    assert validation.webauthn_user_verification == "required"
    assert not validation.configuration_stages.exists()
    assert list(FlowStageBinding.objects.filter(target=login).order_by("order").values_list("stage__name", flat=True)) == [
        "n01e-passkey-validation", "n01e-passkey-session"]
    return {"configured": True, "origin": "http://localhost:19090", "rpId": "localhost",
            "setupUrl": "http://localhost:19090/if/flow/n01e-passkey-setup/",
            "loginFlow": login.slug, "userVerification": "required", "residentKey": "required",
            "attachment": "platform", "passwordFallback": False}


try:
    configuration = json.loads(Path("/lab/bootstrap.json").read_text())
    assert configuration["environment"] == "sproyt-n01c-local" and configuration["passkey_lab"] is True
    application = Application.objects.get(slug="n01-lab")
    assert application.name == "Sprøyt N01c local laboratory"
    provider = application.get_provider()
    assert provider.client_secret == configuration["client_secret"]
    alice = User.objects.get(username="alice", name="N01c Alice", email="alice@example.invalid", is_active=True)
    action = os.environ["N01E_ACTION"]
    assert action in {"configure", "inspect", "contract", "prepare-login", "verify-login"}
    phase = action
    if action == "configure":
        with transaction.atomic():
            setup_flow, _ = Flow.objects.get_or_create(slug="n01e-passkey-setup", defaults={
                "name": "N01e laboratory passkey setup", "title": "Sprøyt N01e · test passkey for Alice",
                "designation": "stage_configuration", "authentication": "require_authenticated"})
            setup, _ = AuthenticatorWebAuthnStage.objects.get_or_create(name="n01e-platform-setup", defaults={
                "configure_flow": setup_flow, "friendly_name": "Sprøyt N01e test passkey",
                "user_verification": "required", "resident_key_requirement": "required",
                "authenticator_attachment": "platform", "hints": ["client-device"], "max_attempts": 3})
            FlowStageBinding.objects.get_or_create(target=setup_flow, stage=setup, order=0)
            login_flow, _ = Flow.objects.get_or_create(slug="n01e-passkey-login", defaults={
                "name": "N01e passkey only", "title": "Sprøyt N01e · sign in with test passkey",
                "designation": "authentication", "authentication": "none"})
            validation, _ = AuthenticatorValidateStage.objects.get_or_create(name="n01e-passkey-validation", defaults={
                "device_classes": ["webauthn"], "not_configured_action": "deny", "last_auth_threshold": "seconds=0",
                "webauthn_user_verification": "required", "webauthn_hints": ["client-device"]})
            login, _ = UserLoginStage.objects.get_or_create(name="n01e-passkey-session", defaults={
                "session_duration": "minutes=30", "remember_me_offset": "seconds=0", "remember_device": "seconds=0"})
            FlowStageBinding.objects.get_or_create(target=login_flow, stage=validation, order=0)
            FlowStageBinding.objects.get_or_create(target=login_flow, stage=login, order=10)
        result = policy()
    elif action == "inspect":
        result = policy()
        result["registeredDevices"] = WebAuthnDevice.objects.filter(user=alice, rp_id="localhost").count()
    elif action == "contract":
        result = policy()
        route = "/api/v3/flows/executor/n01e-passkey-login/"
        anonymous = Client()
        response = anonymous.get(route, HTTP_HOST="localhost:19090")
        assert response.status_code == 200
        challenge = response.json()
        assert challenge["component"] == "ak-stage-authenticator-validate"
        assert {item["device_class"] for item in challenge["device_challenges"]} == {"webauthn"}
        rejected = anonymous.post(route, data=json.dumps({"component": challenge["component"],
            "webauthn": {"id": "bm9uZXhpc3RlbnQ", "rawId": "bm9uZXhpc3RlbnQ", "type": "public-key",
                         "response": {}}}), content_type="application/json", HTTP_HOST="localhost:19090")
        assert rejected.status_code in {200, 400}
        assert rejected.json().get("response_errors")
        assert not anonymous.session.get("_auth_user_id")
        authenticated = Client()
        authenticated.force_login(alice)
        key = authenticated.session.session_key
        authenticated.cookies[settings.SESSION_COOKIE_NAME] = SessionMiddleware.encode_session(key, alice)
        response = authenticated.get(route, HTTP_HOST="localhost:19090")
        assert response.status_code == 200
        assert response.json()["component"] in {"ak-stage-authenticator-validate", "ak-stage-access-denied"}
        result.update({"invalidAssertionRejected": True, "cookieAloneCannotSkipValidation": True,
                       "hardwarePasskeyVerified": False})
    elif action == "prepare-login":
        result = policy()
        devices = list(WebAuthnDevice.objects.filter(user=alice, rp_id="localhost"))
        assert devices
        # Snapshot only private IDs/timestamps, never credential ID or public key.
        snapshot = [{"pk": str(device.pk), "last_t": device.last_t.isoformat()} for device in devices]
        attempt = str(uuid.UUID(os.environ["N01E_ATTEMPT_ID"]))
        prepared_at = now()
        admin = APIClient()
        admin.force_authenticate(user=User.objects.get(username="akadmin"))
        for session in list(AuthenticatedSession.objects.filter(user=alice)):
            response = admin.delete(f"/api/v3/core/authenticated_sessions/{session.uuid}/")
            assert response.status_code == 204
        assert not AuthenticatedSession.objects.filter(user=alice).exists()
        provider.authentication_flow = Flow.objects.get(slug="n01e-passkey-login")
        provider.save(update_fields=["authentication_flow"])
        # Test-only, server-owned, single-use evidence marker. Nothing here is a
        # production recovery signal or a client-controlled authentication claim.
        with transaction.atomic():
            locked = User.objects.select_for_update().get(pk=alice.pk)
            locked.attributes["n01e_pending_proof"] = {"attempt": attempt, "prepared_at": prepared_at.isoformat()}
            locked.save(update_fields=["attributes"])
        result.update({"prepared": True, "userSubject": str(alice.uuid), "devices": snapshot,
                       "attempt": attempt, "preparedAt": prepared_at.isoformat(), "browserSessionsAfterReset": 0})
    else:
        result = policy()
        before = json.loads(Path("/lab/passkey-before.json").read_text())
        candidate = json.loads(Path("/lab/passkey-candidate.json").read_text())
        attempt = str(uuid.UUID(os.environ["N01E_ATTEMPT_ID"]))
        assert before["prepared"] is True and before["userSubject"] == str(alice.uuid)
        assert before["attempt"] == attempt
        if "n01e_pending_proof" not in alice.attributes:
            raise ConsumedProof()
        prepared_at = parse_datetime(before["preparedAt"])
        assert 0 <= (now() - prepared_at).total_seconds() < 600
        assert provider.authentication_flow.slug == "n01e-passkey-login"
        devices = list(WebAuthnDevice.objects.filter(user=alice, rp_id="localhost"))
        assert {str(device.pk) for device in devices} == {item["pk"] for item in before["devices"]}
        prior = {item["pk"]: parse_datetime(item["last_t"]) for item in before["devices"]}
        advanced = {str(device.pk) for device in devices if device.last_t > prior[str(device.pk)] and device.last_t >= prepared_at}
        assert advanced
        fingerprint = candidate["accessFingerprint"]
        assert len(fingerprint) == 64 and all(character in "0123456789abcdef" for character in fingerprint)
        grants = [grant for grant in AccessToken.objects.filter(provider=provider, user=alice)
                  if hmac.compare_digest(hashlib.sha256(grant.token.encode()).hexdigest(), fingerprint)]
        assert len(grants) == 1
        grant = grants[0]
        assert grant.session is not None and grant.session.user_id == alice.pk
        event = get_login_event(grant.session)
        assert event is not None and event.created >= prepared_at and grant.auth_time == event.created
        assert event.context["auth_method"] in {"auth_webauthn_pwl", "auth_mfa"}
        arguments = event.context["auth_method_args"]
        references = list(arguments.get("mfa_devices", []))
        if arguments.get("device"):
            references.append(arguments["device"])
        assert any(isinstance(reference, dict) and str(reference.get("pk")) in advanced
                   and reference.get("app") == "authentik_stages_authenticator_webauthn" for reference in references)
        with transaction.atomic():
            locked = User.objects.select_for_update().get(pk=alice.pk)
            marker = locked.attributes.pop("n01e_pending_proof", None)
            if marker is None:
                raise ConsumedProof()
            assert marker["attempt"] == attempt and parse_datetime(marker["prepared_at"]) == prepared_at
            locked.save(update_fields=["attributes"])
        result.update({"deviceUseAdvanced": True, "newAuthenticatedSession": True, "grantSessionDeviceBound": True,
                       "singleUseProofConsumed": True, "registeredDevices": len(devices), "sameSubject": True})
    print("N01E_RESULT " + json.dumps(result))
except ConsumedProof:
    print("N01E_CONSUMED_PROOF_REJECTED")
    raise SystemExit(2)
except Exception as error:
    location = next((frame.lineno for frame in reversed(traceback.extract_tb(error.__traceback__))
                     if frame.filename == "/lab-code/passkey-fixture.py"), 0)
    print("N01E_FAILED " + phase + " " + type(error).__name__ + " line=" + str(location))
    raise SystemExit(1)
