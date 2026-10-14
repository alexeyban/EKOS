"""Package check, stage 3b: the Commands page and the settings form in a browser (RFC 0186)."""

import glob
import os
import sys

from playwright.sync_api import sync_playwright

LINK = os.environ["E2E_LINK"]
BASE = os.environ["E2E_BASE"]
results: list[tuple[bool, str]] = []


def check(ok: bool, what: str, detail: str = "") -> None:
    results.append((ok, what))
    print(("PASS  " if ok else "FAIL  ") + what + (f"   [{detail}]" if detail and not ok else ""))


def chromium(p):
    try:
        return p.chromium.launch()
    except Exception:  # noqa: BLE001 — fall back to a Chromium Playwright cached earlier
        found = sorted(
            glob.glob(os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome"))
        )
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
    page.wait_for_selector("header")
    ids = [w["id"] for w in page.request.get(f"{BASE}/api/workspaces").json()]
    WID = next(i for i in ids if i.endswith("shop"))

    page.goto(f"{BASE}/w/{WID}/commands")
    page.wait_for_selector("text=query find")
    page.click("button:has-text('query find')")
    page.fill("#arg-query", "PACKAGEMARKER")
    check("ekos query find" in page.inner_text("pre.argv"), "the form previews the command")
    page.click("section.commands-form button:has-text('Run')")
    try:
        page.wait_for_url("**/runs/**", timeout=30_000)
    except Exception:
        print("the form says:", page.inner_text("section.commands-form")[-600:])
        raise
    check("/runs/" in page.url, "running it opens the run")
    page.wait_for_selector("text=README", timeout=60_000)
    check(True, "the run's log shows the result")

    page.goto(f"{BASE}/w/{WID}/config")
    page.wait_for_selector("text=Settings")
    page.click("summary:has-text('[semantics]')")
    page.fill("#cfg-semantics\\.max-enum-values", "40")
    page.click("button:has-text('Apply 1 change')")
    page.wait_for_function("document.querySelector('textarea.toml').value.includes('max-enum-values = 40')")
    check(True, "the form's change lands in the text")
    page.click("button:has-text('Validate')")
    page.wait_for_selector("text=valid ✓", timeout=30_000)
    page.click("button.save")
    page.wait_for_selector("text=saved", timeout=30_000)
    check(True, "and is validated and saved")
    browser.close()

check(not errors, "no script errors", "; ".join(errors)[:300])
bad = [w for ok, w in results if not ok]
print(f"\n{len(results) - len(bad)} passed, {len(bad)} failed")
sys.exit(1 if bad else 0)
