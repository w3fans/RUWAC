"""In-process windows, cross-window tab moves and "Open in new window" for saved
workspaces, on an isolated Mutter display with generated sessions only.

Requires the existing native fixture wrapper with RUWAC_TEST_MUTTER_INPUT=1.
Never run this against an owner desktop, daemon socket or configuration.
"""
from pathlib import Path
import argparse, json, os, subprocess, time
from spike import Client, eventually

p = argparse.ArgumentParser()
for n in ['binary', 'gui', 'evidence']:
    p.add_argument('--' + n, required=True)
a = p.parse_args()
assert os.environ.get('WAYLAND_DISPLAY') == 'ruwac-test'
r = Path(a.evidence) / 'multi-window'
r.mkdir(mode=0o700)
sock = r / 's'
children, checks, seq = [], [], 0
env = {**os.environ, 'GTK_A11Y': 'none', 'GSETTINGS_BACKEND': 'memory', 'SHELL': '/bin/bash'}
for k, n in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache')]:
    d = r / n
    d.mkdir(mode=0o700)
    env[k] = str(d)
for k in ['RUWAC_STATE_FILE', 'RUWAC_SOCKET', 'RUWAC_SESSION', 'RUWAC_PANE', 'TMUX', 'STY']:
    env.pop(k, None)
(r / 'config/RUWAC').mkdir(mode=0o700)


def start(args, name):
    q = subprocess.Popen(list(map(str, args)), env=env, stdout=(r / (name + '.log')).open('w'), stderr=subprocess.STDOUT)
    children.append(q)
    return q


def rpc(m, params=None, gui=False):
    with Client(sock.with_suffix('.gui.sock') if gui else sock) as c:
        return c.call(m, params)


def command(action, **kw):
    global seq
    seq += 1
    t = r / 'request.tmp'
    t.write_text(json.dumps({'seq': seq, 'action': action, **kw}))
    t.replace(r / 'gui-request.json')

    def answer():
        try:
            v = json.loads((r / 'gui-response.json').read_text())
            return v['result'] if v['seq'] == seq else None
        except (OSError, ValueError):
            return None
    return eventually(answer, 12)


def mark(name, **details):
    checks.append({'name': name, **details})
    print(name, 'PASS', flush=True)
    (r / 'checks.json').write_text(json.dumps(checks, indent=2) + '\n')


def panes():
    return {q['id']: q['pid'] for s in rpc('session.list') for q in rpc('pane.list', {'session': s['id']})}


def windows():
    return command('detach-tab', operation='inspect')['windows']


def views():
    return rpc('workspace.list', gui=True)['views']


try:
    start([a.binary, '--socket', sock, 'serve'], 'daemon')
    eventually(sock.exists)
    seeds = [rpc('session.create', {'name': 'generated-' + str(i), 'argv': ['/bin/sleep', '600']})['session'] for i in range(3)]
    baseline = panes()
    args = [a.gui, '--new-instance', '--socket', sock, '--allow-workspace-control', '--fixture-dir', r]
    for seed in seeds:
        args += ['--session', seed]
    gui = start(args, 'gui')
    eventually(lambda: len(windows()) == 1 and len(windows()[0]['tabs']) == 3, 20)

    # 1. The window button opens a window in the same process with a new terminal.
    command('detach-tab', operation='new-window')
    eventually(lambda: len(windows()) == 2 and len(windows()[1]['tabs']) == 1, 20)
    assert gui.poll() is None and sum(1 for c in children if c.poll() is None) == 2  # daemon + one GUI process
    new_session = eventually(lambda: next((v['session'] for v in views() if v['session'] and v['session'] not in seeds), None), 20)
    assert set(baseline.items()) <= set(panes().items())
    mark('window-button-opens-an-in-process-window-with-a-new-terminal', session=new_session)

    # 2. Tabs move between windows (the libadwaita transfer used by tab drops) without restarting PTYs.
    command('detach-tab', operation='return', window=1)
    eventually(lambda: len(windows()[0]['tabs']) == 4, 10)
    command('detach-tab', operation='detach', tab=0)
    eventually(lambda: len(windows()) >= 2, 10)
    assert set(baseline.items()) <= set(panes().items())
    mark('tabs-move-between-windows-and-keep-their-PTYs')
    # Keep that tab in its detached window: the saved layout spans two windows.
    eventually(lambda: sum(len(w['tabs']) for w in windows()) == 4, 10)

    # 3. A saved workspace opens in a new window and continues its sessions.
    command('workspace-picker', operation='show')
    command('workspace-picker', operation='name', value='Generated layout')
    command('workspace-picker', operation='save')
    saved = eventually(lambda: next((s for s in rpc('workspace.saved', gui=True)['saved_workspaces']
                                     if s['name'] == 'Generated layout'), None), 10)
    before_windows = len(windows())
    before_list = views()
    (r / 'views-before.json').write_text(json.dumps(before_list, indent=2))
    before_views = {v['id'] for v in before_list}
    command('workspace-picker', operation='refresh')
    time.sleep(0.5)
    command('workspace-picker', operation='window', value=saved['id'])
    eventually(lambda: len(windows()) == before_windows + 1, 15)
    saved_sessions = {v['session'] for v in before_list if v['session']}
    def moved():
        now = views()
        new_tabs = {t['id'] for t in windows()[-1]['tabs']}
        inside = [v for v in now if v['tab'] in new_tabs]
        return (now, inside) if len(inside) == saved['tabs'] else None
    now, inside = eventually(moved, 15)
    (r / 'views-after.json').write_text(json.dumps(now, indent=2))
    # Same sessions continue in the new window: moved, never duplicated or lost.
    assert {v['session'] for v in inside} == saved_sessions, (inside, saved_sessions)
    assert len(now) == len(before_list) and {v['session'] for v in now} == saved_sessions
    assert set(baseline.items()) <= set(panes().items())
    mark('saved-workspace-opens-in-a-new-window-continuing-the-same-sessions',
         tabs=saved['tabs'], sessions=sorted(saved_sessions))
    (r / 'acceptance.json').write_text(json.dumps({'PASS': True, 'checks': checks,
                                                  'scope': 'Isolated Mutter display; fixture-driven button click and tab transfer; '
                                                           'pointer drag between windows and launcher remote activation not driven.'},
                                                 indent=2) + '\n')
finally:
    for c in reversed(children):
        if c.poll() is None:
            c.terminate()
    for c in reversed(children):
        try:
            c.wait(timeout=8)
        except subprocess.TimeoutExpired:
            c.kill()
