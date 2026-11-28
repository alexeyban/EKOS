"""The embedded engine through the built wheel (RFC 0269, phase 0).

Needs a compiled fact-engine ledger: `EKOS_PY_TEST_WORKSPACE=<workspace dir>`. Without it the
ledger tests are skipped; the tests that need no ledger always run.
"""

from __future__ import annotations

import hashlib
import os
from pathlib import Path

import pytest

import ekos

WORKSPACE = os.environ.get("EKOS_PY_TEST_WORKSPACE")
needs_ledger = pytest.mark.skipif(not WORKSPACE, reason="EKOS_PY_TEST_WORKSPACE is not set")


def tree_digest(root: Path) -> tuple[str, int]:
    digest = hashlib.sha256()
    count = 0
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        digest.update(str(path.relative_to(root)).encode())
        digest.update(path.read_bytes())
        count += 1
    return digest.hexdigest(), count


def test_a_directory_with_no_ledger_is_an_error_and_stays_empty(tmp_path: Path) -> None:
    with pytest.raises(ekos.LedgerError, match="no compiled ledger"):
        ekos.open(tmp_path)
    assert list(tmp_path.iterdir()) == []


def test_every_error_is_an_ekos_error() -> None:
    for error in (ekos.LedgerError, ekos.QueryError, ekos.NotSupported):
        assert issubclass(error, ekos.EkosError)


@needs_ledger
def test_reads_leave_the_ledger_byte_identical() -> None:
    assert WORKSPACE is not None
    root = Path(WORKSPACE)
    before = tree_digest(root)
    kb = ekos.open(root)
    status = kb.status()
    assert status.objects > 0
    hits = kb.search("status", limit=5)
    assert hits and all(h.id and h.name for h in hits)
    state = kb.get(hits[0].id)
    assert state is not None
    assert state["object"]["id"] == hits[0].id
    assert isinstance(state["evidence"], list)
    assert kb.refresh() is False
    del kb
    assert tree_digest(root) == before


@needs_ledger
def test_a_bad_id_and_an_unserved_mode_say_so() -> None:
    assert WORKSPACE is not None
    kb = ekos.open(WORKSPACE)
    with pytest.raises(ekos.QueryError):
        kb.get("not-an-id")
    with pytest.raises(ekos.NotSupported):
        kb.search("x", mode="hybrid")
    assert kb.get("00000000-0000-0000-0000-000000000000") is None
