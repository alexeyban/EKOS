"""One-time sign-in codes for local mode (RFC 0184 §3).

Whoever can write the home's control directory owns the install. `issue` (the console's own
helper; `ekos console open` writes the same thing from Rust) creates `local-signin/<sha256>`,
an empty file named by the hash of a random code. `redeem` accepts a code whose file exists and
is younger than `MAX_AGE`, and deletes it: a code works once. Only the hash is on disk.
"""

from __future__ import annotations

import contextlib
import hashlib
import secrets
import time
from pathlib import Path

DIR = "local-signin"
MAX_AGE = 120.0  # seconds


def _hash(code: str) -> str:
    return hashlib.sha256(code.encode()).hexdigest()


def issue(control_dir: Path) -> str:
    d = control_dir / DIR
    d.mkdir(parents=True, exist_ok=True)
    code = secrets.token_urlsafe(24)
    (d / _hash(code)).touch()
    return code


def _sweep(d: Path, now: float) -> None:
    for f in d.iterdir():
        with contextlib.suppress(OSError):
            if now - f.stat().st_mtime > MAX_AGE:
                f.unlink()


def redeem(control_dir: Path, code: str, now: float | None = None) -> bool:
    d = control_dir / DIR
    if not code or not d.is_dir():
        return False
    now = time.time() if now is None else now
    f = d / _hash(code)
    try:
        fresh = now - f.stat().st_mtime <= MAX_AGE
        # Unlinking is the use: two requests racing for one code cannot both succeed.
        f.unlink()
    except OSError:
        fresh = False
    _sweep(d, now)
    return fresh
