#!/usr/bin/env python3
"""Choose browser/recovery checks from a complete, known change range."""
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
    ({'frontend/src/ui/react/thread-directory.tsx'},
     {'ui-react-thread-directory.spec.ts', 'ui-react-thread-discovery.spec.ts', 'ui-react-reading.spec.ts'}),
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


def result(browser, recovery, backend, reason, extra=(), frontend=True, codec=False):
    return {'browser': browser, 'recovery': str(recovery).lower(),
            'backend': str(backend).lower(), 'frontend': str(frontend).lower(),
            'codec': str(codec or backend).lower(), 'reason': reason,
            'browser_matrix': browser_matrix(browser, extra)}


def scope(paths, force_full=False):
    paths = [p for p in paths if not (p.endswith('.md') or p.startswith(('docs/', '.agents/skills/sproyt-ui/')))]
    if force_full:
        return result('full', True, True, 'full regression requested or baseline unavailable')
    if not paths:
        return result('none', False, False, 'documentation/skill only', frontend=False)
    if set(paths) <= POLICY:
        return result('none', False, False, 'CI policy only; selector and workflow contracts apply', frontend=False)
    if set(paths) <= POLICY | {'tools/verify-media-runtime.sh'}:
        return result('none', False, False, 'isolated codec contract', frontend=False, codec=True)

    def native_probe(path):
        return path.startswith(('native/probes/', 'native/session-client/')) or path == '.github/workflows/native-probe.yml'
    selector_tests = {'tools/ci-scope.py', 'tools/test-ci-scope.py'}
    if any(native_probe(p) for p in paths) and all(native_probe(p) or p in selector_tests for p in paths):
        return result('none', False, False, 'dedicated native interoperability workflow', frontend=False)

    known = ('src/', 'frontend/', 'assets/', 'crates/', 'migrations/', 'packages/sproyt-ui/')
    if any((not p.startswith(known) and p not in POLICY and p != 'tools/verify-media-runtime.sh')
           or p.endswith(('Cargo.toml', 'Cargo.lock')) for p in paths):
        return result('full', True, True, 'build, dependencies, configuration or unclassified changes')

    backend = any(p.startswith(('src/', 'crates/', 'migrations/')) for p in paths)
    recovery = any(p.startswith(('migrations/', 'src/db', 'src/domain/', 'src/operations/'))
                   or p == 'src/main.rs' for p in paths)
    codec = 'tools/verify-media-runtime.sh' in paths
    shared = any(p == 'src/server.rs' or p.startswith(('src/domain/', 'crates/'))
                 or (p.startswith('src/web/') and p != 'src/web/imagegen.rs')
                 or p.startswith('assets/') for p in paths)
    if shared or set(paths) & (POLICY | EXECUTION):
        return result('full', recovery, backend, 'shared contract or mixed CI policy', codec=codec)

    frontend = [p for p in paths if p.startswith(('frontend/', 'packages/sproyt-ui/'))]
    if not frontend:
        return result('none', recovery, backend, 'backend feature; Rust and PostgreSQL contracts', codec=codec)
    changed_tests = [Path(p).name for p in frontend if p.startswith('frontend/tests/') and p.endswith('.spec.ts')]
    if any(not (ROOT / p).is_file() for p in frontend):
        return result('full', recovery, backend, 'removed frontend contract', codec=codec)
    runtime = [p for p in frontend if not (p.endswith('.test.ts') or (p.startswith('frontend/tests/') and p.endswith('.spec.ts')))]
    if not runtime:
        return result('related' if changed_tests else 'none', recovery, backend,
                      'changed browser contracts plus core smoke', changed_tests, codec=codec)
    if all(p.endswith('.css') and p.startswith(('frontend/src/ui/', 'packages/sproyt-ui/src/')) for p in runtime):
        return result('layout', recovery, backend, 'CSS layout and cross-browser interaction contracts', changed_tests, codec=codec)
    image_frontend = {'frontend/src/imagegen.ts', 'frontend/src/ui/react/preview-imagegen.tsx'}
    if all(p in image_frontend for p in runtime):
        return result('imagegen', recovery, backend, 'imagegen, media drafts and composer contracts', changed_tests, codec=codec)
    covered, related = set(), set(changed_tests)
    for sources, tests in FEATURES:
        if set(runtime) & sources:
            covered |= sources
            related |= tests
    if set(runtime) <= covered:
        return result('related', recovery, backend, 'bounded feature contracts plus core smoke', sorted(related), codec=codec)
    return result('full', recovery, backend, 'shared or unclassified frontend logic', codec=codec)


def change_range(event_name, event, ref, manual_base, force_full):
    if force_full or event_name == 'schedule' or ref.startswith('refs/tags/'):
        return [], True
    if event_name == 'pull_request':
        base = event.get('pull_request', {}).get('base', {}).get('sha', '')
    elif event_name == 'push':
        base = event.get('before', '')
    elif event_name == 'workflow_dispatch':
        base = manual_base
    else:
        return [], True
    if not re.fullmatch(r'[0-9a-f]{40}', base) or base == '0' * 40:
        return [], True
    try:
        # A release baseline must be an ancestor, covering every change since it.
        if subprocess.run(['git', 'merge-base', '--is-ancestor', base, 'HEAD'],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
            return [], True
        paths = subprocess.check_output(
            ['git', 'diff', '--name-only', '--no-renames', '-z', base, 'HEAD'])
        return paths.decode('utf-8').rstrip('\0').split('\0') if paths else [], False
    except (OSError, subprocess.CalledProcessError, UnicodeError):
        return [], True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--github-output')
    parser.add_argument('--summary')
    args = parser.parse_args()
    event_path = os.environ.get('GITHUB_EVENT_PATH')
    event = json.loads(Path(event_path).read_text(encoding='utf-8')) if event_path else {}
    paths, full = change_range(os.environ.get('GITHUB_EVENT_NAME', ''), event,
                              os.environ.get('GITHUB_REF', ''), os.environ.get('CI_BASE_REVISION', ''),
                              os.environ.get('CI_FORCE_FULL', '').lower() == 'true')
    selected = scope(paths, full)
    print(json.dumps({**selected, 'changed_files': len(paths)}))
    if args.github_output:
        with open(args.github_output, 'a', encoding='utf-8') as output:
            for key in ('browser', 'recovery', 'backend', 'frontend', 'codec'):
                output.write(f'{key}={selected[key]}\n')
            output.write('browser_matrix=' + json.dumps(selected['browser_matrix'], separators=(',', ':')) + '\n')
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as summary:
            summary.write(f"### Selected release checks\n\nBrowser: **{selected['browser']}**; "
                          f"Rust/PostgreSQL: **{selected['backend']}**; backup/restore: **{selected['recovery']}**. "
                          f"{selected['reason']}. Browser jobs: **{len(selected['browser_matrix']['include'])}**.\n")


if __name__ == '__main__':
    main()
