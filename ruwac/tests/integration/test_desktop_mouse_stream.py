#!/usr/bin/env python3
"""Deterministic chunking guards for the actual generated mouse-test receiver."""
import argparse
import ast
import io
import json
from pathlib import Path
import sys
from unittest.mock import patch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', type=Path)
    args = parser.parse_args()
    source = Path(__file__).with_name('desktop_ergonomics.py').read_text()
    programs = [ast.literal_eval(node.args[0]) for node in ast.walk(ast.parse(source))
                if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute)
                and node.func.attr == 'write_text' and isinstance(node.func.value, ast.Name)
                and node.func.value.id == 'mouse_receiver']
    assert len(programs) == 1
    mouse = b'\x1b[<64;9;6M'
    cases = []
    for name, chunks, expected in [
        ('standalone', [b'C'], True),
        ('mouse-and-command', [mouse + b'C'], True),
        ('command-and-mouse', [b'C' + mouse], True),
        ('separate-reads', [mouse, b'C'], True),
        ('no-command', [mouse], False),
    ]:
        pending = iter(chunks + [b''])
        writes = []
        sink = io.BytesIO()
        with patch('os.read', side_effect=lambda *_: next(pending)), \
             patch('os.write', side_effect=lambda _, data: writes.append(data) or len(data)), \
             patch('tty.setraw'), patch('builtins.open', return_value=sink), \
             patch.object(sys, 'argv', ['mouse-fixture', 'synthetic-input']):
            exec(compile(programs[0], '<generated-mouse-fixture>', 'exec'), {})
        assert any(b'MODE_CLICK' in data for data in writes) == expected, name
        cases.append(dict(case=name, PASS=True, expected_acknowledgment=expected))
    result = dict(PASS=True, cases=cases,
                  scope='Exact generated receiver with deterministic mocked reads/writes; no physical or PTY claim')
    if args.evidence:
        args.evidence.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
