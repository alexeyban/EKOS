"""Command allowlist (RFC 0127 §8.4) — STUB, filled in with the Phase 3 job runner.

"Run EKOS commands from a browser" is a remote-code-execution surface. When this is implemented:
a hardcoded allowlist is the only way to run anything; never `shell=True`; path parameters are
validated against registered workspace roots after `Path.resolve()`; write commands require a
separate role from read access.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Command:
    name: str
    argv_template: list[str]
    param_schema: dict[str, Any] = field(default_factory=dict)
    is_write: bool = False
    timeout: float = 600.0
    requires_role: str | None = None


# RFC 0127 §8.4 initial allowlist — deliberately empty until Phase 3 wires the runner.
COMMAND_ALLOWLIST: list[Command] = []
