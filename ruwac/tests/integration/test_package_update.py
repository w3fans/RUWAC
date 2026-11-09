#!/usr/bin/env python3
"""Package-only updater and idempotent fleet setup, against recording stubs.

No real package manager, repository, systemd unit or RUWAC engine is used:
dnf/rpm/systemctl are generated stubs whose state lives in a private
directory. Every stub call is recorded so tests can prove what never ran.
"""
import fcntl, json, os, subprocess, tempfile, unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
FEDORA = ROOT / 'ruwac/packaging/fedora'
REPO = 'copr:copr.fedorainfracloud.org:zvegelj:ruwac'

STUB = r'''#!/usr/bin/python3
import json, os, sys
from pathlib import Path
state_path = Path(os.environ['STUB_STATE'])
state = json.loads(state_path.read_text())
name, args = Path(sys.argv[0]).name, sys.argv[1:]
with open(os.environ['STUB_LOG'], 'a') as log:
    log.write(json.dumps([name] + args) + '\n')
def save(): state_path.write_text(json.dumps(state))
if name == 'rpm':
    pkg = args[-1]
    if pkg in state['installed']:
        if '--qf' in args: print(state['installed'][pkg], end='')
        sys.exit(0)
    sys.exit(1)
if name == 'systemctl':
    if args[:1] == ['get-default']: print(state['default_target']); sys.exit(0)
    if 'is-enabled' in args: sys.exit(0 if state['timer_enabled'] else 1)
    if 'enable' in args: state['timer_enabled'] = True; save(); sys.exit(0)
    if 'disable' in args: state['timer_enabled'] = False; save(); sys.exit(0)
    sys.exit(0)
if name == 'dnf':
    if args[:1] == ['repolist']:
        print('repo id                     repo name')
        for r in state['repos']: print(r + '   ' + r)
        sys.exit(0)
    if args[:2] == ['copr', '--help']: sys.exit(0)
    if 'copr' in args and 'enable' in args:
        state['repos'].append(os.environ['STUB_REPO']); save(); sys.exit(0)
    if args[:2] == ['config-manager', 'setopt']:
        key, value = args[2].rsplit('=', 1)
        section, option = key.rsplit('.', 1)
        d = Path(os.environ['STUB_ROOT']) / 'etc/dnf/repos.override.d'
        d.mkdir(parents=True, exist_ok=True)
        (d / '99-config_manager.repo').write_text('[%s]\n%s=%s\n' % (section, option, value))
        sys.exit(0)
    if 'check-upgrade' in args: sys.exit(state['check_rc'])
    if 'upgrade' in args:
        if state['upgrade_fails']: sys.exit(1)
        for p in [a for a in args if not a.startswith('-')][1:]:
            if p in state['installed']: state['installed'][p] = state['available']
        save(); sys.exit(0)
    if 'install' in args:
        for p in [a for a in args if not a.startswith('-') and a != 'install']:
            state['installed'][p] = state['available']
        save(); sys.exit(0)
    sys.exit(0)
'''


