#!/usr/bin/env bash
# Local stand-ins for the SonarCloud rules that have turned EKOS's quality gate red, run on the
# lines a change touched (Sonar's "new code"). Run before every push to main and fix what it
# reports; SonarCloud's own analysis after the push still decides the gate.
#
#   scripts/sonar-local.sh            # changes since the merge base with origin/main, committed or not
#   scripts/sonar-local.sh <base>     # changes since <base>
#
# Rust: clippy with Sonar's extra rules (S1612 redundant closure, S2208 wildcard import), plus
#       scripts/cognitive-complexity.sh (S3776, limit 30).
# TS/TSX (web/ui): eslint-plugin-sonarjs, jsx-a11y, react (scripts/sonar-local/eslint.config.mjs;
#       S3776 limit 15, S6847/S1082 interactions, S6772 JSX spacing, slow regexes).
# Python (web/api): ruff ASYNC (S7493 blocking I/O in async), RUF029 (S7503 async without await),
#       scripts/sonar-local/pycogc.py (S3776, limit 15 — Sonar's counting; it matches Sonar's
#       numbers on the functions Sonar flagged here, where `complexipy` scored up to half).
# Shell (*.sh): scripts/sonar-local/shcheck.py (S7688 `[[`, S131 default case, S6505
#       npm --ignore-scripts, S7679 positional parameters).
# Not covered locally: Python regex backtracking (S8786), async methods without await (ruff's
# RUF029 skips methods), new-code coverage and duplication — SonarCloud still decides those.
# Exit status 1 when anything is reported. Needs: cargo, uv, node (npm install in
# scripts/sonar-local once).
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
tools="$root/scripts/sonar-local"
requested_base="${1:-}"
base="${requested_base:-$(git merge-base HEAD origin/main)}"
# Files git knows about: `git add` a new file before running this, as SonarCloud only sees those.
mapfile -t files < <(git diff --name-only --diff-filter=AM "$base")
filter() { python3 "$tools/changed_lines.py" "$base"; }
found=0
has() {
  local pattern="$1"
  printf '%s\n' "${files[@]}" | grep -qE "$pattern"
}

if has '^ekos/.*\.rs$'; then
  echo "── Rust: clippy (changed lines)"
  (cd ekos && cargo clippy --workspace --all-targets --message-format=json -- \
      -W clippy::redundant_closure_for_method_calls -W clippy::wildcard_imports 2>/dev/null) \
    | python3 -c '
import json, sys
for raw in sys.stdin:
    try: m = json.loads(raw)
    except ValueError: continue
    msg = m.get("message") or {}
    if m.get("reason") != "compiler-message" or msg.get("level") not in ("warning", "error"): continue
    for s in msg.get("spans", []):
        if s.get("is_primary"):
            print(json.dumps({"file": "ekos/" + s["file_name"], "line": s["line_start"],
                              "rule": (msg.get("code") or {}).get("code", "rustc"), "message": msg["message"]}))
' | filter || found=1
  echo "── Rust: closures that only call a method on their argument (S1612, changed lines; advisory)"
  # Clippy's redundant_closure_for_method_calls misses `|row| row.values()` when the argument
  # is a reference; Sonar's S1612 does not. Same shape, by pattern — but a pattern cannot see
  # types: when the method takes `self` by value and the closure gets a reference, a method
  # reference does not compile and Sonar does not report it. So these are listed, not failed:
  # check each, and use `Type::method` where the method takes `&self`.
  for f in "${files[@]}"; do
    case "$f" in
      ekos/*.rs) [[ -f "$f" ]] && grep -nE '\|\s*([a-z_][a-z0-9_]*)\s*\|\s*\1\s*\.\s*[a-z_][a-z0-9_]*\s*\(\s*\)\s*[),]' "$f" \
        | python3 -c '
import json, sys
for raw in sys.stdin:
    n, text = raw.split(":", 1)
    print(json.dumps({"file": sys.argv[1], "line": int(n), "rule": "S1612",
                      "message": "replace the closure with a method reference: " + text.strip()[:90]}))
' "$f" ;;
      *) ;;
    esac
  done | filter || true
  echo "── Rust: cognitive complexity (whole tree, limit 30)"
  # The complexity tool is built in its own directory; a CARGO_TARGET_DIR would move it.
  if ! out="$(env -u CARGO_TARGET_DIR scripts/cognitive-complexity.sh)"; then
    echo "scripts/cognitive-complexity.sh failed"; found=1
  elif [[ -n "$out" ]]; then echo "$out"; found=1; fi
fi

# Test files are Sonar test code (`sonar.tests`), held to other rules: left out here too.
mapfile -t ts < <(printf '%s\n' "${files[@]}" | grep -E '^web/ui/src/.*\.tsx?$' | grep -vE '\.test\.tsx?$' || true)
if [[ "${#ts[@]}" -gt 0 ]]; then
  # --ignore-scripts: the linters need no install scripts, and none may run.
  [[ -x "$tools/node_modules/.bin/eslint" ]] || (cd "$tools" && npm ci --ignore-scripts --no-audit --no-fund --loglevel=error)
  echo "── TypeScript: eslint sonarjs + jsx-a11y (changed lines)"
  "$tools/node_modules/.bin/eslint" --config "$tools/eslint.config.mjs" --no-warn-ignored -f json "${ts[@]}" \
    | python3 -c '
import json, sys
for f in json.load(sys.stdin):
    for m in f["messages"]:
        rule = m.get("ruleId") or "parse"
        print(json.dumps({"file": f["filePath"], "line": m.get("line"), "rule": rule, "message": m["message"],
                          "anyline": rule == "sonarjs/cognitive-complexity"}))
' | filter || found=1
fi

# Every Python file Sonar analyses as main code: not docs, demos or fixtures (`sonar.exclusions`),
# not test code (`sonar.tests`: web/api/tests, deploy/e2e; scripts/tests by name).
mapfile -t py < <(printf '%s\n' "${files[@]}" | grep -E '\.py$' \
  | grep -vE '^(docs/|web/api/tests/|deploy/e2e/|deploy/fixtures/|scripts/tests/)|/demo/|/tests/fixtures/' || true)
if [[ "${#py[@]}" -gt 0 ]]; then
  echo "── Python: ruff ASYNC + RUF029 (changed lines)"
  (uv run --project web/api ruff check --preview --select ASYNC,RUF029 --output-format json "${py[@]}" || true) \
    | python3 -c '
import json, sys
for m in json.load(sys.stdin):
    print(json.dumps({"file": m["filename"], "line": m["location"]["row"], "rule": m["code"], "message": m["message"]}))
' | filter || found=1
  echo "── Python: cognitive complexity (changed files, limit 15; Sonar's counting, pycogc.py)"
  python3 "$tools/pycogc.py" 15 "$base" | python3 -c '
import json, sys
for raw in sys.stdin:
    f = json.loads(raw)
    print(json.dumps({"file": f["file"], "line": f["line"], "rule": "S3776", "anyline": True,
                      "message": "cognitive complexity %d > 15 (%s)" % (f["complexity"], f["name"])}))
' | filter || found=1
fi

if has '\.sh$'; then
  echo "── Shell: [[ ]] tests, default case, npm --ignore-scripts, positional parameters (changed lines)"
  python3 "$tools/shcheck.py" "$base" | filter || found=1
fi

if [[ "$found" -eq 0 ]]; then echo "sonar-local: nothing on the changed lines"; else echo "sonar-local: fix the findings above before pushing"; fi
exit "$found"
