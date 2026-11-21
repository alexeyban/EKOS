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
# Fixture C (RFC 0262 Part 0): joblets as Studio writes them — a data joblet (INPUT → tMap →
# OUTPUT) used twice, one as a lookup; trigger joblets chained; component-framework (Snowflake)
# components; tJavaRow; three projects.
fetch C KompallyAkhil/Talend2 8e8887c0b503bc29f8d8aceffe2ad31a56599c7c
# Fixture D: a joblet with two output ports, its second port used by a caller.
fetch D singhSurajsehwag/Talend_SQL 3f98d69965a29a8c9a29704649643a737a98aa3d
# Fixture E: ELT components (tELTInput / tELTMap / tELTOutput, run in the database) and several
# engines in one project (Snowflake, Oracle, PostgreSQL); Talend Cloud 8.0.1.
fetch E magrcrsdg/POC_DataCloudTalend facebc7cf8375707e159ab01c7d43cd0cbbedf20

echo
echo "export EKOS_TALEND_FIXTURE_A=$dest/A"
echo "export EKOS_TALEND_FIXTURE_B=$dest/B"
echo "export EKOS_TALEND_FIXTURE_C=$dest/C"
echo "export EKOS_TALEND_FIXTURE_D=$dest/D"
echo "export EKOS_TALEND_FIXTURE_E=$dest/E"
