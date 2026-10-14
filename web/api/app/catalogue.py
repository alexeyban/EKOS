"""Every `ekos` command, from `ekos commands --json` (RFC 0186 §1).

The binary describes its own commands: arguments, their kinds, and a class that says who may
run each from the web. The console turns a person's form into an argv against that description
and nothing else:

* only a command and arguments the catalogue lists; never a console-managed one (which project,
  home, config, read scope — the console sets those);
* options as `--long=value`, positionals after `--`, so no value can pose as an option;
* a path argument must resolve inside the project's directory or the tenant's source roots;
* for a review, the reviewer (`by`) is the signed-in person, whatever the form said.

It is never a shell: the argv goes to `create_subprocess_exec` through the job runner.
"""

from __future__ import annotations

import asyncio
import json
from pathlib import Path
from typing import Any

from . import _proc, envs, roots

# class → the action `authz` checks.
ACTIONS = {
    "read": "read",
    "write": "operate",
    "review": "review",
    "admin": "admin",
    "external": "admin",
}
CONFIRMED = frozenset({"admin", "external"})
# Set by the console for a review; a person cannot pass someone else's name.
REVIEWER_ARG = "by"

_cached: dict[str, Any] | None = None
_schema: dict[str, Any] | None = None


class ArgvError(ValueError):
    pass


async def load(ekos_bin: str) -> dict[str, Any]:
    """The catalogue, read once from the binary."""
    global _cached
    if _cached is None:
        proc = await _proc.spawn([ekos_bin, "commands", "--json"], env=envs.for_tenant(None))
        out, err = await asyncio.wait_for(proc.communicate(), timeout=30)
        if proc.returncode != 0:
            raise RuntimeError(
                f"`ekos commands --json` failed ({proc.returncode}): {err.decode()[:300]}"
            )
        _cached = json.loads(out)
    return _cached


async def load_schema(ekos_bin: str) -> dict[str, Any]:
    """The JSON schema of `ekos.toml`, from `ekos config schema --json`, read once."""
    global _schema
    if _schema is None:
        proc = await _proc.spawn(
            [ekos_bin, "config", "schema", "--json"], env=envs.for_tenant(None)
        )
        out, err = await asyncio.wait_for(proc.communicate(), timeout=30)
        if proc.returncode != 0:
            raise RuntimeError(f"`ekos config schema` failed: {err.decode()[:300]}")
        _schema = json.loads(out)
    return _schema


def find(cat: dict[str, Any], path: list[str]) -> dict[str, Any] | None:
    return next((e for e in cat["commands"] if e["path"] == path), None)


def _inside_any(p: Path, allowed: list[Path]) -> bool:
    return any(roots.inside(p, r) for r in allowed)


def _path_value(raw: str, base: Path, allowed: list[Path], arg_id: str) -> str:
    p = Path(raw).expanduser()
    p = (p if p.is_absolute() else base / p).resolve()
    if not _inside_any(p, allowed):
        raise ArgvError(f"{arg_id}: {raw} is outside the project and its source roots")
    return str(p)


def build_argv(
    entry: dict[str, Any],
    args: dict[str, Any],
    *,
    reviewer: str,
    base: Path,
    allowed: list[Path],
) -> list[str]:
    """The argv for `entry` with `args`, or `ArgvError` saying what is wrong."""
    specs = {a["id"]: a for a in entry["args"]}
    unknown = sorted(set(args) - set(specs))
    if unknown:
        raise ArgvError(f"{', '.join(unknown)}: not an argument of `{' '.join(entry['path'])}`")
    values = dict(args)
    if entry["class"] == "review" and REVIEWER_ARG in specs:
        values[REVIEWER_ARG] = reviewer

    options: list[str] = []
    positionals: list[tuple[int, list[str]]] = []
    for spec in entry["args"]:
        aid = spec["id"]
        value = values.get(aid)
        if value is None or value == "" or value == []:
            if spec["required"]:
                raise ArgvError(f"{aid} is required")
            continue
        if spec["kind"] == "flag":
            if not isinstance(value, bool):
                raise ArgvError(f"{aid} is a flag: true or false")
            if value:
                options.append(f"--{spec['long']}")
            continue
        items = value if isinstance(value, list) else [value]
        if len(items) > 1 and not spec["multiple"]:
            raise ArgvError(f"{aid} takes one value")
        out = []
        for item in items:
            if not isinstance(item, (str, int, float)) or isinstance(item, bool):
                raise ArgvError(f"{aid}: a value is text or a number")
            text = str(item)
            if "\x00" in text:
                raise ArgvError(f"{aid}: a value cannot hold a NUL byte")
            if spec["possible"] and text not in spec["possible"]:
                raise ArgvError(f"{aid} is one of {', '.join(spec['possible'])}")
            if spec["kind"] == "path":
                text = _path_value(text, base, allowed, aid)
            out.append(text)
        if spec["long"]:
            options.extend(f"--{spec['long']}={v}" for v in out)
        else:
            positionals.append((spec["index"] or 0, out))

    argv = list(entry["path"]) + options
    if positionals:
        argv.append("--")
        for _, vals in sorted(positionals, key=lambda p: p[0]):
            argv.extend(vals)
    return argv


def describe_command(entry: dict[str, Any]) -> str:
    return " ".join(entry["path"])
