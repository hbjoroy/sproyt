"""N01d administrative stimuli inside the isolated lab; never a client integration."""
import json
import os
from pathlib import Path

from django.conf import settings
from django.test import Client
from rest_framework.test import APIClient

from authentik.core.models import Application, AuthenticatedSession, User
from authentik.root.middleware import SessionMiddleware

phase = "guard"
try:
    configuration = json.loads(Path("/lab/bootstrap.json").read_text())
    assert configuration["environment"] == "sproyt-n01c-local"
    application = Application.objects.get(slug="n01-lab")
    assert application.name == "Sprøyt N01c local laboratory"
    assert application.get_provider().client_secret == configuration["client_secret"]
    alice = User.objects.get(username="alice", name="N01c Alice", email="alice@example.invalid")
    bob = User.objects.get(username="bob", name="N01c Bob", email="bob@example.invalid")
    action = os.environ["N01D_ACTION"]
    assert action in {"prepare", "disable-alice", "restore-alice", "delete-alice-one"}
    admin = APIClient()
    admin.force_authenticate(user=User.objects.get(username="akadmin"))
    phase = action
    if action in {"prepare", "restore-alice", "disable-alice"}:
        response = admin.patch(f"/api/v3/core/users/{alice.pk}/",
                               {"is_active": action != "disable-alice"}, format="json")
        assert response.status_code == 200
        alice.refresh_from_db()
        assert alice.is_active == (action != "disable-alice")
    result = {"action": action, "applied": True}
    if action == "prepare":
        sessions = {}
        for slot, user in (("alice-one", alice), ("alice-two", alice), ("bob", bob)):
            client = Client()
            client.force_login(user)
            key = client.session.session_key
            cookie = SessionMiddleware.encode_session(key, user)
            client.cookies[settings.SESSION_COOKIE_NAME] = cookie
            assert client.get("/api/v3/core/users/me/").status_code == 200
            authenticated = AuthenticatedSession.objects.get(session__session_key=key, user=user)
            sessions[slot] = {"cookie": cookie, "session": str(authenticated.uuid)}
        result.update({"cookie_name": settings.SESSION_COOKIE_NAME, "sessions": sessions})
    elif action == "delete-alice-one":
        fixture = json.loads(Path("/lab/revocation-sessions.json").read_text())
        identifier = fixture["sessions"]["alice-one"]["session"]
        assert AuthenticatedSession.objects.filter(uuid=identifier, user=alice).exists()
        response = admin.delete(f"/api/v3/core/authenticated_sessions/{identifier}/")
        assert response.status_code == 204
        assert not AuthenticatedSession.objects.filter(uuid=identifier).exists()
    print("N01D_FIXTURE " + json.dumps(result))
except Exception as error:
    # No response bodies, cookies, tokens, credentials or session handles in logs.
    print("N01D_FIXTURE_FAILED " + phase + " " + type(error).__name__)
    raise SystemExit(1)
