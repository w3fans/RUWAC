"""Explicit local GNOME companion management; packet/resources come from RUWAC.

No network download, Shell restart, clipboard access or automatic account consent.
The caller owns the private stage and bounds this process and its descendants.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import types

UUID = 'primary@w3fans.github.io'
SCHEMA = 'org.gnome.shell.extensions.ruwac.primary'
FILES = {'extension.js', 'prefs.js', 'metadata.json', 'consent_gate.js',
         'consent_state.js', 'consent_preferences.js', 'enrollment.js',
         'lease_receiver.js', 'socket_guard.js', 'README.md', 'LICENSE', 'NOTICE',
         'schemas/'+SCHEMA+'.gschema.xml'}
INSTALLED = FILES | {'schemas/gschemas.compiled'}


class SetupError(ValueError):
    pass


def private_dir(path):
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise SetupError('An owned private setup directory is required')


def safe_dir(path):
    if path.is_symlink():
        raise SetupError('A setup directory is a symlink; existing files were preserved')
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o022:
        raise SetupError('Setup directory ownership or permissions are unsafe')


def regular(path, maximum=262144):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or info.st_mode & 0o022 or info.st_size > maximum:
            raise SetupError('An existing companion file is unsafe; it was preserved')
        value = stream.read(maximum+1)
        if len(value) > maximum:
            raise SetupError('Existing companion file exceeds its bound')
        return value


def snapshot(path):
    if path.is_symlink() or not path.is_dir() or path.stat().st_uid != os.getuid():
        raise SetupError('Existing extension ownership is unsafe; it was preserved')
    entries = list(path.rglob('*'))
    if len(entries) > 32 or any(p.is_symlink() for p in entries):
        raise SetupError('Existing extension contains unexpected entries; it was preserved')
    actual = {str(p.relative_to(path)) for p in entries if not p.is_dir()}
    if actual != INSTALLED:
        raise SetupError('Existing extension inventory differs; it was preserved')
    return {name: hashlib.sha256(regular(path/name)).hexdigest() for name in sorted(actual)}


def run_tool(args, env=None):
    process = subprocess.Popen(args, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=env, umask=0o077)
    try:
        if process.wait(timeout=15) != 0:
            raise SetupError('GNOME extension command failed; inspect GNOME Extensions')
    finally:
        if process.poll() is None:
            process.kill()
        process.wait()


def desktop(packet):
    # Only read this process's explicitly selected desktop; do not pick a socket
    # or another user's session. The bus peer and compositor listener are pinned.
    runtime = Path(os.environ.get('XDG_RUNTIME_DIR', ''))
    display = os.environ.get('WAYLAND_DISPLAY', '')
    if not runtime.is_absolute() or not re.fullmatch(r'wayland-[0-9]{1,3}', display):
        raise SetupError('Run setup from the selected GNOME Wayland desktop')
    private_dir(runtime)
    module = types.ModuleType('ruwac_setup_identity')
    exec(compile(packet['identity_source'], '<ruwac-setup-identity>', 'exec'), module.__dict__)
    from gi.repository import Gio, GLib
    loop = GLib.MainLoop(); cancel = Gio.Cancellable(); result = {'waiting': True}

    def done(_source, outcome):
        try:
            connection = Gio.DBusConnection.new_for_address_finish(outcome)
            if result['waiting']:
                result['connection'] = connection
            else:
                connection.close_sync(None)
        except GLib.Error:
            pass
        loop.quit()

    def timeout():
        result['waiting'] = False; cancel.cancel(); loop.quit()
        return False

    timer = GLib.timeout_add(2500, timeout)
    Gio.DBusConnection.new_for_address('unix:path='+str(runtime/'bus'),
        Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT | Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION,
        None, cancel, done)
    loop.run()
    if result['waiting']:
        GLib.source_remove(timer)
    result['waiting'] = False
    connection = result.get('connection')
    if connection is None:
        raise SetupError('Selected desktop bus is unavailable')
    try:
        identity = module.identify(connection, runtime, display)
        def call(interface, method, params=None):
            return connection.call_sync(identity['owner'], '/org/gnome/Shell', interface,
                method, params, None, Gio.DBusCallFlags.NONE, 1000, None)
        version = call('org.freedesktop.DBus.Properties', 'Get',
            GLib.Variant('(ss)', ('org.gnome.Shell.Extensions', 'ShellVersion'))).unpack()[0]
        if not isinstance(version, str) or version.split('.')[0] != '50':
            raise SetupError('Native selection support currently requires GNOME 50')
        settings = Gio.Settings.new('org.gnome.shell')
        # Prevent a GUI launched with another HOME/data namespace from installing
        # outside the selected compositor's actual startup data directory.
        with Path('/proc', str(identity['pid']), 'environ').open('rb') as stream:
            raw = stream.read(131073)
        if len(raw) > 131072:
            raise SetupError('Desktop environment metadata exceeds the setup bound')
        wanted = {b'HOME', b'XDG_DATA_HOME'}
        shell_env = {k.decode(): v.decode('utf-8', errors='strict')
                     for item in raw.split(b'\0') if b'=' in item
                     for k, v in [item.split(b'=', 1)] if k in wanted}
        data = Path(os.environ.get('XDG_DATA_HOME', str(Path.home()/'.local/share')))
        selected = Path(shell_env.get('XDG_DATA_HOME', str(Path(shell_env.get('HOME', ''))/'.local/share')))
        if not data.is_absolute() or data.resolve() != selected.resolve():
            raise SetupError('Run setup with the selected GNOME desktop data directory')
        module.identify(connection, runtime, display, expected=identity)
        return connection, call, settings, runtime, data
    except Exception:
        connection.close_sync(None)
        raise


def operate(packet, action, stage):
    if (type(packet) is not dict or set(packet) != {'protocol', 'version', 'files', 'identity_source'}
        or type(packet['protocol']) is not int or packet['protocol'] != 1
        or not isinstance(packet['version'], str) or not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', packet['version'])
        or type(packet['files']) is not dict or set(packet['files']) != FILES
        or any(not isinstance(value, str) for value in packet['files'].values())
        or not isinstance(packet['identity_source'], str) or len(packet['identity_source']) > 32768):
        raise SetupError('Invalid bundled companion resources')
    private_dir(stage)
    connection, call, settings, _runtime, data = desktop(packet)
    from gi.repository import GLib
    try:
        if action == 'install' and not settings.get_boolean('allow-extension-installation'):
            raise SetupError('User extension installation is disabled by this desktop policy')
        target = data/'gnome-shell/extensions'/UUID
        state = Path(os.environ.get('XDG_STATE_HOME', str(Path.home()/'.local/state')))/'ruwac'
        if not state.is_absolute():
            raise SetupError('The application state directory must be absolute')
        safe_dir(state)
        lock = os.open(state/'primary-setup.lock', os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        try:
            info = os.fstat(lock)
            if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or info.st_mode & 0o077:
                raise SetupError('Companion setup lock is unsafe')
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            receipt = state/'primary-companion.json'
            managed = json.loads(regular(receipt).decode()) if receipt.exists() or receipt.is_symlink() else None
            exists = target.exists() or target.is_symlink()
            if exists and (type(managed) is not dict or managed.get('schema') != 1 or managed.get('uuid') != UUID or managed.get('files') != snapshot(target)):
                raise SetupError('Existing or modified extension is unmanaged; it was preserved')
            def extension_info():
                return call('org.gnome.Shell.Extensions', 'GetExtensionInfo', GLib.Variant('(s)', (UUID,))).unpack()[0]
            info = extension_info()
            if info.get('path') and Path(info['path']).resolve() != target.resolve():
                raise SetupError('Another companion installation is active; manage it with GNOME Extensions')
            if action == 'status':
                return dict(installed=exists, state='not-installed' if not exists else
                            'active' if info.get('state') == 1 and info.get('version-name') == managed.get('version') else 'login-or-enable-required', uuid=UUID)
            if action == 'preferences':
                if not exists or not info or info.get('version-name') != managed.get('version'):
                    raise SetupError('Start a new GNOME login after installation, then open Desktop permission')
                call('org.gnome.Shell.Extensions', 'OpenExtensionPrefs', GLib.Variant('(ssa{sv})', (UUID, '', {})))
                return dict(state='preferences-opened', uuid=UUID)
            if action == 'uninstall':
                if exists:
                    if info:
                        run_tool(['gnome-extensions', 'uninstall', UUID])
                    else:
                        # A newly copied extension may not be known to the live
                        # Shell yet. Only the exact managed inventory is removed.
                        snapshot(target)
                        shutil.rmtree(target)
                        enabled = settings.get_strv('enabled-extensions')
                        settings.set_strv('enabled-extensions', [item for item in enabled if item != UUID])
                    if target.exists():
                        raise SetupError('GNOME did not remove the managed companion')
                if receipt.exists():
                    receipt.unlink()
                return dict(installed=False, state='removed', uuid=UUID)
            files = dict(packet['files'])
            metadata = json.loads(files['metadata.json'])
            if metadata.get('uuid') != UUID or metadata.get('shell-version') != ['50'] or metadata.get('settings-schema') != SCHEMA:
                raise SetupError('Bundled companion identity is invalid')
            metadata['version-name'] = packet['version']
            files['metadata.json'] = json.dumps(metadata, indent=2)+'\n'
            source_hashes = {name: hashlib.sha256(value.encode()).hexdigest() for name, value in files.items()}
            same = exists and managed.get('source_files') == source_hashes
            if exists and not same and managed.get('version') == packet['version']:
                raise SetupError('Different companion source has the same version; use a new release')
            if not same:
                source = stage/'source'; source.mkdir(mode=0o700)
                for name, value in files.items():
                    path = source/name; path.parent.mkdir(mode=0o700, exist_ok=True)
                    path.write_text(value); path.chmod(0o600)
                output = stage/'bundle'; output.mkdir(mode=0o700)
                (stage/'cache').mkdir(mode=0o700)
                extras = [arg for name in sorted(FILES) if '/' not in name and name not in {'extension.js', 'prefs.js', 'metadata.json'} for arg in ['--extra-source', name]]
                run_tool(['gnome-extensions', 'pack', '--force', '--out-dir', str(output), *extras,
                          '--schema', 'schemas/'+SCHEMA+'.gschema.xml', str(source)])
                safe_dir(data); safe_dir(data/'gnome-shell'); safe_dir(target.parent)
                # Official installation/compilation completes in an owned sibling
                # namespace first. Keep the verified old directory until both
                # the atomic replacement and receipt commit succeed.
                with tempfile.TemporaryDirectory(prefix='.ruwac-primary-', dir=target.parent) as temporary_root:
                    temporary_root = Path(temporary_root)
                    install_env = {**os.environ, 'XDG_DATA_HOME': str(temporary_root),
                                   'XDG_CACHE_HOME': str(stage/'cache')}
                    run_tool(['gnome-extensions', 'install', str(output/(UUID+'.shell-extension.zip'))], env=install_env)
                    prepared = temporary_root/'gnome-shell/extensions'/UUID
                    hashes = snapshot(prepared)
                    saved = dict(schema=1, uuid=UUID, version=packet['version'], source_files=source_hashes, files=hashes)
                    fd, pending_path = tempfile.mkstemp(prefix='.primary-companion-', dir=state)
                    pending = Path(pending_path)
                    with os.fdopen(fd, 'w') as stream:
                        json.dump(saved, stream); stream.flush(); os.fsync(stream.fileno())
                    backup = temporary_root/'previous'
                    changed = False
                    try:
                        if exists:
                            if snapshot(target) != managed['files']:
                                raise SetupError('Existing companion changed during setup; it was preserved')
                            target.replace(backup)
                        prepared.replace(target); changed = True
                        pending.replace(receipt)
                    except Exception:
                        if changed:
                            target.replace(temporary_root/'uncommitted')
                        if backup.exists():
                            backup.replace(target)
                        raise
                    finally:
                        pending.unlink(missing_ok=True)
            # Request only this UUID. Never globally enable other extensions or
            # alter account publication permission; new installation stays off.
            enabled = settings.get_strv('enabled-extensions')
            if UUID not in enabled and not settings.set_strv('enabled-extensions', [*enabled, UUID]):
                raise SetupError('Installed, but enabling this companion is blocked by desktop policy')
            info = extension_info()
            active = info.get('state') == 1 and info.get('version-name') == packet['version']
            return dict(installed=True, state='active' if active else 'new-login-required', uuid=UUID)
        finally:
            os.close(lock)
    finally:
        connection.close_sync(None)


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in {'install', 'status', 'preferences', 'uninstall'}:
        return 2
    try:
        raw = sys.stdin.buffer.read(262145)
        if len(raw) > 262144:
            raise SetupError('Companion resource bound exceeded')
        result = operate(json.loads(raw), sys.argv[1], Path(sys.argv[2]))
        print(json.dumps(dict(ok=True, **result)), flush=True)
        return 0
    except SetupError as error:
        print(json.dumps(dict(ok=False, message=str(error))), flush=True)
        return 1
    except Exception:
        print(json.dumps(dict(ok=False, message='Companion setup failed; existing files and desktop state need inspection')), flush=True)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
