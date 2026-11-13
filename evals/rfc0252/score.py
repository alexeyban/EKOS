import os
import json
import re
import collections
import random
import oracledb

truth = json.load(open("truth.json"))
disc = json.load(
    open("relationships.json")
)  # a copy of <state>/profile/relationships.json
declared = {tuple(x) for x in truth["declared"]}
dic = {tuple(x) for x in truth["dictionary"]}
c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
cur.execute(
    "select lower(table_name), lower(column_name), num_distinct from dba_tab_col_statistics where owner='ADEMPIERE'"
)
nd = {(r[0], r[1]): r[2] or 0 for r in cur}
cands = []
for x in disc["candidates"]:
    m = re.match(r"[^:]+: (\w+)\.(\w+) → (\w+)\.(\w+)", x["name"])
    k = tuple(s.lower() for s in m.groups())
    cands.append((k, x))


def label(k):
    if k in declared:
        return "declared"
    if k in dic:
        return "dictionary"
    # same pair in the other direction
    if (k[2], k[3], k[0], k[1]) in dic | declared:
        return "reversed"
    # child references something per the dictionary, but a different parent
    if any(t[0] == k[0] and t[1] == k[1] for t in dic):
        return "wrong_parent"
    return "no_reference"


by = collections.Counter()
by_v = collections.defaultdict(collections.Counter)
rows = []
for k, x in cands:
    l = label(k)
    by[l] += 1
    by_v[x["verdict"]][l] += 1
    rows.append(
        (
            l,
            x["seed"],
            x["verdict"],
            ".".join(k[:2]) + " -> " + ".".join(k[2:]),
            x["orphans"],
            x["population"],
        )
    )
n = len(cands)
ok = by["declared"] + by["dictionary"]
print("candidates", n, dict(by))
print("precision (dictionary or declared)", ok, "/", n, f"= {ok / n:.3f}")
for v, cnt in by_v.items():
    t = cnt["declared"] + cnt["dictionary"]
    s = sum(cnt.values())
    print(f"  verdict {v}: {t}/{s} = {t / s:.3f}", dict(cnt))
# recall: undeclared dictionary refs whose child column holds data (optimizer stats) and parent has >= 20 distinct
pool = [t for t in dic - declared if nd.get((t[0], t[1]), 0) > 0]
pool20 = [t for t in pool if nd.get((t[2], t[3]), 0) >= 20]
found = {k for k, _ in cands}
print(
    "undeclared dictionary refs with child data:",
    len(pool),
    "found",
    len(found & set(pool)),
)
print(
    "  ... and parent distinct >= 20:", len(pool20), "found", len(found & set(pool20))
)
# single-column dictionary refs where the child column is the parent's name (same-name) to see what blocks recall
json.dump(rows, open("idempiere-candidates-labelled.json", "w"), indent=0)
random.seed(252)
for l in ("no_reference", "wrong_parent"):
    sample = [r for r in rows if r[0] == l]
    print(f"--- sample of {l} ({len(sample)}):")
    # Seeded sampling so the hand-checked sample can be drawn again; not a security use.
    for r in random.sample(sample, min(12, len(sample))):  # NOSONAR
        print("   ", r)
