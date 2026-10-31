"""Create immutable SemVer tags for CI-validated main commits; dry-run by default."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib
from urllib.parse import urlencode

WORKFLOW = '.github/workflows/tag-release.yml'
REQUIRED_JOBS = {
    '.github/workflows/ci.yml': {'check (ubuntu-latest)', 'check (macos-latest)', 'Quality', 'PR title'},
    '.github/workflows/homebrew.yml': {'install (ubuntu-latest)', 'install (macos-latest)'},
}
STABLE = re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)')
HEADER = re.compile(r'^(feat|fix|docs|chore|ci|build|test|refactor|perf|style|revert)(?:\([^()\s]+\))?(!)?: \S.*$')
MAX_COMPONENT = 2**64 - 1


def version(text):
    if not isinstance(text, str) or not STABLE.fullmatch(text):
        raise ValueError('expected a stable SemVer version')
    parsed = tuple(map(int, text.split('.')))
    if any(part > MAX_COMPONENT for part in parsed):
        raise ValueError('SemVer component overflow')
    return parsed


def bump(previous, message):
    lines = message.splitlines()
    header = HEADER.fullmatch(lines[0] if lines else '')
    if not header:
        raise ValueError('main commit must use a Conventional Commit title')
    major, minor, patch = previous
    breaking = header[2] or any(re.match(r'^BREAKING(?: CHANGE|-CHANGE):', line) for line in lines[1:])
    if breaking and major:
        result = (major + 1, 0, 0)
    elif breaking or header[1] == 'feat':
        result = (major, minor + 1, 0)
    else:
        result = (major, minor, patch + 1)
    if any(part > MAX_COMPONENT for part in result):
        raise ValueError('SemVer component overflow')
    return result


def git(root, *args):
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith('GIT_CONFIG') and key not in ('GIT_DIR', 'GIT_WORK_TREE', 'GIT_INDEX_FILE')}
    environment.update(GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM='1', GIT_OPTIONAL_LOCKS='0')
    result = subprocess.run(['git', '-c', 'core.fsmonitor=false', '-C', str(root), *args],
                            env=environment, text=True, capture_output=True, timeout=30)
    if result.returncode:
        raise ValueError('unable to inspect complete local main history')
    return result.stdout.strip()


def manifest_version(root, sha):
    manifest = tomllib.loads(git(root, 'show', f'{sha}:Cargo.toml'))
    return version(manifest['package']['version'])


def plan(root, tags):
    if git(root, 'rev-parse', 'HEAD') != git(root, 'rev-parse', 'origin/main'):
        raise ValueError('release reconciliation requires the origin/main checkout')
    if git(root, 'status', '--porcelain') or git(root, 'rev-parse', '--is-shallow-repository') != 'false':
        raise ValueError('release reconciliation requires a clean, complete checkout')
    history = git(root, 'rev-list', '--reverse', '--first-parent', 'origin/main').splitlines()
    installed = git(root, 'log', 'origin/main', '--first-parent', '--diff-filter=A', '--format=%H', '--', WORKFLOW).splitlines()
    if not installed:
        raise ValueError('automatic tagging is not installed on main')
    floor = history.index(installed[-1])
    positions = {sha: index for index, sha in enumerate(history)}
    released = {}
    for name, sha in tags.items():
        if not name.startswith('v') or not STABLE.fullmatch(name[1:]):
            continue
        parsed = version(name[1:])
        if sha in positions:
            if sha in released:
                raise ValueError('multiple stable versions point to one main commit')
            if parsed < manifest_version(root, sha):
                raise ValueError('existing release tag is below its manifest floor')
            released[sha] = parsed
    previous = None
    last = -1
    for index, sha in enumerate(history):
        if sha in released:
            if previous is not None and released[sha] <= previous:
                raise ValueError('main release tags are not monotonically increasing')
            previous, last = released[sha], index
    from homebrew_release import metadata_sync
    exempt = {sha for sha in history[floor:] if metadata_sync(root, sha, tags)}
    if last >= floor and any(sha not in released and sha not in exempt for sha in history[floor:last + 1]):
        raise ValueError('main release history contains an untagged gap')
    result = []
    for sha in history[max(floor, last + 1):]:
        if sha in exempt:
            continue
        manifest = manifest_version(root, sha)
        message = git(root, 'show', '-s', '--format=%B', sha)
        # Validate even the initial title; the first release uses the manifest floor.
        proposed = bump(previous if previous is not None else manifest, message)
        current = manifest if previous is None else max(proposed, manifest)
        name = 'v' + '.'.join(map(str, current))
        if name in tags and tags[name] != sha:
            raise ValueError('proposed release tag already belongs to another commit')
        result.append((name, sha))
        previous = current
    return result


def latest_runs(sha, repository, runs):
    selected = {}
    for item in runs:
        path = item.get('path')
        if (path not in REQUIRED_JOBS or item.get('head_sha') != sha
                or item.get('head_branch') != 'main' or item.get('event') != 'push'
                or (item.get('head_repository') or {}).get('full_name') != repository):
            continue
        if path not in selected or item['id'] > selected[path]['id']:
            selected[path] = item
    return selected


def successful_checks(sha, repository, runs, jobs):
    selected = latest_runs(sha, repository, runs)
    for path, required in REQUIRED_JOBS.items():
        item = selected.get(path)
        if not item or item.get('status') != 'completed' or item.get('conclusion') != 'success':
            return False
        latest = {}
        for job in jobs.get(item['id'], []):
            name = job['name']
            order = (job.get('run_attempt', 1), job.get('id', 0))
            if name not in latest or order > latest[name][0]:
                latest[name] = (order, job)
        successful = {job['name'] for _, job in latest.values()
                      if job.get('status') == 'completed' and job.get('conclusion') == 'success'}
        if not required <= successful:
            return False
    return True


class GitHub:
    def __init__(self, repository):
        if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
            raise ValueError('invalid repository identity')
        self.repository = repository

    def api(self, route, *, pages=False, fields=None, method=None):
        args = ['gh', 'api', f'repos/{self.repository}/{route}']
        if pages:
            args += ['--paginate', '--slurp']
        if fields:
            args += ['--method', method or 'POST', '--input', '-']
        result = subprocess.run(args, input=json.dumps(fields) if fields else None,
                                text=True, capture_output=True, timeout=120)
        if result.returncode:
            # API errors may contain provider details; never echo raw bodies.
            raise RuntimeError('GitHub API request failed; inspect workflow permissions and service status')
        return json.loads(result.stdout) if result.stdout.strip() else None

    def tags(self):
        return {item['name']: item['commit']['sha']
                for page in self.api('tags?per_page=100', pages=True) for item in page}

    def checks_passed(self, sha):
        query = urlencode(dict(head_sha=sha, branch='main', event='push', per_page=100))
        runs = [item for page in self.api('actions/runs?' + query, pages=True) for item in page['workflow_runs']]
        jobs = {}
        for item in latest_runs(sha, self.repository, runs).values():
            if item.get('status') == 'completed' and item.get('conclusion') == 'success':
                jobs[item['id']] = [job for page in self.api(f"actions/runs/{item['id']}/jobs?filter=all&per_page=100", pages=True)
                                    for job in page['jobs']]
        return successful_checks(sha, self.repository, runs, jobs)

    def create_tag(self, tag, sha):
        try:
            self.api('git/refs', fields={'ref': f'refs/tags/{tag}', 'sha': sha})
        except RuntimeError:
            # A concurrent creator/retry may have created exactly this ref.
            if self.tags().get(tag) != sha:
                raise


def reconcile(root, github, *, apply=False):
    result = []
    for tag, sha in plan(root, github.tags()):
        if not github.checks_passed(sha):
            print(f'Waiting for successful main CI and Homebrew checks at {sha[:12]}; later commits remain queued.')
            break
        if apply:
            github.create_tag(tag, sha)
        print(f'{"Created" if apply else "Would create"} {tag} at {sha[:12]}')
        result.append((tag, sha))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='Create tags; allowed only inside GitHub Actions on main.')
    args = parser.parse_args()
    try:
        if args.apply and (os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('GITHUB_REF') != 'refs/heads/main'):
            raise ValueError('tag creation is restricted to the main-branch Actions workflow')
        reconcile(Path.cwd(), GitHub(os.environ['GITHUB_REPOSITORY']), apply=args.apply)
    except (ValueError, RuntimeError, KeyError, OSError, subprocess.SubprocessError) as error:
        # Only our fixed messages are printed; never include raw subprocess/API data.
        if type(error) in (ValueError, RuntimeError) and str(error) in {
            'main commit must use a Conventional Commit title',
            'proposed release tag already belongs to another commit',
            'main release history contains an untagged gap',
        }:
            raise SystemExit(str(error)) from None
        raise SystemExit('Release reconciliation failed; check repository identity, complete main history, tag consistency, workflow permissions, and CI status') from None
