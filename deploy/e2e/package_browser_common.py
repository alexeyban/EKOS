"""Helpers shared by the browser stages of the package check."""

import glob
import os
import sys

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


def finish(errors: list[str]) -> int:
    check(not errors, "no script errors", "; ".join(errors)[:300])
    bad = [w for ok, w in results if not ok]
    print(f"\n{len(results) - len(bad)} passed, {len(bad)} failed")
    return 1 if bad else 0
