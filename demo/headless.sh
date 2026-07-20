#!/usr/bin/env sh
# Generate one transcript per DEMO.md act, non-interactively.
#
# Uses `claude -p "<prompt>"` for the skill-only acts (1, 6) and
# `claude -p "<prompt>" --agents <name>` for the agent acts (2, 3, 4, 5, 7),
# writing each transcript to demo/transcripts/act-N.md. Run from the
# workspace root so $WORKSPACE_ROOT and the user-scope MCP server both
# resolve; run `cp demo/agents/*.md ~/.claude/agents/` first so the four
# agents are installed.
#
# Each act passes --allowedTools naming exactly the tools that act's agent
# (or, for skill-only acts, the read-only ekos_* set) is scoped to in
# DEMO.md/the agent's frontmatter — headless -p mode has no interactive
# permission prompt, so without this every tool call is silently denied
# and an agent can appear to "succeed" while doing nothing (confirmed: an
# early run of Act 5 replied "Saved" without writing any file).
#
# Usage: sh demo/headless.sh [act-number ...]
#   sh demo/headless.sh          # all acts
#   sh demo/headless.sh 2 7      # just acts 2 and 7

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUT_DIR="$SCRIPT_DIR/transcripts"
mkdir -p "$OUT_DIR"

EKOS_READONLY="mcp__ekos__ekos_status,mcp__ekos__ekos_search,mcp__ekos__ekos_ekl,mcp__ekos__ekos_neighborhood,mcp__ekos__ekos_state,mcp__ekos__ekos_dependents,mcp__ekos__ekos_diff"

run_act() {
  n=$1; shift
  prompt=$1; shift
  agent=${1:-}; [ "$#" -gt 0 ] && shift
  tools=${1:-$EKOS_READONLY}
  out="$OUT_DIR/act-$n.md"
  echo "== Act $n =="
  echo "prompt: $prompt"
  {
    echo "# Act $n transcript"
    echo
    echo "**Prompt:** $prompt"
    echo
    echo '```'
    if [ -n "$agent" ]; then
      claude -p "$prompt" --agents "$agent" --allowedTools "$tools"
    else
      claude -p "$prompt" --allowedTools "$tools"
    fi
    echo '```'
  } > "$out"
  echo "  -> $out"
}

# target-act-number first, then the (possibly empty) list of requested acts.
want() {
  target=$1; shift
  [ "$#" -eq 0 ] && return 0
  for a in "$@"; do [ "$a" = "$target" ] && return 0; done
  return 1
}

if want 1 "$@"; then
  run_act 1 "What do you actually know about my projects right now?"
fi
if want 2 "$@"; then
  run_act 2 "Use the estate-scout agent: find every database table related to orders across my estate, and show me what one of them is connected to." estate-scout \
    "mcp__ekos__ekos_status,mcp__ekos__ekos_search,mcp__ekos__ekos_ekl,mcp__ekos__ekos_neighborhood,mcp__ekos__ekos_state"
fi
if want 3 "$@"; then
  run_act 3 "Ask the impact-analyst: what breaks if I rename the customers table?" impact-analyst \
    "mcp__ekos__ekos_search,mcp__ekos__ekos_ekl,mcp__ekos__ekos_dependents,mcp__ekos__ekos_neighborhood,mcp__ekos__ekos_state"
fi
if want 4 "$@"; then
  run_act 4 "Have I ever hit FTS5 duplicate-row problems before? How did I fix it?" memory-keeper \
    "mcp__ekos__ekos_search,mcp__ekos__ekos_state,mcp__ekos__ekos_status,Read,Grep"
fi
if want 5 "$@"; then
  # Act 5 is a write — verify the write actually happened rather than
  # trusting the transcript. Confirmed in rehearsal: a headless -p run can
  # reply "Saved" without calling Write at all (e.g. because it decided an
  # existing note already covers the fact, or a tool call was silently
  # denied) — a confidently-wrong result that looks identical to success.
  MEM_DIR="${MEMORY_DIR:-$(CDPATH= cd -- "$SCRIPT_DIR/.." >/dev/null 2>&1 && pwd)/../memory}"
  before=$(ls -1 "$MEM_DIR" 2>/dev/null | wc -l)
  run_act 5 "Remember this for future sessions: headless.sh's act filter used to compare the target act number against itself instead of against the requested list, so passing one act number silently ran all of them — always verify a filter function against a case where it should say no." memory-keeper \
    "mcp__ekos__ekos_search,mcp__ekos__ekos_state,mcp__ekos__ekos_status,Read,Write,Bash,Grep"
  after=$(ls -1 "$MEM_DIR" 2>/dev/null | wc -l)
  if [ "$after" -le "$before" ]; then
    echo "  WARNING: no new file appeared in $MEM_DIR — the transcript may" >&2
    echo "  claim success without having actually written anything. Verify" >&2
    echo "  manually before presenting Act 5 live." >&2
  fi
fi
if want 6 "$@"; then
  run_act 6 "What changed across my workspace in the last week?"
fi
if want 7 "$@"; then
  run_act 7 "Design a CDC architecture for ingesting order data into a lakehouse. Base it on my past work — my prior CDC projects, my mistakes, my lessons." estate-architect \
    "mcp__ekos__ekos_status,mcp__ekos__ekos_search,mcp__ekos__ekos_ekl,mcp__ekos__ekos_neighborhood,mcp__ekos__ekos_dependents,mcp__ekos__ekos_state,mcp__ekos__ekos_diff,Read"
fi

echo "Done. Transcripts in $OUT_DIR"
