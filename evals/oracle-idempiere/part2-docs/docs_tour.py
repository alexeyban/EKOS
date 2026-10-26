"""RFC 0230 Phase 2 exit: step 2 on the Documentation page — Generate and Preview publish in the
browser. Publish itself is not clicked: a real publish to a public wiki waits for the maintainer."""
import json, os, subprocess, sys, time
sys.path.insert(0, "/home/legion/PycharmProjects/EKOS/deploy/e2e")
from package_browser_common import check, chromium, finish
from playwright.sync_api import sync_playwright

BASE = "http://127.0.0.1:18765"
WS = "idemdocs"
OUT = sys.argv[1]
LONG = 180_000
errors: list[str] = []
os.makedirs(OUT, exist_ok=True)


def shot(page, name):
    page.screenshot(path=os.path.join(OUT, f"{name}.png"))


def open_page(page):
    page.goto(f"{BASE}/w/{WS}/x/lakehouse/docs")
    page.get_by_text("Home page").first.wait_for(timeout=LONG)
    page.locator('[aria-label="workflow"]').first.wait_for(timeout=LONG)
    page.wait_for_timeout(1500)


def wait_run(page):
    page.wait_for_url("**/runs/**", timeout=LONG)
    run_id = page.url.rstrip("/").split("/")[-1]
    for _ in range(900):
        run = page.request.get(f"{BASE}/api/runs/{run_id}").json()
        if run["status"] not in ("queued", "running"):
            return run_id, run
        time.sleep(1)
    return run_id, run


def action(page, button):
    open_page(page)
    page.get_by_role("button", name=button, exact=True).click()
    typed = page.locator('[aria-label^="type "][aria-label$=" to confirm"]')
    if typed.count() > 0:
        typed.fill(typed.get_attribute("aria-label")[len("type "):-len(" to confirm")])
    run = page.get_by_role("button", name="Run", exact=True)
    try:
        run.wait_for(timeout=3000)
        run.click()
    except Exception:
        pass
    run_id, r = wait_run(page)
    check(r["status"] == "succeeded", f"{button} ran", json.dumps({k: r.get(k) for k in ("status", "exit_code")}))
    events = page.request.get(f"{BASE}/api/runs/{run_id}/events").json()
    return r, events


with sync_playwright() as p:
    b = chromium(p)
    page = b.new_page(viewport={"width": 1500, "height": 1100})
    page.set_default_timeout(LONG)
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(BASE + "/")
    page.locator('[aria-label="console token"]').fill("w-token-review-e2e")
    page.get_by_role("button", name="Sign in").click()
    page.get_by_text("Add a workspace").wait_for(timeout=LONG)

    open_page(page)
    body = page.inner_text("body")
    check("Documentation" in body and "github" in body, "the page shows the store")
    check("alexeyban/EKOS/wiki" in body, "the destination is shown before any publish")
    check("GITHUB_TOKEN" in body, "it names where the credentials come from, never a value")
    check("python3" in body and "ready" in body, "the publisher check")
    check("Data model" in body, "the generated Home page is rendered", body[-600:])
    check("ekos:manual" not in body, "no manual-section markers shown as text")
    bar = page.locator('[aria-label="workflow"]').first.inner_text()
    check("Documentation" in bar, "step 2 is on the bar", bar)
    shot(page, "01-docs-page")

    r, ev = action(page, "Generate")
    check(any(e.get("stage") == "docs" and e.get("msg") == "estate pages written" for e in ev),
          "Generate's run log says what it wrote")
    check(any(e.get("stage") == "llm" and e.get("count") == 0 for e in ev), "Generate made no model call")

    r, ev = action(page, "Preview publish")
    dry = [e for e in ev if e.get("stage") == "docs" and e.get("msg") == "dry run"]
    check(bool(dry) and dry[0].get("ok") is True, "the preview ran the publisher in dry-run mode", json.dumps(ev[-4:]))
    log = page.request.get(f"{BASE}/api/runs/{page.url.rstrip('/').split('/')[-1]}").json().get("log_tail", [])
    check(any("(dry run)" in l for l in log), "the publisher said it sent nothing", "\n".join(log[-3:]))
    shot(page, "02-preview-run")

    open_page(page)
    body = page.inner_text("body")
    check("Last preview" in body and "ok " in body, "the preview is recorded on the page")
    check(page.get_by_role("button", name="Publish", exact=True).count() == 1, "Publish is offered")
    shot(page, "03-after-preview")

    out = subprocess.run([os.environ["EKOS_BIN"], "--project", WS, "workflow", "status", "--json"],
                         capture_output=True, text=True).stdout
    step2 = [s for s in json.loads(out)["steps"] if s["n"] == 2][0]
    print("STEP 2:", step2["state"], "—", step2["detail"])
    check(step2["state"] == "partial" and "not published" in step2["detail"],
          "a preview does not make step 2 done", step2["detail"])
    b.close()
sys.exit(finish(errors))
