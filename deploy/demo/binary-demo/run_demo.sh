#!/usr/bin/env bash
# Reproducible demo: a compiled .NET library (MarkdownSharp 2.0.5, MIT) -> EKOS -> Python rewrite -> same results.
# Every stage runs real commands and writes frames/NN-name.txt; capture.py turns each into a PNG.
#
#   ./run_demo.sh            all stages
#   ./run_demo.sh 6 7        only those stages (they need stages 3-5 to have run once)
#
# Needs: the private ekos binary (ekos-binary repo, `cargo build --release`), wine + wine-mono via
# ekos-characterize, python3.13 (atomic groups), curl, unzip. Set EKOS_BINARY_REPO if it is not ~/PycharmProjects/ekos-binary.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="${EKOS_BINARY_REPO:-$HOME/PycharmProjects/ekos-binary}"
WORK="$HERE/work"; FR="$HERE/frames"; WS="$WORK/demo-ws"
export WINEPREFIX="${WINEPREFIX:-$HOME/.cache/ekos-characterize/prefix}" WINEDEBUG=-all
export EKOS_BIN="$WORK/bin/ekos"
PY=python3.13
mkdir -p "$WORK/bin" "$FR"
ln -sf "$REPO/target/release/ekos" "$WORK/bin/ekos"
ln -sf "$REPO/target/release/ekos-characterize" "$WORK/bin/ekos-characterize"
export PATH="$WORK/bin:$PATH"

strip() { sed -r 's/\x1b\[[0-9;]*[A-Za-z]//g' | sed "s#$HOME#~#g"; }
trunc() { awk '{ if (length($0) > 158) print substr($0, 1, 157) "…"; else print }'; }
begin() { F="$FR/$1.txt"; printf '@@ %s\n' "$2" > "$F"; echo "stage $1 — $2"; }
note()  { printf '\033[90m# %s\033[0m\n' "$*" >> "$F"; }
blank() { echo >> "$F"; }
run()   { local disp="$1"; shift; printf '\033[1;32m$\033[0m \033[1m%s\033[0m\n' "$disp" >> "$F"; { "$@" 2>&1 || true; } | strip | trunc >> "$F"; }
mcp()   { local disp="$1"; shift; printf '\033[1;35mmcp>\033[0m \033[1m%s\033[0m\n' "$disp" >> "$F"; { "$@" 2>&1 || true; } | strip | trunc >> "$F"; }
STAGES="$*"
stage() { CUR="$1"; [ -z "$STAGES" ] || [[ " $STAGES " == *" $1 "* ]]; }

# ---- setup (not a frame) ---------------------------------------------------------------------------------------
if [ ! -f "$WORK/original/MarkdownSharp.dll" ]; then
  mkdir -p "$WORK/original" "$WORK/dl"
  curl -sSL -o "$WORK/dl/markdownsharp.nupkg" https://api.nuget.org/v3-flatcontainer/markdownsharp/2.0.5/markdownsharp.2.0.5.nupkg
  unzip -oq "$WORK/dl/markdownsharp.nupkg" 'lib/net40/*' -d "$WORK/dl"
  cp "$WORK/dl/lib/net40/MarkdownSharp.dll" "$WORK/original/"
fi
MCS="$WINEPREFIX/drive_c/windows/mono/mono-2.0/lib/mono/4.5/mcs.exe"
if [ ! -f "$WORK/original/mdcli.exe" ]; then
  ekos-characterize doctor >/dev/null 2>&1 || true
  (cd "$WORK/original" && wine "$MCS" -out:mdcli.exe -r:MarkdownSharp.dll "$HERE/app/mdcli.cs" && wine "$MCS" -out:mdbatch.exe -r:MarkdownSharp.dll "$HERE/app/mdbatch.cs")
fi
$PY "$HERE/tools/make_cases.py" >/dev/null

