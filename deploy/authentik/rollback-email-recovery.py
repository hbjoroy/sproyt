"""Rollback for the 2026-09-15 Sprøyt email recovery activation.

Run only after disabling blueprint instantiation. This intentionally leaves
the now-unlinked Sprøyt flow and stage objects in place; deleting them is not
required to restore user-facing behaviour and would make rollback riskier.
"""

from django.db import transaction

from authentik.brands.models import Brand
from authentik.core.models import User
from authentik.flows.models import FlowStageBinding, FlowToken
from authentik.policies.models import PolicyBinding
from authentik.providers.oauth2.models import OAuth2Provider, ScopeMapping
from authentik.stages.identification.models import IdentificationStage


MARKER = "email_verified_migrated_2026_09_15"
BACKUP = "email_verified_migration_previous_2026_09_15"


with transaction.atomic():
    identification = IdentificationStage.objects.get(
        name="sproyt-authentication-identification"
    )
    identification.user_fields = ["username"]
    identification.recovery_flow = None
    identification.save(update_fields=["user_fields", "recovery_flow"])

    brand = Brand.objects.get(domain="sproyt-security.bjoroy.me")
    brand.flow_user_settings = None
    brand.save(update_fields=["flow_user_settings"])

    provider = OAuth2Provider.objects.get(name="sproyt-provider")
    mapping_names = [
        "authentik default OAuth Mapping: OpenID 'openid'",
        "authentik default OAuth Mapping: OpenID 'profile'",
        "authentik default OAuth Mapping: OpenID 'email'",
        "authentik default OAuth Mapping: OpenID 'offline_access'",
    ]
    provider.property_mappings.set(ScopeMapping.objects.filter(name__in=mapping_names))

    policy_names = [
        "sproyt-default-settings-invalidate-changed-email",
        "sproyt-invalid-invitation-context",
        "sproyt-mark-invitation-email-verified",
    ]
    PolicyBinding.objects.filter(policy__name__in=policy_names).delete()
    FlowStageBinding.objects.filter(
        target__slug="sproyt-invitation-enrollment",
        stage__name="sproyt-invitation-context-deny",
    ).delete()

    restored = 0
    for user in User.objects.filter(
        type__in=["internal", "external"],
        attributes__email_verified_migrated_2026_09_15=True,
    ).select_for_update():
        attributes = dict(user.attributes or {})
        previous = attributes.get(BACKUP, {})
        if previous.get("email_verified_present"):
            attributes["email_verified"] = previous.get("email_verified")
        else:
            attributes.pop("email_verified", None)
        if previous.get("email_verified_address_present"):
            attributes["email_verified_address"] = previous.get(
                "email_verified_address"
            )
        else:
            attributes.pop("email_verified_address", None)
        attributes.pop(MARKER, None)
        attributes.pop(BACKUP, None)
        user.attributes = attributes
        user.save(update_fields=["attributes"])
        restored += 1

    token_count, _ = FlowToken.objects.filter(
        flow__slug__in=["sproyt-account-recovery", "sproyt-user-settings"]
    ).delete()

print("RESTORED_USERS", restored)
print("EXPIRED_FLOW_TOKEN_OBJECTS", token_count)
