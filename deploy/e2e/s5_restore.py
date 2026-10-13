import sys
from lib import check, mcp, sign_in, summary, tool, run
alice, bob, carol, dave, owner = (sign_in(f"{n}@example.com") for n in ("alice","bob","carol","dave","owner"))
check([w["id"] for w in alice.get("/api/workspaces").json()] == ["acme.shop"], "after restore alice lists acme.shop")
check([w["id"] for w in carol.get("/api/workspaces").json()] == ["globex.billing"], "carol's roles are as they were at backup")
r = alice.get("/api/workspaces/acme.shop/search", params={"q": "compensation bands", "limit": 50})
check("pay.md" in r.text, "alice's search over the restored ledger finds the pay page", r.text[:200])
r = dave.get("/api/workspaces/acme.shop/search", params={"q": "compensation bands", "limit": 50})
check(r.status_code == 200 and "pay.md" not in r.text, "dave's limit survived the restore")
r = bob.get("/api/workspaces/globex.billing/search", params={"q": "invoices"})
check("invoices" in r.text, "bob's search works")
toks = alice.get("/api/tenants/acme/tokens").json()
check(len(toks) >= 4, "acme's tokens are listed", str(len(toks)))
a = alice.get("/api/tenants/acme/audit", params={"limit": 500}).text
check("mcp: ekos_search" in a, "the audit log came back")
ts = owner.get("/api/tenants").json()
acme = [t for t in ts if t["id"] == "acme"][0]
check([v["name"] for v in acme.get("model_env", [])] == ["ACME_MODEL_KEY"] and acme.get("allow_metered") is True, "tenant model settings came back", str(acme))
t = alice.post("/api/tenants/acme/tokens", json={"name": "after", "role": "viewer"}).json()["token"]
r, text = tool("/mcp/acme/shop", t, "ekos_search", {"query": "compensation bands", "limit": 20})
check(r.status_code == 200 and "pay.md" in text, "the gateway serves the restored project", text[:200])
got = run(alice, "acme.shop", "pipeline")
check(got.get("status") == "succeeded", "the pipeline runs again on the restored project", str(got)[:300])
sys.exit(summary())
