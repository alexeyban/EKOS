"""The read-only subprocess allowlist (RFC 0129 §5)."""

from __future__ import annotations

import pytest

from app.readproc import ReadProcError, _check_allowed, read_json


def test_allowlist_accepts_exactly_the_allowed_shapes() -> None:
    _check_allowed(["status", "--json"])
    _check_allowed(["doctor", "--json"])
    _check_allowed(["ledger", "timeline", "--json"])
    _check_allowed(["ledger", "timeline", "--json", "--bucket", "week"])
    _check_allowed(["ledger", "timeline", "--json", "--bucket", "day", "--since", "2026-01-01"])
    _check_allowed(["config", "validate", "--json"])
    _check_allowed(["config", "validate", "--json", "--file", "/tmp/x.toml"])
    _check_allowed(["config", "preview-scan", "--json"])
    _check_allowed(["config", "preview-scan", "--json", "--max-files", "1000"])


def test_allowlist_rejects_anything_else() -> None:
    for argv in (
        ["build"],
        ["commit"],
        ["status"],  # missing --json
        ["status", "--json", "--verbose"],
        ["ledger", "timeline", "--json", "--output", "/etc/passwd"],
        ["ledger", "repair"],
        ["doctor", "--json", "extra"],
        ["config", "validate"],  # missing --json
        ["config", "preview-scan", "--json", "--config", "/etc/passwd"],
        ["config", "set", "--json"],
    ):
        with pytest.raises(ReadProcError):
            _check_allowed(argv)


async def test_read_json_rejects_a_non_directory_workspace(tmp_path) -> None:
    with pytest.raises(ReadProcError):
        await read_json("/bin/true", str(tmp_path / "missing"), ["status", "--json"])


async def test_read_json_surfaces_a_nonzero_exit(tmp_path) -> None:
    with pytest.raises(ReadProcError):
        await read_json("/bin/false", str(tmp_path), ["status", "--json"])
