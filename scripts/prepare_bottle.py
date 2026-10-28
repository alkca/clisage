"""Prepare a temporary stable formula for an existing, matching release tag.

This never creates a tag, edits the source formula, or publishes a bottle.
"""
import argparse
from pathlib import Path
import re
import subprocess
import tomllib

from homebrew_release import update_formula


def git(source: Path, *arguments: str) -> str:
    return subprocess.check_output(['git', '-C', str(source), *arguments], text=True, stderr=subprocess.DEVNULL).strip()


def prepare(source: Path, tap: Path, tag: str) -> None:
    source = source.resolve(strict=True)
    tap = tap.resolve(strict=True)
    if source == tap or source in tap.parents or tap in source.parents:
        raise ValueError('bottle tap must be separate from the source checkout')
    if not re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', tag):
        raise ValueError('expected an existing stable vMAJOR.MINOR.PATCH tag')
    version = tag[1:]
    manifest = tomllib.loads((source / 'Cargo.toml').read_text())['package']['version']
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', manifest):
        raise ValueError('bottles require a stable manifest floor')
    if tuple(map(int, version.split('.'))) < tuple(map(int, manifest.split('.'))):
        raise ValueError('release tag is below the manifest version floor')
    try:
        revision = git(source, 'rev-parse', '--verify', f'refs/tags/{tag}^{{commit}}')
    except subprocess.CalledProcessError:
        raise ValueError('release tag does not exist') from None
    if git(source, 'rev-parse', 'HEAD') != revision or git(source, 'status', '--porcelain'):
        raise ValueError('bottles require a clean checkout at the requested release tag')
    formula = (source / 'Formula/clisage.rb').read_text()
    formula = update_formula(formula, tag, revision)
    destination = tap / 'Formula/clisage.rb'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(formula)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('tap', type=Path)
    parser.add_argument('tag')
    args = parser.parse_args()
    try:
        prepare(args.source, args.tap, args.tag)
    except (ValueError, OSError, KeyError, subprocess.SubprocessError):
        raise SystemExit('Cannot prepare bottle formula; use a clean stable tag at or above the manifest floor and a separate tap') from None
