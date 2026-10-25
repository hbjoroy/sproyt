"""Bounded N01f SMTP/link contract; never fabricates a WebAuthn assertion."""
import html
import json
import re
import time
import traceback
import signal
import threading
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from unittest.mock import patch
from datetime import timedelta
from pathlib import Path
from urllib.parse import parse_qs, urlencode, urlsplit

import requests
from django.core.cache import cache
from django.db import DatabaseError, close_old_connections
from django.test import Client
from django.utils.timezone import now

from authentik.core.models import Application, Token, User
from authentik.flows.models import FlowToken
from authentik.policies.models import PolicyBinding
from authentik.policies.expression.models import ExpressionPolicy
from authentik.policies.process import PolicyProcess
from authentik.stages.authenticator_webauthn.models import WebAuthnDevice
from authentik.flows.views.executor import QS_KEY_TOKEN, QS_QUERY
from authentik.stages.email.stage import EMAIL_RECOVERY_CACHE_KEY

phase = "guard"
observed_component = "none"
observed_status = 0
HOST = "localhost:19090"
BASELINE = "n01f-email-baseline"
GUARDED = "n01f-recovery"
MAILPIT = "http://mailpit:8025"
http = requests.Session()
http.trust_env = False


def require(condition):
    if not condition:
        raise AssertionError("N01f contract assertion failed")


def mail_json(path):
    # URLs are fixed to the owned Mailpit. No redirects, proxies, or unbounded bodies.
    require(path.startswith("/api/v1/"))
    with http.get(MAILPIT + path, timeout=5, allow_redirects=False, stream=True) as response:
        require(response.status_code == 200)
        chunks, length = [], 0
        for chunk in response.iter_content(8192):
            length += len(chunk)
            require(length <= 262144)
            chunks.append(chunk)
        return json.loads(b"".join(chunks))


def mail_ids():
    messages = mail_json("/api/v1/messages?limit=100")
    require(messages["total"] <= 100)
    return {message["ID"] for message in messages["messages"]}


def wait_mail(before):
    for _ in range(40):
        added = mail_ids() - before
        if added:
            require(len(added) == 1)
            identity = added.pop()
            require(re.fullmatch(r"[A-Za-z0-9-]{1,80}", identity) is not None)
            return mail_json("/api/v1/message/" + identity)
        time.sleep(0.25)
    raise AssertionError("N01f SMTP delivery timeout")


def no_mail(before):
    for _ in range(8):
        require(mail_ids() == before)
        time.sleep(0.25)


def body(response):
    require(len(response.content) <= 131072)
    if response.status_code != 200:
        return {}
    require(response.headers.get("Content-Type", "").startswith("application/json"))
    return response.json()


def component(response):
    global observed_component, observed_status
    value = body(response).get("component")
    observed_component = value if value in {"ak-stage-identification", "ak-stage-email", "ak-stage-consent", "ak-stage-authenticator-webauthn", "ak-stage-access-denied"} else "other"
    observed_status = response.status_code
    return value


def route(slug, query=None):
    result = "/api/v3/flows/executor/" + slug + "/"
    return result if query is None else result + "?" + urlencode({QS_QUERY: query})


def settle(client, response):
    for _ in range(8):
        if response.status_code not in (302, 303):
            return response
        target = urlsplit(response.headers.get("Location", ""))
        require(target.scheme in ("", "http") and target.netloc in ("", HOST))
        if target.path not in (route(BASELINE), route(GUARDED)):
            return response
        require(len(target.query) <= 8192)
        response = client.get(target.path + ("?" + target.query if target.query else ""), HTTP_HOST=HOST)
    raise AssertionError("N01f executor redirect limit")


def post(client, path, data):
    return settle(client, client.post(path, data=json.dumps(data), content_type="application/json", HTTP_HOST=HOST))

def reset_send_limits(slug):
    # Never clear the cache globally or touch unrelated EmailStage rate keys.
    for user in (alice, bob):
        cache.delete("n01f:" + slug + ":send:" + str(user.uuid))


