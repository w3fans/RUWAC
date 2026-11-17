#!/usr/bin/env python3
"""Named sessions are found across engines, including from inside a pane of
another engine (RUWAC_SOCKET set); a name held in two engines is refused."""
import argparse, json, os, subprocess, tempfile, time
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--evidence', type=Path, required=True)
a = p.parse_args()
root = Path(tempfile.mkdtemp(prefix='engine-lookup-', dir=a.evidence)); root.chmod(0o700)
runtime = root / 'xdg'; runtime.mkdir(mode=0o700)
engines = runtime / 'ruwac'; engines.mkdir(mode=0o700)
base = {k: v for k, v in os.environ.items() if not k.startswith('RUWAC_')}
base.update(XDG_RUNTIME_DIR=str(runtime), HOME=str(root), SHELL='/bin/sh', TERM='xterm-256color')
first, second = engines / 'control.sock', engines / 'engine-9.9.9.sock'
checks, daemons = [], []


def run(*args, socket=None, pane=None, check=True):
    env = dict(base)
    if pane:
        env['RUWAC_SOCKET'] = str(pane)  # as inherited by every pane of that engine
    argv = [a.binary] + (['--socket', str(socket)] if socket else []) + list(args)
    q = subprocess.run(argv, env=env, capture_output=True, text=True, timeout=20)
    if check and q.returncode:
        raise AssertionError((argv, q.returncode, q.stdout, q.stderr))
    return q


def sessions(socket):
    return json.loads(run('session', 'list', '--json', socket=socket).stdout)['result']


def create(socket, name):
    return json.loads(run('session', 'create', '--name', name, '--detach', '--json',
                          socket=socket).stdout)['result']['session']


try:
    for sock in [first, second]:
        daemons.append(subprocess.Popen([a.binary, '--socket', str(sock), 'serve'], env=base,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
        for _ in range(100):
            if sock.exists():
                break
            time.sleep(.05)
    d = create(first, 'd')

    # A pane of the second engine asks for "d": it lives in the first engine.
    q = run('-dmS', 'd', pane=second)
    assert d in q.stdout, q.stdout
    assert not [s for s in sessions(second) if s['name'] == 'd'], sessions(second)
    checks.append('pane of another engine reuses the existing named session')

    # A genuinely new name from inside a pane stays in the pane's engine.
    run('-dmS', 'fresh', pane=second)
    assert [s for s in sessions(second) if s['name'] == 'fresh']
    assert not [s for s in sessions(first) if s['name'] == 'fresh']
    checks.append('new named session from a pane is created in that pane engine')

    # The same name in two engines is ambiguous: refused, with both IDs.
    d2 = create(second, 'd')
    for pane in [second, None]:
        q = run('-dmS', 'd', pane=pane, check=False)
        assert q.returncode != 0 and 'exists in 2 engines' in q.stderr, (q.stdout, q.stderr)
        assert d in q.stderr and d2 in q.stderr, q.stderr
    assert len([s for s in sessions(first) + sessions(second) if s['name'] == 'd']) == 2
    checks.append('name held in two engines is refused from a pane and from outside')

    q = run('-dmS', d2, pane=first)
    assert d2 in q.stdout, q.stdout
    checks.append('session ID selects the session in its own engine')

    listing = run('-ls', pane=second).stdout
    assert 'Engine 9.9.9' not in listing and listing.count('Engine ') == 2, listing
    assert '! Session name "d" exists in engines' in listing, listing
    rows = json.loads(run('-ls', '--json').stdout)['result']
    dup = {r['id']: r['duplicate_name'] for r in rows}
    assert dup[d] and dup[d2] and not any(dup[r['id']] for r in rows if r['name'] != 'd'), rows
    assert {r['engine_socket'] for r in rows} == {str(first), str(second)}, rows
    checks.append('-ls names each engine and flags the duplicate name')

    (a.evidence / 'engine-session-lookup.json').write_text(
        json.dumps({'status': 'PASS', 'checks': checks}, indent=2))
    print(f'Engine session lookup PASS {len(checks)} checks')
finally:
    for q in daemons:
        q.terminate()
        try:
            q.wait(timeout=5)
        except subprocess.TimeoutExpired:
            q.kill()
