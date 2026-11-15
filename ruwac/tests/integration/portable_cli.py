#!/usr/bin/env python3
"""Install and exercise the same headless release in four isolated OS userspaces."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import select
import shlex
import shutil
import struct
import subprocess
import tarfile
import termios
import time
import uuid


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


class Containers:
    def call(self, args, timeout=30, check=True):
        return subprocess.run(['docker', *args], capture_output=True, text=True, timeout=timeout, check=check)

    def create(self, name, nonce, image, artifacts, control):
        self.call(['run', '--detach', '--name', name, '--label', 'io.ruwac.portable=' + nonce,
                   '--network=none', '--cpus=1', '--memory=1g', '--memory-swap=1g', '--pids-limit=128',
                   '--mount', 'type=bind,src=' + str(artifacts) + ',dst=/input,readonly',
                   '--mount', 'type=bind,src=' + str(control) + ',dst=/control', image, '/bin/sleep', '900'], timeout=180)

    def interactive(self, name, args):
        return ['docker', 'exec', '-it', '--user', str(os.getuid()), '--env', 'HOME=/control/home',
                '--env', 'XDG_RUNTIME_DIR=/control/runtime', '--env', 'TERM=xterm-256color', name, *args]


def wait(fn, seconds=20):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        result = fn()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError('Portable fixture readiness timeout')


def qualify(target, artifacts, evidence, build, backend):
    nonce = uuid.uuid4().hex
    name = 'ruwac-portable-' + nonce
    evidence.mkdir(parents=True, exist_ok=False)
    control = evidence / 'control'
    control.mkdir(mode=0o700)

    def ex(args, check=True, root=False):
        options = [] if root else ['--user', str(os.getuid()), '--env', 'HOME=/control/home',
                                   '--env', 'XDG_RUNTIME_DIR=/control/runtime', '--env', 'TERM=xterm-256color']
        return backend.call(['exec', *options, name, *args], check=check)

    def rpc(method, params=None):
        q = ex(['/usr/bin/ruwac', '--socket', '/control/runtime/s', 'rpc', method,
                '--params', json.dumps(params or {}, ensure_ascii=False)])
        value = json.loads(q.stdout)
        if 'error' in value:
            raise AssertionError(value['error'])
        return value.get('result', value)

    def start():
        ex(['/bin/sh', '-c', 'umask 077; mkdir -p /control/runtime /control/home; '
            'chmod 700 /control/runtime /control/home; '
            'HOME=/control/home XDG_RUNTIME_DIR=/control/runtime RUWAC_STATE_FILE=/control/runtime/history.json '
            'TERM=xterm-256color /usr/bin/ruwac --socket /control/runtime/s serve '
            '>/control/engine.log 2>&1 & echo $! >/control/engine.pid'])
        def healthy():
            q = ex(['/usr/bin/ruwac', '--socket', '/control/runtime/s', 'rpc', 'health'], check=False)
            return q.returncode == 0 and '"ok":true' in q.stdout.replace(' ', '')
        wait(healthy)
        return int(ex(['/bin/cat', '/control/engine.pid']).stdout)

    def process(pid):
        q = ex(['/bin/cat', '/proc/' + str(pid) + '/stat'], check=False)
        if q.returncode:
            return None
        parts = q.stdout.rsplit(')', 1)[1].split()
        return {'state': parts[0], 'start': parts[19]}

    try:
        backend.create(name, nonce, target['image'], artifacts, control)
        os_release = {}
        for line in ex(['/bin/cat', '/etc/os-release']).stdout.splitlines():
            if '=' in line:
                key, value = line.split('=', 1)
                os_release[key] = shlex.split(value)[0]
        assert os_release['ID'] == target['distribution'] and os_release['VERSION_ID'] == target['version']
        assert ex(['/bin/uname', '-m']).stdout.strip() == 'x86_64'
        ex(['dpkg', '--install', '/input/' + build['deb']], root=True)
        assert ex(['/usr/bin/ruwac', '--version']).stdout.strip() == 'RUWAC ' + build['version']
        assert ex(['sha256sum', '/usr/bin/ruwac']).stdout.split()[0] == build['binary_sha256']
        assert not ex(['dpkg', '--verify', 'ruwac']).stdout.strip()
        ex(['/bin/mkdir', '-p', '/control/archive'])
        ex(['tar', '-xzf', '/input/' + build['archive'], '-C', '/control/archive'])
        extracted = '/control/archive/' + build['archive'][:-7] + '/bin/ruwac'
        assert ex([extracted, '--version']).stdout.strip() == 'RUWAC ' + build['version']
        engine_pid = start()
        engine_identity = process(engine_pid)
        row = rpc('session.create', {'name': 'portable-cli', 'rows': 24, 'cols': 80,
                  'argv': ['/bin/sh', '-c', 'printf "PORT_READY\\n"; while IFS= read -r line; do printf "ECHO:%s\\n" "$line"; done']})
        shell_identity = process(row['pid'])
        rpc('pane.sendText', {'pane': row['pane'], 'text': 'port-ž-λ'})
        rpc('pane.sendKey', {'pane': row['pane'], 'key': 'Enter'})
        wait(lambda: 'ECHO:port-ž-λ' in json.dumps(rpc('pane.capture', {'pane': row['pane'], 'lines': 50}), ensure_ascii=False))
        for _ in range(2):
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))
            child = subprocess.Popen(backend.interactive(name, ['/usr/bin/ruwac', '--socket', '/control/runtime/s',
                                      'attach', '--session', row['session']]), stdin=slave, stdout=slave, stderr=slave,
                                     start_new_session=True)
            os.close(slave)
            output = bytearray()
            try:
                end = time.monotonic() + 15
                while b'PORT_READY' not in output and time.monotonic() < end:
                    if select.select([master], [], [], .1)[0]:
                        output.extend(os.read(master, 65536))
                    assert len(output) < 4 * 1024**2
                assert b'PORT_READY' in output
                os.write(master, b'\x01d')
                assert child.wait(timeout=10) == 0
            finally:
                os.close(master)
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)
        ex(['dpkg', '--install', '/input/' + build['deb']], root=True)
        assert process(engine_pid)['start'] == engine_identity['start'] and process(row['pid'])['start'] == shell_identity['start']
        rpc('daemon.checkpoint')
        rpc('daemon.stop')
        def stopped():
            value = process(engine_pid)
            return value is None or value['state'] in ['Z', 'X']
        wait(stopped)
        start()
        panes = rpc('pane.list', {'session': row['session']})
        assert len(panes) == 1 and panes[0]['id'] == row['pane'] and panes[0]['pid'] is None and panes[0]['ended']
        assert 'ECHO:port-ž-λ' in json.dumps(rpc('pane.capture', {'pane': row['pane'], 'lines': 50}), ensure_ascii=False)
        rpc('daemon.stop')
        ex(['dpkg', '--remove', 'ruwac'], root=True)
        assert ex(['/bin/sh', '-c', 'test ! -e /usr/bin/ruwac']).returncode == 0
        return {**target, 'PASS': True, 'version': build['version'], 'binary_sha256': build['binary_sha256'],
                'deb_install_verify_remove': True, 'archive_exec': True, 'Unicode_echo': True,
                'native_TUI_attach_detach_cycles': 2, 'reinstall_preserves_engine_and_shell': True,
                'restore_preserves_ids_and_history_without_process_resurrection': True,
                'glibc': ex(['getconf', 'GNU_LIBC_VERSION']).stdout.strip()}
    finally:
        present = backend.call(['container', 'inspect', name], check=False)
        if present.returncode == 0:
            info = json.loads(present.stdout)[0]
            assert info['Config']['Labels'].get('io.ruwac.portable') == nonce
            backend.call(['rm', '-f', info['Id']])
            assert backend.call(['container', 'inspect', name], check=False).returncode != 0
        shutil.rmtree(control)


def run(root, artifacts, evidence, backend=None):
    root, artifacts, evidence = root.resolve(), artifacts.resolve(), evidence.resolve()
    build = json.loads((artifacts / 'build.json').read_text())
    for name, value in build['artifacts'].items():
        assert Path(name).name == name and sha(artifacts / name) == value
    evidence.mkdir(parents=True, exist_ok=False)
    results = []
    for target in json.loads((root / 'scripts/portable-cli-targets.json').read_text()):
        assert target['image'].startswith('docker.io/library/') and '@sha256:' in target['image']
        result = qualify(target, artifacts, evidence / (target['distribution'] + '-' + target['version']), build, backend or Containers())
        results.append(result)
        (evidence / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
        print(target['distribution'], target['version'], 'PASS', flush=True)
    return results


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--artifacts', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    run(args.root, args.artifacts, args.evidence)
