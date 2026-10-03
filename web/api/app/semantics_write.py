"""Human review of business-semantics hypotheses (RFC 0170 Phase 2/4) — the console's write seam.

Promotion is human-only in EKOS: `ekos semantics confirm|reject|edit` and `ekos import linkml`
are CLI commands that no MCP tool can reach. The console is a human surface, so it runs exactly
those CLI commands — argv lists, never a shell — on behalf of a signed-in **write**-role user, and
records who that user is with `--as`:

* OIDC: the authenticated email (or subject) — an identity.
* token mode: there is no identity, only a shared token. The reviewer types a name, and it is
  recorded as `token:<name>` so the ledger never presents an unauthenticated claim as a person.

These are quick single appends, so they bypass the job queue; if a pipeline run holds the
workspace's ledger write lock, the CLI fails fast and the route answers 409.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import re
import tempfile
from pathlib import Path
from typing import Any, Literal

from . import _proc
from .auth import Principal

Action = Literal["confirm", "reject", "edit"]

_UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
_TIMEOUT = 30.0
_MAX_TEXT = 2000
_MAX_YAML = 8 << 20  # 8 MiB


class ReviewError(RuntimeError):
    """The request is invalid (→ 400/422)."""


class LedgerBusy(RuntimeError):
    """Another process holds the workspace's write lock (→ 409)."""


class CliFailed(RuntimeError):
    """The CLI refused or failed (→ 400 with its message)."""


def reviewer_for(principal: Principal, oidc: bool, typed_name: str | None) -> str:
    """The `--as` value: an OIDC identity, or a marked token-mode claim."""
    if oidc:
        who = principal.email or principal.subject
        if not who:
            raise ReviewError("the signed-in user has no email or subject to record")
        return who
    name = (typed_name or "").strip()
    if not name:
        raise ReviewError("token mode has no identity: give a reviewer name")
    if len(name) > 80 or not re.fullmatch(r"[\w .@'-]+", name):
        raise ReviewError("reviewer name: letters, digits, spaces and . @ ' - only, ≤ 80 chars")
    return f"token:{name}"


def _clean(field: str, value: str | None) -> str | None:
    if value is None:
        return None
    value = value.strip()
    if not value:
        return None
    if len(value) > _MAX_TEXT:
        raise ReviewError(f"{field} is longer than {_MAX_TEXT} characters")
    if "\x00" in value:
        raise ReviewError(f"{field} contains a NUL byte")
    return value


def review_argv(
    action: Action,
    item_id: str,
    reviewer: str,
    *,
    note: str | None = None,
    name: str | None = None,
    description: str | None = None,
    label: str | None = None,
) -> list[str]:
    """The exact argv for one decision. Every value is its own argv element after a flag, so no
    text can become an option: a value starting with `-` is passed with `--flag=value`."""
    if action not in ("confirm", "reject", "edit"):
        raise ReviewError(f"unknown action {action!r}")
    if not _UUID.match(item_id):
        raise ReviewError("item id must be a UUID")
    note, name, description, label = (
        _clean("note", note),
        _clean("name", name),
        _clean("description", description),
        _clean("label", label),
    )
    if action == "reject" and not note:
        raise ReviewError("a rejection needs a note")
    if action == "edit" and not (name or description or label):
        raise ReviewError("an edit needs a name, description or label")
    if action != "edit" and (name or description or label):
        raise ReviewError("name/description/label are only for an edit")
    argv = ["semantics", action, item_id, f"--as={reviewer}"]
    for flag, value in (
        ("--note", note),
        ("--name", name),
        ("--description", description),
        ("--label", label),
    ):
        if value is not None:
            argv.append(f"{flag}={value}")
    return argv


async def _run(ekos_bin: str, workspace_path: str, argv: list[str]) -> tuple[int, str, str]:
    root = Path(workspace_path).resolve()
    if not root.is_dir():
        raise ReviewError(f"workspace path is not a directory: {root}")
    proc = await _proc.spawn([ekos_bin, *argv], cwd=str(root))
    try:
        out, err = await asyncio.wait_for(proc.communicate(), timeout=_TIMEOUT)
    except TimeoutError:
        await _proc.terminate(proc)
        raise CliFailed(f"`ekos {argv[0]} {argv[1]}` timed out") from None
    return proc.returncode or 0, out.decode(errors="replace"), err.decode(errors="replace")


def _raise_for(code: int, stderr: str) -> None:
    if code == 0:
        return
    text = _proc.strip_ansi(stderr).strip()
    low = text.lower()
    if "lock" in low and ("held" in low or "busy" in low or "another" in low):
        raise LedgerBusy(text[-500:])
    raise CliFailed(text[-800:] or f"exited {code}")


async def review(ekos_bin: str, workspace_path: str, argv: list[str]) -> str:
    code, out, err = await _run(ekos_bin, workspace_path, argv)
    _raise_for(code, err)
    return out.strip()


async def import_linkml(
    ekos_bin: str, workspace_path: str, yaml_text: str, *, dry_run: bool, reviewer: str | None
) -> dict[str, Any]:
    """`ekos import linkml <tmp> --json [--dry-run] [--as=…]` on the edited text. The text goes to
    a private temp file outside the workspace (0600 via mkstemp) and is deleted afterwards."""
    if len(yaml_text.encode()) > _MAX_YAML:
        raise ReviewError("schema is larger than 8 MiB")
    if not dry_run and not reviewer:
        raise ReviewError("applying an import needs a reviewer")
    fd, tmp = tempfile.mkstemp(prefix="ekos-linkml-", suffix=".yaml")
    try:
        with os.fdopen(fd, "w") as f:
            f.write(yaml_text)
        argv = ["import", "linkml", tmp, "--json"]
        if dry_run:
            argv.append("--dry-run")
        if reviewer:
            argv.append(f"--as={reviewer}")
        code, out, err = await _run(ekos_bin, workspace_path, argv)
        # With --json the plan is printed even when it holds errors; trust it over the exit code.
        try:
            plan = json.loads(out)
        except json.JSONDecodeError:
            _raise_for(code or 1, err)
            raise CliFailed("import printed no plan") from None
        return plan
    finally:
        with contextlib.suppress(FileNotFoundError):
            Path(tmp).unlink()
