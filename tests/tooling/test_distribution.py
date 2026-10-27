"""Validate archives using a fake CLI; never build or publish releases."""
import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class Distribution(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('package_release', ROOT / 'scripts/package_release.py')
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.binary = self.root / 'fake clisage'
        self.binary.write_text('#!/bin/sh\ncase "$1" in --version) echo "clisage 0.1.0-dev.1+g000000000000";; completions) printf "fake completions for %s\\n" "$2";; *) exit 2;; esac\n')
        self.binary.chmod(0o755)

    def test_archive_is_reproducible_and_includes_completions_licenses_and_checksum(self):
        target = 'x86_64-unknown-linux-gnu'
        first = self.module.package(self.binary, target, self.root / 'first', ROOT)
        second = self.module.package(self.binary, target, self.root / 'second', ROOT)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        digest = hashlib.sha256(first.read_bytes()).hexdigest()
        self.assertEqual(first.with_suffix(first.suffix + '.sha256').read_text(), f'{digest}  {first.name}\n')
        with tarfile.open(first) as archive:
            entries = {Path(item.name).name: item for item in archive.getmembers()}
            for name in ['clisage', 'LICENSE-MIT', 'LICENSE-APACHE', 'README.md', 'clisage.bash', '_clisage', 'clisage.fish']:
                self.assertIn(name, entries)
            self.assertEqual(entries['clisage'].mode, 0o755)
            self.assertTrue(all(item.uid == item.gid == item.mtime == 0 for item in entries.values()))
            self.assertEqual(archive.extractfile(entries['_clisage']).read(), b'fake completions for zsh\n')

    def test_unknown_target_and_invalid_version_do_not_produce_an_archive(self):
        destination = self.root / 'out'
        with self.assertRaises(ValueError):
            self.module.package(self.binary, '../bad-target', destination, ROOT)
        self.binary.write_text('#!/bin/sh\necho "clisage ../bad-version"\n')
        with self.assertRaises(ValueError):
            self.module.package(self.binary, 'aarch64-apple-darwin', destination, ROOT)
        self.assertFalse(destination.exists())


class BottleSource(unittest.TestCase):
    def test_stable_formula_requires_a_clean_matching_release_tag(self):
        spec = importlib.util.spec_from_file_location('prepare_bottle', ROOT / 'scripts/prepare_bottle.py')
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        from test_ci_tools import run
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'source'
            tap = root / 'tap'
            source.mkdir()
            tap.mkdir()
            (source / 'Formula').mkdir()
            (source / 'Formula/clisage.rb').write_text((ROOT / 'Formula/clisage.rb').read_text())
            (source / 'Cargo.toml').write_text('[package]\nname="clisage"\nversion="0.1.0"\n')
            run('git', 'init', '--initial-branch=main', cwd=source)
            run('git', 'config', 'user.name', 'Test Fixture', cwd=source)
            run('git', 'config', 'user.email', 'fixture@example.invalid', cwd=source)
            run('git', 'config', 'commit.gpgsign', 'false', cwd=source)
            run('git', 'add', '.', cwd=source)
            run('git', 'commit', '-m', 'chore: fixture release', cwd=source)
            sha = run('git', 'rev-parse', 'HEAD', cwd=source).stdout.strip()
            for tag in ['v0.1.0', 'v../bad']:
                with self.assertRaises(ValueError):
                    module.prepare(source, tap, tag)
            run('git', 'tag', 'v0.1.0', cwd=source)
            module.prepare(source, tap, 'v0.1.0')
            formula = (tap / 'Formula/clisage.rb').read_text()
            self.assertIn(f'revision: "{sha}"', formula)
            self.assertRegex(formula, r'tag:\s+"v0\.1\.0"')
            self.assertIn('version "0.1.0"', formula)
            self.assertNotIn(source.as_uri(), formula)
            self.assertEqual(run('git', 'status', '--porcelain', cwd=source).stdout, '')
            run('git', 'commit', '--allow-empty', '-m', 'feat: fixture automatic release', cwd=source)
            run('git', 'tag', 'v0.2.0', cwd=source)
            module.prepare(source, tap, 'v0.2.0')
            self.assertIn('version "0.2.0"', (tap / 'Formula/clisage.rb').read_text())
            (source / 'Cargo.toml').write_text('[package]\nversion="0.2.0"\n')
            with self.assertRaises(ValueError):
                module.prepare(source, tap, 'v0.1.0')
