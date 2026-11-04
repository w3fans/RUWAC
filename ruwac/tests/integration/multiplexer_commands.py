#!/usr/bin/env python3
"""Bounded Screen/tmux-style CLI controls against a disposable real daemon/PTY.

--daemon-binary permits a preserved older engine; the new CLI must use existing
RPCs without restarting or replacing it. No owner socket or session is used.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--daemon-binary')
p.add_argument('--evidence', required=True)
a = p.parse_args()
spec = importlib.util.spec_from_file_location('spike', Path(__file__).with_name('spike.py'))
spike = importlib.util.module_from_spec(spec)
spec.loader.exec_module(spike)
binary = str(Path(a.binary).resolve())
engine = str(Path(a.daemon_binary or a.binary).resolve())
root = Path(tempfile.mkdtemp(prefix='mux-commands-', dir=a.evidence))
root.chmod(0o700)
sock = root / 'control.sock'
receiver = Path(__file__).parents[1] / 'fixtures/receiver.py'
checks = []


def run(*args, ok=True):
    reply = subprocess.run([binary, '--socket', str(sock), *args], capture_output=True, timeout=10)
    assert (reply.returncode == 0) == ok, (args, reply.stdout, reply.stderr)
    return json.loads(reply.stdout) if reply.stdout else None


with (root / 'daemon.log').open('wb') as log:
    server = subprocess.Popen([engine, '--socket', str(sock), 'serve'], stdout=log, stderr=log)
    try:
        spike.eventually(sock.exists)
        with spike.Client(sock) as rpc:
            first = rpc.call('session.create', {'name': 'mux-fixture', 'argv': [sys.executable, str(receiver), str(root / 'input.bin')]})
            rpc.call('pane.wait', {'pane': first['pane'], 'contains': 'READY', 'timeout_ms': 5000})
        before_pane = run('pane', 'list', '--session', first['session'])['result'][0]
        before_pid = before_pane['pid']
        assert any(s['id'] == first['session'] for s in run('list-sessions')['result'])
        assert run('has-session', '-t', 'mux-fixture')['result']['exists']
        run('has-session', '-t', 'mux-fixtur', ok=False)
        assert run('list-windows', '-t', first['session'])['result'][0]['id'] == first['window']
        checks.append('exact session resolution, discovery and missing-name refusal')

        text = '-n $HOME žółć 🧪\t\n'
        run('send-keys', '-t', first['pane'], '-l', '--', text)
        spike.eventually(lambda: (root / 'input.bin').read_bytes() == text.encode())
        for bad in [('send-keys', '-t', first['pane'], 'Enter'),
                    ('send-keys', '-t', first['pane'], '-l', 'safe', '-unknown'),
                    ('kill-pane', '-t', '0'), ('kill-pane', '-t', first['pane'], '-a'),
                    ('split-window', '-t', first['window'], '-h', '-v')]:
            run(*bad, ok=False)
        assert (root / 'input.bin').read_bytes() == text.encode()
        checks.append('atomic literal input; invalid commands send no partial bytes')

        payload = 'clipboard-like text, no automatic Enter: λ'
        file = root / 'buffer.txt'
        file.write_text(payload)
        run('buffer', 'set', '-s', first['session'], '-f', str(file))
        assert run('buffer', 'show', '-s', first['session'])['result']['text'] == payload
        run('buffer', 'set', '--session', first['session'], '--text', 'x', '--file', str(file), ok=False)
        run('-S', 'mux-fixture', '-X', 'paste')
        text += payload
        run('-S', 'mux-fixture', '-X', 'stuff', ' literal')
        text += ' literal'
        spike.eventually(lambda: (root / 'input.bin').read_bytes() == text.encode())
        assert run('-S', 'mux-fixture', '-X', 'info')['result']['active_pane'] == first['pane']
        checks.append('session buffer file/show, Screen paste/stuff and window info')

        second = run('split-window', '-t', first['window'], '-h')['result']['pane']
        rows = run('pane', 'list', '--session', first['session'])['result']
        assert len(rows) == 2 and all(row['cols'] < before_pane['cols'] and row['rows'] == before_pane['rows'] for row in rows)
        run('select-pane', '-t', first['pane'])
        run('kill-pane', '-t', second)
        new = run('new-window', '-t', first['session'], '-n', 'second')['result']['window']
        run('rename-window', '-t', new, 'renamed window')
        assert any(w['id'] == new and w['name'] == 'renamed window' for w in run('list-windows', '-t', first['session'])['result'])
        run('select-window', '-t', first['window'])
        assert run('next-window', '-t', first['session'])['result']['selected'] == new
        assert run('last-window', '-t', first['session'])['result']['selected'] == first['window']
        assert run('previous-window', '-t', first['session'])['result']['selected'] == new
        run('window', 'select', '-w', first['window'])
        run('window', 'rename', '-w', first['window'], '-n', 'original renamed')
        run('pane', 'select', '-i', first['pane'])
        new_pane = next(row['id'] for row in run('pane', 'list', '-s', first['session'])['result'] if row['window'] == new)
        run('pane', 'close', '-i', new_pane)
        checks.append('split orientation, stable pane/window selection, rename, cycle and closure')

        run('-S', 'mux-fixture', '-X', 'split', '-v')
        created = next(row['id'] for row in run('pane', 'list', '-s', first['session'])['result'] if row['id'] != first['pane'])
        run('pane', 'close', '-i', created)
        run('select-pane', '-t', 'pane_' + '0' * 32, ok=False)
        assert run('pane', 'list', '-s', first['session'])['result'][0]['pid'] == before_pid
        assert server.poll() is None
        assert (root / 'input.bin').read_bytes() == text.encode()
        checks.append('Screen split; stale target refusal; original child and daemon preserved')
        (root / 'acceptance.json').write_text(json.dumps({'status': 'PASS', 'groups': checks, 'older_daemon': engine != binary}, indent=2) + '\n')
        print('MULTIPLEXER_COMMANDS PASS', len(checks), 'groups')
    finally:
        if server.poll() is None:
            try:
                with spike.Client(sock) as rpc:
                    rpc.call('daemon.stop')
            except Exception:
                server.terminate()
            server.wait(timeout=8)
