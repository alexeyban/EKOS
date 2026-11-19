#!/usr/bin/env bash
# RFC 0170 demonstration: a real LinkML schema for LedgerSMB, recovered from its own source.
#
#   demo/ledgersmb-linkml/run.sh             # clone LedgerSMB, compile, review, export, validate
#   LEDGERSMB_SRC=~/src/LedgerSMB run.sh     # use a local clone instead of GitHub (faster)
#   SKIP_REVIEW=1 run.sh                     # stop before the review step (hypotheses only)
#
# Needs: cargo, git, python3 (with venv). No LLM is called: `[llm]` points at an env var that
# does not exist, and `llm-definitions` stays off. Everything lands in demo/ledgersmb-linkml/work/.
#
# Steps:
#   1. LedgerSMB at a pinned commit (SQL schema + PL/pgSQL, Perl, UI, git history)
#   2. ekos build → recover → resolve → compile → commit, with [semantics] enabled
#   3. look at what came out: concepts, coded values, constraints, gaps — all hypotheses
#   4. review: confirm / edit / reject a small set, each decision justified by LedgerSMB's own
#      schema comments and lookup rows (the script stands in for the human reviewer here)
#   5. the LinkML-native review path: edit the exported YAML, `ekos import linkml`
#   6. export the confirmed schema; lint it and generate JSON Schema + Pydantic with LinkML's
#      own tools; validate sample records (one valid, one with an unknown account category)
set -euo pipefail

DEMO="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$DEMO/../.." && pwd)"
WORK="$DEMO/work"
SRC="$WORK/LedgerSMB"
PIN="544bcd947ec10a5aa661e4b124ac814f89127457"   # LedgerSMB master, 2026-09-13
REVIEWER="demo-reviewer"

step() { printf '\n\033[1m== %s\033[0m\n' "$*"; }

mkdir -p "$WORK"

# ── EKOS ──────────────────────────────────────────────────────────────────────
step "build ekos (release; the first build takes a while)"
cargo build -q --release --manifest-path "$ROOT/ekos/Cargo.toml" -p ekos
EKOS="$ROOT/ekos/target/release/ekos"

# ── 1. LedgerSMB ──────────────────────────────────────────────────────────────
step "1. LedgerSMB at $PIN"
if [ ! -d "$SRC/.git" ]; then
    # Full history, not a shallow clone: rationale links come from `git blame`.
    git clone -q "${LEDGERSMB_SRC:-https://github.com/ledgersmb/LedgerSMB.git}" "$SRC"
fi
git -C "$SRC" checkout -q --detach "$PIN"
git -C "$SRC" log -1 --format='   %h %ad %s' --date=short

cd "$SRC"
rm -rf .ekos .ekos-semantics-demo
cat > ekos.toml <<'TOML'
# EKOS demo workspace: LedgerSMB → business semantics → LinkML (RFC 0170). No LLM.
[workspace]
root = "."
log-level = "warn"

[observe]
paths = ["."]
# Tests, vendored and generated material, translations and artwork carry no business meaning.
ignore-patterns = [".ekos", ".git", "node_modules", "target", "t", "xt", "old", "bin",
  "utils", "templates", "workflows", "locale", "doc", "images", "css", "img", ".github",
  ".circleci", "yarn.lock", "Changelog", ".tx", "ekos-demo"]

[recover.sql]
default-dialect = "postgres"

[llm]
# Deliberately unreachable: structural analysis only, nothing metered.
api-key-env = "EKOS_DEMO_NO_LLM_KEY"

[semantics]
enabled = true
ontology = "ekos-demo/vocabulary.yaml"
TOML
mkdir -p ekos-demo
cp "$ROOT/ekos/docs/rfcs/0170-example-vocabulary.yaml" ekos-demo/vocabulary.yaml

# ── 2. compile ────────────────────────────────────────────────────────────────
step "2. ekos build → recover → resolve → compile → commit"
"$EKOS" init >/dev/null 2>&1 || true
for stage in build recover resolve compile; do
    printf '   %-8s' "$stage"; start=$SECONDS
    "$EKOS" "$stage" >"$WORK/$stage.log" 2>&1 || { echo "failed — see $WORK/$stage.log"; exit 1; }
    echo "ok ($((SECONDS - start))s)"
done
printf '   %-8s' commit; start=$SECONDS
"$EKOS" commit --yes >"$WORK/commit.log" 2>&1 || { echo "failed — see $WORK/commit.log"; exit 1; }
echo "ok ($((SECONDS - start))s)"
grep -i "semantics" "$WORK/commit.log" | sed 's/^/   /' || true

