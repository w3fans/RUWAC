#!/usr/bin/env python3
"""Nested client mouse: wheel scrollback, drag selection and middle-click paste
for programs without mouse support; mouse programs still receive the events."""
import argparse, json, os, pty, re, select, signal, subprocess, sys, tempfile, time
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--binary', required=True)
p.add_argument('--evidence', type=Path, required=True)
a = p.parse_args()
root = Path(tempfile.mkdtemp(prefix='nested-mouse-', dir=a.evidence)); root.chmod(0o700)
sock = root / 'rt' / 'c.sock'; sock.parent.mkdir(mode=0o700)
env = {**os.environ, 'TERM': 'xterm-256color', 'SHELL': '/bin/sh', 'ENV': '', 'PS1': '$ '}
checks = []


def rows(raw):
    """Screen rows drawn in `raw`: {row: text} with style codes removed."""
    text = raw.decode(errors='replace')
    out = {}
    for m in re.finditer(r'\x1b\[(\d+);1H((?:(?!\x1b\[\d+;1H).)*)', text, re.S):
        out[int(m.group(1))] = re.sub(r'\x1b\[[0-9;?]*[A-Za-z]', '', m.group(2))
    return out


def numbers(raw):
    return sorted(int(n) for line in rows(raw).values() for n in re.findall(r'line-(\d+)', line))


def cli(*args):
    return json.loads(subprocess.check_output([a.binary, '--socket', str(sock), *args], env=env))


