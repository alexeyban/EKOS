"""Package check, stage 4: the setup wizard in a real browser (headless Chromium)."""

import glob
import os
import sys

from playwright.sync_api import sync_playwright

LINK = os.environ["E2E_WIZARD"]
HOME2 = os.environ["E2E_HOME2"]
PORT2 = os.environ["E2E_PORT2"]
SOURCE = os.environ["E2E_SOURCE"]


def chromium(p):
    try:
        return p.chromium.launch()
    except Exception:  # noqa: BLE001 — fall back to a Chromium Playwright cached earlier
        found = sorted(glob.glob(os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome")))
        real_home = os.environ.get("E2E_REAL_HOME", "")
        if not found and real_home:
            found = sorted(glob.glob(f"{real_home}/.cache/ms-playwright/chromium-*/chrome-linux*/chrome"))
        if not found:
            print("SKIP  no Chromium for Playwright")
            sys.exit(0)
        return p.chromium.launch(executable_path=found[-1])


errors: list[str] = []
with sync_playwright() as p:
    browser = chromium(p)
    page = browser.new_page()
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(LINK)
    page.wait_for_selector("#home")
    page.fill("#home", HOME2)
    page.fill("#program_dir", "")
    page.uncheck("#add_to_path")
    page.click("#next")  # → Console
    page.select_option("#mode", "local")
    page.fill("#port", PORT2)
    page.click("#next")  # → Project
    page.check("#want-project")
    page.fill("#project_name", "wiz")
    page.fill("#project_sources", SOURCE)
    page.click("#next")  # → Agents
    page.click("#next")  # → Chat
    page.click("#next")  # → Start
    page.uncheck("#at_login")
    page.uncheck("#open")
    page.click("#next")  # → Review
    page.wait_for_selector("#plan li")
    plan = page.inner_text("#plan")
    print("PASS  the review lists the steps" if "create project wiz" in plan else f"FAIL  review: {plan}")
    page.click("#next")  # Install
    page.wait_for_selector("#done-title:text-matches('set up|Stopped|failed')", timeout=120_000)
    title = page.inner_text("#done-title")
    steps = page.inner_text("#steps")
    ok = title == "EKOS is set up"
    print(("PASS  " if ok else "FAIL  ") + f"the wizard finished: {title}")
    if not ok:
        print(steps)
    page.click("#next")  # Close → finish
    browser.close()

if errors:
    print("FAIL  script errors on the page:", errors)
sys.exit(0 if ok and not errors and "create project wiz" in plan else 1)
