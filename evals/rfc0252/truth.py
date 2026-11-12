"""Reference truth for iDempiere references: application dictionary (AD_Column/AD_Ref_Table) + declared FKs."""

import os
import json
import sys
import oracledb

c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
cur.execute("select table_name from dba_tables where owner='ADEMPIERE'")
tables = {r[0].lower() for r in cur}
cur.execute(
    """select owner, table_name, column_name from dba_tab_columns where owner='ADEMPIERE'"""
)
cols = {(r[1].lower(), r[2].lower()) for r in cur}
# declared FKs (single column pairs)
cur.execute("""select a.table_name, a.column_name, b.table_name, b.column_name
  from dba_constraints k join dba_cons_columns a on a.owner=k.owner and a.constraint_name=k.constraint_name
  join dba_cons_columns b on b.owner=k.r_owner and b.constraint_name=k.r_constraint_name and b.position=a.position
  where k.owner='ADEMPIERE' and k.constraint_type='R'""")
declared = {tuple(x.lower() for x in r) for r in cur}
fixed = {
    21: ("c_location", "c_location_id"),
    25: ("c_validcombination", "c_validcombination_id"),
    31: ("m_locator", "m_locator_id"),
    35: ("m_attributesetinstance", "m_attributesetinstance_id"),
    33: ("s_resourceassignment", "s_resourceassignment_id"),
    32: ("ad_image", "ad_image_id"),
    53370: ("ad_chart", "ad_chart_id"),
}
cur.execute("""select lower(t.tablename), lower(c.columnname), c.ad_reference_id,
      lower(rt_t.tablename), lower(rt_c.columnname)
  from adempiere.ad_column c join adempiere.ad_table t on t.ad_table_id=c.ad_table_id
  left join adempiere.ad_ref_table rt on rt.ad_reference_id=c.ad_reference_value_id
  left join adempiere.ad_table rt_t on rt_t.ad_table_id=rt.ad_table_id
  left join adempiere.ad_column rt_c on rt_c.ad_column_id=rt.ad_key
  where c.ad_reference_id in (18,19,30,21,25,31,35,33,32,28,53370) and c.isactive='Y'""")
dictionary = set()
unresolved = 0
for tab, col, ref, rtab, rcol in cur:
    if ref in fixed:
        p = fixed[ref]
    elif ref in (18, 30) and rtab and rcol:
        p = (rtab, rcol)
    elif ref in (19, 30, 28) and col.endswith("_id"):
        p = (col[:-3], col)
    else:
        unresolved += ref != 28
        continue
    if (tab, col) in cols and p in cols and tab in tables and p[0] in tables:
        dictionary.add((tab, col) + p)
    else:
        unresolved += 1
out = {
    "declared": sorted(declared),
    "dictionary": sorted(dictionary),
    "unresolved": unresolved,
    "tables": len(tables),
}
json.dump(out, open(sys.argv[1], "w"))
d = dictionary - declared
print(
    f"declared {len(declared)}, dictionary {len(dictionary)}, dictionary not declared {len(d)}, unresolved {unresolved}"
)
