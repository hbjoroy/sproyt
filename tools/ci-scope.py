#!/usr/bin/env python3
"""Choose browser/recovery checks from a complete, known change range."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess


def scope(paths, force_full=False):
    paths = [p for p in paths if not (p.endswith('.md') or p.startswith('docs/'))]
    if force_full:
        return {'browser': 'full', 'recovery': 'true', 'reason': 'full regression requested or baseline unavailable'}
    if not paths:
        return {'browser': 'none', 'recovery': 'false', 'reason': 'no application changes'}
    policy = {'.github/workflows/ci.yml', 'tools/ci-scope.py', 'tools/test-ci-scope.py'}
    if set(paths) <= policy:
        return {'browser': 'none', 'recovery': 'false', 'reason': 'CI policy only; selector regression checks apply'}

    # Unclassified build/configuration changes retain the complete gate.
    known = ('src/', 'frontend/', 'assets/', 'crates/', 'migrations/', 'packages/sproyt-ui/')
    if any((not p.startswith(known) and p not in policy) or p.endswith(('Cargo.toml', 'Cargo.lock')) for p in paths):
        return {'browser': 'full', 'recovery': 'true', 'reason': 'build, dependencies, configuration or unclassified changes'}

    recovery = any(p.startswith(('migrations/', 'src/db', 'src/domain/', 'src/operations/'))
                   or p == 'src/main.rs' for p in paths)
    frontend = [p for p in paths if p.startswith(('frontend/', 'packages/sproyt-ui/'))]
    image_frontend = {
        'frontend/src/imagegen.ts', 'frontend/src/ui/react/preview-imagegen.tsx',
        'frontend/tests/boundaries.test.ts', 'frontend/tests/ui-react-media.spec.ts',
        'frontend/tests/ui-react-preview.spec.ts',
    }
    image_only = all(p in image_frontend or re.fullmatch(r'frontend/tests/(?:ui-react-)?imagegen[^/]*\.spec\.ts', p)
                     for p in frontend)
    shared_contract = any(
        p == 'src/server.rs' or p.startswith(('src/domain/', 'crates/'))
        or (p.startswith('src/web/') and p != 'src/web/imagegen.rs')
        or (p.startswith('assets/') and p.endswith(('.html', '.js', '.css')))
        for p in paths)
    browser = 'full' if shared_contract or (frontend and not image_only) or set(paths) & policy else 'imagegen' if frontend else 'none'
    return {'browser': browser, 'recovery': str(recovery).lower(),
            'reason': 'checks selected from changed application areas'}


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
            for key in ('browser', 'recovery'):
                output.write(f'{key}={selected[key]}\n')
    if args.summary:
        with open(args.summary, 'a', encoding='utf-8') as summary:
            summary.write(f"### Selected release checks\n\nBrowser: **{selected['browser']}**; "
                          f"backup/restore: **{selected['recovery']}**. {selected['reason']}.\n")


if __name__ == '__main__':
    main()
