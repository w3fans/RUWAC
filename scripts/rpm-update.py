#!/usr/bin/python3 -I
"""Explicit administrator opt-in to verified upstream Fedora testing RPM updates."""
import argparse
from contextlib import contextmanager
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import tempfile

POLICY = Path('/etc/ruwac/rpm-updates.json')
STATE = Path('/var/lib/ruwac-updates')
TIMER = 'ruwac-rpm-update.timer'
RPM_FORMAT = '%{NAME}\t%{VERSION}\t%{RELEASE}\t%{ARCH}\n'


def owned(path, owner=0, directory=False, private=False):
    info = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if (not kind(info.st_mode) or info.st_uid != owner or info.st_mode & (0o077 if private else 0o022)
            or (not directory and info.st_nlink != 1)):
        raise ValueError('Unsafe RPM updater state; configuration preserved')


def directory(path, owner=0, private=False):
    path.mkdir(mode=0o700 if private else 0o755, parents=True, exist_ok=True)
    owned(path, owner, directory=True, private=private)
    return path


@contextmanager
def lock(path, owner=0, blocking=True):
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != owner or info.st_nlink != 1 or info.st_mode & 0o077:
            raise ValueError('Unsafe RPM updater lock')
        try:
            fcntl.flock(fd, fcntl.LOCK_EX | (0 if blocking else fcntl.LOCK_NB))
        except BlockingIOError:
            raise ValueError('Another RPM updater owns the transaction; retry after it finishes') from None
        yield
    finally:
        os.close(fd)


def policy(path=POLICY, owner=0):
    if not path.exists() and not path.is_symlink():
        return False
    owned(path.parent, owner, directory=True)
    owned(path, owner)
    if path.stat().st_size > 4096:
        raise ValueError('Invalid RPM update configuration')
    data = json.loads(path.read_text())
    if set(data) != {'schema', 'enabled'} or type(data['schema']) is not int or data['schema'] != 1 or type(data['enabled']) is not bool:
        raise ValueError('Invalid RPM update configuration')
    return data['enabled']


