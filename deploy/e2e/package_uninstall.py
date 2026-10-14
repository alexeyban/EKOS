"""Package check, stage 5: uninstall from the console's Settings page (RFC 0188).

The button starts `ekos uninstall --ui --detach`; the browser follows the link to the page the
binary serves, which lists the project, takes the confirmation word and removes EKOS."""

import os
import sys

from package_browser_common import check, chromium, finish
from playwright.sync_api import sync_playwright

LINK = os.environ["E2E_LINK"]
BASE = os.environ["E2E_BASE"]

errors: list[str] = []
with sync_playwright() as p:
    browser = chromium(p)
    page = browser.new_page()
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(LINK)
    page.wait_for_selector("header")
    page.goto(f"{BASE}/settings")
    page.click("button:has-text('Uninstall EKOS')")
    page.wait_for_url("http://127.0.0.1:*/?t=*", timeout=30_000)
    check(not page.url.startswith(BASE), "the button opens the uninstall page", page.url)
    page.wait_for_selector("#plan:not(.hidden)")
    check("shop" in page.inner_text("#projects"), "the page lists the project")
    check(page.is_disabled("#go"), "nothing can be removed before the word is typed")
    page.fill("#confirm", "uninstall")
    page.click("#go")
    page.wait_for_selector("#done:not(.hidden)", timeout=60_000)
    title = page.inner_text("#done-title")
    check(title == "EKOS was removed", "the page reports every step done", title + " " + page.inner_text("#results"))
    check("kept" in page.inner_text("#kept"), "and says the projects are kept")
    browser.close()

sys.exit(finish(errors))
