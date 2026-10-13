"""Projects kept in the EKOS home (RFC 0178, RFC 0179 §2).

A registered workspace is either a directory holding `ekos.toml` and `.ekos/` (in place), or a
project directory `<home>/data/<tenant>/<project>` holding `ekos.toml` and `state/`. The two are
run differently, and this module is the one place that knows how: every `ekos` command line the
console builds comes from `command` or `mcp_command`.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from pathlib import Path

from .settings import get_settings

DEFAULT_TENANT = "default"

# The same rule as `ekos_compiler_core::is_valid_name`. A name becomes a directory name and a
# command-line argument, so it is checked before either is built from it.
_NAME = re.compile(r"[a-z0-9][a-z0-9_-]{0,62}")


def is_valid_name(name: str) -> bool:
    return _NAME.fullmatch(name) is not None


@dataclass(frozen=True)
class Project:
    home: Path
    tenant: str
    name: str

    @property
    def dir(self) -> Path:
        return self.home / "data" / self.tenant / self.name

    @property
    def state_dir(self) -> Path:
        return self.dir / "state"

    @property
    def config_path(self) -> Path:
        return self.dir / "ekos.toml"


def configured_home() -> Path | None:
    """The EKOS home this console serves (`EKOS_HOME`), or `None`: then no path is a project."""
    raw = get_settings().ekos_home
    return Path(raw).expanduser().resolve() if raw else None


def project_at(path: str) -> Project | None:
    """The project whose directory `path` is, or `None` for anything else."""
    root = configured_home()
    if root is None:
        return None
    resolved = Path(path).expanduser().resolve()
    try:
        parts = resolved.relative_to(root / "data").parts
    except ValueError:
        return None
    if len(parts) != 2 or not all(is_valid_name(p) for p in parts):
        return None
    return Project(home=root, tenant=parts[0], name=parts[1])


def projects_in(root: Path, tenant: str = DEFAULT_TENANT) -> list[Project]:
    """Every project of `tenant`: a directory with a valid name that holds an `ekos.toml`."""
    base = root / "data" / tenant
    if not base.is_dir():
        return []
    return [
        Project(home=root, tenant=tenant, name=d.name)
        for d in sorted(base.iterdir())
        if d.is_dir() and is_valid_name(d.name) and (d / "ekos.toml").is_file()
    ]


def state_dir(path: str) -> Path:
    """Where the workspace at `path` keeps its compiled state."""
    project = project_at(path)
    if project is not None:
        return project.state_dir
    return Path(path).expanduser().resolve() / ".ekos"


def _selector(project: Project) -> list[str]:
    return ["--home", str(project.home), "--tenant", project.tenant, "--project", project.name]


def command(ekos_bin: str, path: str, argv: list[str]) -> tuple[list[str], str]:
    """The command line and working directory that run `ekos <argv>` for the workspace at `path`.

    An in-place workspace is selected by the working directory. A project is selected by name;
    the CLI then changes into its source root itself, so the working directory given here does
    not matter and is the project directory, which always exists.
    """
    project = project_at(path)
    if project is None:
        return [ekos_bin, *argv], path
    return [ekos_bin, *_selector(project), *argv], path


def mcp_command(ekos_bin: str, path: str, tail: list[str]) -> list[str]:
    """`ekos mcp serve` for the workspace at `path`, followed by `tail` (transport and token)."""
    project = project_at(path)
    if project is None:
        return [ekos_bin, "mcp", "serve", "--workspace", path, *tail]
    return [ekos_bin, *_selector(project), "mcp", "serve", *tail]
