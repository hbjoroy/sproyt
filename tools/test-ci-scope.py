"""Regression checks for omissions that could weaken release validation."""
from contextlib import chdir
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('ci_scope', Path(__file__).with_name('ci-scope.py'))
ci_scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_scope)


class ScopeTests(unittest.TestCase):
    def test_identity_provider_changes_run_contracts_without_application_rebuild(self):
        for paths in ([path] for path in ci_scope.IDENTITY):
            result = ci_scope.scope(paths)
            self.assertEqual((result['browser'], result['backend'], result['frontend'], result['recovery']),
                             ('none', 'false', 'false', 'false'), paths)
            self.assertEqual(result['auth'], 'true', paths)
        result = ci_scope.scope(sorted(ci_scope.IDENTITY | ci_scope.POLICY))
        self.assertEqual(result['browser'], 'none')
        self.assertEqual(ci_scope.scope(['deploy/authentik/unknown.yaml'])['browser'], 'full')
        self.assertEqual(ci_scope.scope(['src/auth.rs', *ci_scope.IDENTITY])['backend'], 'true')
        self.assertEqual(ci_scope.scope(list(ci_scope.IDENTITY), True)['browser'], 'full')

    def test_literal_change_uses_related_ui_tests_without_restore(self):
        result = ci_scope.scope(['src/imagegen.rs', 'src/imagegen_prompt.rs', 'src/web/imagegen.rs',
                                 'frontend/src/imagegen.ts', 'frontend/src/ui/react/preview-imagegen.tsx',
                                 'frontend/tests/boundaries.test.ts', 'docs/image-generation.md'])
        self.assertEqual((result['browser'], result['recovery']), ('imagegen', 'false'))

    def test_backend_feature_has_no_browser_or_restore(self):
        for paths in (['src/imagegen.rs'], ['src/chatbot/weather_followup.rs']):
            self.assertEqual(ci_scope.scope(paths)['browser'], 'none')
            self.assertEqual(ci_scope.scope(paths)['recovery'], 'false')

    def test_prompt_interpretation_runs_the_whole_rust_gate_without_unrelated_jobs(self):
        result = ci_scope.scope(['src/imagegen_prompt.rs'])
        self.assertEqual(result['browser'], 'none')
        self.assertEqual(result['backend'], 'true')
        for flag in ('postgres', 'recovery', 'auth', 'operations', 'codec'):
            self.assertEqual(result[flag], 'false', flag)

    def test_backend_feature_retains_postgres_without_codec_or_operational_checks(self):
        result = ci_scope.scope(['src/chatbot/weather_followup.rs'])
        self.assertEqual((result['backend'], result['postgres']), ('true', 'true'))
        self.assertEqual((result['codec'], result['operations']), ('false', 'false'))

    def test_codec_selection_follows_preview_and_media_runtime_dependencies(self):
        for path in ('src/media_store/preview.rs', 'src/media_worker.rs', 'tools/verify-media-runtime.sh'):
            self.assertEqual(ci_scope.scope([path])['codec'], 'true', path)

    def test_standalone_operator_and_codec_contracts_keep_distinct_gates(self):
        operations = ci_scope.scope(['tools/verify-production-rollout.sh'])
        self.assertEqual(operations, ci_scope.scope(['tools/publish-production-release-news.sh']))
        codec = ci_scope.scope(['tools/verify-media-runtime.sh'])
        self.assertEqual((operations['operations'], operations['codec']), ('true', 'false'))
        self.assertEqual((codec['operations'], codec['codec']), ('false', 'true'))
        for result in (operations, codec):
            self.assertEqual(result['browser'], 'none')
            for flag in ('frontend', 'backend', 'postgres', 'recovery'):
                self.assertEqual(result[flag], 'false', flag)
        combined = ci_scope.scope(['tools/mcp-load.mjs', 'tools/verify-media-runtime.sh'])
        self.assertEqual((combined['operations'], combined['codec']), ('true', 'true'))
        oidc = ci_scope.scope(['tools/verify-oidc-provider.sh'])
        self.assertEqual((oidc['auth'], oidc['operations'], oidc['backend'], oidc['postgres']),
                         ('true', 'true', 'false', 'false'))

    def test_release_news_source_runs_publication_and_operational_contracts(self):
        result = ci_scope.scope(['src/release_news.rs'])
        self.assertEqual((result['operations'], result['backend'], result['postgres']),
                         ('true', 'true', 'true'))

    def test_recovery_fixture_change_exercises_restore_without_the_postgres_repository_job(self):
        result = ci_scope.scope(['tools/verify-media-recovery.sql'])
        self.assertEqual((result['recovery'], result['postgres'], result['backend']), ('true', 'false', 'false'))
        self.assertEqual((result['browser'], result['frontend'], result['codec'], result['operations']),
                         ('none', 'false', 'false', 'false'))

    def test_auth_source_and_adapter_keep_browser_and_postgres_integration(self):
        for path in ('src/auth.rs', 'src/web/auth.rs', 'src/native_sessions.rs'):
            with self.subTest(path=path):
                result = ci_scope.scope([path])
                self.assertEqual((result['auth'], result['backend'], result['postgres']), ('true', 'true', 'true'))
                self.assertEqual(result['browser'], 'full')
                self.assertEqual(result['codec'], 'false')

    def test_migration_combines_with_auth_and_codec_without_losing_any_gate(self):
        result = ci_scope.scope(['migrations/postgres/0072_personal_spaces.sql', 'src/auth.rs', 'src/media_store/preview.rs'])
        for flag in ('auth', 'backend', 'postgres', 'codec'):
            self.assertEqual(result[flag], 'true', flag)
        self.assertEqual(result['browser'], 'full')

    def test_dependencies_unknown_paths_and_missing_baseline_keep_application_gates(self):
        for paths, force_full in ((['Cargo.lock'], False), (['new-runtime.cfg'], False),
                                  (['src/unclassified.rs'], False), ([], True)):
            with self.subTest(paths=paths, force_full=force_full):
                result = ci_scope.scope(paths, force_full)
                self.assertEqual(result['browser'], 'full')
                self.assertEqual(result['recovery'], 'false')
                for flag in ('auth', 'backend', 'postgres', 'operations', 'codec'):
                    self.assertEqual(result[flag], 'true', flag)

    def test_shared_ui_and_http_contracts_get_complete_browser_suite(self):
        for path in ('frontend/src/app.ts', 'frontend/src/session.ts', 'frontend/src/connection.ts',
                     'frontend/src/ui/react/development-preview.tsx', 'frontend/src/ui/react/conversation-view.tsx',
                     'src/server.rs', 'src/web/auth.rs', 'crates/sproyt-protocol/src/lib.rs',
                     'assets/index.html'):
            self.assertEqual(ci_scope.scope([path])['browser'], 'full', path)

    def test_database_migrations_and_runtime_dependencies_do_not_imply_restore(self):
        for path in ('migrations/0058_next.sql', 'src/db/postgres.rs', 'src/domain/repository.rs',
                     'src/main.rs', 'Cargo.lock', 'crates/sproyt-client-core/Cargo.toml',
                     'Dockerfile', 'helm/sproyt/templates/job.yaml', '.github/workflows/other.yml'):
            self.assertEqual(ci_scope.scope([path])['recovery'], 'false', path)

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
            self.assertIn('ui-react-mobile.spec.ts', row['files'])
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
        self.assertEqual((result['backend'], result['recovery']), ('true', 'false'))
        rows = result['browser_matrix']['include']
        self.assertEqual([r['shard'] for r in rows if r['engine'] == 'chromium'], ['1/4', '2/4', '3/4', '4/4'])
        self.assertEqual([r['shard'] for r in rows if r['engine'] == 'webkit'], ['1/2', '2/2'])

    def test_css_mixed_with_backend_migration_does_not_skip_backend(self):
        result = ci_scope.scope(['frontend/src/ui/react-app.css', 'migrations/0071_next.sql'])
        self.assertEqual((result['backend'], result['recovery']), ('true', 'false'))

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

    def test_bounded_presentation_selects_component_contracts_on_supported_engines(self):
        smoke = {'ui-react-preview.spec.ts', 'ui-touch-controls.spec.ts'}
        for path, contracts, webkit in (
            ('frontend/src/ui/thread-directory.css',
             {'ui-react-thread-directory.spec.ts', 'ui-react-thread-discovery.spec.ts', 'ui-react-reading.spec.ts'},
             {'ui-react-thread-directory.spec.ts', 'ui-react-thread-discovery.spec.ts', 'ui-react-reading.spec.ts'}),
            ('frontend/src/ui/react/work-item-supplements.tsx',
             {'ui-react-work-item-supplements.spec.ts', 'ui-react-work-item-information.spec.ts'},
             {'ui-react-work-item-supplements.spec.ts', 'ui-react-work-item-information.spec.ts'}),
            ('frontend/src/ui/react/invitation-card.tsx', {'ui-react-invitations.spec.ts'}, set()),
        ):
            with self.subTest(path=path):
                result = ci_scope.scope([path])
                self.assertEqual((result['browser'], result['frontend'], result['backend'], result['recovery']),
                                 ('related', 'true', 'false', 'false'))
                matrix = {row['engine']: set(row['files']) for row in result['browser_matrix']['include']}
                self.assertEqual(matrix, {'chromium': smoke | contracts, 'webkit': {'ui-touch-controls.spec.ts'} | webkit})

    def test_mixed_layout_features_and_changed_specs_preserve_their_union(self):
        paths = ['frontend/src/ui/react-app.css', 'frontend/src/ui/react/invitation-card.tsx',
                 'frontend/src/ui/react/work-item-supplements.tsx', 'frontend/src/imagegen.ts',
                 'frontend/tests/ui-react-settings-split-layout.spec.ts',
                 'frontend/tests/ui-react-saved-emojis.spec.ts']
        result = ci_scope.scope(paths)
        self.assertEqual((result['browser'], result['backend'], result['recovery']), ('related', 'false', 'false'))
        common = {'ui-react-mobile.spec.ts', 'ui-react-work-item-supplements.spec.ts',
                  'ui-react-work-item-information.spec.ts', 'ui-react-imagegen-draft.spec.ts',
                  'ui-react-saved-emojis.spec.ts', 'ui-touch-controls.spec.ts'}
        chromium_only = {'ui-react-invitations.spec.ts', 'ui-react-settings-split-layout.spec.ts',
                         'imagegen.spec.ts', 'ui-react-preview.spec.ts'}
        for row in result['browser_matrix']['include']:
            with self.subTest(engine=row['engine'], shard=row['shard']):
                files = set(row['files'])
                self.assertTrue(common <= files)
                if row['engine'] == 'chromium':
                    self.assertTrue(chromium_only <= files)
                else:
                    self.assertFalse(chromium_only & files)

    def test_removed_or_unknown_ui_keeps_full_browser_coverage(self):
        mapped = 'frontend/src/ui/react/invitation-card.tsx'
        renamed = 'frontend/src/ui/react/work-item-supplements.tsx'
        unknown = 'frontend/src/ui/react/connection-health.tsx'
        changed_test = 'frontend/tests/ui-react-invitations.spec.ts'
        self.assertTrue((ci_scope.ROOT / unknown).is_file())
        self.assertEqual(ci_scope.scope([mapped, unknown])['browser'], 'full')
        self.assertEqual(ci_scope.scope([renamed])['browser'], 'related')
        is_file = Path.is_file
        with patch.object(Path, 'is_file', lambda path: path != ci_scope.ROOT / mapped and is_file(path)):
            for paths in ([mapped], [mapped, changed_test], [mapped, renamed]):
                self.assertEqual(ci_scope.scope(paths)['browser'], 'full')

    def test_nested_browser_spec_change_or_removal_selects_full_chromium(self):
        root_spec = 'frontend/tests/ui-react-invitations.spec.ts'
        nested = 'frontend/tests/specialized/ui-react-invitations.spec.ts'
        self.assertTrue((ci_scope.ROOT / root_spec).is_file())
        is_file = Path.is_file
        for exists in (True, False):
            with self.subTest(exists=exists), patch.object(Path, 'is_file', lambda path:
                    exists if path == ci_scope.ROOT / nested else is_file(path)):
                result = ci_scope.scope([root_spec, nested])
                self.assertEqual(result['browser'], 'full')
                chromium = [row for row in result['browser_matrix']['include'] if row['engine'] == 'chromium']
                self.assertEqual(len(chromium), 4)
                self.assertTrue(all(row['all'] for row in chromium))

    def test_canonical_design_system_requires_browser_without_database_restore(self):
        for path in ('packages/sproyt-ui/src/react/index.tsx', 'packages/sproyt-ui/src/vue/index.ts',
                     'packages/sproyt-ui/package.json', 'packages/sproyt-ui/package-lock.json'):
            result = ci_scope.scope([path, '.github/workflows/ci.yml'])
            self.assertEqual((result['browser'], result['recovery']), ('full', 'false'))

    def test_unknown_files_and_full_regression_fail_closed(self):
        self.assertEqual(ci_scope.scope(['new-runtime.cfg'])['browser'], 'full')
        self.assertEqual(ci_scope.scope([], True)['recovery'], 'false')

    def test_docs_have_no_expensive_checks(self):
        self.assertEqual(ci_scope.scope(['docs/running.md'])['browser'], 'none')

    def test_policy_only_uses_selector_checks_but_mixed_code_keeps_full_gate(self):
        paths = ['.github/workflows/ci.yml', 'tools/ci-scope.py', 'tools/test-ci-scope.py']
        self.assertEqual(ci_scope.scope(paths)['browser'], 'none')
        self.assertEqual(ci_scope.scope(paths)['recovery'], 'false')
        self.assertEqual(ci_scope.scope(paths + ['src/imagegen.rs'])['browser'], 'full')
        self.assertEqual(ci_scope.scope(paths, True)['recovery'], 'false')

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
        self.assertEqual(ci_scope.scope(paths, True)['recovery'], 'false')

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
            self.assertEqual((result['browser'], result['recovery']), ('full', 'false'), added)
        result = ci_scope.scope(paths, True)
        self.assertEqual((result['browser'], result['recovery']), ('full', 'false'))

    def test_pull_request_and_push_use_the_event_base(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            diff.return_value = b'M\0src/imagegen.rs\0'
            for event, payload in (('pull_request', {'pull_request': {'base': {'sha': 'b' * 40}}}),
                                   ('push', {'before': 'b' * 40})):
                self.assertEqual(ci_scope.change_range(event, payload, '', '', False), (['src/imagegen.rs'], False))
                self.assertIn('b' * 40, diff.call_args.args[0])

    def test_complete_range_preserves_risky_path_statuses(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            for status in (b'A', b'D', b'T'):
                with self.subTest(status=status):
                    diff.return_value = status + b'\0src/imagegen_prompt.rs\0M\0src/imagegen.rs\0'
                    paths, full = ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, False)
                    self.assertEqual(paths, ['src/imagegen_prompt.rs', 'src/imagegen.rs'])
                    self.assertTrue(full)
                    self.assertEqual(ci_scope.scope(paths, full)['recovery'], 'false')
            self.assertIn('--name-status', diff.call_args.args[0])

    def test_malformed_name_status_range_fails_closed(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            for payload in (b'M\0src/auth.rs', b'R100\0src/old.rs\0src/new.rs\0', b'M\0\0'):
                with self.subTest(payload=payload):
                    diff.return_value = payload
                    self.assertEqual(ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, False), ([], True))

    def test_git_range_reports_both_ends_of_backend_rename(self):
        def git(*args):
            return subprocess.check_output(['git', *args], text=True).strip()

        with TemporaryDirectory() as directory, chdir(directory):
            git('init', '--quiet')
            source = Path('src/imagegen_prompt.rs')
            source.parent.mkdir()
            source.write_text('fn prompt() {}\n', encoding='utf-8')
            git('add', '--', str(source))
            git('-c', 'user.name=CI test', '-c', 'user.email=ci@example.invalid',
                'commit', '--quiet', '--no-gpg-sign', '-m', 'Baseline')
            base = git('rev-parse', 'HEAD')
            target = Path('src/new_prompt.rs')
            git('mv', '--', str(source), str(target))
            git('-c', 'user.name=CI test', '-c', 'user.email=ci@example.invalid',
                'commit', '--quiet', '--no-gpg-sign', '-m', 'Rename')
            paths, full = ci_scope.change_range('workflow_dispatch', {}, '', base, False)
            self.assertEqual(set(paths), {source.as_posix(), target.as_posix()})
            self.assertTrue(full)


class RecoveryTests(unittest.TestCase):
    def test_weekly_and_explicit_recovery_are_independent_of_application_scope(self):
        self.assertTrue(ci_scope.recovery_reason([], 'schedule'))
        self.assertTrue(ci_scope.recovery_reason([], 'workflow_dispatch', True))
        for event in ('pull_request', 'push', 'workflow_dispatch'):
            self.assertFalse(ci_scope.recovery_reason([], event))
        for path in ci_scope.RECOVERY_TOOLS:
            self.assertTrue(ci_scope.recovery_reason([path], 'pull_request'))
            self.assertEqual(ci_scope.scope([path], True)['recovery'], 'true')

    def test_additive_migration_keeps_integration_checks_and_can_declare_recovery_risk(self):
        with TemporaryDirectory() as directory, patch.object(ci_scope, 'ROOT', Path(directory)):
            path = 'migrations/postgres/9999_change.sql'
            migration = Path(directory) / path
            migration.parent.mkdir(parents=True)
            migration.write_text('ALTER TABLE messages ADD COLUMN label TEXT;\n', encoding='utf-8')
            self.assertFalse(ci_scope.recovery_reason([path], 'pull_request'))
            migration.write_text('-- ci-recovery: required; changes restored data interpretation\n'
                                 'ALTER TABLE messages DROP COLUMN label;\n', encoding='utf-8')
            self.assertTrue(ci_scope.recovery_reason([path], 'pull_request'))
            migration.unlink()
            self.assertTrue(ci_scope.recovery_reason([path], 'pull_request'))
        selected = ci_scope.scope([path])
        self.assertEqual((selected['backend'], selected['postgres']), ('true', 'true'))

    def test_recovery_job_diff_ignores_unrelated_workflow_edits(self):
        with TemporaryDirectory() as directory, patch.object(ci_scope, 'ROOT', Path(directory)):
            path = '.github/workflows/ci.yml'
            workflow = Path(directory) / path
            workflow.parent.mkdir(parents=True)
            previous = 'jobs:\n  recovery-drill:\n    steps: [dump, restore]\n  other:\n    name: old\n'
            workflow.write_text(previous.replace('name: old', 'name: new'), encoding='utf-8')
            with patch.object(ci_scope.subprocess, 'check_output', return_value=previous):
                self.assertFalse(ci_scope.recovery_reason([path], 'pull_request', base='a' * 40))
                workflow.write_text(previous.replace('[dump, restore]', '[dump, restore, verify]'), encoding='utf-8')
                self.assertTrue(ci_scope.recovery_reason([path], 'pull_request', base='a' * 40))
            self.assertTrue(ci_scope.recovery_reason([path], 'pull_request'))

    def test_force_full_preserves_paths_for_independent_recovery_selection(self):
        with patch.object(ci_scope.subprocess, 'run') as run, patch.object(ci_scope.subprocess, 'check_output') as diff:
            run.return_value.returncode = 0
            diff.return_value = b'M\0tools/verify-media-recovery.sql\0'
            paths, full = ci_scope.change_range('workflow_dispatch', {}, '', 'a' * 40, True)
            self.assertTrue(full)
            self.assertTrue(ci_scope.recovery_reason(paths, 'workflow_dispatch'))

    def test_cli_outputs_and_summary_distinguish_full_application_and_recovery(self):
        with TemporaryDirectory() as directory:
            event = Path(directory) / 'event.json'
            event.write_text('{}', encoding='utf-8')
            for event_name, force_recovery, expected in (('workflow_dispatch', 'false', 'false'),
                                                        ('workflow_dispatch', 'true', 'true'),
                                                        ('schedule', 'false', 'true'),
                                                        ('push', 'false', 'false')):
                output = Path(directory) / 'output'
                summary = Path(directory) / 'summary'
                env = {**os.environ, 'GITHUB_EVENT_PATH': str(event), 'GITHUB_EVENT_NAME': event_name,
                       'GITHUB_REF': 'refs/tags/v1', 'CI_BASE_REVISION': '', 'CI_FORCE_FULL': 'true',
                       'CI_FORCE_RECOVERY': force_recovery}
                selected = json.loads(subprocess.check_output(
                    [sys.executable, str(ci_scope.ROOT / 'tools/ci-scope.py'), '--github-output', str(output),
                     '--summary', str(summary)], env=env, text=True))
                self.assertEqual((selected['backend'], selected['recovery']), ('true', expected))
                self.assertIn('Recovery decision:', summary.read_text(encoding='utf-8'))
                self.assertIn(f'recovery={expected}', output.read_text(encoding='utf-8'))


if __name__ == '__main__':
    unittest.main()
