#!/usr/bin/env python3
"""Run one reviewed CI matrix entry without shell interpolation or implicit all-tests."""
import json
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def command(project, shard, files, all_tests=False):
    if project not in ('chromium', 'webkit-iphone'):
        raise ValueError('unsupported browser project')
    match = re.fullmatch(r'([1-4])/([1-4])', shard)
    if not match or int(match[1]) > int(match[2]):
        raise ValueError('invalid shard')
    if not isinstance(files, list) or any(not isinstance(f, str) for f in files):
        raise ValueError('files must be a list of spec names')
    if all_tests and (project != 'chromium' or files):
        raise ValueError('only full Chromium has an implicit all-tests selection')
    if not all_tests and not files:
        raise ValueError('an empty related suite must not silently run all tests')
    filters = []
    for file in files:
        if not re.fullmatch(r'[a-z0-9.-]+\.spec\.ts', file) or not (ROOT / 'frontend/tests' / file).is_file():
            raise ValueError(f'invalid or missing spec: {file}')
        filters.append(r'(^|[/\\])' + re.escape(file) + '$')
    return ['node', 'frontend/scripts/test-e2e.mjs', '--project=' + project,
            '--shard=' + shard, *filters]


if __name__ == '__main__':
    args = command(os.environ['CI_BROWSER_PROJECT'], os.environ['CI_BROWSER_SHARD'],
                   json.loads(os.environ['CI_BROWSER_FILES']), os.environ.get('CI_BROWSER_ALL') == 'true')
    print('Running', ' '.join(args), flush=True)
    raise SystemExit(subprocess.call(args, cwd=ROOT))
