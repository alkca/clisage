"""Publish validated bottles to GitHub Releases, then prepare a reviewed formula PR."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from auto_release import GitHub, git, version

# Homebrew stopped publishing Intel macOS dependency bottles in September 2026.
PLATFORMS = ('arm64_sequoia', 'arm64_linux', 'x86_64_linux')
CELLARS = ('any', 'any_skip_relocation', '/opt/homebrew/Cellar', '/usr/local/Cellar',
           '/home/linuxbrew/.linuxbrew/Cellar')


def release_url(repository, tag):
    version(tag.removeprefix('v'))
    if not tag.startswith('v') or repository != 'alkca/clisage':
        raise ValueError('unexpected release identity')
    return f'https://github.com/{repository}/releases/download/{tag}'


def marker(tag, sha):
    return f'<!-- clisage-bottles:v1 {tag} {sha} -->'


def validate_manifest(manifest, repository, tag, sha):
    if (not re.fullmatch(r'[0-9a-f]{40}', sha) or manifest.get('schema') != 1
            or manifest.get('tag') != tag or manifest.get('sha') != sha
            or manifest.get('root_url') != release_url(repository, tag)
            or set(manifest.get('bottles', {})) != set(PLATFORMS)):
        raise ValueError('invalid bottle manifest identity')
    for platform, item in manifest['bottles'].items():
        expected = f'clisage-{tag[1:]}.{platform}.bottle.tar.gz'
        if (item.get('filename') != expected or item.get('cellar') not in CELLARS
                or not re.fullmatch('[0-9a-f]{64}', item.get('sha256', ''))):
            raise ValueError('invalid bottle manifest entry')
    return manifest


def collect(directory, repository, tag, sha):
    """Use brew JSON to verify each archive and normalize public asset filenames."""
    manifest = dict(schema=1, tag=tag, sha=sha, root_url=release_url(repository, tag), bottles={})
    assets = []
    output = directory / 'release-assets'
    output.mkdir(exist_ok=True)
    for path in sorted(directory.glob('*/*.bottle.json')):
        from homebrew_release import stable_source
        if stable_source((path.parent / 'clisage.rb').read_text()) != (tag, sha):
            raise ValueError('bottle source differs from release tag')
        document = json.loads(path.read_text())
        if set(document) != {'clisage/release/clisage'}:
            raise ValueError('unexpected bottled formula')
        data = document['clisage/release/clisage']
        if data['formula']['name'] != 'clisage' or data['formula']['pkg_version'] != tag[1:]:
            raise ValueError('bottle version differs from release')
        bottle = data['bottle']
        if bottle['rebuild'] != 0 or bottle['root_url'] != manifest['root_url'] or len(bottle['tags']) != 1:
            raise ValueError('unexpected bottle configuration')
        platform, item = next(iter(bottle['tags'].items()))
        if platform not in PLATFORMS or platform in manifest['bottles']:
            raise ValueError('unexpected or duplicate bottle platform')
        filename = f'clisage-{tag[1:]}.{platform}.bottle.tar.gz'
        local = f'clisage--{tag[1:]}.{platform}.bottle.tar.gz'
        if item['filename'] != filename or item['local_filename'] != local:
            raise ValueError('unexpected bottle filename')
        archive = path.parent / local
        if not archive.is_file():
            raise ValueError('bottle archive is missing')
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        if digest != item['sha256']:
            raise ValueError('bottle checksum mismatch')
        manifest['bottles'][platform] = dict(filename=filename, sha256=digest,
                                            cellar=item.get('cellar', bottle.get('cellar')))
        bundle = path.parent / 'provenance.sigstore.json'
        json.loads(bundle.read_text())
        for source, name in [(archive, filename), (bundle, f'provenance-{platform}.sigstore.json')]:
            destination = output / name
            shutil.copyfile(source, destination)
            assets.append(destination)
    validate_manifest(manifest, repository, tag, sha)
    path = output / 'bottle-manifest.json'
    path.write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    return manifest, assets + [path]


def releases(github):
    return [item for page in github.api('releases?per_page=100', pages=True) for item in page]


def pending(root, github):
    """Recover unpublished releases without retroactively publishing older tags."""
    installed = git(root, 'log', 'HEAD', '--first-parent', '--diff-filter=A', '--format=%H',
                    '--', 'scripts/bottle_release.py').splitlines()
    if not installed:
        return []
    published = {item['tag_name'] for item in releases(github) if not item['draft']}
    result = []
    tags = [(version(tag[1:]), tag, sha) for tag, sha in github.tags().items()
            if re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', tag)]
    for _, tag, sha in sorted(tags):
        if tag in published:
            continue
        if (git(root, 'merge-base', installed[-1], sha) != installed[-1]
                or git(root, 'merge-base', sha, 'HEAD') != sha):
            continue
        if not github.checks_passed(sha):
            break
        result.append(tag)
    return result


def gh(arguments):
    result = subprocess.run(['gh', *map(str, arguments)], capture_output=True, text=True, timeout=300)
    if result.returncode:
        raise RuntimeError('GitHub release asset operation failed')


def verify(root, github, tag):
    release_url(github.repository, tag)
    sha = github.tags().get(tag)
    if not sha or not github.checks_passed(sha):
        raise ValueError('release requires successful main checks')
    if git(root, 'merge-base', sha, 'HEAD') != sha:
        raise ValueError('release must belong to main history')
    return sha


def publish(root, directory, github, tag):
    sha = verify(root, github, tag)
    _, assets = collect(directory, github.repository, tag, sha)
    matches = [item for item in releases(github) if item['tag_name'] == tag]
    if matches:
        release = matches[0]
        if marker(tag, sha) not in (release.get('body') or '') or release.get('prerelease'):
            raise ValueError('existing release is not managed by bottle automation')
    else:
        release = github.api('releases', fields=dict(tag_name=tag, target_commitish=sha, name=tag,
                                                    draft=True, prerelease=False,
                                                    body=marker(tag, sha) + '\n\nTested Homebrew bottles.'))
    existing = {asset['name']: asset for asset in release.get('assets', [])}
    missing = []
    for path in assets:
        digest = 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest()
        if path.name in existing:
            if existing[path.name].get('digest') != digest:
                raise ValueError('refusing to overwrite an existing release asset')
        else:
            missing.append(path)
    if not release['draft']:
        if missing:
            raise ValueError('published release is incomplete; manual investigation required')
        return
    if missing:
        gh(['release', 'upload', tag, '--repo', github.repository, *missing])
    # Confirm all assets arrived before making any of them publicly available.
    uploaded = github.api(f'releases/{release["id"]}')
    remote = {asset['name']: asset.get('digest') for asset in uploaded['assets']}
    if any(remote.get(path.name) != 'sha256:' + hashlib.sha256(path.read_bytes()).hexdigest() for path in assets):
        raise ValueError('release upload is incomplete or differs from tested artifacts')
    if github.tags().get(tag) != sha:
        raise ValueError('release tag changed during publication')
    github.api(f'releases/{release["id"]}', method='PATCH', fields={'draft': False})


def sync(root, github):
    from homebrew_release import publish as formula_pr
    candidates = [item for item in releases(github)
                  if not item['draft'] and not item['prerelease']
                  and re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', item['tag_name'])]
    if not candidates:
        return
    release = max(candidates, key=lambda item: version(item['tag_name'][1:]))
    tag = release['tag_name']
    sha = github.tags().get(tag)
    if not sha or marker(tag, sha) not in (release.get('body') or ''):
        raise ValueError('latest release is not managed by bottle automation')
    with tempfile.TemporaryDirectory() as temporary:
        gh(['release', 'download', tag, '--repo', github.repository, '--pattern', 'bottle-manifest.json',
            '--dir', temporary])
        manifest = validate_manifest(json.loads((Path(temporary) / 'bottle-manifest.json').read_text()),
                                     github.repository, tag, sha)
    remote = {asset['name']: asset.get('digest') for asset in release.get('assets', [])}
    if any(remote.get(item['filename']) != 'sha256:' + item['sha256'] for item in manifest['bottles'].values()):
        raise ValueError('published bottle assets differ from manifest')
    formula_pr(root, github, manifest=manifest)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('pending', 'verify', 'publish', 'sync'))
    parser.add_argument('--tag')
    parser.add_argument('--artifacts', type=Path)
    args = parser.parse_args()
    try:
        if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REF') != 'refs/heads/main':
            raise ValueError('release publication is restricted to main Actions')
        github = GitHub(os.environ['GITHUB_REPOSITORY'])
        if args.operation == 'pending':
            print(json.dumps(pending(Path.cwd(), github)))
        elif args.operation == 'verify':
            print(verify(Path.cwd(), github, args.tag))
        elif args.operation == 'publish':
            publish(Path.cwd(), args.artifacts, github, args.tag)
        else:
            sync(Path.cwd(), github)
    except (ValueError, RuntimeError, KeyError, TypeError, OSError, subprocess.SubprocessError):
        raise SystemExit('Bottle release failed; inspect main checks, tag identity, artifacts, and release consistency') from None
