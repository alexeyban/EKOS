# RFC 0269 — EKOS for Python: the `ekos` package

**Status:** Accepted (2026-11-28) — phases 0 and 1 done (results below); phase 2 not started
**Builds on:** RFC 0097 (read-only open), RFC 0104 (snapshot refresh), RFC 0115 / 0143 / 0182 (MCP over TCP,
HTTP, the console gateway), RFC 0151 (session memory is hidden from reads), RFC 0181 (read scope),
RFC 0153 (release matrix).
**Code:** new crates `ekos/crates/python` (`ekos-python`) and `ekos/crates/read` (`ekos-read`, phase 1), new
directory `python/` (the package), `ekos_runtime::store` (phase 1).
**Not in this RFC:** writes from Python ("contribute") — its own RFC, phase 3 of the plan.

## Problem

A Python developer cannot use EKOS from a program. The ways in today are the `ekos` binary and an MCP
server; both are a separate process, and the only Python MCP client is private to the console
(`web/api/app/mcp_client.py`). There is no `pip install`.

The goal: `pip install ekos`, then from Python
1. **open** a compiled ledger in-process and read it — no server, no Rust toolchain;
2. **run the pipeline** (`init → build → recover → resolve → compile → commit`);
3. **connect** to a remote EKOS through the same interface.

## Design

### Three access modes, one interface

| Mode | Entry point | Mechanism | Why |
|---|---|---|---|
| Embedded read | `ekos.open(path)` | PyO3 extension over `Runtime` | The read seam is already read-only (`Runtime` takes `&dyn KnowledgeReader`) and already hides session memory |
| Pipeline | `ekos.Project` | the `ekos` binary as a subprocess | `main_with` owns the working directory, a global logger and a tokio runtime; none of that belongs inside someone's interpreter |
| Remote | `ekos.connect(url, token=)` | MCP over TCP or the console gateway | Inherits roles, read scope and audit unchanged |

Embedded and remote implement one `Knowledge` protocol. A method a mode cannot serve raises
`ekos.NotSupported` with the reason — never a partial answer.

### The embedded engine

**Layers.** `crates/python/src/engine.rs` is plain Rust with no Python in it: it opens a store, runs a
read through `Runtime` and returns `serde_json::Value`. `lib.rs` is the PyO3 glue behind the crate
feature `python` (off by default), and hands each result to Python as JSON text. `python/ekos/_types.py`
turns it into frozen dataclasses. The dataclasses are the public contract; the Rust types are not, so a
change inside `Runtime` does not change the Python API.

Consequences:
- `cargo build/test/clippy --workspace` never compiles PyO3 and needs no interpreter or libpython;
- the engine is tested by `cargo test`, and compared with the MCP tools from Rust (see Tests);
- the glue stays small enough to read in one sitting (phase 0: 60 lines).

**Rules of the open.**
1. Read-only. The fact engine is opened with `FactLedger::open_read_only`, which takes no write lock
   and never re-indexes — a writable open under another `CONTENT_VERSION` would wipe and replay the
   search index.
2. **Never creates.** The CLI's `open_store_read_only` bootstraps an empty store in a fresh
   workspace (the MCP server relies on it). The engine does not: a path with no compiled ledger is
   `LedgerError`, and the directory is left as it was.
3. Every read goes through `Runtime`. The binding never names `KnowledgeStore` appends. The engine
   takes no `append_*` path at all, so it is not one more place that can write the ledger.
