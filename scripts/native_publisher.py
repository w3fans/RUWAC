"""Native PRIMARY client. Owns one explicit, dedicated bus connection.

No desktop discovery, SSH creation, clipboard reads or fallback receiver. Every
operation is asynchronous; metadata work has one bounded worker. A cancelled
context never dispatches its queued commit. An already committed publication
cannot be undone by a later context change.
"""
import json
import re
import secrets
import threading
import time
from pathlib import Path

from gi.repository import Gio, GLib
from compositor_identity import identify, Refused

MAX_TEXT = 131072
MAX_INTEGER = (1 << 53) - 1
PUBLISHER = 'PrimaryPublisher1'
ENROLLMENT = 'PrimaryEnrollment1'
DESCRIPTION = dict(protocol=1, max_offer_bytes=MAX_TEXT, write_only=True,
                   prepare_commit=True, confirm_before_input=True,
                   identity_bound=True, revocable=True)
ENROLLMENT_DESCRIPTION = dict(protocol=1, explicit_desktop_approval=True,
                             prompt_timeout_ms=30000, grant_timeout_ms=8000, one_use=True)


def integer(value):
    return type(value) is int and 0 < value <= MAX_INTEGER


def exact(value, expected):
    return (type(value) is dict and set(value) == set(expected) and
            all(type(value[key]) is type(sample) and value[key] == sample
                for key, sample in expected.items()))