class Fixture:
    def __init__(self, **state):
        self.temp = tempfile.TemporaryDirectory()
        self.dir = Path(self.temp.name)
        self.root = self.dir / 'root'
        (self.root / 'etc').mkdir(parents=True)
        (self.root / 'etc/os-release').write_text('NAME=Fedora\nID=fedora\nVERSION_ID=45\n')
        self.bin = self.dir / 'bin'
        self.bin.mkdir()
        for name in ['dnf', 'rpm', 'systemctl']:
            (self.bin / name).write_text(STUB)
            (self.bin / name).chmod(0o755)
        self.state = self.dir / 'state.json'
        base = {'installed': {'ruwac': '0.4.73-1.fc45'}, 'available': '0.4.74-1.fc45', 'repos': ['fedora', 'updates', REPO],
                'check_rc': 0, 'upgrade_fails': False, 'timer_enabled': False, 'default_target': 'multi-user.target'}
        base.update(state)
        self.state.write_text(json.dumps(base))
        self.log = self.dir / 'calls'
        self.log.touch()
        self.env = dict(os.environ, PATH=f'{self.bin}:/usr/bin:/bin', STUB_STATE=str(self.state), STUB_LOG=str(self.log),
                        STUB_REPO=REPO, STUB_ROOT=str(self.root),
                        RUWAC_PKGUPDATE_CONF=str(self.root / 'etc/ruwac/update.conf'),
                        RUWAC_PKGUPDATE_STATUS=str(self.dir / 'status.json'),
                        RUWAC_PKGUPDATE_LOCK=str(self.dir / 'update.lock'),
                        RUWAC_PKGUPDATE_OFFLINE_MARKER=str(self.root / 'system-update'),
                        RUWAC_PKGUPDATE_OS_RELEASE=str(self.root / 'etc/os-release'),
                        RUWAC_SETUP_ROOT=str(self.root))

    def ring(self, value):
        (self.root / 'etc/ruwac').mkdir(parents=True, exist_ok=True)
        (self.root / 'etc/ruwac/update.conf').write_text(f'RING={value}\n')

    def update(self):
        q = subprocess.run(['bash', str(FEDORA / 'package-update')], env=self.env, capture_output=True, text=True, timeout=30)
        status = json.loads((self.dir / 'status.json').read_text()) if (self.dir / 'status.json').exists() else None
        return q.returncode, status

    def setup(self, *args):
        q = subprocess.run(['bash', str(FEDORA / 'fleet-setup'), '--json', *args], env=self.env, capture_output=True,
                           text=True, timeout=30)
        return q.returncode, json.loads(q.stdout.strip().splitlines()[-1])

    def calls(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def installed(self):
        return json.loads(self.state.read_text())['installed']

    def close(self):
        self.temp.cleanup()


class PackageUpdate(unittest.TestCase):
    def fixture(self, **state):
        f = Fixture(**state)
        self.addCleanup(f.close)
        f.ring('standard')
        return f

    def assertNeverTouchesEngines(self, f):
        for call in f.calls():
            self.assertNotIn(call[0], {'ruwac', 'kill', 'pkill'}, call)
            self.assertFalse(call[0] == 'systemctl' and ('--user' in call or 'restart' in call or 'stop' in call), call)

    def test_current_packages_change_nothing(self):
        f = self.fixture(check_rc=0)
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (0, 'current'))
        self.assertFalse(any('upgrade' in c and 'check-upgrade' not in c for c in f.calls()))
        self.assertNeverTouchesEngines(f)

    def test_available_update_upgrades_only_ruwac_packages_from_copr_and_fedora(self):
        f = self.fixture(check_rc=100, installed={'ruwac': '0.4.73-1.fc45', 'ruwac-desktop': '0.4.73-1.fc45'})
        rc, status = f.update()
        self.assertEqual((rc, status['result'], status['installed']), (0, 'upgraded', '0.4.74-1.fc45'))
        upgrade = next(c for c in f.calls() if c[0] == 'dnf' and 'upgrade' in c and 'check-upgrade' not in c)
        packages = [a for a in upgrade[1:] if not a.startswith('-') and a != 'upgrade']
        self.assertEqual(packages, ['ruwac', 'ruwac-desktop'])
        self.assertEqual({a for a in upgrade if a.startswith('--repo=')}, {f'--repo={REPO}', '--repo=fedora', '--repo=updates'})
        self.assertIn(f'--setopt={REPO}.skip_if_unavailable=0', upgrade)
        self.assertEqual(f.installed(), {'ruwac': '0.4.74-1.fc45', 'ruwac-desktop': '0.4.74-1.fc45'})
        self.assertNeverTouchesEngines(f)

    def test_query_or_install_failure_is_reported_and_changes_nothing(self):
        f = self.fixture(check_rc=1)
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (1, 'error'))
        self.assertIn('dnf exit 1', status['detail'])
        f = self.fixture(check_rc=100, upgrade_fails=True)
        rc, status = f.update()
        self.assertEqual((rc, status['result'], f.installed()['ruwac']), (1, 'error', '0.4.73-1.fc45'))

    def test_queued_offline_system_update_defers_without_calling_dnf(self):
        f = self.fixture(check_rc=100)
        (f.root / 'system-update').symlink_to('/var/lib/system-update')
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (0, 'deferred'))
        self.assertFalse(any(c[0] == 'dnf' for c in f.calls()))

    def test_disabled_ring_and_held_lock_do_nothing(self):
        f = self.fixture(check_rc=100)
        f.ring('disabled')
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (0, 'disabled'))
        self.assertFalse(any(c[0] == 'dnf' for c in f.calls()))
        f = self.fixture(check_rc=100)
        with open(f.dir / 'update.lock', 'w') as held:
            fcntl.flock(held, fcntl.LOCK_EX)
            rc, status = f.update()
        self.assertEqual((rc, status), (0, None))
        self.assertFalse(any(c[0] == 'dnf' for c in f.calls()))

    def test_missing_copr_or_non_fedora_fails_closed(self):
        f = self.fixture(check_rc=100, repos=['fedora', 'updates'])
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (1, 'error'))
        self.assertFalse(any('upgrade' in c for c in f.calls()))
        f = self.fixture(check_rc=100)
        (f.root / 'etc/os-release').write_text('ID=ubuntu\n')
        rc, status = f.update()
        self.assertEqual((rc, status['result']), (1, 'error'))
        self.assertFalse(any(c[0] == 'dnf' for c in f.calls()))

    def test_status_is_sanitized(self):
        f = self.fixture(check_rc=100)
        f.update()
        text = (f.dir / 'status.json').read_text()
        self.assertEqual(set(json.loads(text)), {'version', 'checked_at', 'ring', 'result', 'installed', 'available', 'detail'})
        self.assertNotIn(os.environ.get('USER', 'nobody-user'), text)
        self.assertNotIn('http', text)