4. A handle is a **snapshot** of the ledger at open. `kb.refresh()` folds in what another process
   committed since (RFC 0104's `refresh_snapshot`) and returns whether anything changed. Reads never
   refresh on their own: two reads in a row must agree.
5. One call at a time per handle: the store sits behind a mutex (`KnowledgeReader` is `Send`, not
   `Sync`), and the interpreter lock is released around every call. Several handles on one ledger
   are independent. Parallel reads on one handle wait for RFC 0112.

**Which stores.**

| Store | Phase 1 |
|---|---|
| Fact engine (default since 2026-08-21) | yes |
| Partitioned (RFC 0111) | yes, read-only handle |
| SQLite (workspaces from before the switch) | `NotSupported`, naming `ekos ledger migrate --v3`. `Ledger::open` sets WAL mode, and a WAL read creates `-shm`/`-wal` files: it cannot meet rule 1 byte for byte |
| Distributed (RFC 0113) | `NotSupported`, naming `ekos.connect` |

**Path.** `ekos.open(path)` takes a workspace directory (`.ekos/` beside an optional `ekos.toml`) or a
home project directory (`<home>/data/<tenant>/<project>` holding `ekos.toml` and `state/`).
`ekos.open(project="erp", tenant="acme")` resolves through the EKOS home the way `--project` does. An
`ekos.toml` that does not parse is an error, as in the CLI.

**Store selection moves to a library.** Which backend a workspace uses is decided in
`crates/cli/src/commands/store.rs` today, and the binding must not link the CLI (see the spike). Phase 1
moves the decision — not the opening — into `ekos_runtime::store::detect(config, cwd) -> Backend`
(`Distributed | Partitioned(dir) | Facts(dir) | Sqlite(file) | None`), used by both `store.rs` and the
engine, so there is one function that decides. `build_partitioned` moves with it.

### Python API (phase 1 surface)

```python
import ekos

kb = ekos.open("~/work/plausible")           # read-only snapshot
kb.status()                                  # Status(entries, objects, relationships, ledger_path)
kb.refresh()                                 # bool: did another process commit since?

hits = kb.search("payment status", limit=10)                 # lexical
hits = kb.search("payment status", mode="hybrid", embedding=vec)
obj  = kb.get(hits[0].id)                    # Object | None, with relationships and evidence
kb.facts(obj.id)                             # attribute → value, each with provenance
kb.neighborhood(obj.id, depth=2)             # Graph(objects, relationships)
kb.impact(obj.id, hops=3)
kb.dependents(obj.id, hops=1); kb.dependencies(obj.id, hops=1)
kb.history(obj.id); kb.audit_trail(obj.id)

past = kb.at("2026-10-01T00:00:00Z")         # a view with the same read methods, as of that time
kb.diff(since="2026-10-01", until="2026-11-01")

rows = kb.ekl("FIND Object WHERE kind = 'Table'")
ans  = kb.query("which jobs write to orders?")   # compiled answer, no model call
for claim in ans.claims:
    claim.text, claim.disputed, claim.evidence   # evidence is always a list, possibly empty
```

Rules:
- **Evidence is part of the type.** Anything that states a conclusion has an `evidence` list. An
  empty list is shown, never dropped.
- **No dependencies.** Frozen dataclasses with `to_dict()`; no pydantic. `py.typed`, a hand-written
  `_native.pyi`.
- **Errors.** `EkosError` → `LedgerError`, `QueryError` (EKL parse position, `UnknownField`, a bad id),
  `NotSupported`; phase 2 adds `PipelineError` and `RemoteError`.
- **Vector and hybrid search take the query vector from the caller** (`embedding=`). The engine does
  not call an embedding provider: that is a network call, possibly a paid one, and nothing in a read
  package should make one implicitly. A wrong dimension or model is `QueryError` naming what the
  index holds.
- **Read scope.** `ekos.open(..., read_scope={...})` wraps the store in `ScopedStore`, with
  `--read-scope`'s rules; under a restricting scope only the reads `mcp.rs::SCOPED_TOOLS` allows are
  served.
- **A search hit's `kind` may be `None`**: the index does not always store it (measured below).
  `Hit.kind` is optional in the type; `get` always has it.
- **Async.** `ekos.aio` runs the same calls on a thread (the interpreter lock is released, so the
  event loop is not blocked).
- **Never in the package:** anything in `review_guard::HUMAN_ONLY`. A Python process is as
  programmable as an agent; approving, confirming, resolving and signing off stay with people in the
  CLI and the console.

### Pipeline and remote (phase 2, outline)

- `ekos-bin`: the CLI binary as a wheel, installed by `pip install "ekos[cli]"`. `ekos.Project` finds
  the binary in `ekos-bin`, then `EKOS_BIN`, then `PATH`, and otherwise fails with the install line.
- `ekos.Project.init(dir, sources={...})`, `.compile()` (the five stages in order), per-stage calls,
  `.status()`, `.doctor()`, `.open()`. Results come from `--json` output and the run log
  (`EKOS_RUN_LOG`, RFC 0220), which already records stages, warnings and model calls.
- `ekos.connect("tcp://host:port", token=)` and `ekos.connect("https://console/mcp/<tenant>/<project>",
  token=)`. The TCP client is `web/api/app/mcp_client.py`, moved into the package; the console then
  imports it from there, so there is one client.

Phase 2 gets a short design section added to this RFC before its code, as RFC 0268's phases did.

### Packaging and versions

- Distribution `ekos`: the Python API and the native engine. One **abi3** wheel per platform
  (`cp310-abi3`), floor Python 3.10, no runtime dependencies.
- Distribution `ekos-bin` (phase 2): the CLI.
- Built by maturin from `python/pyproject.toml` (`manifest-path = ../ekos/crates/python/Cargo.toml`,
  `features = ["python"]`, `module-name = "ekos._native"`).
- The wheel version is the workspace version: `ekos==X` reads what `ekos X` wrote. A read-only open
  under another `CONTENT_VERSION` only warns in the engine; the package turns that into a Python
  `warnings.warn`.
- The Python API is marked provisional (`Development Status :: 3 - Alpha`) until phase 2 lands.
- Release: a `wheels` job in `release.yml` over the existing six targets (`maturin-action`, manylinux
  2_28 containers for the Linux targets, musllinux for the static one), published with PyPI trusted
  publishing. `ekos-python` is `publish = false` for crates.io.

### Public wording

"EKOS — an embedded knowledge engine for Python: open a compiled, evidence-backed knowledge ledger
and query it in-process." Not "database": knowledge enters by compiling sources, there is no `INSERT`
and no delete, and the package description must not suggest either.

## Tests and guards

| Test | Where | Purpose |
|---|---|---|
| Parity: each engine read vs the same MCP tool's JSON on a fixture ledger | `crates/cli/tests/python_parity.rs` (the CLI dev-depends on `ekos-python`, no `python` feature) | The package cannot drift from what agents see |
| Read-only proof: a ledger directory hashed before and after the whole read suite | `python/tests` | No write, no index replay |
| Never creates: an open of an empty directory fails and leaves it empty | `engine.rs` (phase 0) | Rule 2 |
| Reader beside a writer: not blocked, keeps its snapshot, `refresh` catches up | `engine.rs` (phase 0) | Rules 4–5 |
| Session memory: no `Session` / `SessionClaim` reachable from any engine read | `engine.rs` | Same guarantee as `Runtime` |
| No write path: `engine.rs` and `lib.rs` name no `append_` function and nothing in `HUMAN_ONLY` | source scan in the crate, and `review_guard` gains `crates/python/src` as a second scanned root | Rule 3; review stays human-only |
| Read-scope leak suite through the engine | `crates/cli/tests/read_scope.rs` extended | `ScopedStore` holds from Python too |
| Wheel smoke test: install the built wheel in a fresh environment, lowest and highest supported Python, each release target | CI `python` job, `release.yml` | Packaging works where people are |
| `mypy --strict`, `ruff` | CI `python` job | The typed API stays typed |
| `cargo clippy -p ekos-python --features python -- -D warnings` | CI | The glue is not skipped by `--workspace` |

Sonar: `python/ekos` is new Python code under `sonar.sources`; its tests' coverage report is added to
`sonar.python.coverage.reportPaths` and `python/tests` to `sonar.tests` in the same commit that adds
the tests.

## Spike results (phase 0, 2026-11-28)

Minimal crate: `open`, `status`, `search` (lexical), `get`, `refresh`. Linux x86_64, Rust 1.98.0,
PyO3 0.30.1 (`abi3-py310`), maturin 1.16.0, release profile (thin LTO, stripped). Ledger: Plausible
Analytics (`analytics`), fact engine, 98 MB on disk — 83,833 entries, 10,485 objects, 13,902
relationships.

| Question | Measurement | Decision |
|---|---|---|
| Wheel size, engine only | `_native.abi3.so` 9.8 MB; wheel **3.8 MB** | Far under PyPI's 100 MB file limit |
| Wheel size when the extension links the CLI crate for `open_store_read_only` | `.so` 15.0 MB; wheel 6.1 MB (+58%), with thin LTO dropping everything but the store-opening path; the build compiles every connector and the console's HTTP stack for each target | Do not link the CLI — for the coupling more than the size: the read package would rebuild on every connector change. Move `detect` to `ekos_runtime` |
| The CLI binary as a wheel | 80.6 MB stripped (it already is), **30.9 MB** deflated | Fits; two distributions (`ekos`, `ekos-bin`) |
| Clean release build of the extension | 1 min 50 s wall (16 threads) | Acceptable for six targets |
| `open` | 140 ms (cold: loads the memtable and the search reader) | Open once, keep the handle |
| `search`, lexical, limit 10 | 0.09 ms per call (200 calls), including a new `Runtime` and JSON both ways | `Runtime` per call costs nothing measurable; no cached runtime |
| `get` (object + relationships + evidence) | 0.81 ms per call | — |
| `status` | 6.4 ms per call (three counts) | Fine; not a hot path |
| Ledger directory after open + ~2,600 reads | byte-identical (SHA-256 over every file, same file count) | Rule 1 holds on the fact engine |
| A reader open while a writer appends | writer not blocked; reader keeps its snapshot; `refresh` → sees the new object, lexical search included | Rule 4; `kb.refresh()` is in the API |
| A hit's `kind` | `None` for all 30 hits tried | `Hit.kind` is optional |
| abi3 floor | builds and imports on Python 3.10 with `abi3-py310` | Floor 3.10 |
| Workspace builds without Python | `cargo test -p ekos-python` passes with PyO3 not compiled | Feature `python`, off by default |
| manylinux | a plain local build is tagged `manylinux_2_34` (the host's glibc); `maturin build --zig --compatibility manylinux_2_28` builds the tantivy + zstd + bundled-SQLite stack in 2 min and gives a 3.76 MB `manylinux_2_28` wheel | Release wheels are built in manylinux containers, never on a runner's own glibc |

## Phase 1 design (2026-11-28)

Two things in the design above changed when phase 1 was built.

### One implementation of the reads: `ekos-read`

The design said the engine would shape its own JSON and a parity test would hold it to the MCP
tools. Written out, that was the bodies of eleven MCP tools a second time. Instead the bodies moved,
unchanged, out of `crates/cli/src/commands/mcp.rs` into a new library crate, `ekos-read`:

| Module | Content |
|---|---|
| `tools` | `status`, `search`, `query`, `retrieve`, `ekl`, `neighborhood`, `state`, `dependents`, `impact`, `diff`, `graph_export`, `audit` — JSON arguments in, JSON out, the tools' names and shapes |
| `args` | `required_str`, `required_id`, `bounded_arg` and the neighbourhood / impact bounds |
| `conflicts` | the read side of RFC 0172 / 0173: which conflicts are current, `open_for`, `flag_disputed`, `brief` |
| `kinds` | `parse_object_kind` |

`mcp.rs` calls these (`"ekos_impact" => Ok(tools::impact(&runtime, args)?)`) and so does the engine.
`ReadError` has three cases — `Argument`, `NotFound`, `Store` — with the messages the tools always
gave; they become `QueryError`, `NotFound` and `LedgerError` in Python.

`ekos-read` opens no store and makes no network call. `search` therefore takes the query vector as
an argument: the MCP server computes it from the project's embedding provider
(`tools::search_needs_embedding`), the Python package takes it from its caller.

The parity test stayed, as a check of the wiring rather than of two implementations:
`mcp::tests::the_python_engine_answers_as_the_mcp_tools_do` runs every read that names an MCP tool
through the whole MCP path and through `Engine::call`, on one ledger, and compares the JSON. It also
holds the engine's scope decisions equal to `SCOPED_TOOLS`.

### The engine has one entry point

`Engine::call(name, args, query_embedding)` dispatches over `engine::READS`, a table of
`(name, MCP tool, allowed under a restricting scope)`. Two reads have no MCP tool: `facts`
(`Runtime::facts_of`) and `history` (`Runtime::object_history`). The PyO3 glue is `open`, `call`,
`refresh`, `warnings`, `reads`.

### Smaller points

- **A search hit is `Hit(id, name)`.** The MCP tool never returned a kind or a score, and the
  package returns what the tool does.
- **`kb.at(time)`** serves `get` and `graph`, the two reads the ledger answers for a past moment;
  for anything else an EKL query takes `AS OF`.
- **`ekos.open(project=, tenant=, home=)`** resolves a home project the way `--project` does.
- **Content version.** `ekos_ledger::search::recorded_content_version` lets the engine see that a
  search index was written under another `CONTENT_VERSION`; the open then carries a warning, which
  Python raises as a `UserWarning`. Nothing is rebuilt.
- **Wheels are their own workflow**, `.github/workflows/wheels.yml`, not a job of `release.yml`: it
  can be run by hand to test the six targets without building installers. Linux wheels are linked
  through zig (`manylinux_2_28`, `musllinux_1_2`) on the runners, with no container. Publishing waits
  for a PyPI trusted publisher and the repository variable `PYPI_PUBLISH=true`.
- **Wheels only, no source distribution**: an sdist would have to carry the whole workspace.

### Result (phase 1, 2026-11-28, devlog_358)

- 14 reads served; every one compared with its MCP tool where there is one (12).
- Rust: 10 engine tests (snapshot beside a writer, session memory never returned, read scope,
  refused stores, no write path in the engine or `ekos-read`, content-version warning).
- `review_guard` scans `ekos-read` and `ekos-python` for a path to a human-only entry point.
- Python: 17 tests against the installed wheel on a workspace the binary compiles, 100% line
  coverage of the package, `mypy --strict` clean; passes on Python 3.10 and 3.14. The workspace
  directory is byte-identical after the suite.
- Not done in phase 1: the wheels workflow has not built macOS, Windows, aarch64 or musl wheels
  until its first run; a quickstart notebook; `to_pandas` (phase 4).

## Decisions

| # | Decision | Taken |
|---|---|---|
| 1 | One wheel or two | Two: `ekos` (3.8 MB) and `ekos-bin` (~31 MB). Someone who only reads never downloads the binary |
| 2 | Writes | Not in this RFC. Planned as a facts inbox compiled like any source (own RFC); no direct appends, ever, from the binding |
| 3 | Where the code lives | This repository: `ekos/crates/python` + `python/`, version locked to the workspace |
| 4 | Wording | "Embedded knowledge engine", not "database" |
| 5 | Python floor | 3.10, abi3 |
| 6 | Human-only actions | Excluded; the guard scans the crate |
| 7 | Version | The workspace's, from the first release; API provisional until phase 2 |
| 8 | Result transport across the boundary | JSON text, parsed by Python's `json` — measured above; revisit only if a profile shows it |
| 9 | Store selection | One `detect` in `ekos_runtime`, used by the CLI and the engine |
| 10 | Embedding for vector search | Supplied by the caller; the read package makes no network call |

## Phases

| Phase | Content | Exit |
|---|---|---|
| 0 | Spike, this RFC | Done: measurements above, two engine tests |
| 1 | Full read surface, types, errors, `ekos.aio`, `detect` moved, parity + read-only + guard tests, CI `python` job, the wheels workflow, `docs/python.md` | Done 2026-11-28 (see "Phase 1 design"); the six-target wheel build is checked by the workflow's first run |
| 2 | `ekos-bin`, `ekos.Project`, `ekos.connect`, the console imports the package's client, embedded-vs-remote parity | A project compiled and read from Python end to end with no shell |
| 3 | Contribute (own RFC) | — |
| 4 | pandas / Arrow results, notebook display, LangChain / LlamaIndex retrievers that keep citations | incremental |

## Risks

- **API frozen too early.** The dataclasses are the contract and the package is marked provisional
  until phase 2; the engine's JSON is free to follow `Runtime`.
- **Drift from MCP.** The engine shapes its own JSON instead of calling `mcp.rs` (which lives in the
  CLI). The parity test is what holds the two together; a new MCP read tool without an engine
  counterpart is listed there as deliberately absent, or the test fails.
- **A request for `db.add()`.** Decision 2. The answer is the inbox RFC.
- **Build matrix.** Six targets, two wheels at phase 2. The engine wheel builds in under two minutes.
- **Until phase 1 ships, the package is roadmap** and is described that way wherever it is mentioned.

## Open

- The PyPI names `ekos` and `ekos-bin` are free (2026-11-28) and not yet registered. Registration
  needs the owner's PyPI account.
- Windows and macOS wheels are untested until the CI job exists.
