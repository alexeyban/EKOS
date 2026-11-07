#!/usr/bin/env bash
# RFC 0245 — public validation of EKOS's Apache Ossie integration with Ossie's own validator
# (`validation/validate.py`: JSON Schema, names, references, arity and a sqlglot parse of every
# expression), on tests/fixtures/ossie-tpcds (five TPC-DS tables + Ossie's official TPC-DS model).
#
#   0. the validator itself: Ossie's official examples pass, a planted broken copy is rejected
#   1. compile the schema alone, export (YAML + JSON, confirmed + all), validate
#   2. import the official Ossie model (ingest), re-compile, export again, validate
#   3. review in the exported file, `ekos import ossie`, export the confirmed model, validate
#   4. plant two disagreements in the Ossie file: exactly two conflicts are reported
#
# Usage: scripts/ossie-validate.sh [--out DIR]
#   EKOS_BIN=<path>          the ekos binary (default: build ekos/target/debug/ekos)
#   EKOS_OSSIE_REPO=<dir>    an apache/ossie clone (default: clone it at OSSIE_REF)
#   OSSIE_REF=<commit|main>  the Ossie revision to validate against (default: the vendored schema's)
# Needs git and uv (or python3 with pyyaml, jsonschema and sqlglot). Writes every exported file,
# every validator log and summary.md to the output directory; exits non-zero on any failure.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OSSIE_REF="${OSSIE_REF:-8437d59e07c7fa56fcabc4a16d46864fd88d7313}"
OUT=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --out) OUT="$2"; shift 2 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
OUT="${OUT:-$WORK/report}"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
SUMMARY="$OUT/summary.md"

step() {
  local msg="$*"
  printf '\n== %s\n' "$msg"
}
record() {
  local name="$1" result="$2"
  printf '| %s | %s |\n' "$name" "$result" >> "$SUMMARY"
}
die() {
  local name="$1" why="$2"
  record "$name" "**FAIL** — $why"
  echo "FAIL: $name — $why" >&2
  exit 1
}

# ── tools ──────────────────────────────────────────────────────────────────────────────────
if [[ -z "${EKOS_BIN:-}" ]]; then
  step "building ekos"
  (cd "$ROOT/ekos" && cargo build -q -p ekos)
  EKOS_BIN="$ROOT/ekos/target/debug/ekos"
fi
if [[ -z "${EKOS_OSSIE_REPO:-}" ]]; then
  step "cloning apache/ossie at $OSSIE_REF"
  git clone -q https://github.com/apache/ossie.git "$WORK/ossie"
  git -C "$WORK/ossie" checkout -q "$OSSIE_REF"
  EKOS_OSSIE_REPO="$WORK/ossie"
fi
OSSIE="$EKOS_OSSIE_REPO"
if command -v uv >/dev/null; then
  PY=(uv run --no-project --quiet --with pyyaml --with jsonschema --with sqlglot python -I)
else
  PY=(python3 -I)
fi
HELPER="$ROOT/scripts/ossie_e2e.py"
export EKOS_HOME="$WORK/home" NO_COLOR=1
unset EKOS_PROJECT EKOS_TENANT EKOS_CONFIG EKOS_WORKSPACE

{
  echo "# EKOS ↔ Apache Ossie validation"
  echo
  echo "- EKOS: \`$("$EKOS_BIN" --version)\` ($(git -C "$ROOT" rev-parse --short HEAD))"
  echo "- Ossie validator: apache/ossie \`$(git -C "$OSSIE" rev-parse --short HEAD)\`"
  echo "- Fixture: \`tests/fixtures/ossie-tpcds\`"
  echo
  echo "| Step | Result |"
  echo "|---|---|"
} > "$SUMMARY"

# validate <label> <file>...: the official validator must pass every file.
validate() {
  local label="$1"; shift
  local log="$OUT/validate-$label.log"
  if "${PY[@]}" "$OSSIE/validation/validate.py" --schema "$OSSIE/core-spec/ossie-schema.json" -- "$@" > "$log" 2>&1 \
     && [[ "$(grep -c 'Validation PASSED' "$log")" -eq "$#" ]]; then
    echo "  official validator: $# file(s) PASSED"
  else
    cat "$log" >&2
    die "$label" "the official validator rejected an export (see validate-$label.log)"
  fi
}
helper() { "${PY[@]}" "$HELPER" "$@"; }
ekos() { (cd "$WS" && "$EKOS_BIN" "$@"); }
pipeline() {
  local tag="$1"
  for s in build recover resolve compile commit; do ekos "$s"; done > "$OUT/pipeline-$tag.log" 2>&1
}

# ── 0. the validator works ─────────────────────────────────────────────────────────────────
step "0. validator self-check"
validate 0-official-examples "$OSSIE/examples/tpcds_semantic_model.yaml" "$OSSIE/examples/flights.semantic_model.yaml"
helper break "$OSSIE/examples/tpcds_semantic_model.yaml" > "$OUT/0-planted-broken.yaml"
if "${PY[@]}" "$OSSIE/validation/validate.py" --schema "$OSSIE/core-spec/ossie-schema.json" -- "$OUT/0-planted-broken.yaml" > "$OUT/validate-0-planted-broken.log" 2>&1; then
  die "0. validator self-check" "a planted broken model PASSED: the validator is not checking"
fi
echo "  a planted broken model is rejected"
record "0. Validator self-check" "official examples pass; a planted broken model is rejected"

