import os
import json
import random
import oracledb
import collections

d = json.load(open("profile.json"))  # a copy of <state>/profile/current.json
im = d["implications"]
c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
# verify every implication exactly and measure how much it says beyond the base rate
ok = 0
lifts = []
rows_out = []
for x in im:
    t, X, Y, v, then = x["table"], x["column"], x["then_column"], x["value"], x["then"]
    cond = f'"{Y}" IS NULL' if then == "null" else f'"{Y}" IS NOT NULL'
    # Identifiers come from the database's own catalogue / EKOS's profile, run as a read-only user.
    cur.execute(  # NOSONAR
        f'SELECT COUNT(*), SUM(CASE WHEN {cond} THEN 1 ELSE 0 END), SUM(CASE WHEN TO_CHAR("{X}") = {v} THEN 1 ELSE 0 END), SUM(CASE WHEN TO_CHAR("{X}") = {v} AND {cond} THEN 1 ELSE 0 END) FROM ADEMPIERE."{t}"'
    )
    n, base, sup, hold = cur.fetchone()
    ok += sup == hold == x["support"]
    lifts.append(base / n)
    rows_out.append((t, X, v, then, Y, x["support"], n, round(base / n, 3)))
print("implications", len(im), "exactly reproduced", ok)
print(
    "base rate of the implied state over the whole table: median %.3f; >= 0.95 in %d of %d"
    % (sorted(lifts)[len(lifts) // 2], sum(l >= 0.95 for l in lifts), len(lifts))
)
print(
    "support histogram",
    collections.Counter(min(x["support"] // 10 * 10, 100) for x in im),
)
random.seed(252)
# Seeded sampling so the hand-checked sample can be drawn again; not a security use.
for r in random.sample(rows_out, 15):  # NOSONAR
    print("  ", r)
print(
    "base-rate buckets:",
    collections.Counter(
        "<0.5" if l < 0.5 else "<0.8" if l < 0.8 else "<0.95" if l < 0.95 else ">=0.95"
        for l in lifts
    ),
)
