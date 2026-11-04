"""Measurement regressions: real glyphs and bounded silent-peer handling."""
import importlib.util
import json
from pathlib import Path
import socket
import struct
import time
import unittest

spec = importlib.util.spec_from_file_location('typing_latency', Path(__file__).with_name('typing_latency.py'))
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)


def rpc_pair():
    rpc = bench.Rpc.__new__(bench.Rpc)
    rpc.s, peer = socket.socketpair()
    rpc.buffer, rpc.bytes = b'', 0
    return rpc, peer


def frame(text):
    return {'revision': 1, 'panes': [
        {'id': 'unmeasured', 'grid': {'cells': [[[c, False] for c in '{}α']] }},
        {'id': 'echo', 'metadata': '{}α', 'grid': {'cells': [[[c, False] for c in text]]}},
    ]}


class MeasurementTests(unittest.TestCase):
    def test_metadata_and_other_panes_are_not_echo(self):
        self.assertEqual(bench.visible_text(frame('READY'), 'echo'), 'READY')
        for char in '{}α':
            self.assertNotIn(char, bench.visible_text(frame('READY'), 'echo'))

    def test_literal_braces_and_unicode_cells_are_echo(self):
        for char in '{}α中':
            self.assertEqual(bench.visible_text(frame(char), 'echo'), char)
        self.assertEqual(bench.visible_text(frame(''), 'echo'), '')
        with self.assertRaises(ValueError):
            bench.visible_text(frame(''), 'missing')

    def test_wide_spacers_do_not_duplicate_glyphs(self):
        value = frame('中')
        value['panes'][1]['grid']['cells'][0].append([' ', True])
        self.assertEqual(bench.visible_text(value, 'echo'), '中')

    def test_unchanged_frame_reuses_only_visible_text(self):
        rpc, peer = rpc_pair()
        target = bench.GuiTarget.__new__(bench.GuiTarget)
        target.rpc, target.pane, target.bytes, target.revision = rpc, 'echo', 0, None
        try:
            for value in [frame('READY'), {'unchanged': True}]:
                body = json.dumps({'result': value}).encode()
                peer.sendall(struct.pack('!I', len(body)) + body)
                self.assertEqual(target.frame_text(), 'READY')
        finally:
            rpc.close()
            peer.close()

    def test_silent_rpc_closes_at_deadline(self):
        rpc, peer = rpc_pair()
        started = time.monotonic()
        try:
            with self.assertRaises(TimeoutError):
                rpc.call('health', deadline=started + .04)
            self.assertLess(time.monotonic() - started, .5)
            self.assertEqual(rpc.s.fileno(), -1)
        finally:
            rpc.close()
            peer.close()

    def test_silent_probe_is_abandoned_at_deadline(self):
        rpc, peer = rpc_pair()
        started, marks = time.monotonic(), {}
        try:
            bench.finish_probes({'inner': (rpc, 'echo')}, marks, started + .04, started)
            self.assertEqual(marks, {'inner': None})
            self.assertEqual(rpc.s.fileno(), -1)
            self.assertLess(time.monotonic() - started, .5)
        finally:
            rpc.close()
            peer.close()

    def test_input_timeout_is_a_failed_sample(self):
        rpc, peer = rpc_pair()
        target = bench.GuiTarget.__new__(bench.GuiTarget)
        target.rpc, target.pane, target.stages = rpc, 'echo', {}
        started = time.monotonic()
        try:
            value, marks = target.keystroke('{', .04)
            self.assertIsNone(value)
            self.assertEqual(marks, {})
            self.assertLess(time.monotonic() - started, .5)
        finally:
            rpc.close()
            peer.close()

    def test_render_timeout_is_a_failed_sample(self):
        rpc, peer = rpc_pair()
        target = bench.GuiTarget.__new__(bench.GuiTarget)
        target.rpc, target.pane, target.stages = rpc, 'echo', {}
        target.bytes, target.revision = 0, None
        body = json.dumps({'result': {'queued': True}}).encode()
        peer.sendall(struct.pack('!I', len(body)) + body)
        started = time.monotonic()
        try:
            value, marks = target.keystroke('{', .04)
            self.assertIsNone(value)
            self.assertIn('ack_ms', marks)
            self.assertLess(time.monotonic() - started, .5)
        finally:
            rpc.close()
            peer.close()


if __name__ == '__main__':
    unittest.main()
