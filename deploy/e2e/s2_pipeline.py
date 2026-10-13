"""Stage 2: pipelines run by each tenant's admin; who sees and may do what."""

import json
import sys

from lib import check, run, sign_in, summary

alice, bob, carol = (sign_in(f"{n}@example.com") for n in ("alice", "bob", "carol"))

ids = [w["id"] for w in alice.get("/api/workspaces").json()]
check(ids == ["acme.shop"], "the project made with the CLI is listed for alice after a restart", str(ids))
ids = [w["id"] for w in bob.get("/api/workspaces").json()]
check(ids == ["globex.billing"], "bob lists globex.billing only", str(ids))
ids = sorted(w["id"] for w in carol.get("/api/workspaces").json())
check(ids == ["acme.shop", "globex.billing"], "carol, a viewer in both, lists both", str(ids))

for who, c, ws in (("alice", alice, "acme.shop"), ("bob", bob, "globex.billing")):
    cfg = c.get(f"/api/workspaces/{ws}/config")
    raw = cfg.json().get("raw", "") if cfg.status_code == 200 else ""
    print(f"--- {ws} ekos.toml ---\n{raw}\n---")
    if "[llm" in raw or "api-key" in raw:
        print("config names a model; stopping before the pipeline")
        sys.exit(2)
    got = run(c, ws, "pipeline")
    ok = check(got.get("status") == "succeeded", f"{who} runs the pipeline on {ws}", json.dumps(got)[:400])
    if not ok:
        print(c.get(f"/api/runs/{got.get('id', got.get('run_id', ''))}/logs").text[-3000:])

# Cross-tenant: 404, never 403.
for who, c, other in (("alice", alice, "globex.billing"), ("bob", bob, "acme.shop")):
    codes = {
        "stats": c.get(f"/api/workspaces/{other}/stats").status_code,
        "search": c.get(f"/api/workspaces/{other}/search", params={"q": "invoices"}).status_code,
        "config": c.get(f"/api/workspaces/{other}/config").status_code,
        "run": c.post(f"/api/workspaces/{other}/commands/build", json={}).status_code,
        "delete": c.delete(f"/api/workspaces/{other}").status_code,
    }
    check(set(codes.values()) == {404}, f"{who} gets 404 for everything in {other}", str(codes))
    t = other.split(".")[0]
    codes = {
        "audit": c.get(f"/api/tenants/{t}/audit").status_code,
        "bindings": c.get(f"/api/tenants/{t}/bindings").status_code,
        "tokens": c.post(f"/api/tenants/{t}/tokens", json={"name": "x", "role": "viewer"}).status_code,
    }
    check(set(codes.values()) == {404}, f"{who} gets 404 for tenant {t}'s admin pages", str(codes))

# Search finds each tenant's own text and never the other's.
a = alice.get("/api/workspaces/acme.shop/search", params={"q": "ACMESECRETPAYMARKER compensation bands", "limit": 50})
check(a.status_code == 200 and "pay.md" in a.text, "alice's search finds the handbook's pay page", a.text[:300])
check("GLOBEXMARKER" not in a.text and "invoices" not in a.text, "… and nothing of globex")
b = bob.get("/api/workspaces/globex.billing/search", params={"q": "invoices", "limit": 50})
check(b.status_code == 200 and "invoices" in b.text, "bob's search finds invoices", b.text[:300])
b2 = bob.get("/api/workspaces/globex.billing/search", params={"q": "customers orders compensation onboarding", "limit": 50})
check("pay.md" not in b2.text and "customers" not in b2.text.replace("customers orders", ""), "… and nothing of acme", b2.text[:300])

# A viewer reads and does nothing else.
r = carol.get("/api/workspaces/acme.shop/stats")
check(r.status_code == 200, "carol reads acme.shop stats", str(r.status_code))
codes = {
    "run": carol.post("/api/workspaces/acme.shop/commands/build", json={}).status_code,
    "config": carol.put("/api/workspaces/acme.shop/config", json={"raw": "x"}).status_code,
    "resolve": carol.post("/api/workspaces/acme.shop/conflicts/x/resolve", json={"decision": "a", "reason": "r"}).status_code,
    "review": carol.post("/api/workspaces/acme.shop/semantics/items/x/review", json={"decision": "confirm"}).status_code,
    "grant": carol.post("/api/tenants/acme/bindings", json={"email": "carol@example.com", "role": "admin"}).status_code,
    "token": carol.post("/api/tenants/acme/tokens", json={"name": "x", "role": "operator"}).status_code,
    "delete": carol.delete("/api/workspaces/acme.shop").status_code,
}
check(set(codes.values()) == {403}, "carol is refused every write in acme (403)", str(codes))
r = carol.post("/api/workspaces/acme.shop/commands/status", json={})
check(r.status_code == 200, "carol may run a read command", f"{r.status_code} {r.text[:200]}")

# An operator-only person cannot review; a reviewer-only person cannot operate.
sys.exit(summary())