def begin(slug, username, injected=False):
    client = Client()
    require(component(settle(client, client.get(route(slug), HTTP_HOST=HOST))) == "ak-stage-identification")
    data = {"component": "ak-stage-identification", "uid_field": username}
    if injected:
        data["email"] = "injected@example.invalid"
    response = post(client, route(slug), data)
    require(component(response) == "ak-stage-email")
    return client


def send(slug=GUARDED, injected=False):
    before = mail_ids()
    original = begin(slug, "alice", injected)
    message = wait_mail(before)
    recipients = message["To"]
    require(len(recipients) == 1 and recipients[0]["Address"] == alice.email)
    text = html.unescape(message.get("Text", "") + " " + message.get("HTML", ""))
    links = []
    for candidate in re.findall(r"https?://[^\s<>\"']+", text):
        parsed = urlsplit(candidate)
        if parsed.scheme == "http" and parsed.netloc == HOST and parsed.path == "/if/flow/" + slug + "/" and QS_KEY_TOKEN in parse_qs(parsed.query):
            links.append(parsed)
    require(links)
    queries = {link.query for link in links}
    require(len(queries) == 1)
    query = queries.pop()
    token_values = parse_qs(query)[QS_KEY_TOKEN]
    require(len(token_values) == 1)
    return original, route(slug, query), token_values[0]


def open_consent(client, path):
    response = settle(client, client.get(path, HTTP_HOST=HOST))
    if component(response) != "ak-stage-consent":
        return False
    nonce = body(response).get("token")
    require(isinstance(nonce, str) and 0 < len(nonce) <= 128)
    if not hasattr(client, "_n01f_consent_nonces"):
        client._n01f_consent_nonces = {}
    client._n01f_consent_nonces[path] = nonce
    return True


def consent(client, path):
    # Ordinary server-issued consent form nonce; retained only in this client RAM.
    nonce = client._n01f_consent_nonces[path]
    response = post(client, path, {"component": "ak-stage-consent", "token": nonce})
    component(response)
    challenge = body(response)
    return challenge.get("component") == "ak-stage-authenticator-webauthn" and not challenge.get("response_errors")

def denied_link(path, client=None):
    client = client or Client()
    if not open_consent(client, path):
        return True
    return not consent(client, path)


class ContractDeadline(BaseException):
    """Not intercepted by the policy engine's broad Exception handler."""


@contextmanager
def deadline(seconds):
    def expired(_signal, _frame):
        raise ContractDeadline()
    previous = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, seconds)
    try:
        yield
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


@contextmanager
def admission_expression(expression, timeout=None):
    policy = ExpressionPolicy.objects.get(name=GUARDED + "-admission")
    binding = PolicyBinding.objects.get(policy=policy)
    original_expression, original_timeout = policy.expression, binding.timeout
    try:
        ExpressionPolicy.objects.filter(pk=policy.pk).update(expression=expression)
        if timeout is not None:
            PolicyBinding.objects.filter(pk=binding.pk).update(timeout=timeout)
        yield
    finally:
        ExpressionPolicy.objects.filter(pk=policy.pk).update(expression=original_expression)
        PolicyBinding.objects.filter(pk=binding.pk).update(timeout=original_timeout)


def fault_link():
    reset_send_limits(GUARDED)
    _, path, _ = send()
    client = Client()
    require(open_consent(client, path))
    return client, path


