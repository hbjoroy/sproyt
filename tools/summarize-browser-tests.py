#!/usr/bin/env python3
"""Retain test counts and the slowest contracts, including successful runs."""
import json
import os
from pathlib import Path
import sys


def cases(suites):
    for suite in suites:
        yield from cases(suite.get('suites', []))
        for spec in suite.get('specs', []):
            for test in spec.get('tests', []):
                yield (sum(r.get('duration', 0) for r in test.get('results', [])),
                       spec.get('file', ''), spec['title'], test.get('status', 'unknown'))


if __name__ == '__main__':
    report = Path(sys.argv[1])
    if not report.is_file():
        print('No browser report: setup or runner failed before reporting.')
        raise SystemExit(0)  # Never masks the failed test step.
    data = json.loads(report.read_text(encoding='utf-8'))
    rows = sorted(cases(data.get('suites', [])), reverse=True)
    stats = data.get('stats', {})
    lines = [f"### Browser timing — {os.environ.get('CI_BROWSER_PROJECT', '')} {os.environ.get('CI_BROWSER_SHARD', '')}", '',
             f"Passed: {stats.get('expected', 0)}; failed: {stats.get('unexpected', 0)}; skipped: {stats.get('skipped', 0)}.", '',
             '| Seconds | Contract | Status |', '| ---: | --- | --- |']
    for duration, file, title, status in rows[:10]:
        # Test labels are untrusted display data, never workflow commands.
        label = f'{file}: {title}'.replace('|', '/').replace('\n', ' ').replace('<', '&lt;')
        lines.append(f'| {duration / 1000:.1f} | {label} | {status} |')
    text = '\n'.join(lines) + '\n'
    if os.environ.get('GITHUB_STEP_SUMMARY'):
        with open(os.environ['GITHUB_STEP_SUMMARY'], 'a', encoding='utf-8') as output:
            output.write(text)
    else:
        print(text)
