"""Stable formula updates use immutable tags and never publish or merge releases."""
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from test_ci_tools import ROOT

sys.path.insert(0, str(ROOT / 'scripts'))


class HomebrewRelease(unittest.TestCase):
    def setUp(self):
        import homebrew_release
        self.module = homebrew_release
        self.formula = self.module.update_formula((ROOT / 'Formula/clisage.rb').read_text(), 'v0.1.0',
                                                  '61d11fed414a5efeb638fe698ca3a5f9f4305620')
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / 'Formula').mkdir()
        (self.root / 'Formula/clisage.rb').write_text(self.formula)
        git = patch.object(self.module, 'git', return_value='a' * 40)
        git.start()
        self.addCleanup(git.stop)
        self.sha = 'a' * 40

    def test_upsert_is_idempotent_and_keeps_head_and_formula_body(self):
        updated = self.module.update_formula(self.formula, 'v0.2.3', self.sha)
        self.assertEqual(updated.count('  url '), 1)
        self.assertEqual(updated.count('  version '), 1)
        self.assertIn('tag:      "v0.2.3"', updated)
        self.assertIn(f'revision: "{self.sha}"', updated)
        self.assertIn('  version "0.2.3"', updated)
        self.assertIn('  head "https://github.com/alkca/clisage.git", branch: "main"', updated)
        self.assertEqual(self.module.update_formula(updated, 'v0.2.3', self.sha), updated)
        self.assertEqual(self.module.strip_stable(updated), self.module.strip_stable(self.formula))

    def test_invalid_tags_revisions_and_ambiguous_sources_fail(self):
        for tag, sha in [('v01.2.3', self.sha), ('v1.2.3-rc.1', self.sha), ('v1.2.3', 'bad')]:
            with self.assertRaises(ValueError):
                self.module.update_formula(self.formula, tag, sha)
        with self.assertRaises(ValueError):
            self.module.update_formula(self.formula + '  url "https://example.invalid/source"\n', 'v1.2.3', self.sha)

    def test_current_formula_is_stable_and_pinned(self):
        stable = self.module.stable_source((ROOT / 'Formula/clisage.rb').read_text())
        self.assertIsNotNone(stable)
        self.assertGreaterEqual(self.module.version(stable[0][1:]), (0, 1, 0))
        self.assertRegex(stable[1], r'^[a-f0-9]{40}$')

    def test_publisher_noops_when_current_and_never_downgrades(self):
        github = Mock()
        github.tags.return_value = {'v0.1.0': '61d11fed414a5efeb638fe698ca3a5f9f4305620'}
        self.assertIsNone(self.module.publish(self.root, github))
        github.api.assert_not_called()
        github.tags.return_value = {'v0.0.9': self.sha}
        with self.assertRaises(ValueError):
            self.module.publish(self.root, github)
        github.api.assert_not_called()

    def test_publisher_creates_one_formula_pr_and_dispatches_required_checks(self):
        github = Mock(repository='alkca/clisage')
        github.tags.return_value = {'v0.2.3': self.sha}
        github.api.side_effect = [[], {'sha': 'b' * 40}, {'sha': 'c' * 40}, {'sha': 'd' * 40}, {}, {'number': 42}, {}, {}]
        self.module.publish(self.root, github)
        routes = [call.args[0] for call in github.api.call_args_list]
        self.assertEqual(routes, ['pulls?state=all&head=alkca:homebrew-v0.2.3&per_page=100',
                                 'git/blobs', 'git/trees', 'git/commits', 'git/refs', 'pulls',
                                 'actions/workflows/ci.yml/dispatches', 'actions/workflows/homebrew.yml/dispatches'])
        commit = github.api.call_args_list[3].kwargs['fields']
        self.assertEqual(commit['message'], 'chore(homebrew): update clisage to v0.2.3')
        self.assertEqual(github.api.call_args_list[5].kwargs['fields']['base'], 'main')

    def test_existing_open_pr_retries_checks_without_rewriting_branch(self):
        github = Mock(repository='alkca/clisage')
        github.tags.return_value = {'v0.2.3': self.sha}
        github.api.side_effect = [[{'number': 42, 'state': 'open'}], {}, {}]
        self.module.publish(self.root, github)
        self.assertEqual(github.api.call_count, 3)
        self.assertFalse(any('git/' in call.args[0] for call in github.api.call_args_list))

    def test_homebrew_ci_keeps_exact_head_and_checks_stable_on_both_platforms(self):
        workflow = (ROOT / '.github/workflows/homebrew.yml').read_text()
        self.assertIn('os: [ubuntu-latest, macos-latest]', workflow)
        self.assertIn('brew install --HEAD --yes clisage/ci/clisage', workflow)
        self.assertIn('brew install --yes clisage/ci/clisage', workflow)
        self.assertIn('brew test clisage/ci/clisage', workflow)
        self.assertIn('scripts/check-stable-formula.py', workflow)

    def test_stable_install_requires_exact_versions_and_rejects_downgrades(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('check_stable', ROOT / 'scripts/check-stable-formula.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        newer = self.module.update_formula(self.formula, 'v0.2.0', self.sha)
        module.check(newer, self.formula, 'clisage 0.2.0', 'clisage 0.2.0')
        for installed, binary in [('clisage HEAD-aaaaaaa', 'clisage 0.2.0'),
                                  ('clisage 0.2.0', 'clisage 0.2.0-dev.1+gaaaaaaa'),
                                  ('clisage 0.1.0', 'clisage 0.1.0')]:
            with self.assertRaises(ValueError):
                module.check(newer, self.formula, installed, binary)
        with self.assertRaises(ValueError):
            module.check(self.formula, newer, 'clisage 0.1.0', 'clisage 0.1.0')

    def test_closed_updates_and_conflicting_existing_branches_are_preserved(self):
        github = Mock(repository='alkca/clisage')
        github.tags.return_value = {'v0.2.3': self.sha}
        github.api.return_value = [{'state': 'closed', 'number': 42}]
        self.assertIsNone(self.module.publish(self.root, github))
        self.assertEqual(github.api.call_count, 1)
        github.reset_mock()
        github.api.side_effect = [[], {'sha': 'b' * 40}, {'sha': 'c' * 40}, {'sha': 'd' * 40},
                                  RuntimeError('fixed API failure'), {'object': {'sha': 'e' * 40}},
                                  {'tree': {'sha': 'f' * 40}}]
        with self.assertRaises(ValueError):
            self.module.publish(self.root, github)
        self.assertFalse(any(call.args[0] == 'pulls' for call in github.api.call_args_list))

    def test_existing_tag_cannot_change_revision(self):
        github = Mock()
        github.tags.return_value = {'v0.1.0': self.sha}
        with self.assertRaises(ValueError):
            self.module.publish(self.root, github)
        github.api.assert_not_called()
