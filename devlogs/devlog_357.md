# Devlog 357 — EKOS for Python: the spike and RFC 0269

**Date:** 2026-11-28
**PRs:** none (direct to main)
**Branch:** python-sdk-phase0 → main (merged)

---

## Summary

Phase 0 of the Python package plan. A minimal PyO3 extension (`ekos-python`) opens a compiled
ledger read-only inside a Python process and serves `status`, `search`, `get` and `refresh`.
It exists to answer the plan's open questions with measurements, and RFC 0269 records the
answers and the design for phases 1–2. The package is **not** on PyPI and is not a released
capability yet: the README and the reference are unchanged.

---

## RFC 0269 phase 0 — the spike

### Problem / motivation

A Python developer cannot use EKOS from a program: the ways in are the binary and an MCP server.
The plan (embedded reads through PyO3, the pipeline through the binary, remote through MCP) had
questions that only a build could answer: how big the wheel is, whether the binding must link the
CLI crate to open a store, what a `Runtime` per call costs, and what a reader sees while another
process writes.

### What was built

| Piece | What it is |
|---|---|
| `ekos/crates/python/src/engine.rs` | The read engine with no Python in it: open (read-only, never creates), `status`, `search` (lexical), `get`, `refresh`; returns `serde_json::Value` |
| `ekos/crates/python/src/lib.rs` | The PyO3 glue (60 lines) behind the crate feature `python`: a `Ledger` class, `open`, four exception types; results cross as JSON text; the interpreter lock is released around every call |
| `python/` | The package: `pyproject.toml` (maturin, abi3, floor 3.10), `ekos/__init__.py` (dataclasses `Status`, `Hit`; `Knowledge`), `_native.pyi`, `py.typed`, tests |
| CI job `python` | clippy on the glue, a dev-profile wheel, install in a fresh 3.10 environment, ruff, `mypy --strict`, pytest |
| `ekos/docs/rfcs/0269-python-package.md` | The design, the measurements, ten decisions |

### Measurements (Plausible ledger: 83,833 entries, 10,485 objects, 98 MB)

| Question | Result |
|---|---|
| Engine-only wheel | 3.8 MB (`.so` 9.8 MB) |
| Wheel when the extension links the CLI crate to open a store | 6.1 MB (`.so` 15.0 MB) |
| The CLI binary deflated (an `ekos-bin` wheel) | 30.9 MB of 80.6 MB |
| Clean release build | 1 min 50 s |
| `open` | 140 ms |
| lexical `search`, limit 10 | 0.09 ms per call, a new `Runtime` each call |
| `get` | 0.81 ms per call |
| `status` | 6.4 ms per call |
| Ledger directory after ~2,600 reads | byte-identical |
| `manylinux_2_28` with `--zig` | builds in 2 min, 3.76 MB |

### Decisions

- **The binding does not link the CLI.** The size cost is modest (+58%) because thin LTO drops
  what is not reached; the reason is coupling — the read package would be rebuilt by every
  connector change and would compile the console's HTTP stack on six targets. Phase 1 moves the
  backend decision into `ekos_runtime::store::detect`, used by both.
- **PyO3 is behind a feature that is off by default.** `cargo build/test/clippy --workspace`
  never compiles it and needs no interpreter. The price is one extra clippy line in CI.
- **JSON text across the boundary.** The glue stays tiny, and the Python dataclasses, not the Rust
  types, are the public contract. At 0.09 ms per search there is nothing to win by converting
  objects directly.
- **A handle is a snapshot; `refresh()` is explicit.** Two reads in a row must agree.
- **The engine never creates a store.** The CLI's read-only open bootstraps an empty one for the
  MCP server; a library that makes directories in the path it was asked to read is a surprise.
- **SQLite ledgers are `NotSupported`** rather than opened: `Ledger::open` sets WAL mode, and a
  WAL read makes `-shm`/`-wal` files, so the "reads change nothing" guarantee cannot hold.
- **Two distributions**: `ekos` (reads) and `ekos-bin` (the CLI, phase 2).

---

## Knowledge Captured

- **PyO3 0.30 names.** `Python::detach` releases the interpreter lock (it was `allow_threads`),
  `Python::attach` takes it. A declarative `#[pymodule] mod` exports an exception type with
  `#[pymodule_export] use …`. Check the crate's source in `~/.cargo/registry` before trusting an
  older example.
- **`abi3-py310` builds without linking libpython**, and with the glue behind a feature the
  workspace has no Python at all in a normal build. The well-known "cargo test cannot link an
  extension module" problem never appears, because the tests are in the plain-Rust module.
- **maturin from a directory outside the workspace** works with `manifest-path`; the wheel's
  version comes from the workspace version, so it cannot drift. maturin also puts a 540 KB
  CycloneDX SBOM in the wheel by default.
- **A local `maturin build` is tagged with the host's glibc** (`manylinux_2_34` here). Such a
  wheel does not install on an older distribution. Release wheels need a manylinux container or
  `--zig --compatibility manylinux_2_28`.
- **A search hit's `kind` is `None`** on this ledger for every hit: the index does not store it
  and `retrieve` does not hydrate it. The MCP tool never showed it, so nothing depended on it.
  The Python type makes it optional.
- **`status` is the slow read** (6.4 ms): three counts over the store. Search is 70× faster.
- **A read-only fact-engine handle sees a concurrent writer's appends only after
  `refresh_snapshot`**, lexical search included once the writer has committed its index. The test
  `a_reader_is_a_snapshot_that_a_concurrent_writer_neither_blocks_nor_breaks` pins all three
  parts: the writer is not blocked, the snapshot holds, the refresh catches up.
- **`python -m pytest` puts the working directory on `sys.path`.** Run inside `python/`, `import
  ekos` finds the source package, which has no native module, and fails with a misleading
  "circular import". The package test runs from a scratch directory so the installed wheel is
  what gets imported; `[tool.coverage.paths]` maps `site-packages/ekos` back to `python/ekos`
  for Sonar (a bare `ekos/…` path would name the Rust workspace directory).
- **Never `git stash` while a background cargo run is using the tree.** A stash taken to compare
  `cargo audit` against main removed the new crate mid-build and produced a compile error that
  could not be reproduced afterwards.
- **`cargo update -p url` did nothing until the index was refreshed**; `--precise 2.5.9` did it.
  RUSTSEC-2026-0592 (`url` 2.5.8) was on main before this work and is fixed in all three lockfiles.
- **Measuring "what if it linked crate X"**: a temporary optional dependency plus one function
  that reaches the wanted path is enough; LTO removes the rest, so the number is the real cost of
  that path, not of the crate.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/python/` | New crate `ekos-python`: `engine.rs`, `lib.rs`, five tests |
| `ekos/Cargo.toml`, `ekos/Cargo.lock` | Workspace member; PyO3 in the lockfile |
| `python/` | The package skeleton, stub, tests, README |
| `ekos/docs/rfcs/0269-python-package.md` | New RFC (Proposed) |
| `.github/workflows/ci.yml` | Job `python` |
| `scripts/python-package-test.sh` | Builds the wheel, installs it fresh, compiles a two-table workspace with the binary, runs ruff, mypy, pytest with coverage |
| `.github/workflows/sonar.yml`, `sonar-project.properties` | The package's coverage report and tests |
| `benchmark/Cargo.lock`, `tests/integration/Cargo.lock` | `url` 2.5.9 (RUSTSEC-2026-0592) |
| `.gitignore` | Package build artifacts |
| `CLAUDE.md`, `TODO.md` | Crate map row, RFC and devlog numbers, the plan's phases |
