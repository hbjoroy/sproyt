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
                     'assets/index.html'):
            self.assertEqual(ci_scope.scope([path])['browser'], 'full', path)

    def test_database_migrations_and_runtime_dependencies_keep_restore(self):
        for path in ('migrations/0058_next.sql', 'src/db/postgres.rs', 'src/domain/repository.rs',
                     'src/main.rs', 'Cargo.lock', 'crates/sproyt-client-core/Cargo.toml',
                     'Dockerfile', 'helm/sproyt/templates/job.yaml', '.github/workflows/other.yml'):
            self.assertEqual(ci_scope.scope([path])['recovery'], 'true', path)

    def test_frontend_changes_do_not_need_database_restore(self):
        self.assertEqual(ci_scope.scope(['frontend/src/app.ts'])['recovery'], 'false')
        self.assertEqual(ci_scope.scope(['frontend/src/app.ts'])['backend'], 'false')

    def test_css_iteration_uses_layout_contracts_on_both_engines(self):
        result = ci_scope.scope(['frontend/src/ui/react-app.css', 'packages/sproyt-ui/src/styles.css'])
        self.assertEqual((result['browser'], result['backend'], result['recovery']), ('layout', 'false', 'false'))
        matrix = result['browser_matrix']['include']
        self.assertEqual({row['engine'] for row in matrix}, {'chromium', 'webkit'})
        for row in matrix:
            self.assertIn('ui-react-direct-message-picker.spec.ts', row['files'])
            self.assertIn('ui-react-channel-popover-navigation.spec.ts', row['files'])
            self.assertFalse(row['all'])

    def test_changed_browser_contract_is_selected_exactly_without_backend(self):
        result = ci_scope.scope(['frontend/tests/sse-fallback.spec.ts'])
        self.assertEqual((result['browser'], result['backend']), ('related', 'false'))
        chromium = next(r for r in result['browser_matrix']['include'] if r['engine'] == 'chromium')
        self.assertIn('sse-fallback.spec.ts', chromium['files'])
        self.assertIn('ui-react-preview.spec.ts', chromium['files'])

    def test_skill_bundle_is_not_a_database_or_application_change(self):
        paths = ['.agents/skills/sproyt-ui/assets/sproyt-ui-0.1.3.tgz', '.agents/skills/sproyt-ui/SKILL.md']
        self.assertEqual(ci_scope.scope(paths)['frontend'], 'false')
        self.assertEqual(ci_scope.scope(paths + ['frontend/src/ui/react-app.css'])['recovery'], 'false')

    def test_execution_changes_exercise_real_shards_without_unrelated_rust_tests(self):
        result = ci_scope.scope(['frontend/tests/global-setup.ts', '.github/workflows/ci.yml'])
        self.assertEqual((result['browser'], result['backend'], result['recovery']), ('full', 'false', 'false'))

    def test_full_regression_keeps_all_engines_and_backend_gates(self):
        result = ci_scope.scope([], True)
        self.assertEqual((result['backend'], result['recovery']), ('true', 'true'))
        rows = result['browser_matrix']['include']
        self.assertEqual([r['shard'] for r in rows if r['engine'] == 'chromium'], ['1/4', '2/4', '3/4', '4/4'])
        self.assertEqual([r['shard'] for r in rows if r['engine'] == 'webkit'], ['1/2', '2/2'])

    def test_css_mixed_with_backend_migration_does_not_skip_backend(self):
        result = ci_scope.scope(['frontend/src/ui/react-app.css', 'migrations/0071_next.sql'])
        self.assertEqual((result['backend'], result['recovery']), ('true', 'true'))

    def test_unit_test_only_does_not_launch_browser(self):
        result = ci_scope.scope(['frontend/tests/boundaries.test.ts', 'frontend/src/direct-activity.test.ts'])
        self.assertEqual((result['browser'], result['frontend']), ('none', 'true'))

    def test_bounded_feature_union_keeps_each_feature_but_unknown_logic_is_full(self):
        paths = ['frontend/src/direct-activity.ts', 'frontend/src/ui/react/preview-reactions.tsx']
        result = ci_scope.scope(paths)
        self.assertEqual(result['browser'], 'related')
        files = set(result['browser_matrix']['include'][0]['files'])
        self.assertTrue({'ui-react-direct-message-picker.spec.ts', 'ui-react-reactions.spec.ts'} <= files)
        self.assertEqual(ci_scope.scope(paths + ['frontend/src/ui/react/development-preview.tsx'])['browser'], 'full')

    def test_canonical_design_system_requires_browser_without_database_restore(self):
        for path in ('packages/sproyt-ui/src/react/index.tsx', 'packages/sproyt-ui/src/vue/index.ts',
                     'packages/sproyt-ui/package.json', 'packages/sproyt-ui/package-lock.json'):
            result = ci_scope.scope([path, '.github/workflows/ci.yml'])
            self.assertEqual((result['browser'], result['recovery']), ('full', 'false'))

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

    def test_codec_test_only_keeps_runtime_gate_without_unrelated_browser_restore(self):
        paths = ['tools/verify-media-runtime.sh', 'tools/ci-scope.py', 'tools/test-ci-scope.py']
        self.assertEqual(ci_scope.scope(paths)['browser'], 'none')
        self.assertEqual(ci_scope.scope(paths)['recovery'], 'false')
        for added in ('Dockerfile', 'src/web/media.rs', 'frontend/src/app.ts', 'migrations/0065_media.sql'):
            self.assertEqual(ci_scope.scope(paths + [added])['browser'], 'full', added)
        self.assertEqual(ci_scope.scope(paths, True)['recovery'], 'true')

    def test_non_ancestor_release_baseline_selects_full(self):
        with patch.object(ci_scope.subprocess, 'run') as run:
            run.return_value.returncode = 1
            self.assertEqual(ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, False), ([], True))

    def test_isolated_native_probe_uses_its_dedicated_workflow(self):
        paths = ['native/probes/client-core/Cargo.toml', 'native/probes/client-core/Cargo.lock',
                 'native/probes/managed/Program.cs', 'native/probes/run-probe.ps1',
                 'native/session-client/NativeAuthSession.cs',
                 '.github/workflows/native-probe.yml', 'tools/ci-scope.py',
                 'tools/test-ci-scope.py', 'docs/native-client-probe-results.md']
        result = ci_scope.scope(paths)
        self.assertEqual((result['browser'], result['recovery']), ('none', 'false'))
        result = ci_scope.scope(['native/session-client/NativeAuthSession.cs'])
        self.assertEqual((result['browser'], result['recovery']), ('none', 'false'))
        for added in ('Cargo.lock', 'crates/sproyt-client-core/src/lib.rs', 'src/db/postgres.rs',
                      'frontend/src/app.ts', 'migrations/0065_media.sql', 'native/apps/android/build.gradle',
                      '.github/workflows/ci.yml', 'native/unknown-runtime.cfg'):
            result = ci_scope.scope(paths + [added])
            self.assertEqual((result['browser'], result['recovery']), ('full', 'true'), added)
        result = ci_scope.scope(paths, True)
        self.assertEqual((result['browser'], result['recovery']), ('full', 'true'))

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
