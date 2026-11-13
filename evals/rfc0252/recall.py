import os
import json
import collections
import oracledb

truth = json.load(open("truth.json"))
rows = json.load(open("rows.json"))
declared = {tuple(x) for x in truth["declared"]}
dic = {tuple(x) for x in truth["dictionary"]}
pool = sorted(dic - declared)
c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
by_child = collections.defaultdict(set)
for t in pool:
    by_child[t[0]].add(t[1])
nonnull = {}
distinct = {}
for tab, cols in by_child.items():
    cols = sorted(cols)
    sel = ", ".join(
        f'COUNT("{x.upper()}"), COUNT(DISTINCT "{x.upper()}")' for x in cols
    )
    # Identifiers come from the database's own catalogue / EKOS's profile, run as a read-only user.
    cur.execute(f'SELECT {sel} FROM ADEMPIERE."{tab.upper()}"')  # NOSONAR
    r = cur.fetchone()
    for i, x in enumerate(cols):
        nonnull[(tab, x)] = r[2 * i]
        distinct[(tab, x)] = r[2 * i + 1]
pdist = {}
for p in {(t[2], t[3]) for t in pool}:
    # Identifiers come from the database's own catalogue / EKOS's profile, run as a read-only user.
    cur.execute(  # NOSONAR
        f'SELECT COUNT(DISTINCT "{p[1].upper()}") FROM ADEMPIERE."{p[0].upper()}"'
    )
    pdist[p] = cur.fetchone()[0]
with_data = [t for t in pool if nonnull[(t[0], t[1])] > 0]
eligible = [t for t in with_data if pdist[(t[2], t[3])] >= 20]
found = {tuple(r[3].replace(" -> ", ".").split(".")) for r in rows}
print(
    "undeclared dictionary refs",
    len(pool),
    "| child holds data",
    len(with_data),
    "| and parent >= 20 distinct",
    len(eligible),
    "| found",
    len(found & set(eligible)),
)
print(
    "parents of refs with data, distinct-value histogram:",
    collections.Counter(min(pdist[(t[2], t[3])] // 10 * 10, 100) for t in with_data),
)
print(
    "eligible by parent:", collections.Counter((t[2]) for t in eligible).most_common(12)
)
json.dump({"eligible": eligible}, open("eligible.json", "w"))
import statistics

if eligible:
    js = []
    for t in eligible:
        j = distinct[(t[0], t[1])] / pdist[(t[2], t[3])]
        js.append(min(j, 1.0))
    p = [1 - (1 - j * j) ** 8 for j in js]
    print(
        "Jaccard upper bound (child distinct / parent distinct): median %.3f, p90 %.3f, max %.3f"
        % (statistics.median(js), sorted(js)[int(0.9 * len(js))], max(js))
    )
    print("expected LSH proposals (2-row bands x 8): %.1f of %d" % (sum(p), len(p)))
    print(
        "child distinct histogram:",
        collections.Counter(
            min(distinct[(t[0], t[1])], 20) for t in eligible
        ).most_common(),
    )
    top = sorted(eligible, key=lambda t: -distinct[(t[0], t[1])] / pdist[(t[2], t[3])])[
        :12
    ]
    for t in top:
        print(
            t,
            "child distinct",
            distinct[(t[0], t[1])],
            "nonnull",
            nonnull[(t[0], t[1])],
            "parent",
            pdist[(t[2], t[3])],
        )
