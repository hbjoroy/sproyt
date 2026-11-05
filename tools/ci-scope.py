#!/usr/bin/env python3
"""Choose CI checks from a complete, known change range."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parent.parent
SUITES = json.loads((ROOT / 'tools/browser-suites.json').read_text(encoding='utf-8'))
POLICY = {'.github/workflows/ci.yml', 'tools/ci-scope.py', 'tools/test-ci-scope.py',
          'tools/browser-suites.json', 'tools/run-browser-shard.py',
          'tools/test-browser-shards.py', 'tools/summarize-browser-tests.py'}
EXECUTION = {'frontend/playwright.config.ts', 'frontend/tests/global-setup.ts',
             'frontend/scripts/test-e2e.mjs'}
IDENTITY = {'deploy/authentik/kustomization.yaml',
            'deploy/authentik/sproyt-email-recovery.yaml',
            'deploy/authentik/sproyt-invitation-canary.yaml',
            'deploy/authentik/sproyt-invitation-canary-v2.yaml',
            'deploy/authentik/sproyt-invitation-production.yaml',
            'deploy/authentik/z-sproyt-signup-recovery-v2.yaml',
            'tools/test-enrollment-continuation.py',
            'tools/test-invitation-canary.py', 'tools/test-invitation-production.py',
            'tools/test-invitation-canary-v2.py', 'tools/requirements-authentik-tests.txt',
            'tools/test-signup-recovery.py', 'tools/issue-signup-recovery-grant.py',
            'tools/verify-signup-recovery-runtime.py'}
OIDC_TOOLS = {'tools/verify-oidc-provider.sh', 'tools/test-oidc-provider-verifier.sh',
              'tools/fixtures/authentik-discovery.json'}
OPERATIONS_TOOLS = {'tools/verify-production-rollout.sh', 'tools/test-production-rollout-verifier.sh',
                    'tools/mcp-load.mjs', 'tools/test-mcp-load.mjs'}
CODEC_TOOLS = {'tools/verify-media-runtime.sh'}
RECOVERY_TOOLS = {'tools/seed-media-recovery.sql', 'tools/verify-media-recovery.sql',
                  'tools/reset-agent-memory-after-restore.sql'}
BACKEND_PREFIXES = ('src/', 'crates/', 'migrations/')
BACKEND_FILES = {
    'src/agent.rs', 'src/auth.rs', 'src/chat.rs', 'src/chatbot.rs', 'src/config.rs', 'src/enrollment.rs',
    'src/github.rs', 'src/imagegen.rs', 'src/imagegen_prompt.rs', 'src/integration.rs', 'src/main.rs',
    'src/media_store.rs', 'src/media_worker.rs', 'src/native_sessions.rs', 'src/notification.rs',
    'src/operations.rs', 'src/process.rs', 'src/process_pilot.rs', 'src/protocol.rs', 'src/server.rs',
    'src/work_items.rs', 'src/ws.rs',
}
BACKEND_DIRECTORIES = (
    'src/auth/', 'src/chatbot/', 'src/db/', 'src/domain/', 'src/imagegen/', 'src/main_tests/',
    'src/media_store/', 'src/native_sessions/', 'src/operations/', 'src/web/', 'src/work_items/',
    'crates/sproyt-client-core/', 'crates/sproyt-protocol/', 'migrations/',
)
# Prompt interpretation does not own persistence. Keep repository coverage for its callers.
RUST_ONLY = {'src/imagegen_prompt.rs'}
BOOTSTRAP = {'src/config.rs', 'src/main.rs', 'src/server.rs'}
AUTH_SOURCES = BOOTSTRAP | {
    'src/agent.rs', 'src/auth.rs', 'src/chatbot/channel_access.rs', 'src/domain/onboarding.rs',
    'src/domain/policy.rs', 'src/enrollment.rs', 'src/native_sessions.rs', 'src/web/account.rs',
    'src/web/agents.rs', 'src/web/auth.rs', 'src/web/browser.rs', 'src/web/enrollment.rs',
    'src/web/http.rs', 'src/web/mcp.rs', 'src/web/native.rs', 'src/web/native_chat.rs',
    'src/web/socket.rs', 'src/web/stream.rs', 'src/ws.rs',
}
AUTH_PREFIXES = ('src/auth/', 'src/native_sessions/', 'src/web/native/')
CODEC_SOURCES = BOOTSTRAP | {
    'src/chatbot/agent_images.rs', 'src/chatbot/vision.rs', 'src/media_store.rs', 'src/media_worker.rs',
    'src/web/media.rs', 'src/web/media_uploads.rs', 'src/web/native/media_uploads.rs',
}
OPERATIONS_SOURCES = BOOTSTRAP | {
    'src/auth.rs', 'src/main_tests/mcp.rs', 'src/operations.rs', 'src/web/auth.rs',
    'src/web/browser.rs', 'src/web/mcp.rs', 'src/web/system.rs',
}
CHECK_FLAGS = ('backend', 'postgres', 'recovery', 'codec', 'auth', 'operations')
# Bound only reviewed paths. Shared shells and state retain full regression.
FEATURES = [
    ({'frontend/src/direct-conversation.ts', 'frontend/src/direct-activity.ts',
      'frontend/src/ui/react/direct-message-picker.tsx', 'frontend/src/ui/react/new-direct-message.tsx'},
     {'ui-react-direct-message-picker.spec.ts', 'ui-react-product-navigation.spec.ts',
      'ui-react-profile-favorites.spec.ts', 'ui-react-composer-owner.spec.ts'}),
    ({'frontend/src/ui/react/markdown-content.tsx'},
     {'ui-react-markdown.spec.ts', 'ui-react-media.spec.ts', 'ui-react-layout-refinement.spec.ts'}),
    ({'frontend/src/ui/react/preview-reactions.tsx'},
     {'ui-react-reactions.spec.ts', 'ui-react-badge.spec.ts', 'ui-react-thread-discovery.spec.ts'}),
    ({'frontend/src/agent-place-lookup.ts'}, {'ui-react-place-lookup.spec.ts', 'ui-react-agent-location.spec.ts'}),
    ({'frontend/src/ui/react/device-setup-reminders.tsx', 'frontend/src/ui/react/device-setup-notice.tsx'},
     {'ui-react-device-setup-notice.spec.ts', 'ui-react-device-setup-integration.spec.ts', 'ui-react-notifications.spec.ts'}),
    ({'frontend/src/ui/react/thread-directory.tsx', 'frontend/src/ui/thread-directory.css'},
     {'ui-react-thread-directory.spec.ts', 'ui-react-thread-discovery.spec.ts', 'ui-react-reading.spec.ts'}),
    ({'frontend/src/ui/react/invitation-card.tsx'}, {'ui-react-invitations.spec.ts'}),
    ({'frontend/src/ui/react/work-item-supplements.tsx'},
     {'ui-react-work-item-supplements.spec.ts', 'ui-react-work-item-information.spec.ts'}),
]


def browser_matrix(browser, extra=()):
    if browser == 'none':
        return {'include': []}
    available = {p.name for p in (ROOT / 'frontend/tests').glob('*.spec.ts')}
    required = set().union(*map(set, SUITES.values()))
    if required - available:
        raise ValueError(f'Browser suite references missing tests: {sorted(required - available)}')
    selected = set(SUITES.get(browser, SUITES['smoke'])) | set(extra)
    if selected - available:
        raise ValueError(f'Changed browser tests are missing: {sorted(selected - available)}')
    rows = []
    for project, engine in [('chromium', 'chromium'), ('webkit-iphone', 'webkit')]:
        all_tests = browser == 'full' and engine == 'chromium'
        files = [] if all_tests else sorted(set(SUITES['webkit']) if browser == 'full'
            else selected if engine == 'chromium' else selected & set(SUITES['webkit']))
        if not all_tests and not files:
            continue
        # Isolate databases per job instead of racing several workers on one DB.
        count = (4 if engine == 'chromium' else 2) if browser == 'full' else (2 if len(files) >= 5 else 1)
        for index in range(1, count + 1):
            rows.append({'project': project, 'engine': engine, 'shard': f'{index}/{count}',
                         'index': index, 'all': all_tests, 'files': files})
    return {'include': rows}


def result(browser, reason, extra=(), *, frontend=True, **checks):
    return {'browser': browser, 'frontend': str(frontend).lower(),
            **{key: str(checks.get(key, False)).lower() for key in CHECK_FLAGS}, 'reason': reason,
            'browser_matrix': browser_matrix(browser, extra)}


def full_checks(reason):
    return result('full', reason, **{key: True for key in CHECK_FLAGS if key != 'recovery'})


def scope(paths, force_full=False):
    selected = application_scope(paths, force_full)
    # Recovery has its own risk decision, including when application scope is broad.
    selected['recovery'] = str(bool(set(paths) & RECOVERY_TOOLS)).lower()
    return selected


def application_scope(paths, force_full=False):
    paths = {p for p in paths if not (p.endswith('.md') or p.startswith(('docs/', '.agents/skills/sproyt-ui/')))}
    if force_full:
        return full_checks('full regression requested, backend path changed structurally or baseline unavailable')
    if not paths:
        return result('none', 'documentation/skill only', frontend=False)
    if paths <= POLICY:
        return result('none', 'CI policy only; selector and workflow contracts apply', frontend=False)
    standalone_tools = IDENTITY | OIDC_TOOLS | CODEC_TOOLS | OPERATIONS_TOOLS
    auth_tools = bool(paths & (IDENTITY | OIDC_TOOLS))
    operations_tools = bool(paths & (OPERATIONS_TOOLS | OIDC_TOOLS))
    codec_tools = bool(paths & CODEC_TOOLS)
    if paths <= POLICY | standalone_tools:
        return result('none', 'standalone provider, codec or operational contracts', frontend=False,
                      auth=auth_tools, codec=codec_tools, operations=operations_tools)
    if paths & IDENTITY:
        return full_checks('identity provider changes mixed with application or other runtime changes')

    def native_probe(path):
        return path.startswith(('native/probes/', 'native/session-client/')) or path == '.github/workflows/native-probe.yml'
    selector_tests = {'tools/ci-scope.py', 'tools/test-ci-scope.py'}
    if any(native_probe(p) for p in paths) and all(native_probe(p) or p in selector_tests for p in paths):
        return result('none', 'dedicated native interoperability workflow', frontend=False)

    known = BACKEND_PREFIXES + ('frontend/', 'assets/', 'packages/sproyt-ui/')
    if any((not p.startswith(known) and p not in POLICY | standalone_tools | RECOVERY_TOOLS)
           or p.endswith(('Cargo.toml', 'Cargo.lock')) for p in paths):
        return full_checks('build, dependencies, configuration or unclassified changes')

    backend_paths = {p for p in paths if p.startswith(BACKEND_PREFIXES)}
    if any(not p.startswith(BACKEND_DIRECTORIES) and p not in BACKEND_FILES for p in backend_paths):
        return full_checks('unclassified backend source')

    application_auth = bool(paths & AUTH_SOURCES or any(p.startswith(AUTH_PREFIXES) for p in paths))
    checks = {
        'backend': bool(backend_paths),
        'postgres': bool(backend_paths - RUST_ONLY),
        'codec': codec_tools or bool(paths & CODEC_SOURCES) or any(p.startswith('src/media_store/') for p in paths),
        'auth': auth_tools or application_auth,
        'operations': operations_tools or bool(paths & OPERATIONS_SOURCES) or any(p.startswith('src/operations/') for p in paths),
    }
    if application_auth:
        return result('full', 'application identity, session or access contract', **checks)
    shared = any(p in {'src/chat.rs', 'src/protocol.rs'} or p.startswith(('src/domain/', 'crates/'))
                 or (p.startswith('src/web/') and p != 'src/web/imagegen.rs')
                 or p.startswith('assets/') for p in paths)
    if shared or paths & (POLICY | EXECUTION):
        return result('full', 'shared contract or mixed CI policy', **checks)

    frontend = [p for p in paths if p.startswith(('frontend/', 'packages/sproyt-ui/'))]
    if not frontend:
        return result('none', 'selected backend or recovery contracts', frontend=bool(backend_paths), **checks)
    changed_specs = {p for p in frontend if re.fullmatch(r'frontend/tests/[^/]+\.spec\.ts', p)}
    changed_tests = {Path(p).name for p in changed_specs}
    if any(not (ROOT / p).is_file() for p in frontend):
        return result('full', 'removed frontend contract', **checks)
    runtime = {p for p in frontend if not p.endswith('.test.ts')} - changed_specs
    if not runtime:
        return result('related' if changed_tests else 'none',
                      'changed browser contracts plus core smoke', changed_tests, **checks)
    covered, related = set(), set(changed_tests)
    for sources, tests in FEATURES:
        if runtime & sources:
            covered |= sources
            related |= tests
    layout = {p for p in runtime - covered
              if p.endswith('.css') and p.startswith(('frontend/src/ui/', 'packages/sproyt-ui/src/'))}
    if runtime == layout:
        return result('layout', 'CSS layout and cross-browser interaction contracts', changed_tests, **checks)
    imagegen = runtime & {'frontend/src/imagegen.ts', 'frontend/src/ui/react/preview-imagegen.tsx'}
    if runtime == imagegen:
        return result('imagegen', 'imagegen, media drafts and composer contracts', changed_tests, **checks)
    if runtime <= covered | layout | imagegen:
        if layout:
            related.update(SUITES['layout'])
        if imagegen:
            related.update(SUITES['imagegen'])
        return result('related', 'bounded feature contracts plus core smoke', sorted(related), **checks)
    return result('full', 'shared or unclassified frontend logic', **checks)


def event_base(event_name, event, manual_base):
    if event_name == 'pull_request':
        return event.get('pull_request', {}).get('base', {}).get('sha', '')
    if event_name == 'push':
        return event.get('before', '')
    return manual_base if event_name == 'workflow_dispatch' else ''


def change_range(event_name, event, ref, manual_base, force_full):
    base = event_base(event_name, event, manual_base)
    if not re.fullmatch(r'[0-9a-f]{40}', base) or base == '0' * 40:
        return [], True
    try:
        # A release baseline must be an ancestor, covering every change since it.
        if subprocess.run(['git', 'merge-base', '--is-ancestor', base, 'HEAD'],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            return [], True
        diff = subprocess.check_output(
            ['git', 'diff', '--name-status', '--no-renames', '-z', base, 'HEAD'])
        fields = diff.decode('utf-8').split('\0')
        if fields.pop() or len(fields) % 2:
            return [], True
        entries = list(zip(fields[::2], fields[1::2]))
        if any(status not in {'A', 'D', 'M', 'T'} or not path for status, path in entries):
            return [], True
        paths = [path for _, path in entries]
        structural_backend_change = any(status != 'M' and path.startswith(BACKEND_PREFIXES) and not path.endswith('.md')
                                        for status, path in entries)
        return paths, force_full or ref.startswith('refs/tags/') or structural_backend_change
    except (OSError, subprocess.CalledProcessError, UnicodeError):
        return [], True


def recovery_reason(paths, event_name, force_recovery=False, base=''):
    if force_recovery:
        return 'explicit recovery-risk rehearsal requested'
    if event_name == 'schedule':
        return 'weekly recovery rehearsal'
    if set(paths) & RECOVERY_TOOLS:
        return 'backup/restore fixtures or post-restore cleanup changed'
    for path in paths:
        if path.startswith('migrations/') and path.endswith('.sql'):
            migration = ROOT / path
            if not migration.is_file():
                return 'migration removed; recovery compatibility must be checked'
            if re.search(r'^\s*--\s*ci-recovery:\s*required\b',
                         migration.read_text(encoding='utf-8'), re.MULTILINE):
                return 'migration declares a recovery risk'
    if '.github/workflows/ci.yml' in paths:
        # Only changes to the drill itself invalidate its evidence, not every CI edit.
        pattern = r'^  recovery-drill:\n.*?(?=^  [a-zA-Z][\w-]*:|\Z)'
        current = re.search(pattern, (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8'),
                            re.MULTILINE | re.DOTALL)
        try:
            if not re.fullmatch(r'[0-9a-f]{40}', base):
                raise ValueError('no comparison revision for changed recovery workflow')
            previous = subprocess.check_output(['git', 'show', f'{base}:.github/workflows/ci.yml'],
                                               text=True, encoding='utf-8')
            before = re.search(pattern, previous, re.MULTILINE | re.DOTALL)
            if current is None or before is None or current.group() != before.group():
                return 'backup/restore job changed'
        except (OSError, subprocess.CalledProcessError, UnicodeError, ValueError):
            return 'changed recovery workflow could not be compared'
    return ''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--github-output')
    parser.add_argument('--summary')
    args = parser.parse_args()
    event_path = os.environ.get('GITHUB_EVENT_PATH')
    event = json.loads(Path(event_path).read_text(encoding='utf-8')) if event_path else {}
    event_name = os.environ.get('GITHUB_EVENT_NAME', '')
    paths, full = change_range(event_name, event,
                              os.environ.get('GITHUB_REF', ''), os.environ.get('CI_BASE_REVISION', ''),
                              os.environ.get('CI_FORCE_FULL', '').lower() == 'true')
    selected = scope(paths, full)
    recovery = recovery_reason(paths, event_name,
                               os.environ.get('CI_FORCE_RECOVERY', '').lower() == 'true',
                               event_base(event_name, event, os.environ.get('CI_BASE_REVISION', '')))
    selected['recovery'] = str(bool(recovery)).lower()
    selected['recovery_reason'] = recovery or 'no recovery trigger; review risk outside automatic path selection'
    print(json.dumps({**selected, 'changed_files': len(paths)}))
    if args.github_output:
        with open(args.github_output, 'a', encoding='utf-8') as output:
            for key in ('browser', 'frontend', *CHECK_FLAGS):
                output.write(f'{key}={selected[key]}\n')
            output.write('browser_matrix=' + json.dumps(selected['browser_matrix'], separators=(',', ':')) + '\n')
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as summary:
            summary.write(f"### Selected release checks\n\nBrowser: **{selected['browser']}**; "
                          f"Rust: **{selected['backend']}**; PostgreSQL: **{selected['postgres']}**; "
                          f"backup/restore: **{selected['recovery']}**; codec: **{selected['codec']}**; "
                          f"auth: **{selected['auth']}**; operations: **{selected['operations']}**. "
                          f"{selected['reason']}. Browser jobs: **{len(selected['browser_matrix']['include'])}**.\n\n"
                          f"Recovery decision: {selected['recovery_reason']}.\n")


if __name__ == '__main__':
    main()
