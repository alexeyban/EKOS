"""RFC 0213 Phase 3 walk-through: the source-model review in a browser, sign-offs, the digest.

Against a running console in token mode whose workspace is a compiled project (the iDempiere
testbed was `idemdocs`). It records real decisions in that project's ledger — a scratch testbed only.

    EKOS_CONSOLE_URL=http://127.0.0.1:18765 EKOS_CONSOLE_TOKEN_VALUE=<write token> \
      uv run --with playwright python review_tour.py <workspace-id> <screenshot-dir>
"""
import os, sys
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "../../../deploy/e2e"))
from package_browser_common import check, chromium, finish
from playwright.sync_api import sync_playwright, expect

BASE = os.environ.get("EKOS_CONSOLE_URL", "http://127.0.0.1:18765")
TOKEN = os.environ["EKOS_CONSOLE_TOKEN_VALUE"]
WS = sys.argv[1]
OUT = sys.argv[2]
LONG = 240_000
errors: list[str] = []

def shot(page, name):
    page.screenshot(path=os.path.join(OUT, f"{name}.png"), full_page=True)

def settle(page):
    page.wait_for_timeout(300)
    page.locator("text=loading…").first.wait_for(state="detached", timeout=LONG)

def sign_state(page, want, what):
    chip = page.get_by_label("sign-off", exact=True).locator(".chip").first
    try:
        expect(chip).to_have_text(want, timeout=LONG)
        check(True, what)
    except AssertionError:
        check(False, what, chip.inner_text())

def outcome(page, box):
    """Wait for the box's result line; return (ok, text)."""
    panel = page.get_by_label(box, exact=True)
    res = panel.locator(".ok-line, .err").first
    res.wait_for(timeout=LONG)
    return "ok-line" in (res.get_attribute("class") or ""), res.inner_text()

def tab(page, subject_label):
    page.get_by_role("tablist").get_by_role("button", name=subject_label).click()
    settle(page)

def sign(page, subject):
    settle(page)
    page.get_by_label("confirm", exact=True).fill(subject)
    page.get_by_role("button", name="Sign off").click()
    ok, text = outcome(page, "sign-off")
    check(ok, f"{subject}: sign-off accepted", text)
    sign_state(page, "valid", f"{subject}: sign-off now valid")

with sync_playwright() as p:
    b = chromium(p)
    page = b.new_page(viewport={"width": 1500, "height": 1000})
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(BASE + "/")
    page.get_by_label("console token").fill(TOKEN)
    page.get_by_role("button", name="Sign in").click()
    page.goto(f"{BASE}/w/{WS}/review")
    page.get_by_role("tablist").wait_for(timeout=LONG)
    bar = page.get_by_label("workflow")
    bar.wait_for(timeout=LONG)
    check("blocked" in bar.inner_text(), "the step bar shows steps 4-7 blocked")
    shot(page, "01-review-scope")

    # A scope decision: an import table out, as proposed.
    page.get_by_label("filter").fill("I_PRODUCT")
    page.get_by_label("select I_PRODUCT", exact=True).check()
    page.get_by_label("reviewer").fill("Ann")
    page.get_by_label("reason", exact=True).fill("import staging table, not business data")
    page.get_by_role("button", name="Out of scope").click()
    ok, text = outcome(page, "decide")
    check(ok, "a scope decision was recorded from the browser", text)
    sign_state(page, "stale", "the scope sign-off turned stale after the decision")
    shot(page, "02-scope-stale-after-decision")
    sign(page, "scope")

    for label, subject in (("PII", "pii"), ("Write behaviour", "write-behaviour"), ("Glossary", "glossary")):
        tab(page, label)
        if subject == "pii":
            shot(page, "03-pii-stale")
        sign(page, subject)
        digest_shown = page.locator(".review-digest").inner_text()
        check(len(digest_shown) == 64, f"{subject}: digest visible")
    shot(page, "04-glossary-signed")

    tab(page, "Identity")
    check(page.get_by_role("button", name="Sign off").count() == 0, "identity cannot be signed with 25 open matches")
    check("blocking item(s) must be decided" in page.get_by_label("decide and sign off").inner_text(),
          "identity says why")
    shot(page, "05-identity-blocked")
    tab(page, "Conflicts")
    check(page.get_by_role("link", name="Conflicts").count() >= 1, "conflicts link to their page")

    page.reload()
    page.get_by_label("workflow").wait_for(timeout=LONG)
    text = page.get_by_label("workflow").inner_text()
    print("BAR:", text.replace("\n", " | "))
    shot(page, "06-step-bar-after")
    b.close()
sys.exit(finish(errors))
