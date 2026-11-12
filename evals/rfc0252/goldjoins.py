"""Join pairs gold SQL uses (tune split only), and how many no declared FK covers."""

import json
import sqlite3
import sys
import collections
import sqlglot
from sqlglot import exp

qs = {str(json.loads(l)["question_id"]): json.loads(l) for l in open(sys.argv[1])}
tune = [q["id"] for q in json.load(open(sys.argv[2]))["questions"]]
dbdir = sys.argv[3]
decl = {}
real = {}


def declared(db):
    if db in decl:
        return decl[db]
    c = sqlite3.connect(f"file:{dbdir}/{db}/{db}.sqlite?mode=ro", uri=True)
    s = set()
    ts = [r[0] for r in c.execute("select name from sqlite_master where type='table'")]
    for t in ts:
        for r in c.execute(f'pragma foreign_key_list("{t}")'):
            to, frm, tocol = r[2], r[3], r[4]
            if tocol is None:
                tocol = next(
                    (x[1] for x in c.execute(f'pragma table_info("{to}")') if x[5]), ""
                )
            s.add(frozenset({(t.lower(), frm.lower()), (to.lower(), tocol.lower())}))
    decl[db] = s
    real[db] = {t.lower() for t in ts}
    return s


def pairs(sql):
    try:
        tree = sqlglot.parse_one(sql, read="sqlite")
    except Exception:
        return None
    out = set()
    for sel in tree.find_all(exp.Select):
        alias = {}
        for t in sel.find_all(exp.Table):
            alias[(t.alias_or_name).lower()] = t.name.lower()
        for eq in sel.find_all(exp.EQ):
            l, r = eq.left, eq.right
            if (
                isinstance(l, exp.Column)
                and isinstance(r, exp.Column)
                and l.table
                and r.table
            ):
                lt, rt = alias.get(l.table.lower()), alias.get(r.table.lower())
                if lt and rt and lt != rt:
                    out.add(frozenset({(lt, l.name.lower()), (rt, r.name.lower())}))
    return out


stats = collections.Counter()
undeclared = collections.Counter()
for qid in tune:
    q = qs[qid]
    p = pairs(q["SQL"])
    if p is None:
        stats["unparsed"] += 1
        continue
    stats["questions"] += 1
    if p:
        stats["with_join"] += 1
    d = declared(q["db_id"])
    p = {x for x in p if all(t in real[q["db_id"]] for t, _ in x)}
    miss = [x for x in p if x not in d]
    stats["join_pairs"] += len(p)
    stats["undeclared_pairs"] += len(miss)
    if miss:
        stats["questions_with_undeclared"] += 1
    for m in miss:
        undeclared[(q["db_id"],) + tuple(sorted(m))] += 1
print(dict(stats))
for k, v in undeclared.most_common():
    print(v, k)
