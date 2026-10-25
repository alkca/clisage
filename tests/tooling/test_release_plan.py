"""Exercise the actual release planner in disposable repositories; never publish."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

from test_ci_tools import GIT_ENV, ROOT, run


class ReleasePlan(unittest.TestCase):
    def test_semver_changelog_and_lockfile_updates_without_releases(self):
        planner = os.environ.get('RELEASE_PLZ') or shutil.which('release-plz')
        self.assertIsNotNone(planner, 'install release-plz to verify release planning')
        for initial, message, expected in [
            ('0.1.0', 'fix: repair fixture output', '0.1.1'),
            ('0.1.0', 'feat: add fixture output', '0.2.0'),
            ('0.1.0', 'feat!: change fixture interface', '0.2.0'),
            ('1.2.3', 'feat: add fixture output', '1.3.0'),
            ('1.2.3', 'feat!: change fixture interface', '2.0.0'),
        ]:
            with self.subTest(initial=initial, message=message), tempfile.TemporaryDirectory() as temporary:
                path = Path(temporary)
                (path / 'src').mkdir()
                (path / 'Cargo.toml').write_text(f'[package]\nname="release-fixture"\nversion="{initial}"\nedition="2024"\npublish=false\n')
                (path / 'src/main.rs').write_text('fn main() { println!("fixture"); }\n')
                shutil.copy(ROOT / 'release-plz.toml', path / 'release-plz.toml')
                run('cargo', 'generate-lockfile', '--offline', cwd=path)
                run('git', 'init', '--initial-branch=main', cwd=path)
                for key, value in [('user.name', 'Test Fixture'), ('user.email', 'fixture@example.invalid'), ('commit.gpgsign', 'false')]:
                    run('git', 'config', key, value, cwd=path)
                run('git', 'add', '.', cwd=path)
                run('git', 'commit', '-m', 'chore: fixture baseline', cwd=path)
                run('git', 'tag', f'v{initial}', cwd=path)
                with (path / 'src/main.rs').open('a') as source:
                    source.write('// synthetic release fixture change\n')
                run('git', 'add', '.', cwd=path)
                run('git', 'commit', '-m', message, cwd=path)
                environment = dict(GIT_ENV, CARGO_NET_OFFLINE='true')
                for key in ['GITHUB_TOKEN', 'GH_TOKEN', 'GIT_TOKEN', 'GIT_CLIFF_CONFIG']:
                    environment.pop(key, None)
                output = subprocess.run([planner, 'update', '--repo-url', 'https://github.com/fixture/release-fixture'],
                    cwd=path, env=environment, text=True, capture_output=True, timeout=60)
                self.assertEqual(output.returncode, 0, output.stderr)
                manifest = tomllib.loads((path / 'Cargo.toml').read_text())
                lock = tomllib.loads((path / 'Cargo.lock').read_text())
                self.assertEqual(manifest['package']['version'], expected)
                self.assertEqual(lock['package'][0]['version'], expected)
                changelog = (path / 'CHANGELOG.md').read_text()
                self.assertIn(expected, changelog)
                self.assertIn('fixture', changelog)
                self.assertEqual(run('git', 'tag', '--list', cwd=path).stdout.strip(), f'v{initial}')


if __name__ == '__main__':
    unittest.main()