def save_policy(enabled, path=POLICY, owner=0):
    directory(path.parent, owner)
    policy(path, owner)  # Refuse existing unsafe/malformed state instead of replacing it.
    fd, name = tempfile.mkstemp(prefix='.rpm-updates-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as output:
            output.write(json.dumps({'schema': 1, 'enabled': enabled}) + '\n')
            output.flush()
            os.fsync(output.fileno())
            os.fchmod(output.fileno(), 0o644)
        os.replace(name, path)
    finally:
        Path(name).unlink(missing_ok=True)


def query(command):
    result = subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise ValueError('RPM package query failed')
    return result.stdout


@contextmanager
def transaction_log(path, owner=0):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != owner or info.st_nlink != 1 or info.st_mode & 0o077:
            raise ValueError('Unsafe RPM updater log; existing data preserved')
        os.ftruncate(fd, 0)
        with os.fdopen(fd, 'w', closefd=False) as log:
            yield log
    finally:
        os.close(fd)


class Updater:
    def __init__(self, helper, state=STATE, config=POLICY, owner=0):
        self.helper, self.state, self.config, self.owner = helper, state, config, owner

    def installed(self):
        core = query(['/usr/bin/rpm', '-q', '--qf', RPM_FORMAT, 'ruwac']).strip().split('\t')
        if len(core) != 4 or core[0] != 'ruwac' or core[2:] != ['1.fc44', 'x86_64']:
            raise ValueError('Only upstream Fedora 44 testing RPM installations are supported')
        self.helper.version(core[1])
        result = subprocess.run(['/usr/bin/rpm', '-q', '--qf', RPM_FORMAT, 'ruwac-desktop'],
                                stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
        components = ['ruwac']
        if result.returncode == 0:
            if result.stdout.strip().split('\t') != ['ruwac-desktop', *core[1:]]:
                raise ValueError('Installed RPM components differ; update held')
            components.append('ruwac-desktop')
        elif result.returncode != 1:
            raise ValueError('Installed desktop package query failed')
        return core[1], components

    def configure(self, enabled):
        directory(self.config.parent, self.owner)
        with lock(self.config.parent / '.rpm-updates.lock', self.owner):
            if enabled:
                self.installed()
            save_policy(enabled, self.config, self.owner)
            try:
                if enabled:
                    subprocess.run(['/usr/bin/systemctl', 'daemon-reload'], check=True,
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
                subprocess.run(['/usr/bin/systemctl', 'enable' if enabled else 'disable', '--now', TIMER],
                               check=True, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=30)
            except (OSError, subprocess.SubprocessError):
                save_policy(False, self.config, self.owner)
                raise ValueError('RPM update timer configuration failed; automatic installation is disabled') from None
        return {'distribution': 'rpm', 'automatic_install': enabled, 'running_sessions': 'preserved'}

    def asset(self, release, data, component):
        name = component + '-' + release['latest'] + '-1.fc44.x86_64.rpm'
        matches = [a for a in data.get('assets', []) if a.get('name') == name]
        if not matches:
            return None
        if len(matches) != 1:
            raise ValueError('Ambiguous RPM update asset')
        asset = matches[0]
        if (asset.get('state') != 'uploaded' or type(asset.get('size')) is not int
                or not 0 < asset['size'] <= self.helper.MAX_BUNDLE_BYTES
                or asset.get('browser_download_url') != 'https://github.com/w3fans/RUWAC/releases/download/' + release['tag'] + '/' + name
                or not re.fullmatch(r'sha256:[0-9a-f]{64}', asset.get('digest', ''))):
            raise ValueError('Invalid RPM update asset')
        return asset

    def installable(self, current, components):
        """Newest release newer than `current` whose exact commit passed the
        quality and privacy checks and which has every component package."""
        pending = []
        for release in self.helper.newer_releases(current):
            reason = self.helper.release_readiness(release, 'testing-rpm')
            if reason:
                pending.append({'tag': release['tag'], 'reason': reason})
                continue
            data = self.helper.api('/releases/tags/' + release['tag'])
            if data.get('tag_name') != release['tag'] or data.get('draft') is not False or data.get('prerelease') is not False:
                pending.append({'tag': release['tag'], 'reason': 'unexpected release metadata'})
                continue
            assets = [self.asset(release, data, component) for component in components]
            if any(asset is None for asset in assets):
                pending.append({'tag': release['tag'], 'reason': 'packages not attached'})
                continue
            return (release, assets), pending
        return None, pending

    def install(self, automatic):
        if automatic and not policy(self.config, self.owner):
            return {'distribution': 'rpm', 'automatic_install': False, 'skipped': 'automatic installation disabled'}
        directory(self.state, self.owner, private=True)
        with lock(self.state / '.transaction.lock', self.owner, blocking=False):
            current, components = self.installed()
            newest = dict(self.helper.latest(current), distribution='rpm', delivery='testing-rpm')
            if not newest['available']:
                return newest
            # The newest tag may still be in qualification or lack packages;
            # install the newest complete release instead, never an older one
            # than the installed version.
            chosen, pending = self.installable(current, components)
            if chosen is None:
                return dict(newest, installable=None, pending=pending,
                            skipped='no newer release is complete yet (quality/privacy checks and all packages)')
            release, assets = chosen
            release = dict(release, distribution='rpm', delivery='testing-rpm',
                           newest_tag=newest['tag'], skipped_newer=pending)
            with tempfile.TemporaryDirectory(prefix='verified-', dir=self.state) as temporary:
                root = Path(temporary)
                packages = []
                for component, asset in zip(components, assets):
                    package = root / asset['name']
                    self.helper.download_bundle(asset, package)
                    verification = root / component
                    verification.mkdir(mode=0o700)
                    self.helper.verify_release_provenance(release, asset, package, verification, component)
                    metadata = query(['/usr/bin/rpm', '-qp', '--qf', RPM_FORMAT, str(package)]).strip().split('\t')
                    if metadata != [component, release['latest'], '1.fc44', 'x86_64']:
                        raise ValueError('Verified RPM metadata differs from the selected release')
                    packages.append(str(package))
                directory(self.config.parent, self.owner)
                # Opt-out commits independently during downloads. Once a DNF
                # transaction starts it finishes before disable can complete.
                with lock(self.config.parent / '.rpm-updates.lock', self.owner):
                    if automatic and not policy(self.config, self.owner):
                        return dict(release, distribution='rpm', skipped='automatic installation disabled during download')
                    if self.installed() != (current, components):
                        return dict(release, distribution='rpm', skipped='installed components changed during download')
                    with transaction_log(self.state / 'last-transaction.log', self.owner) as log:
                        try:
                            subprocess.run(['/usr/bin/dnf', '-y', 'upgrade', '--no-allow-downgrade', *packages],
                                           stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                           check=True, timeout=600)
                        except (OSError, subprocess.SubprocessError):
                            raise ValueError('Verified RPM transaction failed; inspect the private administrator log') from None
                    installed, after = self.installed()
                    if installed != release['latest'] or after != components:
                        raise ValueError('RPM transaction result differs; inspect the private administrator log')
                return dict(release, distribution='rpm', installed=installed, running_sessions='preserved')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument('-e', '--enable-testing', action='store_true')
    modes.add_argument('-d', '--disable', action='store_true')
    modes.add_argument('-s', '--status', action='store_true')
    modes.add_argument('-i', '--install', action='store_true')
    modes.add_argument('-I', '--install-auto', action='store_true')
    args = parser.parse_args()
    if args.status:
        print(json.dumps({'distribution': 'rpm', 'automatic_install': policy()}))
        return
    if os.geteuid() != 0:
        raise ValueError('Administrator authorization is required for system RPM updates')
    if platform.machine() != 'x86_64' or 'ID=fedora\n' not in Path('/etc/os-release').read_text() or 'VERSION_ID=44\n' not in Path('/etc/os-release').read_text():
        raise ValueError('RPM updates currently require Fedora 44 x86_64')
    path = Path(__file__).resolve()
    if path != Path('/usr/share/ruwac/rpm-update.py'):
        raise ValueError('Use the installed RPM updater')
    for item in [path, path.with_name('update-user.py')]:
        owned(item)
    owned(path.parent, directory=True)
    os.environ.clear()
    os.environ.update(PATH='/usr/sbin:/usr/bin', LANG='C.UTF-8')
    spec = importlib.util.spec_from_file_location('release_updates', path.with_name('update-user.py'))
    helper = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helper)
    updater = Updater(helper)
    result = updater.configure(not args.disable) if args.enable_testing or args.disable else updater.install(args.install_auto)
    print(json.dumps(result))


if __name__ == '__main__':
    try:
        main()
    except Exception:
        print(json.dumps({'error': 'System RPM update failed or was refused; inspect administrator state; running sessions are not restarted'}))
        raise SystemExit(1)
