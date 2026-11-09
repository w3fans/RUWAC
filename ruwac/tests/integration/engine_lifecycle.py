#!/usr/bin/env python3
"""Safe engine lifecycle: atomic stop-if-idle, new-work routing and inventory.

Uses only private generated runtime/state directories and engines started by
this test. Three binaries:
  --binary        the candidate CLI/engine;
  --older-safe    the same source built with a lower version (has stopIfIdle);
  --legacy        a released engine from before stopIfIdle.
No owner engine, socket, session or checkpoint is touched.
"""
import argparse, concurrent.futures, json, os, signal, socket as sk, struct, subprocess, tempfile, time
from pathlib import Path
from spike import Client, eventually

p = argparse.ArgumentParser()
for name in ['binary', 'older-safe', 'legacy']:
    p.add_argument('--' + name, required=True, type=Path)
p.add_argument('--evidence', required=True, type=Path)
a = p.parse_args()
binary, older, legacy = (x.resolve() for x in (a.binary, a.older_safe, a.legacy))
for key in ['RUWAC_STATE_FILE', 'RUWAC_SOCKET', 'RUWAC_SESSION', 'RUWAC_PANE', 'TMUX', 'STY']:
    os.environ.pop(key, None)
root = Path(tempfile.mkdtemp(prefix='lifecycle-', dir=os.environ.get('TMPDIR')))
root.chmod(0o700)
checks, children = [], []


def check(name, condition, detail=None):
    assert condition, (name, detail)
    checks.append(name)
    print('PASS', name, flush=True)


def version(exe):
    return subprocess.run([str(exe), '--version'], capture_output=True, text=True).stdout.split()[-1]


def world(name):
    """A private XDG runtime/state pair, as on a fresh machine."""
    base = root / name
    (base / 'run').mkdir(parents=True, mode=0o700)
    (base / 'state').mkdir(mode=0o700)
    env = dict(os.environ, XDG_RUNTIME_DIR=str(base / 'run'), XDG_STATE_HOME=str(base / 'state'), HOME=str(base))
    return env, base / 'run/ruwac/control.sock'


def serve(exe, sock, env):
    sock.parent.mkdir(mode=0o700, exist_ok=True)
    log = (root / f'{sock.parent.parent.parent.name}-{sock.name}.log').open('a')
    proc = subprocess.Popen([str(exe), '--socket', str(sock), 'serve'], env=env, stdout=log, stderr=subprocess.STDOUT,
                            start_new_session=True)
    children.append(proc)
    eventually(lambda: sock.exists() and Client(sock).call('health')['ok'], 10)
    return proc


def cli(env, *args, ok=True):
    q = subprocess.run([str(binary), *map(str, args)], env=env, capture_output=True, text=True, timeout=30)
    if ok:
        assert q.returncode == 0, (args, q.stdout, q.stderr)
    return q


def raw(sock, method, params=None):
    """Return ('ok', result) or ('error', code) without asserting."""
    try:
        c = Client(sock)
        r = {'version': 0, 'id': os.urandom(8).hex(), 'method': method, 'params': params or {}}
        b = json.dumps(r).encode()
        c.s.sendall(struct.pack('!I', len(b)) + b)
        n = struct.unpack('!I', c.s.recv(4, sk.MSG_WAITALL))[0]
        v = json.loads(c.s.recv(n, sk.MSG_WAITALL))
        c.close()
        return ('error', v['error']['code']) if 'error' in v else ('ok', v['result'])
    except OSError as e:
        return ('error', type(e).__name__)


def live_shell(sock, name):
    c = Client(sock)
    row = c.call('session.create', {'name': name, 'rows': 24, 'cols': 80,
                                    'argv': ['/bin/sh', '-c', 'printf READY_' + name + '; exec cat']})
    c.call('pane.wait', {'pane': row['pane'], 'contains': 'READY_' + name, 'timeout_ms': 5000})
    return row


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except OSError:
        return False


