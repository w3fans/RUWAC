#!/usr/bin/env python3
"""Typing-latency echo target: each received character replaces one screen cell.

Row 1 only ever shows the most recent character, so a repaint can never show a
character before it was typed. --busy adds a 50 Hz alphanumeric noise row, and
--noise-only is a background pane that never reads input. Nothing is logged.
"""
import codecs, os, random, string, sys, termios, threading, time, tty

busy = '--busy' in sys.argv
lock = threading.Lock()


def write(text):
    with lock:
        os.write(1, text.encode())


def noise():
    rng = random.Random(7)
    while True:
        line = ''.join(rng.choice(string.ascii_letters + string.digits) for _ in range(60))
        write('\x1b7\x1b[3;1H' + line + '\x1b8')
        time.sleep(0.02)


if '--noise-only' in sys.argv or busy:
    threading.Thread(target=noise, daemon=True).start()
if '--noise-only' in sys.argv:
    threading.Event().wait()
old = termios.tcgetattr(0)
tty.setraw(0)
decoder = codecs.getincrementaldecoder('utf-8')('replace')
try:
    write('\x1b[2J\x1b[HREADY')
    while True:
        data = os.read(0, 256)
        if not data:
            break
        for char in decoder.decode(data):
            write('\r' + char + '\x1b[K')
finally:
    termios.tcsetattr(0, termios.TCSADRAIN, old)
