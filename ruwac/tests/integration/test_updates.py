#!/usr/bin/env python3
"""Updater fault and immutable per-user installation tests; no production state or network."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location('ruwac_updates', ROOT / 'scripts/update-user.py')
updates = importlib.util.module_from_spec(spec)
spec.loader.exec_module(updates)
COMMIT = 'a' * 40


class Updates(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.environment = patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.root / 'config'), 'XDG_CACHE_HOME': str(self.root / 'cache')})
        self.environment.start()
        # Installation lifecycle fixtures supply an eligible release; the
        # publication qualification policy has separate refusal regressions.
        readiness = patch.object(updates, 'release_readiness', return_value=None)
        readiness.start(); self.addCleanup(readiness.stop)

    def tearDown(self):
        self.environment.stop()
        self.temp.cleanup()

    def metadata(self, **changes):
        release = {'tag_name': 'v0.2.1', 'draft': False, 'prerelease': False,
                   'html_url': 'https://github.com/w3fans/RUWAC/releases/tag/v0.2.1'}
        release.update(changes)
        return release

    def test_numeric_order_not_lexical_and_no_downgrade(self):
        data = self.metadata(tag_name='v0.2.10', html_url='https://github.com/w3fans/RUWAC/releases/tag/v0.2.10')
        with patch.object(updates, 'api', side_effect=[data, {'object': {'type': 'commit', 'sha': COMMIT}}]):
            self.assertTrue(updates.latest('0.2.9')['available'])
        with patch.object(updates, 'api', side_effect=[self.metadata(), {'object': {'type': 'commit', 'sha': COMMIT}}]):
            self.assertFalse(updates.latest('0.3.0')['available'])

    def test_reject_drafts_prereleases_wrong_origin_and_invalid_tag(self):
        for values in [{'draft': True}, {'prerelease': True}, {'html_url': 'https://example.org/'}, {'tag_name': 'v0.2.1;bad'}, {'tag_name': 'v0.02.1'}]:
            with self.subTest(values=values), patch.object(updates, 'api', return_value=self.metadata(**values)):
                with self.assertRaises(ValueError): updates.latest('0.2.0')

    def test_annotated_tag_resolves_exact_commit(self):
        with patch.object(updates, 'api', side_effect=[self.metadata(), {'object': {'type': 'tag', 'sha': 'b' * 40}}, {'object': {'type': 'commit', 'sha': COMMIT}}]) as api:
            self.assertEqual(updates.latest('0.2.0')['commit'], COMMIT)
            self.assertEqual(api.call_args.args[0], '/git/tags/' + 'b' * 40)

    def test_tag_changed_during_fetch_never_checks_out(self):
        with patch.object(updates, 'run') as run, patch.object(subprocess, 'check_output', return_value='b' * 40):
            with self.assertRaisesRegex(ValueError, 'changed'):
                updates.checkout({'tag': 'v0.2.1', 'commit': COMMIT}, self.root / 'source')
            self.assertFalse(any('checkout' in call.args[0] for call in run.call_args_list))

    def test_preferences_opt_out_preserves_unknown_and_symlink_files(self):
        self.assertTrue(updates.automatic_setting())
        self.assertFalse(updates.automatic_setting(False))
        self.assertFalse(updates.automatic_setting())
        path = updates.settings_path()
        path.write_text('{"schema":2,"automatic_check":true}')
        before = path.read_bytes()
        with self.assertRaises(ValueError): updates.automatic_setting(True)
        self.assertEqual(path.read_bytes(), before)
        path.unlink(); other = self.root / 'foreign'; other.write_text('preserve')
        path.symlink_to(other)
        with self.assertRaises(ValueError): updates.automatic_setting(True)
        self.assertEqual(other.read_text(), 'preserve')

    def test_disabled_automatic_check_performs_no_network(self):
        updates.automatic_setting(False)
        with patch.object(sys, 'argv', ['updater', '--current', '0.2.0', '--check-auto']), patch.object(updates, 'api') as api, contextlib.redirect_stdout(io.StringIO()) as output:
            updates.main()
        api.assert_not_called()
        self.assertFalse(json.loads(output.getvalue())['available'])

    def test_lock_contention_and_unsafe_lock(self):
        path = self.root / 'lock'; held = updates.locked_file(path)
        try:
            with self.assertRaisesRegex(ValueError, 'active'): updates.locked_file(path)
        finally: os.close(held)
        path.unlink(); other = self.root / 'other'; other.write_text('preserve'); path.symlink_to(other)
        with self.assertRaises(OSError): updates.locked_file(path)
        self.assertEqual(other.read_text(), 'preserve')

    def fixture_install(self, version):
        source = self.root / ('build-' + version)
        (source / 'ruwac').mkdir(parents=True)
        (source / 'ruwac/Cargo.toml').write_text('[workspace.package]\nversion="' + version + '"\n')
        binary = source / 'target/release/ruwac'; binary.parent.mkdir(parents=True)
        binary.write_text('#!/bin/sh\nprintf "RUWAC ' + version + '\\n"\n'); binary.chmod(0o755)
        original = subprocess.check_output
        def output(args, **kwargs):
            if args[0] == 'git': return (COMMIT + '\n') if kwargs.get('text') else (COMMIT + '\n').encode()
            return original(args, **kwargs)
        with patch.object(sys, 'argv', ['installer', '--build-root', str(source), '--prefix', str(self.root / 'prefix')]), patch.object(subprocess, 'check_output', side_effect=output), contextlib.redirect_stdout(io.StringIO()):
            result = runpy.run_path(str(ROOT / 'scripts/install-user.py'), run_name='__main__')
            os.close(result['lock'])
        return source

    def test_install_success_requires_actual_version_receipt_links_and_payload(self):
        source = self.fixture_install('0.0.1')
        prefix = self.root / 'prefix'
        expected = updates.installation_artifacts(source / 'target/release', False)
        release = {'latest': '0.0.1', 'commit': COMMIT, 'current': '0.0.0', 'available': True}
        result = updates.confirm_installation(release, prefix, expected)
        self.assertEqual(result['installed_version'], '0.0.1')
        self.assertEqual(result['current'], '0.0.1')
        self.assertFalse(result['available'])
        with self.assertRaisesRegex(ValueError, 'without installing'):
            updates.confirm_installation(dict(release, latest='0.0.2'), prefix, expected)
        with self.assertRaisesRegex(ValueError, 'receipt'):
            updates.confirm_installation(dict(release, commit='b' * 40), prefix, expected)
        command = prefix / 'bin/ruwac'
        command.unlink(); command.symlink_to('/bin/true')
        with self.assertRaisesRegex(ValueError, 'command'):
            updates.confirm_installation(release, prefix, expected)
        command.unlink(); command.symlink_to('../lib/ruwac/current/ruwac')
        (prefix / 'lib/ruwac/current/ruwac').write_bytes(b'changed installed payload')
        with self.assertRaisesRegex(ValueError, 'command'):
            updates.confirm_installation(release, prefix, expected)
        fifo = self.root / 'fifo'; os.mkfifo(fifo)
        with self.assertRaisesRegex(ValueError, 'size or type'):
            updates.artifact_digest(fifo)

    def test_real_install_and_rollback_keep_running_executable_inode(self):
        self.fixture_install('0.0.1')
        prefix = self.root / 'prefix'
        old = (prefix / 'bin/ruwac').resolve()
        old_stat = old.stat()
        self.fixture_install('0.0.2')
        self.assertEqual(updates.installed_version(prefix, '0.0.1'), '0.0.2')
        self.assertEqual(old.stat().st_ino, old_stat.st_ino)
        self.assertEqual(subprocess.check_output([str(old)], text=True).strip(), 'RUWAC 0.0.1')
        self.assertEqual(updates.rollback(prefix)['installed'], '0.0.1')
        self.assertEqual((prefix / 'bin/ruwac').resolve(), old)
        self.assertEqual(updates.rollback(prefix)['installed'], '0.0.2')

    def test_rollback_revokes_install_consent_and_retains_check_preferences(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        data = updates.preferences()
        data.update(automatic_install=True, automatic_check=False, extension={'generated': True})
        updates.save_preferences(data)
        with patch.object(updates, 'run') as run:
            result = updates.rollback(self.root / 'prefix')
        run.assert_not_called()
        self.assertFalse(result['automatic_install'])
        self.assertFalse(updates.automatic_install_setting())
        self.assertFalse(updates.automatic_setting())
        self.assertEqual(updates.preferences()['extension'], {'generated': True})
        with patch.object(sys, 'argv', ['updater', '--current', '0.0.1', '--install-auto']), patch.object(updates, 'api') as api, contextlib.redirect_stdout(io.StringIO()):
            updates.main()
        api.assert_not_called()

    def test_failed_rollback_validation_does_not_revoke_consent(self):
        self.fixture_install('0.0.1')
        data = updates.preferences(); data['automatic_install'] = True; updates.save_preferences(data)
        with self.assertRaisesRegex(ValueError, 'previous'): updates.rollback(self.root / 'prefix')
        self.assertTrue(updates.automatic_install_setting())

    def test_rollback_preference_write_failure_preserves_artifact_links(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        base = self.root / 'prefix/lib/ruwac'
        before = {name: (base / name).readlink() for name in ['current', 'previous']}
        with patch.object(updates, 'save_preferences', side_effect=OSError('generated disk failure')):
            with self.assertRaises(OSError): updates.rollback(self.root / 'prefix')
        self.assertEqual({name: (base / name).readlink() for name in before}, before)

    def test_rollback_preference_lock_contention_preserves_links(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        prefix = self.root / 'prefix'
        lock = updates.locked_file(updates.settings_path().parent / '.settings.lock')
        try:
            with self.assertRaisesRegex(ValueError, 'active'): updates.rollback(prefix)
            self.assertEqual(updates.installed_version(prefix, '0.0.0'), '0.0.2')
        finally:
            os.close(lock)
        self.assertEqual(updates.rollback(prefix)['installed'], '0.0.1')

    def test_rollback_preserves_unknown_preference_schema(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        path = updates.settings_path(); path.write_text('{"schema":2,"automatic_check":true}')
        before = path.read_bytes()
        with self.assertRaises(ValueError): updates.rollback(self.root / 'prefix')
        self.assertEqual(path.read_bytes(), before)
        self.assertEqual(updates.installed_version(self.root / 'prefix', '0.0.0'), '0.0.2')

    def test_tampered_previous_artifact_refuses_rollback(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        base = self.root / 'prefix/lib/ruwac'
        (base / 'previous/ruwac').write_text('changed')
        before = (base / 'current').readlink()
        with self.assertRaisesRegex(ValueError, 'changed'): updates.rollback(self.root / 'prefix')
        self.assertEqual((base / 'current').readlink(), before)

    def test_newer_running_frontend_does_not_mask_rolled_back_installation(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        prefix = self.root / 'prefix'
        updates.rollback(prefix)
        self.assertEqual(updates.installed_version(prefix, '0.0.2'), '0.0.1')
        data = self.metadata(tag_name='v0.0.2', html_url='https://github.com/w3fans/RUWAC/releases/tag/v0.0.2')
        with patch.object(sys, 'argv', ['updater', '--current', '0.0.2', '--check', '--prefix', str(prefix)]), patch.object(updates, 'api', side_effect=[data, {'object': {'type': 'commit', 'sha': COMMIT}}]), patch.object(updates, 'newer_releases', return_value=[]), contextlib.redirect_stdout(io.StringIO()) as output:
            updates.main()
        result = json.loads(output.getvalue())
        self.assertTrue(result['available'])
        self.assertEqual(result['current'], '0.0.1')
        self.assertEqual(result['running'], '0.0.2')

    def test_missing_previous_and_foreign_prefix_preserved(self):
        self.fixture_install('0.0.1')
        prefix = self.root / 'prefix'
        with self.assertRaisesRegex(ValueError, 'previous'): updates.rollback(prefix)
        other = self.root / 'foreign'; other.mkdir(); linked = self.root / 'linked'; linked.symlink_to(other)
        with self.assertRaisesRegex(ValueError, 'Unsafe'): updates.installed_version(linked, '0.0.1')
        self.assertTrue(linked.is_symlink())

    def test_build_failure_never_runs_installer(self):
        cache = updates.private_directory(self.root / 'update-cache')
        def fail(args, **kwargs):
            if args[0] == 'cargo': raise subprocess.CalledProcessError(1, args)
        original = Path.read_text
        def text(path, *args, **kwargs):
            return 'ID=fedora\nVERSION_ID=44\n' if str(path) == '/etc/os-release' else original(path, *args, **kwargs)
        with patch.object(Path, 'read_text', text), patch.object(updates, 'checkout'), patch.object(shutil, 'which', return_value='/usr/bin/tool'), patch.object(updates, 'run', side_effect=fail) as run:
            with self.assertRaises(subprocess.CalledProcessError):
                updates.install({'tag': 'v0.2.1', 'latest': '0.2.1', 'commit': COMMIT}, self.root / 'prefix', cache)
            self.assertFalse(any('install-user.py' in str(call.args[0]) for call in run.call_args_list))
        self.assertFalse((self.root / 'prefix').exists())
        self.assertFalse(list(cache.glob('source-*')))

    def test_rollback_refuses_while_an_update_build_is_active(self):
        self.fixture_install('0.0.1'); self.fixture_install('0.0.2')
        cache = updates.private_directory(self.root / 'cache/RUWAC/updates')
        lock = updates.locked_file(cache / '.update.lock')
        try:
            with patch.object(sys, 'argv', ['updater', '--current', '0.0.2', '--rollback', '--prefix', str(self.root / 'prefix')]):
                with self.assertRaisesRegex(ValueError, 'active'): updates.main()
            self.assertEqual(updates.installed_version(self.root / 'prefix', '0.0.0'), '0.0.2')
        finally:
            os.close(lock)


    def test_automatic_install_defaults_off_and_performs_no_network(self):
        self.assertFalse(updates.automatic_install_setting())
        with patch.object(sys, 'argv', ['updater', '--current', '0.2.0', '--install-auto']), patch.object(updates, 'api') as api, contextlib.redirect_stdout(io.StringIO()) as output:
            updates.main()
        api.assert_not_called()
        self.assertFalse(json.loads(output.getvalue())['automatic_install'])

    def test_install_preference_retains_existing_check_and_extension_fields(self):
        updates.automatic_setting(False)
        data = updates.preferences(); data['extension'] = {'generated': True}; updates.save_preferences(data)
        self.fixture_install('0.0.1')
        with patch.object(updates, 'run'):
            self.assertTrue(updates.automatic_install_setting(True, self.root / 'prefix'))
            self.assertFalse(updates.automatic_install_setting(False, self.root / 'prefix'))
        self.assertFalse(updates.automatic_setting())
        self.assertEqual(updates.preferences()['extension'], {'generated': True})

    def test_scheduler_units_escape_paths_and_keep_installed_components(self):
        self.fixture_install('0.0.1')
        with patch.object(updates, 'run') as run:
            updates.automatic_install_setting(True, self.root / 'prefix')
        directory = updates.scheduler_directory()
        service = (directory / 'ruwac-update.service').read_text()
        self.assertIn('--install-auto', service)
        self.assertIn('ExecStart=:', service)
        self.assertIn('OnUnitActiveSec=6h', (directory / 'ruwac-update.timer').read_text())
        self.assertEqual((directory / 'ruwac-update.service').stat().st_mode & 0o777, 0o600)
        self.assertIn(['systemctl', '--user', 'enable', '--now', 'ruwac-update.timer'], [call.args[0] for call in run.call_args_list])
        self.assertIn('%%', updates.unit_quote('generated%prefix'))
        with self.assertRaises(ValueError): updates.unit_quote('generated\nUnit=foreign')
        with patch.object(sys, 'argv', ['updater', '--current', '0.0.1', '--install-auto', '--prefix', str(self.root / 'prefix')]), patch.object(updates, 'verified_status', return_value={'available': True, 'latest': '0.0.2', 'latest_verified':'0.0.2'}), patch.object(updates, 'automatic_release_verified', return_value=True), patch.object(updates, 'install', return_value={'installed': '0.0.2'}) as install, contextlib.redirect_stdout(io.StringIO()):
            updates.main()
        self.assertFalse(install.call_args.args[3])
        self.assertTrue(install.call_args.kwargs['automatic'])

    def test_scheduler_error_restores_units_and_preferences(self):
        self.fixture_install('0.0.1'); updates.automatic_setting(False)
        before = updates.settings_path().read_bytes()
        def run(args):
            if 'enable' in args: raise subprocess.CalledProcessError(1, args)
        with patch.object(updates, 'run', side_effect=run), patch.object(subprocess, 'run'):
            with self.assertRaises(subprocess.CalledProcessError): updates.automatic_install_setting(True, self.root / 'prefix')
        self.assertEqual(updates.settings_path().read_bytes(), before)
        self.assertFalse(list(updates.scheduler_directory().glob('ruwac-update.*')))

    def test_edited_and_foreign_scheduler_units_preserved(self):
        self.fixture_install('0.0.1')
        with patch.object(updates, 'run'):
            updates.automatic_install_setting(True, self.root / 'prefix')
        path = updates.scheduler_directory() / 'ruwac-update.service'
        path.write_text('owner edited unit')
        with self.assertRaisesRegex(ValueError, 'edited'): updates.automatic_install_setting(True, self.root / 'prefix')
        self.assertEqual(path.read_text(), 'owner edited unit')
        path.unlink(); path.symlink_to(self.root / 'foreign')
        with self.assertRaisesRegex(ValueError, 'Unsafe'): updates.automatic_install_setting(True, self.root / 'prefix')
        self.assertTrue(path.is_symlink())

    def test_opt_out_stays_committed_when_user_manager_unavailable(self):
        self.fixture_install('0.0.1')
        with patch.object(updates, 'run'): updates.automatic_install_setting(True, self.root / 'prefix')
        with patch.object(updates, 'run', side_effect=subprocess.CalledProcessError(1, ['systemctl'])):
            with self.assertRaises(subprocess.CalledProcessError): updates.automatic_install_setting(False)
        self.assertFalse(updates.automatic_install_setting())

    def test_opt_out_during_build_prevents_automatic_install(self):
        self.fixture_install('0.0.1')
        data = updates.preferences(); data['automatic_install'] = True; updates.save_preferences(data)
        cache = updates.private_directory(self.root / 'update-cache')
        called = []
        def run(args, **kwargs):
            called.append(args)
            if args[0] == 'cargo':
                data = updates.preferences(); data['automatic_install'] = False; updates.save_preferences(data)
        original = Path.read_text
        def text(path, *args, **kwargs):
            return 'ID=fedora\nVERSION_ID=44\n' if str(path) == '/etc/os-release' else original(path, *args, **kwargs)
        with patch.object(Path, 'read_text', text), patch.object(updates, 'checkout'), patch.object(shutil, 'which', return_value='/usr/bin/tool'), patch.object(updates, 'run', side_effect=run):
            result = updates.install({'latest': '0.0.2'}, self.root / 'prefix', cache, False, automatic=True)
        self.assertIn('disabled during build', result['skipped'])
        self.assertFalse(any('install-user.py' in str(args[0:2]) for args in called))
        self.assertEqual(updates.installed_version(self.root / 'prefix', '0.0.0'), '0.0.1')

    def test_missing_installation_or_unrelated_command_refuses_scheduler(self):
        with self.assertRaisesRegex(ValueError, 'Install RUWAC'): updates.automatic_install_setting(True, self.root / 'prefix')
        self.fixture_install('0.0.1')
        command = self.root / 'prefix/bin/ruwac'; command.unlink(); command.write_text('unrelated')
        with self.assertRaisesRegex(ValueError, 'Unrelated'): updates.automatic_install_setting(True, self.root / 'prefix')
        self.assertEqual(command.read_text(), 'unrelated')


    def test_disable_does_not_stop_unrelated_scheduler(self):
        directory = updates.scheduler_directory()
        path = directory / 'ruwac-update.timer'; path.write_text('foreign timer')
        with patch.object(updates, 'run') as run:
            self.assertFalse(updates.automatic_install_setting(False))
        run.assert_not_called()
        self.assertEqual(path.read_text(), 'foreign timer')


    def test_automatic_quality_gate_is_bound_to_release_commit(self):
        matching = {'id': 2, 'head_sha': COMMIT, 'head_branch': 'main', 'path': '.github/workflows/quality.yml'}
        for jobs, expected in [([{'name': 'quality', 'conclusion': 'success'}, {'name': 'privacy', 'conclusion': 'success'}, {'name': 'advisories', 'conclusion': 'failure'}], True), ([{'name': 'quality', 'conclusion': 'success'}, {'name': 'privacy', 'conclusion': None}], False)]:
            with patch.object(updates, 'api', side_effect=[{'workflow_runs': [matching]}, {'jobs': jobs}]):
                self.assertEqual(updates.automatic_release_verified({'commit': COMMIT}), expected)
        for mismatch in [{'head_sha': 'b' * 40}, {'head_branch': 'feature'}, {'path': '.github/workflows/other.yml'}]:
            with patch.object(updates, 'api', return_value={'workflow_runs': [dict(matching, **mismatch)]}):
                self.assertFalse(updates.automatic_release_verified({'commit': COMMIT}))

    def test_newer_releases_are_stable_newer_and_newest_first(self):
        rows = [{'tag_name': t, 'draft': d, 'prerelease': pr} for t, d, pr in [
            ('v0.0.3', False, False), ('v0.0.9', True, False), ('v0.0.2', False, False),
            ('v0.0.1', False, False), ('v0.0.4', False, True), ('v0.0.10', False, False)]]
        with patch.object(updates, 'api', return_value=rows), patch.object(updates, 'describe', side_effect=lambda r, c: r['tag_name']):
            self.assertEqual(updates.newer_releases('0.0.1'), ['v0.0.10', 'v0.0.3', 'v0.0.2'])

    def test_pending_automatic_quality_gate_never_invokes_installer(self):
        data = updates.preferences(); data['automatic_install'] = True; updates.save_preferences(data)
        with patch.object(sys, 'argv', ['updater', '--current', '0.0.1', '--install-auto']), patch.object(updates, 'verified_status', return_value={'available': False, 'skipped': 'waiting for quality: failure'}), patch.object(updates, 'install') as install, contextlib.redirect_stdout(io.StringIO()) as output:
            updates.main()
        install.assert_not_called()
        self.assertIn('waiting', json.loads(output.getvalue())['skipped'])

    def test_bootstrap_installs_without_a_preexisting_cli(self):
        with patch.object(sys, 'argv', ['updater', '--bootstrap', '--prefix', str(self.root / 'prefix')]), patch.object(updates, 'verified_status', return_value={'available': True}) as status, patch.object(updates, 'install', return_value={'installed': 'generated-release'}) as install, contextlib.redirect_stdout(io.StringIO()):
            updates.main()
        self.assertEqual(status.call_args.args[0], '0.0.0')
        self.assertTrue(install.call_args.args[3])

    def test_remote_bootstrap_checks_selected_cli_and_never_shadows_a_newer_one(self):
        with patch.object(sys, 'argv', ['updater', '--bootstrap', '--remote-cli', 'ruwac', '--prefix', str(self.root / 'prefix')]), patch.object(updates, 'remote_cli_version', return_value='0.4.99') as probe, patch.object(updates, 'verified_status', return_value={'available': False}) as status, patch.object(updates, 'install') as install, contextlib.redirect_stdout(io.StringIO()):
            updates.main()
        probe.assert_called_once_with('ruwac')
        status.assert_called_once_with('0.4.99', 'source-build')
        install.assert_not_called()

    def test_remote_cli_version_probes_actual_executable_and_rejects_invalid_output(self):
        with patch.object(updates.shutil, 'which', return_value='/generated/ruwac'), patch.object(updates.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'RUWAC 0.4.23\n', '')) as probe:
            self.assertEqual(updates.remote_cli_version('ruwac'), '0.4.23')
            self.assertEqual(probe.call_args.args[0], ['/generated/ruwac', '--version'])
        with patch.object(updates.shutil, 'which', return_value='/generated/ruwac'), patch.object(updates.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'unexpected', '')):
            with self.assertRaises(ValueError):
                updates.remote_cli_version('ruwac')

    def test_bootstrap_respects_installed_version_and_does_not_downgrade(self):
        self.fixture_install('0.0.2')
        with patch.object(sys, 'argv', ['updater', '--bootstrap', '--prefix', str(self.root / 'prefix')]), patch.object(updates, 'verified_status', return_value={'available': False}) as status, patch.object(updates, 'install') as install, contextlib.redirect_stdout(io.StringIO()):
            updates.main()
        self.assertEqual(status.call_args.args[0], '0.0.2')
        install.assert_not_called()


if __name__ == '__main__':
    unittest.main()