def fault_observations():
    global phase
    observations = {}
    devices_before = WebAuthnDevice.objects.filter(user=alice).count()
    phase = "admission-evaluator-failure"
    client, path = fault_link()
    with admission_expression("raise RuntimeError('N01f evaluator fault')"):
        require(not consent(client, path))
        require(observed_component == "ak-stage-access-denied")
    observations["evaluatorFailureDenied"] = True

    phase = "admission-claim-store-failure"
    client, path = fault_link()
    manager = Token.objects
    original_get = manager.get_or_create
    def failing_store(*args, **kwargs):
        if kwargs.get("identifier", "").startswith("n01f-admission-"):
            raise DatabaseError("N01f claim store fault")
        return original_get(*args, **kwargs)
    with patch.object(manager, "get_or_create", side_effect=failing_store):
        require(not consent(client, path))
        require(observed_component == "ak-stage-access-denied")
    observations["claimStoreFailureDenied"] = True

    phase = "admission-real-timeout-observation"
    client, path = fault_link()
    started = time.monotonic()
    with deadline(10), admission_expression("import time\ntime.sleep(2)\nreturn True", timeout=1):
        timeout_setup = consent(client, path)
    elapsed = time.monotonic() - started
    # Pinned-version measurement: a synchronous task.run does not enforce timeout.
    require(timeout_setup and 2 <= elapsed < 10)
    observations["policyTimeoutEnforced"] = False
    observations["timeoutAllowsSetup"] = True

    phase = "admission-outer-process-failure"
    client, path = fault_link()
    original_wrapper = PolicyProcess.profiling_wrapper
    def failing_wrapper(process):
        if process.binding.policy and process.binding.policy.name == GUARDED + "-admission":
            raise RuntimeError("N01f outer process fault")
        return original_wrapper(process)
    with patch.object(PolicyProcess, "profiling_wrapper", failing_wrapper):
        outer_setup = consent(client, path)
    # The outer catch returns False without binding negate/failure-result, so
    # the Deny stage is skipped. This is an observed security blocker, not a pass.
    require(outer_setup)
    observations["outerPolicyFailureAllowsSetup"] = True
    observations["recoveryGateApproved"] = False

    phase = "concurrent-real-claim-insert"
    reset_send_limits(GUARDED)
    _, path, _ = send()
    first, second = Client(), Client()
    require(open_consent(first, path) and open_consent(second, path))
    barrier = threading.Barrier(2, timeout=5)
    hits = []
    def synchronized_store(*args, **kwargs):
        if kwargs.get("identifier", "").startswith("n01f-admission-"):
            hits.append(True)
            barrier.wait()
        return original_get(*args, **kwargs)
    def submit(client):
        close_old_connections()
        try:
            return consent(client, path)
        finally:
            close_old_connections()
    with patch.object(manager, "get_or_create", side_effect=synchronized_store):
        with ThreadPoolExecutor(max_workers=2) as pool:
            pending = [pool.submit(submit, client) for client in (first, second)]
            admissions = sum(int(future.result(timeout=10)) for future in pending)
    require(len(hits) == 2 and admissions == 1)
    observations["concurrentRealClaimAdmissions"] = admissions
    require(WebAuthnDevice.objects.filter(user=alice).count() == devices_before)
    observations["faultTestsCreatedNoDevices"] = True
    return observations

