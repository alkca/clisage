"""Bind a CI-only formula copy to the exact checked-out source commit."""
import json
from pathlib import Path
import re
import subprocess
import sys


def prepare(source: Path, tap: Path, revision: str) -> None:
    source = source.resolve(strict=True)
    tap = tap.resolve(strict=True)
    if not re.fullmatch(r'[0-9a-f]{40}|[0-9a-f]{64}', revision):
        raise ValueError('expected a full Git commit hash')
    current = subprocess.check_output(['git', '-C', str(source), 'rev-parse', 'HEAD'], text=True).strip()
    if revision != current:
        raise ValueError('requested commit differs from checked-out HEAD')
    if source == tap or source in tap.parents or tap in source.parents:
        raise ValueError('CI tap must be separate from the source checkout')
    formula = (source / 'Formula/clisage.rb').read_text()
    # as_uri percent-encodes quotes and interpolation characters before Ruby quoting.
    replacement = (f'  head {json.dumps(source.as_uri())},\n'
                   f'       using:    :git,\n'
                   f'       revision: "{revision}"')
    formula, count = re.subn(r'^  head .+$', lambda _: replacement, formula, flags=re.MULTILINE)
    if count != 1:
        raise ValueError('expected exactly one formula head declaration')
    destination = tap / 'Formula/clisage.rb'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(formula)
    print(f'Homebrew CI source pinned to {revision}')


if __name__ == '__main__':
    try:
        prepare(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])
    except (ValueError, OSError, IndexError, subprocess.CalledProcessError) as error:
        raise SystemExit(f'Cannot prepare Homebrew CI source: {error}') from None