# ── 3. what came out ──────────────────────────────────────────────────────────
step "3. recovered hypotheses"
for kind in concept enum constraint gap conflict rationale; do
    # awk, not head: reading every line keeps ekos from writing into a closed pipe.
    printf '   %-11s %s\n' "$kind" "$("$EKOS" semantics list --kind "$kind" | awk 'NF && !p {print; p=1}')"
done

step "3a. one concept, with its evidence"
"$EKOS" semantics show EntityCreditAccountCustomer | sed 's/^/   /' 

step "3b. a coded value: where its meaning came from"
"$EKOS" semantics list --kind enum | grep "entity_credit_account.entity_class = [123] " | sed 's/^/ /'

step "3c. what EKOS could not explain (first gaps)"
"$EKOS" semantics gaps | awk 'NR <= 12 {print "   " $0}' 

if [ "${SKIP_REVIEW:-0}" = 1 ]; then
    "$EKOS" export linkml --status hypothesis --out "$WORK/ledgersmb.hypotheses.linkml.yaml"
    echo; echo "SKIP_REVIEW=1: hypotheses only → $WORK/ledgersmb.hypotheses.linkml.yaml"
    exit 0
fi

# ── 4. review (CLI) ───────────────────────────────────────────────────────────
# Confirming is human-only: no MCP tool can do it. Here the script plays the reviewer, and every
# decision cites the LedgerSMB text it rests on. A real review would be a domain expert's.
step "4. review on the CLI (as '$REVIEWER')"
review() { "$EKOS" semantics "$@" --as "$REVIEWER" | sed 's/^/   /'; }

# account.category: COMMENT ON COLUMN account.category IS 'A=asset,L=liability,Q=Equity,I=Income,E=expense'
review edit "account.category = 'A'" --label Asset     --note "column comment, sql/Pg-database.sql"
review edit "account.category = 'L'" --label Liability --note "column comment"
review edit "account.category = 'Q'" --label Equity    --note "column comment"
review edit "account.category = 'I'" --label Income    --note "column comment"
review edit "account.category = 'E'" --label Expense   --note "column comment"
review confirm "account CHECK (category IN ('A', 'L', 'Q', 'I', 'E'))" --note "the declared domain"

# entity_credit_account.entity_class: seeded by INSERT INTO entity_class, and LedgerSMB::Magic EC_* constants
review confirm "entity_credit_account.entity_class = 1" "entity_credit_account.entity_class = 2" \
               "entity_credit_account.entity_class = 3" --note "entity_class seed rows + LedgerSMB::Magic EC_* constants"
review edit EntityCreditAccountCustomer --name CustomerAccount \
       --description "A credit account held by a customer (entity_class 2)." --note "entity_class seed row"
review edit EntityCreditAccountVendor --name VendorAccount \
       --description "A credit account held by a vendor (entity_class 1)." --note "entity_class seed row"

# oe: COMMENT ON TABLE oe lists sales orders, purchase orders, quotations, RFQs; oe_class seeds them
review confirm "oe.oe_class_id = 1" "oe.oe_class_id = 2" "oe.oe_class_id = 3" "oe.oe_class_id = 4" \
       --note "oe_class seed rows"
review edit OeNotClosed --name OpenOrder --description "An order or quotation not yet closed." \
       --note "oe.closed"

# parts: COMMENT ON TABLE parts — "If assembly is true, then an assembly"
review edit PartsAssembly --name Assembly --description "A part built from other parts." \
       --note "parts table comment"
review edit PartsWithInventoryAccnoId --name InventoryPart \
       --description "A part stocked as goods: it has an inventory account." --note "parts table comment"
review edit PartsNotObsolete --name ActivePart --description "A part still offered: not obsolete." \
       --note "parts.obsolete"

# transactions.approved gates every report in sql/modules (28 sites)
review edit TransactionsApproved --name ApprovedTransaction \
       --description "A posted transaction: approved, so it counts in reports." --note "28 report sites"

# A reject, to show one: 't' is PostgreSQL's text form of boolean true, so this concept is the
# same filter as AccountLinkDescriptionSummary written differently.
review reject AccountLinkDescriptionSummaryT --note "duplicate of AccountLinkDescriptionSummary ('t' = true)"