try:
    configuration = json.loads(Path("/lab/bootstrap.json").read_text())
    require(configuration["environment"] == "sproyt-n01c-local" and configuration["recovery_lab"] is True and configuration["passkey_lab"] is True)
    application = Application.objects.get(slug="n01-lab", name="Sprøyt N01c local laboratory")
    require(application.get_provider().client_secret == configuration["client_secret"])
    alice = User.objects.get(username="alice", name="N01c Alice", email="alice@example.invalid", is_active=True)
    bob = User.objects.get(username="bob", name="N01c Bob", email="bob@example.invalid")
    results = {}

    phase = "verified-recipient-and-consent"
    reset_send_limits(GUARDED)
    _, path, _ = send(injected=True)
    client = Client()
    require(open_consent(client, path) and consent(client, path))
    results.update(verifiedRecipientOnly=True, emailConsentReachesWebAuthnWithoutOldMfa=True)
    phase = "used-link"
    require(denied_link(path))
    results["usedLinkDenied"] = True

    phase = "uniform-no-mail"
    reset_send_limits(GUARDED)
    for username in ("n01f-unknown-identifier", "bob"):
        before = mail_ids()
        begin(GUARDED, username)
        no_mail(before)
    alice.is_active = False
    alice.save(update_fields=["is_active"])
    try:
        before = mail_ids()
        begin(GUARDED, "alice")
        no_mail(before)
    finally:
        alice.is_active = True
        alice.save(update_fields=["is_active"])
    results["unknownUnverifiedDisabledUniformWithoutSmtp"] = True

    phase = "changed-email"
    reset_send_limits(GUARDED)
    _, path, _ = send()
    original_email = alice.email
    alice.email = "n01f-changed@example.invalid"
    alice.save(update_fields=["email"])
    try:
        require(denied_link(path))
    finally:
        alice.email = original_email
        alice.save(update_fields=["email"])
    results["changedEmailDenied"] = True

    phase = "expired-link"
    reset_send_limits(GUARDED)
    _, path, token_key = send()
    require(FlowToken.objects.filter(key=token_key, flow__slug=GUARDED).update(expires=now() - timedelta(seconds=1)) == 1)
    require(denied_link(path))
    results["expiredLinkDenied"] = True

    phase = "wrong-pending-user"
    reset_send_limits(GUARDED)
    _, path, _ = send()
    before = mail_ids()
    wrong = begin(GUARDED, "bob")
    no_mail(before)
    require(denied_link(path, wrong))
    results["wrongPendingUserDenied"] = True

    phase = "forked-consent-race"
    for slug, expected in ((BASELINE, 2), (GUARDED, 1)):
        reset_send_limits(slug)
        _, path, _ = send(slug)
        first, second = Client(), Client()
        # Both restore the very same token before either consent POST.
        require(open_consent(first, path) and open_consent(second, path))
        admitted = int(consent(first, path)) + int(consent(second, path))
        require(admitted == expected)
        results["baselineForkAdmissions" if slug == BASELINE else "guardedForkAdmissions"] = admitted

    phase = "same-browser-consent-race"
    reset_send_limits(GUARDED)
    _, path, _ = send()
    same = Client()
    require(open_consent(same, path) and open_consent(same, path))
    admitted = int(consent(same, path)) + int(consent(same, path))
    require(admitted <= 1)
    results["sameBrowserAtMostOneAdmission"] = True

    results.update(fault_observations())

    phase = "bounded-initial-sends-and-resends"
    reset_send_limits(GUARDED)
    # This is the exact synthetic Alice resend key, not a global cache reset.
    # Previous contract runs must not consume this independent test's budget.
    from hashlib import sha256
    cache.delete(EMAIL_RECOVERY_CACHE_KEY + sha256(alice.email.lower().encode()).hexdigest())
    first_client = None
    for _ in range(3):
        before = mail_ids()
        current = begin(GUARDED, "alice")
        first_client = first_client or current
        wait_mail(before)
    before = mail_ids()
    begin(GUARDED, "alice")
    no_mail(before)
    # Built-in EmailStage has a separate three-resend budget. It is deliberately
    # not described as a shared three-message budget with initial sends.
    for _ in range(3):
        before = mail_ids()
        require(component(post(first_client, route(GUARDED), {"component": "ak-stage-email"})) == "ak-stage-email")
        wait_mail(before)
    before = mail_ids()
    post(first_client, route(GUARDED), {"component": "ak-stage-email"})
    no_mail(before)
    results.update(initialSendRateLimit=True, separateResendRateLimit=True, combinedMessageBudgetAtMostSix=True)
    print("N01F_RESULT " + json.dumps({"passed": True, "scope": "n01f_recovery_link_contract", "hardwarePasskeyVerified": False, "fullRecoveryVerified": False, **results}))
except (Exception, ContractDeadline) as error:
    location = next((frame.lineno for frame in reversed(traceback.extract_tb(error.__traceback__)) if frame.filename.endswith("recovery-contract.py") and frame.name != "require"), 0)
    print("N01F_FAILED " + phase + " " + type(error).__name__ + " line=" + str(location) + " component=" + observed_component + " status=" + str(observed_status))
    raise SystemExit(1)
finally:
    http.close()
