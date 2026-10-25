"""Native publication with optional, independent legacy PRIMARY observation.

It never discovers a desktop or creates SSH.
"""
import argparse
import json
import os
import re
import select
import stat
import subprocess
import time
import threading
from pathlib import Path
from types import SimpleNamespace

from gi.repository import Gio, GLib
from framed_publisher import Adapter
from compositor_identity import identify, Refused

OBSERVE_LIMIT = 1024 * 1024
OUTPUT_FRAME = OBSERVE_LIMIT * 6 + 1024
ORIGIN_UNSUPPORTED = object()


def desktop_environment(wayland):
    if not re.fullmatch(r'wayland-[0-9]{1,3}', wayland):
        raise ValueError('An exact desktop is required')
    process = subprocess.Popen(['systemctl', '--user', 'show-environment'],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    output = bytearray()
    deadline = time.monotonic()+5
    try:
        os.set_blocking(process.stdout.fileno(), False)
        while True:
            remaining = deadline-time.monotonic()
            if remaining <= 0 or not select.select([process.stdout], [], [], remaining)[0]:
                raise ValueError('Desktop environment deadline')
            chunk = os.read(process.stdout.fileno(), min(8192, 65537-len(output)))
            if not chunk:
                break
            output.extend(chunk)
            if len(output) > 65536:
                raise ValueError('Desktop environment bound exceeded')
        if process.wait(timeout=max(.01, deadline-time.monotonic())) != 0:
            raise ValueError('Desktop environment unavailable')
    finally:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=1)
        process.stdout.close()
    values = dict(line.split('=', 1) for line in output.decode('utf-8', errors='strict').splitlines() if '=' in line)
    if values.get('WAYLAND_DISPLAY') != wayland:
        raise ValueError('Configured desktop differs from account desktop')
    runtime = values.get('XDG_RUNTIME_DIR', '/run/user/'+str(os.getuid()))
    info = Path(runtime).lstat()
    if not Path(runtime).is_absolute() or not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise ValueError('Desktop runtime must be owned and private')
    # This is the selected account's canonical session bus, not a discovered bus
    # or process environment. Native identity verification binds its exact Shell
    # owner to the configured compositor listener before any enrollment.
    return dict(runtime=runtime, display=wayland, bus='unix:path='+runtime+'/bus',
                xdisplay=values.get('DISPLAY', ''), authority=values.get('XAUTHORITY', ''))


class OriginProbe:
    """Read-only origin metadata with one independently bounded identity worker.

    Shares the native client's owned bus; never changes its publication job,
    lease or authorization. No clipboard contents pass through this query.
    """
    def __init__(self, client):
        self.client = client
        self.job = None
        self.worker = False
        self.closed = False

    def cancel(self):
        job, self.job = self.job, None
        if job:
            job['cancel'].cancel()
            job['callback'](None)

    def close(self):
        self.closed = True
        self.cancel()

    def request(self, callback, *, allow_unsupported=False):
        if self.closed or self.client.closed or self.job is not None:
            GLib.idle_add(lambda: (callback(None), False)[1])
            return
        self.job = dict(callback=callback, cancel=Gio.Cancellable(),
                        generation=self.client.generation, allow_unsupported=allow_unsupported)
        if not self.worker:
            self._start(self.job)

    def _start(self, job):
        self.worker = True
        expected = dict(self.client.identity)

        def work():
            try:
                pin = identify(self.client.connection, self.client.runtime,
                               self.client.display, expected=expected)
            except (Refused, OSError, ValueError, GLib.Error):
                pin = None
            GLib.idle_add(lambda: self._identified(job, pin))
        try:
            threading.Thread(target=work, daemon=True).start()
        except RuntimeError:
            self.worker = False
            self._finish(job, None)

    def _finish(self, job, value):
        if self.job is job:
            self.job = None
            job['callback'](value)

    def _identified(self, job, pin):
        self.worker = False
        if self.job is not job or self.closed:
            if self.job and not self.closed:
                self._start(self.job)
            return False
        if pin is None or self.client.closed or job['generation'] != self.client.generation:
            self._finish(job, None)
            return False
        self.client.connection.call(pin['owner'], '/org/ruwac/PrimaryPublisher1',
            'org.ruwac.PrimaryPublisher1', 'Origin', None, None,
            Gio.DBusCallFlags.NONE, 1500, job['cancel'],
            lambda connection, result: self._reply(job, connection, result))
        return False

    def _reply(self, job, connection, result):
        value = None
        try:
            raw = connection.call_finish(result).unpack()[0]
            if not isinstance(raw, str) or len(raw) > 1024:
                raise ValueError('Origin metadata bound')
            candidate = json.loads(raw)
            if (type(candidate) is not dict or set(candidate) != {'ok', 'protocol', 'revision', 'published'} or
                candidate['ok'] is not True or type(candidate['protocol']) is not int or candidate['protocol'] != 1 or
                type(candidate['revision']) is not int or not 0 <= candidate['revision'] < (1 << 53) or
                type(candidate['published']) is not bool):
                raise ValueError('Origin metadata')
            value = candidate
        except GLib.Error as error:
            # Only an absent optional method permits publication-only startup.
            # Policy refusal, endpoint transitions and timeouts require retry;
            # they must not silently disable observation for this helper's life.
            if (job['allow_unsupported'] and Gio.DBusError.get_remote_error(error) ==
                    'org.freedesktop.DBus.Error.UnknownMethod'):
                value = ORIGIN_UNSUPPORTED
        except (ValueError, TypeError, IndexError):
            pass
        if self.client.closed or self.closed or job['generation'] != self.client.generation:
            value = None
        self._finish(job, value)


