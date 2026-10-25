#!/usr/bin/env bash
set -euo pipefail

# Validate the installed binary against the same checkout used by package CI.
expected=${1:?expected checkout SHA}
reported=${2:?expected clisage --version output}
[[ "$expected" == "$(git rev-parse HEAD)" ]]
[[ "$reported" == 'clisage '* ]]
version=${reported#clisage }
short=${expected:0:12}
if [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+-[0-9A-Za-z.-]+\+g[0-9a-f]{12}$ ]]; then
    [[ "$version" == *"+g$short" ]]
else
    [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]
    [[ "$(git rev-parse -q --verify "refs/tags/v$version^{commit}")" == "$expected" ]]
fi
