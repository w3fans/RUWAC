#!/usr/bin/env python3
"""Checkpoint snapshots remain current and immutable across real PTY changes."""
import argparse, hashlib, json, os, signal, subprocess, sys, tempfile
from pathlib import Path
from spike import Client, eventually

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--evidence', required=True, type=Path)
a = p.parse_args()
root = Path(tempfile.mkdtemp(prefix='cc-', dir=a.evidence))
root.chmod(0o700)
sock, state = root/'c.sock', root/'state.json'
app = root/'app.py'
app.write_text('''import os,tty
tty.setraw(0)
os.write(1,b'PRIMARY-READY')
while True:
 b=os.read(0,1)
 if not b:break
 if b==b'u':os.write(1,'\\r\\n\\x1b[31mŽe\\u0301界-UPDATE-READY'.encode())
 if b==b'a':os.write(1,b'\\x1b[?1049h\\x1b[?2004hALT-READY')
 if b==b'p':os.write(1,b'\\x1b[?1049l\\r\\nPRIMARY-RETURNED')
''')
env = dict(os.environ)
for key in ['RUWAC_SOCKET', 'RUWAC_SESSION', 'RUWAC_PANE', 'RUWAC_STATE_FILE']:
    env.pop(key, None)
env['RUWAC_STATE_FILE'] = str(state)
for key in ['HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_DATA_HOME']:
    path = root/key.lower()
    path.mkdir(mode=0o700)
    env[key] = str(path)
server, view = None, None
logs, children, cases = [], {}, []

def rpc(method, params=None):
    with Client(sock) as c:
        return c.call(method, params)

def start(label):
    global server
    log = (root/(label+'.log')).open('wb')
    logs.append(log)
    server = subprocess.Popen([a.binary, '--socket', str(sock), 'serve'],
                              env=env, stdout=log, stderr=log)
    def ready():
        assert server.poll() is None, 'Owned daemon exited'
        try:
            return rpc('health')
        except (OSError, EOFError):
            return None
    eventually(ready, 15)

def checkpoint():
    rpc('daemon.checkpoint')
    value = json.loads(state.read_bytes())
    assert value['schema'] == 5
    return {p['id']: p['terminal'] for p in value['panes']}

def update(pane, text, marker, buffer='active'):
    rpc('pane.sendText', {'pane': pane, 'text': text})
    eventually(lambda: marker in rpc('pane.capture', {'pane': pane, 'buffer': buffer})['text'])

def passed(name):
    cases.append(name)
    print(name, 'PASS', flush=True)

try:
    start('live')
    s = rpc('session.create', {'name': 'Checkpoint consistency', 'rows': 24,
                              'cols': 80, 'argv': [sys.executable, str(app)]})
    q = rpc('window.create', {'session': s['session'], 'argv': [sys.executable, str(app)]})
    active, quiet = s['pane'], q['pane']
    for row in [s, q]:
        pid = row['pid']
        children[pid] = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[19]
        eventually(lambda: 'PRIMARY-READY' in rpc('pane.capture', {'pane': row['pane']})['text'])
    view = Client(sock)
    view.call('session.attach', {'session': s['session'], 'rows': 24, 'cols': 80})
    view.call('window.select', {'window': s['window']})
    initial = checkpoint()
    before = state.read_bytes()
    assert checkpoint() == initial and state.read_bytes() == before
    passed('CHECKPOINT-UNCHANGED-CANONICAL-CONTENTS')
    update(active, 'u', 'UPDATE-READY')
    updated = checkpoint()
    assert updated[active] != initial[active] and updated[quiet] == initial[quiet]
    assert any('Že\u0301界-UPDATE-READY' in row[0] for row in updated[active]['history'])
    passed('CHECKPOINT-UPDATED-PANE-AND-UNCHANGED-HIDDEN-PANE')
    update(active, 'a', 'ALT-READY')
    alternate = checkpoint()
    assert alternate[active]['grid']['alternate_screen']
    assert alternate[active]['primary']['history'] == updated[active]['history']
    assert alternate[quiet] == initial[quiet]
    passed('CHECKPOINT-ALTERNATE-AND-HIDDEN-PRIMARY')
    view.call('client.resize', {'rows': 30, 'cols': 90})
    resized = checkpoint()
    assert resized[active]['rows'] == 30 and resized[active]['cols'] == 90
    passed('CHECKPOINT-RESIZE-INVALIDATES-SNAPSHOT')
    update(active, 'p', 'PRIMARY-RETURNED')
    returned = checkpoint()
    assert not returned[active]['grid']['alternate_screen']
    assert any('PRIMARY-RETURNED' in row[0] for row in returned[active]['history'])
    for pid, start_time in children.items():
        assert Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[19] == start_time
    passed('CHECKPOINT-PRIMARY-RETURN-KEEPS-LIVE-PROCESSES')
    view.close()
    view = None
    rpc('daemon.stop')
    server.wait(timeout=10)
    server = None
    start('restored')
    assert 'PRIMARY-RETURNED' in rpc('pane.capture', {'pane': active})['text']
    rows = rpc('pane.list', {'session': s['session']})
    assert {r['id'] for r in rows} == {active, quiet}
    assert all(r['ended'] and r['restored'] and r['pid'] is None for r in rows)
    checkpoint()
    passed('CHECKPOINT-RESTORES-IDS-AND-CONTENTS-WITHOUT-PROCESSES')
    rpc('pane.close', {'pane': quiet})
    assert quiet not in checkpoint()
    passed('CHECKPOINT-CLOSED-PANE-IS-NOT-RETAINED')
    rpc('daemon.stop')
    server.wait(timeout=10)
    server = None
finally:
    if view is not None:
        view.close()
    if server is not None and server.poll() is None:
        server.kill()
        server.wait(timeout=5)
    for pid in children:
        try:
            if str(app).encode() in Path(f'/proc/{pid}/cmdline').read_bytes():
                os.kill(pid, signal.SIGKILL)
        except FileNotFoundError:
            pass
    for log in logs:
        log.close()
    receipt = {'PASS': len(cases) == 7, 'cases': cases,
               'binary_sha256': hashlib.sha256(Path(a.binary).read_bytes()).hexdigest(),
               'test_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
               'owner_processes_changed': False, 'evidence': str(root)}
    (a.evidence/'checkpoint-consistency.json').write_text(json.dumps(receipt, indent=2)+'\n')
assert receipt['PASS']
