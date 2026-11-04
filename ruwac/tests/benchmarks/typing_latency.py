#!/usr/bin/env python3
"""Typing latency: direct PTY, OpenSSH, GNU Screen, tmux, RUWAC and the GUI path.

Measurement only; no RUWAC timing or protocol is changed. One observer process
writes each keystroke and timestamps its echo with one monotonic clock, so no
clocks are compared across machines or processes.

Topologies (all on generated, isolated fixtures on this machine):
  pty          observer PTY -> echo program (baseline)
  ssh          observer PTY -> ssh -tt (generated sshd on 127.0.0.1) -> echo
  screen/tmux/ruwac          observer PTY -> multiplexer client -> echo
  ssh-screen/ssh-tmux/ssh-ruwac   the same multiplexers behind the SSH hop
  gui          GUI-protocol client (client.input + client.render on the
               desktop worker's 8/20/100 ms schedule) -> RUWAC daemon -> echo
  ssh-gui      the GUI on a remote RUWAC: GUI-protocol client -> local RUWAC
               pane running ssh -> remote "ruwac attach" -> remote daemon -> echo
The GUI topologies emulate the desktop worker's RPC schedule; GTK paint and
compositor/photon latency are NOT included (reported as "frame received").

Stages for RUWAC topologies use pane.wait (woken by the daemon revision
channel), pre-issued before each keystroke on a dedicated connection:
  inner_daemon_ms  keystroke -> the echo is on the innermost daemon's screen
  outer_daemon_ms  (ssh-gui) -> the remote TUI's redraw is on the local pane
  ack_ms           (gui) client.input reply
The SSH hop has no network delay here; --rtt-ms is not emulated. Add real
network delay only with an explicit, owned remote (not part of this script).
"""
import argparse, codecs, json, math, os, platform, pty, pwd, random, re, select, shlex, shutil
import signal, socket, statistics, struct, subprocess, sys, tempfile, termios, fcntl, time, uuid
from pathlib import Path

HERE = Path(__file__).resolve()
ECHO = HERE.parents[1] / 'fixtures/typing_echo.py'
ALPHABETS = {'ascii': '~`|{}', 'unicode': 'αβγδε中文字'}
TOPOLOGIES = ['pty', 'ssh', 'screen', 'tmux', 'ruwac', 'ssh-screen', 'ssh-tmux', 'ssh-ruwac', 'gui', 'ssh-gui']
CSI = re.compile(r'\x1b(\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)|[()][0-9A-B]|[78=>DEHMNOZc])')
ROWS, COLS = 40, 120


def pct(values, q):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(q * len(ordered)) - 1)] if ordered else None


def summary(values):
    if not values:
        return {'n': 0}
    return {'n': len(values), 'p50_ms': round(statistics.median(values), 3), 'p95_ms': round(pct(values, .95), 3),
            'p99_ms': round(pct(values, .99), 3), 'max_ms': round(max(values), 3), 'mean_ms': round(statistics.fmean(values), 3)}


def cpu_seconds(pids):
    total = 0.0
    for pid in pids:
        try:
            fields = Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()
            total += (int(fields[11]) + int(fields[12])) / os.sysconf('SC_CLK_TCK')
        except (OSError, ValueError, IndexError):
            pass
    return total


def descendants(roots):
    children = {}
    for entry in Path('/proc').iterdir():
        if entry.name.isdigit():
            try:
                ppid = int(entry.joinpath('stat').read_text().rsplit(')', 1)[1].split()[1])
            except (OSError, ValueError, IndexError):
                continue
            children.setdefault(ppid, []).append(int(entry.name))
    seen, stack = set(), [p for p in roots if p]
    while stack:
        pid = stack.pop()
        if pid not in seen:
            seen.add(pid)
            stack.extend(children.get(pid, []))
    return seen


