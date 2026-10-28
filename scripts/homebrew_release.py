"""Synchronize the stable Homebrew source through reviewed, CI-tested PRs."""
import argparse
import base64
import os
from pathlib import Path
import re
import subprocess

from auto_release import GitHub, git, version

SOURCE = 'https://github.com/alkca/clisage.git'
STABLE_BLOCK = re.compile(
    r'^  url "https://github.com/alkca/clisage\.git",\n'
    r'      tag:      "(v[0-9]+\.[0-9]+\.[0-9]+)",\n'
    r'      revision: "([0-9a-f]{40})"\n'
    r'  version "([0-9]+\.[0-9]+\.[0-9]+)"\n', re.MULTILINE)


def stable_source(formula):
    matches = list(STABLE_BLOCK.finditer(formula))
    remainder = STABLE_BLOCK.sub('', formula)
    if len(matches) > 1 or re.search(r'^  (?:url|version)\s', remainder, re.MULTILINE):
        raise ValueError('unexpected stable formula declarations')
    if not matches:
        return None
    tag, sha, declared = matches[0].groups()
    version(tag[1:])
    if declared != tag[1:]:
        raise ValueError('formula version differs from source tag')
    return tag, sha


def strip_stable(formula):
    stable_source(formula)
    return STABLE_BLOCK.sub('', formula)


def update_formula(formula, tag, sha):
    if not tag.startswith('v'):
        raise ValueError('expected a stable release tag')
    version(tag[1:])
    if not re.fullmatch('[0-9a-f]{40}', sha):
        raise ValueError('expected an immutable source revision')
    formula = strip_stable(formula)
    if len(re.findall(r'^  head ', formula, re.MULTILINE)) != 1:
        raise ValueError('expected exactly one formula head declaration')
    block = (f'  url "{SOURCE}",\n'
             f'      tag:      "{tag}",\n'
             f'      revision: "{sha}"\n'
             f'  version "{tag[1:]}"\n')
    result, count = re.subn(r'(?=^  license )', lambda _: block, formula, flags=re.MULTILINE)
    if count != 1:
        raise ValueError('expected exactly one formula license declaration')
    return result


def metadata_sync(root, sha, tags):
    """Only a stable-source-only change to an existing release avoids tagging."""
    title = git(root, 'show', '-s', '--format=%s', sha).split(' (#')[0]
    if not title.startswith('chore(homebrew): update clisage to v'):
        return False
    if git(root, 'diff-tree', '--no-commit-id', '--name-only', '-r', sha) != 'Formula/clisage.rb':
        return False
    try:
        before = git(root, 'show', f'{sha}^:Formula/clisage.rb') + '\n'
        after = git(root, 'show', f'{sha}:Formula/clisage.rb') + '\n'
        old, new = stable_source(before), stable_source(after)
        return bool(old and new and version(new[0][1:]) > version(old[0][1:])
                    and tags.get(new[0]) == new[1]
                    and git(root, 'merge-base', new[1], f'{sha}^') == new[1]
                    and title == f'chore(homebrew): update clisage to {new[0]}'
                    and strip_stable(before) == strip_stable(after))
    except ValueError:
        return False


def publish(root, github):
    tags = github.tags()
    stable = [(version(tag[1:]), tag, sha) for tag, sha in tags.items()
              if re.fullmatch(r'v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', tag)]
    if not stable:
        return None
    _, tag, sha = max(stable)
    formula = (root / 'Formula/clisage.rb').read_text()
    current = stable_source(formula)
    if current and version(current[0][1:]) > version(tag[1:]):
        raise ValueError('refusing to downgrade stable Homebrew')
    updated = update_formula(formula, tag, sha)
    if updated == formula:
        return None
    if current and current[0] == tag:
        raise ValueError('stable release revision changed')
    # Remote tag must be an ancestor of this trusted main checkout.
    if git(root, 'merge-base', sha, 'HEAD') != sha:
        raise ValueError('release source is outside main history')
    branch = f'homebrew-{tag}'
    owner = github.repository.split('/')[0]
    prs = github.api(f'pulls?state=all&head={owner}:{branch}&per_page=100')
    if prs and not any(pr['state'] == 'open' for pr in prs):
        print(f'Homebrew update {tag} was closed; review it manually before retrying.')
        return None
    if not prs:
        blob = github.api('git/blobs', fields={'content': base64.b64encode(updated.encode()).decode(), 'encoding': 'base64'})
        tree = github.api('git/trees', fields={'base_tree': git(root, 'rev-parse', 'HEAD^{tree}'),
                                              'tree': [{'path': 'Formula/clisage.rb', 'mode': '100644',
                                                        'type': 'blob', 'sha': blob['sha']}]})
        commit = github.api('git/commits', fields={'message': f'chore(homebrew): update clisage to {tag}',
                                                  'tree': tree['sha'], 'parents': [git(root, 'rev-parse', 'HEAD')]})
        # Never force-push; recover an interrupted branch creation only at this exact content.
        try:
            github.api('git/refs', fields={'ref': f'refs/heads/{branch}', 'sha': commit['sha']})
        except RuntimeError:
            existing = github.api(f'git/ref/heads/{branch}')['object']['sha']
            existing_tree = github.api(f'git/commits/{existing}')['tree']['sha']
            if existing_tree != tree['sha']:
                raise ValueError('existing Homebrew branch differs; manual review required') from None
        github.api('pulls', fields={'title': f'chore(homebrew): update clisage to {tag}', 'head': branch,
                                   'base': 'main', 'body': f'Update the stable Homebrew source to {tag} at `{sha}`.\n\n'
                                   'Generated from an immutable, CI-validated release tag. Normal review and required '
                                   'checks apply; this workflow does not approve or merge this PR.'})
    # GITHUB_TOKEN-created PR events do not trigger checks automatically.
    for workflow in ('ci.yml', 'homebrew.yml'):
        github.api(f'actions/workflows/{workflow}/dispatches', fields={'ref': branch})
    print(f'Homebrew update PR prepared for {tag}; merge it after review and CI.')
    return tag


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()
    try:
        if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REF') != 'refs/heads/main':
            raise ValueError('formula publication is restricted to main Actions')
        publish(Path.cwd(), GitHub(os.environ['GITHUB_REPOSITORY']))
    except (ValueError, RuntimeError, KeyError, OSError, subprocess.SubprocessError):
        raise SystemExit('Cannot prepare Homebrew PR; inspect tag consistency, branch state, and workflow permissions') from None