own = version(binary)
assert version(older) != own and version(legacy) != own, 'fixture binaries must have distinct versions'
try:
    # 1. stopIfIdle refuses a live PTY; the program keeps running and accepts input.
    env, sock = world('busy')
    engine = serve(binary, sock, env)
    row = live_shell(sock, 'work')
    health = Client(sock).call('health')
    check('health reports live and ended terminals', health['live_panes'] == 1 and health['ended_panes'] == 0 and health['safe_stop'])
    refused = raw(sock, 'daemon.stopIfIdle')
    check('stopIfIdle refuses a live terminal', refused == ('error', 'ENGINE_BUSY'), refused)
    c = Client(sock)
    c.call('pane.sendText', {'pane': row['pane'], 'text': 'STILL_HERE\r'})
    c.call('pane.wait', {'pane': row['pane'], 'contains': 'STILL_HERE', 'timeout_ms': 5000})
    check('refused stop leaves the program running and interactive', alive(row['pid']) and engine.poll() is None)
    check('engine stop CLI refuses while a terminal runs', cli(env, '--socket', sock, 'engine', 'stop', ok=False).returncode != 0
          and alive(row['pid']))

    # 2. An attached client blocks retirement even when every terminal has ended.
    Client(sock).call('pane.close', {'pane': row['pane']})
    ended = Client(sock).call('session.create', {'name': 'ended', 'rows': 24, 'cols': 80,
                                                 'argv': ['/bin/sh', '-c', 'exit 0']})
    eventually(lambda: Client(sock).call('health')['live_panes'] == 0, 10)
    viewer = Client(sock)
    viewer.call('session.attach', {'session': ended['session'], 'rows': 24, 'cols': 80})
    check('stopIfIdle refuses an attached client', raw(sock, 'daemon.stopIfIdle') == ('error', 'ENGINE_BUSY'))
    viewer.call('session.detach')
    viewer.close()

    # 3. Idle: one checkpoint, clean exit, socket released, history readable on restart.
    idle_row = Client(sock).call('session.create', {'name': 'history', 'rows': 24, 'cols': 80,
                                                    'argv': ['/bin/sh', '-c', 'printf HISTORY_KEPT; exit 0']})
    eventually(lambda: Client(sock).call('health')['live_panes'] == 0, 10)
    stopped = raw(sock, 'daemon.stopIfIdle')
    check('idle engine stops through stopIfIdle', stopped[0] == 'ok' and stopped[1]['stopped'] and stopped[1]['live_panes'] == 0, stopped)
    engine.wait(timeout=10)
    check('stopped engine releases its socket', raw(sock, 'version')[0] == 'error')
    serve(binary, sock, env)
    text = Client(sock).call('pane.capture', {'pane': idle_row['pane'], 'lines': 200})['text']
    check('ended history stays readable after retirement', 'HISTORY_KEPT' in text)

    # 4. Race: stopIfIdle against session.create never both succeed.
    outcomes = []
    for i in range(20):
        env_r, sock_r = world(f'race{i}')
        proc = serve(binary, sock_r, env_r)
        with concurrent.futures.ThreadPoolExecutor(2) as pool:
            stop_f = pool.submit(raw, sock_r, 'daemon.stopIfIdle')
            make_f = pool.submit(raw, sock_r, 'session.create',
                                 {'name': 'race', 'argv': ['/bin/sh', '-c', 'exec cat']})
            stop_r, make_r = stop_f.result(), make_f.result()
        both = stop_r[0] == 'ok' and make_r[0] == 'ok'
        outcomes.append((stop_r[0], make_r[0]))
        assert not both, ('stop and create both succeeded', i)
        if make_r[0] == 'ok':
            assert proc.poll() is None and alive(make_r[1]['pid'])
            os.killpg(proc.pid, signal.SIGTERM)
        proc.wait(timeout=10)
    check('stopIfIdle and session.create are serialized (20 races)', True, outcomes)

    # 5. Routing: a busy older safe engine is preserved; new work goes beside it.
    env, sock = world('route-busy')
    old = serve(older, sock, env)
    kept = live_shell(sock, 'kept')
    made = json.loads(cli(env, 'session', 'create', '--name', 'fresh', '--json').stdout)['result']
    sibling = sock.parent / f'engine-{own}.sock'
    check('busy older engine is preserved untouched', old.poll() is None and alive(kept['pid'])
          and raw(sock, 'version')[1]['package'] == version(older))
    check('new session starts on the installed engine beside it', raw(sibling, 'version')[1]['package'] == own
          and any(s['id'] == made['session'] for s in Client(sibling).call('session.list')))
    route = json.loads((sock.parent / 'route.json').read_text())
    check('route record names the installed engine', route['socket'] == str(sibling) and route['engine'] == own
          and (sock.parent / 'route.json').stat().st_mode & 0o777 == 0o600)
    listing = json.loads(cli(env, '-ls', '--json').stdout)['result']
    check('session list spans both engines with labels', {(r['name'], r['engine']) for r in listing}
          >= {('kept', version(older)), ('fresh', own)})
    status = json.loads(cli(env, 'engine', 'status', '--json').stdout)['result']
    machine = status['machine']
    versions = {e['version']: e for e in machine['engines']}
    check('status inventories every engine with live counts', set(versions) == {version(older), own}
          and versions[version(older)]['live_terminals'] == 1 and machine['route_for_new_sessions'] == str(sibling)
          and status['installed_cli'] == own and 'running' in status)
    again = json.loads(cli(env, 'session', 'create', '--name', 'fresh2', '--json').stdout)['result']
    check('later new sessions reuse the same installed engine', any(s['id'] == again['session'] for s in Client(sibling).call('session.list'))
          and len(list(sock.parent.glob('engine-*.sock'))) == 1)

    # 6. Routing: an idle older safe engine is retired atomically on the default socket.
    env, sock = world('route-idle')
    idle_old = serve(older, sock, env)
    Client(sock).call('session.create', {'name': 'old-history', 'argv': ['/bin/sh', '-c', 'printf OLD_HISTORY; exit 0']})
    eventually(lambda: Client(sock).call('health')['live_panes'] == 0, 10)
    made = json.loads(cli(env, 'session', 'create', '--name', 'fresh', '--json').stdout)['result']
    idle_old.wait(timeout=10)
    check('idle older engine retired; installed engine took the default socket',
          raw(sock, 'version')[1]['package'] == own and not list(sock.parent.glob('engine-*.sock')))
    names = {s['name'] for s in Client(sock).call('session.list')}
    check('retired engine history is readable on the new engine', {'old-history', 'fresh'} <= names)

    # 7. Concurrent new-work launches converge on one installed engine.
    env, sock = world('route-concurrent')
    busy = serve(older, sock, env)
    live_shell(sock, 'busy')
    with concurrent.futures.ThreadPoolExecutor(12) as pool:
        results = list(pool.map(lambda i: cli(env, 'session', 'create', '--name', f'c{i}', '--json', ok=False), range(24)))
    ok = [r for r in results if r.returncode == 0]
    engines = list(sock.parent.glob('engine-*.sock'))
    check('24 concurrent launches converge on one installed engine', len(engines) == 1 and len(ok) == 24 and busy.poll() is None,
          [r.stderr[-200:] for r in results if r.returncode])

    # 8. Legacy engines (no stopIfIdle) are never stopped automatically.
    env, sock = world('legacy')
    leg = serve(legacy, sock, env)
    Client(sock).call('session.create', {'name': 'gone', 'argv': ['/bin/sh', '-c', 'exit 0']})
    eventually(lambda: all(p['ended'] for s in Client(sock).call('session.list') for p in Client(sock).call('pane.list', {'session': s['id']})), 10)
    cli(env, 'session', 'create', '--name', 'fresh', '--json')
    check('idle legacy engine is preserved, never auto-stopped', leg.poll() is None
          and raw(sock, 'version')[1]['package'] == version(legacy))
    q = cli(env, '--socket', sock, 'engine', 'stop', ok=False)
    check('legacy stop requires naming the engine PID', q.returncode != 0 and 'legacy-confirm' in q.stderr + q.stdout and leg.poll() is None)
    q = cli(env, '--socket', sock, 'engine', 'stop', '--legacy-confirm', '1', ok=False)
    check('legacy stop refuses a wrong PID', q.returncode != 0 and leg.poll() is None)
    q = cli(env, '--socket', sock, 'engine', 'stop', '--legacy-confirm', leg.pid)
    leg.wait(timeout=10)
    check('explicit confirmed legacy stop of an idle engine succeeds', leg.poll() is not None)

    (a.evidence / 'acceptance.json').write_text(json.dumps({'PASS': True, 'checks': checks, 'versions': {
        'candidate': own, 'older_safe': version(older), 'legacy': version(legacy)}}, indent=2) + '\n')
    print('engine lifecycle PASS', len(checks), 'checks')
finally:
    for proc in children:
        if proc.poll() is None:
            try:
                os.killpg(proc.pid, signal.SIGTERM)
            except OSError:
                pass
    for proc in children:
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(proc.pid, signal.SIGKILL)
    # Engines started by the routed CLI (siblings) belong to this test too.
    for base in root.iterdir() if root.exists() else []:
        for s in (base / 'run/ruwac').glob('*.sock') if (base / 'run/ruwac').exists() else []:
            try:
                c = sk.socket(sk.AF_UNIX)
                c.connect(str(s))
                pid = struct.unpack('3i', c.getsockopt(sk.SOL_SOCKET, sk.SO_PEERCRED, 12))[0]
                c.close()
                os.kill(pid, signal.SIGTERM)
            except OSError:
                pass