class Observer:
    def __init__(self, emit, display, authority, client, failed, origin=None):
        if not re.fullmatch(r':[0-9]{1,5}(?:\.[0-9]+)?', display):
            raise ValueError('No configured Xwayland observer')
        auth = Path(authority).lstat()
        if not stat.S_ISREG(auth.st_mode) or auth.st_uid != os.getuid() or auth.st_mode & 0o077:
            raise ValueError('Observer authority must be owned and private')
        os.environ.update(DISPLAY=display, XAUTHORITY=authority, GDK_BACKEND='x11')
        import gi
        gi.require_version('Gdk', '4.0')
        gi.require_version('Gtk', '4.0')
        from gi.repository import Gdk, Gtk
        import legacy_primary
        legacy_primary.Gdk, legacy_primary.Gtk = Gdk, Gtk
        legacy_primary.Gio, legacy_primary.GLib = Gio, GLib
        self.legacy = legacy_primary
        self.origin = origin if origin is not None else OriginProbe(client)
        self.emit, self.failed = emit, failed
        self.epoch = 0
        self.enabled = False
        self.timer = None
        self.dirty = False
        self.changed_epoch = None
        self.serial = 0
        self.busy = False
        self.closed = False
        self.primary = legacy_primary.Primary(self.changed)
        self.mode_revision = self.primary.changes.revision()

    def changed(self):
        if self.closed:
            return
        self.serial += 1
        self.busy = False
        self.origin.cancel()
        # Preserve released first-selection/XFixes epoch classification.
        self.legacy.Peer.changed(self)

    def mode(self, epoch, observe):
        if epoch != self.epoch or observe != self.enabled:
            self.serial += 1
            self.busy = False
            self.origin.cancel()
            self.primary.cancel()
            self.primary.last_digest = None
            self.changed_epoch = None
            self.mode_revision = self.primary.changes.revision()
        self.epoch, self.enabled = epoch, observe

    def publish(self):
        if self.closed or not self.enabled:
            return
        if self.busy:
            self.dirty = True
            return
        self.busy = True
        self.dirty = False
        serial, epoch, generation = self.serial, self.epoch, self.primary.generation
        revision = self.primary.changes.revision()
        initial = self.changed_epoch != epoch

        def valid():
            return (not self.closed and self.enabled and self.serial == serial and self.epoch == epoch and
                    self.primary.generation == generation and self.primary.changes.revision() == revision)

        def finish():
            if self.serial != serial or self.closed:
                return
            self.busy = False
            if self.dirty and self.enabled:
                GLib.idle_add(lambda: (self.publish(), False)[1])

        def before(state):
            if not valid():
                finish()
                return
            if state is None:
                self.failed()
                return
            if state['published']:
                finish()
                return

            def ready(text):
                if not valid():
                    finish()
                    return

                def after(current):
                    if valid():
                        if current is None:
                            self.failed()
                            return
                        if not current['published'] and current['revision'] == state['revision']:
                            self.emit(dict(type='text', epoch=epoch, initial=initial, text=text))
                    finish()
                self.origin.request(after)
            # Successful delivery remains busy through the second origin
            # query. Empty, duplicate, refused or cancelled reads also need a
            # terminal notification so a coalesced request can run afterwards.
            self.primary.read(ready, finished=lambda delivered: None if delivered else finish())
        self.origin.request(before)

    def close(self):
        if self.closed:
            return
        self.closed = True
        self.serial += 1
        self.enabled = False
        self.origin.close()
        self.primary.clip.disconnect(self.primary.changed_handler)
        self.primary.cancel()
        self.primary.changes.close()
        if self.timer is not None:
            GLib.source_remove(self.timer)
            self.timer = None


