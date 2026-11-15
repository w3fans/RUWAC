#!/usr/bin/env python3
"""Refuse publication without executed gates bound to the current source/binaries."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import runpy
import stat
import subprocess
import tomllib

REQUIRED = frozenset({
    'terminal-pty', 'machine-inventory', 'ssh-key-access', 'ssh-gateway',
    'sftp-rsync-recovery', 'terminal-clipboard', 'rdp-cliprdr-loopback',
    'multi-tab-focus', 'agent-launch', 'png-attachment',
    'package-installation-rollback', 'original-endpoint-recovery',
    'primary-bridge', 'legacy-cli-comparison', 'idle-resources', 'privacy',
    'terminal-desktop-ergonomics', 'rdp-clipboard-ordering', 'web-preview',
})
SHA = re.compile(r'[0-9a-f]{64}\Z')


def read(path, limit):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    with os.fdopen(fd, 'rb') as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_size > limit:
            raise ValueError('Acceptance input is not a bounded regular file')
        data = stream.read(limit + 1)
    if len(data) > limit:
        raise ValueError('Acceptance input grew beyond its limit')
    return data


def digest(path):
    return hashlib.sha256(read(path, 256 * 1024 * 1024)).hexdigest()


def sources(root):
    inventory = runpy.run_path(str(Path(__file__).with_name('public-files.py')))['files'](root)
    return {p.as_posix(): digest(root / p) for p in inventory}


def validate(root, receipt_path, staged=False):
    root = root.resolve(strict=True)
    receipt = json.loads(read(receipt_path, 4 * 1024 * 1024))
    if not isinstance(receipt, dict) or type(receipt.get('schema')) is not int or receipt['schema'] != 1 or receipt.get('status') != 'PASS':
        raise ValueError('Final frozen acceptance has not passed')
    manifests = [tomllib.loads(read(root / p, 1024 * 1024).decode()) for p in
                 ['ruwac/Cargo.toml', 'ruwac/desktop/Cargo.toml']]
    current = manifests[0]['workspace']['package']['version']
    if receipt.get('version') != current or manifests[1]['package']['version'] != current:
        raise ValueError('Acceptance and compiled package versions differ')
    expected = receipt.get('source')
    if not isinstance(expected, dict) or not expected or expected != sources(root):
        raise ValueError('Source changed or is missing from the frozen acceptance')
    binaries = receipt.get('binaries')
    if not isinstance(binaries, dict) or not {'ruwac', 'ruwac-desktop'} <= binaries.keys():
        raise ValueError('Both compiled applications require acceptance hashes')
    for name, fingerprint in binaries.items():
        path = Path(name)
        if path.is_absolute() or '..' in path.parts or not isinstance(fingerprint, str) or not SHA.fullmatch(fingerprint):
            raise ValueError('Invalid accepted binary identity')
        if digest(root / 'target/release' / path) != fingerprint:
            raise ValueError('Compiled binary changed after acceptance: ' + name)
    gates = receipt.get('gates')
    if not isinstance(gates, list) or any(not isinstance(g, dict) for g in gates):
        raise ValueError('Acceptance gate results are missing')
    names = [g.get('name') for g in gates]
    if any(not isinstance(n, str) for n in names) or len(names) != len(set(names)):
        raise ValueError('Acceptance gate names are invalid or duplicated')
    by_name = {g['name']: g for g in gates}
    missing = sorted(n for n in REQUIRED if by_name.get(n, {}).get('status') != 'PASS')
    if missing:
        raise ValueError('Required acceptance gates have not passed: ' + ', '.join(missing))
    evidence_roots = (root, root.parent / (root.name + '-private'))
    for name in sorted(REQUIRED):
        evidence = by_name[name].get('evidence')
        if not isinstance(evidence, list) or not evidence:
            raise ValueError('Executed evidence is missing for ' + name)
        for item in evidence:
            if not isinstance(item, dict) or not isinstance(item.get('path'), str):
                raise ValueError('Invalid evidence identity for ' + name)
            path = Path(item['path'])
            resolved = path.resolve(strict=True)
            if not path.is_absolute() or path != resolved or not any(resolved.is_relative_to(p) for p in evidence_roots):
                raise ValueError('Evidence must be an owned project path without symlinks')
            if not isinstance(item.get('sha256'), str) or not SHA.fullmatch(item['sha256']) or digest(path) != item['sha256']:
                raise ValueError('Executed evidence changed for ' + name)
    if staged:
        # A later documentation/code edit must not slip into the index after
        # the tests. Read each staged blob, not just the working tree again.
        for name, fingerprint in expected.items():
            size = int(subprocess.check_output(['git', '-C', str(root), 'cat-file', '-s', ':' + name], timeout=15))
            if size != (root / name).stat().st_size:
                raise ValueError('Staged source differs from accepted source')
            data = subprocess.check_output(['git', '-C', str(root), 'show', ':' + name], timeout=15)
            if hashlib.sha256(data).hexdigest() != fingerprint:
                raise ValueError('Staged source differs from accepted source')
    return {'status': 'PASS', 'version': current, 'required_gates': sorted(REQUIRED)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-root', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    parser.add_argument('--staged', action='store_true')
    args = parser.parse_args()
    try:
        print(json.dumps(validate(args.build_root, args.receipt, args.staged)))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        raise SystemExit('Publication held: ' + str(error)) from None


if __name__ == '__main__':
    main()