class NativePublisher:
    """The caller transfers exclusive ownership of connection to this client."""

    def __init__(self, connection, runtime, display, label):
        if not isinstance(display, str) or not re.fullmatch(r'wayland-[0-9]{1,3}', display):
            raise ValueError('Explicit desktop required')
        try:
            runtime = Path(runtime)
            if not runtime.is_absolute() or len(str(runtime / display).encode('utf-8')) >= 108:
                raise ValueError('Invalid runtime identity')
            valid_label = isinstance(label, str) and label.strip() and len(label.encode('utf-8')) <= 160
        except (TypeError, UnicodeError):
            raise ValueError('Invalid explicit desktop or label') from None
        if not valid_label:
            raise ValueError('Invalid connection label')
        if re.search(r'[\x00-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]', label):
            raise ValueError('Invalid connection label')
        self.connection, self.runtime, self.display = connection, runtime, display
        self.label = label
        self.identity = None
        self.lease = None
        self.epoch = 0
        self.active = False
        self.server_epoch = 0
        self.generation = 0
        self.last_mode = 0.0
        self.last_id = 0
        self.pending = None
        self.committed = None
        self.committed_until = 0.0
        self.ready = False
        self.closed = False
        self.reason = 'initializing'
        self._job = None
        self._worker_running = False
        self._closed_watch = connection.connect('closed', self._bus_closed)

    @staticmethod
    def later(callback, value):
        GLib.idle_add(lambda: (callback(value), False)[1])

    def notify(self, callback, value):
        generation = self.generation
        GLib.idle_add(lambda: (callback(value and not self.closed and generation == self.generation), False)[1])

    def _finish(self, job, result):
        if self._job is not job:
            return
        self._job = None
        if not job['answered']:
            job['answered'] = True
            job['callback'](result)

    def _abort(self):
        job, self._job = self._job, None
        if job is not None:
            job['cancel'].cancel()
            if not job['answered']:
                job['answered'] = True
                job['cancelled']()
        return job

    def _bus_closed(self, *_args):
        self._invalidate('connection-closed', close_bus=False)

    def _invalidate(self, reason, *, close_bus=True):
        if self.closed:
            return
        self.closed = True
        self.reason = reason
        self.ready = False
        self.generation += 1
        self.pending = None
        self.committed = None
        self.lease = None
        self._abort()
        if close_bus and not self.connection.is_closed():
            # Exclusive ownership: closing also revokes an unknown registration
            # whose reply was cancelled before its lease token reached us.
            self.connection.close(None, self._closed_result)

    @staticmethod
    def _closed_result(connection, result):
        try:
            connection.close_finish(result)
        except GLib.Error:
            pass

    def close(self):
        self._invalidate('closed')

    def _request(self, interface, method, request, callback, *, timeout=1500, cancelled):
        if self.closed or self._job is not None:
            cancelled()
            return
        job = dict(interface=interface, method=method, request=request,
                   callback=callback, generation=self.generation,
                   cancel=Gio.Cancellable(), answered=False, timeout=timeout, cancelled=cancelled)
        self._job = job
        if not self._worker_running:
            self._identity_worker(job)

    def _identity_worker(self, job):
        self._worker_running = True
        expected = None if self.identity is None else dict(self.identity)

        def work():
            try:
                pin = identify(self.connection, self.runtime, self.display, expected=expected)
            except (Refused, OSError, ValueError, GLib.Error):
                pin = None
            GLib.idle_add(lambda: self._identity_done(job, pin))

        try:
            threading.Thread(target=work, daemon=True).start()
        except RuntimeError:
            self._worker_running = False
            self._invalidate('metadata-unavailable')

    def _identity_done(self, job, pin):
        self._worker_running = False
        if self._job is not job or self.generation != job['generation'] or self.closed:
            if self._job is not None and not self.closed:
                self._identity_worker(self._job)
            return False
        if pin is None:
            self._invalidate('desktop-changed')
            return False
        if self.identity is None:
            self.identity = pin
        request = job['request']
        params = None
        if request is not None:
            params = GLib.Variant('(s)', (json.dumps(request, ensure_ascii=True, separators=(',', ':')),))
        self.connection.call(self.identity['owner'], '/org/ruwac/' + job['interface'],
                             'org.ruwac.' + job['interface'], job['method'], params,
                             None, Gio.DBusCallFlags.NONE, job['timeout'], job['cancel'],
                             lambda connection, result: self._reply(job, connection, result))
        return False

    def _reply(self, job, connection, result):
        if self._job is not job or self.generation != job['generation'] or self.closed:
            return
        try:
            raw = connection.call_finish(result).unpack()[0]
            if not isinstance(raw, str) or len(raw) > 4096:
                raise ValueError('Bounded response required')
            value = json.loads(raw)
            if type(value) is not dict:
                raise ValueError('Object response required')
        except (GLib.Error, ValueError, TypeError, IndexError):
            self._invalidate('receiver-unavailable')
            return
        self._finish(job, value)

    def initialize(self, callback):
        if self._job is not None:
            self.later(callback, False)
            return
        if self.ready or self.closed:
            self.notify(callback, self.ready and not self.closed)
            return

        def publisher(value):
            if not exact(value, DESCRIPTION):
                self._invalidate('unsupported-publisher')
                self.later(callback, False)
                return

            def enrollment(value):
                if not exact(value, ENROLLMENT_DESCRIPTION):
                    self._invalidate('unsupported-enrollment')
                    self.later(callback, False)
                    return
                self.ready = True
                self.reason = 'approval-required'
                self.notify(callback, True)
            self._request(ENROLLMENT, 'Describe', None, enrollment, cancelled=lambda: self.later(callback, False))
        self._request(PUBLISHER, 'Describe', None, publisher, cancelled=lambda: self.later(callback, False))

    def _result(self, value, expected):
        if exact(value, expected):
            return True
        if exact(value, dict(ok=False)) or value is None:
            return False
        self._invalidate('invalid-receiver-response')
        return False

    def enroll(self, callback):
        if self.closed or not self.ready or not self.active or self.lease or self._job:
            self.later(callback, False)
            return
        nonce = secrets.token_hex(24)
        request = dict(protocol=1, nonce=nonce, binding=self.identity, label=self.label)
        self.reason = 'awaiting-desktop-approval'

        def approved(value):
            if not self._result(value, dict(ok=True, enrolled=True, expires_in_ms=8000)):
                if not self.closed:
                    self.reason = 'approval-not-received'
                self.later(callback, False)
                return

            def registered(value):
                if (type(value) is not dict or set(value) != {'ok', 'protocol', 'lease'} or
                    type(value['ok']) is not bool or value['ok'] is not True or
                    type(value['protocol']) is not int or value['protocol'] != 1 or
                    not isinstance(value['lease'], str) or not 32 <= len(value['lease']) <= 128 or
                    re.search(r'[^0-9a-f-]', value['lease'])):
                    self._invalidate('registration-not-received')
                    self.later(callback, False)
                    return
                self.lease = value['lease']
                self.reason = 'registered'
                self._send_mode(callback)
            self._request(PUBLISHER, 'Request', dict(op='register', protocol=1,
                          nonce=nonce, binding=self.identity), registered, cancelled=lambda: self.later(callback, False))
        self._request(ENROLLMENT, 'Enroll', request, approved, timeout=35000, cancelled=lambda: self.later(callback, False))

    def _send_mode(self, callback):
        if not self.lease or self.closed:
            self.later(callback, False)
            return
        self.server_epoch += 1
        if self.server_epoch > MAX_INTEGER:
            self._invalidate('epoch-exhausted')
            self.later(callback, False)
            return

        def complete(value):
            ok = self._result(value, dict(ok=True))
            if ok:
                self.last_mode = time.monotonic()
            elif not self.closed:
                self._invalidate('registration-revoked')
            self.notify(callback, ok)
        self._request(PUBLISHER, 'Request', dict(op='mode', lease=self.lease,
                      epoch=self.server_epoch, active=self.active), complete, cancelled=lambda: self.later(callback, False))

    def mode(self, epoch, active, callback):
        if not integer(epoch) or type(active) is not bool or epoch < self.epoch:
            self._invalidate('invalid-context')
            self.later(callback, False)
            return
        changed = epoch != self.epoch or active != self.active
        if changed and epoch <= self.epoch:
            self._invalidate('invalid-context')
            self.later(callback, False)
            return
        if changed:
            self.epoch, self.active = epoch, active
            self.generation += 1
            self.pending = None
            self.committed = None
            old = self._abort()
            if old and (old['interface'] == ENROLLMENT and old['method'] == 'Enroll' or
                        old['request'] and old['request'].get('op') == 'register'):
                self._invalidate('approval-context-changed')
        if self.committed and time.monotonic() > self.committed_until:
            self.committed = None
        if self.closed or not self.lease:
            self.later(callback, False)
        elif not changed and (self._job or self.pending or self.committed or time.monotonic() - self.last_mode < 2):
            self.notify(callback, True)  # Do not invalidate a prepared offer with a heartbeat.
        else:
            self._send_mode(callback)

    def prepare(self, offer_id, text, callback):
        try:
            valid = (integer(offer_id) and offer_id > self.last_id and isinstance(text, str) and
                     text and '\0' not in text and len(text.encode('utf-8')) <= MAX_TEXT)
        except UnicodeError:
            valid = False
        if self.closed or not self.lease or not self.active or self.pending or self._job or not valid:
            self.later(callback, False)
            return
        self.last_id = offer_id
        self.committed = None
        epoch, generation = self.epoch, self.generation

        def complete(value):
            ok = self._result(value, dict(ok=True))
            if ok and self.epoch == epoch and self.generation == generation:
                self.pending = (epoch, offer_id, generation)
            else:
                ok = False
            self.notify(callback, ok)
        self._request(PUBLISHER, 'Request', dict(op='prepare', lease=self.lease,
                      epoch=self.server_epoch, id=offer_id, text=text), complete, cancelled=lambda: self.later(callback, False))

    def _complete_offer(self, offer_id, commit, callback):
        context = (self.epoch, offer_id, self.generation)
        pending = self.pending == context or not commit and self.committed == context
        if (not integer(offer_id) or self.closed or not self.lease or not self.active or self._job or not pending):
            self.later(callback, False)
            return
        self.pending = None
        self.committed = None
        expected = dict(ok=True, committed=True) if commit else dict(ok=True)
        def complete(value):
            ok = self._result(value, expected)
            if ok and commit and context == (self.epoch, offer_id, self.generation):
                self.committed = context
                self.committed_until = time.monotonic() + .75
            self.notify(callback, ok)
        self._request(PUBLISHER, 'Request', dict(op='commit' if commit else 'cancel',
                      lease=self.lease, epoch=self.server_epoch, id=offer_id),
                      complete, cancelled=lambda: self.later(callback, False))

    def commit(self, offer_id, callback):
        self._complete_offer(offer_id, True, callback)

    def cancel(self, offer_id, callback):
        self._complete_offer(offer_id, False, callback)

    def confirm(self, offer_id, callback):
        if (not integer(offer_id) or self.closed or not self.lease or not self.active or self._job or
            self.committed != (self.epoch, offer_id, self.generation)):
            self.later(callback, False)
            return
        self.committed = None
        self._request(PUBLISHER, 'Request', dict(op='confirm', lease=self.lease,
                      epoch=self.server_epoch, id=offer_id),
                      lambda value: self.notify(callback, self._result(value, dict(ok=True, confirmed=True))),
                      cancelled=lambda: self.later(callback, False))
