"""Bounded stdin/stdout adapter; no SSH creation or desktop discovery.

Protocol 3 is intentionally incompatible with older publication-only protocols.
An explicit enrollment request must follow a current parent context. Publication
requires prepare and commit, then a separate ownership confirmation before input.
"""
import argparse
import json
import os
import sys
import time

from gi.repository import Gio, GLib
from native_publisher import NativePublisher, integer, MAX_TEXT

FRAME = MAX_TEXT * 6 + 8192


def object_pairs(pairs):
    value = {}
    for key, item in pairs:
        if key in value:
            raise ValueError('Duplicate field')
        value[key] = item
    return value


class Adapter:
    def __init__(self, args):
        self.args = args
        self.loop = GLib.MainLoop()
        self.client = None
        self.connection = None
        self.cancellable = Gio.Cancellable()
        self.stopped = False
        self.exit_code = 0
        self.input = bytearray()
        self.output = bytearray()
        self.writer = None
        self.busy = None
        self.deferred = None
        self.ready = False
        self.epoch = 0
        self.active = False
        self.last_mode = time.monotonic()
        os.set_blocking(0, False)
        os.set_blocking(1, False)
        GLib.io_add_watch(0, GLib.IO_IN | GLib.IO_HUP | GLib.IO_ERR, self.read)
        GLib.timeout_add(250, self.tick)
        GLib.timeout_add(2500, self.connect_deadline)
        Gio.DBusConnection.new_for_address(
            args.bus, Gio.DBusConnectionFlags.AUTHENTICATION_CLIENT |
            Gio.DBusConnectionFlags.MESSAGE_BUS_CONNECTION, None,
            self.cancellable, self.connected)

    def stop(self, code=0):
        if self.stopped:
            return
        self.stopped = True
        self.deferred = None
        self.exit_code = code
        self.cancellable.cancel()
        if self.client:
            self.client.close()
        elif self.connection and not self.connection.is_closed():
            self.connection.close(None, NativePublisher._closed_result)
        self.loop.quit()

    def emit(self, value):
        if self.stopped:
            return
        raw = json.dumps(value, separators=(',', ':'), ensure_ascii=True).encode() + b'\n'
        # Responses never contain a payload, binding, label, nonce or lease.
        if len(self.output) + len(raw) > 8192:
            self.stop(2)
            return
        self.output.extend(raw)
        if self.writer is None:
            self.writer = GLib.io_add_watch(1, GLib.IO_OUT | GLib.IO_ERR | GLib.IO_HUP, self.write)

    def write(self, _fd, condition):
        if condition & (GLib.IO_ERR | GLib.IO_HUP):
            self.stop(2)
            return False
        try:
            count = os.write(1, self.output)
            del self.output[:count]
        except BlockingIOError:
            return True
        except OSError:
            self.stop(2)
            return False
        if self.output:
            return True
        self.writer = None
        return False

    def connected(self, _source, result):
        try:
            connection = Gio.DBusConnection.new_for_address_finish(result)
            if self.stopped:
                connection.close(None, NativePublisher._closed_result)
                return
            self.connection = connection
            self.client = NativePublisher(connection, self.args.runtime, self.args.display, self.args.label)
            self.client.initialize(self.initialized)
        except (GLib.Error, ValueError, TypeError):
            self.stop(2)

    def initialized(self, ok):
        if not ok:
            self.stop(2)
            return
        self.ready = True
        self.emit(dict(type='hello', protocol=3, publisher='native-primary',
                       max_offer_bytes=MAX_TEXT, prepare_commit=True,
                       confirm_before_input=True, observes_selection=False))

    def connect_deadline(self):
        if not self.ready:
            self.stop(2)
        return False

    def tick(self):
        if self.stopped:
            return False
        if time.monotonic() - self.last_mode > 10 or self.client and self.client.closed:
            self.stop(2)
            return False
        if self.deferred and time.monotonic() - self.deferred[1] > 2.5:
            self.discard_deferred()
        return True

    def discard_deferred(self):
        pending, self.deferred = self.deferred, None
        if pending:
            value = pending[0]
            self.emit(dict(type='prepare-result', epoch=value['epoch'], id=value['id'], ok=False))

    def drain_deferred(self):
        if self.deferred is None or self.client._job is not None:
            return
        value, started = self.deferred
        if time.monotonic() - started > 2.5 or value['epoch'] != self.epoch or not self.active:
            self.discard_deferred()
            return
        self.deferred = None
        self.message(value)

    def read(self, _fd, condition):
        if condition & GLib.IO_ERR:
            self.stop(2)
            return False
        try:
            raw = os.read(0, 65536)
            if not raw:
                self.stop(2 if self.input else 0)
                return False
            self.input.extend(raw)
            if len(self.input) > FRAME + 65536:
                raise ValueError('Input bound')
            count = 0
            while b'\n' in self.input:
                count += 1
                if count > 64:
                    raise ValueError('Frame flood')
                line, _, rest = self.input.partition(b'\n')
                self.input = bytearray(rest)
                if len(line) > FRAME:
                    raise ValueError('Frame bound')
                value = json.loads(line.decode('utf-8', errors='strict'), object_pairs_hook=object_pairs)
                self.message(value)
                if self.stopped:
                    return False
            if len(self.input) > FRAME:
                raise ValueError('Frame bound')
        except BlockingIOError:
            return True
        except (OSError, ValueError, TypeError, UnicodeError, RecursionError):
            self.stop(2)
            return False
        return True

    def message(self, value):
        if not self.ready or type(value) is not dict:
            raise ValueError('Protocol state')
        kind = value.get('type')
        fields = {'mode': {'type', 'epoch', 'active'},
                  'enroll': {'type', 'epoch', 'id'},
                  'prepare': {'type', 'epoch', 'id', 'text'},
                  'commit': {'type', 'epoch', 'id'},
                  'confirm': {'type', 'epoch', 'id'},
                  'cancel': {'type', 'epoch', 'id'}}
        if not isinstance(kind, str) or kind not in fields or set(value) != fields[kind] or not integer(value['epoch']):
            raise ValueError('Invalid frame')
        epoch = value['epoch']
        if kind == 'mode':
            if type(value['active']) is not bool or epoch < self.epoch:
                raise ValueError('Invalid context')
            if epoch != self.epoch or value['active'] != self.active:
                self.discard_deferred()
            self.epoch, self.active, self.last_mode = epoch, value['active'], time.monotonic()
            def changed(ok):
                self.emit(dict(type='mode-result', epoch=epoch, ok=not self.client.closed,
                               registered=bool(ok and self.client.lease)))
                self.drain_deferred()
            self.client.mode(epoch, self.active, changed)
            return
        offer_id = value['id']
        if not integer(offer_id):
            raise ValueError('Invalid intent')
        response = dict(type=kind+'-result', epoch=epoch, id=offer_id)
        if epoch != self.epoch or not self.active or self.busy is not None:
            self.emit(dict(response, ok=False))
            if kind == 'cancel' and self.busy is not None:
                # Cancellation of an unknown/in-flight registration or offer
                # must revoke the dedicated peer, not leave it publishable.
                self.stop(2)
            return
        if self.deferred is not None:
            queued = self.deferred[0]
            if kind == 'cancel' and queued['id'] == offer_id:
                self.discard_deferred()
                self.emit(dict(response, ok=True))
            else:
                self.emit(dict(response, ok=False))
                if queued['id'] == offer_id:
                    self.stop(2)
            return
        job = self.client._job
        if (kind == 'prepare' and job is not None and job['method'] == 'Request'
                and job['request'] and job['request'].get('op') == 'mode'):
            # A same-read mode/prepare pair must not fail just because identity
            # verification runs asynchronously. Hold one bounded protocol frame;
            # a changed context, explicit cancel, EOF or deadline discards it.
            self.deferred = (value, time.monotonic())
            return
        token = object()
        self.busy = token

        def done(ok):
            if self.busy is token:
                self.busy = None
            self.emit(dict(response, ok=bool(ok and epoch == self.epoch and self.active)))

        if kind == 'enroll':
            self.client.enroll(done)
        elif kind == 'prepare':
            self.client.prepare(offer_id, value['text'], done)
        elif kind == 'commit':
            self.client.commit(offer_id, done)
        elif kind == 'confirm':
            self.client.confirm(offer_id, done)
        else:
            self.client.cancel(offer_id, done)


def main():
    parser = argparse.ArgumentParser()
    for name in ['bus', 'runtime', 'display', 'label']:
        parser.add_argument('--'+name, required=True)
    args = parser.parse_args()
    # Local authenticated Unix transport only; no TCP, autolaunch or address lists.
    if not args.bus.startswith('unix:') or ';' in args.bus or len(args.bus) > 4096:
        return 2
    adapter = Adapter(args)
    try:
        adapter.loop.run()
    finally:
        adapter.stop()
    return adapter.exit_code


if __name__ == '__main__':
    sys.exit(main())
