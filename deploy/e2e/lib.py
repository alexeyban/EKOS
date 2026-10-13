"""Helpers for the end-to-end check: sign in through Dex, call the console, call the gateway."""

from __future__ import annotations

import html
import json
import os
import re
import socket
import time
from urllib.parse import urljoin

import httpx

BASE = "http://127.0.0.1:" + os.environ.get("EKOS_E2E_PORT", "8009")
PASSWORD = "password"

# The issuer is `http://dex:5556/dex`, which the console's container resolves on the Compose
# network. From the host that name is the published loopback port.
_real = socket.getaddrinfo


def _getaddrinfo(host, *a, **kw):
    if host == "dex":
        host = "127.0.0.1"
    return _real(host, *a, **kw)


socket.getaddrinfo = _getaddrinfo

RESULTS: list[tuple[bool, str]] = []


def check(ok: bool, what: str, detail: str = "") -> bool:
    RESULTS.append((bool(ok), what))
    print(("PASS  " if ok else "FAIL  ") + what + (f"   [{detail}]" if detail and not ok else ""))
    return bool(ok)


def summary() -> int:
    bad = [w for ok, w in RESULTS if not ok]
    print(f"\n{len(RESULTS) - len(bad)} passed, {len(bad)} failed")
    for w in bad:
        print("  FAILED:", w)
    return 1 if bad else 0


def _follow(c: httpx.Client, r: httpx.Response, stop_at_console: bool = False) -> httpx.Response:
    for _ in range(12):
        if r.status_code not in (301, 302, 303, 307):
            break
        loc = urljoin(str(r.url), r.headers["location"])
        if stop_at_console and loc.startswith(BASE):
            break
        r = c.get(loc)
    return r


def sign_in(email: str | None = None, connector: str = "local") -> httpx.Client:
    """A client holding a session cookie obtained through the real OIDC flow. `local` is Dex's
    password form for `email`; `mock` is its mock connector, one fixed person with a group."""
    c = httpx.Client(base_url=BASE, timeout=120)
    r = _follow(c, c.get("/api/auth/login"), stop_at_console=True)
    if 'name="password"' not in r.text:
        # Several connectors: Dex shows a page of links, one per connector.
        m = re.search(rf'href="([^"]*/auth/{connector}[?/][^"]*)"', r.text)
        if m is None:
            raise RuntimeError(f"no link for connector {connector!r} at {r.url}")
        r = _follow(c, c.get(urljoin(str(r.url), html.unescape(m.group(1)))))
    if connector == "local":
        m = re.search(r'<form[^>]*action="([^"]+)"', r.text)
        action = urljoin(str(r.url), html.unescape(m.group(1))) if m else str(r.url)
        r = _follow(c, c.post(action, data={"login": email, "password": PASSWORD}))
    me = c.get("/api/auth/me")
    if me.status_code != 200:
        raise RuntimeError(
            f"sign-in as {email or connector} failed: {me.status_code} {me.text[:200]} "
            f"(last {r.status_code} {r.url})"
        )
    return c


def run(c: httpx.Client, ws: str, name: str, body: dict | None = None, wait: int = 300) -> dict:
    r = c.post(f"/api/workspaces/{ws}/commands/{name}", json=body or {})
    r.raise_for_status()
    run_id = r.json()["run_id"]
    end = time.time() + wait
    while time.time() < end:
        got = c.get(f"/api/runs/{run_id}").json()
        if got.get("status") not in ("queued", "running"):
            return got
        time.sleep(1)
    raise TimeoutError(run_id)


def mcp(path: str, token: str | None, method: str, params: dict | None = None, rid: int = 1) -> httpx.Response:
    headers = {"Accept": "application/json", "Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    payload = {"jsonrpc": "2.0", "id": rid, "method": method, "params": params or {}}
    return httpx.post(BASE + path, headers=headers, content=json.dumps(payload), timeout=120)


def tool(path: str, token: str, name: str, args: dict | None = None) -> tuple[httpx.Response, str]:
    r = mcp(path, token, "tools/call", {"name": name, "arguments": args or {}})
    return r, r.text
