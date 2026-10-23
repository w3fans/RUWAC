#!/usr/bin/env python3
"""Actual loopback SSH/PTY launcher acceptance with a generated agent process.

Requires OpenSSH and explicit --run-owned-fixture. Never launches real agents,
uses owner SSH configuration, or connects to an external host.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import pty
import pwd
import select
import shlex
import socket
import struct
import subprocess
import sys
import tempfile
import time

from spike import Client, eventually


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--run-owned-fixture', action='store_true', required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    root = Path(tempfile.mkdtemp(prefix='agent-session-', dir=args.evidence)).resolve()
    root.chmod(0o700)
    runtime = Path(tempfile.mkdtemp(prefix='ruwac-agent-', dir=f'/run/user/{os.getuid()}'))
    runtime.chmod(0o700)
    sock = runtime / 'control.sock'
    binary = str(args.binary.resolve())
    script = (Path(__file__).resolve().parents[3] / 'scripts/remote-agent.py').read_text()
    clients, results = [], []
    server = None
    log = (root / 'sshd.log').open('wb')

    def rpc(method, params=None):
        with Client(sock) as client:
            return client.call(method, params)

    def drain(item):
        while select.select([item['fd']], [], [], .01)[0]:
            try:
                data = os.read(item['fd'], 65536)
            except OSError:
                break
            if not data:
                break
            item['output'].extend(data)
        return bytes(item['output'])

    def launch(name='codex', command=None, shared=''):
        command = command or shlex.join(['python3', '-u', str(root / 'receiver.py'), str(root / name)])
        remote = shlex.join(['bash', '-lc', shlex.join(
            ['python3', '-c', script, binary, str(sock), name, command, shared])])
        master, slave = pty.openpty()
        fcntl.ioctl(slave, 0x5414, struct.pack('HHHH', 30, 100, 0, 0))
        child = subprocess.Popen(ssh + [remote], stdin=slave, stdout=slave, stderr=slave,
                                 env={**os.environ, 'TERM': 'xterm-256color'})
        os.close(slave)
        item = {'child': child, 'fd': master, 'output': bytearray()}
        clients.append(item)
        return item

    def attached(item):
        return b'\x1b[H\x1b[2J' in drain(item)

    def detach(item):
        os.write(item['fd'], b'\x01d')
        eventually(lambda: (drain(item), item['child'].poll() is not None)[1], 10)
        assert item['child'].returncode == 0

    def failed(item):
        eventually(lambda: (drain(item), item['child'].poll() is not None)[1], 10)
        assert item['child'].returncode != 0
        assert b'Remote agent launch failed' in drain(item)

    def case(name):
        results.append({'id': name, 'status': 'PASS'})
        print(name, 'PASS', flush=True)

    try:
        for name in ('host', 'client'):
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(root / name)], check=True)
            (root / name).chmod(0o600)
        authorized = runtime / 'authorized_keys'
        authorized.write_bytes((root / 'client.pub').read_bytes())
        authorized.chmod(0o600)
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        config = root / 'sshd.conf'
        config.write_text(f'ListenAddress 127.0.0.1\nPort {port}\nHostKey {root}/host\n'
                          f'PidFile {runtime}/sshd.pid\nAuthorizedKeysFile {authorized}\nStrictModes yes\n'
                          f'PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\n'
                          f'AllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nDisableForwarding yes\nLogLevel ERROR\n')
        key = (root / 'host.pub').read_text().split()
        known = root / 'known_hosts'
        known.write_text(f'[127.0.0.1]:{port} {key[0]} {key[1]}\n')
        ssh = ['ssh', '-F', '/dev/null', '-tt', '-p', str(port), '-i', str(root / 'client'),
               '-o', 'BatchMode=yes', '-o', 'IdentitiesOnly=yes', '-o', 'StrictHostKeyChecking=yes',
               '-o', f'UserKnownHostsFile={known}', '-o', 'GlobalKnownHostsFile=/dev/null',
               '-o', 'ForwardAgent=no', '-o', 'ClearAllForwardings=yes', '-o', 'ConnectTimeout=5',
               '--', f'{pwd.getpwuid(os.getuid()).pw_name}@127.0.0.1']
        server = subprocess.Popen(['/usr/sbin/sshd', '-D', '-e', '-f', str(config)], stdout=log, stderr=log)

        def listening():
            assert server.poll() is None, 'Owned SSH server failed'
            try:
                with socket.create_connection(('127.0.0.1', port), .1):
                    return True
            except OSError:
                return False

        eventually(listening)
        (root / 'receiver.py').write_text(
            "import os,sys,tty\nfrom pathlib import Path\n"
            "tty.setraw(0)\np=Path(sys.argv[1])\n"
            "with p.with_suffix('.starts').open('a') as f:f.write(str(os.getpid())+'\\n')\n"
            "print('GENERATED_AGENT_READY',flush=True)\n"
            "while True:\n data=os.read(0,1024)\n"
            " if data==b'q':break\n"
            " with p.with_suffix('.input').open('ab') as f:f.write(data)\n")
        # Race two actual SSH PTYs against an initially absent isolated daemon.
        first, second = launch(), launch()
        eventually(lambda: attached(first) and attached(second), 30)
        eventually(lambda: (root / 'codex.starts').exists())
        sessions = rpc('session.list')
        assert len(sessions) == 1 and sessions[0]['name'] == 'codex', sessions
        sid = sessions[0]['id']
        pane = rpc('pane.list', {'session': sid})[0]
        pid, pane_id = pane['pid'], pane['id']
        assert len((root / 'codex.starts').read_text().splitlines()) == 1
        assert b'Remote RUWAC ' in drain(first) and b'Remote RUWAC ' in drain(second)
        case('SSH-CONCURRENT-LAUNCH-ONE-SESSION')
        os.write(first['fd'], b'FIRST')
        os.write(second['fd'], b'SECOND')
        eventually(lambda: (root / 'codex.input').exists()
                   and len((root / 'codex.input').read_bytes()) == 11)
        received = (root / 'codex.input').read_bytes()
        assert received.count(b'FIRST') == 1 and received.count(b'SECOND') == 1
        detach(first)
        assert rpc('pane.list', {'session': sid})[0]['pid'] == pid
        detach(second)
        case('SSH-SHARED-INPUT-AND-DETACH-PERSISTENCE')
        # Preference changes affect only creation; never inject commands into a live agent.
        third = launch(command='touch ' + shlex.quote(str(root / 'must-not-exist')))
        eventually(lambda: attached(third))
        assert not (root / 'must-not-exist').exists()
        assert rpc('pane.list', {'session': sid})[0]['pid'] == pid
        assert len((root / 'codex.starts').read_text().splitlines()) == 1
        assert (root / 'codex.input').read_bytes() == received
        detach(third)
        case('SSH-REATTACH-KEEPS-PID-AND-IGNORES-NEW-COMMAND')
        other = launch('claude')
        eventually(lambda: attached(other))
        assert {s['name'] for s in rpc('session.list')} == {'codex', 'claude'}
        detach(other)
        case('SSH-AGENTS-USE-SEPARATE-SESSIONS')
        rpc('pane.sendText', {'pane': pane_id, 'text': 'q'})
        eventually(lambda: next(p for p in rpc('pane.list', {'session': sid}) if p['id'] == pane_id)['ended'])
        ended = launch()
        eventually(lambda: attached(ended))
        assert len((root / 'codex.starts').read_text().splitlines()) == 1
        detach(ended)
        case('SSH-ENDED-SESSION-IS-NOT-RESTARTED')
        rpc('session.create', {'name': 'codex', 'argv': ['sleep', '120']})
        count = len(rpc('session.list'))
        failed(launch())
        assert len(rpc('session.list')) == count
        case('SSH-AMBIGUOUS-NAME-REFUSES-WITHOUT-CREATING')
        lock = runtime / '.agent-claude.lock'
        lock.unlink()
        marker = root / 'unchanged'
        marker.write_text('generated protected bytes')
        lock.symlink_to(marker)
        failed(launch('claude'))
        assert marker.read_text() == 'generated protected bytes'
        assert len(rpc('session.list')) == count
        case('SSH-UNSAFE-LOCK-REFUSED')
        # Opt-in: two agents in independently selected windows of one session.
        def shared_command(name):
            return shlex.join(['python3', '-u', str(root / 'receiver.py'), str(root / ('shared-' + name))])
        code = launch('codex', shared_command('codex'), 'agents')
        claude = launch('claude', shared_command('claude'), 'agents')
        eventually(lambda: attached(code) and attached(claude), 30)
        shared = next(s for s in rpc('session.list') if s['name'] == 'agents')
        windows = rpc('window.list', {'session': shared['id']})
        assert len(windows) == 2 and {w['name'] for w in windows} == {'codex', 'claude'}, windows
        panes = rpc('pane.list', {'session': shared['id']})
        identities = {p['id']: p['pid'] for p in panes}
        case('SSH-SHARED-SESSION-CONCURRENT-NAMED-AGENT-WINDOWS')
        eventually(lambda: (root/'shared-codex.starts').exists() and (root/'shared-claude.starts').exists(),15)
        os.write(code['fd'], b'CODEX_ONLY')
        os.write(claude['fd'], b'CLAUDE_ONLY')
        eventually(lambda: (root/'shared-codex.input').exists() and (root/'shared-claude.input').exists())
        assert (root/'shared-codex.input').read_bytes() == b'CODEX_ONLY'
        assert (root/'shared-claude.input').read_bytes() == b'CLAUDE_ONLY'
        detach(code); detach(claude)
        assert {p['id']:p['pid'] for p in rpc('pane.list',{'session':shared['id']})} == identities
        case('SSH-SHARED-SESSION-INDEPENDENT-INPUT-AND-PERSISTENCE')
        kimi = launch('kimi', shared_command('kimi'), 'agents')
        eventually(lambda: attached(kimi), 30)
        eventually(lambda: (root/'shared-kimi.starts').exists(),15)
        os.write(kimi['fd'], b'KIMI_ONLY')
        eventually(lambda: (root/'shared-kimi.input').exists())
        assert (root/'shared-kimi.input').read_bytes() == b'KIMI_ONLY'
        assert {w['name'] for w in rpc('window.list',{'session':shared['id']})} == {'codex','claude','kimi'}
        assert (root/'shared-codex.input').read_bytes() == b'CODEX_ONLY'
        assert (root/'shared-claude.input').read_bytes() == b'CLAUDE_ONLY'
        detach(kimi)
        kimi = launch('kimi', 'touch ' + shlex.quote(str(root/'kimi-must-not-exist')), 'agents')
        eventually(lambda: attached(kimi))
        assert not (root/'kimi-must-not-exist').exists()
        assert len((root/'shared-kimi.starts').read_text().splitlines()) == 1
        detach(kimi)
        case('SSH-KIMI-THIRD-SHARED-WINDOW-ISOLATION-AND-REATTACH')
        again = launch('codex', 'touch ' + shlex.quote(str(root/'shared-must-not-exist')), 'agents')
        eventually(lambda:attached(again))
        assert not (root/'shared-must-not-exist').exists()
        assert len((root/'shared-codex.starts').read_text().splitlines()) == 1
        target = next(w for w in windows if w['name']=='codex')['panes'][0]
        rpc('pane.sendText',{'pane':target,'text':'q'})
        eventually(lambda: (drain(again),again['child'].poll() is not None)[1],10)
        assert again['child'].returncode == 0
        assert (root/'shared-claude.input').read_bytes() == b'CLAUDE_ONLY'
        case('SSH-NAMED-AGENT-EXIT-DOES-NOT-SWITCH-INPUT-TO-OTHER-AGENT')
        ended = launch('codex',shared_command('codex'),'agents')
        eventually(lambda:attached(ended))
        assert len((root/'shared-codex.starts').read_text().splitlines()) == 1
        detach(ended)
        case('SSH-SHARED-ENDED-WINDOW-HISTORY-NO-RESTART')
        existing = next(s for s in rpc('session.list') if s['name']=='claude')
        old_panes = {p['id']:p['pid'] for p in rpc('pane.list',{'session':existing['id']})}
        added = launch('codex',shlex.join(['python3','-u',str(root/'receiver.py'),str(root/'added-codex')]),'claude')
        eventually(lambda:attached(added))
        after = {p['id']:p['pid'] for p in rpc('pane.list',{'session':existing['id']})}
        assert all(after[k]==v for k,v in old_panes.items()) and len(after)==len(old_panes)+1
        assert {w['name'] for w in rpc('window.list',{'session':existing['id']})} == {'codex','claude'}
        detach(added)
        case('SSH-ADD-SECOND-AGENT-TO-EXISTING-AGENT-SESSION')

    finally:
        for index, item in enumerate(clients):
            if item['child'].poll() is None:
                item['child'].terminate()
                item['child'].wait(timeout=5)
            drain(item)
            (root / f'client-{index}.pty').write_bytes(item['output'])
            os.close(item['fd'])
        if sock.exists():
            rpc('daemon.stop')
        if server and server.poll() is None:
            server.terminate()
            server.wait(timeout=5)
        log.close()
        # Revoke generated login credentials even on a failed run.
        (runtime / 'authorized_keys').unlink(missing_ok=True)
        receipt = {'complete': len(results) == 13, 'results': results, 'binary_sha256': hashlib.sha256(Path(binary).read_bytes()).hexdigest(),
                   'script_sha256': hashlib.sha256(script.encode()).hexdigest(),
                   'scope': 'Actual authenticated loopback SSH and PTY; generated agent, no provider login',
                   'root': str(root), 'runtime': str(runtime)}
        (root / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()
