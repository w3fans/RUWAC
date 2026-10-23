#!/usr/bin/env python3
"""GitHub source updates for per-user Fedora installations; never publishes a release."""
import argparse
import fcntl
import hashlib
import json
import os
import gzip
import platform
import tarfile
import time
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
import tomllib
import urllib.request
import urllib.parse

API = 'https://api.github.com/repos/w3fans/RUWAC'
REPOSITORY = 'https://github.com/w3fans/RUWAC.git'
VERSION = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z')
SHA = re.compile(r'[0-9a-f]{40}\Z')


def version(value):
    if not isinstance(value, str) or not VERSION.fullmatch(value):
        raise ValueError('Invalid numeric release version')
    return tuple(map(int, value.split('.')))


def private_directory(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077:
        raise ValueError('Update state directory must be private and owned by this user')
    return path


def api(path):
    request = urllib.request.Request(API + path, headers={
        'Accept': 'application/vnd.github+json', 'User-Agent': 'RUWAC-updater',
        'Cache-Control': 'no-cache'})
    with urllib.request.urlopen(request, timeout=20) as response:
        if urllib.parse.urlparse(response.url).hostname != 'api.github.com':
            raise ValueError('Unexpected GitHub API redirect')
        data = response.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024:
            raise ValueError('GitHub response exceeds limit')
        return json.loads(data)


def latest(current):
    version(current)
    return describe(api('/releases/latest'), current)


def newer_releases(current, limit=10):
    """Stable releases newer than `current`, newest first (bounded)."""
    version(current)
    found = []
    for release in api('/releases?per_page=30'):
        tag = release.get('tag_name', '')
        if (release.get('draft') is not False or release.get('prerelease') is not False
                or not isinstance(tag, str) or not re.fullmatch(r'v\d+\.\d+\.\d+', tag)):
            continue
        if version(tag[1:]) > version(current):
            found.append(release)
    found.sort(key=lambda r: version(r['tag_name'][1:]), reverse=True)
    return [describe(release, current) for release in found[:limit]]


def describe(release, current):
    tag = release.get('tag_name', '')
    if not isinstance(tag, str) or not tag.startswith('v'):
        raise ValueError('Invalid GitHub release tag')
    newer = version(tag[1:]) > version(current)
    url = 'https://github.com/w3fans/RUWAC/releases/tag/' + tag
    if release.get('draft') is not False or release.get('prerelease') is not False or release.get('html_url') != url:
        raise ValueError('Expected a published stable RUWAC source release')
    ref = api('/git/ref/tags/' + tag)['object']
    if not isinstance(ref.get('sha'), str) or not SHA.fullmatch(ref['sha']):
        raise ValueError('Invalid release reference')
    if ref['type'] == 'tag':
        ref = api('/git/tags/' + ref['sha'])['object']
    if ref['type'] != 'commit' or not SHA.fullmatch(ref['sha']):
        raise ValueError('Release does not identify an immutable commit')
    return {'current': current, 'latest': tag[1:], 'tag': tag, 'commit': ref['sha'],
            'available': newer, 'url': url, 'delivery': 'source-build'}


def quality_failure(release):
    """Exact-commit, newest-run policy. Advisories remain separately reported."""
    commit = release.get('commit', '')
    if not isinstance(commit, str) or not SHA.fullmatch(commit):
        raise ValueError('Update lacks an immutable release commit')
    runs = api('/actions/runs?head_sha=' + commit + '&event=push&per_page=100').get('workflow_runs', [])
    matching = [run for run in runs if run.get('head_sha') == commit and run.get('head_branch') == 'main' and run.get('path') == '.github/workflows/quality.yml']
    if not matching:
        return 'No main-branch quality/privacy workflow for the release commit'
    newest = max(matching, key=lambda run: run['id'])
    jobs = api('/actions/runs/' + str(newest['id']) + '/jobs?per_page=100').get('jobs', [])
    pending = []
    for name in ['quality', 'privacy']:
        matching_jobs = [job for job in jobs if job.get('name') == name]
        if len(matching_jobs) != 1 or matching_jobs[0].get('conclusion') != 'success':
            conclusion = matching_jobs[0].get('conclusion') or matching_jobs[0].get('status') or 'pending' if matching_jobs else 'missing'
            pending.append(name + ': ' + conclusion)
    return ('Exact-commit run ' + str(newest['id']) + ' — ' + ', '.join(pending)) if pending else None


def automatic_release_verified(release):
    return quality_failure(release) is None


def release_readiness(release, delivery):
    """Publication eligibility; download hashes/provenance are checked at install."""
    reason = quality_failure(release)
    if reason:
        return reason
    data = api('/releases/tags/' + release['tag'])
    if data.get('tag_name') != release['tag'] or data.get('draft') is not False or data.get('prerelease') is not False:
        return 'Unexpected release metadata'
    # Supported Fedora releases are complete only once both delivery paths
    # exist. Source installation must not bypass a failed/incomplete release.
    names = ['ruwac-' + release['latest'] + '-fedora44-x86_64-testing.tar.gz',
             'ruwac-' + release['latest'] + '-1.fc44.x86_64.rpm',
             'ruwac-desktop-' + release['latest'] + '-1.fc44.x86_64.rpm']
    names += [name + '.sigstore.json' for name in names]
    for name in names:
        assets = [a for a in data.get('assets', []) if a.get('name') == name]
        if not assets:
            return 'Missing published asset: ' + name
        if len(assets) != 1:
            return 'Ambiguous published asset: ' + name
        asset = assets[0]
        expected = 'https://github.com/w3fans/RUWAC/releases/download/' + release['tag'] + '/' + name
        if (asset.get('state') != 'uploaded' or type(asset.get('size')) is not int
                or not 0 < asset['size'] <= (1024 * 1024 if name.endswith('.sigstore.json') else MAX_BUNDLE_BYTES)
                or asset.get('browser_download_url') != expected
                or not re.fullmatch(r'sha256:[0-9a-f]{64}', asset.get('digest', ''))):
            return 'Invalid published asset metadata: ' + name
    return None


def verified_status(current, delivery):
    """Keep publication, eligibility and installation separate, without downgrade."""
    published = latest(current)
    pending = []
    reason = release_readiness(published, delivery)
    chosen = published if reason is None else None
    if reason:
        pending.append({'tag': published['tag'], 'reason': reason})
        # Avoid resolving ten historical tags for the usual current-release
        # check. Fetch fallback identities only when the newest is ineligible.
        for candidate in newer_releases('0.0.0'):
            if candidate['tag'] == published['tag']:
                continue
            reason = release_readiness(candidate, delivery)
            if reason:
                pending.append({'tag': candidate['tag'], 'reason': reason})
            else:
                chosen = candidate
                break
    result = dict(chosen or published, current=current, installed_version=current,
                  latest_published=published['latest'], latest_verified=chosen['latest'] if chosen else None,
                  available=chosen is not None and version(chosen['latest']) > version(current),
                  delivery=delivery, skipped_releases=pending,
                  verification_scope='Exact-commit quality/privacy and complete published asset metadata; downloaded hashes and provenance are verified before installation')
    if chosen is None:
        result['skipped'] = 'No verified installable release: ' + (pending[0]['reason'] if pending else 'no candidates')
    return result


def installation_identity(prefix, running, executable=None):
    """Read-only: distinguish the running frontend from both installed routes."""
    user = installed_version(prefix, '0.0.0')
    rpm = subprocess.run(['/usr/bin/rpm', '-q', '--qf', '%{VERSION}', 'ruwac-desktop'],
                         capture_output=True, text=True, timeout=15) if Path('/usr/bin/rpm').exists() else None
    system = rpm.stdout.strip() if rpm and rpm.returncode == 0 else None
    if system:
        version(system)
    return {'running': running, 'running_executable': executable,
            'user_installed': user if user != '0.0.0' else None, 'system_installed': system,
            'mixed_installation': user != '0.0.0' and system is not None}


def installation_status(result, identity, managed_rpm=False):
    """An uninstalled running build must not masquerade as an installation."""
    installed = identity['system_installed' if managed_rpm else 'user_installed']
    result = dict(result, **identity, installed_version=installed)
    if installed is not None:
        result['current'] = installed
        eligible = result.get('latest_verified')
        result['available'] = isinstance(eligible, str) and version(eligible) > version(installed)
    return result


MAX_BUNDLE_BYTES = 128 * 1024 * 1024
MAX_UNPACKED_BYTES = 256 * 1024 * 1024


def testing_bundle_setting(enabled=None):
    if enabled is None:
        return preferences().get('testing_bundles', False)
    lock = locked_file(settings_path().parent / '.settings.lock')
    try:
        data = preferences()
        data['testing_bundles'] = bool(enabled)
        save_preferences(data)
    finally:
        os.close(lock)
    return bool(enabled)


def bundle_asset(release):
    data = api('/releases/tags/' + release['tag'])
    if data.get('tag_name') != release['tag'] or data.get('draft') is not False or data.get('prerelease') is not False:
        raise ValueError('Unexpected testing bundle release')
    name = 'ruwac-' + release['latest'] + '-fedora44-x86_64-testing.tar.gz'
    matches = [a for a in data.get('assets', []) if a.get('name') == name]
    if not matches:
        return None
    if len(matches) != 1:
        raise ValueError('Ambiguous testing bundle asset')
    asset = matches[0]
    expected_url = 'https://github.com/w3fans/RUWAC/releases/download/' + release['tag'] + '/' + name
    if (asset.get('state') != 'uploaded' or type(asset.get('size')) is not int
            or not 0 < asset['size'] <= MAX_BUNDLE_BYTES
            or asset.get('browser_download_url') != expected_url
            or not re.fullmatch(r'sha256:[0-9a-f]{64}', asset.get('digest', ''))):
        raise ValueError('Testing bundle asset lacks trusted size, SHA256 or URL')
    return asset


def asset_url_allowed(url):
    parsed = urllib.parse.urlparse(url)
    return (parsed.scheme == 'https' and parsed.hostname in {'github.com', 'release-assets.githubusercontent.com'}
            and parsed.port in {None, 443} and parsed.username is None and parsed.password is None and not parsed.fragment)


class AssetRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, message, headers, url):
        if not asset_url_allowed(url):
            raise ValueError('Untrusted testing bundle redirect')
        return super().redirect_request(request, fp, code, message, headers, url)


