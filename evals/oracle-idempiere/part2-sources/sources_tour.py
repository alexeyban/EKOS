"""RFC 0223 Phase 2 exit: empty project → sources → database (password in the console) → test →
Init & compile → census vs compiled, all in a browser. Phase A starts the run; phase B checks.

    EKOS_CONSOLE_URL=… EKOS_CONSOLE_TOKEN_VALUE=<write token> IDEMPIERE_SRC=<checkout> \
    IDEMPIERE_DOCS=<docs checkout> ORACLE_CONNECT=localhost:51521/XEPDB1 EKOS_RO_PASSWORD=… \
      uv run --with playwright python sources_tour.py a <out>   # then, after the run: … b <out>
"""
import json, os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../../deploy/e2e"))
from package_browser_common import check, chromium, finish
from playwright.sync_api import sync_playwright, expect

BASE = os.environ.get("EKOS_CONSOLE_URL", "http://127.0.0.1:18765")
WS = "idemconsole"
OUT = sys.argv[2]
PHASE = sys.argv[1]
LONG = 300_000
errors: list[str] = []


def shot(page, name):
    page.screenshot(path=os.path.join(OUT, f"{name}.png"), full_page=True)


def sign_in(page):
    page.goto(BASE + "/")
    page.get_by_label("console token").fill(os.environ["EKOS_CONSOLE_TOKEN_VALUE"])
    page.get_by_role("button", name="Sign in").click()
    page.get_by_text("Add a workspace").wait_for(timeout=LONG)


def test_db(page):
    box = page.get_by_label("database idempiere-db", exact=True)
    box.get_by_role("button", name="Test connection").click()
    box.get_by_label("census vs compiled").wait_for(timeout=LONG)
    return box


with sync_playwright() as p:
    b = chromium(p)
    page = b.new_page(viewport={"width": 1500, "height": 1000})
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.on("dialog", lambda d: d.accept())
    sign_in(page)
    if PHASE == "a":
        page.get_by_placeholder("id").fill(WS)
        page.get_by_placeholder("name").fill("iDempiere (console)")
        page.get_by_placeholder("/abs/path/to/sources").fill(os.environ["IDEMPIERE_SRC"])
        page.get_by_role("button", name="Add", exact=True).click()
        page.get_by_text("iDempiere (console)").first.wait_for(timeout=LONG)
        check(True, "a project created from a source directory")
        page.goto(f"{BASE}/w/{WS}/sources")
        page.get_by_text("Directories").wait_for(timeout=LONG)
        page.get_by_label("source name").fill("docs")
        page.get_by_label("source path").fill(os.environ["IDEMPIERE_DOCS"])
        page.get_by_label("source kind").select_option("git")
        page.get_by_role("button", name="Add directory").click()
        page.get_by_text("Added source docs").wait_for(timeout=LONG)
        expect(page.get_by_text(os.environ["IDEMPIERE_SRC"], exact=True)).to_be_visible(timeout=LONG)
        check(True, "two named directory sources (the root named first)")
        page.get_by_text("Add an Oracle database").click()
        page.get_by_label("database name").fill("idempiere-db")
        page.get_by_label("connect").fill(os.environ.get("ORACLE_CONNECT", "localhost:51521/XEPDB1"))
        page.get_by_label("user", exact=True).fill("ekos_ro")
        page.get_by_label("password").fill(os.environ["EKOS_RO_PASSWORD"])
        page.get_by_label("owners").fill("ADEMPIERE")
        page.get_by_label("profile").select_option("p2")
        page.get_by_label("dictionary").select_option("idempiere")
        page.get_by_role("button", name="Add database").click()
        page.get_by_text("Added database idempiere-db").wait_for(timeout=LONG)
        check(page.get_by_label("password").input_value() == "", "the password field is cleared")
        expect(page.get_by_text("password stored")).to_be_visible(timeout=LONG)
        box = test_db(page)
        text = box.inner_text()
        check("writes refused (read-only)" in text, "the test connects read-only", text[:300])
        check("not compiled yet" in text, "before analysis nothing is compiled", text[:300])
        shot(page, "01-sources-before-analysis")
        page.get_by_role("button", name="Init & compile").click()
        page.wait_for_url("**/runs/**", timeout=LONG)
        run_id = page.url.rstrip("/").split("/")[-1]
        open(os.path.join(OUT, "run_id"), "w").write(run_id)
        check(True, f"analysis started: run {run_id}")
        shot(page, "02-run-started")
    else:
        run_id = open(os.path.join(OUT, "run_id")).read().strip()
        run = page.request.get(f"{BASE}/api/runs/{run_id}").json()
        check(run["status"] == "succeeded", "the analysis run succeeded", json.dumps({k: run[k] for k in ("status", "exit_code")}))
        page.goto(f"{BASE}/runs/{run_id}")
        page.wait_for_timeout(3000)
        shot(page, "03-run-finished")
        page.goto(f"{BASE}/w/{WS}/sources")
        page.get_by_text("Directories").wait_for(timeout=LONG)
        box = test_db(page)
        rows = box.get_by_label("census vs compiled").inner_text()
        print("CENSUS:", rows.replace("\n", " | "))
        check("differs" not in rows and "not compiled yet" not in rows, "census equals compiled for every kind", rows)
        bar = page.get_by_label("workflow")
        bar.wait_for(timeout=LONG)
        check("Analyse sources" in bar.inner_text() and "done" in bar.inner_text(), "step 1 is done on the bar", bar.inner_text()[:200])
        shot(page, "04-sources-after-analysis")
    b.close()
sys.exit(finish(errors))
