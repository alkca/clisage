"""Reuse validated bottle metadata and merge only generated formula updates."""
import argparse
import os
from pathlib import Path
import re
import subprocess
from urllib.parse import urlencode

from auto_release import GitHub, REQUIRED_JOBS, git, version
from bottle_release import published_manifest
from homebrew_release import stable_source, strip_stable, update_formula


def metadata_only(root, github, base, head, expected_tag=None):
    if git(root, 'diff', '--name-only', base, head) != 'Formula/clisage.rb':
        return False
    before = git(root, 'show', f'{base}:Formula/clisage.rb') + '\n'
    after = git(root, 'show', f'{head}:Formula/clisage.rb') + '\n'
    try:
        previous, current = stable_source(before), stable_source(after)
        if (not previous or not current or version(current[0][1:]) <= version(previous[0][1:])
                or strip_stable(before) != strip_stable(after)):
            return False
    except ValueError:
        return False
    tag, sha = current
    if expected_tag is not None and tag != expected_tag:
        raise ValueError('formula release differs from the generated branch and title')
    if github.tags().get(tag) != sha or git(root, 'merge-base', sha, base) != sha:
        raise ValueError('formula source is not an immutable ancestor release')
    if not github.checks_passed(sha):
        raise ValueError('formula source has not passed required main checks')
    manifest = published_manifest(github)
    if manifest is None or (manifest['tag'], manifest['sha']) != current:
        raise ValueError('formula must use the latest published bottle release')
    if update_formula(before, tag, sha, manifest) != after:
        raise ValueError('formula differs from verified published bottle metadata')
    return True


def classify(root, github, event, base, ref):
    if not base or set(base) == {'0'}:
        if event == 'workflow_dispatch' and ref != 'refs/heads/main':
            base = 'origin/main'
        else:
            return False
    return metadata_only(root, github, base, 'HEAD')


def merge(root, github, branch, sha):
    if not re.fullmatch(r'homebrew-v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', branch):
        return
    if not re.fullmatch('[a-f0-9]{40}', sha):
        raise ValueError('invalid formula commit')
    owner = github.repository.split('/')[0]
    query = urlencode(dict(state='open', head=f'{owner}:{branch}', base='main', per_page=100))
    prs = github.api('pulls?' + query)
    if not prs:
        return
    if len(prs) != 1:
        raise ValueError('ambiguous formula update')
    pr = prs[0]
    if (pr['user']['login'] != 'github-actions[bot]' or pr['draft']
            or pr['head']['sha'] != sha or pr['head']['repo']['full_name'] != github.repository
            or pr['title'] != f'chore(homebrew): update clisage to {branch.removeprefix("homebrew-")}'):
        raise ValueError('not an unchanged generated formula PR')
    # Fetch only Git objects. Never check out or execute formula-branch code with a write token.
    git(root, 'fetch', '--no-tags', 'origin', sha)
    if not metadata_only(root, github, pr['base']['sha'], sha,
                         expected_tag=branch.removeprefix('homebrew-')):
        raise ValueError('automatic merge is restricted to verified release metadata')
    pages = github.api(f'commits/{sha}/check-runs?per_page=100', pages=True)
    checks = {}
    for page in pages:
        for check in page['check_runs']:
            if check['app']['slug'] == 'github-actions':
                old = checks.get(check['name'])
                if old is None or check['id'] > old['id']:
                    checks[check['name']] = check
    required = set().union(*REQUIRED_JOBS.values())
    if any(name not in checks or checks[name]['status'] != 'completed'
           or checks[name]['conclusion'] != 'success' for name in required):
        print('Waiting for all required formula checks; the next completion will reconcile.')
        return
    # Do not discard review feedback, including automatically submitted Copilot findings.
    for route in (f'pulls/{pr["number"]}/comments', f'pulls/{pr["number"]}/reviews',
                  f'issues/{pr["number"]}/comments'):
        if any(item.get('body', '').strip() for page in github.api(route + '?per_page=100', pages=True)
               for item in page):
            print('Formula review feedback requires manual disposition before merging.')
            return
    result = github.api(f'pulls/{pr["number"]}/merge', method='PUT', fields={
        'sha': sha, 'merge_method': 'squash', 'commit_title': pr['title']})
    if not result.get('merged'):
        raise ValueError('protected formula merge was not accepted')
    print(f'Merged verified formula metadata for {branch.removeprefix("homebrew-")}.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('classify', 'merge'))
    args = parser.parse_args()
    try:
        root = Path.cwd()
        github = GitHub(os.environ['GITHUB_REPOSITORY'])
        if args.operation == 'classify':
            print(str(classify(root, github, os.environ['GITHUB_EVENT_NAME'],
                               os.environ.get('CHANGE_BASE', ''), os.environ['GITHUB_REF'])).lower())
        else:
            if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REF') != 'refs/heads/main':
                raise ValueError('formula merging requires the trusted main workflow')
            merge(root, github, os.environ['FORMULA_BRANCH'], os.environ['FORMULA_SHA'])
    except (ValueError, RuntimeError, KeyError, TypeError, OSError, subprocess.SubprocessError):
        raise SystemExit('Formula verification failed; inspect release identity, published metadata, and protected checks') from None
