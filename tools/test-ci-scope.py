"""Regression checks for omissions that could weaken release validation."""
import importlib.util
from pathlib import Path
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('ci_scope', Path(__file__).with_name('ci-scope.py'))
ci_scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_scope)


class ScopeTests(unittest.TestCase):
    def test_literal_change_uses_related_ui_tests_without_restore(self):
        result = ci_scope.scope(['src/imagegen.rs', 'src/imagegen_prompt.rs', 'src/web/imagegen.rs',
                                 'frontend/src/imagegen.ts', 'frontend/src/ui/react/preview-imagegen.tsx',
                                 'frontend/tests/boundaries.test.ts', 'docs/image-generation.md'])
        self.assertEqual((result['browser'], result['recovery']), ('imagegen', 'false'))

    def test_backend_feature_has_no_browser_or_restore(self):
        for paths in (['src/imagegen.rs'], ['src/chatbot/weather_followup.rs']):
            self.assertEqual(ci_scope.scope(paths)['browser'], 'none')
            self.assertEqual(ci_scope.scope(paths)['recovery'], 'false')

    def test_shared_ui_and_http_contracts_get_complete_browser_suite(self):
        for path in ('frontend/src/app.ts', 'frontend/src/ui/react/development-preview.tsx',
                     'src/server.rs', 'src/web/auth.rs', 'crates/sproyt-protocol/src/lib.rs',
                     'assets/index.html', 'frontend/tests/ui-react-mobile.spec.ts'):
            self.assertEqual(ci_scope.scope([path])['browser'], 'full', path)

    def test_database_migrations_and_runtime_dependencies_keep_restore(self):
        for path in ('migrations/0058_next.sql', 'src/db/postgres.rs', 'src/domain/repository.rs',
                     'src/main.rs', 'Cargo.lock', 'crates/sproyt-client-core/Cargo.toml',
                     'Dockerfile', 'helm/sproyt/templates/job.yaml', '.github/workflows/other.yml'):
            self.assertEqual(ci_scope.scope([path])['recovery'], 'true', path)

    def test_frontend_changes_do_not_need_database_restore(self):
        self.assertEqual(ci_scope.scope(['frontend/src/app.ts'])['recovery'], 'false')

    def test_unknown_files_and_full_regression_fail_closed(self):
        self.assertEqual(ci_scope.scope(['new-runtime.cfg'])['browser'], 'full')
        self.assertEqual(ci_scope.scope([], True)['recovery'], 'true')

    def test_docs_have_no_expensive_checks(self):
        self.assertEqual(ci_scope.scope(['docs/running.md'])['browser'], 'none')

    def test_policy_only_uses_selector_checks_but_mixed_code_keeps_full_gate(self):
        paths = ['.github/workflows/ci.yml', 'tools/ci-scope.py', 'tools/test-ci-scope.py']
        self.assertEqual(ci_scope.scope(paths)['browser'], 'none')
        self.assertEqual(ci_scope.scope(paths)['recovery'], 'false')
        self.assertEqual(ci_scope.scope(paths + ['src/imagegen.rs'])['browser'], 'full')
        self.assertEqual(ci_scope.scope(paths, True)['recovery'], 'true')

    def test_missing_invalid_and_tag_baselines_select_full(self):
        for event, ref, base in (('workflow_dispatch', 'refs/heads/main', ''),
                                 ('workflow_dispatch', 'refs/heads/main', '--bad'),
                                 ('schedule', 'refs/heads/main', 'a' * 40),
                                 ('push', 'refs/tags/v1', 'a' * 40)):
            self.assertEqual(ci_scope.change_range(event, {}, ref, base, False), ([], True))

    def test_non_ancestor_release_baseline_selects_full(self):
        with patch.object(ci_scope.subprocess, 'run') as run:
            run.return_value.returncode = 1
            self.assertEqual(ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, False), ([], True))

    def test_pull_request_and_push_use_the_event_base(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            diff.return_value = b'src/imagegen.rs\0'
            for event, payload in (('pull_request', {'pull_request': {'base': {'sha': 'b' * 40}}}),
                                   ('push', {'before': 'b' * 40})):
                self.assertEqual(ci_scope.change_range(event, payload, '', '', False), (['src/imagegen.rs'], False))
                self.assertIn('b' * 40, diff.call_args.args[0])

    def test_complete_range_includes_deletions_and_renamed_originals(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            diff.return_value = b'migrations/0057_old.sql\0src/imagegen.rs\0'
            paths, full = ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, False)
            self.assertFalse(full)
            self.assertEqual(ci_scope.scope(paths)['recovery'], 'true')
            self.assertIn('--no-renames', diff.call_args.args[0])


if __name__ == '__main__':
    unittest.main()
