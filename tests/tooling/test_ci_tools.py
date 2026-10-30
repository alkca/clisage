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


class WorkflowTriggers(unittest.TestCase):
    def test_only_pr_runs_share_a_cancellable_concurrency_group(self):
        for filename in ('ci.yml', 'homebrew.yml'):
            with self.subTest(workflow=filename):
                workflow = (ROOT / '.github/workflows' / filename).read_text()
                self.assertIn(
                    "concurrency:\n"
                    "  group: ${{ github.workflow }}-${{ github.event_name }}-${{ github.event.pull_request.number || github.run_id }}\n"
                    "  cancel-in-progress: ${{ github.event_name == 'pull_request' }}\n",
                    workflow)

    def test_standard_library_python_helpers_do_not_cache_dependencies(self):
        for path in sorted((ROOT / '.github/workflows').glob('*.yml')):
            for step in path.read_text().split('      - uses: astral-sh/setup-uv@')[1:]:
                with self.subTest(workflow=path.name):
                    settings = step.split('\n      - ', 1)[0]
                    self.assertIn('\n          enable-cache: false', settings)

    def test_push_checks_only_run_on_main_and_preserve_pr_and_manual_checks(self):
        for filename in ('ci.yml', 'homebrew.yml'):
            with self.subTest(workflow=filename):
                workflow = (ROOT / '.github/workflows' / filename).read_text()
                triggers = workflow.split('\non:\n', 1)[1].split('\npermissions:', 1)[0]
                self.assertIn('\n  push:\n    branches: [main]\n', '\n' + triggers)
                self.assertIn('\n  pull_request:', triggers)
                self.assertIn('\n  workflow_dispatch:', triggers)


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


class SourceFixture:
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

class BrewSource(SourceFixture, unittest.TestCase):
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

class ReleaseDispatch(unittest.TestCase):
    def dispatch(self, list_status, branches):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary)
            fake = path / 'gh'
            fake.write_text('#!/bin/sh\nif [ "$1" = pr ]; then\n  printf "%s\\n" "$TEST_BRANCHES"\n  exit "$TEST_LIST_STATUS"\nfi\nprintf "%s\\n" "$*" >> "$TEST_CALLS"\n')
            fake.chmod(0o700)
            calls = path / 'calls'
            environment = dict(os.environ, PATH=str(path) + os.pathsep + os.environ['PATH'],
                               TEST_BRANCHES=branches, TEST_LIST_STATUS=str(list_status),
                               TEST_CALLS=str(calls), GITHUB_REPOSITORY='fixture/repository')
            result = subprocess.run(['bash', str(ROOT / 'scripts/dispatch-release-checks.sh')],
                                    env=environment, text=True, capture_output=True)
            return result.returncode, calls.read_text().splitlines() if calls.exists() else []

    def test_failed_listing_does_not_silently_pass(self):
        status, calls = self.dispatch(1, '')
        self.assertNotEqual(status, 0)
        self.assertEqual(calls, [])

    def test_dispatches_both_checks_for_each_release_branch(self):
        status, calls = self.dispatch(0, 'release-plz-fixture\nrelease-plz-second')
        self.assertEqual(status, 0)
        self.assertEqual(len(calls), 4)
        self.assertIn('--ref release-plz-fixture', calls[0])
        self.assertTrue(any('homebrew.yml' in call for call in calls))

    def test_no_release_branches_does_not_dispatch(self):
        self.assertEqual(self.dispatch(0, ''), (0, []))


class InstallSource(unittest.TestCase):
    def test_public_formula_survives_feature_branch_deletion(self):
        formula = (ROOT / 'Formula/clisage.rb').read_text()
        self.assertIn('head "https://github.com/alkca/clisage.git", branch: "main"', formula)


class BuildVersion(SourceFixture, unittest.TestCase):
    def verify(self, version):
        return subprocess.run(['bash', str(ROOT / 'scripts/check-build-version.sh'), self.sha, version],
                              cwd=self.source, text=True, capture_output=True, env=GIT_ENV)

    def test_installed_build_must_identify_the_tested_revision(self):
        self.assertEqual(self.verify(f'clisage 0.1.0-dev.1+g{self.sha[:12]}').returncode, 0)
        for version in ['clisage 0.1.0', 'clisage 0.1.0-dev.0+unknown',
                        'clisage 0.1.0-dev.1+g000000000000',
                        f'clisage 0.1.0-dev.1+g{self.sha[:12]}.dirty',
                        f'clisage 0.1.0-dev.1+g{self.sha[:12]}.shallow']:
            with self.subTest(version=version):
                self.assertNotEqual(self.verify(version).returncode, 0)

    def test_clean_release_version_requires_a_matching_tag_at_the_tested_commit(self):
        run('git', 'tag', 'v0.1.0', cwd=self.source)
        self.assertEqual(self.verify('clisage 0.1.0').returncode, 0)
        self.assertNotEqual(self.verify('clisage 0.2.0').returncode, 0)


if __name__ == '__main__':
    unittest.main()
