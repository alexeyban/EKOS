# Devlog 358 — EKOS for Python, phase 1: the read engine

**Date:** 2026-11-28
**PRs:** none (direct to main)
**Branch:** python-sdk-phase1 → main (merged)

---

## Summary

RFC 0269 was accepted and its phase 1 built. The `ekos` Python package now reads a compiled
ledger in-process through 14 reads — search, objects with evidence, facts, history, graph
neighbourhood, impact, EKL, compiled answers, diff, time travel — with typed results and an
asyncio facade. To get there the bodies of the MCP server's read tools moved into a library
crate, `ekos-read`, so the MCP server and the Python engine run one implementation instead of
two. Store selection moved into `ekos_runtime::store::detect` for the same reason. The package
is still not on PyPI: the wheels workflow exists and publishing is gated on the owner
registering the name.

---

## RFC 0269 phase 1

### Problem / motivation

Phase 0 left a spike with three reads and its own JSON shaping. The RFC planned to grow that
shaping to the full surface and hold it to the MCP tools with a parity test. Written out, that
plan was the bodies of eleven MCP tools typed a second time: two copies to keep equal for ever,
and a duplication finding in Sonar on day one.

### What was built

| Piece | What it is |
|---|---|
| `ekos/crates/read` (`ekos-read`) | The read tools as library functions: `tools` (12 tool bodies, moved unchanged from `mcp.rs`), `args`, `conflicts` (read side of RFC 0172/0173, moved from `commands/conflicts.rs`), `kinds`; `ReadError::{Argument, NotFound, Store}` |
| `ekos_runtime::store` | `detect(config, cwd) -> Backend` and `open_partitioned`; `commands/store.rs` matches on it in its writable open, read-only open and `store_display` |
| `ekos-python` engine | `Engine::open(path, read_scope)`, `Engine::call(name, args, embedding)` over the `READS` table, `refresh`, `warnings`; partitioned stores read-only; SQLite and distributed refused |
| `python/ekos` | `_types.py` (15 frozen dataclasses), `_embedded.py` (`Knowledge`, `KnowledgeAt`, `open(path | project=)`), `aio.py` (`AsyncKnowledge`), `_native.pyi` |
| Tests | 10 engine tests; `mcp::tests::the_python_engine_answers_as_the_mcp_tools_do`; `review_guard` over both new crates; 17 Python tests against the installed wheel |
| `.github/workflows/wheels.yml` | Six targets, abi3, zig for Linux (`manylinux_2_28`, `musllinux_1_2`), install-and-import on Python 3.10 and 3.14, publish with trusted publishing when `PYPI_PUBLISH=true` |
| Docs | `docs/python.md`, `python/README.md`, a section in `docs/REFERENCE.md` and in the capabilities page, the RFC's "Phase 1 design" |

### Implementation details worth remembering

- **The move was mechanical on purpose.** The tool bodies went to `ekos-read` with three edits
  only: `fn tool_x` → `pub fn x`, `anyhow!` / `bail!` → `ReadError::Argument`, and
  `"object not found"` → `ReadError::NotFound`. The messages are the same text, so every
  existing MCP test passed without a change. About 470 lines left `mcp.rs`.
- **`search` no longer embeds.** The one tool body that reached outside the store called the
  project's embedding provider. `ekos-read::tools::search` takes the vector as an argument;
  `mcp.rs` computes it when `search_needs_embedding(args)`. The Python package passes its
  caller's vector through, and makes no network call.
- **One entry point across the language boundary.** `Ledger.call(name, json_arguments,
  embedding)` returns JSON text. Adding a read is a row in `READS`, an arm in `Engine::call` and
  a method on `Knowledge`; the PyO3 glue does not change.
- **Scope decisions live in the table.** `READS[i].scoped` says whether a reader with a
  restricting scope may call the read. The CLI test holds it equal to `SCOPED_TOOLS`.
- **Errors keep their class across the boundary**: `ReadError` → `EngineError` → the Python
  exception. `Knowledge.get` turns `NotFound` into `None`; `facts` and `dependents` raise it.

