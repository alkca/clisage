"""Release publishing uses fake artifacts and APIs; never contact a provider."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import textwrap
import unittest
from unittest.mock import Mock, patch

from test_ci_tools import ROOT, run

sys.path.insert(0, str(ROOT / 'scripts'))


class BottleRelease(unittest.TestCase):
    def setUp(self):
        import bottle_release
        self.module = bottle_release
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tag = 'v0.2.3'
        self.sha = 'a' * 40
        self.github = Mock(repository='alkca/clisage')
        self.github.tags.return_value = {self.tag: self.sha}
        self.github.checks_passed.return_value = True
        for platform in self.module.PLATFORMS:
            directory = self.root / platform
            directory.mkdir()
            filename = f'clisage--0.2.3.{platform}.bottle.tar.gz'
            archive = directory / filename
            archive.write_bytes(f'fake bottle {platform}'.encode())
            metadata = {'clisage/release/clisage': {
                'formula': {'name': 'clisage', 'pkg_version': '0.2.3'},
                'bottle': {'rebuild': 0, 'root_url': self.module.release_url(self.github.repository, self.tag),
                           'cellar': 'any_skip_relocation', 'tags': {platform: {
                               'local_filename': filename,
                               'filename': filename.replace('--', '-'),
                               'sha256': hashlib.sha256(archive.read_bytes()).hexdigest()}}}}}
            (directory / 'clisage.bottle.json').write_text(json.dumps(metadata))
            (directory / 'provenance.sigstore.json').write_text('{}')
            import homebrew_release
            (directory / 'clisage.rb').write_text(homebrew_release.update_formula(
                (ROOT / 'Formula/clisage.rb').read_text(), self.tag, self.sha))

    def manifest(self):
        return self.module.collect(self.root, self.github.repository, self.tag, self.sha)[0]

    def test_collect_requires_all_platforms_and_matching_hashes(self):
        manifest, assets = self.module.collect(self.root, self.github.repository, self.tag, self.sha)
        self.assertEqual(set(manifest['bottles']), set(self.module.PLATFORMS))
        self.assertEqual(manifest['sha'], self.sha)
        self.assertEqual(len(assets), 7)  # three bottles, three provenance bundles, manifest
        self.assertTrue(all(path.name != 'clisage.rb' for path in assets))
        archive = next(path for path in self.root.rglob('*.tar.gz') if path.parent.name != 'release-assets')
        archive.write_bytes(b'corrupt fixture')
        with self.assertRaises(ValueError):
            self.manifest()
        archive.unlink()
        with self.assertRaises(ValueError):
            self.manifest()

    def test_rejects_metadata_injection_wrong_versions_and_duplicate_platforms(self):
        metadata = next(self.root.rglob('*.bottle.json'))
        original = metadata.read_text()
        for field, value in [('local_filename', '../escape'), ('filename', 'wrong.tar.gz'),
                             ('sha256', 'bad')]:
            data = json.loads(original)
            item = next(iter(data['clisage/release/clisage']['bottle']['tags'].values()))
            item[field] = value
            metadata.write_text(json.dumps(data))
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.manifest()
        metadata.write_text(original.replace('0.2.3', '9.9.9'))
        with self.assertRaises(ValueError):
            self.manifest()
        metadata.write_text(original)
        (metadata.parent / 'duplicate.bottle.json').write_text(original)
        with self.assertRaises(ValueError):
            self.manifest()

    def test_publish_uploads_before_exposing_release_and_reuses_identical_assets(self):
        _, assets = self.module.collect(self.root, self.github.repository, self.tag, self.sha)
        uploaded = {'assets': [{'name': path.name, 'digest': 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()}
                               for path in assets]}
        self.github.api.side_effect = [[[]], {'id': 42, 'draft': True, 'assets': []}, uploaded, {}]
        with patch.object(self.module, 'gh') as command, patch.object(self.module, 'git', return_value=self.sha):
            self.module.publish(self.root, self.root, self.github, self.tag)
        routes = [call.args[0] for call in self.github.api.call_args_list]
        self.assertEqual(routes, ['releases?per_page=100', 'releases', 'releases/42', 'releases/42'])
        creation = self.github.api.call_args_list[1].kwargs['fields']
        self.assertTrue(creation['draft'])
        self.assertEqual(creation['target_commitish'], self.sha)
        self.assertEqual(self.github.api.call_args_list[-1].kwargs['fields'], {'draft': False})
        self.assertEqual(command.call_count, 1)
        self.assertNotIn('--clobber', command.call_args.args[0])

    def test_existing_draft_conflicting_assets_or_unvalidated_tags_never_publish(self):
        self.github.api.return_value = [[{'id': 42, 'tag_name': self.tag, 'draft': True,
                                         'body': self.module.marker(self.tag, self.sha),
                                         'assets': [{'name': 'bottle-manifest.json', 'digest': 'sha256:bad'}]}]]
        with patch.object(self.module, 'gh') as command, patch.object(self.module, 'git', return_value=self.sha):
            with self.assertRaises(ValueError):
                self.module.publish(self.root, self.root, self.github, self.tag)
            command.assert_not_called()
        self.github.checks_passed.return_value = False
        self.github.reset_mock()
        with self.assertRaises(ValueError):
            self.module.publish(self.root, self.root, self.github, self.tag)
        self.github.api.assert_not_called()

    def test_existing_published_release_is_not_changed_and_incomplete_upload_stays_draft(self):
        _, assets = self.module.collect(self.root, self.github.repository, self.tag, self.sha)
        existing = {'id': 42, 'tag_name': self.tag, 'draft': False,
                    'body': self.module.marker(self.tag, self.sha),
                    'assets': [{'name': path.name, 'digest': 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()}
                               for path in assets]}
        self.github.api.return_value = [[existing]]
        with patch.object(self.module, 'gh') as command, patch.object(self.module, 'git', return_value=self.sha):
            self.module.publish(self.root, self.root, self.github, self.tag)
            command.assert_not_called()
        self.assertEqual(self.github.api.call_count, 1)
        self.github.reset_mock()
        self.github.api.side_effect = [[[]], {'id': 42, 'draft': True}, {'assets': []}]
        with patch.object(self.module, 'gh'), patch.object(self.module, 'git', return_value=self.sha):
            with self.assertRaises(ValueError):
                self.module.publish(self.root, self.root, self.github, self.tag)
        self.assertFalse(any(call.kwargs.get('method') == 'PATCH' for call in self.github.api.call_args_list))

    def test_sync_uses_published_manifest_and_creates_a_reviewed_formula_pr(self):
        manifest = self.manifest()
        assets = [{'name': item['filename'], 'digest': 'sha256:' + item['sha256']}
                  for item in manifest['bottles'].values()]
        self.github.api.return_value = [[dict(tag_name=self.tag, draft=False, prerelease=False,
                                              body=self.module.marker(self.tag, self.sha), assets=assets)]]

        def download(arguments):
            destination = Path(arguments[arguments.index('--dir') + 1])
            (destination / 'bottle-manifest.json').write_text(json.dumps(manifest))

        with patch.object(self.module, 'gh', side_effect=download), patch('homebrew_release.publish') as formula:
            self.module.sync(self.root, self.github)
        formula.assert_called_once_with(self.root, self.github, manifest=manifest)
        assets[0]['digest'] = 'sha256:' + '0' * 64
        with patch.object(self.module, 'gh', side_effect=download), patch('homebrew_release.publish') as formula:
            with self.assertRaises(ValueError):
                self.module.sync(self.root, self.github)
            formula.assert_not_called()

    def test_generated_bottle_formula_merge_is_exempt_but_behavioral_changes_are_not(self):
        import homebrew_release
        from test_auto_release import AutoRelease
        fixture = AutoRelease()
        fixture.setUp()
        self.addCleanup(fixture.doCleanups)
        formula = fixture.root / 'Formula/clisage.rb'
        formula.parent.mkdir()
        original = homebrew_release.update_formula((ROOT / 'Formula/clisage.rb').read_text(), 'v0.1.0', fixture.install)
        formula.write_text(original)
        release_sha = fixture.commit('feat: fixture bottles')
        manifest = self.manifest()
        manifest['sha'] = release_sha
        formula.write_text(homebrew_release.update_formula(original, self.tag, release_sha, manifest))
        sync = fixture.commit(f'chore(homebrew): update clisage to {self.tag}')
        tags = {self.tag: release_sha}
        self.assertTrue(homebrew_release.metadata_sync(fixture.root, sync, tags))
        formula.write_text(formula.read_text().replace('  def install', '  # fixture behavior\n  def install'))
        behavior = fixture.commit(f'chore(homebrew): update clisage to {self.tag}')
        self.assertFalse(homebrew_release.metadata_sync(fixture.root, behavior, tags))

    def test_pending_only_includes_validated_tags_since_automatic_publication_installation(self):
        run('git', 'init', '--initial-branch=main', cwd=self.root)
        for key, value in [('user.name', 'Test Fixture'), ('user.email', 'fixture@example.invalid'),
                           ('commit.gpgsign', 'false')]:
            run('git', 'config', key, value, cwd=self.root)
        (self.root / 'old.txt').write_text('fixture')
        run('git', 'add', '.', cwd=self.root)
        run('git', 'commit', '-m', 'chore: historical fixture', cwd=self.root)
        old = run('git', 'rev-parse', 'HEAD', cwd=self.root).stdout.strip()
        (self.root / 'scripts').mkdir()
        (self.root / 'scripts/bottle_release.py').write_text('fixture')
        run('git', 'add', '.', cwd=self.root)
        run('git', 'commit', '-m', 'feat: automatic fixture release', cwd=self.root)
        current = run('git', 'rev-parse', 'HEAD', cwd=self.root).stdout.strip()
        self.github.tags.return_value = {'v0.2.2': old, 'v0.2.3': current}
        self.github.api.return_value = [[]]
        self.assertEqual(self.module.pending(self.root, self.github), ['v0.2.3'])
        self.github.api.return_value = [[{'tag_name': 'v0.2.3', 'draft': False, 'prerelease': False}]]
        self.assertEqual(self.module.pending(self.root, self.github), [])
        self.github.api.return_value = [[]]
        self.github.checks_passed.return_value = False
        self.assertEqual(self.module.pending(self.root, self.github), [])

    def test_formula_metadata_includes_bottles_and_removes_stale_bottles_for_source_builds(self):
        import homebrew_release
        original = (ROOT / 'Formula/clisage.rb').read_text()
        manifest = self.manifest()
        formula = homebrew_release.update_formula(original, self.tag, self.sha, manifest)
        self.assertIn('  bottle do\n', formula)
        self.assertIn(f'    root_url "{manifest["root_url"]}"', formula)
        self.assertIn('cellar: :any_skip_relocation', formula)
        self.assertLess(formula.index('  head '), formula.index('  bottle do'))
        self.assertLess(formula.index(' arm64_sequoia:'), formula.index(' arm64_linux:'))
        digest_columns = [line.rindex('"') - 65 for line in formula.splitlines() if line.startswith('    sha256 ')]
        # All hashes, including different-length platform names, align for brew style.
        self.assertLessEqual(len(set(digest_columns)), 1)
        self.assertEqual(homebrew_release.strip_stable(formula), homebrew_release.strip_stable(original))
        source = homebrew_release.update_formula(formula, 'v0.2.4', 'b' * 40)
        self.assertNotIn('  bottle do', source)
        for key, value in [('sha', 'b' * 40), ('tag', 'v0.2.4'), ('root_url', 'https://example.invalid')]:
            bad = dict(manifest, **{key: value})
            with self.subTest(key=key), self.assertRaises(ValueError):
                homebrew_release.update_formula(original, self.tag, self.sha, bad)

    def test_automation_calls_bottle_builds_directly_and_limits_publication_to_main(self):
        workflow = (ROOT / '.github/workflows/tag-release.yml').read_text()
        self.assertIn('uses: ./.github/workflows/bottles.yml', workflow)
        self.assertIn('max-parallel: 1', workflow)
        self.assertIn('scripts/bottle_release.py pending', workflow)
        bottles = (ROOT / '.github/workflows/bottles.yml').read_text()
        self.assertIn('  workflow_call:', bottles)
        self.assertIn('needs: bottle', bottles)
        self.assertIn("github.ref == 'refs/heads/main'", bottles)
        self.assertIn('scripts/bottle_release.py publish', bottles)
        self.assertIn('needs: validate', bottles)
        self.assertIn('ref: ${{ needs.validate.outputs.sha }}', bottles)
        self.assertIn('scripts/bottle_release.py sync', workflow)
        self.assertNotIn('--clobber', bottles)

    def test_build_preflight_rejects_tags_outside_main_or_without_required_checks(self):
        with patch.object(self.module, 'git', return_value=self.sha):
            self.assertEqual(self.module.verify(self.root, self.github, self.tag), self.sha)
        self.github.checks_passed.return_value = False
        with self.assertRaises(ValueError):
            self.module.verify(self.root, self.github, self.tag)
        self.github.checks_passed.return_value = True
        with patch.object(self.module, 'git', return_value='b' * 40), self.assertRaises(ValueError):
            self.module.verify(self.root, self.github, self.tag)

    def test_bottle_artifacts_must_identify_the_tagged_source_revision(self):
        formula = next(self.root.rglob('clisage.rb'))
        formula.write_text(formula.read_text().replace(self.sha, 'b' * 40))
        with self.assertRaises(ValueError):
            self.manifest()

    def test_workflow_reinstalls_and_attests_homebrews_actual_bottle_filename(self):
        import subprocess
        workflow = (ROOT / '.github/workflows/bottles.yml').read_text()
        step = workflow.split('      - name: Build and test the bottle\n', 1)[1]
        commands = textwrap.dedent(step.split('        run: |\n', 1)[1].split('      - name:', 1)[0])
        binaries = self.root / 'bin'
        binaries.mkdir()
        brew = binaries / 'brew'
        brew.write_text('#!/bin/bash\nset -eu\ncase "$1" in\n'
                        'bottle) touch clisage--0.2.3.arm64_sequoia.bottle.tar.gz;;\n'
                        'reinstall) [[ -f "$2" ]] || { echo "bottle glob did not resolve" >&2; exit 1; };;\n'
                        'esac\n')
        brew.chmod(0o755)
        clisage = binaries / 'clisage'
        clisage.write_text('#!/bin/sh\necho "clisage 0.2.3"\n')
        clisage.chmod(0o755)
        environment = dict(os.environ, PATH=str(binaries) + os.pathsep + os.environ['PATH'],
                           RELEASE_TAG=self.tag, GITHUB_REPOSITORY='alkca/clisage')
        result = subprocess.run(['bash', '-euo', 'pipefail', '-c', commands], cwd=self.root,
                                env=environment, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        subject = workflow.split('          subject-path: ', 1)[1].splitlines()[0]
        self.assertEqual(len(list(self.root.glob(subject))), 1,
                         'provenance must attest the bottle produced by brew bottle')

    def test_release_platforms_do_not_require_unsupported_intel_macos_dependencies(self):
        self.assertEqual(set(self.module.PLATFORMS), {'arm64_sequoia', 'arm64_linux', 'x86_64_linux'})
        workflow = (ROOT / '.github/workflows/bottles.yml').read_text()
        self.assertIn('os: [ubuntu-24.04, ubuntu-24.04-arm, macos-15]', workflow)
        self.assertNotIn('macos-15-intel', workflow)
