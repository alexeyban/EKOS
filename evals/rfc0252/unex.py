import os
import json
import oracledb

d = json.load(open("profile.json"))  # a copy of <state>/profile/current.json
c = oracledb.connect(
    user="EKOS_RO",
    password=os.environ["EKOS_RO_PASSWORD"],
    dsn=os.environ.get("EKOS_ORACLE_DSN", "127.0.0.1:51521/XEPDB1"),
)
cur = c.cursor()
# Does the application dictionary (AD_Ref_List) name the unexplained values? (truth for "a value no code names")
named = total = 0
for x in d["columns"]:
    if x["coverage"]["unexplained"]["rows"] == 0:
        continue
    lits = {l[1:-1] if l.startswith("'") else l for l in x["coverage"]["literals"]}
    # Identifiers come from the database's own catalogue / EKOS's profile, run as a read-only user.
    cur.execute(  # NOSONAR
        f'SELECT DISTINCT TO_CHAR("{x["column"]}") FROM ADEMPIERE."{x["table"]}" WHERE "{x["column"]}" IS NOT NULL'
    )
    vals = {r[0] for r in cur} - lits
    cur.execute(
        """select l.value from adempiere.ad_ref_list l join adempiere.ad_column c on c.ad_reference_value_id=l.ad_reference_id
                   join adempiere.ad_table t on t.ad_table_id=c.ad_table_id where upper(t.tablename)=:t and upper(c.columnname)=:c""",
        t=x["table"],
        c=x["column"],
    )
    reflist = {r[0] for r in cur}
    total += 1
    named += bool(reflist) and vals <= reflist
print(
    f"columns with unexplained values: {total}; every unexplained value is an AD_Ref_List entry: {named}"
)