### Decisions

- **Share the code, not the shape** (above). The parity test remains, now as a check that both
  front ends are wired to the same function with the same arguments.
- **`Hit` is `(id, name)`.** Phase 0 returned `kind` and `score`; the MCP tool never did, and
  `kind` was `None` on every real hit. The package returns what the tool returns.
- **`KnowledgeAt` has two reads**, `get` and `graph`. An `ekl` that appended `AS OF` to the
  caller's query was written and removed: where the clause may go depends on the rest of the
  query.
- **Wheels in their own workflow.** As a job of `release.yml` it could only be tried by building
  every installer. Alone, it can be dispatched by hand.
- **zig instead of manylinux containers.** Measured in phase 0; it keeps the workflow to plain
  runner steps and the pinned toolchain file applies unchanged.
- **No source distribution.** It would have to contain the workspace.

---

## Knowledge Captured

- **An EKL `FIND` without `ORDER BY` returns rows in the store's iteration order, and that
  order differs between two handles on the same ledger.** The parity test failed on it the
  first time it ran. Anything that compares two reads needs `ORDER BY`.
- **`review_guard`'s "the file was really read" probe named `conflicts::open_for`**, which
  `mcp.rs` no longer calls after the move. A probe that names a moved function fails with "mcp.rs
  was not read" — misleading, and worth knowing before moving anything else out of `mcp.rs`. It
  now names `conflicts::agent_list`.
- **`every_argument_a_handler_reads_is_declared` scans only the `call_tool` body**, not the
  `tool_*` functions, so moving the bodies did not weaken it — and it never covered them.
- **A guard that scans source for a forbidden word also reads the doc comments.** The engine's
  own module doc said "no `append_*`" and failed the no-write-path test. The test builds its
  words with `concat!` so it does not match itself.
- **A read-only search index of another content version only logged a warning through
  `tracing`**, which a library user never sees. `recorded_content_version` makes it a value the
  caller can read.
- **`python -m pytest` and `coverage run -m pytest` put the working directory first on
  `sys.path`.** Recorded in devlog_357; it is why every Python step of the test script and of the
  wheels workflow runs from a scratch directory.
- **mypy --strict rejects `f(**{"a": "x"})` for a function with differently typed keyword
  parameters** (`dict` is invariant). Tests call with real keywords.
- **`serde` writes `ObjectKind::Custom("X")` as `{"Custom": "X"}`** in a full object and as
  `"X"` where a tool calls `.to_string()`. `Object.from_json` accepts both.
- **The package test runs on Python 3.14** as well as 3.10 with the same abi3 wheel.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/read/` | New crate `ekos-read` |
| `ekos/crates/runtime/src/store.rs` | New: `Backend`, `detect`, `open_partitioned`, the path helpers |
| `ekos/crates/cli/src/commands/mcp.rs` | Read tools call `ekos_read::tools`; parity test |
| `ekos/crates/cli/src/commands/{store,conflicts,graph,review_guard}.rs` | Use the moved functions; guard over the new crates |
| `ekos/crates/ledger/src/search.rs` | `CONTENT_VERSION` public, `recorded_content_version` |
| `ekos/crates/python/src/{engine,lib}.rs` | The full engine and its glue |
| `python/ekos/{__init__,_types,_embedded,aio}.py`, `_native.pyi` | The package |
| `python/tests/test_embedded.py` | 17 tests |
| `.github/workflows/wheels.yml` | New workflow |
| `docs/python.md`, `docs/REFERENCE.md`, `docs/generated/ekos-self-documentation.html`, `python/README.md` | Guide and reference |
| `ekos/docs/rfcs/0269-python-package.md` | Accepted; "Phase 1 design" and result |
| `scripts/publish-crates.sh`, `ekos/Cargo.toml`, `ekos/Cargo.lock` | The new crate |
| `CLAUDE.md`, `TODO.md` | Crate map rows, phases |
