#!/usr/bin/env bash
set -euo pipefail
pr_title=${1:-${PR_TITLE:-}}
pattern='^(feat|fix|docs|chore|ci|build|test|refactor|perf|style|revert)(\([^()[:space:]]+\))?!?: [^[:space:]].*$'
if [[ $pr_title == *$'\n'* || $pr_title == *$'\r'* || ! $pr_title =~ $pattern ]]; then
    printf 'Use a Conventional Commit PR title, e.g. feat(auth): add profiles or fix!: change CLI.\n' >&2
    exit 1
fi
