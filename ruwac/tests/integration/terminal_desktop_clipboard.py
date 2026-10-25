#!/usr/bin/env python3
"""Generated image through a new terminal's real X11 clipboard connection.

Runs inside the bounded graphical wrapper. No owner clipboard or provider turn.
"""
import argparse
import json
import os
from pathlib import Path
import re
import select
import shutil
import subprocess
import sys
import tempfile
import time

from spike import Client

p = argparse.ArgumentParser()
for name in ['binary', 'daemon', 'evidence']:
    p.add_argument('--' + name, type=Path, required=True)
a = p.parse_args()
assert os.environ.get('RUWAC_TEST_MUTTER_INPUT') == '1'
root = Path(tempfile.mkdtemp(prefix='terminal-clipboard-', dir=a.evidence))
root.chmod(0o700)
children = []
runtime = Path(tempfile.mkdtemp(prefix='ti-', dir='/tmp'))
runtime.chmod(0o700)
desktop_keys = ['DISPLAY', 'WAYLAND_DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR',
                'DBUS_SESSION_BUS_ADDRESS', 'XDG_SESSION_TYPE']
try:
    read, write = os.pipe()
    log = (root / 'xvfb.log').open('wb')
    server = subprocess.Popen(['Xvfb', '-displayfd', str(write), '-screen', '0',
                               '800x600x24', '-nolisten', 'tcp', '-ac'],
                              pass_fds=[write], stdout=log, stderr=log)
    children.append(server)
    os.close(write)
    number = b''
    deadline = time.monotonic() + 5
    try:
        while not number.endswith(b'\n'):
            remaining = deadline - time.monotonic()
            assert remaining > 0 and select.select([read], [], [], remaining)[0]
            chunk = os.read(read, 32 - len(number))
            assert chunk and len(number) + len(chunk) <= 32
            number += chunk
    finally:
        os.close(read)
    assert re.fullmatch(rb'[0-9]+\n', number)
    os.environ.update(DISPLAY=':' + number.decode().strip(), GDK_BACKEND='x11')
    os.environ.pop('XAUTHORITY', None)
    os.environ.pop('WAYLAND_DISPLAY', None)
    import gi
    gi.require_version('Gtk', '3.0')
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gtk, Gdk, GdkPixbuf, GLib
    Gtk.init()
    clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
    image = GdkPixbuf.Pixbuf.new(GdkPixbuf.Colorspace.RGB, False, 8, 3, 2)
    image.fill(0x12ab34ff)
    clipboard.set_image(image)
    result = root / 'clipboard-result.json'
    shell = root / 'selected-shell'
    shell.write_text('#!/usr/bin/python3\nimport gi,json,os\nfrom pathlib import Path\n'
                     'gi.require_version("Gtk","3.0");gi.require_version("Gdk","3.0")\n'
                     'from gi.repository import Gtk,Gdk\nGtk.init()\n'
                     'image=Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD).wait_for_image()\n'
                     'assert image is not None\n'
                     'q=dict(width=image.get_width(),height=image.get_height(),'
                     'channels=image.get_n_channels(),pixels=list(image.get_pixels()),'
                     'rowstride=image.get_rowstride())\n'
                     f'Path({str(result)!r}).write_text(json.dumps(q))\n'
                     'print("GENERATED_IMAGE_READ",flush=True)\nos.execl("/bin/cat","cat")\n')
    shell.chmod(0o700)
    env = {**os.environ, 'SHELL': str(shell), 'RUWAC_STATE_FILE': str(root / 'state.json')}
    # Evidence paths can exceed AF_UNIX's pathname limit. Keep only the private
    # control socket in a short owned directory, and remove it in finally.
    sock = runtime / 'control.sock'
    daemon_env = {key: value for key, value in env.items() if key not in desktop_keys}
    daemon = subprocess.Popen([str(a.daemon.resolve()), '--socket', str(sock), 'serve'],
                              env=daemon_env, stdout=(root / 'daemon.log').open('wb'),
                              stderr=subprocess.STDOUT)
    children.append(daemon)
    while not sock.exists():
        assert daemon.poll() is None
        assert time.monotonic() < deadline + 5
        time.sleep(.02)
    create = subprocess.run([str(a.binary.resolve()), '--socket', str(sock), 'session',
                             'create', '--name', 'generated-image', '--detach', '--json'],
                            env=env, capture_output=True, timeout=10)
    assert create.returncode == 0, create.stderr
    pane = json.loads(create.stdout)['result']['pane']
    deadline = time.monotonic() + 12
    while not result.exists():
        assert time.monotonic() < deadline, 'New terminal could not read the generated clipboard image'
        while GLib.MainContext.default().pending():
            GLib.MainContext.default().iteration(False)
        time.sleep(.02)
    q = json.loads(result.read_text())
    assert (q['width'], q['height'], q['channels']) == (3, 2, 3)
    for y in range(2):
        start = y * q['rowstride']
        assert q['pixels'][start:start + 9] == [0x12, 0xab, 0x34] * 3
    while True:
        with Client(sock) as c:
            if 'GENERATED_IMAGE_READ' in c.call('pane.capture', {'pane': pane})['text']:
                break
        assert time.monotonic() < deadline, 'Image read did not report completion in the PTY'
        time.sleep(.02)
    (a.evidence / 'acceptance.json').write_text(json.dumps({
        'PASS': True, 'checks': ['headless-daemon-preserved', 'new-terminal-connects-to-client-X11',
                                  'exact-generated-image-pixels-read', 'clipboard-read-completes-in-PTY'],
        'scope': 'Real generated X11 clipboard image; no Codex UI input or owner screenshot used'
    }, indent=2) + '\n')
    print('TERMINAL_DESKTOP_CLIPBOARD PASS', flush=True)
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
    shutil.rmtree(runtime)
