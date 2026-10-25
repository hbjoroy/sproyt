"""Pre-authenticated synthetic browser sessions; NOT password/passkey acceptance."""
import json
from django.conf import settings
from django.test import Client
from authentik.core.models import Application, User
from authentik.root.middleware import SessionMiddleware

try:
    app = Application.objects.get(slug="n01-lab")
    assert app.name == "Sprøyt N01c local laboratory"
    cookies = {}
    for account in ("alice", "bob"):
        user = User.objects.get(username=account)
        assert user.name == f"N01c {account.title()}" and user.email.endswith("@example.invalid")
        client = Client()
        client.force_login(user)
        # Authentik wraps Django's session key in its own signed browser cookie.
        # force_login alone emits a raw Django key, which is not an Authentik cookie.
        cookies[account] = SessionMiddleware.encode_session(client.session.session_key, user)
    print("N01C_SESSIONS " + json.dumps({"cookie_name": settings.SESSION_COOKIE_NAME, "cookies": cookies}))
except Exception as error:
    print("N01C_SESSION_FIXTURE_FAILED " + type(error).__name__)
    raise SystemExit(1)
