"""Command allowlist (RFC 0127 §8.4, RFC 0131 §2).

"Run EKOS commands from a browser" is a remote-code-execution surface. The rules:

1. This hardcoded list is the ONLY way to run anything. No endpoint accepts a command string.
2. Never a shell — the runner uses `create_subprocess_exec` with `argv` built from `base_argv`
   plus validated params.
3. The only string params are literals passed as a single argv element (never split, never
   interpolated). Path params do not exist in this allowlist — every command runs with
   `cwd = <registered workspace root>`.
4. `is_write` commands require the `write` role.
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Param:
    kind: str  # "bool" | "string"
    required: bool = False
    help: str = ""


@dataclass(frozen=True)
class Command:
    name: str
    base_argv: tuple[str, ...]
    is_write: bool = False
    timeout: float = 1800.0
    params: dict[str, Param] = field(default_factory=dict)
    # For chained runs: the ordered sub-commands. Empty = a single subprocess of `base_argv`.
    stages: tuple[str, ...] = ()
    summary: str = ""

    def render_argv(self, params: dict) -> list[str]:
        """`base_argv` + the validated params as argv elements. `ValueError` on a bad param."""
        argv = list(self.base_argv)
        for pname, spec in self.params.items():
            if pname not in params or params[pname] in (None, "", False):
                if spec.required:
                    raise ValueError(f"missing required parameter {pname!r}")
                continue
            value = params[pname]
            if spec.kind == "bool":
                if value is True:
                    argv.append(f"--{pname}")
            elif spec.kind == "string":
                if not isinstance(value, str) or "\x00" in value:
                    raise ValueError(f"parameter {pname!r} must be a string")
                argv += [f"--{pname}", value]
            else:  # pragma: no cover - unreachable given the specs below
                raise ValueError(f"unknown param kind {spec.kind!r}")
        return argv


_PIPELINE_STAGES = ("build", "recover", "resolve", "compile", "commit")

COMMAND_ALLOWLIST: list[Command] = [
    Command("doctor", ("doctor",), summary="Environment + config checks"),
    Command("status", ("status",), summary="Ledger entry / object counts"),
    Command("ledger-status", ("ledger", "status", "--storage"), summary="Storage breakdown"),
    Command(
        "graph-export",
        ("graph", "export", "--format", "json"),
        summary="Export the whole compiled graph as JSON",
    ),
    Command(
        "ekl",
        ("ekl",),
        params={"query": Param("string", required=True, help="An EKL query")},
        summary="Run an Enterprise Knowledge Language query",
    ),
    Command("build", ("build",), is_write=True, summary="Observe enterprise systems"),
    Command(
        "recover",
        ("recover",),
        is_write=True,
        params={"parallel": Param("bool", help="Run DAG-independent passes concurrently")},
        summary="Knowledge-recovery passes → KIR",
    ),
    Command(
        "resolve",
        ("resolve",),
        is_write=True,
        params={"force": Param("bool", help="Report conflicts but don't fail the pipeline")},
        summary="Identity resolution",
    ),
    Command("compile", ("compile",), is_write=True, summary="Semantic compiler → CKM"),
    Command("commit", ("commit", "--yes"), is_write=True, summary="Commit the CKM to the ledger"),
    Command(
        "pipeline",
        (),
        is_write=True,
        stages=_PIPELINE_STAGES,
        summary="build → recover → resolve → compile → commit",
    ),
    Command("clean", ("clean",), is_write=True, summary="Clear the artifact cache"),
    Command("ledger-repair", ("ledger", "repair"), is_write=True, summary="Verify sealed segments"),
    Command(
        "artifact-repack", ("artifact", "repack"), is_write=True, summary="Repack loose artifacts"
    ),
    Command(
        "docs-generate",
        ("docs", "generate", "--layout", "curated", "--output", "doc"),
        is_write=True,
        summary="Generate curated Markdown docs into doc/",
    ),
]

BY_NAME: dict[str, Command] = {c.name: c for c in COMMAND_ALLOWLIST}


def catalogue() -> list[dict]:
    """The UI-facing description of every allowlisted command."""
    return [
        {
            "name": c.name,
            "summary": c.summary,
            "is_write": c.is_write,
            "stages": list(c.stages),
            "params": {
                k: {"kind": p.kind, "required": p.required, "help": p.help}
                for k, p in c.params.items()
            },
        }
        for c in COMMAND_ALLOWLIST
    ]
