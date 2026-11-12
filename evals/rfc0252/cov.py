import os
import sys
import json
import random
import oracledb

d = json.load(open(sys.argv[1]))  # <state>/profile/current.json
c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
cols = [x for x in d["columns"] if x["coverage"]["non_null"] > 0]
random.seed(252)
sample = random.sample(cols, 40)
ok = bad = 0
dead = 0
for x in sample:
    t, col, cov = x["table"], x["column"], x["coverage"]
    lits = cov["literals"]
    cur.execute(
        f'SELECT TO_CHAR("{col}"), COUNT(*) FROM ADEMPIERE."{t}" WHERE "{col}" IS NOT NULL GROUP BY TO_CHAR("{col}")'
    )
    data = dict(cur.fetchall())
    for lit, v in lits.items():
        val = lit[1:-1].replace("''", "'") if lit.startswith("'") else lit
        if data.get(val, 0) == v["rows"]:
            ok += 1
        else:
            bad += 1
            print("MISMATCH", t, col, lit, v["rows"], data.get(val))
    unexpl = sum(
        n
        for k, n in data.items()
        if not any((l[1:-1] if l.startswith("'") else l) == k for l in lits)
    )
    if unexpl != cov["unexplained"]["rows"]:
        bad += 1
        print("UNEXPL MISMATCH", t, col, unexpl, cov["unexplained"])
    else:
        ok += 1
print("checked", ok + bad, "equal", ok)
dd = [
    (x["table"], x["column"], l, v)
    for x in d["columns"]
    for l, v in x["coverage"]["literals"].items()
    if v.get("dead")
]
print("dead codes", len(dd))
for r in random.sample(dd, 8):
    print("  ", r)
