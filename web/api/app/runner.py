"""Job runner (RFC 0127 §8.5) — STUB, Phase 3.

One bounded queue per workspace plus a per-workspace mutex (EKOS takes a real cross-process write
lock on writes, RFC 0104, so two concurrent `build`s on one workspace is a guaranteed conflict).
`asyncio.create_subprocess_exec` only — never a shell. Cancellation is SIGTERM then SIGKILL after
a grace period; an interrupted `commit` is safe because commits are idempotent. Chained runs
(`build -> recover -> resolve -> compile -> commit`) are one queue entry with per-stage status.
"""

from __future__ import annotations


class JobRunner:
    def __init__(self) -> None:  # pragma: no cover - stub
        raise NotImplementedError("job runner arrives with RFC 0127 Phase 3")