# ── 5. review in LinkML ───────────────────────────────────────────────────────
# The other path: an expert edits the exported YAML, and `ekos import linkml` turns the edits
# into the same review decisions — matched by `ekos_id`, applied all or nothing.
step "5. review in the YAML: edit the export, then ekos import linkml"
"$EKOS" export linkml --status hypothesis --name ledgersmb --out "$WORK/review.linkml.yaml"
python3 - "$WORK/review.linkml.yaml" "$WORK/review-edited.linkml.yaml" <<'PY'
import re, sys
src, dst = sys.argv[1], sys.argv[2]
text = open(src).read()
# Confirm and rename one concept by editing its YAML, the way an expert would in an editor:
# YearendNotReversed → ActiveYearEnd, status hypothesis → confirmed.
block = re.search(r"^  YearendNotReversed:\n(?:    .*\n|      .*\n)+", text, re.M)
assert block, "YearendNotReversed not in the export"
b = block.group(0).replace("  YearendNotReversed:", "  ActiveYearEnd:", 1)
b = b.replace("ekos_status: hypothesis", "ekos_status: confirmed", 1)
open(dst, "w").write(text.replace(block.group(0), b))
print("   edited: YearendNotReversed → ActiveYearEnd, ekos_status: confirmed")
PY
"$EKOS" import linkml "$WORK/review-edited.linkml.yaml" --dry-run | sed 's/^/   /'
"$EKOS" import linkml "$WORK/review-edited.linkml.yaml" --as "$REVIEWER" | sed 's/^/   /'

step "after review"
"$EKOS" semantics list --status confirmed | grep -v '^$' | sed 's/^/   /' | tail -25

# ── 6. export + LinkML's own tooling ──────────────────────────────────────────
step "6. ekos export linkml (confirmed only — the default)"
OUT="$WORK/ledgersmb.linkml.yaml"
"$EKOS" export linkml --name ledgersmb --out "$OUT"
"$EKOS" export linkml --name ledgersmb --status all --out "$WORK/ledgersmb.all.linkml.yaml"
printf '   %s: %s lines\n' "$OUT" "$(wc -l <"$OUT")"
printf '   %s: %s lines (every element, with its status)\n' "$WORK/ledgersmb.all.linkml.yaml" \
    "$(wc -l <"$WORK/ledgersmb.all.linkml.yaml")"

VENV="$WORK/linkml-venv"
if [ ! -x "$VENV/bin/linkml-lint" ]; then
    step "install LinkML into $VENV"
    python3 -m venv "$VENV"
    "$VENV/bin/pip" install -q "linkml==1.11.1"
fi

step "6a. linkml-lint"
# Errors fail the demo. Warnings are LinkML style advice: slots without a description (the SQL
# had no comment for them) and permissible values named by their code ('A', '1').
"$VENV/bin/linkml-lint" --format tsv --ignore-warnings "$OUT" >"$WORK/lint.tsv"
awk -F'\t' 'NR > 1 && $3 != "" {n[$4]++; r[$3]++}
    END {printf "   errors: %d, warnings: %d (", n["error"], n["warning"]
         for (k in r) {printf "%s%s %d", sep, k, r[k]; sep=", "}; print ")"}' "$WORK/lint.tsv"

step "6b. gen-json-schema, gen-pydantic"
"$VENV/bin/gen-json-schema" "$OUT" >"$WORK/ledgersmb.schema.json"
"$VENV/bin/gen-pydantic" "$OUT" >"$WORK/ledgersmb_models.py"
printf '   %s (%s bytes)\n   %s (%s lines)\n' \
    "$WORK/ledgersmb.schema.json" "$(wc -c <"$WORK/ledgersmb.schema.json")" \
    "$WORK/ledgersmb_models.py" "$(wc -l <"$WORK/ledgersmb_models.py")"

step "6c. linkml-validate: records against the recovered schema"
# Two `account` rows. Required fields are the table's NOT NULL columns; `category`'s range is the
# confirmed AccountCategory enum, because its CHECK constraint was confirmed in step 4.
cat >"$WORK/account-ok.yaml" <<'YAML'
id: 1
accno: "1060"
description: Checking account
category: A
heading: 1
is_temp: false
contra: false
tax: false
obsolete: false
YAML
sed -e 's/^id: 1/id: 2/' -e 's/^accno: .*/accno: "9999"/' -e 's/^category: A/category: X/' \
    -e 's/^description: .*/description: A category LedgerSMB does not have/' \
    "$WORK/account-ok.yaml" >"$WORK/account-bad.yaml"
validate() {
    printf '   %s (category %s): ' "$(basename "$1")" "$(sed -n 's/^category: //p' "$1")"
    if out="$("$VENV/bin/linkml-validate" -s "$OUT" -C Account "$1" 2>&1)"; then
        echo "valid"
    else
        echo "INVALID"
    fi
    printf '%s\n' "$out" | sed "s|$WORK/||; s/^/     /"
}
validate "$WORK/account-ok.yaml"
validate "$WORK/account-bad.yaml"

step "done"
cat <<EOF
   Schema (confirmed):   $OUT
   Schema (everything):  $WORK/ledgersmb.all.linkml.yaml
   JSON Schema:          $WORK/ledgersmb.schema.json
   Pydantic models:      $WORK/ledgersmb_models.py
   EKOS workspace:       $SRC   (try: $EKOS semantics gaps --config $SRC/ekos.toml)
EOF
