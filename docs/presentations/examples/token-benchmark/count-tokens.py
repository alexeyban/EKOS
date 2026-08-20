#!/usr/bin/env python3
"""Real token-counting script used to produce every number in the
token-benchmark.html deck. Run from this directory with the sibling
transcript files present, plus a checkout of plausible/analytics at
../../../../../analytics (or edit the two absolute paths below).

Uses tiktoken's cl100k_base encoding (GPT-4-class) as a standard,
reproducible reference tokenizer -- not a hand-rolled heuristic.
"""
import tiktoken

enc = tiktoken.get_encoding("cl100k_base")


def tok(path: str) -> int:
    with open(path, "r", errors="ignore") as f:
        return len(enc.encode(f.read()))


# Q1: "What columns does the imported_browsers table have?"
grep1 = tok("q1-grep-targeted.txt")  # best-case: agent already knows the exact file+line
grep2 = tok("q1-grep-realistic-repo-wide.txt")  # realistic: grep -C3 across all 32 real hits
grep3 = tok("/home/legion/PycharmProjects/analytics/priv/ingest_repo/structure.sql")  # naive: read the whole file
ekos_search = tok("q1-ekos-search-response.json")
ekos_state = tok("q1-ekos-state-response.json")
ekos_full = ekos_search + ekos_state

print("=== Q1: What columns does imported_browsers have? ===")
print(f"grep, best-case targeted (-A15, agent already knows file+line): {grep1} tokens")
print(f"grep, realistic repo-wide (-C3, all 32 real hits):              {grep2} tokens")
print(f"grep, naive (read the whole 366-line containing file):          {grep3} tokens")
print(f"EKOS (ekos_search + ekos_state, full round trip):               {ekos_full} tokens")
print(f"  vs best-case grep:  EKOS costs {ekos_full / grep1:.1f}x MORE (grep wins when omniscient)")
print(f"  vs realistic grep:  {100 * (1 - ekos_full / grep2):.1f}% fewer tokens")
print(f"  vs naive full read: {100 * (1 - ekos_full / grep3):.1f}% fewer tokens")
print()

# Q2: "What tables exist in the Postgres schema?"
grep_q2 = tok("/home/legion/PycharmProjects/analytics/priv/repo/structure.sql")
ekos_q2 = tok("q2-ekos-ekl-list.txt")

print("=== Q2: What tables exist in the Postgres schema (all 42)? ===")
print(f"grep/naive (read the whole 2,738-line schema file): {grep_q2} tokens")
print(f"EKOS (single ekl structured query):                 {ekos_q2} tokens")
print(f"  reduction: {100 * (1 - ekos_q2 / grep_q2):.1f}% fewer tokens")