def download_bundle(asset, destination):
    if not asset_url_allowed(asset['browser_download_url']):
        raise ValueError('Untrusted testing bundle URL')
    opener = urllib.request.build_opener(AssetRedirect())
    request = urllib.request.Request(asset['browser_download_url'], headers={'User-Agent': 'RUWAC-updater'})
    started = time.monotonic()
    count, digest = 0, hashlib.sha256()
    with opener.open(request, timeout=20) as response, destination.open('xb') as output:
        if not asset_url_allowed(response.url):
            raise ValueError('Untrusted testing bundle response')
        while True:
            if time.monotonic() - started > 120:
                raise ValueError('Testing bundle download deadline exceeded')
            chunk = response.read(65536)
            if not chunk:
                break
            count += len(chunk)
            if count > asset['size'] or count > MAX_BUNDLE_BYTES:
                raise ValueError('Testing bundle download exceeds declared size')
            output.write(chunk)
            digest.update(chunk)
    if count != asset['size'] or digest.hexdigest() != asset['digest'][7:]:
        raise ValueError('Testing bundle SHA256 or size mismatch')


class BoundedArchive:
    def __init__(self, stream):
        self.stream, self.count = stream, 0

    def read(self, size=-1):
        data = self.stream.read(min(65536, size if size >= 0 else 65536))
        self.count += len(data)
        if self.count > MAX_UNPACKED_BYTES:
            raise ValueError('Testing bundle expanded stream exceeds limit')
        return data