class FleetSetup(unittest.TestCase):
    def fixture(self, **state):
        f = Fixture(**state)
        self.addCleanup(f.close)
        return f

    def test_fresh_headless_machine_then_idempotent_rerun(self):
        f = self.fixture(installed={}, repos=['fedora', 'updates'])
        rc, out = f.setup('--ring', 'standard')
        self.assertEqual(rc, 0, out)
        self.assertEqual(set(out['changed']), {'copr', 'metadata_expire', 'packages', 'ring', 'timer_interval', 'timer'})
        self.assertEqual(set(f.installed()), {'ruwac'})  # headless: no desktop
        override = (f.root / 'etc/dnf/repos.override.d/99-config_manager.repo').read_text()
        self.assertIn(f'[{REPO}]', override)
        self.assertIn('metadata_expire=300', override)
        self.assertIn('OnUnitActiveSec=3h', (f.root / 'etc/systemd/system/ruwac-package-update.timer.d/ring.conf').read_text())
        before = len(f.calls())
        rc, again = f.setup('--ring', 'standard')
        self.assertEqual((rc, again['changed']), (0, []), again)
        self.assertFalse(any(c[0] == 'dnf' and ('install' in c or 'enable' in c or 'setopt' in c) for c in f.calls()[before:]))

    def test_desktop_machine_gets_matching_desktop_and_canary_interval(self):
        # A graphical default without a display manager is still headless.
        f = self.fixture(installed={}, repos=['fedora', 'updates'], default_target='graphical.target')
        rc, out = f.setup('--ring', 'standard')
        self.assertEqual((rc, set(f.installed())), (0, {'ruwac'}), out)
        f = self.fixture(installed={'gdm': '49-1.fc45'}, repos=['fedora', 'updates'], default_target='graphical.target')
        rc, out = f.setup('--ring', 'canary')
        self.assertEqual(rc, 0, out)
        self.assertEqual(set(f.installed()), {'gdm', 'ruwac', 'ruwac-desktop'})
        self.assertTrue(out['desktop'])
        self.assertIn('OnUnitActiveSec=45min', (f.root / 'etc/systemd/system/ruwac-package-update.timer.d/ring.conf').read_text())

    def test_disabled_ring_disables_timer_and_installed_desktop_is_kept(self):
        f = self.fixture(installed={'ruwac': '0.4.73-1.fc45', 'ruwac-desktop': '0.4.73-1.fc45'}, timer_enabled=True)
        rc, out = f.setup('--ring', 'disabled', '--headless')
        self.assertEqual(rc, 0, out)
        self.assertIn('timer', out['changed'])
        self.assertFalse(json.loads(f.state.read_text())['timer_enabled'])
        self.assertIn('ruwac-desktop', f.installed())

    def test_non_fedora_is_refused_without_changes(self):
        f = self.fixture()
        (f.root / 'etc/os-release').write_text('ID=debian\n')
        rc, out = f.setup()
        self.assertEqual((rc, out['ok']), (1, False))
        self.assertEqual(f.calls(), [])

    def test_existing_upgrade_never_touches_engines(self):
        f = self.fixture(installed={}, repos=['fedora', 'updates'])
        f.setup()
        for call in f.calls():
            self.assertNotIn(call[0], {'ruwac', 'kill', 'pkill'})
            self.assertFalse(call[0] == 'systemctl' and '--user' in call)


if __name__ == '__main__':
    unittest.main()
