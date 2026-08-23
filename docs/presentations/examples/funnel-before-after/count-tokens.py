#!/usr/bin/env python3
"""Real token-counting script for the funnel-before-after deck.

Measures one thing precisely: the token size (tiktoken, cl100k_base) of the
context artifacts Claude Code actually pulled into its own context window to
answer "how do ad-hoc/exploration funnel queries work" -- once before EKOS
had this repo indexed (raw Read of the two backend files it found by
scoped `find`), and once after (real MCP tool responses, plus the two
frontend files it still had to Read in full because ekos_state/neighborhood
excerpts are truncated to ~500 chars).

This is NOT a full end-to-end session token bill (system prompt, thinking
tokens, tool schemas, etc. aren't measured or measurable from here) -- it's
the same "content gathered" proxy used in ../token-benchmark/count-tokens.py,
applied to this session's real artifacts instead of synthetic grep tiers.
"""
import tiktoken

enc = tiktoken.get_encoding("cl100k_base")


def tok(path: str) -> int:
    with open(path, "r", errors="ignore") as f:
        return len(enc.encode(f.read()))


print("=== Pass 1: before EKOS indexed this repo (raw Read + scoped find) ===")
p1_find = tok("pass1-find-command.txt")
p1_exploration = tok("pass1-read-exploration.ex")
p1_journey_step = tok("pass1-read-journey-step.ex")
p1_total = p1_find + p1_exploration + p1_journey_step
print(f"find command + output (scoped to extra/, missed frontend): {p1_find} tokens")
print(f"Read extra/lib/plausible/stats/exploration.ex (full file): {p1_exploration} tokens")
print(f"Read extra/lib/plausible/stats/journey/step.ex (full file): {p1_journey_step} tokens")
print(f"TOTAL: {p1_total} tokens")
print("Coverage: backend query engine only. Frontend UI/state layer never discovered.")
print()

print("=== Pass 2: after EKOS indexed this repo (real MCP calls + 2 follow-up Reads) ===")
p2_ekl = tok("pass2-ekos-ekl-exploration.json")
p2_search_nbhd = tok("pass2-ekos-search-and-neighborhood.json")
p2_state = tok("pass2-ekos-state.json")
p2_read_state = tok("pass2-read-exploration-state.ts")
p2_read_journey = tok("pass2-read-journey.ts")
p2_mcp_only = p2_ekl + p2_search_nbhd + p2_state
p2_total = p2_mcp_only + p2_read_state + p2_read_journey
print(f"ekos_ekl (list all 'exploration' Files, any dir):         {p2_ekl} tokens")
print(f"ekos_search x2 + ekos_neighborhood:                       {p2_search_nbhd} tokens")
print(f"ekos_state x2 (exploration-state.ts, journey.ts):         {p2_state} tokens")
print(f"  MCP calls subtotal:                                     {p2_mcp_only} tokens")
print(f"Read exploration-state.ts in full (excerpt was truncated): {p2_read_state} tokens")
print(f"Read journey.ts in full (excerpt was truncated):           {p2_read_journey} tokens")
print(f"TOTAL: {p2_total} tokens")
print("Coverage: backend query engine AND frontend UI/state layer (6 files found, 2 read).")
print()

print("=== Comparison ===")
print(f"Pass 1 (backend-only, incomplete): {p1_total} tokens")
print(f"Pass 2 (backend + frontend, complete): {p2_total} tokens")
print(f"Pass 2 costs {100 * (p2_total / p1_total - 1):.0f}% MORE tokens than pass 1 --")
print("  but pass 1 was missing an entire code layer. EKOS's value here wasn't fewer")
print("  tokens per file -- ekos_state/neighborhood excerpts are truncated, so full")
print("  accuracy still required Reading the frontend files in full, same as pass 1's")
print("  backend files. The value was DISCOVERY: one ekl query, indexed by content,")
print("  found all 6 real 'exploration' files regardless of directory -- vs. a `find`")
print("  scoped (wrongly) to extra/ only, which structurally could not see assets/js/.")
print()
print(f"MCP-calls-only subtotal ({p2_mcp_only} tokens) vs the same 2 backend files pass 1 read")
backend_only_p2_equiv = p2_ekl + p2_search_nbhd  # search/ekl portion, roughly comparable lookup cost
print(f"  (discovery step alone, before any Read): {backend_only_p2_equiv} tokens")
print(f"  vs pass 1's discovery step (scoped find): {p1_find} tokens")
print("  -- discovery itself is cheap either way; the difference is what each one finds.")
