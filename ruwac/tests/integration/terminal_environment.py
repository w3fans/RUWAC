#!/usr/bin/env python3
"""Actual new-terminal environment against an independently started daemon.

All values are generated. No clipboard contents or owner desktop is accessed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from spike import Client, Tui, eventually


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--daemon', type=Path, required=True)
    p.add_argument('--evidence', type=Path, required=True)
    a = p.parse_args()
    a.evidence.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='terminal-env-', dir=a.evidence)).resolve()
    root.chmod(0o700)
    runtime = Path(tempfile.mkdtemp(prefix='te-'))
    runtime.chmod(0o700)
    binary, daemon_binary = a.binary.resolve(), a.daemon.resolve()
    keys = ['DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR',
            'DBUS_SESSION_BUS_ADDRESS', 'XDG_SESSION_TYPE']
    selected = root / 'generated login shell'
    selected.write_text('#!/usr/bin/python3\nimport os,json,sys\nfrom pathlib import Path\n'
                        'assert sys.argv[1:] == ["-l"]\n'
                        f'keys={keys!r}\n'
                        f'root=Path({str(root)!r})\n'
                        'snapshot={k:os.environ[k] for k in keys if k in os.environ}\n'
                        '(root/("environment-"+str(os.getpid())+".json")).write_text(json.dumps(snapshot))\n'
                        'print("ENV_SNAPSHOT_READY",flush=True)\n'
                        'os.execl("/bin/cat","cat")\n')
    selected.chmod(0o700)
    stale = {key: 'generated-stale-' + key for key in keys}
    fresh = {key: 'generated-fresh-' + key for key in keys}
    fresh['XAUTHORITY'] = '/generated/auth $(literal) file'
    base = {**os.environ, 'HOME': str(root), 'SHELL': str(selected),
            'RUWAC_STATE_FILE': str(root / 'state.json')}
    sock = runtime / 'control.sock'
    log = (root / 'daemon.log').open('wb')
    daemon = subprocess.Popen([str(daemon_binary), '--socket', str(sock), 'serve'],
                              env={**base, **stale}, stdout=log, stderr=log)
    views, checks = [], []

    def rpc(method, params=None):
        with Client(sock) as c:
            return c.call(method, params)

    def cli(args, snapshot):
        env = {k: v for k, v in base.items() if k not in keys}
        env.update(snapshot)
        r = subprocess.run([str(binary), '--socket', str(sock), *args],
                           env=env, capture_output=True, timeout=15)
        assert r.returncode == 0, (args, r.stderr)
        return json.loads(r.stdout)['result']

    def snapshot(pane):
        eventually(lambda: 'ENV_SNAPSHOT_READY' in rpc('pane.capture', {'pane': pane})['text'])
        row = next(row for session in rpc('session.list')
                   for row in rpc('pane.list', {'session': session['id']}) if row['id'] == pane)
        # Read the child's completed startup record, not /proc during execve's
        # transient address-space replacement. The exact-value assertions stay.
        return json.loads((root / ('environment-' + str(row['pid']) + '.json')).read_text())

    def mark(name):
        checks.append(name)
        print(name, 'PASS', flush=True)

    try:
        eventually(lambda: sock.exists() and rpc('health')['ok'])
        first = cli(['session', 'create', '--name', 'generated-fresh', '--detach', '--json'], fresh)
        actual = snapshot(first['pane'])
        (a.evidence / 'environment-observation.json').write_text(
            json.dumps({'expected': fresh, 'actual': actual}, indent=2) + '\n')
        assert actual == fresh
        mark('creating-client-overrides-all-stale-daemon-desktop-values')
        sid = first['session']
        for kind, args in [
            ('window', ['window', 'create', '--session', sid, '--json']),
            ('screen', ['-S', 'generated-fresh', '-X', 'screen', 'generated-window']),
            ('split', ['pane', 'split', '--window', first['window'], '--json']),
        ]:
            before = {row['id'] for row in rpc('pane.list', {'session': sid})}
            cli(args, fresh)
            pane = next(row for row in rpc('pane.list', {'session': sid}) if row['id'] not in before)
            assert snapshot(pane['id']) == fresh
            mark(kind + '-creation-uses-current-client-desktop')
        absent = cli(['session', 'create', '--name', 'generated-no-desktop', '--detach', '--json'], {})
        assert snapshot(absent['pane']) == {}
        mark('headless-client-clears-stale-daemon-desktop')
        before = rpc('pane.list', {'session': sid})
        view = Tui(binary, sock, sid, root / 'client.pty', env={**base, **fresh})
        views.append(view)
        eventually(lambda: b'ENV_SNAPSHOT_READY' in view.data)
        assert rpc('pane.list', {'session': sid})[0]['pid'] == before[0]['pid']
        mark('reattach-preserves-existing-process-and-environment')
        for name, command in [('window', b'\x01c'), ('split', b'\x01|')]:
            prior = {row['id'] for row in rpc('pane.list', {'session': sid})}
            view.send(command)
            pane = eventually(lambda: next((row for row in rpc('pane.list', {'session': sid})
                                           if row['id'] not in prior), None))
            assert snapshot(pane['id']) == fresh
            mark('tui-' + name + '-uses-attaching-client-desktop')
        custom = rpc('session.create', {'name': 'generated-literal', 'argv': [str(selected), '-l']})
        assert snapshot(custom['pane']) == stale
        mark('raw-rpc-custom-command-keeps-literal-daemon-inheritance')
        result = {'PASS': True, 'checks': checks,
                  'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                  'daemon_sha256': hashlib.sha256(daemon_binary.read_bytes()).hexdigest(),
                  'scope': 'Generated PTYs only; no actual clipboard image consumption claim'}
        (a.evidence / 'acceptance.json').write_text(json.dumps(result, indent=2) + '\n')
    finally:
        for view in views:
            view.finish('term')
        if daemon.poll() is None:
            daemon.terminate()
            try:
                daemon.wait(timeout=8)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait()
        log.close()
        shutil.rmtree(runtime)


if __name__ == '__main__':
    main()
