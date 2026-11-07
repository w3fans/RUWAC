#!/usr/bin/env python3
"""One portable Linux CLI build; deterministic archive and native Debian package."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import runpy
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
import urllib.request

ZIG_URL = 'https://ziglang.org/download/0.16.0/zig-x86_64-linux-0.16.0.tar.xz'
ZIG_SHA = '70e49664a74374b48b51e6f3fdfbf437f6395d42509050588bd49abe52ba3d00'
VERSION = re.compile(r'(0|[1-9][0-9]*)\.[0-9]\.([0-9]|[1-9][0-9])\Z')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def source_identity(root):
    git = ['git', '-c', 'safe.directory=' + str(root), '-C', str(root)]
    if subprocess.check_output(git + ['diff', '--name-only'], text=True).strip() or subprocess.check_output(git + ['diff', '--cached', '--name-only'], text=True).strip():
        raise ValueError('Release source must be clean')
    commit = subprocess.check_output(git + ['rev-parse', 'HEAD'], text=True).strip()
    files = subprocess.check_output(git + ['ls-files', '-z']).decode().split('\0')
    return {'commit': commit, 'files': {name: digest(root / name) for name in files if name}}


def identity(version, commit):
    if not VERSION.fullmatch(version) or not re.fullmatch('[0-9a-f]{40}', commit):
        raise ValueError('Invalid controller version or source commit')


def names(version):
    identity(version, '0' * 40)
    prefix = 'ruwac-' + version + '-linux-x86_64'
    return [prefix + '.tar.gz', 'ruwac_' + version + '-1_amd64.deb', prefix + '.tests.json']


def tar_bytes(files, epoch):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w', format=tarfile.GNU_FORMAT) as archive:
        directories = set()
        for name in files:
            path = Path(name)
            if path.is_absolute() or '..' in path.parts or not name or '\n' in name:
                raise ValueError('Unsafe package path')
            directories.update(str(p) for p in path.parents if str(p) != '.')
        for name in sorted(directories, key=lambda p: (len(Path(p).parts), p)):
            item = tarfile.TarInfo(name + '/')
            item.type, item.mode, item.mtime = tarfile.DIRTYPE, 0o755, epoch
            item.uid = item.gid = 0
            item.uname = item.gname = 'root'
            archive.addfile(item)
        for name, (data, mode) in sorted(files.items()):
            path = Path(name)
            if path.is_absolute() or '..' in path.parts or not name or '\n' in name:
                raise ValueError('Unsafe package path')
            item = tarfile.TarInfo(name)
            item.size, item.mode, item.mtime = len(data), mode, epoch
            item.uid = item.gid = 0
            item.uname = item.gname = 'root'
            archive.addfile(item, io.BytesIO(data))
    return gzip.compress(output.getvalue(), compresslevel=9, mtime=0)


def deb_bytes(files, version, epoch):
    identity(version, '0' * 40)
    control = ('Package: ruwac\nVersion: ' + version + '-1\nArchitecture: amd64\n'
               'Maintainer: RUWAC maintainers <noreply@github.com>\n'
               'Section: utils\nPriority: optional\n'
               'Depends: libc6 (>= 2.35), libgcc-s1\nRecommends: openssh-client\n'
               'Homepage: https://github.com/w3fans/RUWAC\n'
               'Description: Persistent terminal workspace and remote agent CLI\n'
               ' Headless CLI; no desktop dependencies or automatic engine restart.\n').encode()
    md5 = ''.join(hashlib.md5(data, usedforsecurity=False).hexdigest() + '  ' + name + '\n'
                  for name, (data, _) in sorted(files.items())).encode()
    members = [('debian-binary', b'2.0\n'),
               ('control.tar.gz', tar_bytes({'control': (control, 0o644), 'md5sums': (md5, 0o644)}, epoch)),
               ('data.tar.gz', tar_bytes(files, epoch))]
    result = bytearray(b'!<arch>\n')
    for name, data in members:
        header = f'{name + "/":<16}{epoch:<12}{0:<6}{0:<6}{"100644":<8}{len(data):<10}`\n'.encode()
        if len(header) != 60:
            raise ValueError('Invalid Debian ar header')
        result.extend(header + data + (b'\n' if len(data) % 2 else b''))
    return bytes(result)


def immutable(path, data):
    if path.exists() or path.is_symlink():
        raise FileExistsError('Preserve existing artifact: ' + path.name)
    path.write_bytes(data)


def artifacts(output, binary, documents, version, commit, epoch):
    identity(version, commit)
    if 'LICENSE' not in documents or not documents['LICENSE']:
        raise ValueError('Missing application license')
    if set(documents) & {'bin/ruwac', 'manifest.json'}:
        raise ValueError('Document collides with application payload')
    output.mkdir(parents=True, exist_ok=True)
    archive_name, deb_name, _ = names(version)
    prefix = archive_name[:-7]
    manifest = {'schema': 1, 'version': version, 'source_commit': commit,
                'architecture': 'x86_64', 'glibc_baseline': '2.35', 'CLI_only': True,
                'binary_sha256': hashlib.sha256(binary).hexdigest(),
                'documents': {name: hashlib.sha256(data).hexdigest() for name, data in sorted(documents.items())}}
    payload = {'bin/ruwac': (binary, 0o755), 'manifest.json': ((json.dumps(manifest, indent=2) + '\n').encode(), 0o644)}
    payload.update({name: (data, 0o644) for name, data in documents.items()})
    immutable(output / archive_name, tar_bytes({prefix + '/' + name: value for name, value in payload.items()}, epoch))
    deb_payload = {'usr/bin/ruwac': payload['bin/ruwac']}
    deb_payload.update({'usr/share/doc/ruwac/' + name: value for name, value in payload.items() if name != 'bin/ruwac'})
    immutable(output / deb_name, deb_bytes(deb_payload, version, epoch))
    receipt = {**manifest, 'archive': archive_name, 'deb': deb_name,
               'artifacts': {name: digest(output / name) for name in [archive_name, deb_name]}}
    immutable(output / 'build.json', (json.dumps(receipt, indent=2) + '\n').encode())
    return receipt


def build(root, work, zig_archive):
    root, work = root.resolve(), work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    frozen = source_identity(root)
    if not zig_archive.exists():
        with urllib.request.urlopen(ZIG_URL, timeout=90) as response, zig_archive.open('xb') as out:
            shutil.copyfileobj(response, out)
    if digest(zig_archive) != ZIG_SHA:
        raise ValueError('Zig archive checksum mismatch')
    tools = work / 'tools'
    tools.mkdir(exist_ok=True)
    with tarfile.open(zig_archive) as archive:
        archive.extractall(tools, filter='data')
    zig = tools / 'zig-x86_64-linux-0.16.0/zig'
    linker = work / 'cc'
    # Only a generated, controlled work path enters this executable wrapper.
    import shlex
    linker.write_text('#!/bin/sh\nexec ' + shlex.quote(str(zig)) + ' cc -target x86_64-linux-gnu.2.35 "$@"\n')
    linker.chmod(0o700)
    env = {**os.environ, 'CARGO_TARGET_DIR': str(work / 'target'), 'CARGO_BUILD_JOBS': '2',
           'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER': str(linker),
           'CC_x86_64_unknown_linux_gnu': str(linker), 'ZIG_GLOBAL_CACHE_DIR': str(work / 'zig-cache'),
           'ZIG_LOCAL_CACHE_DIR': str(work / 'zig-local'),
           'RUSTFLAGS': '--remap-path-prefix=' + str(root) + '=/usr/src/ruwac --remap-path-prefix=' + str(work) + '=/usr/src/portable-build'}
    subprocess.run(['cargo', 'build', '--release', '--locked', '--manifest-path', str(root / 'ruwac/Cargo.toml'),
                    '-p', 'ruwac-cli', '--target', 'x86_64-unknown-linux-gnu'], env=env, check=True)
    binary = work / 'target/x86_64-unknown-linux-gnu/release/ruwac'
    subprocess.run([str(binary), '--version'], check=True)
    shutil.copy2(binary, work / 'ruwac')
    bundle = runpy.run_path(str(root / 'scripts/bundle-user.py'))
    old_env = os.environ.copy()
    try:
        os.environ.update(env)
        bundle['notices'](root, work / 'notices', bundle['runtime_packages'](root, {'ruwac-cli'}))
    finally:
        os.environ.clear()
        os.environ.update(old_env)
    if source_identity(root) != frozen:
        raise ValueError('Source changed during portable build')
    (work / 'build-source.json').write_text(json.dumps(frozen, indent=2) + '\n')


def pack(root, work, output, commit):
    root = root.resolve()
    version = tomllib.loads((root / 'ruwac/Cargo.toml').read_text())['workspace']['package']['version']
    identity(version, commit)
    git = ['git', '-c', 'safe.directory=' + str(root), '-C', str(root)]
    if subprocess.check_output(git + ['rev-parse', 'HEAD'], text=True).strip() != commit:
        raise ValueError('Source commit differs')
    if subprocess.check_output(git + ['rev-parse', 'v' + version + '^{commit}'], text=True).strip() != commit:
        raise ValueError('Missing exact controller tag')
    if json.loads((work / 'build-source.json').read_text()) != source_identity(root):
        raise ValueError('Portable build source binding differs')
    binary = work / 'ruwac'
    if subprocess.check_output([str(binary), '--version'], text=True).strip() != 'RUWAC ' + version:
        raise ValueError('Binary version differs')
    needed = subprocess.check_output(['readelf', '-d', str(binary)], text=True)
    allowed = {'libc.so.6', 'libgcc_s.so.1', 'libm.so.6', 'libpthread.so.0', 'libdl.so.2', 'librt.so.1', 'libutil.so.1', 'ld-linux-x86-64.so.2'}
    if not set(re.findall(r'NEEDED.*\[(.*?)\]', needed)) <= allowed:
        raise ValueError('Unexpected headless runtime dependency')
    documents = {'LICENSE': (root / 'LICENSE').read_bytes()}
    notices = work / 'notices'
    if not (notices / 'index.json').is_file():
        raise ValueError('Missing dependency notice closure')
    for path in sorted(notices.rglob('*')):
        if path.is_symlink():
            raise ValueError('Symlink in notices')
        if path.is_file():
            documents['notices/' + path.relative_to(notices).as_posix()] = path.read_bytes()
    bundle = runpy.run_path(str(root / 'scripts/bundle-user.py'))
    with tempfile.TemporaryDirectory(prefix='portable-scan-', dir=output.parent) as folder:
        stage = Path(folder)
        (stage / 'target/release').mkdir(parents=True)
        shutil.copy2(binary, stage / 'target/release/ruwac')
        for name, data in documents.items():
            path = stage / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        bundle['scan'](root, stage, artifacts=('ruwac',))
    epoch = int(subprocess.check_output(git + ['show', '-s', '--format=%ct', commit], text=True))
    return artifacts(output, binary.read_bytes(), documents, version, commit, epoch)


def finish(root, output, results):
    build_receipt = json.loads((output / 'build.json').read_text())
    expected = json.loads((root / 'scripts/portable-cli-targets.json').read_text())
    if len(results) != len(expected) or {v['image'] for v in results} != {v['image'] for v in expected}:
        raise ValueError('Incomplete OS matrix')
    for row in results:
        if not row['PASS'] or row['binary_sha256'] != build_receipt['binary_sha256'] or row['version'] != build_receipt['version']:
            raise ValueError('Portable runtime qualification failed')
    for name, value in build_receipt['artifacts'].items():
        if digest(output / name) != value:
            raise ValueError('Tested artifact changed')
    receipt = {**build_receipt, 'PASS': True, 'OS_tests': results, 'actual_Proxmox_host_tested': False,
               'GUI_tested': False, 'one_binary_for_all_distributions': True}
    immutable(output / names(build_receipt['version'])[2], (json.dumps(receipt, indent=2) + '\n').encode())
    for name in names(build_receipt['version']):
        immutable(output / (name + '.sha256'), (digest(output / name) + '  ' + name + '\n').encode())
    (output / 'subjects.sha256').write_text(''.join(digest(output / n) + '  ' + str((output / n).resolve()) + '\n' for n in names(build_receipt['version'])))


def verify(root, output, commit):
    version = tomllib.loads((root / 'ruwac/Cargo.toml').read_text())['workspace']['package']['version']
    identity(version, commit)
    archive_name, deb_name, receipt_name = names(version)
    receipt = json.loads((output / receipt_name).read_text())
    if not receipt['PASS'] or receipt['source_commit'] != commit or receipt['version'] != version:
        raise ValueError('Portable receipt source differs')
    if receipt['actual_Proxmox_host_tested'] is not False or receipt['GUI_tested'] is not False:
        raise ValueError('Unsupported physical/GUI acceptance claim')
    expected = json.loads((root / 'scripts/portable-cli-targets.json').read_text())
    rows = receipt['OS_tests']
    if len(rows) != 4 or {v['image'] for v in rows} != {v['image'] for v in expected}:
        raise ValueError('Incomplete portable OS qualification')
    required = ['PASS', 'deb_install_verify_remove', 'archive_exec', 'Unicode_echo',
                'reinstall_preserves_engine_and_shell', 'restore_preserves_ids_and_history_without_process_resurrection']
    for row in rows:
        if not all(row[k] is True for k in required) or row['native_TUI_attach_detach_cycles'] != 2 or row['binary_sha256'] != receipt['binary_sha256'] or row['version'] != version:
            raise ValueError('Portable runtime contract incomplete')
    if set(receipt['artifacts']) != {archive_name, deb_name}:
        raise ValueError('Unexpected portable package set')
    for name, value in receipt['artifacts'].items():
        if digest(output / name) != value:
            raise ValueError('Portable artifact checksum differs')
    for name in names(version):
        if (output / (name + '.sha256')).read_text().split() != [digest(output / name), name]:
            raise ValueError('Portable checksum companion differs')
    with tarfile.open(output / archive_name) as archive:
        binary = archive.extractfile(archive_name[:-7] + '/bin/ruwac').read()
        manifest = json.load(archive.extractfile(archive_name[:-7] + '/manifest.json'))
    if hashlib.sha256(binary).hexdigest() != receipt['binary_sha256'] or manifest['source_commit'] != commit:
        raise ValueError('Archive binary/source binding differs')
    data = subprocess.check_output(['ar', 'p', str(output / deb_name), 'data.tar.gz'])
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        binary = archive.extractfile('usr/bin/ruwac').read()
    if hashlib.sha256(binary).hexdigest() != receipt['binary_sha256']:
        raise ValueError('Debian binary differs from tested archive')
    (output / 'subjects.sha256').write_text(''.join(digest(output / n) + '  ' + str((output / n).resolve()) + '\n' for n in names(version)))
    return receipt


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('mode', choices=['build', 'pack', 'finish', 'verify'])
    p.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    p.add_argument('--work', type=Path)
    p.add_argument('--output', type=Path)
    p.add_argument('--commit')
    p.add_argument('--results', type=Path)
    p.add_argument('--zig-archive', type=Path)
    a = p.parse_args()
    if a.mode == 'build':
        if a.work is None or a.zig_archive is None:
            p.error('build requires --work and --zig-archive')
        build(a.root, a.work, a.zig_archive)
    elif a.mode == 'pack':
        if a.work is None or a.output is None or a.commit is None:
            p.error('pack requires --work, --output and --commit')
        pack(a.root, a.work, a.output, a.commit)
    elif a.mode == 'finish':
        if a.output is None or a.results is None:
            p.error('finish requires --output and --results')
        finish(a.root, a.output, json.loads(a.results.read_text()))
    else:
        if a.output is None or a.commit is None:
            p.error('verify requires --output and --commit')
        verify(a.root, a.output, a.commit)


if __name__ == '__main__':
    main()
