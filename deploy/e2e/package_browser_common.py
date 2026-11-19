"""Helpers shared by the browser stages of the package check."""

import glob
import os
import sys

results: list[tuple[bool, str]] = []


def check(ok: bool, what: str, detail: str = "") -> None:
    results.append((ok, what))
    print(("PASS  " if ok else "FAIL  ") + what + (f"   [{detail}]" if detail and not ok else ""))


def browser(p):
    """The browser the console stages run in: `E2E_BROWSER` = chromium (default), webkit or firefox.

    RFC 0261 U1: the console must work in WebKit too, the engine EKOS.DEV uses on macOS and Linux.
    """
    name = os.environ.get("E2E_BROWSER", "chromium")
    if name not in ("chromium", "webkit", "firefox"):
        sys.exit(f"E2E_BROWSER: chromium, webkit or firefox, not {name!r}")
    try:
        return getattr(p, name).launch()
    except Exception:  # noqa: BLE001 — fall back to a Chromium Playwright cached earlier
        found = sorted(
            glob.glob(os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome"))
        )
        if name != "chromium" or not found:
            print(f"SKIP  no {name} for Playwright (uv run --with playwright playwright install {name})")
            sys.exit(0)
        return p.chromium.launch(executable_path=found[-1])


def finish(errors: list[str]) -> int:
    check(not errors, "no script errors", "; ".join(errors)[:300])
    bad = [w for ok, w in results if not ok]
    print(f"\n{len(results) - len(bad)} passed, {len(bad)} failed")
    return 1 if bad else 0
