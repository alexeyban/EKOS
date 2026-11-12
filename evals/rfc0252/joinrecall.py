"""Model-free: share of gold base-table join pairs (tune split) offered by each cell's pack."""

import json
import sys
import re
import collections
import sqlglot
from sqlglot import exp

qs = {str(json.loads(l)["question_id"]): json.loads(l) for l in open(sys.argv[1])}
tune = [q["id"] for q in json.load(open(sys.argv[2]))["questions"]]
ctx = sys.argv[3]


def offered(db, cell):
    out = set()
    for line in open(f"{ctx}/{db}.{cell}.txt"):
        m = re.match(r"--\s+(\w+)\.(\w+) = (\w+)\.(\w+)\s*$", line)
        if m:
            a, b, c, d = (x.lower() for x in m.groups())
            out.add(frozenset({(a, b), (c, d)}))
    return out


def tables(db):
    return {
        m.group(1).lower()
        for m in re.finditer(
            r'CREATE TABLE [`"]?(\w+)[`"]?', open(f"{ctx}/{db}.c-prime.txt").read()
        )
    }


def pairs(sql, real):
    tree = sqlglot.parse_one(sql, read="sqlite")
    out = set()
    for sel in tree.find_all(exp.Select):
        alias = {
            t.alias_or_name.lower(): t.name.lower() for t in sel.find_all(exp.Table)
        }
        for eq in sel.find_all(exp.EQ):
            l, r = eq.left, eq.right
            if (
                isinstance(l, exp.Column)
                and isinstance(r, exp.Column)
                and l.table
                and r.table
            ):
                lt, rt = alias.get(l.table.lower()), alias.get(r.table.lower())
                if lt in real and rt in real and lt != rt:
                    out.add(frozenset({(lt, l.name.lower()), (rt, r.name.lower())}))
    return out


tot = collections.Counter()
per_db = collections.defaultdict(collections.Counter)
gained = []
questions = collections.Counter()
for qid in tune:
    q = qs[qid]
    db = q["db_id"]
    p = pairs(q["SQL"], tables(db))
    if not p:
        continue
    a, b = offered(db, "c-prime"), offered(db, "c-prime-sig")
    tot["pairs"] += len(p)
    tot["without"] += len(p & a)
    tot["with"] += len(p & b)
    questions["with_join"] += 1
    questions["all_without"] += p <= a
    questions["all_with"] += p <= b
    for x in (p & b) - a:
        gained.append((qid, db, tuple(sorted(x))))
print(dict(tot), dict(questions))
print("gained:", gained)
noise = {
    db: len(offered(db, "c-prime-sig") - offered(db, "c-prime"))
    for db in ["card_games", "codebase_community", "financial", "formula_1"]
}
print("data join lines added per db:", noise)
