#!/usr/bin/env python3
"""Portable package contracts, using independent system ar/tar readers."""
import gzip
import io
import json
from pathlib import Path
import runpy
import subprocess
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
P = runpy.run_path(str(ROOT / 'scripts/portable-cli.py'))


class PortablePackages(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='portable-package-test-')
        self.root = Path(self.temp.name)
        self.binary = b'\x7fELF-generated-unit-fixture'
        self.docs = {'LICENSE': b'Fixture license', 'notices/index.json': b'{}\n'}

    def tearDown(self):
        self.temp.cleanup()

    def make(self, directory):
        return P['artifacts'](directory, self.binary, self.docs, '0.4.71', 'a' * 40, 1234)

    def test_deterministic_archive_and_debian_payload(self):
        a, b = self.root / 'a', self.root / 'b'
        info = self.make(a)
        self.make(b)
        for name in [info['archive'], info['deb']]:
            self.assertEqual((a / name).read_bytes(), (b / name).read_bytes())
        listing = subprocess.check_output(['ar', 't', str(a / info['deb'])], text=True).splitlines()
        self.assertEqual(listing, ['debian-binary', 'control.tar.gz', 'data.tar.gz'])
        data = subprocess.check_output(['ar', 'p', str(a / info['deb']), 'data.tar.gz'])
        with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
            binary = archive.getmember('usr/bin/ruwac')
            self.assertEqual((binary.uid, binary.gid, binary.mode, binary.mtime), (0, 0, 0o755, 1234))
            self.assertEqual(archive.extractfile(binary).read(), self.binary)
        control = subprocess.check_output(['ar', 'p', str(a / info['deb']), 'control.tar.gz'])
        with tarfile.open(fileobj=io.BytesIO(control), mode='r:gz') as archive:
            self.assertEqual(set(archive.getnames()), {'control', 'md5sums'})
            text = archive.extractfile('control').read().decode()
            self.assertIn('Version: 0.4.71-1\n', text)
            self.assertIn('Architecture: amd64\n', text)
            self.assertNotIn('gtk', text.lower())

    def test_reject_unsafe_paths_missing_license_and_payload_collisions(self):
        for name in ['../escape', '/absolute', 'notices/../../escape', 'bad\npath']:
            with self.assertRaises(ValueError):
                P['tar_bytes']({name: (b'x', 0o644)}, 1)
        for docs in [{}, {'LICENSE': b''}, {'LICENSE': b'x', 'bin/ruwac': b'override'}]:
            with self.assertRaises(ValueError):
                P['artifacts'](self.root / 'bad', self.binary, docs, '0.4.71', 'a' * 40, 1)

    def test_preserve_existing_immutable_assets(self):
        info = self.make(self.root / 'out')
        before = (self.root / 'out' / info['archive']).read_bytes()
        with self.assertRaises(FileExistsError):
            self.make(self.root / 'out')
        self.assertEqual((self.root / 'out' / info['archive']).read_bytes(), before)

    def test_controller_identity_validation(self):
        for version in ['v0.4.71', '0.4.71\nInjected: field', '0.4.100', '../0.4.71']:
            with self.assertRaises(ValueError):
                P['identity'](version, 'a' * 40)
        with self.assertRaises(ValueError):
            P['identity']('0.4.71', 'main')

    def test_matrix_source_and_checksum_refusals(self):
        root = self.root / 'source'
        (root / 'scripts').mkdir(parents=True)
        (root / 'ruwac').mkdir()
        targets = json.loads((ROOT / 'scripts/portable-cli-targets.json').read_text())
        (root / 'scripts/portable-cli-targets.json').write_text(json.dumps(targets))
        (root / 'ruwac/Cargo.toml').write_text('[workspace.package]\nversion="0.4.71"\n')
        output = self.root / 'out'
        info = self.make(output)
        with self.assertRaises(ValueError):
            P['finish'](root, output, [])
        flags = {'PASS': True, 'deb_install_verify_remove': True, 'archive_exec': True,
                 'Unicode_echo': True, 'reinstall_preserves_engine_and_shell': True,
                 'restore_preserves_ids_and_history_without_process_resurrection': True,
                 'native_TUI_attach_detach_cycles': 2, 'version': '0.4.71',
                 'binary_sha256': info['binary_sha256']}
        # Metadata fixtures exercise the verifier; actual OS acceptance is a
        # separate mandatory Docker runtime job before release publication.
        P['finish'](root, output, [{**target, **flags} for target in targets])
        P['verify'](root, output, 'a' * 40)
        with self.assertRaises(ValueError):
            P['verify'](root, output, 'b' * 40)
        name = P['names']('0.4.71')[0] + '.sha256'
        (output / name).write_text('0' * 64 + '  altered\n')
        with self.assertRaises(ValueError):
            P['verify'](root, output, 'a' * 40)


if __name__ == '__main__':
    unittest.main()
