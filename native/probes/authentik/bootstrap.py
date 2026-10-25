"""Executed only inside the isolated Authentik worker; never against production."""
import json
from pathlib import Path
from django.db import transaction

phase = "imports"


def configure():
    global phase
    from authentik.core.models import Application, Group, User
    from authentik.crypto.models import CertificateKeyPair
    from authentik.flows.models import Flow
    from authentik.policies.models import PolicyBinding
    from authentik.providers.oauth2.models import OAuth2Provider, ScopeMapping

    # HTTP readiness can precede worker-managed default blueprints on cold CI.
    required_flows = {"default-authentication-flow", "default-provider-authorization-implicit-consent", "default-provider-invalidation-flow"}
    required_scopes = {"openid", "profile", "email", "offline_access"}
    if (set(Flow.objects.filter(slug__in=required_flows).values_list("slug", flat=True)) != required_flows
        or not CertificateKeyPair.objects.filter(key_data__isnull=False).exclude(key_data="").exists()
        or not set(ScopeMapping.objects.filter(scope_name__in=required_scopes).values_list("scope_name", flat=True)) >= required_scopes):
        print("N01C_NOT_READY")
        raise SystemExit(2)
    data = json.loads(Path("/lab/bootstrap.json").read_text())
    assert data["environment"] == "sproyt-n01c-local"
    assert data["redirect"] == "http://127.0.0.1:19191/probe/callback"
    existing = Application.objects.filter(slug="n01-lab").first()
    if existing is not None:
        provider = OAuth2Provider.objects.get(pk=existing.provider_id)
        assert existing.name == "Sprøyt N01c local laboratory"
        assert provider.client_id == "n01-lab" and provider.client_secret == data["client_secret"]
        return summary(provider, {account: str(User.objects.get(username=account).uuid) for account in ("alice", "bob")})
    phase = "users"
    group = Group.objects.create(name="N01c synthetic users")
    subjects = {}
    for account in ("alice", "bob"):
        user = User.objects.create(username=account, name=f"N01c {account.title()}",
            email=f"{account}@example.invalid", is_active=True,
            attributes={"email_verified": True})
        user.set_password(data["test_password"])
        user.save()
        user.ak_groups.add(group)
        subjects[account] = str(user.uuid)
    phase = "provider"
    key = CertificateKeyPair.objects.filter(key_data__isnull=False).exclude(key_data="").first()
    assert key is not None
    provider = OAuth2Provider.objects.create(name="N01c local lab", client_id="n01-lab",
        client_secret=data["client_secret"], client_type="confidential",
        authentication_flow=Flow.objects.get(slug="default-authentication-flow"),
        authorization_flow=Flow.objects.get(slug="default-provider-authorization-implicit-consent"),
        invalidation_flow=Flow.objects.get(slug="default-provider-invalidation-flow"),
        grant_types=["authorization_code", "refresh_token"],
        _redirect_uris=[{"matching_mode": "strict", "url": data["redirect"], "redirect_uri_type": "authorization"}],
        signing_key=key, sub_mode="user_uuid", issuer_mode="per_provider",
        access_code_validity="minutes=1", access_token_validity="minutes=5",
        refresh_token_validity="hours=1", refresh_token_threshold="seconds=0")
    phase = "scopes"
    mappings = ScopeMapping.objects.filter(scope_name__in=["openid", "profile", "email", "offline_access"])
    assert set(mappings.values_list("scope_name", flat=True)) >= {"openid", "profile", "email", "offline_access"}
    provider.property_mappings.set(mappings)
    phase = "application"
    application = Application.objects.create(name="Sprøyt N01c local laboratory", slug="n01-lab", provider=provider)
    PolicyBinding.objects.create(target=application, group=group, order=0)
    return summary(provider, subjects)


def summary(provider, subjects):
    return {"configured": True, "subjects": subjects, "sub_mode": provider.sub_mode,
        "access_token_validity": provider.access_token_validity,
        "refresh_token_validity": provider.refresh_token_validity,
        "refresh_token_threshold": provider.refresh_token_threshold,
        "grants": provider.grant_types}


try:
    with transaction.atomic():
        configured = configure()
    print("N01C_CONFIGURED " + json.dumps(configured))
except Exception as error:
    # Django errors can include request/model values. Keep secrets out of logs.
    print("N01C_CONFIGURATION_FAILED " + phase + " " + type(error).__name__)
    raise SystemExit(1)
