"""Reuse validated bottle metadata and merge only generated formula updates."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import time
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


def authorize_runs(github, branch, sha, number):
    query = urlencode(dict(branch=branch, head_sha=sha, event='pull_request', per_page=100))
    for page in github.api('actions/runs?' + query, pages=True):
        for run in page['workflow_runs']:
            if (run['path'] in REQUIRED_JOBS and run['head_sha'] == sha
                    and run['head_branch'] == branch and run['event'] == 'pull_request'
                    and run['actor']['login'] == 'github-actions[bot]'
                    and run['head_repository']['full_name'] == github.repository
                    and any(pr['number'] == number for pr in run['pull_requests'])
                    and run['conclusion'] == 'action_required'):
                github.api(f'actions/runs/{run["id"]}/approve', method='POST')


def merge(root, github, branch, sha, authorize=False):
    if not re.fullmatch(r'homebrew-v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', branch):
        return
    if not re.fullmatch('[a-f0-9]{40}', sha):
        raise ValueError('invalid formula commit')
    owner = github.repository.split('/')[0]
    query = urlencode(dict(state='all', head=f'{owner}:{branch}', base='main', per_page=100))
    prs = github.api('pulls?' + query)
    if not prs:
        print('Waiting for the generated formula PR to become discoverable.')
        return False
    if not any(pr['state'] == 'open' for pr in prs):
        if any(pr.get('merged_at') and pr['head']['sha'] == sha for pr in prs):
            return True
        raise ValueError('formula PR was closed without merging this exact head')
    prs = [pr for pr in prs if pr['state'] == 'open']
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
    if authorize:
        # Approve execution only after proving the PR changes no workflow or application code.
        # This authorizes read-only verification jobs, never a pull-request review approval.
        authorize_runs(github, branch, sha, pr['number'])
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
        print('Waiting for all required formula checks.')
        return False
    # Do not discard review feedback, including automatically submitted Copilot findings.
    for route in (f'pulls/{pr["number"]}/comments', f'pulls/{pr["number"]}/reviews',
                  f'issues/{pr["number"]}/comments'):
        if any((item.get('body') or '').strip()
               or item.get('state') in {'CHANGES_REQUESTED', 'COMMENTED', 'PENDING'}
               for page in github.api(route + '?per_page=100', pages=True)
               for item in page):
            print('Formula review feedback requires manual disposition before merging.')
            return True
    if authorize and github.api(f'pulls/{pr["number"]}').get('mergeable_state') != 'clean':
        print('Waiting for GitHub to confirm the protected PR is ready to merge.')
        return False
    result = github.api(f'pulls/{pr["number"]}/merge', method='PUT', fields={
        'sha': sha, 'merge_method': 'squash', 'commit_title': pr['title']})
    if not result.get('merged'):
        raise ValueError('protected formula merge was not accepted')
    print(f'Merged verified formula metadata for {branch.removeprefix("homebrew-")}.')
    return True


def publish(root, github, branch):
    if not re.fullmatch(r'homebrew-v(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)', branch):
        raise ValueError('invalid generated formula branch')
    sha = github.api(f'git/ref/heads/{branch}')['object']['sha']
    deadline = time.monotonic() + 600
    while time.monotonic() < deadline:
        if merge(root, github, branch, sha, authorize=True):
            return
        time.sleep(5)
    raise ValueError('formula publication timed out waiting for protected checks')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('classify', 'merge', 'publish'))
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
            if args.operation == 'publish':
                publish(root, github, os.environ['FORMULA_BRANCH'])
            else:
                merge(root, github, os.environ['FORMULA_BRANCH'], os.environ['FORMULA_SHA'])
    except (ValueError, RuntimeError, KeyError, TypeError, OSError, subprocess.SubprocessError):
        raise SystemExit('Formula verification failed; inspect release identity, published metadata, and protected checks') from None