class Rpc:
    """Length-prefixed RUWAC control client; send() and poll for replies separately."""

    def __init__(self, path):
        self.s = socket.socket(socket.AF_UNIX)
        self.s.settimeout(30)
        self.s.connect(str(path))
        self.buffer = b''
        self.bytes = 0

    def send(self, method, params=None):
        body = json.dumps({'version': 0, 'id': uuid.uuid4().hex, 'method': method, 'params': params or {}},
                          ensure_ascii=False).encode()
        self.s.sendall(struct.pack('!I', len(body)) + body)

    def ready(self):
        while len(self.buffer) < 4 or len(self.buffer) < 4 + struct.unpack('!I', self.buffer[:4])[0]:
            if not select.select([self.s], [], [], 0)[0]:
                return None
            data = self.s.recv(1 << 20)
            if not data:
                raise EOFError('daemon closed the connection')
            self.buffer += data
        size = struct.unpack('!I', self.buffer[:4])[0]
        reply, self.buffer = self.buffer[4:4 + size], self.buffer[4 + size:]
        self.bytes += 4 + size
        value = json.loads(reply)
        if 'error' in value:
            raise RuntimeError(value['error'])
        return value['result']

    def call(self, method, params=None):
        self.send(method, params)
        while True:
            select.select([self.s], [], [], 30)
            value = self.ready()
            if value is not None:
                return value

    def close(self):
        self.s.close()


