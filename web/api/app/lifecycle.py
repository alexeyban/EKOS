"""Stop and restart under a launcher (RFC 0185 §2).

`ekos console run` starts the console and starts it again when it asks to be. Both directions
go through files in the control directory, the same on every OS, so no signal has to cross
between processes: `stop-requested` and `restart-requested`. The console watches for either and
shuts down as on Ctrl-C (the lifespan stops its MCP servers); the launcher reads which file it
was, removes it, and exits or starts the console again.
"""

from __future__ import annotations

import asyncio
import signal
from collections.abc import Callable
from pathlib import Path

STOP = "stop-requested"
RESTART = "restart-requested"


def shut_down() -> None:
    """Shut down as on Ctrl-C: uvicorn's handler runs, then the lifespan's cleanup."""
    signal.raise_signal(signal.SIGTERM)


async def watch(
    control: Path, stop: Callable[[], None] | None = None, *, interval: float = 1.0
) -> None:
    while True:
        if (control / STOP).exists() or (control / RESTART).exists():
            # Looked up now, not at definition, so a test can replace it.
            (stop or shut_down)()
            return
        await asyncio.sleep(interval)
