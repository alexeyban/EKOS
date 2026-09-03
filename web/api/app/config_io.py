"""`ekos.toml` read / patch / validate + scan preview (RFC 0127 §8.6) — STUB, Phase 2.

Editing must use `tomlkit` (comments and formatting preserved) and must surface the append-only
warning: narrowing `[observe] paths` / `ignore-patterns` is a two-step fix (config change **plus**
full rebuild) because the ledger never retroactively drops already-committed data. The
preview-scan endpoint also makes concrete that `WalkDir` ignore matching is directory-**name**
equality, not a glob — `fixtures` excludes every directory named `fixtures` anywhere in the tree.
"""

from __future__ import annotations


def read_config(_path: str) -> dict:  # pragma: no cover - stub
    raise NotImplementedError("config editing arrives with RFC 0127 Phase 2")