class Fixture:
    def __init__(self, args):
        self.args = args
        self.root = Path(tempfile.mkdtemp(prefix='typing-latency-', dir=args.evidence))
        self.root.chmod(0o700)
        # Unix sockets (Screen, tmux, RUWAC) need short paths; keep them private and short.
        self.sockets = Path(tempfile.mkdtemp(prefix='rtl-', dir=f'/run/user/{os.getuid()}'))
        self.binary = str(Path(args.binary).resolve())
        self.processes = []
        self.env = {k: v for k, v in os.environ.items() if k not in {'TMUX', 'STY', 'RUWAC_SOCKET', 'RUWAC_SESSION', 'RUWAC_PANE'}}
        self.env.update({'TERM': 'xterm-256color', 'LANG': 'C.UTF-8', 'LC_ALL': 'C.UTF-8', 'SCREENDIR': str(self.dir('screen')),
                         'HOME': str(self.dir('home'))})
        self.echo = [sys.executable, '-I', str(ECHO)] + (['--busy'] if args.output == 'busy' else [])
        self.noise = [sys.executable, '-I', str(ECHO), '--noise-only' if args.output == 'busy' else '--idle']

    def dir(self, name):
        path = (self.sockets if name in {'screen', 'remote-screen', 'tmux', 'ruwac'} else self.root) / name
        path.mkdir(mode=0o700, exist_ok=True)
        return path

    def start(self, argv, name, **kw):
        log = (self.root / (name + '.log')).open('wb')
        proc = subprocess.Popen(argv, env=kw.pop('env', self.env), stdout=log, stderr=subprocess.STDOUT, start_new_session=True, **kw)
        self.processes.append(proc)
        return proc

    def wait(self, check, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            try:
                if check():
                    return
            except (OSError, EOFError, RuntimeError):
                pass
            time.sleep(.02)
        raise TimeoutError('fixture did not become ready')

    # Generated OpenSSH server on loopback; owner keys, config and hosts are never used.
    def sshd(self):
        auth = Path(tempfile.mkdtemp(prefix='ruwac-typing-auth-', dir=f'/run/user/{os.getuid()}'))
        self.auth = auth
        for name in ['client', 'host']:
            subprocess.run(['ssh-keygen', '-q', '-t', 'ed25519', '-N', '', '-f', str(self.root / name)], check=True)
        (auth / 'authorized_keys').write_bytes((self.root / 'client.pub').read_bytes())
        (auth / 'authorized_keys').chmod(0o600)
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            port = s.getsockname()[1]
        config = self.root / 'sshd.conf'
        config.write_text(f'ListenAddress 127.0.0.1\nPort {port}\nHostKey {self.root}/host\nPidFile {self.root}/sshd.pid\n'
                          f'AuthorizedKeysFile {auth}/authorized_keys\nSetEnv HOME={self.dir("remote-home")} SCREENDIR={self.dir("remote-screen")} LANG=C.UTF-8 LC_ALL=C.UTF-8\n'
                          'StrictModes yes\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\n'
                          f'AllowTcpForwarding no\nAllowUsers {pwd.getpwuid(os.getuid()).pw_name}\nLogLevel ERROR\n')
        self.sshd_proc = self.start(['/usr/sbin/sshd', '-D', '-e', '-f', str(config)], 'sshd')
        known = self.root / 'known_hosts'
        key = (self.root / 'host.pub').read_text().split()
        known.write_text(f'[127.0.0.1]:{port} {key[0]} {key[1]}\n')
        self.ssh_config = self.root / 'ssh_config'
        self.ssh_config.write_text(f'Host bench\n HostName 127.0.0.1\n Port {port}\n IdentityFile {self.root}/client\n IdentitiesOnly yes\n'
                                   f' UserKnownHostsFile {known}\n StrictHostKeyChecking yes\n BatchMode yes\n ControlMaster no\n'
                                   ' ForwardAgent no\n LogLevel ERROR\n')
        self.ssh_config.chmod(0o600)

        def up():
            with socket.create_connection(('127.0.0.1', port), timeout=.2):
                return True
        self.wait(up)

    def ssh(self, command):
        return ['ssh', '-F', str(self.ssh_config), '-tt', 'bench', shlex.join(command)]

    def ruwac_daemon(self, name):
        sock = self.dir('ruwac') / (name + '.sock')
        self.start([self.binary, '--socket', str(sock), 'serve'], 'ruwac-' + name)
        self.wait(lambda: sock.exists() and Rpc(sock).call('health')['ok'])
        return sock

    def ruwac_session(self, sock, name, argv):
        rpc = Rpc(sock)
        row = rpc.call('session.create', {'name': name, 'rows': ROWS, 'cols': COLS, 'argv': argv})
        for index in range(self.args.panes - 1):
            rpc.call('window.create', {'session': row['session'], 'argv': self.noise})
        rpc.call('window.select', {'window': row['window']})
        rpc.close()
        return row

    def tmux(self, name):
        sock = self.dir('tmux') / name
        base = ['tmux', '-f', '/dev/null', '-S', str(sock)]
        subprocess.run(base + ['new-session', '-d', '-s', 'bench', '-x', str(COLS), '-y', str(ROWS)] + self.echo, env=self.env, check=True)
        for _ in range(self.args.panes - 1):
            subprocess.run(base + ['new-window', '-d'] + self.noise, env=self.env, check=True)
        return base + ['attach-session', '-t', 'bench'], base + ['kill-server']

    def screen(self, name, env_dir):
        session = 'bench-' + name
        env = {**self.env, 'SCREENDIR': str(env_dir)}
        # A detached Screen ignores window selection, and each new window becomes
        # current: create the background windows first and the measured echo last.
        first = self.noise if self.args.panes > 1 else self.echo
        subprocess.run(['screen', '-c', '/dev/null', '-dmS', session] + first, env=env, check=True)
        self.wait(lambda: subprocess.run(['screen', '-S', session, '-Q', 'select', '.'], env=env, capture_output=True).returncode == 0)
        for index in range(self.args.panes - 1):
            subprocess.run(['screen', '-S', session, '-X', 'screen'] + (self.echo if index == self.args.panes - 2 else self.noise), env=env, check=True)
        return ['screen', '-c', '/dev/null', '-r', session], ['screen', '-S', session, '-X', 'quit'], env

    def close(self):
        for proc in reversed(self.processes):
            if proc.poll() is None:
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                except OSError:
                    pass
        for proc in reversed(self.processes):
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(proc.pid, signal.SIGKILL)
                proc.wait()
        for path in [getattr(self, 'auth', None), self.sockets]:
            if path:
                shutil.rmtree(path, ignore_errors=True)


class PtyTarget:
    """A keystroke goes into an observer-owned PTY; latency ends when its character is drawn."""

    def __init__(self, fx, name, argv, env=None, stages=None, quit=None):
        self.name, self.fx, self.stages, self.quit = name, fx, stages or {}, quit
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', ROWS, COLS, 0, 0))
        log = (fx.root / (name + '.log')).open('wb')
        self.proc = subprocess.Popen(argv, stdin=slave, stdout=slave, stderr=log, env=env or fx.env, start_new_session=True)
        fx.processes.append(self.proc)
        os.close(slave)
        self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
        self.text, self.bytes = '', 0
        self.expect('READY', 20)

    def drain(self, timeout):
        if select.select([self.master], [], [], timeout)[0]:
            data = os.read(self.master, 1 << 16)
            self.bytes += len(data)
            self.text = (self.text + self.decoder.decode(data))[-8192:]
            return True
        return False

    def expect(self, needle, timeout):
        end = time.monotonic() + timeout
        while needle not in CSI.sub('', self.text):
            if time.monotonic() > end:
                raise TimeoutError(f'{self.name}: {needle!r} not shown; last output {CSI.sub("", self.text)[-300:]!r}')
            self.drain(.05)

    def keystroke(self, char, timeout):
        while self.drain(0):
            pass
        self.text = ''
        probes = {name: (Rpc(sock), pane) for name, (sock, pane) in self.stages.items()}
        for rpc, pane in probes.values():
            rpc.send('pane.wait', {'pane': pane, 'contains': char, 'timeout_ms': int(timeout * 1000)})
        t0 = time.monotonic()
        os.write(self.master, char.encode())
        marks, end, shown = {}, t0 + timeout, None
        while time.monotonic() < end and (shown is None or len(marks) < len(probes)):
            sockets = [rpc.s for name, (rpc, _) in probes.items() if name not in marks]
            readable = select.select([self.master] + sockets, [], [], max(0, end - time.monotonic()))[0]
            now = time.monotonic()
            if self.master in readable and shown is None:
                self.drain(0)
                if char in CSI.sub('', self.text):
                    shown = now
            for name, (rpc, _) in probes.items():
                if name not in marks and rpc.s in readable:
                    try:
                        if rpc.ready() is not None:
                            marks[name] = (now - t0) * 1000
                    except RuntimeError:  # pane.wait timed out
                        marks[name] = None
        for name, (rpc, _) in probes.items():
            while name not in marks:  # never leave a pane.wait reply for the next keystroke
                select.select([rpc.s], [], [], 1)
                try:
                    if rpc.ready() is not None:
                        marks[name] = None
                except RuntimeError:
                    marks[name] = None
        for rpc, _ in probes.values():
            rpc.close()
        return (None if shown is None else (shown - t0) * 1000), marks

    def pids(self):
        return [self.proc.pid]

    def close(self):
        if self.quit:
            subprocess.run(self.quit[0], env=self.quit[1], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            os.killpg(self.proc.pid, signal.SIGHUP)
        except OSError:
            pass
        os.close(self.master)


class GuiTarget:
    """The desktop worker's RPC sequence: client.input, then client.render until the frame shows it.

    Schedule copied from desktop/src/worker.rs: render immediately after input,
    then every 8 ms for 150 ms after typing, 20 ms within 1 s of a change, else 100 ms.
    """

    def __init__(self, fx, name, sock, session, pane, stages):
        self.name, self.pane, self.stages, self.sock, self.session = name, pane, stages, sock, session
        self.rpc, self.reconnects = None, 0
        self.connect()
        self.revision, self.typed, self.active, self.bytes = None, 0.0, 0.0, 0
        end = time.monotonic() + 20
        while 'READY' not in self.frame_text() and time.monotonic() < end:
            time.sleep(.02)

    def connect(self):
        if self.rpc:
            self.rpc.close()
        self.rpc = Rpc(self.sock)
        self.rpc.call('session.attach', {'session': self.session, 'rows': ROWS, 'cols': COLS})
        self.revision = None

    def frame_text(self):
        before = self.rpc.bytes
        frame = self.rpc.call('client.render', {'revision': self.revision} if self.revision is not None else {})
        self.bytes += self.rpc.bytes - before
        if frame.get('unchanged'):
            return self.last if hasattr(self, 'last') else ''
        self.revision = frame['revision']
        self.last = json.dumps(frame.get('panes'), ensure_ascii=False)
        self.active = time.monotonic()
        return self.last

    def idle(self, seconds):
        # Interleaving leaves this connection unused while other topologies type,
        # and the daemon closes long-idle connections: re-attach before measuring.
        try:
            self.frame_text()
        except (OSError, EOFError):
            self.reconnects += 1
            self.connect()
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            self.frame_text()
            now = time.monotonic()
            wait = .008 if now - self.typed < .15 else .02 if now - self.active < 1 else .1
            time.sleep(max(0, min(wait, end - time.monotonic())))

    def keystroke(self, char, timeout):
        probes = {name: (Rpc(sock), pane) for name, (sock, pane) in self.stages.items()}
        for rpc, pane in probes.values():
            rpc.send('pane.wait', {'pane': pane, 'contains': char, 'timeout_ms': int(timeout * 1000)})
        t0 = time.monotonic()
        self.rpc.call('client.input', {'pane': self.pane, 'kind': 'keyboard', 'text': char})
        marks = {'ack_ms': (time.monotonic() - t0) * 1000}
        self.typed = time.monotonic()
        shown = None
        while time.monotonic() < t0 + timeout:
            text = self.frame_text()
            now = time.monotonic()
            for name, (rpc, _) in probes.items():
                try:
                    if name not in marks and rpc.ready() is not None:
                        marks[name] = (now - t0) * 1000
                except RuntimeError:
                    marks[name] = None
            if char in text:
                shown = now
                break
            # Wait out the worker's 8 ms poll, but timestamp daemon stages the moment they reply.
            until = time.monotonic() + .008
            while True:
                waiting = [rpc.s for name, (rpc, _) in probes.items() if name not in marks]
                left = until - time.monotonic()
                if left <= 0:
                    break
                if not waiting:
                    time.sleep(left)
                    break
                for sock in select.select(waiting, [], [], left)[0]:
                    for name, (rpc, _) in probes.items():
                        if rpc.s is sock and name not in marks:
                            try:
                                if rpc.ready() is not None:
                                    marks[name] = (time.monotonic() - t0) * 1000
                            except RuntimeError:
                                marks[name] = None
        for name, (rpc, _) in probes.items():
            while name not in marks:
                select.select([rpc.s], [], [], 1)
                try:
                    if rpc.ready() is not None:
                        marks[name] = (time.monotonic() - t0) * 1000
                except RuntimeError:
                    marks[name] = None
        for rpc, _ in probes.values():
            rpc.close()
        return (None if shown is None else (shown - t0) * 1000), marks

    def pids(self):
        return []

    def close(self):
        try:
            self.rpc.call('session.detach')
        except (OSError, EOFError, RuntimeError):
            pass
        self.rpc.close()


def render_probe(sock, session, pane, rounds=30):
    """Daemon cost of the frontends' full frame vs an unchanged poll, and its effect on echo.

    Measures only; nothing is changed. echo_input_ms: client.input -> echo on the
    daemon screen; echo_input_render_ms: the same with client.render sent right
    after the input, as the GUI worker and TUI do.
    """
    client, probe_rpc = Rpc(sock), Rpc(sock)
    client.call('session.attach', {'session': session, 'rows': ROWS, 'cols': COLS})
    out = {}
    for mode in ['echo_input_ms', 'echo_input_render_ms']:
        values = []
        for index in range(rounds):
            char = ALPHABETS['ascii'][index % len(ALPHABETS['ascii'])]
            probe_rpc.send('pane.wait', {'pane': pane, 'contains': char, 'timeout_ms': 2000})
            t0 = time.monotonic()
            client.call('client.input', {'pane': pane, 'kind': 'keyboard', 'text': char})
            if mode == 'echo_input_render_ms':
                client.call('client.render', {})
            while True:
                select.select([probe_rpc.s], [], [], 2)
                if probe_rpc.ready() is not None:
                    break
            values.append((time.monotonic() - t0) * 1000)
            time.sleep(.03)
        out[mode] = summary(values)
    full, sizes = [], []
    for _ in range(rounds):
        before, t0 = client.bytes, time.monotonic()
        frame = client.call('client.render', {})
        full.append((time.monotonic() - t0) * 1000)
        sizes.append(client.bytes - before)
    unchanged = []
    for _ in range(rounds):
        t0 = time.monotonic()
        client.call('client.render', {'revision': frame['revision']})
        unchanged.append((time.monotonic() - t0) * 1000)
    out.update({'full_render_ms': summary(full), 'full_render_bytes': statistics.median(sizes),
                'unchanged_render_ms': summary(unchanged), 'screen': f'{ROWS}x{COLS}'})
    client.call('session.detach')
    client.close()
    probe_rpc.close()
    return out


def build(fx, names):
    targets, roots = {}, []
    needs_ssh = any(n.startswith('ssh') for n in names)
    if needs_ssh:
        fx.sshd()
        roots.append(fx.sshd_proc.pid)
    local = remote = None
    if any(n in {'ruwac', 'gui', 'ssh-gui'} for n in names):
        local = fx.ruwac_daemon('local')
    if any(n in {'ssh-ruwac', 'ssh-gui'} for n in names):
        remote = fx.ruwac_daemon('remote')
    roots += [p.pid for p in fx.processes]
    for name in names:
        if name == 'pty':
            targets[name] = PtyTarget(fx, name, fx.echo)
        elif name == 'ssh':
            targets[name] = PtyTarget(fx, name, fx.ssh(fx.echo))
        elif name in {'tmux', 'ssh-tmux'}:
            attach, kill = fx.tmux(name)
            targets[name] = PtyTarget(fx, name, fx.ssh(attach) if name == 'ssh-tmux' else attach, quit=(kill, fx.env))
            roots.append(int(subprocess.check_output(attach[:5] + ['display-message', '-p', '#{pid}'], env=fx.env)))
        elif name in {'screen', 'ssh-screen'}:
            remote_side = name == 'ssh-screen'
            attach, quit, env = fx.screen(name, fx.dir('remote-screen') if remote_side else fx.dir('screen'))
            if remote_side:
                attach = ['env', 'SCREENDIR=' + str(fx.dir('remote-screen'))] + attach
            targets[name] = PtyTarget(fx, name, fx.ssh(attach) if remote_side else attach, env=env, quit=(quit, env))
        elif name in {'ruwac', 'ssh-ruwac'}:
            sock = local if name == 'ruwac' else remote
            row = fx.ruwac_session(sock, name, fx.echo)
            attach = [fx.binary, '--socket', str(sock), 'attach', '--session', row['session']]
            targets[name] = PtyTarget(fx, name, fx.ssh(attach) if name == 'ssh-ruwac' else attach,
                                      stages={'inner_daemon_ms': (sock, row['pane'])})
        elif name == 'gui':
            row = fx.ruwac_session(local, name, fx.echo)
            targets[name] = GuiTarget(fx, name, local, row['session'], row['pane'], {'inner_daemon_ms': (local, row['pane'])})
        elif name == 'ssh-gui':
            inner = fx.ruwac_session(remote, name + '-remote', fx.echo)
            outer = fx.ruwac_session(local, name, fx.ssh([fx.binary, '--socket', str(remote), 'attach', '--session', inner['session']]))
            targets[name] = GuiTarget(fx, name, local, outer['session'], outer['pane'],
                                      {'inner_daemon_ms': (remote, inner['pane']), 'outer_daemon_ms': (local, outer['pane'])})
    return targets, roots


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--binary', required=True, help='ruwac CLI/daemon binary under test')
    p.add_argument('--evidence', required=True, help='directory for the private fixture tree and result JSON')
    p.add_argument('--topologies', default=','.join(TOPOLOGIES))
    p.add_argument('--keys', type=int, default=200, help='measured keystrokes per topology')
    p.add_argument('--block', type=int, default=20, help='keystrokes per topology before rotating (interleaving)')
    p.add_argument('--pacing', choices=['active', 'quiet'], default='active', help='active: 40-120 ms between keys; quiet: 1.2-2 s')
    p.add_argument('--panes', type=int, choices=[1, 5, 20], default=1, help='windows per multiplexer session (extra ones hidden)')
    p.add_argument('--output', choices=['quiet', 'busy'], default='quiet', help='busy: 50 Hz noise in every pane')
    p.add_argument('--charset', choices=sorted(ALPHABETS), default='ascii')
    p.add_argument('--timeout', type=float, default=2.0)
    p.add_argument('--seed', type=int, default=1)
    a = p.parse_args()
    names = [n for n in a.topologies.split(',') if n]
    unknown = set(names) - set(TOPOLOGIES)
    if unknown:
        p.error('unknown topology: ' + ', '.join(sorted(unknown)))
    for tool, needed in [('screen', 'screen' in a.topologies), ('tmux', 'tmux' in a.topologies), ('/usr/sbin/sshd', 'ssh' in a.topologies)]:
        if needed and not shutil.which(tool):
            p.error(tool + ' is required for the selected topologies')
    rng = random.Random(a.seed)
    fx = Fixture(a)
    targets = {}
    try:
        targets, roots = build(fx, names)
        samples = {n: [] for n in names}
        stages = {n: {} for n in names}
        timeouts = {n: 0 for n in names}
        first = {}
        alphabet = ALPHABETS[a.charset]
        last = {n: '' for n in names}
        pids = descendants(roots + [pid for t in targets.values() for pid in t.pids()])
        cpu_before = cpu_seconds(pids)
        bytes_before = {n: t.bytes for n, t in targets.items()}
        started = time.monotonic()
        for name, target in targets.items():  # one unmeasured cold keystroke per topology
            char = rng.choice(alphabet)
            first[name], _ = target.keystroke(char, a.timeout)
            last[name] = char
        while any(len(samples[n]) + timeouts[n] < a.keys for n in names):
            for name in names:
                target = targets[name]
                for _ in range(min(a.block, a.keys - len(samples[name]) - timeouts[name])):
                    gap = rng.uniform(.04, .12) if a.pacing == 'active' else rng.uniform(1.2, 2.0)
                    (target.idle if isinstance(target, GuiTarget) else time.sleep)(gap)
                    char = rng.choice([c for c in alphabet if c != last[name]])
                    last[name] = char
                    latency, marks = target.keystroke(char, a.timeout)
                    if latency is None:
                        timeouts[name] += 1
                        time.sleep(.3)
                        continue
                    samples[name].append(latency)
                    for stage, value in marks.items():
                        if value is not None:
                            stages[name].setdefault(stage, []).append(value)
        elapsed = time.monotonic() - started
        pids |= descendants(roots + [pid for t in targets.values() for pid in t.pids()])
        cpu = cpu_seconds(pids) - cpu_before
        results = {}
        for name in names:
            n = len(samples[name]) + timeouts[name]
            results[name] = {'latency': summary(samples[name]), 'timeouts': timeouts[name], 'cold_first_keystroke_ms': first[name],
                             'stages': {k: summary(v) for k, v in stages[name].items()},
                             'bytes_per_keystroke': round((targets[name].bytes - bytes_before[name]) / max(1, n), 1),
                             'bytes_meaning': 'client.render JSON received' if name in {'gui', 'ssh-gui'} else 'terminal output received by the observer PTY'}
        med = {n: results[n]['latency'].get('p50_ms') for n in names}

        def diff(a_, b_):
            return round(med[a_] - med[b_], 3) if med.get(a_) is not None and med.get(b_) is not None else None
        contributions = {
            'ssh_transport_ms': diff('ssh', 'pty'),
            'screen_ms': diff('screen', 'pty'), 'tmux_ms': diff('tmux', 'pty'), 'ruwac_tui_ms': diff('ruwac', 'pty'),
            'screen_behind_ssh_ms': diff('ssh-screen', 'ssh'), 'tmux_behind_ssh_ms': diff('ssh-tmux', 'ssh'),
            'ruwac_behind_ssh_ms': diff('ssh-ruwac', 'ssh'), 'gui_protocol_ms': diff('gui', 'pty'),
            'gui_on_remote_ruwac_over_ssh_ruwac_ms': diff('ssh-gui', 'ssh-ruwac')}
        for name in ['ruwac', 'ssh-ruwac', 'gui', 'ssh-gui']:
            inner = results.get(name, {}).get('stages', {}).get('inner_daemon_ms', {}).get('p50_ms')
            if inner is not None and med.get(name) is not None:
                contributions[name + ':after_inner_daemon_ms'] = round(med[name] - inner, 3)
        report = {
            'schema': 1, 'kind': 'ruwac-typing-latency',
            'scenario': {k: getattr(a, k) for k in ['keys', 'block', 'pacing', 'panes', 'output', 'charset', 'timeout', 'seed']},
            'environment': {'cpus': os.cpu_count(), 'loadavg_after': Path('/proc/loadavg').read_text().split()[:3], 'kernel': platform.release(),
                            'python': platform.python_version(),
                            'ruwac': subprocess.run([fx.binary, '--version'], capture_output=True, text=True).stdout.strip(),
                            'tmux': subprocess.run(['tmux', '-V'], capture_output=True, text=True).stdout.strip() if shutil.which('tmux') else None,
                            'screen': subprocess.run(['screen', '--version'], capture_output=True, text=True).stdout.strip() if shutil.which('screen') else None,
                            'openssh': subprocess.run(['ssh', '-V'], capture_output=True, text=True).stderr.strip()},
            'results': results,
            'stage_contributions_from_medians': contributions,
            'fixture_cpu_seconds': round(cpu, 3), 'fixture_cpu_ms_per_keystroke': round(cpu * 1000 / max(1, sum(len(v) for v in samples.values())), 3),
            'elapsed_s': round(elapsed, 1),
            'limits': ['Single machine; the SSH hop is loopback with no network delay.',
                       'gui/ssh-gui emulate the desktop worker RPC schedule; GTK paint, compositor and display latency are not included.',
                       'CPU is the whole fixture process tree for all topologies together (they are interleaved), not per topology.',
                       'Stage contributions are differences of medians between topologies, not per-keystroke decomposition.'],
        }
        if 'ruwac' in names or 'gui' in names:
            sock = fx.dir('ruwac') / 'local.sock'
            row = fx.ruwac_session(sock, 'render-probe', fx.echo)
            Rpc(sock).call('pane.wait', {'pane': row['pane'], 'contains': 'READY', 'timeout_ms': 10000})
            report['ruwac_render_probe'] = render_probe(sock, row['session'], row['pane'])
        out = fx.root / 'typing-latency.json'
        out.write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n')
        print(f"{'topology':<11}{'p50':>9}{'p95':>9}{'p99':>9}{'max':>9}{'cold':>9}{'t/o':>5}{'B/key':>9}  stages p50")
        for name in names:
            r = results[name]
            lat = r['latency']
            stage = ' '.join(f"{k.replace('_ms', '')}={v.get('p50_ms')}" for k, v in r['stages'].items())
            fmt = lambda v: f'{v:9.2f}' if isinstance(v, (int, float)) else f'{"-":>9}'
            print(f"{name:<11}{fmt(lat.get('p50_ms'))}{fmt(lat.get('p95_ms'))}{fmt(lat.get('p99_ms'))}{fmt(lat.get('max_ms'))}{fmt(r['cold_first_keystroke_ms'])}{r['timeouts']:>5}{r['bytes_per_keystroke']:>9}  {stage}")
        print('contributions (median differences, ms):', json.dumps({k: v for k, v in contributions.items() if v is not None}))
        if 'ruwac_render_probe' in report:
            probe = report['ruwac_render_probe']
            print('ruwac render probe (p50 ms): echo after input', probe['echo_input_ms']['p50_ms'],
                  '| with immediate client.render', probe['echo_input_render_ms']['p50_ms'],
                  '| full frame', probe['full_render_ms']['p50_ms'], f"({probe['full_render_bytes']:.0f} B)",
                  '| unchanged poll', probe['unchanged_render_ms']['p50_ms'])
        print('result', out)
    finally:
        for target in targets.values():
            try:
                target.close()
            except OSError:
                pass
        fx.close()


if __name__ == '__main__':
    main()