def unpack_bundle(archive, destination, release):
    name = 'ruwac-' + release['latest'] + '-fedora44-x86_64-testing'
    names, total = set(), 0
    with gzip.open(archive, 'rb') as zipped, tarfile.open(fileobj=BoundedArchive(zipped), mode='r|') as source:
        for member in source:
            parts = member.name.split('/')
            if (len(names) >= 2000 or len(member.name) > 1024 or member.name in names
                    or parts[0] != name or any(p in {'', '.', '..'} for p in parts)
                    or '\\' in member.name or not (member.isfile() or member.isdir())
                    or member.size < 0 or member.size > MAX_BUNDLE_BYTES):
                raise ValueError('Unsafe testing bundle archive member')
            names.add(member.name)
            total += member.size
            if total > MAX_UNPACKED_BYTES:
                raise ValueError('Testing bundle extracted files exceed limit')
            source.extract(member, destination, filter='data')
    root = destination / name
    metadata = root / 'bundle.json'
    if not metadata.is_file() or metadata.stat().st_size > 1024 * 1024:
        raise ValueError('Missing or oversized testing bundle manifest')
    manifest = json.loads(metadata.read_text())
    if (manifest.get('source_commit') != release['commit'] or manifest.get('version') != release['latest']
            or manifest.get('kind') != 'fedora-testing-bundle' or manifest.get('schema') != 1
            or manifest.get('distribution') != 'fedora' or manifest.get('distribution_version') != '44'
            or manifest.get('architecture') != 'x86_64' or manifest.get('security_qualified') is not False
            or manifest.get('known_advisories') != ['RUSTSEC-2023-0071']):
        raise ValueError('Testing bundle manifest identity or qualification mismatch')
    files = manifest.get('files')
    actual = {str(p.relative_to(root)) for p in root.rglob('*') if p.is_file() and p != metadata}
    if not isinstance(files, dict) or set(files) != actual or 'install-user.py' not in files:
        raise ValueError('Testing bundle file inventory mismatch')
    for relative, expected in files.items():
        if not isinstance(expected, str) or not SHA256.fullmatch(expected) or hashlib.sha256((root / relative).read_bytes()).hexdigest() != expected:
            raise ValueError('Testing bundle file checksum mismatch')
    return root


SHA256 = re.compile(r'[0-9a-f]{64}\Z')


