#!/usr/bin/env python3
"""Verify the standalone companion archive with official GNOME tools.

Needs gnome-extensions and glib-compile-schemas, but no running desktop.
Run under the project's bounded test runner; outputs stay in --evidence.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import zipfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=Path(__file__).resolve().parents[3])
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    component = source / 'ruwac/desktop/primary-companion'
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    names = {
        'extension.js', 'prefs.js', 'metadata.json', 'consent_gate.js',
        'consent_state.js', 'consent_preferences.js', 'enrollment.js',
        'lease_receiver.js', 'socket_guard.js', 'README.md', 'LICENSE', 'NOTICE',
        'schemas/org.gnome.shell.extensions.ruwac.primary.gschema.xml',
    }
    assert (component / 'LICENSE').read_bytes() == (source / 'LICENSE').read_bytes()
    extras = sorted(names - {'extension.js', 'prefs.js', 'metadata.json'} -
                    {name for name in names if '/' in name})
    subprocess.run([
        'gnome-extensions', 'pack', '--force', '--out-dir', str(evidence),
        *[arg for name in extras for arg in ['--extra-source', name]],
        '--schema', 'schemas/org.gnome.shell.extensions.ruwac.primary.gschema.xml',
        str(component),
    ], check=True, timeout=30)
    archives = list(evidence.glob('*.shell-extension.zip'))
    assert len(archives) == 1
    archive = archives[0]
    extracted = evidence / 'extracted'
    with zipfile.ZipFile(archive) as packed:
        files = [name for name in packed.namelist() if not name.endswith('/')]
        assert len(files) == len(set(files)), 'Duplicate archive entries'
        assert set(files) == names, 'Missing resources or unexpected archive files'
        for name in files:
            assert packed.read(name) == (component / name).read_bytes(), name
        # Every allowed path above is a fixed, relative product resource.
        for name in files:
            destination = extracted / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(packed.read(name))
    subprocess.run(['glib-compile-schemas', '--strict', str(extracted / 'schemas')],
                   check=True, timeout=15)
    assert (extracted / 'schemas/gschemas.compiled').stat().st_size > 0
    receipt = {
        'PASS': True,
        'scope': 'Official standalone archive and schema compilation; no installation or desktop login',
        'archive_sha256': hashlib.sha256(archive.read_bytes()).hexdigest(),
        'source_files': {name: hashlib.sha256((component / name).read_bytes()).hexdigest()
                         for name in sorted(names)},
        'exact_inventory_and_bytes': True,
        'project_license_preserved': True,
        'schema_compiles': True,
    }
    (evidence / 'acceptance.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print('Standalone companion package PASS')


if __name__ == '__main__':
    main()