class NativePeer(Adapter):
    def __init__(self, args, *, xdisplay='', authority=''):
        self.observer = None
        self.origin_probe = None
        self.observer_config = xdisplay, authority
        self.loop_guard = None
        if xdisplay and authority:
            import legacy_primary
            self.loop_guard = legacy_primary.LoopGuard(GLib)
        try:
            super().__init__(args)
        except BaseException:
            if self.loop_guard is not None:
                self.loop_guard.close()
            raise

    def initialized(self, ok):
        if ok and all(self.observer_config):
            self.origin_probe = OriginProbe(self.client)

            def checked(state):
                if self.stopped:
                    return
                if state is None:
                    self.stop(2)
                    return
                if state is not ORIGIN_UNSUPPORTED:
                    try:
                        self.observer = Observer(self.emit, *self.observer_config,
                            self.client, lambda: self.stop(2), origin=self.origin_probe)
                    except (ValueError, OSError, RuntimeError, GLib.Error, ImportError):
                        self.observer = None
                if self.observer is None:
                    self.origin_probe.close()
                    self.origin_probe = None
                super(NativePeer, self).initialized(ok)
            self.origin_probe.request(checked, allow_unsupported=True)
            return
        super().initialized(ok)

    def emit(self, value):
        if self.stopped:
            return
        if value.get('type') == 'hello':
            value = dict(value, observes_selection=self.observer is not None)
        if value.get('type') == 'text':
            text = value.get('text')
            if not isinstance(text, str) or not text or '\0' in text or len(text.encode('utf-8')) > OBSERVE_LIMIT:
                self.stop(2)
                return
        raw = json.dumps(value, ensure_ascii=True, separators=(',', ':')).encode()+b'\n'
        if len(raw) > OUTPUT_FRAME or len(self.output)+len(raw) > OUTPUT_FRAME*2:
            self.stop(2)
            return
        self.output.extend(raw)
        if self.writer is None:
            self.writer = GLib.io_add_watch(1, GLib.IO_OUT | GLib.IO_ERR | GLib.IO_HUP, self.write)

    def message(self, value):
        observe = False
        if type(value) is dict and value.get('type') == 'mode' and 'observe' in value:
            if set(value) != {'type', 'epoch', 'active', 'observe'} or type(value['observe']) is not bool:
                raise ValueError('Invalid observation context')
            observe = value['observe']
            value = {key: item for key, item in value.items() if key != 'observe'}
        super().message(value)
        if type(value) is dict and value.get('type') == 'mode' and self.observer is not None:
            self.observer.mode(value['epoch'], observe)

    def stop(self, code=0):
        if self.observer is not None:
            self.observer.close()
            self.observer = None
        if self.origin_probe is not None:
            self.origin_probe.close()
            self.origin_probe = None
        super().stop(code)
        if self.loop_guard is not None:
            self.loop_guard.close()


def main():
    parser = argparse.ArgumentParser()
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument('--desktop')
    selection.add_argument('--bus')
    parser.add_argument('--runtime')
    parser.add_argument('--display')
    parser.add_argument('--label', required=True)
    args = parser.parse_args()
    try:
        if args.desktop:
            if args.runtime or args.display:
                return 2
            selected = desktop_environment(args.desktop)
        else:
            if not args.runtime or not args.display or not args.bus.startswith('unix:') or ';' in args.bus or len(args.bus) > 4096:
                return 2
            selected = dict(bus=args.bus, runtime=args.runtime, display=args.display,
                            xdisplay='', authority='')
        peer = NativePeer(SimpleNamespace(bus=selected['bus'], runtime=selected['runtime'],
                          display=selected['display'], label=args.label),
                          xdisplay=selected['xdisplay'], authority=selected['authority'])
        try:
            peer.loop.run()
        finally:
            peer.stop()
        return peer.exit_code
    except (OSError, ValueError, subprocess.SubprocessError):
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
