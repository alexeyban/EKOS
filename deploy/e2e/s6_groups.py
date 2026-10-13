"""A group whose members are whoever the provider says is in it (RFC 0180)."""

import sys

from lib import check, sign_in, summary

alice = sign_in("alice@example.com")
kilgore = sign_in(connector="mock")
me = kilgore.get("/api/auth/me").json()
check(me["email"] == "kilgore@kilgore.trout" and me["tenants"] == [], "a person signs in through a second connector and has no role", str(me))

r = alice.post("/api/tenants/acme/groups", json={"name": "writers", "oidc_value": "authors"})
check(r.status_code in (200, 201), "alice creates a group for the claim value `authors`", f"{r.status_code} {r.text[:200]}")
gid = r.json()["id"]
r = alice.post("/api/tenants/acme/bindings", json={"group_id": gid, "role": "viewer"})
check(r.status_code in (200, 201), "… and grants it viewer", f"{r.status_code} {r.text[:200]}")

me = kilgore.get("/api/auth/me").json()
check([(t["id"], t["roles"]) for t in me["tenants"]] == [("acme", ["viewer"])],
      "the person, never listed as a member, is a viewer in acme by the provider's group claim", str(me))
check([w["id"] for w in kilgore.get("/api/workspaces").json()] == ["acme.shop"], "… and lists acme.shop")
check(kilgore.post("/api/workspaces/acme.shop/commands/build", json={}).status_code == 403, "… and cannot run the pipeline")
check(kilgore.get("/api/workspaces/globex.billing/stats").status_code == 404, "… and sees nothing of globex")

r = alice.delete(f"/api/tenants/acme/groups/{gid}")
check(r.status_code in (200, 204), "alice deletes the group", str(r.status_code))
check(kilgore.get("/api/workspaces").json() == [], "the person's access ends with the group, on the same session")
sys.exit(summary())
