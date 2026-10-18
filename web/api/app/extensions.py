"""Console pages declared by extensions (RFC 0198), from `<ekos_bin> extensions --json`.

The binary has already checked each page against its own catalogue. The console checks again
before running anything: a panel's argv must start with a catalogue command of class `read`, and
whatever follows must be that command's flags. The argv comes from the binary's declaration;
the only values from a request are the page's selections a panel declares (RFC 0207), each
passed as one `--name=value` to a value option of that read command.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

from . import _proc, catalogue, envs

_cached: dict[str, Any] | None = None


class PanelError(ValueError):
    pass


async def load(ekos_bin: str) -> dict[str, Any]:
    """The pages, read once from the binary. A binary older than RFC 0198 has none."""
    global _cached
    if _cached is None:
        proc = await _proc.spawn([ekos_bin, "extensions", "--json"], env=envs.for_tenant(None))
        out, _err = await asyncio.wait_for(proc.communicate(), timeout=30)
        _cached = json.loads(out) if proc.returncode == 0 else {"extensions": []}
    return _cached


def pages(spec: dict[str, Any]) -> list[dict[str, Any]]:
    """Every page, each with the extension it came from."""
    return [
        {**page, "extension": ext["name"]}
        for ext in spec.get("extensions", [])
        for page in ext.get("pages", [])
    ]


def find_panel(
    spec: dict[str, Any], ext: str, page_id: str, panel_id: str
) -> dict[str, Any] | None:
    for page in pages(spec):
        if page["extension"] == ext and page["id"] == page_id:
            return next((p for p in page["panels"] if p["id"] == panel_id), None)
    return None


MAX_SELECTION = 512


class SelectionError(ValueError):
    """A panel needs a selection the request does not give, or gives a bad value."""


def panel_argv(
    cat: dict[str, Any], panel: dict[str, Any], selections: dict[str, str] | None = None
) -> list[str]:
    """The panel's argv when the catalogue says it only reads; `PanelError` otherwise. RFC 0207:
    each selection the panel declares is appended as `--name=value` (`SelectionError` when one
    is missing or not plain text)."""
    selections = selections or {}
    argv = [str(a) for a in panel.get("command", [])]
    base_len = len(argv)
    matches = [
        e
        for e in cat["commands"]
        if len(e["path"]) <= len(argv) and argv[: len(e["path"])] == e["path"]
    ]
    if not matches:
        raise PanelError(f"`{' '.join(argv)}` is not a command of this binary")
    entry = max(matches, key=lambda e: len(e["path"]))
    if entry["class"] != "read":
        raise PanelError(f"`{catalogue.describe_command(entry)}` is not a read command")
    flags = {a["long"] for a in entry["args"] if a["kind"] == "flag" and a.get("long")}
    for extra in argv[len(entry["path"]) : base_len]:
        if not (extra.startswith("--") and extra[2:] in flags):
            raise PanelError(f"`{extra}` is not a flag of `{catalogue.describe_command(entry)}`")
    wanted = [str(p) for p in panel.get("params", [])]
    options = {a["long"] for a in entry["args"] if a["kind"] == "value" and a.get("long")}
    for name in wanted:
        if name not in options:
            raise PanelError(
                f"`{catalogue.describe_command(entry)}` has no value option `--{name}`"
            )
        value = selections.get(name)
        if value is None or value == "":
            raise SelectionError(f"choose a {name} first")
        if len(value) > MAX_SELECTION or "\x00" in value:
            raise SelectionError(f"{name}: at most {MAX_SELECTION} characters, no NUL")
        argv.append(f"--{name}={value}")
    if any(a["required"] and a.get("long") not in wanted for a in entry["args"]):
        raise PanelError(f"`{catalogue.describe_command(entry)}` needs arguments")
    return argv
