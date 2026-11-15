#!/usr/bin/env python3
"""Classify a push: versioned source release or ordinary development commit.

Never allocates a version or creates a tag. A release is published only for the
tag the release controller assigned to the package version, at that exact
commit. Ordinary commits and pull-request merges on a branch are development
commits: the Source release workflow ends successfully without publishing, and
quality/privacy run in the Fedora source checks workflow as for any push.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tomllib

TAG = re.compile(r'v(0|[1-9][0-9]*)\.[0-9]\.([0-9]|[1-9][0-9])')


class ReleaseRefused(ValueError):
    """The push names a release that does not match the controller's tag."""


def git(root, *args):
    return subprocess.run(['git', *args], cwd=root, capture_output=True, text=True)


def decide(root, ref):
    root = Path(root)
    version = tomllib.loads((root / 'ruwac/Cargo.toml').read_text())['workspace']['package']['version']
    tag = 'v' + version
    if not TAG.fullmatch(tag):
        raise ReleaseRefused('Package version ' + version + ' is not a releasable tag')
    head = git(root, 'rev-parse', 'HEAD').stdout.strip()
    tagged = git(root, 'rev-parse', '--verify', '--quiet', tag + '^{commit}')
    tag_commit = tagged.stdout.strip() if tagged.returncode == 0 else None
    release = {'publish': True, 'tag': tag,
               'prerelease': int(version.split('.')[1]) % 2 == 1}
    if ref.startswith('refs/tags/'):
        pushed = ref[len('refs/tags/'):]
        if pushed != tag:
            raise ReleaseRefused('Tag ' + pushed + ' does not match package version ' + tag + '; nothing published')
        if tag_commit != head:
            raise ReleaseRefused('Tag ' + tag + ' does not point at the pushed commit; nothing published')
        return release
    if ref.startswith('refs/heads/'):
        if tag_commit == head:
            return release
        return {'publish': False, 'tag': tag,
                'reason': 'Development commit: ' + tag + ' is not tagged at this commit; '
                          'no source release is published. Quality and privacy run in Fedora source checks.'}
    raise ReleaseRefused('Unsupported ref ' + ref)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ref', required=True)
    parser.add_argument('--root', type=Path, default=Path('.'))
    parser.add_argument('--env', type=Path, help='GITHUB_ENV file to append release variables to')
    parser.add_argument('--output', type=Path, help='GITHUB_OUTPUT file to append the decision to')
    args = parser.parse_args()
    try:
        result = decide(args.root, args.ref)
    except ReleaseRefused as error:
        parser.exit(1, str(error) + '\n')
    publish = 'true' if result['publish'] else 'false'
    if args.output:
        with args.output.open('a') as stream:
            stream.write('publish=' + publish + '\n')
    if result['publish']:
        if args.env:
            with args.env.open('a') as stream:
                stream.write('RELEASE_TAG=' + result['tag'] + '\n')
                stream.write('RELEASE_PRERELEASE=' + str(result['prerelease']).lower() + '\n')
        print('Versioned release ' + result['tag'] + ' at this commit; publication checks follow')
    else:
        print(result['reason'])


if __name__ == '__main__':
    main()
