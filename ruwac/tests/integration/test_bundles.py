#!/usr/bin/env python3
"""Candidate archive integrity and reversible installation tests, without network."""
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import runpy
import shutil
import struct
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location('ruwac_bundles', ROOT / 'scripts/bundle-user.py')
bundles = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bundles)


class Bundles(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.environment = patch.dict(os.environ, {'XDG_CONFIG_HOME': str(self.root / 'config'), 'XDG_DATA_HOME': str(self.root / 'data')})
        self.environment.start()

    def tearDown(self):
        self.environment.stop()
        self.temp.cleanup()

    def package(self):
        source = self.root / 'registry/generated-package'
        source.mkdir(parents=True)
        (source / 'Cargo.toml').write_text('')
        (source / 'LICENSE').write_text('Generated license fixture, not a real legal grant.\n')
        (source / '.cargo-checksum.json').write_text(json.dumps({'files': {'LICENSE': bundles.digest(source / 'LICENSE')}}))
        return {'name': 'generated-package', 'version': '1.0.0', 'manifest_path': str(source / 'Cargo.toml'), 'license': 'MIT', 'license_file': None, 'repository': 'https://example.org/generated'}

    def fixture(self, version='0.0.1'):
        root = self.root / ('bundle-' + version)
        binary = root / 'target/release/ruwac'
        binary.parent.mkdir(parents=True)
        (root / 'ruwac').mkdir()
        (root / 'ruwac/Cargo.toml').write_text('[workspace.package]\nversion="' + version + '"\n')
        for name in ['ruwac', 'ruwac-desktop']:
            binary = root / 'target/release' / name
            binary.write_text('#!/bin/sh\nprintf "RUWAC ' + version + '\\n"\n')
            binary.chmod(0o755)
        packaging=root/'ruwac/packaging';packaging.mkdir()
        for name in ['ruwac.1','si.z4.ruwac.svg']:
            shutil.copyfile(ROOT/'ruwac/packaging'/name,packaging/name)
        (root / 'notices').mkdir()
        (root / 'notices/index.json').write_text('{"generated":true}\n')
        (root / 'LICENSE').write_text('Generated fixture\n')
        files = {str(path.relative_to(root)): bundles.digest(path) for path in root.rglob('*') if path.is_file()}
        manifest = {'schema': 1, 'kind': 'fedora-testing-bundle', 'version': version, 'source_commit': 'a' * 40,
                    'distribution': 'fedora', 'distribution_version': '44', 'architecture': 'x86_64',
                    'security_qualified': False, 'known_advisories': ['RUSTSEC-2023-0071'],
                    'files': files, 'artifacts': {name: files['target/release/' + name] for name in ['ruwac', 'ruwac-desktop']}}
        (root / 'bundle.json').write_text(json.dumps(manifest))
        return root

    def install(self, root):
        prefix = self.root / 'prefix'
        original = Path.read_text
        def read(path, *args, **kwargs):
            if str(path) == '/etc/os-release':
                return 'ID=fedora\nVERSION_ID=44\n'
            return original(path, *args, **kwargs)
        with patch.object(sys, 'argv', ['installer', '--build-root', str(root), '--desktop', '--prefix', str(prefix)]), patch.object(Path, 'read_text', read), patch('platform.machine', return_value='x86_64'), contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            result = runpy.run_path(str(ROOT / 'scripts/install-user.py'), run_name='__main__')
            os.close(result['lock'])
        return prefix

    def test_manual_icon_keywords_and_owned_launcher_migration(self):
        first=self.fixture('0.0.1');prefix=self.install(first)
        man=prefix/'share/man/man1/ruwac.1';icon=prefix/'share/icons/hicolor/scalable/apps/si.z4.ruwac.svg'
        self.assertTrue(man.is_file() and icon.is_file())
        launcher=prefix/'share/applications/si.z4.ruwac.desktop'
        text=launcher.read_text();self.assertIn('Keywords=terminal;ssh;rdp;',text)
        legacy=text.replace('Icon=si.z4.ruwac\nKeywords=terminal;ssh;rdp;remote desktop;multiplexer;screen;tmux;sftp;Codex;Claude;Kimi;','Icon=utilities-terminal')
        launcher.write_text(legacy)
        self.install(self.fixture('0.0.2'))
        self.assertEqual(launcher.read_text(),text)
        self.assertIn('releases/0.0.2/',str(man.resolve()))
        self.assertIn('releases/0.0.2/',str(icon.resolve()))

    def test_unrelated_manpage_is_protected(self):
        prefix=self.root/'prefix';folder=prefix/'share/man/man1';folder.mkdir(parents=True)
        page=folder/'ruwac.1';page.write_text('unrelated')
        with self.assertRaisesRegex(SystemExit,'unrelated resource'):
            self.install(self.fixture())
        self.assertEqual(page.read_text(),'unrelated')

    def test_registry_license_tampering_is_rejected(self):
        package = self.package()
        documents = bundles.license_documents(self.root, package)
        self.assertEqual(len(documents), 1)
        documents[0][1].write_text('Modified fixture')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            bundles.license_documents(self.root, package)

    def test_notices_retain_exact_bytes_and_public_identity(self):
        package = self.package()
        output = self.root / 'notices'
        rows = bundles.notices(self.root, output, [package])
        self.assertEqual(rows[0]['name'], package['name'])
        self.assertEqual((output / 'generated-package-1.0.0/LICENSE').read_bytes(), (Path(package['manifest_path']).parent / 'LICENSE').read_bytes())
        self.assertNotIn(str(self.root), (output / 'index.json').read_text())

    def test_unlisted_upstream_license_and_stale_vcs_fail_closed(self):
        package = self.package()
        source = Path(package['manifest_path']).parent
        (source / 'LICENSE').unlink()
        (source / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': 'a' * 40}}))
        (self.root / 'scripts').mkdir()
        (self.root / 'scripts/license-sources.json').write_text('{"documents":[]}')
        with self.assertRaisesRegex(ValueError, 'no verified'):
            bundles.license_documents(self.root, package)
        notice = self.root / 'ruwac/notices/generated-package-1.0.0/LICENSE-MIT'
        notice.parent.mkdir(parents=True)
        notice.write_text('Generated fixture')
        (self.root / 'scripts/license-sources.json').write_text(json.dumps({'documents': [{'package': package['name'], 'version': package['version'], 'vcs_commit': 'b' * 40, 'file': str(notice.relative_to(self.root)), 'sha256': bundles.digest(notice), 'source': 'https://example.org/generated'}]}))
        with self.assertRaisesRegex(ValueError, 'provenance'):
            bundles.license_documents(self.root, package)

    def test_archive_assembly_is_deterministic_and_has_no_private_headers(self):
        directory = self.root / 'generated-archive'
        directory.mkdir()
        (directory / 'README').write_text('Generated fixture')
        first, second = self.root / 'first.tar.gz', self.root / 'second.tar.gz'
        bundles.archive(directory, first, 1)
        bundles.archive(directory, second, 1)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        with tarfile.open(first) as archive:
            for member in archive.getmembers():
                self.assertEqual(member.uid, 0)
                self.assertEqual(member.uname, '')
                self.assertNotIn(str(self.root), member.name)

    def test_archive_refuses_symlinks_and_preserves_existing_output(self):
        directory = self.root / 'generated-archive'
        directory.mkdir()
        (directory / 'linked').symlink_to(self.root / 'other')
        with self.assertRaises(ValueError): bundles.archive(directory, self.root / 'archive.tar.gz', 1)
        self.assertFalse((self.root / 'archive.tar.gz').exists())
        existing = self.root / 'existing.tar.gz'; existing.write_text('preserve')
        with self.assertRaises(FileExistsError): bundles.archive(directory, existing, 1)
        self.assertEqual(existing.read_text(), 'preserve')

    def test_bundle_install_needs_no_git_and_retains_notices_and_old_executable(self):
        first = self.fixture()
        prefix = self.install(first)
        binary = (prefix / 'bin/ruwac').resolve()
        inode = binary.stat().st_ino
        self.install(self.fixture('0.0.2'))
        self.assertEqual(binary.stat().st_ino, inode)
        self.assertEqual((prefix / 'lib/ruwac/previous').readlink(), Path('releases/0.0.1'))
        self.assertTrue((prefix / 'lib/ruwac/current/notices/index.json').is_file())
        self.assertEqual(json.loads((prefix / 'lib/ruwac/current/installation.json').read_text())['source_commit'], 'a' * 40)

    def test_tampered_bundle_is_rejected_before_any_executable_runs(self):
        root = self.fixture()
        (root / 'target/release/ruwac').write_text('#!/bin/sh\ntouch "' + str(self.root / 'executed') + '"\n')
        with self.assertRaisesRegex(SystemExit, 'integrity'): self.install(root)
        self.assertFalse((self.root / 'executed').exists())
        self.assertFalse((self.root / 'prefix').exists())

    def test_extra_files_and_directory_symlinks_are_rejected(self):
        root = self.fixture()
        (root / 'extra').write_text('Generated fixture')
        with self.assertRaisesRegex(SystemExit, 'inventory'): self.install(root)
        (root / 'extra').unlink()
        (root / 'notices/linked').symlink_to(self.root, target_is_directory=True)
        with self.assertRaisesRegex(SystemExit, 'symlinks'): self.install(root)

    def test_wrong_architecture_or_security_claim_is_rejected(self):
        root = self.fixture()
        path = root / 'bundle.json'
        manifest = json.loads(path.read_text())
        manifest['architecture'] = 'generated-incompatible-architecture'
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SystemExit, 'architecture'): self.install(root)
        manifest['architecture'] = 'x86_64'; manifest['security_qualified'] = True
        path.write_text(json.dumps(manifest))
        with self.assertRaisesRegex(SystemExit, 'qualification'): self.install(root)

    def elf(self, code, constants):
        path = self.root / 'generated-elf'
        table = 64
        start = table + 3 * 64
        header = struct.pack('<16sHHIQQQIHHHHHH', b'\x7fELF\x02\x01' + bytes(10), 3, 62, 1, 0, 0, table, 0, 64, 0, 0, 64, 3, 0)
        sections = bytes(64) + struct.pack('<IIQQQQIIQQ', 0, 1, 6, 0, start, len(code), 0, 0, 1, 0) + struct.pack('<IIQQQQIIQQ', 0, 1, 2, 0, start + len(code), len(constants), 0, 0, 1, 0)
        path.write_bytes(header + sections + code + constants)
        return path

    def test_elf_scanner_separates_opcode_bytes_from_private_text(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        path = self.elf(b'\x90labx\x00', b'portable fixture\x00')
        found, strings = bundles.artifact_findings(path, checker, ['labx'])
        self.assertFalse(found)
        self.assertNotIn(b'labx', strings)
        path = self.elf(b'\x90', b'labx\x00')
        self.assertTrue(bundles.artifact_findings(path, checker, ['labx'])[0])

    def test_elf_scanner_rejects_long_private_values_even_in_code(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        path = self.elf(b'generated-private-host\x00', b'portable fixture\x00')
        self.assertTrue(bundles.artifact_findings(path, checker, ['generated-private-host'])[0])

    def relocation_elf(self, literal):
        # Generated pointer bytes happen to spell a three-letter identifier.
        # An actual literal must still fail independently of the numeric slots.
        path = self.root / 'relocation-elf'
        table, count = 64, 4
        start = table + count * 64
        relocation = struct.pack('<QQq', 0x2000, 8, int.from_bytes(b'toy' + bytes(5), 'little'))
        pointer = b'toy' + bytes(5)
        header = struct.pack('<16sHHIQQQIHHHHHH', b'\x7fELF\x02\x01' + bytes(10), 3, 62, 1, 0, 0, table, 0, 64, 0, 0, 64, count, 0)
        records = bytes(64)
        records += struct.pack('<IIQQQQIIQQ', 0, 4, 2, 0x1000, start, len(relocation), 0, 0, 8, 24)
        records += struct.pack('<IIQQQQIIQQ', 0, 1, 3, 0x2000, start + len(relocation), len(pointer), 0, 0, 8, 0)
        records += struct.pack('<IIQQQQIIQQ', 0, 1, 2, 0x3000, start + len(relocation) + len(pointer), len(literal), 0, 0, 1, 0)
        path.write_bytes(header + records + relocation + pointer + literal)
        return path

    def test_relocated_numeric_addresses_do_not_become_short_identifiers(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertFalse(bundles.artifact_findings(self.relocation_elf(b'generated fixture'), checker, ['toy'])[0])

    def test_actual_short_string_is_still_rejected_beside_relocations(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertTrue(bundles.artifact_findings(self.relocation_elf(b'toy\x00'), checker, ['toy'])[0])

    def branch_elf(self, literal=b'generated fixture', bad_target=False, unchecked=False):
        # The generated relative address happens to encode a short identifier.
        table, dispatch = 0x3000, 0x1000
        target = table + int.from_bytes(b'toy\0', 'little')
        code = (b'\x48\x8d\x41\xff\x48\x83\xf8\x02\x0f\x83'
                + struct.pack('<i', 0) + b'\x48\x8d\x05'
                + struct.pack('<i', table - dispatch - 21)
                + b'\x48\x63\x0c\x88\x48\x01\xc1\xff\xe1')
        if unchecked:
            code = b'\x90' + code[1:]
        numbers = b'toy\0' * 3
        if bad_target:
            numbers = numbers[:8] + struct.pack('<i', -1)
        payloads = [code, b'\x90\xc3', numbers + literal]
        header = struct.pack('<16sHHIQQQIHHHHHH', b'\x7fELF\x02\x01' + bytes(10),
                             3, 62, 1, 0, 0, 64, 0, 64, 0, 0, 64, 4, 0)
        records = bytes(64)
        offset = 64 + 4 * 64
        for address, flags, content in zip([dispatch, target, table], [6, 6, 2], payloads):
            records += struct.pack('<IIQQQQIIQQ', 0, 1, flags, address, offset, len(content), 0, 0, 1, 0)
            offset += len(content)
        path = self.root / 'generated-branch-elf'
        path.write_bytes(header + records + b''.join(payloads))
        return path

    def test_validated_relative_branch_addresses_are_numeric(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertFalse(bundles.artifact_findings(self.branch_elf(), checker, ['toy'])[0])

    def test_private_literal_beside_branch_table_is_rejected(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertTrue(bundles.artifact_findings(self.branch_elf(b'toy\0'), checker, ['toy'])[0])

    def test_unchecked_or_invalid_branch_targets_retain_full_scan(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        for options in [{'unchecked': True}, {'bad_target': True}]:
            self.assertTrue(bundles.artifact_findings(self.branch_elf(**options), checker, ['toy'])[0])

    def byte_branch_elf(self, literal=b'generated fixture', bad_target=False, unknown=False, spill=True, count=4, positive=False, writable=False, unaligned=False, wrong_jump=False, extra_reference=False, far=False):
        table = (1342177280 if far else 16777216) + (1 if unaligned else 0)
        dispatch = 4096
        numeric = b'toy' + (b'\xc3' if far else b'\x00' if positive else b'\xff')
        target = table + int.from_bytes(numeric, 'little', signed=True)
        code = b'\x0f\xb6F\x10H\x8d\r' + struct.pack('<i', table - dispatch - 11) + b'Hc\x04\x81H\x01\xc8' + (b'H\x89t$\x08' if spill else b'') + b'\xff\xe0'
        if unknown:
            code = b'\x90' + code[1:]
        if wrong_jump:
            code = code[:-1] + b'\xe1'
        if extra_reference:
            code += b'H\x8d\r' + struct.pack('<i', table + 16 - dispatch - len(code) - 7)
        numbers = numeric * count
        if bad_target:
            numbers = numbers[:8] + b'toy\xfe' + numbers[12:]
        payloads = [code, b'\x90' * 64 + b'\xc3', numbers + literal]
        header = struct.pack('<16sHHIQQQIHHHHHH', b'\x7fELF\x02\x01' + bytes(10), 3, 62, 1, 0, 0, 64, 0, 64, 0, 0, 64, 4, 0)
        records = bytes(64)
        offset = 64 + 4 * 64
        for address, flags, content in zip([dispatch, target, table], [6, 6, 3 if writable else 2], payloads):
            records += struct.pack('<IIQQQQIIQQ', 0, 1, flags, address, offset, len(content), 0, 0, 1, 0)
            offset += len(content)
        path = self.root / 'synthetic-elf'
        path.write_bytes(header + records + b''.join(payloads))
        return path

    def byte_branch_findings(self, **kw):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        return bundles.artifact_findings(self.byte_branch_elf(**kw), checker, ['toy'])[0]

    def test_byte_dispatch_numeric_prefix_with_and_without_spill(self):
        for spill in [False, True]:
            self.assertFalse(self.byte_branch_findings(spill=spill))

    def test_byte_dispatch_adjacent_literal_is_still_rejected(self):
        self.assertTrue(self.byte_branch_findings(literal=b'toy\x00'))

    def test_byte_dispatch_unknown_instruction_is_not_masked(self):
        self.assertTrue(self.byte_branch_findings(unknown=True))

    def test_byte_dispatch_wrong_jump_register_is_not_masked(self):
        self.assertTrue(self.byte_branch_findings(wrong_jump=True))

    def test_byte_dispatch_invalid_target_retains_the_finding(self):
        self.assertTrue(self.byte_branch_findings(bad_target=True))

    def test_byte_dispatch_positive_printable_word_is_not_masked(self):
        self.assertTrue(self.byte_branch_findings(positive=True))

    def test_byte_dispatch_writable_data_is_not_masked(self):
        self.assertTrue(self.byte_branch_findings(writable=True))

    def test_byte_dispatch_unaligned_table_is_not_masked(self):
        self.assertTrue(self.byte_branch_findings(unaligned=True))

    def test_byte_dispatch_byte_index_bounds_256_slots(self):
        self.assertFalse(self.byte_branch_findings(count=256))
        self.assertTrue(self.byte_branch_findings(count=257))

    def test_byte_dispatch_one_offset_is_insufficient(self):
        self.assertTrue(self.byte_branch_findings(count=1))

    def test_byte_dispatch_far_negative_offsets_remain_fully_scanned(self):
        self.assertTrue(self.byte_branch_findings(far=True))

    def test_byte_dispatch_independent_reference_stops_numeric_prefix(self):
        self.assertTrue(self.byte_branch_findings(count=8, extra_reference=True))

    def test_byte_dispatch_key_material_is_still_rejected(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        # Generated fake PEM; construct markers as in the existing key fixture.
        value = b'-----BEGIN ' + b'PRIVATE KEY-----\n' + b'A' * 64 + b'\n-----END ' + b'PRIVATE KEY-----'
        found = bundles.artifact_findings(self.byte_branch_elf(literal=value), checker, [])[0]
        self.assertTrue(any((rule == 'private-key-material' for rule, _ in found)))

    def unwind_elf(self, literal=b'generated fixture', invalid_range=False):
        path = self.root / 'unwind-elf'
        cie_body = struct.pack('<I', 0) + b'\x01zR\x00\x01\x78\x10\x01\x1b\x0c\x07\x08'
        cie = struct.pack('<I', len(cie_body)) + cie_body
        unwind_address = 0x3000
        begin_offset = len(cie) + 8
        encoded = b'abc\x00'  # Generated numeric offset, not a site identifier.
        code_address = unwind_address + begin_offset + int.from_bytes(encoded, 'little')
        fde_body = struct.pack('<I', len(cie) + 4) + encoded + struct.pack('<I', 8) + b'\x00\x00\x00\x00'
        unwind = cie + struct.pack('<I', len(fde_body)) + fde_body + bytes(4)
        names = b'\x00.text\x00.eh_frame\x00.rodata\x00.shstrtab\x00'
        payloads = [b'\x90' * 8, unwind, literal, names]
        offsets = []; offset = 64 + 5 * 64
        for payload in payloads:offsets.append(offset);offset += len(payload)
        header = struct.pack('<16sHHIQQQIHHHHHH', b'\x7fELF\x02\x01'+bytes(10), 3, 62, 1, 0, 0, 64, 0, 64, 0, 0, 64, 5, 4)
        records = bytes(64)
        records += struct.pack('<IIQQQQIIQQ', 1, 1, 6, code_address + (100 if invalid_range else 0), offsets[0], 8, 0, 0, 1, 0)
        records += struct.pack('<IIQQQQIIQQ', 7, 1, 2, unwind_address, offsets[1], len(unwind), 0, 0, 4, 0)
        records += struct.pack('<IIQQQQIIQQ', 17, 1, 2, 0x5000, offsets[2], len(literal), 0, 0, 1, 0)
        records += struct.pack('<IIQQQQIIQQ', 25, 3, 0, 0, offsets[3], len(names), 0, 0, 1, 0)
        path.write_bytes(header + records + b''.join(payloads))
        return path

    def test_unwind_numeric_code_offset_is_not_a_short_name(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertFalse(bundles.artifact_findings(self.unwind_elf(), checker, ['abc'])[0])

    def test_unwind_mask_does_not_hide_actual_string(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertTrue(bundles.artifact_findings(self.unwind_elf(b'abc\x00'), checker, ['abc'])[0])
        self.assertTrue(bundles.artifact_findings(self.unwind_elf(b'generated-private-host\x00'), checker, ['generated-private-host'])[0])

    def test_unwind_pointer_must_resolve_to_executable_code(self):
        with self.assertRaisesRegex(ValueError, 'outside executable'):
            bundles.binary_data(self.unwind_elf(invalid_range=True))

    def test_unwind_malformed_records_fail_closed(self):
        path = self.unwind_elf();data = bytearray(path.read_bytes())
        # Corrupt the unwind record length while retaining valid ELF bounds.
        struct.pack_into('<I', data, 64 + 5 * 64 + 8, 0xffffffff)
        path.write_bytes(data)
        with self.assertRaisesRegex(ValueError, 'record bounds'):
            bundles.binary_data(path)

    def test_unwind_index_only_masks_valid_numeric_offsets(self):
        address, frame_address = 0x1000, 0x3000
        code = address + int.from_bytes(b'abc\0', 'little')
        frames = struct.pack('<II', 4, 0) + struct.pack('<II', 4, 4) + bytes(4)
        index = b'\x01\x1b\x03\x3b' + struct.pack('<iIii', frame_address-address-4, 1, code-address, frame_address+8-address)
        masked = bundles.unwind_index_data(index, address, frame_address, frames, [(code, code+8)])
        self.assertEqual(masked[:4], index[:4])
        self.assertEqual(masked[4:], bytes(len(index)-4))
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        self.assertFalse(checker['findings'](bytes(masked), ['abc']))
        # A real string outside the validated index is still rejected.
        self.assertTrue(checker['findings'](bytes(masked) + b'abc\0', ['abc']))
        for corrupt in [index[:-1], index+b'abc', b'\x02'+index[1:], index[:8]+struct.pack('<I',2)+index[12:], index[:16]+struct.pack('<i',123)]:
            with self.assertRaises(ValueError):
                bundles.unwind_index_data(corrupt, address, frame_address, frames, [(code, code+8)])
        with self.assertRaises(ValueError):
            bundles.unwind_index_data(index, address, frame_address, frames, [(code+1, code+8)])

    def test_elf_scanner_rejects_key_material_but_allows_parser_markers(self):
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        marker = b'-----BEGIN ' + b'PRIVATE KEY-----'
        path = self.elf(b'\x90', marker + b'\x00')
        self.assertFalse(bundles.artifact_findings(path, checker, [])[0])
        path = self.elf(b'\x90', marker + b'\n' + b'A' * 64 + b'\n-----END ' + b'PRIVATE KEY-----')
        self.assertTrue(bundles.artifact_findings(path, checker, [])[0])

    def test_elf_scanner_rejects_truncated_sections(self):
        path = self.elf(b'\x90', b'portable fixture')
        path.write_bytes(path.read_bytes()[:-4])
        with self.assertRaisesRegex(ValueError, 'bounds'): bundles.binary_data(path)

    def test_exact_reviewed_notice_cannot_allow_changed_or_other_text(self):
        source = self.root / 'source'; source.mkdir()
        destination = self.root / 'candidate'
        notice = destination / 'notices/generated-1.0.0/LICENSE'
        notice.parent.mkdir(parents=True)
        notice.write_text('Generated upstream copyright generated-author-name\n')
        policy = self.root / 'private-notice-review.json'
        policy.write_text(json.dumps({'schema': 1, 'documents': {str(notice.relative_to(destination)): bundles.digest(notice)}}))
        policy.chmod(0o600)
        checker = runpy.run_path(str(ROOT / 'scripts/privacy-check.py'))
        with patch.dict(os.environ, {'RUWAC_PRIVATE_NOTICE_ALLOWLIST': str(policy)}):
            bundles.text_scan(source, destination, checker, ['generated-author-name'])
            notice.write_text('Changed generated-author-name\n')
            with self.assertRaises(ValueError): bundles.text_scan(source, destination, checker, ['generated-author-name'])
            notice.unlink()
            (destination / 'README').write_text('generated-author-name')
            with self.assertRaises(ValueError): bundles.text_scan(source, destination, checker, ['generated-author-name'])

    def test_notice_review_policy_rejects_public_permissions_and_source_paths(self):
        source = self.root / 'source'; source.mkdir()
        policy = self.root / 'private-notice-review.json'
        policy.write_text('{"schema":1,"documents":{}}'); policy.chmod(0o644)
        with patch.dict(os.environ, {'RUWAC_PRIVATE_NOTICE_ALLOWLIST': str(policy)}):
            with self.assertRaises(ValueError): bundles.notice_allowlist(source)
        policy.chmod(0o600)
        with patch.dict(os.environ, {'RUWAC_PRIVATE_NOTICE_ALLOWLIST': str(policy)}):
            with self.assertRaises(ValueError): bundles.notice_allowlist(self.root)

    def test_scanner_diagnostic_review_is_exact_and_does_not_allow_other_findings(self):
        directory = self.root / 'strings'; directory.mkdir()
        path = directory / 'ruwac-desktop.txt'
        generated = b'Generated static diagnostic fixture'
        path.write_bytes(generated + b'\n')
        finding = {'RuleID': 'generic-api-key', 'File': str(path), 'StartLine': 1, 'EndLine': 1}
        with patch.object(bundles, 'REVIEWED_BINARY_DIAGNOSTICS', {hashlib.sha256(generated).hexdigest()}):
            self.assertTrue(bundles.reviewed_diagnostic(finding, directory))
            self.assertFalse(bundles.reviewed_diagnostic(dict(finding, RuleID='private-key'), directory))
            path.write_bytes(generated + b' changed\n')
            self.assertFalse(bundles.reviewed_diagnostic(finding, directory))
            other = self.root / 'foreign.txt'; other.write_bytes(generated + b'\n')
            self.assertFalse(bundles.reviewed_diagnostic(dict(finding, File=str(other)), directory))


if __name__ == '__main__':
    unittest.main()
