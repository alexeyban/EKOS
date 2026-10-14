"""Package check, stage 3b: the Commands page and the settings form in a browser (RFC 0186)."""

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
    ids = [w["id"] for w in page.request.get(f"{BASE}/api/workspaces").json()]
    WID = next(i for i in ids if i.endswith("shop"))

    # RFC 0189: the one button in the project's bar.
    page.goto(f"{BASE}/w/{WID}")
    page.once("dialog", lambda d: d.accept())
    page.click("button:has-text('Init & compile')")
    page.wait_for_url("**/runs/**", timeout=30_000)
    page.wait_for_selector("text=every input kind produced objects", timeout=300_000)
    check(True, "Init & compile runs every stage through coverage")

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

    # RFC 0187: the Chat page with the local model.
    try:
        import httpx

        ollama_up = httpx.get("http://127.0.0.1:11434/api/tags", timeout=3).status_code == 200
    except Exception:  # noqa: BLE001
        ollama_up = False
    if ollama_up:
        page.goto(f"{BASE}/w/{WID}/chat")
        page.wait_for_selector("textarea[aria-label=message]")
        check("not paid per call" in page.inner_text("section.chat-main"), "the page says the model is local")
        page.fill("textarea[aria-label=message]", "Use the search tool to find PACKAGEMARKER.")
        page.click("button:has-text('Send')")
        page.wait_for_selector(".chat-answer", timeout=180_000)
        check(page.locator(".chat-tool").count() >= 1, "the page shows the tool calls")
        check(len(page.inner_text(".chat-answer").strip()) > 0, "and the answer")
    else:
        print("SKIP  chat page: Ollama does not answer")
    browser.close()

sys.exit(finish(errors))
