"""Check stable Homebrew identity and prevent outdated formula PR downgrades."""
from pathlib import Path
import subprocess
import sys

from auto_release import git, version
from homebrew_release import stable_source


def check(formula, baseline, installed, binary):
    source = stable_source(formula)
    previous = stable_source(baseline)
    if source is None:
        raise ValueError('stable source is missing')
    if previous and version(source[0][1:]) < version(previous[0][1:]):
        raise ValueError('stable formula would downgrade main')
    if previous and source[0] == previous[0] and source[1] != previous[1]:
        raise ValueError('stable source revision changed')
    expected = f'clisage {source[0][1:]}'
    if installed.strip() != expected or binary.strip() != expected:
        raise ValueError('stable package and binary must identify the exact release')


if __name__ == '__main__':
    try:
        root = Path.cwd()
        formula = (root / 'Formula/clisage.rb').read_text()
        check(formula, git(root, 'show', 'origin/main:Formula/clisage.rb'), sys.argv[1], sys.argv[2])
        tag, sha = stable_source(formula)
        if git(root, 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}') != sha:
            raise ValueError('source revision differs from immutable release tag')
    except (ValueError, OSError, IndexError, subprocess.SubprocessError):
        raise SystemExit('Stable Homebrew verification failed: inspect source pin, version, and main baseline') from None
