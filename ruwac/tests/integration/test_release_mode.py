#!/usr/bin/env python3
"""Versioned publication vs ordinary development commits in Source release.

Failed required quality/privacy checks are covered by test_verified_quality.py,
which the publication path still runs.
"""
from pathlib import Path
import runpy
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
MODE = runpy.run_path(str(ROOT / 'scripts/release-mode.py'))
decide, Refused = MODE['decide'], MODE['ReleaseRefused']


class ReleaseMode(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.repo = Path(self.temp.name)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.email', 'generated@example.invalid')
        self.git('config', 'user.name', 'Generated')
        self.commit('0.4.56', 'RUWAC v0.4.56')
        self.git('tag', '-a', 'v0.4.56', '-m', 'RUWAC v0.4.56')

    def tearDown(self):
        self.temp.cleanup()

    def git(self, *args):
        subprocess.run(['git', *args], cwd=self.repo, check=True, capture_output=True)

    def commit(self, version, message):
        cargo = self.repo / 'ruwac/Cargo.toml'
        cargo.parent.mkdir(exist_ok=True)
        cargo.write_text('[workspace.package]\nversion = "' + version + '"\n')
        (self.repo / 'change.txt').write_text(message)
        self.git('add', '-A')
        self.git('commit', '-q', '-m', message)

    def test_tagged_release_publishes_from_branch_and_tag_pushes(self):
        for ref in ['refs/heads/main', 'refs/tags/v0.4.56']:
            result = decide(self.repo, ref)
            self.assertEqual((result['publish'], result['tag'], result['prerelease']), (True, 'v0.4.56', False))

    def test_odd_minor_release_is_a_prerelease(self):
        self.commit('0.5.1', 'RUWAC v0.5.1')
        self.git('tag', '-a', 'v0.5.1', '-m', 'RUWAC v0.5.1')
        self.assertTrue(decide(self.repo, 'refs/tags/v0.5.1')['prerelease'])

    def test_untagged_development_commit_publishes_nothing(self):
        self.commit('0.4.56', 'ci: development change after the release')
        result = decide(self.repo, 'refs/heads/main')
        self.assertFalse(result['publish'])
        self.assertIn('Development commit', result['reason'])

    def test_version_bump_before_its_tag_publishes_nothing_from_the_branch(self):
        self.commit('0.4.57', 'RUWAC v0.4.57')
        self.assertFalse(decide(self.repo, 'refs/heads/main')['publish'])

    def test_tag_that_does_not_match_the_package_version_is_refused(self):
        self.commit('0.4.57', 'RUWAC v0.4.57')
        self.git('tag', '-a', 'v0.4.58', '-m', 'wrong')
        with self.assertRaisesRegex(Refused, 'does not match package version'):
            decide(self.repo, 'refs/tags/v0.4.58')

    def test_release_tag_on_another_commit_is_refused(self):
        self.commit('0.4.56', 'development change, tag still on the release commit')
        with self.assertRaisesRegex(Refused, 'does not point at the pushed commit'):
            decide(self.repo, 'refs/tags/v0.4.56')

    def test_invalid_version_and_unsupported_ref_are_refused(self):
        with self.assertRaises(Refused):
            decide(self.repo, 'refs/pull/7/merge')
        self.commit('0.4.056', 'invalid')
        with self.assertRaisesRegex(Refused, 'not a releasable tag'):
            decide(self.repo, 'refs/heads/main')

    def test_every_publication_step_is_gated_on_the_decision(self):
        import yaml
        workflow = yaml.safe_load((ROOT / '.github/workflows/release.yml').read_text())
        steps = workflow['jobs']['release']['steps']
        index = next(i for i, step in enumerate(steps) if step.get('id') == 'mode')
        self.assertIn('scripts/release-mode.py', steps[index]['run'])
        later = steps[index + 1:]
        self.assertTrue(later)
        for step in later:
            self.assertEqual(step.get('if'), "steps.mode.outputs.publish == 'true'", step['name'])
        names = [step['name'] for step in later]
        self.assertIn('Require exact-commit quality and privacy before source publication', names)
        self.assertIn('Publish source release', names)
        bundle = workflow['jobs']['fedora-testing-bundle']
        self.assertIn("startsWith(github.ref, 'refs/tags/')", bundle['if'])


if __name__ == '__main__':
    unittest.main()
