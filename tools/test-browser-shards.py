"""Check selection, isolation and publication gates before using CI output."""
import importlib.util
from pathlib import Path
import re
import subprocess
import sys
import unittest

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parent.parent


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'tools' / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runner = load('run-browser-shard')
selector = load('ci-scope')


class BrowserShardTests(unittest.TestCase):
    def test_every_matrix_entry_has_a_valid_exact_selection(self):
        for profile in ('none', 'layout', 'imagegen', 'related', 'full'):
            for row in selector.browser_matrix(profile)['include']:
                args = runner.command(row['project'], row['shard'], row['files'], row['all'])
                self.assertIn('--project=' + row['project'], args)
                for file, pattern in zip(row['files'], args[4:]):
                    self.assertRegex('tests/' + file, pattern)
                    self.assertRegex('tests\\' + file, pattern)
                    self.assertIsNone(re.search(pattern, 'tests/prefix-' + file))

    def test_empty_related_missing_spec_and_injected_arguments_fail(self):
        for files in ([], ['missing.spec.ts'], ['../smoke.spec.ts'], ['--pass-with-no-tests'], 'smoke.spec.ts'):
            with self.assertRaises(ValueError):
                runner.command('chromium', '1/1', files)
        for project, shard in (('other', '1/1'), ('chromium', '0/4'), ('chromium', '4/2'), ('chromium', '1/1; echo bad')):
            with self.assertRaises(ValueError):
                runner.command(project, shard, ['smoke.spec.ts'])
        with self.assertRaises(ValueError):
            runner.command('webkit-iphone', '1/1', [], True)

    def test_workflow_uses_isolated_jobs_and_fails_closed_at_publication(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        browser = workflow.split('  browser:\n', 1)[1].split('  browser-gate:\n', 1)[0]
        self.assertIn('needs: [changes, quality]', browser)
        self.assertIn('fromJSON(needs.changes.outputs.browser_matrix)', browser)
        self.assertIn('SPROYT_E2E_SERVER_BINARY:', browser)
        self.assertNotIn('cargo build', browser)
        self.assertNotIn('run test:e2e', browser)
        gate = workflow.split('  browser-gate:\n', 1)[1].split('  postgres-schema:\n', 1)[0]
        shell = gate.split('        run: |\n', 1)[1]
        script = '\n'.join(line[10:] for line in shell.splitlines() if line.startswith('          '))
        # Exercise the actual aggregate gate script, including failure/cancel cases.
        if sys.platform != 'win32':
            import os
            for selection, result, expected in [('none', 'skipped', 0), ('full', 'success', 0),
                    ('full', 'failure', 1), ('full', 'cancelled', 1), ('full', 'skipped', 1),
                    ('none', 'failure', 1), ('', 'skipped', 1)]:
                env = {**os.environ, 'CHANGES': 'success', 'QUALITY': 'success', 'SELECTION': selection, 'BROWSERS': result}
                code = subprocess.run(['bash', '-e', '-c', script], env=env).returncode
                self.assertEqual(code == 0, expected == 0, (selection, result))
            for prerequisite in ('CHANGES', 'QUALITY'):
                for failed in ('failure', 'cancelled', 'skipped'):
                    env = {**os.environ, 'CHANGES': 'success', 'QUALITY': 'success', 'SELECTION': 'full', 'BROWSERS': 'success', prerequisite: failed}
                    self.assertNotEqual(subprocess.run(['bash', '-e', '-c', script], env=env).returncode, 0)
        publish = workflow.split('  publish-image:\n', 1)[1].split('    runs-on:', 1)[0]
        for condition in ("needs.quality.result == 'success'", "needs.browser-gate.result == 'success'",
                          "needs.delivery-arm64.result == 'success'", "needs.security-audit.result == 'success'",
                          "needs.changes.outputs.backend == 'false' && needs.postgres-schema.result == 'skipped'",
                          "needs.changes.outputs.recovery == 'false' && needs.recovery-drill.result == 'skipped'"):
            self.assertIn(condition, publish)
        self.assertIn('      - browser-gate\n', publish)
        self.assertNotIn('continue-on-error', browser)
        self.assertNotIn('branches: [main]', workflow.split('permissions:', 1)[0])
        self.assertIn('needs: [quality, browser-gate]', workflow.split('  publish-heart-image:\n', 1)[1])


if __name__ == '__main__':
    unittest.main()
