#!/usr/bin/env python3
"""Execute the exact generated COPR shell with disposable package-manager stubs."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

source = (Path(__file__).parents[2] / 'desktop/src/agent_launch.rs').read_text()
script = re.search(r'format!\(\s*r#"(.*?)"#,', source, re.S).group(1)
script = script.replace('{{', '{').replace('}}', '}')
script = script.replace('{owner}', 'zvegelj').replace('{name}', 'ruwac').replace('{copr}', 'zvegelj/ruwac')


def case(status, repos='fedora\nupdates', marker=True, distro='fedora', sudo=0, repo_status=0):
    with tempfile.TemporaryDirectory(prefix='ruwac-copr-contract-') as tmp:
        root = Path(tmp)
        (root / 'os-release').write_text('ID=' + distro + '\n')
        if marker:
            (root / 'marker').write_text('{"kind":"distribution"}')
        bodies = {
            'dnf': f'case "$*" in *repolist*) printf "%s\\n" "{repos}"; exit {repo_status};; *check-upgrade*) echo query-diagnostic >&2; exit {status};; esac',
            'rpm': 'exit 0',
            'sudo': f'exit {sudo}',
        }
        for name, body in bodies.items():
            path = root / name
            path.write_text(f'#!/bin/sh\necho "{name} $*" >> "{root}/calls"\n{body}\n')
            path.chmod(0o700)
        generated = script.replace('/etc/os-release', str(root / 'os-release')).replace('/usr/share/ruwac/rpm-installation.json', str(root / 'marker'))
        result = subprocess.run(['bash', '-c', generated + '\ncopr_ruwac'], env={**os.environ, 'PATH': f'{root}:/usr/bin:/bin'}, capture_output=True, text=True)
        calls = (root / 'calls').read_text() if (root / 'calls').exists() else ''
        assert 'daemon.stop' not in generated and 'pkill' not in generated
        return result, calls


for status in (0, 100, 1, 2, 127):
    result, calls = case(status)
    assert result.returncode == (0 if status in (0, 100) else 2), (status, result)
    assert ('sudo dnf' in calls) == (status == 100), calls
    assert 'skip_if_unavailable=0' in calls
    assert 'query-diagnostic' in result.stderr
    if status == 100:
        assert 'ruwac ruwac-desktop' in calls
        assert '--repo=fedora --repo=updates' in calls
result, calls = case(100, 'rawhide\nthird-party')
assert result.returncode == 0 and '--repo=rawhide' in calls
assert '--repo=fedora' not in calls and '--repo=third-party' not in calls
assert case(0, 'third-party')[0].returncode == 2
assert case(0, repo_status=1)[0].returncode == 2
assert case(0, distro='ubuntu')[0].returncode == 1
assert case(100, sudo=1)[0].returncode == 2
assert case(0, marker=False)[0].returncode == 2
print('PASS: COPR exit codes, strict query, Rawhide allowlist, unsupported distribution, sudo failure, missing marker, matching desktop; disposable stubs only')
