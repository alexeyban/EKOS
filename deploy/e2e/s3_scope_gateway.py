"""Stage 3: a restricted viewer, API tokens, the MCP gateway, the audit log."""

import json
import sys

from lib import check, mcp, sign_in, summary, tool

SECRET = "ACMESECRETPAYMARKER"
alice, bob, carol, dave = (sign_in(f"{n}@example.com") for n in ("alice", "bob", "carol", "dave"))
WS = "acme.shop"
A = "/mcp/acme/shop"
G = "/mcp/globex/billing"

# ── a person limited to part of a project ────────────────────────────────────
r = alice.post("/api/tenants/acme/bindings", json={"email": "dave@example.com", "role": "viewer", "workspace_id": WS})
check(r.status_code in (200, 201), "alice grants dave viewer on acme.shop", f"{r.status_code} {r.text[:200]}")
r = alice.post("/api/tenants/acme/bindings", json={
    "email": "dave@example.com", "role": "viewer", "workspace_id": WS,
    "effect": "deny", "source": "handbook", "path": "hr/**"})
check(r.status_code in (200, 201), "… and denies him handbook hr/**", f"{r.status_code} {r.text[:200]}")
r = alice.post("/api/tenants/acme/bindings", json={
    "email": "dave@example.com", "role": "operator", "workspace_id": WS, "source": "api"})
check(r.status_code == 400, "a rule on an operator grant is refused", f"{r.status_code} {r.text[:200]}")

ws = [w for w in dave.get("/api/workspaces").json()]
check(len(ws) == 1 and ws[0]["id"] == WS and ws[0]["restricted"] is True, "dave lists acme.shop, marked restricted", str(ws))

queries = ["compensation bands", SECRET, "pay", "band seven confidential", "onboarding", "customers orders"]
seen_d, seen_c = "", ""
for q in queries:
    seen_d += dave.get(f"/api/workspaces/{WS}/search", params={"q": q, "limit": 100}).text
    seen_c += carol.get(f"/api/workspaces/{WS}/search", params={"q": q, "limit": 100}).text
gd = dave.get(f"/api/workspaces/{WS}/graph", params={"include_properties": "true"})
gc = carol.get(f"/api/workspaces/{WS}/graph", params={"include_properties": "true"})
check(gd.status_code == 200 and gc.status_code == 200, "both read the graph", f"{gd.status_code} {gc.status_code}")
seen_d += gd.text
seen_c += gc.text
check("pay.md" in seen_c or SECRET in seen_c, "control: unrestricted carol sees the pay page")
check("pay.md" not in seen_d and SECRET not in seen_d and "Compensation" not in seen_d,
      "restricted dave sees nothing of handbook/hr in search or graph")
check("onboarding" in seen_d.lower() and "customers" in seen_d, "… and still sees the rest (onboarding, customers)")
nd, nc = len(gd.json().get("nodes", [])), len(gc.json().get("nodes", []))
check(0 < nd < nc, f"dave's graph is smaller than carol's ({nd} < {nc})")

# every object carol can name, asked for by id as dave
hidden = [n for n in gc.json()["nodes"] if n["id"] not in {m["id"] for m in gd.json()["nodes"]}]
print("   hidden from dave:", len(hidden), "objects")
leaks = []
for n in hidden:
    for path in (f"objects/{n['id']}", f"neighborhood/{n['id']}", f"impact/{n['id']}"):
        got = dave.get(f"/api/workspaces/{WS}/{path}")
        if got.status_code == 200 and (SECRET in got.text or "pay.md" in got.text):
            leaks.append((path, got.text[:120]))
check(not leaks, "asking for each hidden object by id returns nothing of it", str(leaks[:3]))

closed = {p: dave.get(f"/api/workspaces/{WS}/{p}").status_code
          for p in ("stats", "config", "conflicts", "semantics/items", "health", "stats/queries")}
closed["runs"] = dave.get("/api/runs", params={"workspace_id": WS}).status_code
closed["status"] = dave.post(f"/api/workspaces/{WS}/commands/status", json={}).status_code
closed["ekl"] = dave.post(f"/api/workspaces/{WS}/commands/ekl", json={"query": "FIND Object"}).status_code
print("   closed to dave:", closed)
check(all(c == 403 for k, c in closed.items() if k != "runs"), "pages a scope cannot cover are 403 for dave", str(closed))
runs = dave.get("/api/runs", params={"workspace_id": WS})
check(runs.status_code == 403 or runs.json() in ([], {"runs": []}), "dave gets no run history", f"{runs.status_code} {runs.text[:200]}")

# ── tokens ───────────────────────────────────────────────────────────────────
r = alice.post("/api/tenants/acme/tokens", json={"name": "bad", "role": "reviewer"})
check(r.status_code == 422, "a reviewer token cannot be created", str(r.status_code))
r = alice.post("/api/tenants/acme/tokens", json={"name": "bad", "role": "admin"})
check(r.status_code == 422, "an admin token cannot be created", str(r.status_code))


