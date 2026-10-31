#!/usr/bin/env python3
"""Non-sizing viewer vertical slice; owned real PTYs, unchanged legacy votes."""
import argparse
import hashlib
import fcntl
import json
import os
import re
import signal
import struct
import termios
from pathlib import Path
import subprocess
import sys
import tempfile
import traceback

from spike import Client, Tui, eventually


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    args.evidence.mkdir(parents=True, exist_ok=True)
    # Socket paths must stay short even when the evidence root is deeply nested.
    root = Path(tempfile.mkdtemp(prefix='sv-'))
    root.chmod(0o700)
    binary = args.binary.resolve()
    socket = root / 's'
    env = dict(os.environ)
    for key, leaf in [('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_DATA_HOME', 'data'), ('XDG_CACHE_HOME', 'cache')]:
        (root / leaf).mkdir(mode=0o700)
        env[key] = str(root / leaf)
    fixture = Path(__file__).resolve().parents[1] / 'fixtures/sizing_receiver.py'
    results = []
    daemon = None
    live = []
    try:
        with (args.evidence / 'daemon.log').open('wb') as log:
            daemon = subprocess.Popen([str(binary), '--socket', str(socket), 'serve'],
                                      env=env, stdout=log, stderr=log)
        eventually(socket.exists)
        with Client(socket) as admin, Client(socket) as owner, Client(socket) as viewer:
            created = admin.call('session.create', {'name': 'Generated sizing', 'rows': 42, 'cols': 120,
                'argv': [sys.executable, str(fixture), str(root)]})
            sid, pane, window, pid = (created[k] for k in ['session', 'pane', 'window', 'pid'])
            admin.call('pane.wait', {'pane': pane, 'contains': 'SIZING_READY', 'timeout_ms': 5000})
            owner.call('session.attach', {'session': sid, 'rows': 42, 'cols': 120})
            total = 0

            def probe():
                nonlocal total
                total += 1
                owner.call('client.input', {'pane': pane, 'kind': 'keyboard', 'text': '?'})
                eventually(lambda: (root / 'state.json').exists() and
                           json.loads((root / 'state.json').read_text())['bytes'] == total)
                return json.loads((root / 'state.json').read_text())

            def record(name):
                results.append({'id': name, 'status': 'PASS'})
                print(name, 'PASS', flush=True)

            # Changing an existing sizing attachment into a viewer removes its
            # old minimum vote atomically while the larger owner remains.
            with Client(socket) as role_change:
                role_change.call('session.attach', {'session':sid,'rows':18,'cols':56})
                assert (probe()['rows'], probe()['cols']) == (18,56)
                role_change.call('session.attach', {'session':sid,'rows':18,'cols':56,'size_role':'viewer'})
                assert (probe()['rows'], probe()['cols']) == (42,120)
            record('VIEWER-ROLE-CHANGE-REMOVES-OLD-SIZE-VOTE')
            initial = probe()
            viewer.call('session.attach', {'session': sid, 'rows': 18, 'cols': 56, 'size_role': 'viewer'})
            joined = probe()
            assert joined['pid'] == pid and (joined['rows'], joined['cols']) == (42, 120), (initial, joined)
            assert joined['signals'] == initial['signals'], (initial, joined)
            assert admin.call('capabilities')['non_sizing_viewer'] == 1
            record('VIEWER-REAL-PTY-JOIN-NO-WINCH')
            for bad in ['unknown',None,True,1]:
                viewer.call('session.attach', {'session':sid,'rows':10,'cols':30,'size_role':bad}, error='INVALID_PARAMS')
                assert viewer.call('client.render')['size_role']=='viewer'
            assert probe()['signals']==initial['signals']
            record('VIEWER-INVALID-ROLE-PRESERVES-ATTACHMENT')
            before=viewer.call('client.render')
            viewer.call('client.resize', {'rows':17,'cols':55})
            changed=viewer.call('client.render', {'revision':before['revision']})
            assert changed.get('unchanged') is not True and changed['viewport']=={'rows':17,'cols':55}
            assert changed['panes'][0]['grid']['cols']==120 and probe()['signals']==initial['signals']
            record('VIEWER-ALLOCATION-INVALIDATES-PROJECTION-WITHOUT-WINCH')

            for _ in range(100):
                reply = viewer.call('client.resize', {'rows': 12, 'cols': 44})
                assert reply['resized'] is False and reply['viewport_resized'] is True
                viewer.call('pane.layout', {'window': window, 'zoom': True})
                frame = viewer.call('client.render')
                assert frame['size_role'] == 'viewer' and frame['viewport'] == {'rows': 12, 'cols': 44}
                assert (frame['panes'][0]['grid']['rows'], frame['panes'][0]['grid']['cols']) == (42, 120)
                viewer.call('session.detach')
                viewer.call('session.attach', {'session': sid, 'rows': 18, 'cols': 56, 'size_role': 'viewer'})
            after = probe()
            assert after['pid'] == pid and after['signals'] == initial['signals'], (initial, after)
            assert (after['rows'], after['cols']) == (42, 120)
            record('VIEWER-100-RESIZE-ZOOM-REATTACH-CYCLES')

            total += len('VIEWER_INPUT')
            viewer.call('client.input', {'pane': pane, 'kind': 'keyboard', 'text': 'VIEWER_INPUT'})
            after = probe()
            assert b'VIEWER_INPUT' in (root / 'input.bin').read_bytes()
            assert after['signals'] == initial['signals']
            record('VIEWER-INDEPENDENT-INPUT')

            viewer.call('window.create', {'session': sid}, error='SIZE_NOT_OWNED')
            viewer.call('pane.create', {'window': window}, error='SIZE_NOT_OWNED')
            viewer.call('pane.close', {'pane': pane}, error='SIZE_NOT_OWNED')
            viewer.call('pane.layout', {'window': window, 'split': 'not-owned', 'ratio': 250}, error='SIZE_NOT_OWNED')
            assert probe()['signals'] == initial['signals']
            record('VIEWER-STRUCTURAL-RESIZE-ACTIONS-REFUSED')

            # Old plain attachments remain sizing clients. Keep this explicit.
            with Client(socket) as legacy:
                legacy.call('session.attach', {'session': sid, 'rows': 25, 'cols': 80})
                legacy_size = probe()
                assert (legacy_size['rows'], legacy_size['cols']) == (25, 80)
                legacy.call('session.detach')
            assert (probe()['rows'], probe()['cols']) == (42, 120)
            record('VIEWER-LEGACY-MINIMUM-UNCHANGED')

            # Invalid role cannot detach/replace an existing sizing attachment.
            owner.call('session.attach', {'session': sid, 'rows': 12, 'cols': 44,
                                         'size_role': 'unknown'}, error='INVALID_PARAMS')
            assert (probe()['rows'], probe()['cols']) == (42, 120)
            record('VIEWER-INVALID-ROLE-ATOMIC-REFUSAL')

            # An unseen window must keep canonical default focus and zoom;
            # there is no sizing client viewing this particular window.
            dirs = [root / 'unseen-a', root / 'unseen-b']
            for d in dirs: d.mkdir()
            unseen = admin.call('window.create', {'session': sid,
                'argv': [sys.executable, str(fixture), str(dirs[0])]})
            unseen_b = admin.call('pane.create', {'window': unseen['window'], 'orientation': 'horizontal',
                'argv': [sys.executable, str(fixture), str(dirs[1])]})
            for target in [unseen, unseen_b]:
                admin.call('pane.wait', {'pane': target['pane'], 'contains': 'SIZING_READY', 'timeout_ms': 5000})
            def unseen_kernel_sizes():
                out = {}
                for p in admin.call('pane.list', {'session': sid}):
                    if p['window'] == unseen['window']:
                        fd = os.open(f"/proc/{p['pid']}/fd/0", os.O_RDONLY | os.O_NONBLOCK | os.O_NOCTTY)
                        try: out[p['id']] = struct.unpack('HHHH', fcntl.ioctl(fd, termios.TIOCGWINSZ, b'\0' * 8))[:2]
                        finally: os.close(fd)
                return out
            old_sizes = unseen_kernel_sizes()
            viewer.call('window.select', {'window': unseen['window']})
            viewer.call('pane.select', {'pane': unseen_b['pane']})
            for zoom in [True, False, True]:
                viewer.call('pane.layout', {'window': unseen['window'], 'zoom': zoom})
                assert unseen_kernel_sizes() == old_sizes
            viewer.call('window.select', {'window': window})
            for target in [unseen_b, unseen]: admin.call('pane.close', {'pane': target['pane']})
            record('VIEWER-UNWATCHED-WINDOW-DEFAULTS-DO-NOT-RESIZE')

            # Split once using the sizing client; zoom it. Viewer navigation may
            # never contribute a smaller rectangle or undo the owner's zoom.
            split_dir = root / 'split'
            split_dir.mkdir()
            split_params = {'window': window, 'orientation': 'vertical',
                'argv': [sys.executable, str(fixture), str(split_dir)]}
            viewer.call('pane.create', split_params, key='shared-owner-layout', error='SIZE_NOT_OWNED')
            second = owner.call('pane.create', split_params, key='shared-owner-layout')
            viewer.call('pane.create', split_params, key='shared-owner-layout', error='SIZE_NOT_OWNED')
            record('VIEWER-DENIAL-CANNOT-POISON-OR-REPLAY-OWNER-CACHE')
            owner.call('pane.select', {'pane': pane})
            owner.call('pane.layout', {'window': window, 'zoom': True})
            zoomed = probe()
            assert (zoomed['rows'], zoomed['cols']) == (42, 120)
            viewer.call('pane.select', {'pane': second['pane']})
            viewer.call('pane.layout', {'window': window, 'zoom': False})
            assert probe()['signals'] == zoomed['signals']
            owner.call('session.detach')
            viewer.call('client.resize', {'rows': 10, 'cols': 30})
            # Probe with explicit admin input after the sizing client detaches.
            total += 1
            admin.call('pane.sendText', {'pane': pane, 'text': '?'})
            eventually(lambda: json.loads((root / 'state.json').read_text())['bytes'] == total)
            frozen = json.loads((root / 'state.json').read_text())
            assert (frozen['rows'], frozen['cols']) == (42, 120) and frozen['signals'] == zoomed['signals']
            viewer.call('session.detach')
            viewer.call('session.attach', {'session': sid, 'rows': 18, 'cols': 56, 'size_role': 'viewer'})
            assert next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == pane)['cols'] == 120
            record('VIEWER-SPLIT-ZOOM-AND-FINAL-SIZER-LOSS-FREEZE')
            # Rebinding a viewer to another session is a presentation detach,
            # not a reason to restore the original session's default split.
            admin.call('pane.layout', {'window': window, 'zoom': False})
            other = admin.call('session.create', {'name': 'Other generated namespace', 'argv': ['/bin/cat']})
            viewer.call('pane.select', {'pane': other['pane']}, error='TARGET_SESSION')
            viewer.call('window.select', {'window': other['window']}, error='TARGET_SESSION')
            viewer.call('pane.layout', {'window': other['window'], 'zoom': True}, error='TARGET_SESSION')
            rename = {'window': other['window'], 'name': 'Generated isolated name'}
            viewer.call('window.rename', rename, key='foreign-viewer-denied', error='TARGET_SESSION')
            admin.call('window.rename', rename, key='foreign-viewer-denied')
            record('VIEWER-FOREIGN-NAVIGATION-ISOLATED-BEFORE-CACHE')
            viewer.call('session.attach', {'session': other['session'], 'rows': 12, 'cols': 44, 'size_role': 'viewer'})
            assert next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == pane)['cols'] == 120
            viewer.call('session.attach', {'session': sid, 'rows': 18, 'cols': 56, 'size_role': 'viewer'})
            admin.call('pane.close', {'pane': other['pane']})
            record('VIEWER-CROSS-SESSION-REBIND-FREEZES-OLD-PTY')

            tui = Tui(binary, socket, sid, args.evidence / 'viewer.pty', rows=19, cols=56, env=env,
                      command=['attach', '--session', sid, '-O'])
            live.append(tui)
            eventually(lambda: b'Viewer' in tui.data or tui.p.poll() is not None)
            assert tui.p.poll() is None, bytes(tui.data)
            assert next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == pane)['cols'] == 120
            # Viewer rendering must not run the normal client-side automatic
            # split retirement, which itself resizes the remaining live pane.
            eventually(lambda: b'cropped' in tui.data)
            ended_pid = next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == second['pane'])['pid']
            fd = os.pidfd_open(ended_pid)
            signal.pidfd_send_signal(fd, signal.SIGTERM)
            os.close(fd)
            eventually(lambda: next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == second['pane'])['ended'])
            tui.send(b'VIEWER_LIVE')
            eventually(lambda: (root / 'input.bin').read_bytes().endswith(b'VIEWER_LIVE'))
            assert tui.p.poll() is None
            assert next(p for p in admin.call('pane.list', {'session': sid}) if p['id'] == pane)['cols'] == 120
            record('VIEWER-ENDED-SPLIT-IS-NOT-AUTO-RETIRED')
            assert tui.finish() == 0
            positions = [(int(y), int(x)) for y, x in re.findall(rb'\x1b\[(\d+);(\d+)H', tui.data)]
            assert positions and all(1 <= y <= 19 and 1 <= x <= 56 for y, x in positions), positions
            live.remove(tui)
            record('VIEWER-REAL-TUI-NEGOTIATED-ATTACH-DETACH')
            admin.call('daemon.stop')
        daemon.wait(timeout=5)
    except Exception:
        results.append({'id': 'UNHANDLED', 'status': 'FAIL', 'traceback': traceback.format_exc()})
        raise
    finally:
        for tui in live:
            tui.finish('term')
        if daemon is not None and daemon.poll() is None:
            daemon.terminate()
            daemon.wait(timeout=5)
        (args.evidence / 'result.json').write_text(json.dumps({'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'fixture_sha256': hashlib.sha256(fixture.read_bytes()).hexdigest(), 'cases': results,
            'PASS': len(results) == 16 and all(c['status'] == 'PASS' for c in results)}, indent=2) + '\n')
        # Bounded wrapper owns the short temporary root and cleans it on exit.


if __name__ == '__main__':
    main()
