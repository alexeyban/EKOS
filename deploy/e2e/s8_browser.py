"""The console pages in a real browser (Chromium, headless): sign in through the provider's
form, open every page, and fail on a script error, a failed request or an empty page.

Skipped, with a line saying so, when no Chromium is installed for Playwright
(`uv run --with playwright playwright install chromium`)."""

import glob
import os
import sys

from playwright.sync_api import sync_playwright

from lib import BASE, PASSWORD, check, summary

SHOTS = os.environ.get("EKOS_E2E_SHOTS", "")
# 401 before sign-in and 403/404 on pages closed to a role are answers, not faults.
EXPECTED = {("alice", 404, "/evals/reports")}


def launch(p):
    args = ["--host-resolver-rules=MAP dex 127.0.0.1"]
    try:
        return p.chromium.launch(args=args)
    except Exception:
        # A Chromium from another Playwright version is good enough for this.
        found = sorted(glob.glob(os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome")))
        if not found:
            return None
        return p.chromium.launch(args=args, executable_path=found[-1])


def session(browser, who: str, pages: list[tuple[str, str]], must_fail: set[int] = frozenset()):
    ctx = browser.new_context(viewport={"width": 1400, "height": 900})
    page = ctx.new_page()
    errors: list[str] = []
    bad: list[str] = []
    page.on("pageerror", lambda e: errors.append(str(e)[:200]))
    page.on("console", lambda m: errors.append(m.text[:200]) if m.type == "error" and "Failed to load resource" not in m.text else None)

    def on_response(r):
        if r.status >= 400 and BASE in r.url:
            path = r.url.replace(BASE, "")
            if r.status in must_fail or r.status == 401 and "/auth/me" in path:
                return
            if any(w == who and s == r.status and frag in path for w, s, frag in EXPECTED):
                return
            bad.append(f"{r.status} {path[:90]}")

    page.on("response", on_response)

    page.goto(BASE + "/")
    page.get_by_role("button", name="Sign in").click()
    page.get_by_text("Log in with Email").click()
    page.fill('input[name="login"]', f"{who}@example.com")
    page.fill('input[name="password"]', PASSWORD)
    page.click('button[type="submit"]')
    page.wait_for_url(BASE + "/**")
    page.wait_for_load_state("networkidle")
    check(f"{who}@example.com" in page.content(), f"{who} signs in through the browser and the page names them")

    for path, expect in pages:
        errors.clear()
        bad.clear()
        page.goto(BASE + path)
        page.wait_for_load_state("networkidle")
        page.wait_for_timeout(400)
        body = page.inner_text("body")
        if SHOTS:
            page.screenshot(path=f"{SHOTS}/{who}{path.replace('/', '_') or '_home'}.png")
        ok = expect.lower() in body.lower() and not errors and not bad
        check(ok, f"{who}: {path or '/'} shows “{expect}”, no script error, no failed request",
              f"errors={errors[:3]} failed={bad[:4]} text={body[:160]!r}")
    ctx.close()


with sync_playwright() as p:
    browser = launch(p)
    if browser is None:
        print("SKIP  no Chromium for Playwright; the pages were not opened in a browser")
        sys.exit(0)
    W = "/w/acme.shop"
    session(browser, "alice", [
        ("/", "shop"), (W, "Dashboard"), (W + "/graph", "Graph"), (W + "/run", "pipeline"),
        (W + "/runs", "pipeline"), (W + "/evals", "Evals"), (W + "/semantics", "Semantics"),
        (W + "/conflicts", "Conflicts"), (W + "/config", "Validate"), ("/schedules", "Schedule"),
        ("/admin", "API tokens"),
    ])
    # A limited viewer: the graph and nothing else of the workspace.
    session(browser, "dave", [("/", "shop"), (W + "/graph", "Graph")])
    # Nobody's project: the page says so instead of breaking.
    session(browser, "bob", [("/", "billing"), ("/w/acme.shop", "unknown workspace")], must_fail={404})
    browser.close()

sys.exit(summary())
