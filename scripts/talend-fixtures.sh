#!/usr/bin/env bash
# RFC 0262 Part 0: fetch the Talend fixture projects at their pinned commits into
# .local/talend-fixtures/ (git-ignored). They are third-party projects not cleared for
# redistribution, so they are fetched, never vendored. Prints the variable the gated tests read.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="$root/.local/talend-fixtures"
mkdir -p "$dest"

fetch() {
  local name="$1" repo="$2" commit="$3"
  local dir="$dest/$name"
  if [[ ! -d "$dir/.git" ]]; then
    git init -q "$dir"
    git -C "$dir" remote add origin "https://github.com/$repo"
  fi
  git -C "$dir" fetch -q --depth 1 origin "$commit"
  git -C "$dir" checkout -q --detach "$commit"
  echo "fixture $name: $repo @ $(git -C "$dir" rev-parse --short HEAD)"
}

fetch A AymaneSab/Data-Integration-Project-with-Talend aed50909ea1c18a520983f460504c999668ba826
fetch B BelkacemYerfa/talend-project-big-data 3d46b6f22ebbc42d43565fca2c45222bec4a23d1

echo
echo "export EKOS_TALEND_FIXTURE_A=$dest/A"
echo "export EKOS_TALEND_FIXTURE_B=$dest/B"
