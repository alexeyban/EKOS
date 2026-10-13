import json, sys
from lib import check, mcp, sign_in, summary, tool
alice = sign_in("alice@example.com"); WS="acme.shop"; A="/mcp/acme/shop"
full = alice.post("/api/tenants/acme/tokens", json={"name":"ids-full","role":"viewer"}).json()["token"]
t = alice.post("/api/tenants/acme/tokens", json={"name":"ids-limited","role":"viewer","workspace_id":WS}).json()
alice.post("/api/tenants/acme/bindings", json={"token_id":t["id"],"role":"viewer","workspace_id":WS,"effect":"deny","source":"handbook","path":"hr/**"})
lim = t["token"]
def ids(tok):
    out = json.loads(tool(A, tok, "ekos_graph_export", {"include_properties": True})[0].json()["result"]["content"][0]["text"])
    return {n["id"]: n for n in out["nodes"]}
all_, vis = ids(full), ids(lim)
hidden = [i for i in all_ if i not in vis]
check(len(hidden) >= 2, f"{len(hidden)} objects are hidden from the limited token")
MARK = ("pay.md", "ACMESECRETPAYMARKER", "Compensation")
def call(tok, name, i):
    r, text = tool(A, tok, name, {"id": i})
    return r.json()["result"], text
leaks, ctrl_ok, arg_errors = [], 0, 0
for i in hidden:
    for name in ("ekos_state", "ekos_neighborhood", "ekos_dependents", "ekos_impact"):
        res, text = call(lim, name, i)
        if "missing required" in text: arg_errors += 1
        if any(m in text for m in MARK): leaks.append((name, text[:200]))
        cres, ctext = call(full, name, i)
        if not cres.get("isError") and any(m in ctext for m in MARK): ctrl_ok += 1
check(arg_errors == 0, "the calls were well-formed", str(arg_errors))
check(ctrl_ok > 0, f"control: the unrestricted token gets hidden text for the same ids ({ctrl_ok} calls)")
check(not leaks, "the limited token gets nothing for a hidden object's id", str(leaks[:2]))
# visible neighbours must not name a hidden id
blob = ""
for i in vis:
    for name in ("ekos_state", "ekos_neighborhood", "ekos_dependents", "ekos_impact"):
        blob += call(lim, name, i)[1]
blob += tool(A, lim, "ekos_ekl", {"query": "FIND Relationship"})[1] + tool(A, lim, "ekos_diff", {"from": "2020-01-01T00:00:00Z"})[1]
named = [i for i in hidden if i in blob]
check(not named and not any(m in blob for m in MARK), f"no visible object's answer names a hidden id or its text ({len(blob)} bytes)", str(named))
sys.exit(summary())
