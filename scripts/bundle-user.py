#!/usr/bin/env python3
"""Assemble a tested Fedora candidate from built artifacts; never assigns versions."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import runpy
import shutil
import stat
import struct
import subprocess
import tarfile
import tempfile
import tomllib

TARGET = 'x86_64-unknown-linux-gnu'
DOCUMENT = re.compile(r'(?i)^(licen[cs]e|notice|copying|copyright|unlicense)([._-]|$)')
# Exact public libssh2-sys 0.3.3 diagnostic, openssl.c error calls at lines
# 4864/4971. Its generic "key file:" wording is not a credential. Preserve every
# other finding, including altered text, other rules and additional matches.
REVIEWED_BINARY_DIAGNOSTICS = {'396ea0240201a02f4c5c68e40bf8fb2ca5bc83ef1e89a67cfc24aeaa57104826'}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def runtime_packages(root, applications=None):
    packages = {}
    for manifest, application in [('ruwac/Cargo.toml', 'ruwac-cli'), ('ruwac/desktop/Cargo.toml', 'ruwac-desktop')]:
        if applications is not None and application not in applications:
            continue
        metadata = json.loads(subprocess.check_output([
            'cargo', 'metadata', '--locked', '--filter-platform', TARGET,
            '--format-version', '1', '--manifest-path', str(root / manifest),
        ], text=True))
        nodes = {node['id']: node for node in metadata['resolve']['nodes']}
        declarations = {item['id']: item for item in metadata['packages']}
        pending = [item['id'] for item in declarations.values() if item['name'] == application]
        if len(pending) != 1:
            raise ValueError('Missing or ambiguous application package')
        visited = set()
        while pending:
            identity = pending.pop()
            if identity in visited:
                continue
            visited.add(identity)
            item = declarations[identity]
            key = (item['name'], item['version'])
            if key in packages and packages[key]['source'] != item['source']:
                raise ValueError('Ambiguous dependency source')
            packages[key] = item
            for dependency in nodes[identity]['deps']:
                if any(kind['kind'] != 'dev' for kind in dependency['dep_kinds']):
                    pending.append(dependency['pkg'])
    return sorted(packages.values(), key=lambda item: (item['name'], item['version']))


def license_documents(root, package):
    source = Path(package['manifest_path']).parent.resolve()
    if package['name'].startswith('ruwac-') and source.is_relative_to(root / 'ruwac'):
        return [('LICENSE', root / 'LICENSE', None)]
    documents = []
    checksum_file = source / '.cargo-checksum.json'
    checksums = json.loads(checksum_file.read_text())['files'] if checksum_file.is_file() else None
    for path in sorted(source.rglob('*')):
        relative = path.relative_to(source)
        if any(part in {'target', '.git', 'node_modules'} for part in relative.parts):
            continue
        if not DOCUMENT.match(path.name) or not path.is_file():
            continue
        if path.is_symlink() or not path.resolve().is_relative_to(source) or path.stat().st_size > 2 * 1024 * 1024:
            raise ValueError('Unsafe dependency license document')
        if checksums is not None and checksums.get(relative.as_posix()) != digest(path):
            raise ValueError('Registry license document differs from its extracted archive checksum')
        documents.append((relative.as_posix(), path, None))
    if package.get('license_file'):
        path = source / package['license_file']
        if not path.resolve().is_relative_to(source) or path.is_symlink() or not path.is_file():
            raise ValueError('Invalid declared license file')
        if not any(path == document[1] for document in documents):
            if checksums is not None and checksums.get(str(path.relative_to(source))) != digest(path):
                raise ValueError('Declared registry license file checksum mismatch')
            documents.append((str(path.relative_to(source)), path, None))
    if not documents:
        overrides = json.loads((root / 'scripts/license-sources.json').read_text())
        vcs = json.loads((source / '.cargo_vcs_info.json').read_text())['git']['sha1']
        for item in overrides['documents']:
            if (item['package'], item['version']) != (package['name'], package['version']):
                continue
            path = root / item['file']
            if item['vcs_commit'] != vcs or not path.resolve().is_relative_to(root / 'ruwac/notices') or path.is_symlink() or digest(path) != item['sha256']:
                raise ValueError('Pinned upstream license document does not match package provenance')
            documents.append((path.name, path, item['source']))
    if not documents or not package.get('license') and not package.get('license_file'):
        raise ValueError('Dependency has no verified license documents: ' + package['name'])
    return documents


def notices(root, destination, packages):
    rows = []
    for package in packages:
        name, version = package['name'], package['version']
        if not re.fullmatch(r'[A-Za-z0-9_-]+', name) or not re.fullmatch(r'[A-Za-z0-9.+_-]+', version):
            raise ValueError('Invalid dependency identity')
        documents = []
        for relative, path, origin in license_documents(root, package):
            target = destination / (name + '-' + version) / relative
            if Path(relative).is_absolute() or '..' in Path(relative).parts:
                raise ValueError('Unsafe dependency document destination')
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, target)
            documents.append({'file': str(target.relative_to(destination)), 'sha256': digest(target), 'pinned_upstream': origin})
        rows.append({'name': name, 'version': version, 'declared_license': package.get('license'),
                     'repository': package.get('repository'), 'documents': documents})
    (destination / 'index.json').write_text(json.dumps({'scope': 'Conservative Linux runtime/build dependency closure; excludes dev-only and other-target edges. Declarations and preserved texts do not prove legal or security qualification.', 'packages': rows}, indent=2) + '\n')
    return rows


def unwind_address_data(content, address, executable):
    """Mask only validated FDE PC address/range fields, retaining all other bytes.

    LSB exception-frame records describe numeric code ranges. Their linked
    offsets can coincidentally spell a short site label; they are not names.
    Unknown/truncated encodings fail closed. Augmentations and instructions
    remain subject to the normal string/private-policy scan.
    """
    data = bytearray(content)
    cies = {}
    at = 0

    def leb(position, end):
        value = 0
        for shift in range(0, 70, 7):
            if position >= end:
                raise ValueError('Truncated exception-frame integer')
            byte = data[position]
            position += 1
            value |= (byte & 127) << shift
            if not byte & 128:
                return value, position
        raise ValueError('Oversized exception-frame integer')

    def width(encoding):
        sizes = {0: 8, 2: 2, 3: 4, 4: 8, 10: 2, 11: 4, 12: 8}
        if encoding & 15 not in sizes:
            raise ValueError('Unsupported exception-frame pointer encoding')
        return sizes[encoding & 15]

    while at < len(data):
        if at + 4 > len(data):
            raise ValueError('Truncated exception-frame record')
        size = struct.unpack_from('<I', data, at)[0]
        if size == 0:
            if any(data[at:]):
                raise ValueError('Unexpected exception-frame trailer')
            break
        end = at + 4 + size
        if size == 0xffffffff or size < 4 or end > len(data):
            raise ValueError('Invalid exception-frame record bounds')
        reference = struct.unpack_from('<I', data, at + 4)[0]
        if reference == 0:
            if at + 9 >= end or data[at + 8] != 1:
                raise ValueError('Unsupported exception-frame CIE version')
            nul = data.find(b'\0', at + 9, end)
            if nul < 0:
                raise ValueError('Unterminated exception-frame augmentation')
            augmentation = bytes(data[at + 9:nul])
            if augmentation not in {b'zR', b'zPLR'}:
                raise ValueError('Unsupported exception-frame augmentation')
            _, position = leb(nul + 1, end)  # Code alignment.
            _, position = leb(position, end)  # Signed data alignment: same length encoding.
            if position >= end:
                raise ValueError('Missing exception-frame return register')
            length, position = leb(position + 1, end)
            aug_end = position + length
            if aug_end > end:
                raise ValueError('Invalid exception-frame augmentation length')
            encoding = None
            for letter in augmentation[1:]:
                if position >= aug_end:
                    raise ValueError('Truncated exception-frame augmentation data')
                value = data[position]
                position += 1
                if letter == ord('P'):
                    position += width(value)
                elif letter == ord('R'):
                    encoding = value
            if position != aug_end or encoding not in {0, 3, 11, 0x1b}:
                raise ValueError('Invalid or unsupported FDE address encoding')
            cies[at] = encoding
        else:
            cie = at + 4 - reference
            if cie not in cies:
                raise ValueError('Invalid exception-frame CIE reference')
            encoding = cies[cie]
            size_pc = width(encoding)
            begin = at + 8
            if begin + 2 * size_pc > end:
                raise ValueError('Truncated exception-frame code range')
            signed = encoding & 15 in {10, 11, 12}
            location = int.from_bytes(data[begin:begin + size_pc], 'little', signed=signed)
            if encoding & 0x70 == 0x10:
                location += address + begin
            extent = int.from_bytes(data[begin + size_pc:begin + 2 * size_pc], 'little', signed=signed)
            if extent < 0 or not any(start <= location and location + extent <= stop for start, stop in executable):
                raise ValueError('Exception-frame code range is outside executable sections')
            data[begin:begin + 2 * size_pc] = bytes(2 * size_pc)
        at = end
    return data


def unwind_index_data(content, address, frame_address, frames, executable):
    """Validate the x86_64 linker unwind index before excluding numeric slots."""
    if len(content) < 12 or content[:4] != b'\x01\x1b\x03\x3b':
        raise ValueError('Unsupported unwind-index encoding')
    frame_offset, count = struct.unpack_from('<iI', content, 4)
    if address + 4 + frame_offset != frame_address or len(content) != 12 + count * 8:
        raise ValueError('Invalid unwind-index bounds or frame reference')
    fdes = set()
    at = 0
    while at + 4 <= len(frames):
        size = struct.unpack_from('<I', frames, at)[0]
        if size == 0:
            if any(frames[at:]):
                raise ValueError('Invalid unwind-index frame trailer')
            at = len(frames)
            break
        if size < 4 or size == 0xffffffff or at + 4 + size > len(frames):
            raise ValueError('Invalid unwind-index frame record')
        if struct.unpack_from('<I', frames, at + 4)[0]:
            fdes.add(frame_address + at)
        at += 4 + size
    if at != len(frames) or count != len(fdes):
        raise ValueError('Unwind-index count differs from frame records')
    previous = -1
    for at in range(12, len(content), 8):
        code, fde = struct.unpack_from('<ii', content, at)
        code += address
        fde += address
        if code < previous or not any(start <= code < stop for start, stop in executable) or fde not in fdes:
            raise ValueError('Unwind-index entry is not an executable/frame address')
        previous = code
        fdes.remove(fde)
    return bytearray(content[:4]) + bytearray(len(content) - 4)


def indexed_branch_slots(data, records):
    """Recognize a bounded x86_64 relative branch table emitted by LLVM.

    This deliberately handles only one fully checked instruction form. Unknown
    forms remain ordinary scanned data. The index is checked as index-1 < count;
    every slot, including the unused zero slot, must target executable bytes.
    No text/section-name allowlist or private matching value is involved.
    """
    executable = [(s[3], s[3] + s[5]) for s in records if s[2] & 4]
    slots = set()
    # lea -1(%rcx),%rax; cmp $count,%rax; jae fallback;
    # lea table(%rip),%rax; movslq (%rax,%rcx,4),%rcx;
    # add %rax,%rcx; jmp *%rcx
    pattern = re.compile(rb'\x48\x8d\x41\xff\x48\x83\xf8(.)\x0f\x83(.{4})'
                         rb'\x48\x8d\x05(.{4})\x48\x63\x0c\x88\x48\x01\xc1\xff\xe1', re.S)
    for section in records:
        if not section[2] & 4:
            continue
        start, size = section[4:6]
        if start + size > len(data):
            raise ValueError('Executable section exceeds artifact bounds')
        for match in pattern.finditer(data[start:start + size]):
            count = match[1][0]
            if not 1 <= count <= 127:  # cmp imm8 is sign extended.
                continue
            pc = section[3] + match.start()
            fallback = pc + 14 + struct.unpack('<i', match[2])[0]
            table = pc + 21 + struct.unpack('<i', match[3])[0]
            length = (count + 1) * 4
            candidates = [s for s in records if s[1] == 1 and s[2] & 2
                          and not s[2] & 5 and s[3] <= table
                          and table + length <= s[3] + s[5]]
            if len(candidates) != 1 or not any(a <= fallback < b for a, b in executable):
                continue
            source = candidates[0]
            offset = source[4] + table - source[3]
            if offset + length > len(data):
                raise ValueError('Indexed branch table exceeds artifact bounds')
            targets = [table + struct.unpack_from('<i', data, offset + i * 4)[0]
                       for i in range(count + 1)]
            if all(any(a <= target < b for a, b in executable) for target in targets):
                slots.update(range(table, table + length, 4))
    return slots


def binary_data(path):
    """Read bounded non-executable ELF sections, retaining constants and symbols."""
    data = path.read_bytes()
    if len(data) < 64 or data[:6] != b'\x7fELF\x02\x01':
        raise ValueError('Expected a little-endian ELF64 artifact')
    header = struct.unpack_from('<16sHHIQQQIHHHHHH', data)
    offset, width, count = header[6], header[11], header[12]
    if header[2] != 62 or width != 64 or not 0 < count < 4096 or offset + width * count > len(data):
        raise ValueError('Invalid x86_64 ELF section table')
    records = [struct.unpack_from('<IIQQQQIIQQ', data, offset + index * width) for index in range(count)]
    branch_slots = indexed_branch_slots(data, records)
    relocations = set()
    for section in records:
        if section[1] not in {4, 9}:  # SHT_RELA / SHT_REL: ABI numeric records.
            continue
        start, size, entry = section[4], section[5], section[9]
        expected_entry = 24 if section[1] == 4 else 16
        if entry != expected_entry or size % entry or start + size > len(data):
            raise ValueError('Invalid ELF64 relocation table')
        for position in range(start, start + size, entry):
            address, info = struct.unpack_from('<QQ', data, position)
            if info & 0xffffffff in {1, 6, 7, 8}:  # x86_64 pointer-sized relocation kinds.
                relocations.add(address)
    names = b''
    if 0 <= header[13] < len(records):
        names_record = records[header[13]]
        if names_record[4] + names_record[5] > len(data):
            raise ValueError('ELF section names exceed artifact bounds')
        names = data[names_record[4]:names_record[4] + names_record[5]]
    executable = [(s[3], s[3] + s[5]) for s in records if s[2] & 4]
    frames = [s for s in records if names[s[0]:].split(b'\0', 1)[0] == b'.eh_frame']
    sections = []
    for section in records:
        kind, flags, address, start, size = section[1:6]
        if kind in [0, 8]:
            continue
        if start + size > len(data):
            raise ValueError('ELF section exceeds artifact bounds')
        if flags & 4 or kind in {4, 9}:  # Instructions and ABI numeric relocation tables.
            continue
        content = bytearray(data[start:start + size])
        name = names[section[0]:].split(b'\0', 1)[0] if section[0] < len(names) else b''
        if name == b'.eh_frame':
            content = unwind_address_data(content, address, executable)
        if name == b'.eh_frame_hdr':
            if len(frames) != 1 or frames[0][4] + frames[0][5] > len(data):
                raise ValueError('Unwind index requires one bounded frame section')
            frame = frames[0]
            content = unwind_index_data(content, address, frame[3], data[frame[4]:frame[4] + frame[5]], executable)
        if flags & 2:  # SHF_ALLOC: r_offset is an allocated virtual address.
            for pointer in relocations:
                if address <= pointer and pointer + 8 <= address + size:
                    at = pointer - address
                    content[at:at + 8] = bytes(8)
            for pointer in branch_slots:
                if address <= pointer and pointer + 4 <= address + size:
                    at = pointer - address
                    content[at:at + 4] = bytes(4)
        sections.append(bytes(content))
    return data, sections


def artifact_findings(path, checker, terms):
    data, sections = binary_data(path)
    text = b'\n'.join(sections)
    # Crypto parsers contain BEGIN markers as constants. Actual key blocks remain
    # rejected, while the source publication checker retains its stricter rule.
    found = [(rule, line) for rule, line in checker['findings'](text, terms) if rule != 'private-key']
    if re.search(rb'-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\r\n]{32,}-----END (?:[A-Z0-9 ]+ )?PRIVATE KEY-----', data):
        found.append(('private-key-material', 0))
    # Private source roots must be absent even outside ordinary data sections.
    long_terms = checker['PolicyTerms']([term for term in terms if '/' in term or len(term) >= 8], getattr(terms, 'public_attributions', ()))
    found.extend((rule, line) for rule, line in checker['findings'](data, long_terms) if rule == 'private-policy')
    return found, b'\n'.join(re.findall(rb'[\x20-\x7e]{4,}', text))


def notice_allowlist(root):
    filename = os.environ.get('RUWAC_PRIVATE_NOTICE_ALLOWLIST')
    if not filename:
        return {}
    path = Path(filename).resolve(strict=True)
    if path.is_relative_to(root.resolve()) or path.stat().st_uid != os.geteuid() or path.stat().st_mode & 0o077 or not stat.S_ISREG(path.stat().st_mode) or path.stat().st_size > 8192:
        raise ValueError('Notice review policy must be private and outside source')
    data = json.loads(path.read_text())
    if data.get('schema') != 1 or not isinstance(data.get('documents'), dict) or len(data['documents']) > 16:
        raise ValueError('Invalid private notice review policy')
    for name, value in data['documents'].items():
        if not name.startswith('notices/') or '..' in Path(name).parts or not re.fullmatch(r'[0-9a-f]{64}', value):
            raise ValueError('Invalid reviewed notice hash')
    return data['documents']


def text_scan(root, destination, checker, terms):
    reviewed = notice_allowlist(root)
    for path in destination.rglob('*'):
        if not path.is_file():
            continue
        relative = str(path.relative_to(destination))
        if relative in {'target/release/ruwac', 'target/release/ruwac-desktop'}:
            continue
        if path.is_symlink() or checker['findings'](relative, terms):
            raise ValueError('Bundle path failed privacy rules')
        found = checker['findings'](path.read_bytes(), terms)
        if found and reviewed.get(relative) == digest(path):
            # Only an externally reviewed exact upstream legal document may
            # retain a coincidentally matching public copyright name. All other
            # rules and content changes retain the ordinary rejection.
            found = [(rule, line) for rule, line in found if rule != 'private-policy']
        if found:
            raise ValueError('Bundle text failed privacy rules')


def reviewed_diagnostic(finding, directory):
    if finding.get('RuleID') != 'generic-api-key' or finding.get('StartLine') != finding.get('EndLine'):
        return False
    path = Path(finding['File']).resolve()
    if not path.is_relative_to(directory.resolve()) or path.name != 'ruwac-desktop.txt':
        return False
    lines = path.read_bytes().splitlines()
    number = finding['StartLine']
    return isinstance(number, int) and 0 < number <= len(lines) and hashlib.sha256(lines[number - 1]).hexdigest() in REVIEWED_BINARY_DIAGNOSTICS


def scan(root, destination, require_private=False, artifacts=("ruwac", "ruwac-desktop")):
    checker = runpy.run_path(str(root / 'scripts/privacy-check.py'))
    terms = checker['private_terms'](root, required=require_private)
    text_scan(root, destination, checker, terms)
    scanner = os.environ.get('RUWAC_GITLEAKS')
    if not scanner:
        raise ValueError('A checksum-verified secret scanner is required for bundle assembly')
    # Raw matches are never placed in the bundle or echoed by this helper.
    with tempfile.TemporaryDirectory(prefix='.binary-scan-', dir=destination.parent) as temporary:
        for name in artifacts:
            found, strings = artifact_findings(destination / 'target/release' / name, checker, terms)
            if found:
                raise ValueError('Artifact privacy scan failed; inspect privately')
            (Path(temporary) / (name + '.txt')).write_bytes(strings)
        for index, source in enumerate([destination, Path(temporary)]):
            # Keep reports outside both the archive and scanner input tree.
            descriptor, report = tempfile.mkstemp(prefix='.scan-report-', dir=destination.parent)
            os.close(descriptor)
            try:
                result = subprocess.run([scanner, 'dir', str(source), '--config', str(root / 'scripts/gitleaks.toml'), '--no-banner', '--redact', '--report-format', 'json', '--report-path', report], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                if result.returncode:
                    findings = json.loads(Path(report).read_text()) if result.returncode == 1 and index == 1 else []
                    if not findings or not all(reviewed_diagnostic(finding, source) for finding in findings):
                        raise ValueError('Bundle secret scan failed; inspect privately')
            finally:
                Path(report).unlink(missing_ok=True)


def archive(directory, output, epoch):
    # Normalized headers provide deterministic archive assembly for fixed inputs.
    if output.exists() or output.is_symlink():
        raise FileExistsError('Archive already exists; preserve it')
    descriptor, temporary = tempfile.mkstemp(prefix='.archive-', dir=output.parent)
    try:
        with os.fdopen(descriptor, 'wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0) as compressed, tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as stream:
            for path in [directory] + sorted(directory.rglob('*')):
                if path.is_symlink() or not (path.is_dir() or path.is_file()):
                    raise ValueError('Bundle contains unsupported filesystem entries')
                info = stream.gettarinfo(str(path), arcname=str(path.relative_to(directory.parent)))
                info.uid = info.gid = 0
                info.uname = info.gname = ''
                info.mtime = epoch
                info.mode = 0o755 if path.is_dir() or path.name in {'ruwac', 'ruwac-desktop'} else 0o644
                if path.is_file():
                    with path.open('rb') as source:
                        stream.addfile(info, source)
                else:
                    stream.addfile(info)
        os.link(temporary, output)
    finally:
        Path(temporary).unlink(missing_ok=True)


def assemble(root, output, require_private=False):
    root = root.resolve()
    version = tomllib.loads((root / 'ruwac/Cargo.toml').read_text())['workspace']['package']['version']
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.[0-9]\.([0-9]|[1-9][0-9])', version):
        raise ValueError('Invalid controller-owned version')
    os_release = Path('/etc/os-release').read_text()
    if 'ID=fedora\n' not in os_release or 'VERSION_ID=44\n' not in os_release or platform.machine() != 'x86_64':
        raise ValueError('This candidate bundle supports Fedora 44 x86_64 only')
    commit = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Invalid source commit')
    if subprocess.check_output(['git', '-C', str(root), 'diff', '--name-only'], text=True).strip() or subprocess.check_output(['git', '-C', str(root), 'diff', '--cached', '--name-only'], text=True).strip():
        raise ValueError('Commit the reviewed source through the release workflow before assembling a bundle')
    tracked = set(subprocess.check_output(['git', '-C', str(root), 'ls-files', '-z'], text=True).split('\0'))
    inventory = runpy.run_path(str(root / 'scripts/public-files.py'))['files'](root)
    if any(str(path) not in tracked for path in inventory):
        raise ValueError('Uncommitted public source cannot be attributed to the release tag')
    tag = subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'v' + version + '^{commit}'], text=True).strip()
    if tag != commit:
        raise ValueError('Bundle source must match its existing release tag')
    epoch = int(subprocess.check_output(['git', '-C', str(root), 'show', '-s', '--format=%ct', commit], text=True))
    output.mkdir(parents=True, exist_ok=True)
    if output.is_symlink():
        raise ValueError('Unsafe bundle output directory')
    name = 'ruwac-' + version + '-fedora44-x86_64-testing'
    filename = output / (name + '.tar.gz')
    checksum_file = filename.with_suffix(filename.suffix + '.sha256')
    if filename.exists() or filename.is_symlink() or checksum_file.exists() or checksum_file.is_symlink():
        raise FileExistsError('Bundle output already exists; preserve it')
    with tempfile.TemporaryDirectory(prefix='.bundle-', dir=output) as temporary:
        destination = Path(temporary) / name
        destination.mkdir()
        artifacts = {}
        for artifact in ['ruwac', 'ruwac-desktop']:
            binary = root / 'target/release' / artifact
            if binary.is_symlink() or not binary.is_file():
                raise ValueError('Missing built artifact')
            if not subprocess.check_output([str(binary), '--version'], text=True).strip().endswith(' ' + version):
                raise ValueError('Artifact version differs from its released source')
            target = destination / 'target/release' / artifact
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(binary, target)
            target.chmod(0o755)
            artifacts[artifact] = digest(target)
        (destination / 'ruwac').mkdir()
        (destination / 'ruwac/Cargo.toml').write_text('[workspace.package]\nversion = "' + version + '"\n')
        packaging = destination / 'ruwac/packaging'
        packaging.mkdir(parents=True)
        for name in ['ruwac.1', 'si.z4.ruwac.svg']:
            shutil.copyfile(root / 'ruwac/packaging' / name, packaging / name)
        shutil.copyfile(root / 'scripts/install-user.py', destination / 'install-user.py')
        shutil.copyfile(root / 'LICENSE', destination / 'LICENSE')
        (destination / 'README.txt').write_text('''RUWAC Fedora 44 x86_64 testing candidate

Install runtime prerequisites:
  sudo dnf install gh gtk4 libadwaita webkitgtk6.0 openssl-libs libssh2 sqlite-libs python3 python3-pillow tesseract tesseract-langpack-eng
Inside this extracted directory:
  python3 install-user.py --build-root . --desktop
  "$HOME/.local/bin/ruwac-desktop"

No root privileges are required for the per-user installation itself. Existing
credentials, profiles, terminal daemons and running windows are preserved.
Notices are retained with each immutable installed release.

This archive is a testing candidate, not a signed RPM or security-qualified
distribution. RUSTSEC-2023-0071 remains unresolved. Compare its SHA256 with the
trusted delivery reference before extraction; the internal manifest checks
integrity and does not establish a cryptographic publisher signature.

Source-build updates remain the default. To use checked Fedora testing bundles:
  "$HOME/.local/bin/ruwac" update --testing-bundles on
To opt into six-hour automatic installation:
  "$HOME/.local/bin/ruwac" update --auto-install on
Use --testing-bundles off for source builds (development dependencies required),
or --auto-install off to stop unattended installation. Running windows and
terminal daemons are preserved; restart the frontend when ready.
''')
        rows = notices(root, destination / 'notices', runtime_packages(root))
        document_hashes = {str(path.relative_to(destination)): digest(path) for path in destination.rglob('*') if path.is_file()}
        manifest = {'schema': 1, 'kind': 'fedora-testing-bundle', 'version': version, 'source_commit': commit,
                    'distribution': 'fedora', 'distribution_version': '44', 'architecture': 'x86_64',
                    'security_qualified': False, 'known_advisories': ['RUSTSEC-2023-0071'],
                    'artifacts': artifacts, 'files': document_hashes, 'dependency_packages': len(rows)}
        (destination / 'bundle.json').write_text(json.dumps(manifest, indent=2) + '\n')
        scan(root, destination, require_private)
        archive(destination, filename, epoch)
    checksum = digest(filename)
    with checksum_file.open('x') as stream:
        stream.write(checksum + '  ' + filename.name + '\n')
    return {'archive': filename.name, 'sha256': checksum, 'version': version, 'source_commit': commit,
            'dependency_packages': len(rows), 'security_qualified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-root', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--require-private', action='store_true')
    args = parser.parse_args()
    try:
        print(json.dumps(assemble(args.build_root, args.output, args.require_private), indent=2))
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError):
        print('Candidate bundle assembly failed; preserve private diagnostic evidence', file=__import__('sys').stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
