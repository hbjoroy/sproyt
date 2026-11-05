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
                          "needs.delivery-arm64.result == 'success'", "needs.security-audit.result == 'success'"):
            self.assertIn(condition, publish)
        self.assertIn('      - browser-gate\n', publish)
        self.assertNotIn('continue-on-error', browser)
        self.assertNotIn('branches: [main]', workflow.split('permissions:', 1)[0])
        self.assertIn('needs: [quality, browser-gate]', workflow.split('  publish-heart-image:\n', 1)[1])


    def test_database_job_publication_guards_require_success_or_explicit_scope_skip(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        publish = workflow.split('  publish-image:\n', 1)[1].split('    runs-on:', 1)[0]
        condition, dependencies = publish.split('    needs:\n', 1)
        condition = ' '.join(condition.split())
        for job, flag in (('postgres-schema', 'postgres'), ('recovery-drill', 'recovery')):
            with self.subTest(job=job):
                self.assertIn(f'{flag}: ${{{{ steps.scope.outputs.{flag} }}}}', workflow)
                self.assertIn(f'      - {job}\n', dependencies)
                # Read the workflow's accepted results and skip flag from its actual gate.
                guard = re.search(rf"&& \(needs\.{job}\.result == '(\w+)' \|\| "
                                  rf"\(needs\.changes\.outputs\.{flag} == '(\w+)' && "
                                  rf"needs\.{job}\.result == '(\w+)'\)\)", condition)
                self.assertIsNotNone(guard)
                required_result, skip_flag, skip_result = guard.groups()
                for scope, outcome, permits in (
                        ('true', 'success', True), ('true', 'skipped', False),
                        ('true', 'failure', False), ('true', 'cancelled', False),
                        ('false', 'skipped', True), ('false', 'failure', False),
                        ('false', 'cancelled', False), ('', 'skipped', False),
                        ('unknown', 'skipped', False)):
                    with self.subTest(scope=scope, outcome=outcome):
                        allowed = outcome == required_result or (scope == skip_flag and outcome == skip_result)
                        self.assertEqual(allowed, permits)
                definition = workflow.split(f'  {job}:\n', 1)[1].split('    runs-on:', 1)[0]
                self.assertIn(f"if: needs.changes.outputs.{flag} == 'true'", definition)

    def test_quality_runs_whole_rust_and_independently_gates_specialized_contracts(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text(encoding='utf-8')
        quality = workflow.split('  quality:\n', 1)[1].split('  browser:\n', 1)[0]
        for name, flag, command in (
                ('Check formatting', 'backend', 'cargo fmt --all --check'),
                ('Lint all targets and features', 'backend', 'cargo clippy --all-targets --all-features -- -D warnings'),
                ('Run Rust tests', 'backend', 'cargo test --all-targets --all-features --locked'),
                ('Verify isolated codec runtime and large-image preview', 'codec', 'bash tools/verify-media-runtime.sh'),
                ('Validate OIDC verifier contract', 'auth', 'bash tools/test-oidc-provider-verifier.sh'),
                ('Validate operational contracts', 'operations', 'node tools/test-mcp-load.mjs')):
            with self.subTest(name=name):
                step = quality.split(f'      - name: {name}\n', 1)[1].split('      - name:', 1)[0]
                self.assertIn(f"if: needs.changes.outputs.{flag} == 'true'\n", step)
                self.assertIn(command + '\n', step)
        node = quality.split('      - name: Install pinned Node.js\n', 1)[1].split('      - name:', 1)[0]
        self.assertIn("if: needs.changes.outputs.frontend == 'true' || needs.changes.outputs.operations == 'true'", node)


if __name__ == '__main__':
    unittest.main()