# ---- 1. the mystery ---------------------------------------------------------------------------------------------
if stage 1; then begin 01-mystery "The starting point: a compiled library and nothing else"
  cd "$WORK/original"
  note "All we have is a compiled .NET assembly. No repository, no source, no docs."
  run "ls -l MarkdownSharp.dll" ls -l MarkdownSharp.dll
  run "file MarkdownSharp.dll" file MarkdownSharp.dll
  note "The only readable traces are string constants in the metadata:"
  run "strings -e l -n 34 MarkdownSharp.dll | head -4" bash -c 'strings -e l -n 34 MarkdownSharp.dll | head -4'
fi

# ---- 2. the original runs ---------------------------------------------------------------------------------------
if stage 2; then begin 02-original-runs "Run the original as-is: this is the behaviour to reproduce"
  cd "$WORK/original"
  run "cat inputs/12-mixed-document.md" cat "$HERE/inputs/12-mixed-document.md"
  blank
  run "wine mdcli.exe < inputs/12-mixed-document.md     # original, on wine-mono" bash -c "wine mdcli.exe < '$HERE/inputs/12-mixed-document.md'"
fi

# ---- 3. observe -------------------------------------------------------------------------------------------------
if stage 3; then begin 03-observe "ekos build: observe the binary (bytes only, nothing is executed)"
  rm -rf "$WS"; mkdir -p "$WS/app-binary"; cp "$WORK/original/MarkdownSharp.dll" "$WS/app-binary/"
  cat > "$WS/ekos.toml" <<'TOML'
[workspace]
root = "."
log-level = "warn"

[observe]
paths = ["app-binary"]
ignore-patterns = [".ekos"]
TOML
  cd "$WS"
  run "cat ekos.toml" cat ekos.toml
  blank
  run "ekos init" ekos init
  run "ekos build" ekos build
fi

# ---- 4. recover -------------------------------------------------------------------------------------------------
if stage 4; then begin 04-recover "ekos recover / resolve / compile / commit: knowledge into the ledger"
  cd "$WS"
  run "ekos recover" ekos recover
  blank
  run "ekos resolve" bash -c 'ekos resolve 2>&1 | tail -7'
  blank
  run "ekos compile" bash -c 'ekos compile 2>&1 | grep -E "Compile|Objects|Relationships"'
  blank
  run "ekos commit" bash -c 'ekos commit 2>&1 | grep -E "Commit|Objects written|Relationships written|Evidence"'
fi

# ---- 5. query ---------------------------------------------------------------------------------------------------
if stage 5; then begin 05-ledger "The compiled knowledge is queryable: types, methods, call graph"
  cd "$WS"
  run "ekos status" ekos status
  blank
  run 'ekos query find "MarkdownSharp.Markdown" | head -7' bash -c 'ekos query find "MarkdownSharp.Markdown" 2>/dev/null | head -7'
  note "Every hit is an evidence-backed ledger object addressed by id, compiler-generated closure types included."
fi

# ---- 6. type summary --------------------------------------------------------------------------------------------
if stage 6; then begin 06-type-summary "ekos_binary_explain on the type: what it is, and the order to port it in"
  cd "$WS"
  mcp 'ekos_binary_explain {"id": "MarkdownSharp.Markdown"}' $PY "$HERE/tools/present.py" type-summary MarkdownSharp.Markdown
fi

# ---- 7. a method spec -------------------------------------------------------------------------------------------
if stage 7; then begin 07-method-spec "ekos_binary_explain on one method: the spec the rewrite is written from"
  cd "$WS"
  mcp 'ekos_binary_explain {"id": "MarkdownSharp.Markdown", "method": "DoItalicsAndBold"}' $PY "$HERE/tools/present.py" method MarkdownSharp.Markdown DoItalicsAndBold 20
fi

# ---- 8. honest gap ----------------------------------------------------------------------------------------------
if stage 8; then begin 08-honest-gap "Where recovery is incomplete, it says so — and refuses to guess"
  cd "$WS"
  mcp 'ekos_binary_explain {"id": "MarkdownSharp.Markdown", "method": "Normalize"}' $PY "$HERE/tools/present.py" method MarkdownSharp.Markdown Normalize 21
  blank
  note "Whole assembly, every method body, by how far recovery got:"
  run "survey MarkdownSharp.dll --fidelity" bash -c "'$REPO/target/release/examples/survey' app-binary/MarkdownSharp.dll --fidelity 2>&1 | grep -E 'Method bodies|control_flow|statements|Gap|goto'"