def verify_release_provenance(release, asset, archive, root, component=None):
    """Verify the downloaded bytes against their hosted release-build identity."""
    verifier = bundle_verifier()
    version(release['latest'])
    if release['tag'] != 'v' + release['latest'] or not SHA.fullmatch(release['commit']):
        raise ValueError('Invalid testing bundle source identity')
    data = api('/releases/tags/' + release['tag'])
    if (data.get('tag_name') != release['tag'] or data.get('draft') is not False
            or data.get('prerelease') is not False):
        raise ValueError('Unexpected testing bundle attestation release')
    if component is None:
        expected_name = 'ruwac-' + release['latest'] + '-fedora44-x86_64-testing.tar.gz'
    elif component in {'ruwac', 'ruwac-desktop'}:
        expected_name = component + '-' + release['latest'] + '-1.fc44.x86_64.rpm'
    else:
        raise ValueError('Unexpected release component')
    expected_url = 'https://github.com/w3fans/RUWAC/releases/download/' + release['tag'] + '/' + expected_name
    if asset.get('name') != expected_name or asset.get('browser_download_url') != expected_url:
        raise ValueError('Unexpected attestation subject')
    signatures = [a for a in data.get('assets', []) if a.get('name') == expected_name + '.sigstore.json']
    if len(signatures) != 1:
        raise ValueError('Testing bundle attestation is missing or ambiguous; installation held')
    signature = signatures[0]
    if (signature.get('state') != 'uploaded' or type(signature.get('size')) is not int
            or not 0 < signature['size'] <= 1024 * 1024
            or signature.get('browser_download_url') != expected_url + '.sigstore.json'
            or not re.fullmatch(r'sha256:[0-9a-f]{64}', signature.get('digest', ''))):
        raise ValueError('Invalid testing bundle attestation asset')
    bundle = root / 'attestation.sigstore.json'
    download_bundle(signature, bundle)
    config = root / 'verifier-config'
    config.mkdir(mode=0o700)
    env = {key: value for key, value in os.environ.items()
           if key not in {'GH_TOKEN', 'GITHUB_TOKEN', 'GH_ENTERPRISE_TOKEN', 'GITHUB_ENTERPRISE_TOKEN',
                          'GH_HOST', 'GH_REPO', 'GH_DEBUG'}}
    env['GH_CONFIG_DIR'] = str(config)
    args = [verifier, 'attestation', 'verify', str(archive), '--bundle', str(bundle),
            '--repo', 'w3fans/RUWAC', '--signer-workflow', 'w3fans/RUWAC/.github/workflows/release.yml',
            '--source-ref', 'refs/tags/' + release['tag'], '--source-digest', release['commit'],
            '--deny-self-hosted-runners']
    try:
        verified = subprocess.run(args, env=env, stdin=subprocess.DEVNULL,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        raise ValueError('Testing bundle attestation verifier unavailable or timed out; installation held') from None
    if verified.returncode != 0:
        raise ValueError('Testing bundle attestation verification failed; installation held')


def verify_bundle_provenance(release, asset, archive, root):
    return verify_release_provenance(release, asset, archive, root)


def bundle_verifier():
    verifier = shutil.which('gh')
    if verifier is None:
        raise ValueError('Verified RUWAC downloads require GitHub CLI (gh). '
                         'On this Fedora host run: sudo dnf install gh; then retry. '
                         'A prebuilt installation does not require Cargo. No installation was changed.')
    return verifier


def rpm_operation(operation):
    helper = Path('/usr/bin/ruwac-rpm-update')
    if not helper.exists() or helper.resolve() != Path('/usr/share/ruwac/rpm-update.py'):
        raise ValueError('RPM installation is managed by DNF; install a release with the verified system updater first')
    info = helper.resolve().lstat()
    if not stat.S_ISREG(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
        raise ValueError('Unsafe system RPM updater; installation held')
    for directory in [Path('/usr'), Path('/usr/bin'), Path('/usr/share'), helper.resolve().parent]:
        info = directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise ValueError('Unsafe system RPM updater directory; installation held')
    commands = {'status': '--status', 'on': '--enable-testing', 'off': '--disable', 'install': '--install'}
    if operation not in commands:
        raise ValueError('Unsupported system RPM update operation')
    args = [str(helper), commands[operation]]
    if operation != 'status' and os.geteuid() != 0:
        args.insert(0, '/usr/bin/pkexec')
    result = subprocess.run(args, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                            text=True, timeout=2200)
    if result.returncode or len(result.stdout) > 32768:
        raise ValueError('Administrator authorization or verified RPM update failed; running sessions remain open')
    value = json.loads(result.stdout)
    if not isinstance(value, dict) or value.get('distribution') != 'rpm':
        raise ValueError('Unexpected system RPM updater response')
    return value


def install_testing_bundle(release, asset, prefix, cache, automatic, desktop=True, prebuilt=False):
    # Diagnose prerequisites before downloading an archive or taking an install lock.
    bundle_verifier()
    lock = locked_file(cache / '.update.lock')
    root = Path(tempfile.mkdtemp(prefix='testing-', dir=cache))
    try:
        archive = root / 'bundle.tar.gz'
        download_bundle(asset, archive)
        verify_bundle_provenance(release, asset, archive, root)
        bundle = unpack_bundle(archive, root / 'unpacked', release)
        if version(installed_version(prefix, '0.0.0')) > version(release['latest']):
            raise ValueError('A newer installation appeared; refusing bundle downgrade')
        preference_lock = locked_file(settings_path().parent / '.settings.lock')
        try:
            if not (prebuilt or testing_bundle_setting()) or (automatic and not automatic_install_setting()):
                return dict(release, skipped='testing bundle or automatic installation disabled during download')
            expected = installation_artifacts(bundle / 'target/release', desktop)
            run(['python3', str(bundle / 'install-user.py'), '--build-root', str(bundle), '--prefix', str(prefix)]
                + (['--desktop'] if desktop else []))
            result = confirm_installation(release, prefix, expected)
        finally:
            os.close(preference_lock)
        return dict(result, delivery='testing-bundle')
    finally:
        shutil.rmtree(root)
        os.close(lock)


def settings_path():
    root = Path(os.environ.get('XDG_CONFIG_HOME', Path.home() / '.config')) / 'RUWAC'
    return private_directory(root) / 'updates.json'


def preferences():
    path = settings_path()
    data = {'schema': 1, 'automatic_check': True, 'automatic_install': False, 'testing_bundles': False}
    if path.exists() or path.is_symlink():
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077 or info.st_size > 4096:
            raise ValueError('Unsafe update preferences; preserve and repair them')
        data = json.loads(path.read_text())
        if data.get('schema') != 1 or not isinstance(data.get('automatic_check'), bool) or not isinstance(data.get('automatic_install', False), bool) or not isinstance(data.get('testing_bundles', False), bool):
            raise ValueError('Unknown update preferences; preserve and repair them')
    return data


def save_preferences(data):
    path = settings_path()
    encoded = json.dumps(data) + '\n'
    if len(encoded.encode()) > 4096:
        raise ValueError('Update preferences exceed the size limit')
    fd, name = tempfile.mkstemp(prefix='.updates-', dir=path.parent)
    try:
        with os.fdopen(fd, 'w') as stream:
            stream.write(encoded); stream.flush(); os.fsync(stream.fileno())
        Path(name).replace(path)
    finally:
        Path(name).unlink(missing_ok=True)


def automatic_setting(value=None):
    if value is None:
        return preferences()['automatic_check']
    lock = locked_file(settings_path().parent / '.settings.lock')
    try:
        data = preferences(); data['automatic_check'] = value
        save_preferences(data)
        return value
    finally:
        os.close(lock)


UNIT_MARKER = '# Managed by RUWAC automatic updates v1\n'
UNIT_NAMES = ('ruwac-update.service', 'ruwac-update.timer')


def unit_quote(value):
    if any(ord(c) < 32 or ord(c) == 127 for c in value):
        raise ValueError('Update paths cannot contain control characters')
    return '\"' + value.replace('\\', '\\\\').replace('\"', '\\\"').replace('%', '%%') + '\"'


def update_units(prefix):
    current = installed_version(prefix, '0.0.0')
    if current == '0.0.0' or not (prefix / 'bin/ruwac').is_file():
        raise ValueError('Install RUWAC before enabling unattended updates')
    binary = prefix / 'bin/ruwac'
    if not binary.is_symlink() or str(binary.readlink()) != '../lib/ruwac/current/ruwac':
        raise ValueError('Unrelated RUWAC command; automatic-update configuration preserved')
    cache = Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')).absolute()
    config = settings_path().parent.parent.absolute()
    service = UNIT_MARKER + '[Unit]\nDescription=RUWAC unattended source update\n\n[Service]\nType=oneshot\n'
    # ':' disables shell-style environment expansion; percent specifiers are escaped.
    service += 'ExecStart=:' + unit_quote(str(prefix / 'bin/ruwac')) + ' update --install-auto --prefix ' + unit_quote(str(prefix)) + '\n'
    service += 'Environment=' + unit_quote('XDG_CONFIG_HOME=' + str(config)) + '\n'
    service += 'Environment=' + unit_quote('XDG_CACHE_HOME=' + str(cache)) + '\n'
    service += 'UMask=0077\nNice=10\nIOSchedulingClass=idle\nCPUWeight=20\nNoNewPrivileges=yes\nTimeoutStartSec=2h\n'
    timer = UNIT_MARKER + '[Unit]\nDescription=Check for RUWAC updates every six hours\n\n[Timer]\nOnActiveSec=5min\nOnUnitActiveSec=6h\nRandomizedDelaySec=5min\nUnit=' + UNIT_NAMES[0] + '\n\n[Install]\nWantedBy=timers.target\n'
    return dict(zip(UNIT_NAMES, (service, timer)))


def scheduler_directory():
    # The directory can contain other application units: retain existing permissions.
    base = settings_path().parent.parent / 'systemd'
    for directory in [base, base / 'user']:
        directory.mkdir(mode=0o700, exist_ok=True)
        info = directory.lstat()
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError('Unsafe user service directory; existing configuration preserved')
    return base / 'user'


def automatic_install_setting(value=None, prefix=None):
    if value is None:
        return preferences().get('automatic_install', False)
    prefix = (prefix or Path.home() / '.local').absolute()
    lock = locked_file(settings_path().parent / '.settings.lock')
    try:
        data = preferences()
        if value is False:
            # Commit opt-out first. In-flight builds recheck it before installation.
            data['automatic_install'] = False; save_preferences(data)
            expected = data.get('automatic_install_units_sha256', {})
            if not expected:
                return False
            directory = scheduler_directory()
            for name in UNIT_NAMES:
                path = directory / name
                if path.is_symlink() or not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected.get(name):
                    raise ValueError('Automatic installation disabled; edited scheduler configuration was preserved')
            run(['systemctl', '--user', 'disable', '--now', UNIT_NAMES[1]])
            return False
        generated = update_units(prefix)
        directory = scheduler_directory()
        expected = data.get('automatic_install_units_sha256', {})
        before = {}
        for name in UNIT_NAMES:
            path = directory / name
            if path.exists() or path.is_symlink():
                info = path.lstat()
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077 or info.st_size > 8192:
                    raise ValueError('Unsafe automatic-update unit; existing configuration preserved')
                raw = path.read_bytes()
                if expected.get(name) != hashlib.sha256(raw).hexdigest():
                    raise ValueError('Automatic-update unit was edited or belongs to another tool; preserved')
                before[name] = raw
            else:
                before[name] = None
        try:
            for name, content in generated.items():
                fd, temporary = tempfile.mkstemp(prefix='.ruwac-unit-', dir=directory)
                try:
                    with os.fdopen(fd, 'w') as stream:
                        stream.write(content); stream.flush(); os.fsync(stream.fileno())
                    Path(temporary).replace(directory / name)
                finally:
                    Path(temporary).unlink(missing_ok=True)
            run(['systemctl', '--user', 'daemon-reload'])
            run(['systemctl', '--user', 'enable', '--now', UNIT_NAMES[1]])
            data['automatic_install'] = True
            data['automatic_install_units_sha256'] = {name: hashlib.sha256(content.encode()).hexdigest() for name, content in generated.items()}
            save_preferences(data)
        except Exception:
            for name, raw in before.items():
                path = directory / name
                if raw is None:
                    path.unlink(missing_ok=True)
                else:
                    path.write_bytes(raw); path.chmod(0o600)
            # Restoring the old units is best effort; original failure is retained.
            subprocess.run(['systemctl', '--user', 'daemon-reload'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            raise
        return True
    finally:
        os.close(lock)


def run(command, *, cwd=None, env=None):
    subprocess.run(command, cwd=cwd, env=env, check=True, stdout=2, stderr=2)


def checkout(release, root, repository=REPOSITORY):
    # No global Git aliases, URL rewrites, hooks, credentials or SSH transport.
    env = dict(os.environ, GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT='0')
    for key in list(env):
        if key.startswith('GIT_CONFIG_') and key not in {'GIT_CONFIG_NOSYSTEM', 'GIT_CONFIG_GLOBAL'}:
            del env[key]
        elif key.startswith('GIT_') and key not in {'GIT_CONFIG_NOSYSTEM', 'GIT_CONFIG_GLOBAL', 'GIT_TERMINAL_PROMPT'}:
            del env[key]
    run(['git', '-c', 'init.templateDir=', 'init', str(root)], env=env)
    run(['git', '-c', 'core.hooksPath=/dev/null', '-C', str(root), 'fetch', '--depth=1',
         repository, 'refs/tags/' + release['tag']], env=env)
    found = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'FETCH_HEAD^{commit}'], env=env, text=True).strip()
    if found != release['commit']:
        raise ValueError('Release tag changed during download; nothing installed')
    run(['git', '-c', 'core.hooksPath=/dev/null', '-C', str(root), 'checkout', '--detach', found], env=env)
    for path in ['ruwac/Cargo.toml', 'ruwac/desktop/Cargo.toml']:
        data = tomllib.loads((root / path).read_text())
        package = data.get('workspace', {}).get('package', data.get('package', {}))
        if package.get('version') != release['latest']:
            raise ValueError('Source version differs from GitHub release')


def locked_file(path):
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    info = os.fstat(fd)
    if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_nlink != 1 or info.st_mode & 0o077:
        os.close(fd)
        raise ValueError('Unsafe update lock')
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except Exception:
        os.close(fd)
        raise ValueError('Another RUWAC update or installation is active')
    return fd


def installed_version(prefix, fallback):
    for path in [prefix, prefix / 'lib', prefix / 'lib/ruwac', prefix / 'lib/ruwac/releases']:
        if path.exists() or path.is_symlink():
            if path.is_symlink() or not path.is_dir() or path.stat().st_uid != os.geteuid():
                raise ValueError('Unsafe per-user installation path')
    current = prefix / 'lib/ruwac/current'
    if not current.exists() and not current.is_symlink():
        return fallback
    if not current.is_symlink():
        raise ValueError('Unsafe current installation link')
    target = str(current.readlink())
    if not target.startswith('releases/') or len(Path(target).parts) != 2:
        raise ValueError('Unsafe current release target')
    installed = Path(target).name
    version(installed)
    receipt = current / 'installation.json'
    info = receipt.lstat()
    if (current.parent / target).is_symlink() or not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077 or info.st_size > 4096:
        raise ValueError('Unsafe current installation receipt')
    if json.loads(receipt.read_text())['version'] != installed:
        raise ValueError('Current installation receipt mismatch')
    return installed


def artifact_digest(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= 256 * 1024 * 1024:
            raise ValueError('Unsafe installed artifact size or type')
        digest = hashlib.sha256()
        remaining = info.st_size
        while remaining:
            block = os.read(fd, min(remaining, 1024 * 1024))
            if not block:
                raise ValueError('Artifact changed during verification')
            digest.update(block)
            remaining -= len(block)
        after = os.fstat(fd)
        if os.read(fd, 1) or any(getattr(after, field) != getattr(info, field)
                                for field in ['st_dev', 'st_ino', 'st_size', 'st_mtime_ns', 'st_ctime_ns']):
            raise ValueError('Artifact changed during verification')
        return digest.hexdigest()
    finally:
        os.close(fd)


def installation_artifacts(folder, desktop):
    return {name: artifact_digest(folder / name)
            for name in ['ruwac'] + (['ruwac-desktop'] if desktop else [])}


def confirm_installation(release, prefix, expected):
    if installed_version(prefix, '0.0.0') != release['latest']:
        raise ValueError('Installer finished without installing the selected version')
    current = prefix / 'lib/ruwac/current'
    receipt = json.loads((current / 'installation.json').read_text())
    if receipt.get('source_commit') != release['commit'] or receipt.get('artifacts') != expected:
        raise ValueError('Installed receipt differs from the selected release')
    for name, digest in expected.items():
        link = prefix / 'bin' / name
        if (not link.is_symlink() or str(link.readlink()) != '../lib/ruwac/current/' + name
                or artifact_digest(current / name) != digest):
            raise ValueError('Installed command differs from the verified artifact')
    return dict(release, installed=release['latest'], installed_version=release['latest'],
                user_installed=release['latest'], current=release['latest'], available=False,
                running_sessions='preserved')


def rollback(prefix):
    base = prefix / 'lib/ruwac'
    if base.is_symlink() or not base.is_dir() or base.stat().st_uid != os.geteuid():
        raise ValueError('No owned per-user installation')
    lock = locked_file(base / '.install.lock')
    try:
        links = []
        for name in ['current', 'previous']:
            link = base / name
            if not link.is_symlink():
                raise ValueError('No reversible previous installation')
            target = str(link.readlink())
            if not target.startswith('releases/') or len(Path(target).parts) != 2:
                raise ValueError('Unsafe release link')
            version(Path(target).name)
            folder = base / target
            if (base / 'releases').is_symlink() or folder.is_symlink() or folder.stat().st_uid != os.geteuid():
                raise ValueError('Unsafe release directory')
            receipt = folder / 'installation.json'
            info = receipt.lstat()
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o077 or info.st_size > 4096:
                raise ValueError('Unsafe installation receipt')
            manifest = json.loads(receipt.read_text())
            if manifest['version'] != Path(target).name or set(manifest['artifacts']) not in [{'ruwac'}, {'ruwac', 'ruwac-desktop'}]:
                raise ValueError('Invalid installation receipt')
            for name, digest in manifest['artifacts'].items():
                binary = folder / name
                info = binary.lstat()
                if not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid() or info.st_mode & 0o022 or hashlib.sha256(binary.read_bytes()).hexdigest() != digest:
                    raise ValueError('Installed artifact changed; refusing rollback')
            links.append((target, manifest))
        if set(links[0][1]['artifacts']) != set(links[1][1]['artifacts']):
            raise ValueError('Rollback must retain the same installed components')
        for name, target in [('previous', links[0][0]), ('current', links[1][0])]:
            temp = base / ('.' + name + '-update')
            if temp.exists() or temp.is_symlink():
                raise ValueError('Rollback conflict; preserve existing paths')
        # A deliberate rollback must survive the next automatic timer expiry.
        # Persist opt-out before swapping artifacts, while serializing consent
        # changes. No service-manager availability is required for recovery.
        preference_lock = locked_file(settings_path().parent / '.settings.lock')
        try:
            data = preferences()
            data['automatic_install'] = False
            save_preferences(data)
            for name, target in [('previous', links[0][0]), ('current', links[1][0])]:
                temp = base / ('.' + name + '-update')
                temp.symlink_to(target); temp.replace(base / name)
        finally:
            os.close(preference_lock)
        return {'installed': links[1][1]['version'], 'previous': links[0][1]['version'], 'automatic_install': False, 'running_sessions': 'preserved'}
    finally:
        os.close(lock)


def install(release, prefix, cache, desktop=True, automatic=False, prebuilt=False):
    reason = release_readiness(release, 'testing-bundle' if prebuilt or (desktop and testing_bundle_setting()) else 'source-build')
    if reason:
        return dict(release, skipped=reason, available=False, running_sessions='preserved')
    os_release = Path('/etc/os-release').read_text()
    os_info = dict(line.split('=', 1) for line in os_release.splitlines() if '=' in line)
    if os_info.get('ID', '').strip('"') != 'fedora' or os_info.get('VERSION_ID', '').strip('"') != '44':
        raise ValueError('This source updater is currently validated for Fedora 44')
    # `prebuilt` (remote CLI installation) uses the verified bundle when one
    # exists for this platform, so the host needs no compiler toolchain.
    if prebuilt and platform.machine() == 'x86_64':
        asset = bundle_asset(release)
        if asset is not None and automatic_release_verified(release):
            return install_testing_bundle(release, asset, prefix, cache, automatic, desktop, prebuilt=True)
    if desktop and testing_bundle_setting():
        if platform.machine() != 'x86_64':
            raise ValueError('Fedora testing bundles currently require x86_64; select source builds on other architectures')
        asset = bundle_asset(release)
        if asset is None or not automatic_release_verified(release):
            return dict(release, delivery='testing-bundle', skipped='waiting for the Fedora testing bundle and successful exact-commit quality/privacy checks')
        return install_testing_bundle(release, asset, prefix, cache, automatic)
    for tool in ['git', 'cargo', 'cc', 'pkg-config', 'python3']:
        if not shutil.which(tool):
            raise ValueError('Missing build dependency: ' + tool + '; no verified prebuilt bundle is available for this release and platform; see Fedora update instructions')
    if desktop:
        # libssh2-sys already builds its bundled library when system headers are absent.
        for package in ['gtk4', 'libadwaita-1', 'openssl', 'sqlite3', 'webkitgtk-6.0']:
            run(['pkg-config', '--exists', package])
    lock = locked_file(cache / '.update.lock')
    root = Path(tempfile.mkdtemp(prefix='source-', dir=cache))
    try:
        checkout(release, root)
        target = private_directory(cache / 'target')
        cargo = private_directory(cache / 'cargo')
        (root / 'target').symlink_to(target)
        env = dict(os.environ, CARGO_TARGET_DIR=str(target), CARGO_HOME=str(cargo), CARGO_BUILD_JOBS='2')
        # User installation builds published source, not the private publication controller.
        for manifest in ['ruwac/Cargo.toml'] + (['ruwac/desktop/Cargo.toml'] if desktop else []):
            run(['cargo', 'build', '--release', '--locked', '--manifest-path', str(root / manifest)], env=env)
        if version(installed_version(prefix, '0.0.0')) > version(release['latest']):
            raise ValueError('A newer installation appeared during this build; refusing a downgrade')
        args = ['python3', str(root / 'scripts/install-user.py'), '--build-root', str(root), '--prefix', str(prefix)]
        if desktop:
            args.append('--desktop')
        preference_lock = locked_file(settings_path().parent / '.settings.lock') if automatic else None
        try:
            if automatic and not automatic_install_setting():
                return dict(release, skipped='automatic updates disabled during build', running_sessions='preserved')
            expected = installation_artifacts(target / 'release', desktop)
            run(args)
            result = confirm_installation(release, prefix, expected)
        finally:
            if preference_lock is not None:
                os.close(preference_lock)
        return result
    finally:
        shutil.rmtree(root)
        os.close(lock)


def remote_cli_version(binary):
    """Inspect the selected SSH executable, with the same user-local fallback."""
    executable = shutil.which(binary)
    if executable is None and binary == 'ruwac':
        candidate = Path.home() / '.local/bin/ruwac'
        if candidate.is_file() and os.access(candidate, os.X_OK):
            executable = str(candidate)
    if executable is None:
        return '0.0.0'
    result = subprocess.run([executable, '--version'], capture_output=True, text=True, timeout=5)
    match = re.fullmatch(r'RUWAC ([0-9]+\.[0-9]+\.[0-9]+)\s*', result.stdout)
    if result.returncode or not match:
        raise ValueError('Selected remote RUWAC executable has an invalid version')
    version(match[1])
    return match[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    modes = parser.add_mutually_exclusive_group(required=True)
    modes.add_argument('-c', '--check', action='store_true')
    modes.add_argument('-C', '--check-auto', action='store_true')
    modes.add_argument('-i', '--install', action='store_true')
    modes.add_argument('-b', '--bootstrap', action='store_true', help='Build/install the latest published version when no CLI is installed')
    parser.add_argument('-p', '--prebuilt', action='store_true', help='Prefer the verified prebuilt Fedora bundle over a source build')
    modes.add_argument('-I', '--install-auto', action='store_true')
    modes.add_argument('-a', '--auto-install', choices=['on', 'off', 'status'])
    modes.add_argument('-r', '--rollback', action='store_true')
    modes.add_argument('-A', '--auto-check', choices=['on', 'off', 'status'])
    modes.add_argument('-t', '--testing-bundles', choices=['on', 'off', 'status'])
    parser.add_argument('-v', '--current', default='0.0.0')
    parser.add_argument('-R', '--remote-cli', help='Check the executable selected for a remote agent before a verified per-user update')
    parser.add_argument('-P', '--prefix', type=Path, default=Path.home() / '.local')
    parser.add_argument('-H', '--headless', action='store_true')
    parser.add_argument('-e', '--running-executable', help=argparse.SUPPRESS)
    parser.add_argument('-m', '--managed-rpm', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    if not args.bootstrap and args.current == '0.0.0':
        parser.error('--current is required except for bootstrap installation')
    version(args.current)
    if args.remote_cli:
        if not args.bootstrap or args.managed_rpm:
            parser.error('--remote-cli requires per-user bootstrap mode')
        args.current = max(args.current, remote_cli_version(args.remote_cli), key=version)
    if args.managed_rpm:
        if args.auto_install:
            result = rpm_operation(args.auto_install)
        elif args.install:
            result = rpm_operation('install')
        elif args.check:
            query = subprocess.run(['/usr/bin/rpm', '-q', '--qf', '%{VERSION}', 'ruwac-desktop'],
                                   capture_output=True, text=True, timeout=15)
            if query.returncode:
                raise ValueError('Unable to determine the installed desktop RPM version')
            installed = query.stdout.strip()
            version(installed)
            result = verified_status(installed, 'testing-rpm')
            result = installation_status(result, installation_identity(args.prefix.absolute(), args.current, args.running_executable), managed_rpm=True)
            result.update(distribution='rpm', running=args.current,
                          delivery='testing-rpm', update_method='Verified system updater or DNF',
                          updates_url='https://github.com/w3fans/RUWAC/releases/tag/' + result['tag'])
        elif args.check_auto or args.install_auto:
            result = {'distribution': 'rpm', 'available': False, 'automatic_install': False,
                      'automatic_check': False, 'skipped': 'RPM updates are managed by DNF'}
        elif args.auto_check == 'status' or args.testing_bundles == 'status':
            result = {'distribution': 'rpm', 'automatic_install': False,
                      'automatic_check': False, 'testing_bundles': False}
        else:
            raise ValueError('RPM installation is managed by DNF. Download the verified release RPMs and install with DNF; per-user installations and timers are unchanged.')
        print(json.dumps(result))
        return
    prefix = args.prefix.absolute()
    if args.testing_bundles:
        result = {'testing_bundles': testing_bundle_setting({'on': True, 'off': False}.get(args.testing_bundles))}
    elif args.auto_install:
        result = {'automatic_install': automatic_install_setting({'on': True, 'off': False}.get(args.auto_install), prefix)}
    elif args.install_auto and not automatic_install_setting():
        result = {'automatic_install': False, 'available': False, 'skipped': 'automatic installation disabled'}
    elif args.auto_check:
        result = {'automatic_check': automatic_setting({'on': True, 'off': False}.get(args.auto_check))}
    elif args.check_auto and not automatic_setting():
        result = {'automatic_check': False, 'available': False}
    elif args.rollback:
        installed_version(prefix, args.current)
        cache = private_directory(Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'RUWAC/updates')
        lock = locked_file(cache / '.update.lock')
        try:
            result = rollback(prefix)
        finally:
            os.close(lock)
    else:
        installed = installed_version(prefix, '0.0.0' if args.bootstrap else args.current)
        if args.remote_cli:
            installed = max(installed, args.current, key=version)
        delivery = 'testing-bundle' if args.prebuilt or (not args.headless and testing_bundle_setting()) else 'source-build'
        result = verified_status(installed, delivery)
        result = installation_status(result, installation_identity(prefix, args.current, args.running_executable))
        if 'skipped' not in result and (args.install or args.install_auto or args.bootstrap) and result['available']:
            cache = private_directory(Path(os.environ.get('XDG_CACHE_HOME', Path.home() / '.cache')) / 'RUWAC/updates')
            # --headless installs only the CLI on a fresh machine; an existing
            # per-user desktop is updated with it rather than refused.
            desktop = not args.headless or (prefix / 'bin/ruwac-desktop').is_symlink()
            if args.install_auto:
                receipt = prefix / 'lib/ruwac/current/installation.json'
                artifacts = json.loads(receipt.read_text()).get('artifacts', {})
                if set(artifacts) not in [{'ruwac'}, {'ruwac', 'ruwac-desktop'}]:
                    raise ValueError('Unknown installed components; automatic update refused')
                desktop = 'ruwac-desktop' in artifacts
            result = install(result, prefix, cache, desktop, automatic=args.install_auto, prebuilt=args.prebuilt)
    print(json.dumps(result))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        print(json.dumps({'error': str(error)}))
        raise SystemExit(1)
