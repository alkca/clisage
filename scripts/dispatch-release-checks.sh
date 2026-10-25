#!/usr/bin/env bash
set -euo pipefail

# Capture first so a listing failure cannot be hidden by process substitution.
branches=$(gh pr list --repo "$GITHUB_REPOSITORY" --state open \
  --json headRefName --jq '.[] | select(.headRefName | startswith("release-plz-")) | .headRefName')
while IFS= read -r branch; do
  [[ -n "$branch" ]] || continue
  gh workflow run ci.yml --repo "$GITHUB_REPOSITORY" --ref "$branch"
  gh workflow run homebrew.yml --repo "$GITHUB_REPOSITORY" --ref "$branch"
done <<< "$branches"