daemon = subprocess.Popen([a.binary, '--socket', str(sock), 'serve'], env=env,
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
pid = None
try:
    for _ in range(100):
        if sock.exists():
            break
        time.sleep(.05)
    subprocess.run([a.binary, '--socket', str(sock), '-dmS', 'mouse'], env=env, check=True,
                   stdout=subprocess.DEVNULL)
    session = cli('session', 'list', '--json')['result'][0]['id']
    pane = cli('pane', 'list', '--session', session, '--json')['result'][0]['id']
    pid, fd = pty.fork()
    if pid == 0:
        os.execve(a.binary, [a.binary, '--socket', str(sock), '-r', 'mouse'], env)
    import fcntl, struct, termios
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 80, 0, 0))

    def drain(t):
        end = time.time() + t; out = b''
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], .05)
            if r:
                try:
                    out += os.read(fd, 1 << 20)
                except OSError:
                    break
        return out

    def capture():
        return cli('pane', 'capture', '--id', pane, '--lines', '400', '--json')['result']['text']

    drain(1.5)
    os.write(fd, b'i=1; while [ $i -le 200 ]; do echo line-$i; i=$((i+1)); done\r')
    for _ in range(100):
        if 'line-200' in capture():
            break
        time.sleep(.05)
    out = drain(.8)
    assert b'\x1b[?1002h' in out, 'client requests drag reports for a shell'
    checks.append('shell-pane-requests-drag-and-wheel-reports')

    os.write(fd, b'\x1b[<64;10;10M' * 20)
    older = numbers(drain(.8))
    assert older and min(older) < 150, older[:5]
    checks.append('wheel-up-opens-scrollback-at-older-lines')
    os.write(fd, b'\x1b[<65;10;10M' * 40)
    out = drain(.8)
    assert b'\x1b[2J' in out or b'line-200' in out
    checks.append('wheel-down-returns-to-live-view')

    # Scroll up again and pick a visible history line to select.
    os.write(fd, b'\x1b[<64;10;10M' * 20)
    visible = rows(drain(.8))
    target_row = next(((r, m.group(0)) for r, line in sorted(visible.items())
                       for m in [re.match(r'line-\d+', line.strip())] if m), None)
    assert target_row, visible
    row, word = target_row
    os.write(fd, f'\x1b[<0;1;{row}M\x1b[<32;{len(word)};{row}M\x1b[<0;{len(word)};{row}m'.encode())
    drain(.8)
    copied = cli('rpc', 'buffer.get', '--params', json.dumps({'session': session}))['result']['text']
    assert copied.strip() == word, (copied, word)
    checks.append('drag-selects-into-session-buffer')

    os.write(fd, b'\x1b[<1;5;5M\x1b[<1;5;5m')
    for _ in range(100):
        if capture().rstrip().endswith(word):
            break
        time.sleep(.05)
    assert capture().rstrip().endswith(word), capture()[-200:]
    checks.append('middle-click-pastes-session-buffer')

    logger = root / 'logger.py'; log = root / 'mouse.log'
    logger.write_text("import os,sys,tty\nlog=open(sys.argv[1],'ab',0)\ntty.setraw(0)\n"
                      "os.write(1,b'\\x1b[?1000h\\x1b[?1006hREADY')\n"
                      "while True:\n b=os.read(0,256)\n if not b or b'q' in b: break\n log.write(b)\n")
    os.write(fd, f'\x15python3 {logger} {log}\r'.encode())
    for _ in range(100):
        if 'READY' in capture():
            break
        time.sleep(.05)
    time.sleep(.5); drain(.5)
    os.write(fd, b'\x1b[<64;7;5M')
    for _ in range(100):
        if log.exists() and b'<64;7;' in log.read_bytes():
            break
        time.sleep(.05)
    assert log.exists() and b'\x1b[<64;7;' in log.read_bytes(), log.read_bytes() if log.exists() else 'no log'
    checks.append('mouse-program-receives-wheel-in-pane-coordinates')

    # Middle-click (three-finger tap) pastes even while a program tracks the
    # mouse: the program gets the pasted text, never the button event.
    before = log.read_bytes()
    os.write(fd, b'\x1b[<1;5;5M\x1b[<1;5;5m')
    for _ in range(100):
        if word.encode() in log.read_bytes()[len(before):]:
            break
        time.sleep(.05)
    delta = log.read_bytes()[len(before):]
    assert word.encode() in delta and b'\x1b[<1;' not in delta, delta
    checks.append('middle-click-pastes-into-mouse-program-without-forwarding-the-button')
    os.write(fd, b'q')
    for _ in range(100):
        if capture().rstrip().endswith('$'):
            break
        time.sleep(.05)

    # An agent (here a generated program named codex) that tracks all mouse
    # motion on the alternate screen: a plain drag selects in the client, a
    # plain click and the wheel still reach the agent.
    import shutil
    agent = root / 'codex'; shutil.copy2(sys.executable, agent); agent.chmod(0o700)
    fixture = root / 'agent.py'; agent_log = root / 'agent.log'
    fixture.write_text("import os,sys,tty\nlog=open(sys.argv[1],'ab',0)\ntty.setraw(0)\n"
                       "os.write(1,b'\\x1b[?1049h\\x1b[?1000h\\x1b[?1002h\\x1b[?1003h\\x1b[?1006h\\x1b[2J\\x1b[3;1HAGENTWORD remainder\\x1b[5;1HAGENT_READY')\n"
                       "while True:\n b=os.read(0,256)\n if not b or b'q' in b: break\n log.write(b)\n"
                       "os.write(1,b'\\x1b[?1003l\\x1b[?1002l\\x1b[?1000l\\x1b[?1006l\\x1b[?1049l')\n")
    os.write(fd, f'\x15{agent} {fixture} {agent_log}\r'.encode())
    for _ in range(100):
        if 'AGENT_READY' in capture():
            break
        time.sleep(.05)
    assert 'AGENT_READY' in capture(), capture()[-200:]
    probe = cli('rpc', 'agent.probe', '--params', json.dumps({'pane': pane}))['result']
    assert probe['adapter'] == 'codex' and probe['process_detected'], probe
    time.sleep(.5); drain(.5)
    os.write(fd, b'\x1b[<0;1;3M\x1b[<32;5;3M\x1b[<32;9;3M\x1b[<0;9;3m')
    for _ in range(100):
        if cli('rpc', 'buffer.get', '--params', json.dumps({'session': session}))['result']['text'].strip() == 'AGENTWORD':
            break
        time.sleep(.05)
    copied = cli('rpc', 'buffer.get', '--params', json.dumps({'session': session}))['result']['text']
    assert copied.strip() == 'AGENTWORD', copied
    seen = agent_log.read_bytes() if agent_log.exists() else b''
    assert b'\x1b[<0;' not in seen and b'\x1b[<32;' not in seen, seen
    checks.append('agent-pane-drag-selects-in-client-not-in-agent')
    drain(.5)
    os.write(fd, b'\x1b[<0;4;6M\x1b[<0;4;6m')
    for _ in range(100):
        seen = agent_log.read_bytes() if agent_log.exists() else b''
        if b'\x1b[<0;4;6M' in seen and b'\x1b[<0;4;6m' in seen:
            break
        time.sleep(.05)
    assert b'\x1b[<0;4;6M' in seen and b'\x1b[<0;4;6m' in seen, seen
    checks.append('agent-pane-plain-click-still-reaches-agent')
    os.write(fd, b'\x1b[<64;7;5M')
    for _ in range(100):
        if b'\x1b[<64;7;5M' in agent_log.read_bytes():
            break
        time.sleep(.05)
    assert b'\x1b[<64;7;5M' in agent_log.read_bytes(), agent_log.read_bytes()
    checks.append('agent-pane-wheel-still-reaches-agent')
    before = agent_log.read_bytes()
    os.write(fd, b'\x1b[<1;5;5M\x1b[<1;5;5m')
    for _ in range(100):
        if b'AGENTWORD' in agent_log.read_bytes()[len(before):]:
            break
        time.sleep(.05)
    delta = agent_log.read_bytes()[len(before):]
    assert b'AGENTWORD' in delta and b'\x1b[<1;' not in delta, delta
    checks.append('agent-pane-middle-click-pastes-the-selection')

    (a.evidence / 'nested-mouse.json').write_text(json.dumps({'status': 'PASS', 'checks': checks}, indent=2))
    print(f'Nested mouse PASS {len(checks)} checks')
finally:
    if pid:
        os.kill(pid, signal.SIGKILL)
    daemon.terminate(); daemon.wait(timeout=10)