def new_token(c, tenant, name, role, ws=None):
    body = {"name": name, "role": role}
    if ws:
        body["workspace_id"] = ws
    r = c.post(f"/api/tenants/{tenant}/tokens", json=body)
    r.raise_for_status()
    return r.json()


tv = new_token(alice, "acme", "agent-viewer", "viewer")
to = new_token(alice, "acme", "agent-operator", "operator")
tr = new_token(alice, "acme", "agent-limited", "viewer", WS)
tg = new_token(bob, "globex", "globex-agent", "viewer")
print("   token response keys:", sorted(tv))
T = lambda t: t.get("token") or t.get("value")  # noqa: E731
check(all(str(T(t)).startswith("ekos_") for t in (tv, to, tr, tg)), "tokens are issued once, as ekos_…")
listed = alice.get("/api/tenants/acme/tokens").text
check(T(tv) not in listed and T(tv)[12:] not in listed, "the token list does not hold token values")

for role in ("reviewer", "admin"):
    r = alice.post("/api/tenants/acme/bindings", json={"token_id": to["id"], "role": role})
    check(r.status_code in (400, 422), f"a token cannot be granted {role}", f"{r.status_code} {r.text[:150]}")
r = alice.post("/api/tenants/acme/bindings", json={
    "token_id": tr["id"], "role": "viewer", "workspace_id": WS,
    "effect": "deny", "source": "handbook", "path": "hr/**"})
check(r.status_code in (200, 201), "the limited token is denied handbook hr/**", f"{r.status_code} {r.text[:200]}")

# a token on the console API
import httpx  # noqa: E402
from lib import BASE  # noqa: E402

h = {"Authorization": f"Bearer {T(tv)}"}
check(httpx.get(BASE + f"/api/workspaces/{WS}/stats", headers=h).status_code == 200, "a viewer token reads the API")
codes = {
    "run": httpx.post(BASE + f"/api/workspaces/{WS}/commands/build", headers=h, json={}).status_code,
    "admin": httpx.get(BASE + "/api/tenants/acme/bindings", headers=h).status_code,
    "other": httpx.get(BASE + "/api/workspaces/globex.billing/stats", headers=h).status_code,
}
check(codes == {"run": 403, "admin": 403, "other": 404}, "… and is refused run (403), admin (403), globex (404)", str(codes))
ho = {"Authorization": f"Bearer {T(to)}"}
codes = {
    "review": httpx.post(BASE + f"/api/workspaces/{WS}/semantics/items/x/review", headers=ho, json={"decision": "confirm"}).status_code,
    "resolve": httpx.post(BASE + f"/api/workspaces/{WS}/conflicts/x/resolve", headers=ho, json={"decision": "a", "reason": "r"}).status_code,
    "admin": httpx.post(BASE + "/api/tenants/acme/tokens", headers=ho, json={"name": "x", "role": "viewer"}).status_code,
}
check(set(codes.values()) == {403}, "an operator token cannot review or administer", str(codes))

# ── the gateway ──────────────────────────────────────────────────────────────
check(mcp(A, None, "initialize").status_code == 401, "gateway without a token is 401")
check(mcp(A, "ekos_" + "0" * 40, "initialize").status_code == 401, "gateway with an unknown token is 401")
check(mcp(G, T(tv), "initialize").status_code == 404, "acme's token on globex's project is 404")
check(mcp("/mcp/acme/nope", T(tv), "initialize").status_code == 404, "… the same as a project that does not exist")
check(mcp(A, T(tg), "tools/list").status_code == 404, "globex's token on acme's project is 404")

r = mcp(A, T(tv), "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "e2e", "version": "0"}})
check(r.status_code == 200 and r.json()["result"]["serverInfo"]["name"] == "ekos-gateway", "initialize answers", r.text[:200])
r = httpx.post(BASE + A, headers={**h, "Accept": "text/event-stream", "Content-Type": "application/json"},
               content=json.dumps({"jsonrpc": "2.0", "id": 9, "method": "ping"}), timeout=60)
check(r.headers.get("content-type", "").startswith("text/event-stream") and '"id":9' in r.text.replace(" ", ""), "an SSE client gets an event stream", r.text[:200])
r = httpx.post(BASE + A, headers={**h, "Accept": "application/json", "Content-Type": "application/json"},
               content=json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}), timeout=60)
check(r.status_code == 202, "a notification is 202", str(r.status_code))
check(httpx.get(BASE + A, headers=h).status_code == 405, "GET is 405")


def names(token):
    r = mcp(A, token, "tools/list")
    return {t["name"] for t in r.json()["result"]["tools"]}


nv, no, nr = names(T(tv)), names(T(to)), names(T(tr))
print("   viewer tools:", len(nv), " operator:", len(no), " limited:", len(nr), sorted(nr))
check("ekos_search" in nv and not ({"ekos_identity_review", "ekos_architecture_review"} & (nv | no)),
      "no token is offered a review tool")
