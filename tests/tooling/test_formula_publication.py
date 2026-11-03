"""Verified metadata updates reuse published bottles; behavioral changes run full CI."""
import sys
import unittest
from unittest.mock import Mock, patch

from test_ci_tools import ROOT, SourceFixture, run

sys.path.insert(0, str(ROOT / 'scripts'))


class MetadataChecks(SourceFixture, unittest.TestCase):
    def setUp(self):
        super().setUp()
        import homebrew_release
        from bottle_release import PLATFORMS, release_url
        self.module = __import__('formula_publication')
        self.base = self.sha
        self.tag = 'v0.2.0'
        run('git', 'tag', self.tag, self.base, cwd=self.source)
        self.manifest = dict(schema=1, tag=self.tag, sha=self.base,
                             root_url=release_url('alkca/clisage', self.tag), bottles={
                                 platform: dict(cellar='any_skip_relocation', sha256='b' * 64,
                                                filename=f'clisage-0.2.0.{platform}.bottle.tar.gz')
                                 for platform in PLATFORMS})
        self.before = homebrew_release.update_formula(
            (ROOT / 'Formula/clisage.rb').read_text(), 'v0.1.0', 'a' * 40)
        (self.source / 'Formula/clisage.rb').write_text(self.before)
        run('git', 'add', '.', cwd=self.source)
        run('git', 'commit', '-m', 'chore: fixture formula', cwd=self.source)
        self.base = run('git', 'rev-parse', 'HEAD', cwd=self.source).stdout.strip()
        self.after = homebrew_release.update_formula(self.before, self.tag, self.sha, self.manifest)
        self.github = Mock(repository='alkca/clisage')
        self.github.tags.return_value = {self.tag: self.sha}

    def commit(self, text, extra=False):
        (self.source / 'Formula/clisage.rb').write_text(text)
        if extra:
            (self.source / 'behavior').write_text('fixture behavior')
        run('git', 'add', '.', cwd=self.source)
        run('git', 'commit', '-m', 'chore: fixture update', cwd=self.source)

    def classify(self):
        with patch.object(self.module, 'published_manifest', return_value=self.manifest):
            return self.module.metadata_only(self.source, self.github, self.base, 'HEAD')

    def test_only_exact_published_metadata_can_reuse_bottle_validation(self):
        self.commit(self.after)
        self.assertTrue(self.classify())
        self.github.checks_passed.assert_called_with(self.sha)

    def test_behavioral_changes_and_extra_files_require_full_checks(self):
        self.commit(self.after.replace('  def install', '  # behavior\n  def install'), extra=True)
        self.assertFalse(self.classify())
        self.github.checks_passed.assert_not_called()

    def test_wrong_digest_or_missing_release_checks_cannot_skip_builds(self):
        self.commit(self.after.replace('b' * 64, 'c' * 64))
        with self.assertRaises(ValueError):
            self.classify()

    def test_unvalidated_release_cannot_skip_builds(self):
        self.commit(self.after)
        self.github.checks_passed.return_value = False
        with self.assertRaises(ValueError):
            self.classify()

    def test_unchanged_formula_runs_full_checks(self):
        self.assertFalse(self.classify())

    def test_branch_title_and_formula_release_must_match(self):
        self.commit(self.after)
        with self.assertRaises(ValueError):
            self.module.metadata_only(self.source, self.github, self.base, 'HEAD', expected_tag='v0.1.1')


class WorkflowPolicy(unittest.TestCase):
    def test_heavy_steps_are_gated_but_required_jobs_remain(self):
        for filename in ('ci.yml', 'homebrew.yml'):
            workflow = (ROOT / '.github/workflows' / filename).read_text()
            self.assertIn('scripts/formula_publication.py classify', workflow)
            self.assertIn("if: needs.classify.outputs.metadata != 'true'", workflow)
        workflow = (ROOT / '.github/workflows/formula-merge.yml').read_text()
        self.assertIn('workflow_run:', workflow)
        self.assertIn('scripts/formula_publication.py merge', workflow)
        self.assertIn('persist-credentials: false', workflow)


class ProtectedMerge(unittest.TestCase):
    def setUp(self):
        import formula_publication
        self.module = formula_publication
        self.github = Mock(repository='alkca/clisage')
        self.sha = 'a' * 40
        self.pr = dict(number=42, draft=False, user={'login': 'github-actions[bot]'},
                       title='chore(homebrew): update clisage to v0.2.0',
                       head={'sha': self.sha, 'repo': {'full_name': 'alkca/clisage'}},
                       base={'sha': 'b' * 40})
        names = sorted(set().union(*self.module.REQUIRED_JOBS.values()))
        self.checks = [dict(id=index, name=name, status='completed', conclusion='success',
                            app={'slug': 'github-actions'}) for index, name in enumerate(names)]
        self.git = patch.object(self.module, 'git', return_value='').start()
        self.metadata = patch.object(self.module, 'metadata_only', return_value=True).start()
        self.addCleanup(patch.stopall)

    def merge(self):
        self.module.merge(ROOT, self.github, 'homebrew-v0.2.0', self.sha)

    def test_merge_uses_exact_head_and_normal_protected_squash_api(self):
        self.github.api.side_effect = [[self.pr], [{'check_runs': self.checks}],
                                       [[]], [[]], [[]], {'merged': True}]
        self.merge()
        self.metadata.assert_called_once_with(ROOT, self.github, 'b' * 40, self.sha, expected_tag='v0.2.0')
        self.git.assert_called_once_with(ROOT, 'fetch', '--no-tags', 'origin', self.sha)
        self.assertEqual(self.github.api.call_args.args, ('pulls/42/merge',))
        self.assertEqual(self.github.api.call_args.kwargs, dict(method='PUT', fields={
            'sha': self.sha, 'merge_method': 'squash', 'commit_title': self.pr['title']}))

    def test_newer_failed_check_overrides_previous_success(self):
        failed = dict(self.checks[0], id=99, conclusion='failure')
        self.github.api.side_effect = [[self.pr], [{'check_runs': self.checks + [failed]}]]
        self.merge()
        self.assertEqual(self.github.api.call_count, 2)

    def test_missing_checks_and_other_apps_cannot_authorize_merge(self):
        self.checks[0]['app']['slug'] = 'fake-app'
        self.github.api.side_effect = [[self.pr], [{'check_runs': self.checks}]]
        self.merge()
        self.assertEqual(self.github.api.call_count, 2)

    def test_review_feedback_including_copilot_is_not_ignored(self):
        self.github.api.side_effect = [[self.pr], [{'check_runs': self.checks}],
                                       [[dict(body='fixture actionable review finding')]]]
        self.merge()
        self.assertEqual(self.github.api.call_count, 3)

    def test_changed_head_untrusted_author_and_behavior_are_never_merged(self):
        for field, value in [('user', {'login': 'fixture-user'}),
                             ('head', {'sha': 'c' * 40, 'repo': {'full_name': 'alkca/clisage'}})]:
            with self.subTest(field=field):
                pr = dict(self.pr, **{field: value})
                self.github.api.side_effect = [[pr]]
                with self.assertRaises(ValueError):
                    self.merge()
        self.github.api.side_effect = [[self.pr]]
        self.metadata.return_value = False
        with self.assertRaises(ValueError):
            self.merge()

    def test_retries_after_merge_are_noops_and_never_recreate_prs(self):
        self.github.api.return_value = []
        self.merge()
        self.github.api.assert_called_once()
