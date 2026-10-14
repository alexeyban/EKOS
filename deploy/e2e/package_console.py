"""Package check, stage 3: the console an installed package started (local mode)."""

import os
import sys
import time

import httpx

BASE = os.environ["E2E_BASE"]
LINK = os.environ["E2E_LINK"]
results: list[tuple[bool, str]] = []


def check(ok: bool, what: str, detail: str = "") -> None:
    results.append((ok, what))
    print(("PASS  " if ok else "FAIL  ") + what + (f"   [{detail}]" if detail and not ok else ""))


c = httpx.Client(base_url=BASE, timeout=60)
r = c.get(LINK.replace(BASE, ""), follow_redirects=False)
check(r.status_code == 303, "the one-time link signs in", str(r.status_code))
me = c.get("/api/auth/me").json()
check(me.get("kind") == "local" and me.get("owner") is True, "as the local owner", str(me))
check(c.get(LINK.replace(BASE, "")).status_code == 401, "the link works once")
check(c.get("/").status_code == 200 and "root" in c.get("/").text, "the UI is served")
check(
    httpx.get(BASE + "/api/health", headers={"Host": "evil.example"}).status_code == 403,
    "a foreign Host is refused",
)

# A setting changed from the web and applied by a restart.
r = c.put("/api/platform/settings", json={"values": {"mcp_max_servers": 3}})
check(r.status_code == 200, "an owner saves a setting", r.text[:200])
r = c.post("/api/platform/restart")
check(r.status_code == 202, "and restarts the console", r.text[:200])
time.sleep(2)
for _ in range(60):
    try:
        if c.get("/api/health").status_code == 200:
            break
    except httpx.HTTPError:
        pass
    time.sleep(0.5)
fields = c.get("/api/platform/settings").json()["fields"]
value = next(f for f in fields if f["name"] == "mcp_max_servers")
check(value["value"] == 3 and value["source"] == "file", "the new value is in force", str(value))

# The project setup created, compiled from the web.
ws = [w["id"] for w in c.get("/api/workspaces").json()]
check("default.shop" in ws or "shop" in ws, "the project is in the console", str(ws))
wid = "default.shop" if "default.shop" in ws else "shop"
r = c.post(f"/api/workspaces/{wid}/commands/pipeline", json={})
run_id = r.json().get("run_id", "")
status = ""
for _ in range(300):
    status = c.get(f"/api/runs/{run_id}").json().get("status", "")
    if status not in ("queued", "running"):
        break
    time.sleep(1)
check(status == "succeeded", "the pipeline runs from the console", status)
r = c.get(f"/api/workspaces/{wid}/search", params={"q": "PACKAGEMARKER customers"})
check(r.status_code == 200 and "README" in r.text, "search finds the source", r.text[:200])

# RFC 0186: a command outside the old allowlist, from the binary's own catalogue.
listed = c.get(f"/api/workspaces/{wid}/cli").json()["commands"]
names = {" ".join(e["path"]) for e in listed}
check({"query find", "semantics confirm", "coverage"} <= names, "every command is listed", "")
check("console start" not in names, "commands that are not run from the web are not listed")
r = c.post(
    f"/api/workspaces/{wid}/cli",
    json={"command": ["query", "find"], "args": {"query": "PACKAGEMARKER"}},
)
check(r.status_code == 200, "a command outside the old list runs", r.text[:200])
run_id = r.json().get("run_id", "")
status = ""
for _ in range(120):
    status = c.get(f"/api/runs/{run_id}").json().get("status", "")
    if status not in ("queued", "running"):
        break
    time.sleep(0.5)
log = c.get(f"/api/runs/{run_id}/logs").text
check(status == "succeeded" and "README" in log, "and finds the source", f"{status} {log[:200]}")
r = c.post(f"/api/workspaces/{wid}/cli", json={"command": ["clean"], "args": {}})
check(r.status_code == 409, "a destructive command needs its name typed", str(r.status_code))

# RFC 0186: a setting changed through the settings form, saved through the validated write.
schema = c.get("/api/config-schema").json()
check("llm" in schema.get("properties", {}), "the config schema is served")
cfg = c.get(f"/api/workspaces/{wid}/config").json()
r = c.post(
    f"/api/workspaces/{wid}/config/edit",
    json={"raw": cfg["raw"], "changes": [{"path": ["semantics", "min-sites"], "value": 3}]},
)
check(r.status_code == 200, "the form's change is applied to the text", r.text[:200])
saved = c.put(f"/api/workspaces/{wid}/config", json={"raw": r.json()["raw"]})
check(saved.status_code == 200, "and saved after validation", saved.text[:300])
after = c.get(f"/api/workspaces/{wid}/config").json()
check(after["values"].get("semantics", {}).get("min-sites") == 3, "the file holds it")

bad = [w for ok, w in results if not ok]
print(f"\n{len(results) - len(bad)} passed, {len(bad)} failed")
sys.exit(1 if bad else 0)