check("ekos_session_note" not in nv, "a viewer token is not offered the note tool")
check(nr < nv and "ekos_status" not in nr and "ekos_conflicts" not in nr, "the limited token is offered the scoped tools only")

r, text = tool(A, T(tv), "ekos_search", {"query": "compensation bands", "limit": 20})
check(r.status_code == 200 and "pay.md" in text, "viewer token: search finds the pay page", text[:300])
r, text = tool(A, T(tv), "ekos_search", {"query": "invoices GLOBEXMARKER", "limit": 20})
check("GLOBEXMARKER_note" not in text and "billing" not in text.lower(), "… and nothing of globex", text[:300])
r, text = tool(G, T(tg), "ekos_search", {"query": "invoices", "limit": 20})
check("invoices" in text and "customers" not in text, "globex's token finds invoices in its own project", text[:300])

for t, label in ((T(tv), "viewer"), (T(to), "operator")):
    r, text = tool(A, t, "ekos_identity_review", {"relationship_id": "x", "decision": "confirm"})
    body = r.json().get("result", {})
    check(body.get("isError") is True and "review" in text, f"{label} token calling a review tool is refused", text[:200])
r, text = tool(A, T(tv), "ekos_session_note", {"text": "x"})
check(r.json().get("result", {}).get("isError") is True, "viewer token calling the note tool is refused", text[:200])

# the limited token: every scoped tool, looking for the hidden page
out = ""
for q in queries:
    if q == SECRET:
        continue  # `ekos_retrieve` echoes the question in its plan; a marker we sent proves nothing
    for name, args in (("ekos_search", {"query": q, "limit": 50}), ("ekos_query", {"question": q}),
                       ("ekos_retrieve", {"question": q})):
        out += tool(A, T(tr), name, args)[1]
for q in ("FIND Object", "FIND Relationship", "FIND Object WHERE kind = 'Section'", "FIND Object WHERE kind = 'File'",
          "FIND Object WHERE kind = 'Document'"):
    out += tool(A, T(tr), "ekos_ekl", {"query": q})[1]
out += tool(A, T(tr), "ekos_graph_export", {"include_properties": True})[1]
# Hidden objects asked for by id are in s4_ids.py, which also runs a control.
ctrl = tool(A, T(tv), "ekos_ekl", {"query": "FIND Object"})[1] + tool(A, T(tv), "ekos_graph_export", {"include_properties": True})[1]
check("pay.md" in ctrl, "control: the unrestricted token's EKL and graph export hold the pay page")
check("pay.md" not in out and SECRET not in out and "Compensation" not in out,
      f"the limited token gets nothing of handbook/hr from any scoped tool ({len(out)} bytes read)")
check("onboarding" in out.lower() and "customers" in out, "… and still gets the rest")
for name in ("ekos_status", "ekos_conflicts", "ekos_audit"):
    r, text = tool(A, T(tr), name, {})
    check(r.json().get("result", {}).get("isError") is True and "restricted" in text, f"the limited token is refused {name}", text[:200])
# a scope the client supplies itself must not widen its own
r = mcp(A, T(tr), "tools/call", {"name": "ekos_ekl", "arguments": {"query": "FIND Object"},
                                 "_meta": {"ekos/readScope": {"default": "allow", "rules": []}}})
check("pay.md" not in r.text and SECRET not in r.text, "a scope sent by the client does not replace its own")

# ── audit ────────────────────────────────────────────────────────────────────
log = alice.get("/api/tenants/acme/audit", params={"limit": 500})
text = log.text
check(log.status_code == 200 and "mcp: ekos_search" in text and "agent-limited" in text, "acme's audit log records the gateway calls by token name", text[:300])
check("denied" in text and "mcp: ekos_identity_review" in text, "… including the refused review calls")
check("compensation" not in text and SECRET not in text, "… and not the arguments")
check("globex" not in text.lower(), "… and nothing of globex")
check(carol.get("/api/tenants/acme/audit").status_code == 403, "a viewer cannot read the audit log")

# revoke
r = alice.delete(f"/api/tenants/acme/tokens/{tv['id']}")
check(r.status_code in (200, 204), "alice revokes the viewer token", str(r.status_code))
check(mcp(A, T(tv), "tools/list").status_code == 401, "the revoked token is 401 on its next call")

# removing a role takes effect on the next request
bs = alice.get("/api/tenants/acme/bindings").json()
mine = [b for b in bs if "carol" in json.dumps(b)]
for b in mine:
    alice.delete(f"/api/tenants/acme/bindings/{b['id']}")
check(carol.get(f"/api/workspaces/{WS}/stats").status_code == 404, "carol, her role removed, gets 404 with the same session", str(mine)[:200])
check(carol.get("/api/workspaces/globex.billing/stats").status_code == 200, "… and still reads globex")

sys.exit(summary())
