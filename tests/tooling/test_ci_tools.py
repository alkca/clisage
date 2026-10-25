import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
GIT_ENV = {key: value for key, value in os.environ.items()
           if not key.startswith('GIT_CONFIG') and key not in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE')}
GIT_ENV.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_SYSTEM=os.devnull, GIT_CONFIG_NOSYSTEM='1')


def run(*args, cwd=None):
    return subprocess.run(args, cwd=cwd, text=True, capture_output=True, check=True, env=GIT_ENV)


class Titles(unittest.TestCase):
    def test_semantic_pr_titles(self):
        for title in ['feat: add profiles', 'fix(auth): redact errors', 'feat!: change CLI',
                      'refactor(shell)!: change widgets', 'chore: release v0.2.0']:
            with self.subTest(title=title):
                run('bash', str(ROOT / 'scripts/check-pr-title.sh'), title)
        for title in ['Fix a bug', 'feat: ', 'feat(): bad scope', 'feat: a\nfix: b', 'unknown: thing']:
            with self.subTest(title=title):
                result = subprocess.run(['bash', str(ROOT / 'scripts/check-pr-title.sh'), title], capture_output=True)
                self.assertNotEqual(result.returncode, 0)


class BrewSource(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.source = Path(self.tmp.name) / 'source with spaces'
        self.source.mkdir()
        self.tap = Path(self.tmp.name) / 'tap'
        self.tap.mkdir()
        run('git', 'init', '--initial-branch=main', cwd=self.source)
        run('git', 'config', 'user.name', 'Test Fixture', cwd=self.source)
        run('git', 'config', 'user.email', 'fixture@example.invalid', cwd=self.source)
        run('git', 'config', 'commit.gpgsign', 'false', cwd=self.source)
        (self.source / 'Formula').mkdir()
        self.original = 'class Clisage < Formula\n  head "https://invalid.example/source.git", branch: "wrong"\nend\n'
        (self.source / 'Formula/clisage.rb').write_text(self.original)
        run('git', 'add', '.', cwd=self.source)
        run('git', 'commit', '-m', 'chore: fixture', cwd=self.source)
        self.sha = run('git', 'rev-parse', 'HEAD', cwd=self.source).stdout.strip()

    def prepare(self, sha):
        return subprocess.run([sys.executable,
            str(ROOT / 'scripts/prepare_homebrew_ci.py'), str(self.source), str(self.tap), sha],
            text=True, capture_output=True, env=GIT_ENV)

    def test_formula_is_bound_to_checked_out_commit_and_source_is_unchanged(self):
        result = self.prepare(self.sha)
        self.assertEqual(result.returncode, 0, result.stderr)
        generated = (self.tap / 'Formula/clisage.rb').read_text()
        self.assertIn(self.source.as_uri(), generated)
        self.assertIn(f'revision: "{self.sha}"', generated)
        self.assertNotIn('invalid.example', generated)
        self.assertEqual((self.source / 'Formula/clisage.rb').read_text(), self.original)
        self.assertEqual(run('git', 'status', '--porcelain', cwd=self.source).stdout, '')

    def test_wrong_revision_cannot_mutate_tap(self):
        for sha in ['0' * 40, 'not-a-revision']:
            with self.subTest(sha=sha):
                self.assertNotEqual(self.prepare(sha).returncode, 0)
                self.assertFalse((self.tap / 'Formula').exists())

    def test_detached_pr_merge_revision_is_supported(self):
        (self.source / 'change').write_text('test change')
        run('git', 'add', '.', cwd=self.source)
        run('git', 'commit', '-m', 'feat: fixture update', cwd=self.source)
        run('git', 'checkout', '--detach', self.sha, cwd=self.source)
        self.assertEqual(self.prepare(self.sha).returncode, 0)
        generated = (self.tap / 'Formula/clisage.rb').read_text()
        self.assertIn(f'revision: "{self.sha}"', generated)


if __name__ == '__main__':
    unittest.main()