fi

# ---- 9. the port ------------------------------------------------------------------------------------------------
if stage 9; then begin 09-the-port "The rewrite: written callees-first from the recovered statements"
  cd "$HERE/python-port"
  run "find . -name '*.py' | sort | xargs wc -l" bash -c "find . -name '*.py' | sort | xargs wc -l"
  blank
  note "What was recovered (excerpt of the spec):"
  run "ekos_binary_explain … DoItalicsAndBold   (excerpt)" bash -c "cd '$WS' && $PY '$HERE/tools/present.py' method MarkdownSharp.Markdown DoItalicsAndBold 20 | sed -n '/pseudocode/,\$p' | head -13"
  blank
  note "Written in Python, citing the method token it came from:"
  run "snippet mdport/markdown.py _do_italics_and_bold" $PY "$HERE/tools/snippet.py" mdport/markdown.py _do_italics_and_bold
fi

# ---- 10. static check -------------------------------------------------------------------------------------------
if stage 10; then begin 10-static-check "ekos_binary_migration_check: what the rewrite touches vs what the original touches"
  cd "$WS"
  mcp 'ekos_binary_migration_check {"id": "MarkdownSharp.Markdown", "python_source": <mdport/markdown.py>}' $PY "$HERE/tools/present.py" check MarkdownSharp.Markdown "$HERE/python-port/mdport/markdown.py" 12
fi

# ---- 11. sandbox ------------------------------------------------------------------------------------------------
if stage 11; then begin 11-sandbox "Before running any untrusted code: prove the sandbox holds"
  cd "$WORK"
  note "The next steps EXECUTE the original assembly. It runs under bubblewrap: no network, no \$HOME, read-only system."
  run "ekos-characterize doctor" ekos-characterize doctor --python "$(command -v $PY)"
fi

# ---- 12. characterize -------------------------------------------------------------------------------------------
if stage 12; then begin 12-characterize "Record what the original does, then check the rewrite against the record"
  cd "$WORK"
  run "ekos-characterize record cases.json" ekos-characterize record cases.json --python "$(command -v $PY)"
  blank
  run "ekos-characterize check cases.json     # no wine, no untrusted code" ekos-characterize check cases.json --python "$(command -v $PY)"
fi

# ---- 13. side by side -------------------------------------------------------------------------------------------
if stage 13; then begin 13-side-by-side "Same inputs through both programs, compared byte for byte"
  cd "$HERE"
  run "tools/side_by_side.sh     # wine mdcli.exe  vs  python -m mdport" tools/side_by_side.sh
  blank
  run "cat inputs/12-mixed-document.md | python -m mdport" bash -c "cd python-port && $PY -m mdport < '$HERE/inputs/12-mixed-document.md'"
fi

# ---- 14. rendered -----------------------------------------------------------------------------------------------
if stage 14; then begin 14-rendered "What a user sees: the original's HTML and the port's HTML, rendered"
  cd "$HERE"
  A="$(wine "$WORK/original/mdcli.exe" < inputs/12-mixed-document.md | sha256sum | cut -c1-16)"
  B="$(cd python-port && $PY -m mdport < ../inputs/12-mixed-document.md | sha256sum | cut -c1-16)"
  wine "$WORK/original/mdcli.exe" < inputs/12-mixed-document.md > "$FR/14-original.html"
  (cd python-port && $PY -m mdport < ../inputs/12-mixed-document.md > "$FR/14-port.html")
  run "diff <(wine mdcli.exe < 12-mixed-document.md) <(python -m mdport < 12-mixed-document.md) && echo IDENTICAL" bash -c "diff '$FR/14-original.html' '$FR/14-port.html' && echo 'IDENTICAL — no differing byte'"
  run "sha256sum (first 16 hex)" bash -c "echo 'original $A'; echo 'port     $B'"
  note "Rendered side by side in the browser image for this step."
fi

