"""Stage 1: owner sets up two tenants; a tenant admin creates a project through the API."""

import sys

import httpx

from lib import BASE, check, sign_in, summary

r = httpx.get(BASE + "/api/workspaces")
check(r.status_code == 401, "no session: the workspace list is 401", str(r.status_code))
r = httpx.post(BASE + "/api/auth/token-login", json={"token": "ekos-dev-token"})
check(r.status_code == 400, "static-token sign-in is refused", f"{r.status_code} {r.text[:100]}")

owner = sign_in("owner@example.com")
me = owner.get("/api/auth/me").json()
check(me["owner"] is True and me["kind"] == "user", "owner signs in through the provider and is an owner", str(me))

for tid, name in (("acme", "Acme"), ("globex", "Globex")):
    r = owner.post("/api/tenants", json={"id": tid, "name": name, "source_roots": [f"/sources/{tid}"]})
    check(r.status_code in (200, 201), f"owner creates tenant {tid}", f"{r.status_code} {r.text[:200]}")
    r = owner.patch(
        f"/api/tenants/{tid}",
        json={"model_env": [f"{tid.upper()}_MODEL_KEY"], "allow_metered": True},
    )
    check(r.status_code == 200, f"owner names {tid}'s model variable", f"{r.status_code} {r.text[:200]}")

grants = [("acme", "alice@example.com", "admin"), ("globex", "bob@example.com", "admin"),
          ("acme", "carol@example.com", "viewer"), ("globex", "carol@example.com", "viewer")]
for tid, email, role in grants:
    r = owner.post(f"/api/tenants/{tid}/bindings", json={"email": email, "role": role})
    check(r.status_code in (200, 201), f"owner grants {role} in {tid} to {email}", f"{r.status_code} {r.text[:200]}")

# Someone with no role at all.
dave = sign_in("dave@example.com")
me = dave.get("/api/auth/me").json()
check(me["tenants"] == [] and not me["owner"], "a signed-in person with no role sees no tenant", str(me))
check(dave.get("/api/workspaces").json() == [], "… and no workspace")
r = dave.post("/api/tenants", json={"id": "evil", "name": "x"})
check(r.status_code in (403, 404), "… and cannot create a tenant", str(r.status_code))

alice = sign_in("alice@example.com")
me = alice.get("/api/auth/me").json()
check([t["id"] for t in me["tenants"]] == ["acme"], "alice sees tenant acme only", str(me))
r = alice.patch("/api/tenants/acme", json={"source_roots": ["/sources"]})
check(r.status_code in (403, 404), "a tenant admin cannot widen the tenant's source roots", f"{r.status_code} {r.text[:200]}")
r = alice.patch("/api/tenants/acme", json={"model_env": ["GLOBEX_MODEL_KEY"]})
check(r.status_code in (403, 404), "a tenant admin cannot name model variables", f"{r.status_code} {r.text[:200]}")
r = alice.post("/api/workspaces", json={"id": "steal", "name": "x", "source": "/sources/globex/billing", "tenant": "acme"})
check(r.status_code in (400, 403), "alice cannot create a project over globex's sources", f"{r.status_code} {r.text[:200]}")
r = alice.post("/api/workspaces", json={"id": "steal", "name": "x", "source": "/sources/globex/billing", "tenant": "globex"})
check(r.status_code == 404, "alice creating a project in globex is 404", f"{r.status_code} {r.text[:200]}")
r = alice.post("/api/workspaces", json={"id": "etc", "name": "x", "source": "/etc", "tenant": "acme"})
check(r.status_code in (400, 403), "alice cannot create a project over /etc", f"{r.status_code} {r.text[:200]}")

bob = sign_in("bob@example.com")
r = bob.post("/api/workspaces", json={"id": "billing", "name": "Billing", "source": "/sources/globex/billing", "tenant": "globex"})
check(r.status_code == 201, "bob creates project globex/billing through the API", f"{r.status_code} {r.text[:300]}")
if r.status_code == 201:
    print("   ", r.json()["id"], r.json()["path"])

sys.exit(summary())
