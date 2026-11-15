#!/usr/bin/env python3
"""Owned raw PTY receiver: report real ioctl size and delivered SIGWINCH count."""
import fcntl
import json
import os
from pathlib import Path
import signal
import struct
import sys
import termios
import tty

tty.setraw(0)
root = Path(sys.argv[1])
received = (root / 'input.bin').open('ab', buffering=0)
signals = 0


def resized(*_):
    global signals
    signals += 1


signal.signal(signal.SIGWINCH, resized)
os.write(1, b'\x1b[?1049h\x1b[31mSIZING_READY\x1b[0m\r\n')
while True:
    data = os.read(0, 8192)
    if not data:
        break
    received.write(data)
    if b'?' in data:
        rows, cols, _, _ = struct.unpack('HHHH', fcntl.ioctl(0, termios.TIOCGWINSZ, b'\0' * 8))
        temporary = root / 'state.tmp'
        temporary.write_text(json.dumps({'pid': os.getpid(), 'rows': rows, 'cols': cols,
                                         'signals': signals, 'bytes': (root / 'input.bin').stat().st_size}))
        temporary.replace(root / 'state.json')