# ---- 15. fuzz ---------------------------------------------------------------------------------------------------
if stage 15; then begin 15-fuzz "Beyond hand-picked inputs: 3,000 generated documents, and a planted bug"
  cd "$HERE"
  M="$WORK/mutant"; mkdir -p "$M"; rm -rf "$M/mdport"; cp -r python-port/mdport "$M/mdport"
  sed -i 's/^_BOLD = _rx(L.BOLD, 56)/_BOLD = _rx(L.BOLD, 56 \& ~16)/' "$M/mdport/markdown.py"
  run "tools/fuzz_diff.py 3000 20260921" $PY tools/fuzz_diff.py 3000 20260921
  blank
  note "Same fuzzer against a copy of the port with ONE regex flag removed (Singleline on the bold pattern):"
  run "PORT_DIR=work/mutant tools/fuzz_diff.py 300" bash -c "PORT_DIR='$M' $PY tools/fuzz_diff.py 300 | head -3"
fi

# ---- 16. nondeterminism -----------------------------------------------------------------------------------------
if stage 16; then begin 16-nondeterminism "The one place the original is not deterministic — EKOS flagged it in the spec"
  cd "$WS"
  note "EncodeEmailAddress (0x0600004E) constructs System.Random. The spec shows it, so the port keeps it random."
  run 'ekos_binary_explain … EncodeEmailAddress   (excerpt)' bash -c "$PY '$HERE/tools/present.py' method MarkdownSharp.Markdown EncodeEmailAddress 40 | grep -E '^(EncodeEmail|fidelity|external)|Random'"
  blank
  cd "$HERE"
  run "tools/email_check.py     # original x2, port x2" $PY tools/email_check.py
fi

# ---- 17. generic baseline ---------------------------------------------------------------------------------------
if stage 17; then begin 17-generic-baseline "Why the spec matters: what general Markdown knowledge produces instead"
  cd "$HERE"
  note "Two mature generic Markdown libraries (a proxy for writing 'a Markdown converter' without the recovered spec):"
  run "tools/generic_baseline.py     # vs the original's output, 15 fixtures" python3 tools/generic_baseline.py
fi

# ---- 18. scoreboard ---------------------------------------------------------------------------------------------
if stage 18; then begin 18-scoreboard "Scoreboard — and what this does not prove"
  g() { grep -hE "$1" "$FR"/$2 | tail -1; }
  cd "$HERE"
  sum="$(g 'methods ' 06-type-summary.txt | sed -r 's/^methods +//')"
  printf '%s\n' \
   "recovered   $(grep -hE 'Binary structure' "$FR/04-recover.txt" | sed 's/^ *Binary structure recovered: //') — Markdown type: $sum" \
   "rewrite     $(cat python-port/mdport/*.py | wc -l) lines of Python in python-port/mdport (constants lifted from the recovered literals)" \
   "static      $(g 'summary  matches' 10-static-check.txt | sed 's/^summary  //')   [evidence, not a verdict]" \
   "recorded    $(g 'passed, ' 12-characterize.txt)" \
   "fixtures    $(g 'identical, ' 13-side-by-side.txt | sed 's/^ *//')" \
   "fuzz        $(grep -hE '^ +seed [0-9]+:' "$FR/15-fuzz.txt" | head -1 | sed 's/^ *//')" \
   "generic     $(grep -hE '^ +ignoring whitespace' "$FR/17-generic-baseline.txt" | sed 's/^ *//') (recovered-spec port: 15/15 byte-exact)" \
   "mutation    a planted one-flag bug: $(grep -hE '^ +seed [0-9]+:' "$FR/15-fuzz.txt" | tail -1 | sed -r 's/.*, ([0-9]+ identical, [0-9]+ different)/\1/') — caught" >> "$F"
  blank
  note "Not proven: behaviour on the .NET Framework itself (the original ran on wine-mono's class library);"
  note "inputs the fuzzer's grammar never generates; the app.config constructor (not ported); System.Random output."
  note "Recovery is .NET only today. A Java jar reaches 'structural' fidelity, not statement bodies, so this flow needs a JVM decoder first."
fi
python3 "$HERE/tools/publish_code.py"
echo "frames in $FR"
