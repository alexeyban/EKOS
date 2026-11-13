"""RFC 0252 §8: paired comparison of two BIRD cells over the questions both answered.

Usage: python3 bird_compare.py, from a folder holding `reports/` (cells c-prime vs c-prime-sig)
Reads every `<stamp>-<cell>-tune.json` report (chunked runs) and pairs questions by id.
"""

import collections
import glob
import json
import math


def load(reports, cell):
    out = {}
    for f in sorted(glob.glob(f"{reports}/*-{cell}-tune.json")):
        for q in json.load(open(f))["questions"]:
            out[q["id"]] = q
    return out


def mcnemar_exact(b, c):
    """Two-sided exact McNemar p-value on discordant counts b, c."""
    n = b + c
    if n == 0:
        return 1.0
    k = min(b, c)
    p = sum(math.comb(n, i) for i in range(k + 1)) / 2**n
    return min(1.0, 2 * p)


def main():
    reports, a_cell, b_cell = "reports", "c-prime", "c-prime-sig"
    a, b = load(reports, a_cell), load(reports, b_cell)
    ids = sorted(set(a) & set(b), key=int)
    stats = collections.Counter()
    for i in ids:
        sa, sb = a[i]["score"], b[i]["score"]
        stats["n"] += 1
        stats["ex_a"] += bool(sa["ex"])
        stats["ex_b"] += bool(sb["ex"])
        stats["exec_a"] += bool(sa["executed"])
        stats["exec_b"] += bool(sb["executed"])
        stats["a_only"] += bool(sa["ex"]) and not sb["ex"]
        stats["b_only"] += bool(sb["ex"]) and not sa["ex"]
        stats["same_sql"] += a[i]["prediction"].get("sql") == b[i]["prediction"].get(
            "sql"
        )
        for cell, s in (("a", sa), ("b", sb)):
            if s.get("join_recall") is not None:
                stats[f"jr_n_{cell}"] += 1
                stats[f"jr_sum_{cell}"] += s["join_recall"]
    n = stats["n"]
    print(f"paired questions: {n}")
    if not n:
        return
    for cell, name in (("a", a_cell), ("b", b_cell)):
        jr = stats[f"jr_sum_{cell}"] / max(stats[f"jr_n_{cell}"], 1)
        print(
            f"  {name}: EX {stats[f'ex_{cell}']}/{n}, executed {stats[f'exec_{cell}']}/{n}, "
            f"mean join recall {100 * jr:.1f}% over {stats[f'jr_n_{cell}']}"
        )
    print(
        f"  discordant: {a_cell} only {stats['a_only']}, {b_cell} only {stats['b_only']}, "
        f"exact McNemar p = {mcnemar_exact(stats['a_only'], stats['b_only']):.3f}; "
        f"identical SQL in {stats['same_sql']}"
    )


main()
