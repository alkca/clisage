"""Exercise tagging plans and GitHub gates with local Git and fake API data."""
import importlib.util
from pathlib import Path
import tempfile
import sys
import unittest

from test_ci_tools import ROOT, run

sys.path.insert(0, str(ROOT / "scripts"))


def load():
    spec = importlib.util.spec_from_file_location('auto_release', ROOT / 'scripts/auto_release.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FakeGitHub:
    def __init__(self):
        self.refs = {}
        self.ready = {}
        self.created = []

    def tags(self):
        return dict(self.refs)

    def checks_passed(self, sha):
        return self.ready.get(sha, False)

    def create_tag(self, tag, sha):
        if tag in self.refs and self.refs[tag] != sha:
            raise ValueError('tag already belongs to another commit')
        self.refs[tag] = sha
        self.created.append((tag, sha))


class AutoRelease(unittest.TestCase):
    def setUp(self):
        self.module = load()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        run('git', 'init', '--initial-branch=main', cwd=self.root)
        run('git', 'config', 'user.name', 'Test Fixture', cwd=self.root)
        run('git', 'config', 'user.email', 'fixture@example.invalid', cwd=self.root)
        run('git', 'config', 'commit.gpgsign', 'false', cwd=self.root)
        (self.root / 'Cargo.toml').write_text('[package]\nname="clisage"\nversion="0.1.0"\npublish=false\n')
        self.historical = self.commit('chore: historical baseline')
        workflow = self.root / '.github/workflows/tag-release.yml'
        workflow.parent.mkdir(parents=True)
        workflow.write_text('fixture workflow\n')
        self.install = self.commit('feat: enable automatic tags')
        self.github = FakeGitHub()

    def commit(self, message):
        run('git', 'add', '.', cwd=self.root)
        run('git', 'commit', '--allow-empty', '-m', message, cwd=self.root)
        sha = run('git', 'rev-parse', 'HEAD', cwd=self.root).stdout.strip()
        run('git', 'update-ref', 'refs/remotes/origin/main', sha, cwd=self.root)
        return sha

    def release(self, apply=True):
        return self.module.reconcile(self.root, self.github, apply=apply)

    def test_bootstrap_tags_only_automation_installation_and_is_idempotent(self):
        self.github.ready[self.install] = True
        self.assertEqual(self.release(), [('v0.1.0', self.install)])
        self.assertEqual(self.release(), [])
        self.assertEqual(self.github.created, [('v0.1.0', self.install)])
        self.assertNotIn(self.historical, self.github.refs.values())

    def test_every_merge_is_versioned_in_order_after_its_own_checks(self):
        feature = self.commit('feat(shell): add a fixture command')
        fix = self.commit('fix: repair fixture output')
        docs = self.commit('docs: describe fixtures')
        for sha in [self.install, fix, docs]:
            self.github.ready[sha] = True
        self.assertEqual(self.release(), [('v0.1.0', self.install)])
        self.github.ready[feature] = True
        self.assertEqual(self.release(), [('v0.2.0', feature), ('v0.2.1', fix), ('v0.2.2', docs)])
        self.assertEqual(self.release(), [])

    def test_only_stable_formula_metadata_syncs_are_exempt_from_tags(self):
        import homebrew_release
        formula = self.root / 'Formula/clisage.rb'
        formula.parent.mkdir()
        original = homebrew_release.update_formula((ROOT / 'Formula/clisage.rb').read_text(), 'v0.1.0', self.install)
        formula.write_text(original)
        first = self.commit('feat: stable Homebrew installs')
        self.github.refs = {'v0.1.0': self.install, 'v0.2.0': first}
        formula.write_text(homebrew_release.update_formula(original, 'v0.2.0', first))
        sync = self.commit('chore(homebrew): update clisage to v0.2.0')
        self.assertEqual(self.module.plan(self.root, self.github.refs), [])
        fix = self.commit('fix: improve output')
        self.assertEqual(self.module.plan(self.root, self.github.refs), [('v0.2.1', fix)])
        self.github.refs['v0.2.1'] = fix
        self.assertEqual(self.module.plan(self.root, self.github.refs), [])
        formula.write_text(formula.read_text().replace('  def install', '  # changed behavior\n  def install'))
        behavioral = self.commit('chore(homebrew): update clisage to v0.2.0')
        self.assertEqual(self.module.plan(self.root, self.github.refs), [('v0.2.2', behavioral)])
        self.assertNotEqual(sync, behavioral)

    def test_introducing_a_formula_with_a_sync_title_still_receives_a_tag(self):
        import homebrew_release
        formula = self.root / 'Formula/clisage.rb'
        formula.parent.mkdir()
        formula.write_text(homebrew_release.update_formula((ROOT / 'Formula/clisage.rb').read_text(),
                                                          'v0.1.0', self.install))
        added = self.commit('chore(homebrew): update clisage to v0.1.0')
        self.github.refs = {'v0.1.0': self.install}
        self.assertEqual(self.module.plan(self.root, self.github.refs), [('v0.1.1', added)])

    def test_breaking_changes_follow_existing_pre_one_and_stable_rules(self):
        for baseline, message, expected in [
            ((0, 1, 0), 'feat!: incompatible fixture', (0, 2, 0)),
            ((1, 2, 3), 'fix!: incompatible fixture', (2, 0, 0)),
            ((1, 2, 3), 'refactor(api): change fixture\n\nBREAKING CHANGE: changed API', (2, 0, 0)),
            ((1, 2, 3), 'feat: extend fixture', (1, 3, 0)),
            ((1, 2, 3), 'ci: maintain fixture', (1, 2, 4)),
        ]:
            self.assertEqual(self.module.bump(baseline, message), expected)
        for message in ['unstructured message', 'feat: ', 'unknown: fixture']:
            with self.assertRaises(ValueError):
                self.module.bump((1, 0, 0), message)

    def test_dry_run_never_creates_tags(self):
        self.github.ready[self.install] = True
        self.assertEqual(self.release(apply=False), [('v0.1.0', self.install)])
        self.assertEqual(self.github.created, [])

    def test_unrelated_tag_collision_and_ambiguous_or_incomplete_history_fail_closed(self):
        self.github.ready[self.install] = True
        self.github.refs['v0.1.0'] = '0' * 40
        with self.assertRaises(ValueError):
            self.release()
        self.github.refs = {'v0.1.0': self.install, 'v0.2.0': self.install}
        with self.assertRaises(ValueError):
            self.release()
        latest = self.commit('fix: fixture')
        self.github.refs = {'v0.1.0': latest}
        with self.assertRaises(ValueError):
            self.release()
        self.assertEqual(self.github.created, [])

    def test_missing_checks_or_a_checkout_other_than_main_never_publish(self):
        self.assertEqual(self.release(), [])
        run('git', 'checkout', '--detach', self.historical, cwd=self.root)
        with self.assertRaises(ValueError):
            self.release()
        self.assertEqual(self.github.created, [])

    def test_a_higher_manifest_version_is_preserved(self):
        self.github.refs['v0.1.0'] = self.install
        (self.root / 'Cargo.toml').write_text('[package]\nname="clisage"\nversion="1.0.0"\n')
        sha = self.commit('chore: prepare fixture version')
        self.github.ready[sha] = True
        self.assertEqual(self.release(), [('v1.0.0', sha)])

    def test_existing_pre_automation_tags_are_a_baseline_not_a_backfill_request(self):
        self.github.refs['v0.1.0'] = self.historical
        self.github.ready[self.install] = True
        self.assertEqual(self.release(), [('v0.2.0', self.install)])

    def test_version_components_cannot_overflow_rust_semver(self):
        with self.assertRaises(ValueError):
            self.module.version('18446744073709551616.0.0')
        with self.assertRaises(ValueError):
            self.module.bump((1, 0, 18446744073709551615), 'fix: fixture')


class GitHubGates(unittest.TestCase):
    def setUp(self):
        self.module = load()

    def test_release_gates_match_the_committed_branch_protection_checks(self):
        import json
        protection = json.loads((ROOT / '.github/branch-protection.json').read_text())
        required = {item['context'] for item in protection['required_status_checks']['checks']}
        self.assertEqual(set().union(*self.module.REQUIRED_JOBS.values()), required)

    def test_only_successful_main_push_workflows_with_all_required_jobs_count(self):
        sha = 'a' * 40
        workflows = {
            '.github/workflows/ci.yml': ['check (ubuntu-latest)', 'check (macos-latest)', 'Quality', 'PR title'],
            '.github/workflows/homebrew.yml': ['install (ubuntu-latest)', 'install (macos-latest)'],
        }
        runs = []
        jobs = {}
        for index, (path, names) in enumerate(workflows.items(), 1):
            runs.append(dict(id=index, path=path, head_sha=sha, head_branch='main', event='push',
                             head_repository={'full_name': 'fixture/repo'}, status='completed', conclusion='success'))
            jobs[index] = [dict(name=name, conclusion='success', status='completed') for name in names]
        self.assertTrue(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        jobs[2][0]['conclusion'] = 'skipped'
        self.assertFalse(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        jobs[2][0]['conclusion'] = 'success'
        runs[1]['event'] = 'pull_request'
        self.assertFalse(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        runs[1]['event'] = 'push'
        runs.append(dict(runs[0], id=3, status='in_progress', conclusion=None))
        self.assertFalse(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        runs.pop()
        runs[0]['head_repository'] = {'full_name': 'untrusted/fork'}
        self.assertFalse(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))

    def test_partial_reruns_reuse_prior_successes_without_masking_new_failures(self):
        sha = 'b' * 40
        runs, jobs = [], {}
        for index, (path, names) in enumerate(self.module.REQUIRED_JOBS.items(), 1):
            runs.append(dict(id=index, path=path, head_sha=sha, head_branch='main', event='push',
                             head_repository={'full_name': 'fixture/repo'}, status='completed', conclusion='success'))
            jobs[index] = [dict(id=index * 100 + offset, run_attempt=1, name=name,
                                status='completed', conclusion='success')
                           for offset, name in enumerate(sorted(names))]
        self.assertTrue(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        retried = dict(jobs[1][0], id=900, run_attempt=2, conclusion='failure')
        jobs[1].append(retried)
        self.assertFalse(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))
        jobs[1].append(dict(retried, id=901, run_attempt=3, conclusion='success'))
        self.assertTrue(self.module.successful_checks(sha, 'fixture/repo', runs, jobs))


class GitHubAdapter(unittest.TestCase):
    def test_paginated_tags_are_combined_and_api_errors_are_redacted(self):
        from unittest.mock import patch
        from subprocess import CompletedProcess
        module = load()
        client = module.GitHub('fixture/repo')
        pages = '[[{"name":"v0.1.0","commit":{"sha":"aaaa"}}],[{"name":"v0.2.0","commit":{"sha":"bbbb"}}]]'
        with patch.object(module.subprocess, 'run', return_value=CompletedProcess([], 0, pages, '')) as call:
            self.assertEqual(client.tags(), {'v0.1.0': 'aaaa', 'v0.2.0': 'bbbb'})
            self.assertIn('--paginate', call.call_args.args[0])
            self.assertIn('--slurp', call.call_args.args[0])
        with patch.object(module.subprocess, 'run', return_value=CompletedProcess([], 1, '', 'fake-sensitive-error-body')):
            with self.assertRaises(RuntimeError) as error:
                client.tags()
            self.assertNotIn('fake-sensitive-error-body', str(error.exception))

    def test_json_posts_preserve_nested_fields_and_accept_empty_dispatch_responses(self):
        import json
        from unittest.mock import patch
        from subprocess import CompletedProcess
        module = load()
        client = module.GitHub('fixture/repo')
        fields = {'parents': ['a' * 40], 'tree': [{'path': 'Formula/clisage.rb', 'mode': '100644'}]}
        with patch.object(module.subprocess, 'run', return_value=CompletedProcess([], 0, '', '')) as call:
            self.assertIsNone(client.api('actions/workflows/ci.yml/dispatches', fields=fields))
            self.assertEqual(json.loads(call.call_args.kwargs['input']), fields)
            self.assertEqual(call.call_args.args[0][-4:], ['--method', 'POST', '--input', '-'])

    def test_creation_retries_accept_only_an_existing_identical_target(self):
        module = load()
        class Conflict(module.GitHub):
            def api(self, *args, **kwargs):
                raise RuntimeError('GitHub request failed')
            def tags(self):
                return self.refs
        client = Conflict('fixture/repo')
        client.refs = {'v0.1.0': 'a' * 40}
        client.create_tag('v0.1.0', 'a' * 40)
        with self.assertRaises(RuntimeError):
            client.create_tag('v0.1.0', 'b' * 40)

    def test_cli_apply_refuses_feature_branches_before_accessing_github(self):
        import os
        import subprocess
        environment = dict(os.environ, GITHUB_ACTIONS='true', GITHUB_REF='refs/heads/feature',
                           GITHUB_REPOSITORY='fixture/repo', PATH='')
        result = subprocess.run([sys.executable, str(ROOT / 'scripts/auto_release.py'), '--apply'],
                                env=environment, text=True, capture_output=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Release reconciliation failed', result.stderr)
        self.assertNotIn('Traceback', result.stderr)
