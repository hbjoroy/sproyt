#!/usr/bin/env python3
"""Issue one reviewed, snapshot-bound grant inside the Authentik Django shell.

Dry run is the default. This helper neither activates a user nor sends mail.
An operator grant is necessary for historical, unmarked interrupted signups;
ordinary inactive accounts must never acquire recovery rights automatically.
"""
import argparse
import hashlib
import json
from secrets import token_urlsafe

MARKER = "sproyt_pending_signup_v1"
SALT = "sproyt.pending-signup.v1"
PRODUCTION = "645a51d1-1d8a-5369-867d-b2a93ac7504e"
SIGNUP_PATH = "/api/v3/flows/executor/sproyt-invitation-production/"
RECOVERY_PATH = "/api/v3/flows/executor/sproyt-account-recovery/"


def snapshot(user):
    from authentik.stages.authenticator import devices_for_user

    return {"pk": user.pk, "uuid": str(user.uuid),
            "email": str(user.email).strip().casefold(),
            "updated": user.last_updated.isoformat(),
            "password": hashlib.sha256(user.password.encode()).hexdigest(),
            "groups": sorted(map(str, user.groups.values_list("pk", flat=True))),
            "roles": sorted(map(str, user.roles.values_list("pk", flat=True))),
            "sources": sorted(map(str, user.sources.values_list("pk", flat=True))),
            "mfa": sorted(str(d._meta.label_lower) + ":" + str(d.pk) for d in devices_for_user(user)),
            "superuser": bool(user.is_superuser), "active": bool(user.is_active),
            "type": str(user.type), "path": str(user.path),
            "last_login": user.last_login.isoformat() if user.last_login else None}


def fingerprint(state):
    return hashlib.sha256(json.dumps(state, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def event_path(event):
    return (event.context or {}).get("http_request", {}).get("path")


def check_evidence(user, creation, model_created, password_set, changes):
    """Require the exact historical production creation and failed reset trail."""
    c = creation.context or {}
    model = (model_created.context or {}).get("model", {})
    if (creation.action != "user_write" or c.get("created") is not True
        or str(c.get("email", "")).strip().casefold() != user.email.strip().casefold()
        or c.get("username") != user.username or event_path(creation) != SIGNUP_PATH
        or model_created.action != "model_created"
        or str(model.get("pk")) != str(user.pk)
        or model.get("app") != "authentik_core" or model.get("model_name") != "user"
        or event_path(model_created) != SIGNUP_PATH
        or password_set.action != "password_set"
        or str((password_set.user or {}).get("pk")) != str(user.pk)
        or event_path(password_set) != RECOVERY_PATH
        or password_set.created < creation.created
        or abs((model_created.created - creation.created).total_seconds()) > 60):
        raise ValueError("Audit evidence does not establish this interrupted production signup")
    for event in changes:
        # Normal administrative writes, including disable/re-enable actions,
        # disqualify this helper. They require a separate operator investigation.
        if event.action in {"model_updated", "model_deleted", "password_set", "user_write"}:
            if event_path(event) not in {SIGNUP_PATH, RECOVERY_PATH}:
                raise ValueError("Unreviewed account change after creation: " + str(event.pk))


def main(argv=None):
    from authentik.core.models import Group, User
    from authentik.events.models import Event
    from django.core import signing
    from django.db import transaction
    from django.db.models import Q

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--username", required=True)
    parser.add_argument("--expected-email", required=True)
    parser.add_argument("--expected-uuid", required=True)
    parser.add_argument("--creation-event", required=True)
    parser.add_argument("--model-created-event", required=True)
    parser.add_argument("--password-set-event", required=True)
    parser.add_argument("--reason", required=True)
    parser.add_argument("--expected-snapshot-sha256")
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args(argv)
    if args.apply and not args.expected_snapshot_sha256:
        parser.error("--apply requires --expected-snapshot-sha256 from a reviewed dry run")
    if len(args.reason.strip()) < 12:
        parser.error("Provide a specific operator review reason")
    with transaction.atomic():
        user = User.objects.select_for_update().get(username=args.username)
        group = Group.objects.get(name="sproyt-users")
        if (str(user.uuid) != args.expected_uuid
            or user.email.strip().casefold() != args.expected_email.strip().casefold()
            or user.is_active or user.is_superuser or user.last_login is not None
            or user.type != "external" or user.path != "users/sproyt"
            or list(user.groups.values_list("pk", flat=True)) != [group.pk]
            or user.roles.exists() or user.sources.exists()
            or MARKER in (user.attributes or {})):
            raise ValueError("Account identity/state is ineligible for this one-account grant")
        creation = Event.objects.get(pk=args.creation_event)
        model_created = Event.objects.get(pk=args.model_created_event)
        password_set = Event.objects.get(pk=args.password_set_event)
        changes = Event.objects.filter(
            Q(user__pk=user.pk) | Q(user__pk=str(user.pk))
            | Q(context__model__pk=user.pk) | Q(context__model__pk=str(user.pk))
            | Q(context__username=user.username),
            created__gte=min(creation.created, model_created.created),
        ).order_by("created")
        check_evidence(user, creation, model_created, password_set, changes)
        state = snapshot(user)
        digest = fingerprint(state)
        if args.expected_snapshot_sha256 and digest != args.expected_snapshot_sha256:
            raise ValueError("Account snapshot changed after operator review")
        report = {"applied": args.apply, "username": user.username,
                  "snapshot": state, "snapshot_sha256": digest,
                  "creation_event": args.creation_event, "model_created_event": args.model_created_event,
                  "password_set_event": args.password_set_event,
                  "reason": args.reason, "grant_lifetime_seconds": 172800}
        if args.apply:
            grant = {"v": 1, "kind": "operator", "flow": PRODUCTION,
                     "nonce": token_urlsafe(32), "snapshot": state}
            user.attributes = dict(user.attributes or {}, **{MARKER: signing.dumps(grant, salt=SALT)})
            user.save(update_fields=["attributes"])
        print(json.dumps(report, sort_keys=True, indent=2))
        return report


if __name__ == "__main__":
    main()