# ── 1. schema only → export → validate ─────────────────────────────────────────────────────
step "1. export the compiled schema"
WS="$WORK/ws"
mkdir -p "$WS"
cp -r "$ROOT/tests/fixtures/ossie-tpcds/db" "$WS/"
ekos init > /dev/null
printf '\n[semantics]\nenabled = true\n' >> "$WS/ekos.toml"
grep -q '^\[recover.sql\]' "$WS/ekos.toml" && die "1. Export" "ekos init now writes [recover.sql]; update this script"
printf '\n[recover.sql]\ndefault-dialect = "postgres"\n' >> "$WS/ekos.toml"
pipeline 1
ekos export ossie --out "$OUT/1-export.ossie.yaml" > /dev/null
ekos export ossie --status all --out "$OUT/1-export-all.ossie.yaml" > /dev/null
ekos export ossie --status all --json --out "$OUT/1-export-all.ossie.json" > /dev/null
validate 1-export "$OUT/1-export.ossie.yaml" "$OUT/1-export-all.ossie.yaml" "$OUT/1-export-all.ossie.json"
helper check-first "$OUT/1-export-all.ossie.json" || die "1. Export" "content check failed"
record "1. Export the compiled schema" "3 exports (YAML/JSON, confirmed/all) pass; 6 datasets, composite key, 4 relationships, no invented metric"

# ── 2. import the Ossie model → export again → validate ────────────────────────────────────
step "2. import the official Ossie model, export again"
mkdir -p "$WS/semantic"
cp "$ROOT/tests/fixtures/ossie-tpcds/semantic/tpcds.ossie.yaml" "$WS/semantic/"
pipeline 2
grep -q "Ossie datasets       : 5 linked to a table, 0 unresolved, 0 ambiguous" "$OUT/pipeline-2.log" \
  || die "2. Import" "not all 5 Ossie datasets linked to their tables (see pipeline-2.log)"
open=$(ekos conflicts list --type ossie_vs_schema --json | "${PY[@]}" -c 'import json,sys; print(len(json.load(sys.stdin)))')
[[ "$open" -eq 0 ]] || die "2. Import" "$open unexpected Ossie-vs-schema conflicts"
ekos export ossie --status all --out "$OUT/2-reexport-all.ossie.yaml" > /dev/null
ekos export ossie --status all --json --out "$OUT/2-reexport-all.ossie.json" > /dev/null
validate 2-reexport "$OUT/2-reexport-all.ossie.yaml" "$OUT/2-reexport-all.ossie.json"
helper check-ingest "$OUT/2-reexport-all.ossie.json" || die "2. Import" "content check failed"
record "2. Import the Ossie model, export again" "5/5 datasets linked, 0 conflicts; re-export passes and carries the model's metric and synonyms"

# ── 3. review in the file → import ossie → export → validate ───────────────────────────────
step "3. review round trip"
helper review "$OUT/2-reexport-all.ossie.json" > "$OUT/3-reviewed.ossie.json"
ekos import ossie "$OUT/3-reviewed.ossie.json" --dry-run --json > "$OUT/3-import-plan.json"
ekos import ossie "$OUT/3-reviewed.ossie.json" --as ossie-validate --json > "$OUT/3-import-applied.json"
applied=$("${PY[@]}" -c 'import json,sys; print(json.load(open(sys.argv[1]))["applied"])' "$OUT/3-import-applied.json")
[[ "$applied" -eq 3 ]] || die "3. Review" "$applied decisions applied, expected 3"
ekos export ossie --out "$OUT/3-export-confirmed.ossie.yaml" > /dev/null
ekos export ossie --json --out "$OUT/3-export-confirmed.ossie.json" > /dev/null
validate 3-confirmed "$OUT/3-export-confirmed.ossie.yaml" "$OUT/3-export-confirmed.ossie.json"
helper check-review "$OUT/3-export-confirmed.ossie.json" || die "3. Review" "content check failed"
again=$(ekos import ossie "$OUT/3-reviewed.ossie.json" --dry-run --json | "${PY[@]}" -c 'import json,sys; p=json.load(sys.stdin); print(len(p["decisions"]) + len(p["errors"]))')
[[ "$again" -eq 0 ]] || die "3. Review" "re-importing the same file planned $again decision(s) or error(s)"
record "3. Review round trip" "3 decisions applied (confirm, edit, reject); confirmed export passes; re-import plans nothing"

# ── 4. planted disagreements are reported ──────────────────────────────────────────────────
step "4. planted disagreements"
helper plant "$WS/semantic/tpcds.ossie.yaml" > "$WORK/planted.yaml"
mv "$WORK/planted.yaml" "$WS/semantic/tpcds.ossie.yaml"
pipeline 4
ekos conflicts list --type ossie_vs_schema --json > "$OUT/4-conflicts.json"
found=$("${PY[@]}" -c 'import json,sys; print(" ".join(sorted(c["attribute"] for c in json.load(open(sys.argv[1])))))' "$OUT/4-conflicts.json")
[[ "$found" == "columns.s_city.datatype columns.s_manager" ]] || die "4. Planted disagreements" "found: $found"
ekos export ossie --status all --out "$OUT/4-export-all.ossie.yaml" > /dev/null
validate 4-planted "$OUT/4-export-all.ossie.yaml"
record "4. Planted disagreements" "exactly the 2 planted (missing column, wrong type) reported; export still passes"

echo
cat "$SUMMARY"
echo
echo "All Ossie validation steps passed. Report: $OUT"
