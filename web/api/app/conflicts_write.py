"""Deciding a conflict (RFC 0172/0174) — the console's second human write seam.

`ekos conflicts resolve` is human-only: no MCP tool reaches it. The console is a human surface, so
it runs exactly that command for a signed-in **write**-role user and records who decided with
`--as`, by the same identity rule as a semantics review (`semantics_write.reviewer_for`).
"""

from __future__ import annotations

from .semantics_write import _UUID, ReviewError, _clean

_MAX_CLAIMS = 100


def resolve_argv(
    conflict_id: str,
    reviewer: str,
    *,
    pick: int | None = None,
    both_valid: bool = False,
    note: str | None = None,
) -> list[str]:
    """The exact argv for one decision: `--pick=<n>` (that claim is right) or `--both-valid`.
    The conflict is named by id only, and every value rides on its flag with `=`."""
    if not _UUID.match(conflict_id):
        raise ReviewError("conflict id must be a UUID")
    if both_valid == (pick is not None):
        raise ReviewError("decide one way: pick a claim, or say both are valid")
    note = _clean("note", note)
    argv = ["conflicts", "resolve", conflict_id, f"--as={reviewer}"]
    if both_valid:
        if not note:
            raise ReviewError("saying both claims are valid needs a note")
        argv.append("--both-valid")
    else:
        if isinstance(pick, bool) or not isinstance(pick, int) or not 1 <= pick <= _MAX_CLAIMS:
            raise ReviewError("pick is the 1-based number of a claim")
        argv.append(f"--pick={pick}")
    if note:
        argv.append(f"--note={note}")
    return argv
