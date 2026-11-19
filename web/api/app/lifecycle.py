"""Stop and restart under a launcher (RFC 0185 §2, RFC 0261 §2.3).

`ekos console run` starts the console and starts it again when it asks to be. Both directions
go through files in the control directory, the same on every OS, so no signal has to cross
between processes: `stop-requested` and `restart-requested`. The console watches for either and
shuts down as on Ctrl-C (the lifespan stops its MCP servers); the launcher reads which file it
was, removes it, and exits or starts the console again.

RFC 0261 §2.3: a stop never loses a run. While runs are queued or running, `stop-requested` is
refused: the console writes `stop-refused` (the runs, as JSON), removes the request and keeps
running. A request whose text contains `force` stops it anyway (`ekos console stop --force`, which
uninstall uses after telling the person). The console also keeps `active-runs.json` current, so
`ekos console status --json` can say what runs without signing in.
"""

from __future__ import annotations

import asyncio
import json
import signal
from collections.abc import Callable
from pathlib import Path
from typing import Any

STOP = "stop-requested"
RESTART = "restart-requested"
REFUSED = "stop-refused"
ACTIVE = "active-runs.json"

ActiveRuns = Callable[[], list[dict[str, Any]]]


def shut_down() -> None:
    """Shut down as on Ctrl-C: uvicorn's handler runs, then the lifespan's cleanup."""
    signal.raise_signal(signal.SIGTERM)


def _write(path: Path, data: object) -> None:
    """Write through a temporary file, so a reader never sees half a file."""
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(data), encoding="utf-8")
    tmp.replace(path)


def stop_is_forced(marker: Path) -> bool:
    try:
        return "force" in marker.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return False


def decide_stop(control: Path, runs: list[dict[str, Any]]) -> bool:
    """A stop request is in `control`: True to shut down now, False when it was refused."""
    marker = control / STOP
    if runs and not stop_is_forced(marker):
        _write(control / REFUSED, {"active_runs": runs})
        marker.unlink(missing_ok=True)
        return False
    (control / REFUSED).unlink(missing_ok=True)
    return True


async def watch(
    control: Path,
    stop: Callable[[], None] | None = None,
    *,
    interval: float = 1.0,
    active_runs: ActiveRuns | None = None,
) -> None:
    last: list[dict[str, Any]] | None = None
    while True:
        # A database read: on a worker thread, never in the event loop.
        runs = await asyncio.to_thread(active_runs) if active_runs is not None else []
        if active_runs is not None and runs != last:
            _write(control / ACTIVE, runs)
            last = runs
        restart = (control / RESTART).exists()
        if restart or ((control / STOP).exists() and decide_stop(control, runs)):
            # Looked up now, not at definition, so a test can replace it.
            (stop or shut_down)()
            return
        await asyncio.sleep(interval)
