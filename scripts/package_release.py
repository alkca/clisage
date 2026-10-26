"""Build deterministic distribution archives from an already-built native CLI.

No tags, releases, credentials, or network calls are created by this helper.
"""
import argparse
import gzip
import hashlib
import io
from pathlib import Path
import re
import subprocess
import tarfile

TARGETS = {
    'x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu',
    'x86_64-apple-darwin', 'aarch64-apple-darwin',
}


def cli_output(binary: Path, *arguments: str) -> bytes:
    result = subprocess.run([str(binary), *arguments], capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError('CLI failed while preparing distribution assets')
    return result.stdout


def package(binary: Path, target: str, destination: Path, source: Path) -> Path:
    if target not in TARGETS:
        raise ValueError('unsupported distribution target')
    binary = binary.resolve(strict=True)
    reported = cli_output(binary, '--version').decode('utf-8').strip()
    if not re.fullmatch(r'clisage [0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?', reported):
        raise ValueError('invalid binary version')
    name = f'clisage-{reported.removeprefix("clisage ")}-{target}'
    files = {'clisage': binary.read_bytes()}
    for filename in ['README.md', 'LICENSE-MIT', 'LICENSE-APACHE']:
        files[filename] = (source / filename).read_bytes()
    for shell, filename in [('bash', 'clisage.bash'), ('zsh', '_clisage'), ('fish', 'clisage.fish')]:
        files[f'completions/{filename}'] = cli_output(binary, 'completions', shell)
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode='wb', filename='', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for filename, data in sorted(files.items()):
                item = tarfile.TarInfo(f'{name}/{filename}')
                item.size = len(data)
                item.mode = 0o755 if filename == 'clisage' else 0o644
                archive.addfile(item, io.BytesIO(data))
    destination.mkdir(parents=True, exist_ok=True)
    path = destination / f'{name}.tar.gz'
    path.write_bytes(buffer.getvalue())
    digest = hashlib.sha256(buffer.getvalue()).hexdigest()
    path.with_suffix('.gz.sha256').write_text(f'{digest}  {path.name}\n')
    return path


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=Path)
    parser.add_argument('target', choices=sorted(TARGETS))
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    try:
        result = package(args.binary, args.target, args.destination, Path(__file__).resolve().parents[1])
    except (ValueError, OSError, subprocess.SubprocessError):
        raise SystemExit('Cannot package binary; check the target, binary version, permissions, and completion commands') from None
    print(result.name)
