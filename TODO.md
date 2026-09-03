# TODO — EKOS Development Plan

Status legend: `[ ]` not started · `[~]` in progress · `[x]` done

---

## Phase -1 — RFC Process (pre-code)

**Goal:** Establish architectural contracts for every major subsystem, written just-in-time before the
phase that implements them.

**Context:** EKOS is a novel system with no obvious prior art to copy. Without written RFCs, every
implementation decision becomes a guess that future phases will contradict. RFCs freeze interfaces,
explain trade-offs, and give Claude Code stable contracts to implement against. An RFC is accepted when
it has been reviewed, all open questions are resolved, and the proposed design is consistent with the
overall compiler architecture documented in `ekos.md`.

**Just-in-time authoring:** Do NOT write all eight RFCs before coding starts — for a small team that
is months of design without feedback, and early RFCs would be invalidated by what implementation
teaches. Only the spike, RFC 0001, and RFC 0002 gate the start of coding (Phases 0–2). Every other
RFC must be accepted before *its consuming phase* starts: RFC 0006 before Phase 3, RFC 0003 before
Phase 5, RFC 0008 before Phase 6, RFC 0007 before Phase 7, RFC 0004 before Phase 9, RFC 0005 before
Phase 10.

**Inputs:** `ekos.md` (vision + architecture), `ekos_todo.md` (roadmap), this TODO.

**Outputs:** `docs/rfcs/` directory containing one Markdown file per RFC, each accepted and merged
before its corresponding implementation phase begins.

**Validation:** Every RFC file exists, contains all required sections (Motivation, Design, Alternatives,
Open Questions), and has a status of `Accepted` in its header. No phase 0–14 task is started until
the RFC for *that phase* is accepted (per the just-in-time schedule above — later RFCs may still be
unwritten while earlier phases are being implemented).

---

- [x] **Establish `docs/rfcs/` directory and RFC template**
  - *What:* Create `docs/rfcs/` and write `docs/rfcs/0000-template.md` with required sections:
    `Status`, `Motivation`, `Design`, `Alternatives Considered`, `Open Questions`, `Acceptance Criteria`.
  - *Output:* `docs/rfcs/0000-template.md` exists and is committed.
  - *Test/Validate:* `ls docs/rfcs/0000-template.md` exits 0. Template contains all six required
    section headings.

- [x] **Spike: end-to-end knowledge-recovery prototype (throwaway, do this FIRST)**
  - *What:* The riskiest hypothesis in the whole project is Phase 6 ("an LLM can reliably recover
    business meaning from SQL/Git"), yet it isn't tested until five phases of infrastructure are
    built. De-risk it now: a 1–2 day throwaway script (any language) that reads
    `tests/fixtures/ecommerce.sql`, calls Claude, and prints extracted entities + relationships +
    evidence fragments as JSON. No compiler infra, no crates.
  - *Output:* `docs/spikes/recovery-spike.md` — results write-up with sample output, prompt used,
    observed failure modes. The script itself may be discarded.
  - *Test/Validate:* Spike correctly identifies ≥ 80% of the entities and FK relationships in the
    fixture. Findings feed directly into RFC 0003 (KIR shape) and RFC 0008 (LLM policy). If the
    spike fails badly, the roadmap is rethought before any Rust is written — that is the point.

- [x] **RFC 0001 — Compiler Core architecture**
  - *What:* Define the `Compiler` lifecycle, the `CompilerPass` trait interface, how `PassManager`
    orders passes, what `Scheduler` controls, and the top-level data flow from CLI invocation through
    pass execution to output artifact. Specify error propagation and cancellation semantics. Must
    also decide the concurrency model up front: sync or async (tokio) end-to-end — `Observer::scan`
    (Phase 3), `LlmProvider::complete` (Phase 6), and parallel scheduling (Phase 13) must all share
    one model, because retrofitting async through a trait hierarchy later means rewriting every trait.
  - *Output:* `docs/rfcs/0001-compiler-core.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: What is the signature of `CompilerPass::run()`? How are pass
    dependencies declared? What happens when a pass fails — do subsequent passes still run? Is the
    pipeline sync or async, and if async, is tokio adopted from Phase 0?

- [x] **RFC 0002 — Artifact system and content-addressing scheme**
  - *What:* Define the artifact type hierarchy, the fields every artifact must carry (id, checksum
    algorithm, metadata shape, dependency list, version), the on-disk storage layout under `.ekos/`,
    the cache-hit / cache-miss decision algorithm, and serialization format (JSON v1).
  - *Output:* `docs/rfcs/0002-artifact-system.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: What hashing algorithm for content-addressing? What is the on-disk
    path formula for a given artifact id? How are artifact dependencies expressed?

- [x] **RFC 0003 — Knowledge Intermediate Representation (KIR)**
  - *What:* Define the four KIR node types (`KirObject`, `KirRelationship`, `KirEvent`,
    `KirEvidence`), their mandatory and optional fields, how ids are assigned, how Evidence links to
    its source artifact, and the JSON schema for serialized KIR. Must define a **single evidence
    model** used across the whole system: `KirEvidence` is the one canonical type; Phase 2's
    `EvidenceArtifact` is its storage wrapper and Phase 8's `EvidenceRecord` is a denormalized
    projection of it — three views of one type, never three independently evolving types.
  - *Output:* `docs/rfcs/0003-kir.md` with status `Accepted`.
  - *Test/Validate:* RFC includes a worked example: one SQL table → one `KirObject` with two
    `KirEvidence` nodes showing exact JSON shape.

- [x] **RFC 0004 — Semantic Knowledge Ledger** — **the one genuinely missing RFC document, found by
  the 2026-08-26 audit and written retroactively that same day** (`docs/rfcs/0004-semantic-
  knowledge-ledger.md`, not the `0004-ledger.md` filename originally sketched below — matches this
  directory's own real naming convention, e.g. `0001-compiler-core.md`). The ledger itself was
  real and built (this session worked with it extensively); only the design document was skipped
  at the time. Written to describe what was actually shipped, including one real, still-open gap
  it surfaced: `source_artifact_id`/full audit-trail provenance was never implemented (see the
  Phase 9 item below, corrected to reflect this).
  - *What:* Define the ledger's append-only guarantees, the entry format, indexing strategy for
    current-state and historical queries, and how the ledger enforces immutability at the API level.
    Must make two explicit decisions. (1) **Event-sourcing vs. snapshots:** `ekos.md` declares
    "events are the only mechanism that changes enterprise state" (state = a fold over events), but
    the Phase 9 tasks as written store full object snapshots — these imply different schemas; pick
    one and adjust Phase 9 accordingly. (2) **Storage engine:** SQLite is acceptable as an
    explicitly disposable v0.x backend, but it must live behind a `LedgerBackend` trait, and the RFC
    must document what SQLite does NOT solve — concurrent writers, unbounded append-only growth and
    compaction, and branch-by-file-copy — so the v1.0 backend swap is planned, not a rescue.
  - *Output:* `docs/rfcs/0004-ledger.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: Can an entry ever be deleted? Snapshot or event-sourced — and how
    is current state reconstructed? What happens on a write failure mid-append? What is the
    migration path off SQLite when a single enterprise ledger exceeds ~10 GB or needs concurrent
    writers?

- [x] **RFC 0005 — Runtime and state reconstruction**
  - *What:* Define the Runtime's read-only API, how it reconstructs current and historical object
    state from ledger events, the `Neighborhood` concept (depth-bounded graph traversal), and the
    interface the AI layer will call.
  - *Output:* `docs/rfcs/0005-runtime.md` with status `Accepted`.
  - *Test/Validate:* RFC includes a worked example: ledger with three events on one Object → Runtime
    returns correct reconstructed state showing field-by-field evolution.

- [x] **RFC 0006 — Observation SDK and connector contract**
  - *What:* Define the `Observer` trait signature, `ScanContext` contents (config, logger, progress
    sink), `ObservationPackage` output structure, how connectors are discovered and loaded (static
    linking vs. dynamic plugins), and the versioning contract between SDK and plugins.
  - *Output:* `docs/rfcs/0006-observation-sdk.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: Can a connector be written in isolation without depending on
    `compiler-core`? What is the minimal `Cargo.toml` for a new connector crate?

- [x] **RFC 0007 — Identity Resolution algorithm**
  - *What:* Define the similarity scoring approach (name normalization, structural fingerprint,
    contextual embedding), the merge confidence threshold, how conflicts are surfaced, and the output
    format (canonical `KirObject` with provenance linking back to all merged sources).
  - *Output:* `docs/rfcs/0007-identity-resolution.md` with status `Accepted`.
  - *Test/Validate:* RFC includes a worked example: `Customer` (Postgres), `Buyer` (Confluence),
    `client` (Git commit message) → merged canonical Object with confidence score ≥ 0.85.

- [x] **RFC 0008 — LLM policy: determinism, caching, model pinning**
  - *What:* The coding rules require "every compiler pass deterministic", but Phase 6 passes call
    an LLM — inherently non-deterministic. This RFC resolves the contradiction: pin the model
    version, use temperature 0, cache every response keyed by content hash of (model, prompt,
    params), and treat a cached response as part of the build's input set. Re-running a build with
    a warm cache is then bit-for-bit reproducible; invalidating the cache is an explicit,
    audited action. Also define cost controls (token budgets per pass) and fallback behaviour when
    the LLM is unavailable (deterministic extraction still runs; LLM-derived knowledge is skipped
    with a diagnostic).
  - *Output:* `docs/rfcs/0008-llm-policy.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: What exactly is hashed for the cache key? What happens to cached
    knowledge when the pinned model version is upgraded? How is LLM-derived knowledge distinguished
    from deterministically extracted knowledge in Evidence confidence scores?

---

## Phase 0 — Bootstrap

**Goal:** A Cargo workspace that compiles and tests cleanly on a fresh clone.

**Context:** Before any domain logic is written, the build toolchain, CI pipeline, and repository
skeleton must exist. This phase has zero business logic — its only job is proving that the development
environment is reproducible. Every subsequent phase depends on `cargo build --workspace` being green.

**Inputs:** None (greenfield).

**Outputs:** A Cargo workspace at `ekos/` with skeletal crates, a passing CI pipeline, a Docker dev
image, and a `ekos --help` CLI that runs without panicking.

**Validation:**
```bash
git clone <repo> && cd ekos
cargo build --workspace      # exits 0
cargo test --workspace       # exits 0
cargo clippy --workspace     # zero warnings
cargo fmt --check            # exits 0
ekos --help                  # prints usage, exits 0
```

---

- [x] **Initialise Cargo workspace (`ekos/Cargo.toml`)**
  - *What:* Create `ekos/Cargo.toml` as a `[workspace]` manifest listing all planned member crates:
    `crates/compiler-core`, `crates/compiler-sdk`, `crates/scheduler`, `crates/artifact`,
    `crates/observation-sdk`, `crates/cli`, `crates/common`. Set `resolver = "2"` and
    `edition = "2024"` in each member's `Cargo.toml`. Each member has an empty `src/lib.rs`
    (or `src/main.rs` for `cli`).
  - *Output:* `ekos/Cargo.toml` workspace manifest; `ekos/crates/*/Cargo.toml`; `ekos/crates/*/src/lib.rs`.
  - *Test/Validate:* `cargo build --workspace` from `ekos/` exits 0 with no source files beyond
    empty `lib.rs` stubs.

- [x] **Scaffold crate skeletons: `compiler-core`, `compiler-sdk`, `scheduler`, `artifact`, `observation-sdk`, `cli`, `common`**
  - *What:* For each crate, add a `[package]` section with correct name, version `0.1.0`, edition
    `2024`. Add inter-crate dependencies (e.g., `cli` depends on `compiler-core`). Ensure no circular
    dependencies exist. `cli` gets `src/main.rs` with `fn main() {}`.
  - *Output:* All crates compile individually (`cargo build -p <crate>`) and as a workspace.
  - *Test/Validate:* `for crate in compiler-core compiler-sdk scheduler artifact observation-sdk cli common; do cargo build -p $crate; done` — all exit 0.

- [x] **GitHub Actions CI: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`**
  - *What:* Create `.github/workflows/ci.yml` with a single job that runs on `push` and
    `pull_request` to `main`. Steps: checkout → install stable Rust toolchain → `cargo build
    --workspace` → `cargo test --workspace` → `cargo clippy --workspace -- -D warnings` → `cargo fmt
    --check`.
  - *Output:* `.github/workflows/ci.yml`; a green CI run on the first push.
  - *Test/Validate:* Push a branch; GitHub Actions shows all steps green. Introduce a `clippy`
    warning intentionally; confirm CI fails on that step.

- [x] **Docker development image** — done (2026-08-26), the one genuinely missing item found by a
  direct spot-check audit of Phase -1 through Phase 13 (most items in that range were already
  implemented — real work, just never had their stale planning-template checkbox updated; see the
  "Phase -1 through 13 checkbox audit" entry below for the rest). `Dockerfile.dev` (repo root):
  `rust:1.93-slim` (the real version this workspace already builds with, `rustc --version` —
  matching the spec's `rust:1.XX-slim` template, not inventing a new toolchain-pinning policy; CI
  itself still floats on `stable`), `build-essential`/`pkg-config` (checked `ekos/Cargo.toml`
  directly first: only `zstd-sys`'s C build script needs a compiler — `rusqlite` uses its
  `bundled` feature, `reqwest` uses `rustls-tls`, no OpenSSL/protobuf anywhere), `rustfmt`+`clippy`
  components (matching `.github/workflows/ci.yml` exactly). `docker-compose.dev.yml`: mounts the
  repo, named volumes for `target/`/the cargo registry (a bind-mounted `target/` would otherwise
  mix container-built Linux/glibc artifacts with host-built ones). *Test/Validate*: live-verified
  for real — `docker compose -f docker-compose.dev.yml build` succeeds, then `docker compose -f
  docker-compose.dev.yml run --rm ekos cargo build -p ekos-kir` really compiles a real EKOS crate
  inside the container end to end (dependency resolution, a real C-backed crate build via
  `zstd-sys`, successful link) — not just "the image builds," the container's toolchain actually
  produces this workspace's real binaries.

- [x] **`ekos --help` produces output without panicking**
  - *What:* Wire up a minimal CLI in `crates/cli/src/main.rs` using `clap` (derive API). Define the
    top-level `ekos` command with `--version` and `--help`. No subcommands yet — just the skeleton.
  - *Output:* Binary `ekos` built by `cargo build -p cli`. Running `ekos --help` prints name,
    version, and usage line; exits 0.
  - *Test/Validate:* `cargo run -p cli -- --help` prints usage and exits 0. `cargo run -p cli --
    --version` prints `ekos 0.1.0` and exits 0.

---

## Phase 1 — Compiler Core

**Goal:** Build the compiler's infrastructure skeleton — pass management, scheduling, diagnostics,
config, and logging — with no enterprise or AI logic.

**Context:** This phase is to EKOS what LLVM's `PassManager` is to a C++ compiler: the machinery
that orchestrates compilation without knowing anything about what is being compiled. Getting this right
before writing any passes is critical because every future phase plugs into these abstractions.
Correctness here means deterministic, testable, dependency-ordered pass execution with rich diagnostics.

**Inputs:** Phase 0 workspace skeleton; RFC 0001 (Compiler Core architecture).

**Outputs:** `compiler-core` crate with public traits and structs for `Compiler`, `PassManager`,
`Scheduler`, `Diagnostics`, `Configuration`, `Logging`; CLI subcommands `init`, `build`, `clean`,
`doctor`.

**Validation:**
```bash
cargo test -p compiler-core         # all unit tests pass
cargo run -p cli -- init            # creates .ekos/ directory
cargo run -p cli -- doctor          # prints environment check, exits 0
cargo run -p cli -- build           # runs zero passes, prints "Build complete", exits 0
```

---

- [x] **`compiler-core`: `Compiler` struct and lifecycle**
  - *What:* Define `pub struct Compiler` in `crates/compiler-core/src/lib.rs`. It holds a
    `PassManager`, a `Configuration`, and a `DiagnosticSink`. Implement `Compiler::new(config) ->
    Self` and `Compiler::run() -> Result<(), CompilerError>` which delegates to `PassManager::run_all()`.
  - *Output:* `Compiler` struct with `new` and `run` methods; `CompilerError` enum.
  - *Test/Validate:* Unit test: `Compiler::new(default_config()).run()` with zero registered passes
    returns `Ok(())`. With a pass that returns `Err`, `run()` propagates the error.

- [x] **`compiler-core`: `PassManager` — registers and sequences compiler passes**
  - *What:* Define `pub trait CompilerPass` with `fn name(&self) -> &str`, `fn dependencies(&self)
    -> &[&str]`, and `fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError>`. Implement
    `PassManager` that holds `Vec<Box<dyn CompilerPass>>`, validates the dependency DAG (reject
    cycles), and returns an execution order via topological sort.
  - *Output:* `CompilerPass` trait; `PassManager::register()`, `PassManager::execution_order()`,
    `PassManager::run_all()`.
  - *Test/Validate:* Unit tests: (1) three passes A→B→C returns order [A, B, C]; (2) cycle A→B→A
    returns `Err(CycleDetected)`; (3) unknown dependency returns `Err(UnknownDependency)`.

- [x] **`compiler-core`: `Scheduler` — controls pass execution order and dependencies**
  - *What:* `Scheduler` wraps `PassManager` and adds execution policy: sequential (default),
    with hooks for future parallel execution. Exposes `Scheduler::execute(passes, ctx)` which runs
    passes in declared order, collecting all diagnostics rather than stopping at the first error
    (configurable via `FailureMode::FailFast | FailureMode::Collect`).
  - *Output:* `Scheduler` struct; `FailureMode` enum; `ExecutionReport` containing pass outcomes.
  - *Test/Validate:* Unit test: two passes where the second fails — in `Collect` mode both run and
    `ExecutionReport` contains two entries; in `FailFast` mode the second pass does not run.

- [x] **`compiler-core`: `Diagnostics` — structured error and warning reporting**
  - *What:* Define `Diagnostic { severity: Severity, code: &str, message: String, location: Option<SourceLocation> }` and `DiagnosticSink` (collects diagnostics during a build). `Severity` = `Error | Warning | Info`. Implement `DiagnosticSink::emit()`, `::errors()`, `::has_errors()`.
  - *Output:* `Diagnostic`, `DiagnosticSink`, `Severity`, `SourceLocation` types in `compiler-core`.
  - *Test/Validate:* Unit test: emit two warnings and one error; assert `has_errors()` = true;
    assert `errors().len()` = 1; assert total `diagnostics().len()` = 3.

- [x] **`compiler-core`: `Configuration` — typed config loading**
  - *What:* Define `EkosConfig` struct with fields for workspace root, artifact cache directory,
    log level, and enabled connectors. Implement loading from `ekos.toml` at the workspace root using
    `toml` crate. Provide `EkosConfig::default()` and `EkosConfig::from_file(path) -> Result<Self>`.
  - *Output:* `EkosConfig` struct; `ekos.toml` example file at repo root; config loading and
    validation logic.
  - *Test/Validate:* Unit test: parse a fixture `ekos.toml` string; assert field values match.
    Pass a malformed TOML; assert `from_file` returns `Err`.

- [x] **`compiler-core`: `Logging` — structured, levelled, per-crate**
  - *What:* Initialise `tracing` / `tracing-subscriber` in `compiler-core`. Configure log level from
    `EkosConfig`. Each crate uses `tracing::instrument` on its public entry points. Log format:
    structured JSON in CI/production, human-readable in development (controlled by `EKOS_LOG_FORMAT`
    env var).
  - *Output:* `init_logging(config: &EkosConfig)` function in `compiler-core`; `tracing` calls in
    all public functions.
  - *Test/Validate:* `EKOS_LOG=debug cargo run -p cli -- doctor 2>&1 | grep '"level":"DEBUG"'`
    finds at least one structured log line.

- [x] **`cli`: `ekos init`**
  - *What:* Subcommand that creates a `.ekos/` directory at the current workspace root containing:
    `config/` (empty), `artifacts/` (empty), `ledger/` (empty). Writes a default `ekos.toml` if none
    exists. Idempotent — safe to run twice.
  - *Output:* `.ekos/` directory tree on disk; default `ekos.toml`.
  - *Test/Validate:* Run `ekos init` in an empty directory; assert `.ekos/artifacts/`, `.ekos/ledger/`
    exist. Run again; assert no error and no duplicate files.

- [x] **`cli`: `ekos build`**
  - *What:* Subcommand that loads `ekos.toml`, constructs a `Compiler`, registers configured passes
    (none yet), runs the compiler, and prints a summary of the `ExecutionReport`. Exit code 0 on
    success, non-zero if any `Error`-severity diagnostic was emitted.
  - *Output:* Running `ekos build` prints `Build complete. 0 passes run, 0 errors.` and exits 0.
  - *Test/Validate:* `cargo run -p cli -- build` exits 0 and prints the summary line. Inject a
    failing pass via test harness; assert non-zero exit and error message on stderr.

- [x] **`cli`: `ekos clean`**
  - *What:* Subcommand that deletes `.ekos/artifacts/` contents (cached artifacts) but preserves
    `.ekos/ledger/` and `ekos.toml`. Prints count of deleted files.
  - *Output:* Artifact cache cleared; ledger untouched.
  - *Test/Validate:* Create dummy files in `.ekos/artifacts/`; run `ekos clean`; assert artifact
    files gone. Assert `.ekos/ledger/` still exists.

- [x] **`cli`: `ekos doctor`**
  - *What:* Subcommand that checks the environment and prints a status report: Rust version, workspace
    root location, `ekos.toml` validity, `.ekos/` directory presence, writability of artifact cache.
    Each check prints `[OK]` or `[FAIL]` with a description.
  - *Output:* Human-readable diagnostic report on stdout; exits 0 if all checks pass, 1 if any fail.
  - *Test/Validate:* `ekos doctor` in a properly initialised workspace prints all `[OK]` and exits 0.
    Remove `ekos.toml`; assert `ekos doctor` prints `[FAIL]` for config check and exits 1.

---

## Phase 1.5 — Walking Skeleton (vertical slice)

**Goal:** One thin end-to-end path — observe a directory, produce minimal knowledge, store it,
query it back — before any single layer is widened.

**Context:** Without this phase, nothing user-visible exists until Phase 11 (`ekos ask`) — a pure
waterfall where integration risk accumulates silently for months. The skeleton exercises every
interface boundary (observer → artifact → KIR → ledger → query) while each piece is still small
enough to change cheaply; interface mistakes surface in days, not phases. Quality bar: real crates,
real tests, minimal scope. Each piece here is deliberately a stub that a later phase replaces or
widens — that is by design, not technical debt. No LLM, no identity resolution, no CKM, no SDK.

**Inputs:** Phase 1 compiler core (Compiler, PassManager, CLI skeleton).

**Outputs:** `ekos init && ekos build && ekos query object <id>` works end-to-end against a
directory of files, with evidence attached to every stored object.

**Validation:**
```bash
ekos init
ekos build          # observes fixture dir, writes minimal KIR to the skeleton ledger
ekos query object <id-printed-by-build>   # returns JSON with name + evidence
cargo test --test skeleton                # end-to-end test passes in CI
```

---

- [x] **Minimal file observer (inline, pre-SDK)**
  - *What:* A hard-coded pass inside `compiler-core` (no `Observer` trait yet) that walks a
    configured directory and emits one observation per file (path, size, sha256). Replaced by the
    real SDK-based connectors in Phases 3–4.
  - *Output:* `ekos build` prints "N files observed" for the fixture directory.
  - *Test/Validate:* Run against `tests/fixtures/sample_project/`; assert observation count equals
    the known fixture file count.

- [x] **Minimal KIR subset (`KirObject` + `KirEvidence` only)**
  - *What:* Just two of the four node types, with only mandatory fields, defined in `crates/kir`.
    Each observed file becomes a `KirObject(kind=File)` with one `KirEvidence` pointing at the file
    path. Extended to the full four-type model in Phase 5 (the crate and ids carry forward).
  - *Output:* `KirObject` and `KirEvidence` structs in `crates/kir`; build produces one object per file.
  - *Test/Validate:* Unit test: object → JSON → object round-trip. Build output artifact contains
    one object per fixture file, each with exactly one evidence node.

- [x] **Minimal ledger append + read-by-id**
  - *What:* A single SQLite table (`entries`) with `append(entry)` and `get(id)` — no indexes, no
    history, no integrity checking. `ekos build` writes each `KirObject` straight to it. Replaced by
    the full ledger in Phase 9 (same crate, same `LedgerBackend` trait shape).
  - *Output:* `crates/ledger` with the two-function skeleton API; `.ekos/ledger/ledger.db` populated
    by `ekos build`.
  - *Test/Validate:* Unit test: append then get returns the identical entry. After `ekos build`,
    row count in SQLite equals fixture file count.

- [x] **Minimal `ekos query object <id>`**
  - *What:* CLI subcommand that calls `ledger.get(id)` and prints the object as JSON, including its
    evidence. Widened into the full Runtime-backed query in Phase 10.
  - *Output:* `ekos query object <id>` subcommand.
  - *Test/Validate:* Query an id printed by `ekos build`; assert JSON output contains the file path
    in both `name` and the evidence fragment. Unknown id prints "Not found", exits 1.

- [x] **End-to-end skeleton test in CI**
  - *What:* One integration test (`tests/skeleton.rs`) that runs init → build → query
    programmatically against `tests/fixtures/sample_project/` and asserts the full loop. This test
    stays green through Phases 2–10 as each stub is swapped for its real implementation — it is the
    canary proving the pipeline never breaks while layers are widened.
  - *Output:* `cargo test --test skeleton` passes; wired into CI.
  - *Test/Validate:* CI runs the skeleton test on every PR. Deliberately break the ledger append;
    assert the skeleton test fails.

---

## Phase 2 — Artifact System

**Goal:** Make every compiler input and output a typed, content-addressable, cacheable artifact.

**Context:** Artifacts are the currency of the EKOS compiler — they flow between passes the same way
object files flow between compiler stages in a traditional build system. By making all data
content-addressable from the start, the compiler gains caching, reproducibility, and dependency
tracking for free. This phase has no business logic; it is pure infrastructure.

**Inputs:** Phase 1 `compiler-core` (PassContext, DiagnosticSink); RFC 0002 (artifact system).

**Outputs:** `artifact` crate with five artifact types, a content-addressable store, and serialization.
`compiler-core` updated to read/write/cache artifacts through a unified API.

**Validation:**
```bash
cargo test -p artifact              # all tests pass
cargo test -p compiler-core        # cache-hit tests pass
# Manual: write an artifact, read it back, mutate its content, verify checksum mismatch is detected
```

---

- [x] **`artifact`: `ObservationArtifact`**
  - *What:* `pub struct ObservationArtifact` containing: `id: ArtifactId`, `checksum: Checksum`,
    `metadata: ArtifactMeta`, `dependencies: Vec<ArtifactId>`, `version: u32`, `source: SourceRef`
    (connector name + target), `raw_data: serde_json::Value`. `ArtifactId` is a `[u8; 32]` SHA-256
    hash of content.
  - *Output:* `ObservationArtifact` type in `crates/artifact/src/observation.rs`; serializes/deserializes to JSON.
  - *Test/Validate:* Unit test: construct an `ObservationArtifact`, serialize to JSON, deserialize
    back, assert round-trip equality. Assert two artifacts with identical content produce identical ids.

- [x] **`artifact`: `KnowledgeArtifact`**
  - *What:* `pub struct KnowledgeArtifact` holding compiled KIR output: `id`, `checksum`, `metadata`,
    `dependencies: Vec<ArtifactId>` (points to source `ObservationArtifact`s), `version`, `kir:
    Vec<KirNode>` (placeholder type until Phase 5).
  - *Output:* `KnowledgeArtifact` type in `crates/artifact/src/knowledge.rs`; JSON serializable.
  - *Test/Validate:* Unit test: round-trip serialization. Assert `dependencies` field is preserved.

- [x] **`artifact`: `EvidenceArtifact`**
  - *What:* `pub struct EvidenceArtifact` holding provenance records: `id`, `checksum`, `metadata`,
    `source_artifact_id: ArtifactId`, `location: SourceLocation` (file, line, column), `fragment: String`
    (the raw text snippet that was the evidence). Links a knowledge claim to its source.
  - *Output:* `EvidenceArtifact` type in `crates/artifact/src/evidence.rs`; JSON serializable.
  - *Test/Validate:* Unit test: construct with a SQL snippet as fragment; serialize and deserialize;
    assert `fragment` and `location` are preserved exactly.

- [x] **`artifact`: `DiagnosticArtifact`**
  - *What:* `pub struct DiagnosticArtifact` collecting compiler diagnostics for a build run: `id`,
    `checksum`, `metadata`, `diagnostics: Vec<Diagnostic>` (from `compiler-core`). Allows storing
    and diffing diagnostic output across builds.
  - *Output:* `DiagnosticArtifact` type in `crates/artifact/src/diagnostic.rs`; JSON serializable.
  - *Test/Validate:* Unit test: create with two warnings; serialize; deserialize; assert `diagnostics.len() == 2`.

- [x] **`artifact`: `IndexArtifact`**
  - *What:* `pub struct IndexArtifact` acting as a manifest for a build run: `id`, `checksum`,
    `metadata`, `entries: HashMap<String, ArtifactId>` (logical name → artifact id). Used by the
    compiler to locate artifacts by name without scanning the store.
  - *Output:* `IndexArtifact` type in `crates/artifact/src/index.rs`; JSON serializable.
  - *Test/Validate:* Unit test: insert three entries; serialize; deserialize; assert all three entries
    are present by key lookup.

- [x] **Each artifact carries: unique id, checksum, metadata, dependencies, version**
  - *What:* Extract shared fields into `pub struct ArtifactMeta { created_at: DateTime<Utc>,
    produced_by: String, schema_version: u32 }` and a blanket `Artifact` trait with `fn id()`,
    `fn checksum()`, `fn meta()`, `fn dependencies()`, `fn version()`. All five types implement this trait.
  - *Output:* `Artifact` trait in `crates/artifact/src/lib.rs`; `ArtifactMeta` struct; all types impl trait.
  - *Test/Validate:* Unit test using the trait object: `let a: &dyn Artifact = &obs_artifact; assert_eq!(a.version(), 1)`.

- [x] **`compiler-core`: artifact read / write / cache / reuse API**
  - *What:* Add `ArtifactStore` to `compiler-core` with: `fn write<A: Artifact>(&self, artifact: A)
    -> Result<ArtifactId>`, `fn read<A: Artifact>(&self, id: &ArtifactId) -> Result<A>`, `fn
    exists(&self, id: &ArtifactId) -> bool`. `PassContext` gains an `artifact_store: &ArtifactStore`
    field so passes can read inputs and write outputs.
  - *Output:* `ArtifactStore` trait and filesystem implementation in `compiler-core`.
  - *Test/Validate:* Unit test: write an `ObservationArtifact`, read it back by id, assert equality.
    Call `exists()` for a known id (true) and an unknown id (false).

- [x] **Content-addressable artifact store (local filesystem backend)**
  - *What:* Implement `FileSystemArtifactStore` that stores artifacts at
    `.ekos/artifacts/<first-2-hex-bytes>/<full-id>.json` (Git object store layout). The id is the
    SHA-256 of the canonically serialized content **excluding volatile metadata** (`created_at` and
    similar wall-clock fields) — otherwise identical content hashed at different times yields
    different ids and the cache never hits. On write, compute hash, check if file already exists
    (cache hit), skip write if so.
  - *Output:* `FileSystemArtifactStore` in `compiler-core/src/store.rs`. On-disk files appear at
    the expected paths.
  - *Test/Validate:* Write the same artifact twice; assert only one file is written (check `mtime`
    or file count). Write two artifacts with different content; assert two different files exist.
    Construct the same logical artifact at two different wall-clock times; assert identical ids.

- [x] **Serialization for all artifact types (JSON initially)**
  - *What:* Derive `serde::Serialize` + `serde::Deserialize` on all artifact types and their
    constituent types. Add a `schema_version: u32` field to `ArtifactMeta` (currently `1`). Ensure
    all `DateTime` fields serialize as ISO-8601 strings.
  - *Output:* All five artifact types round-trip through `serde_json::to_string` / `from_str` without data loss.
  - *Test/Validate:* Property-based test (using `proptest` or hand-written): for each artifact type,
    construct with arbitrary field values, serialize, deserialize, assert structural equality.

---

## Phase 3 — Observation SDK

**Goal:** Define the public contract that all connectors must implement, and ship two reference connectors.

**Context:** The Observation SDK is EKOS's plugin boundary. It must be stable before any real connectors
are written, because changing the `Observer` trait later would break all existing connectors. The SDK
crate must have zero dependency on `compiler-core` internals — a connector author should be able to
implement `Observer` by depending only on `observation-sdk`. The two reference connectors (File, Git)
serve as the SDK's acceptance test and as copy-paste starting points.

**Inputs:** Phase 2 artifact system (specifically `ObservationArtifact`); RFC 0006 (Observation SDK contract).

**Outputs:** `observation-sdk` crate with `Observer` trait, `ScanContext`, `ObservationPackage`;
two working connectors in `plugins/file/` and `plugins/git/`; integration guide in `docs/`.

**Validation:**
```bash
cargo test -p observation-sdk
cargo test -p plugin-file
cargo test -p plugin-git
# Run file observer against the repo itself; assert ObservationPackage is written to disk
cargo run -p cli -- build   # should discover and run the file observer if configured
```

---

- [x] **`observation-sdk`: `Observer` trait (`fn scan(...) -> ObservationArtifact`)**
  - *What:* Define `pub trait Observer: Send + Sync { fn name(&self) -> &str; fn scan(&self, ctx:
    &ScanContext) -> Result<ObservationPackage, ObserverError>; }`. `ObserverError` is a structured
    error type (not `Box<dyn Error>`). The trait must be object-safe so connectors can be boxed.
  - *Output:* `Observer` trait in `crates/observation-sdk/src/lib.rs`.
  - *Test/Validate:* Write a `NoopObserver` that implements the trait and returns an empty package;
    box it as `Box<dyn Observer>`; call `scan()`; assert `Ok(empty_package)`.

- [x] **`observation-sdk`: `ScanContext` — passes config and logging into connectors**
  - *What:* `pub struct ScanContext { pub config: ConnectorConfig, pub logger: tracing::Span, pub
    artifact_store: Arc<dyn ArtifactStore>, pub workspace_root: PathBuf }`. `ConnectorConfig` is a
    `HashMap<String, serde_json::Value>` allowing arbitrary connector-specific settings loaded from
    `ekos.toml`. Connectors must not access global state — everything they need comes via `ScanContext`.
  - *Output:* `ScanContext` and `ConnectorConfig` structs in `observation-sdk`.
  - *Test/Validate:* Unit test: build a `ScanContext` with a mock config map; pass to `NoopObserver`;
    assert the connector reads a config value via `ctx.config.get("key")`.

- [x] **`observation-sdk`: `ObservationPackage` — output format**
  - *What:* `pub struct ObservationPackage { pub observer: String, pub target: String, pub artifacts:
    Vec<ObservationArtifact>, pub metadata: PackageMeta }` where `PackageMeta` includes `scanned_at:
    DateTime<Utc>`, `duration_ms: u64`, `item_count: usize`. The package is itself serializable to
    a directory: `snapshot/<observer-name>/package.json` + individual artifact JSON files.
  - *Output:* `ObservationPackage` type; `fn write_to_dir(&self, dir: &Path) -> Result<()>` method.
  - *Test/Validate:* Unit test: write a package with two artifacts to a temp dir; assert
    `snapshot/<name>/package.json` exists and `artifact_count` in JSON matches 2.

- [x] **Example connector: File Observer (reference implementation)**
  - *What:* Create `plugins/file/` crate depending only on `observation-sdk`. Implement `Observer`
    to walk a directory tree from `ctx.workspace_root`, emit one `ObservationArtifact` per file with
    fields: `path`, `size_bytes`, `sha256`, `modified_at`. Respects a `ignore_patterns` config field
    (gitignore-style).
  - *Output:* `plugins/file/src/lib.rs`; passes `cargo test -p plugin-file`.
  - *Test/Validate:* Integration test: run the File Observer against `tests/fixtures/sample_project/`;
    assert the returned package contains exactly the expected number of file artifacts, each with
    correct path and non-zero size.

- [x] **Example connector: Git Observer (basic)**
  - *What:* Create `plugins/git/` crate using `git2` crate. Implement `Observer` to walk the commit
    history of a repo at `ctx.workspace_root`, emitting one `ObservationArtifact` per commit with
    fields: `sha`, `author`, `timestamp`, `message`, `changed_files: Vec<String>`.
  - *Output:* `plugins/git/src/lib.rs`; passes `cargo test -p plugin-git`.
  - *Test/Validate:* Integration test: point the Git Observer at the EKOS repo itself (has at least
    one commit); assert the returned package contains at least one commit artifact with a non-empty
    `sha` and `author`.

- [~] **SDK documentation and integration guide** — **real, partial gap confirmed by the 2026-08-26
  Phase -1 through 13 audit**: `observation-sdk`'s public types (`Observer`, `ScanContext`,
  `ObservationPackage`) all carry real rustdoc (checked directly — the `Observer` trait's own
  contract doc comment is substantive, not a stub), and ~10 real, working connector crates under
  `plugins/` already serve as concrete, battle-tested reference implementations (`file`, `git`,
  `github`, `confluence`, `localdocs`, `pentaho`, `crypto`, `python`, `rust`, plus five
  scaffolded-only ones). What's genuinely still missing: a standalone `docs/connector-guide.md`
  walking a new connector author through the five steps this item named — never written. A real,
  well-scoped, low-risk documentation task for whoever picks it up next, not a design gap.
  - *What:* Write `docs/connector-guide.md` explaining: (1) how to create a new connector crate,
    (2) the minimal `Cargo.toml`, (3) how to implement `Observer`, (4) how to register it in
    `ekos.toml`, (5) how to test it. Include a complete minimal example (copy of `NoopObserver`).
  - *Output:* `docs/connector-guide.md`; all public types in `observation-sdk` have rustdoc.
  - *Test/Validate:* A developer following the guide from scratch can produce a working connector
    without reading any `compiler-core` source. `cargo doc -p observation-sdk --open` shows complete
    API documentation.

---

## Phase 4 — Observation Compiler

**Goal:** Ship production-grade connectors for Git, filesystem, PostgreSQL, and SQL Server.

**Context:** This is the first phase where EKOS touches real enterprise systems. The connectors must
be robust (retry logic, partial failure handling), faithful (they record facts, never interpret them),
and produce `ObservationPackage`s that are rich enough for the Knowledge Recovery passes in Phase 6.
The output of this phase — a `snapshot/` directory — is the input to all downstream compilation phases.

**Inputs:** Phase 3 Observation SDK; real or dockerized PostgreSQL / SQL Server / Git instances for
integration tests.

**Outputs:** Four production connector plugins; `snapshot/` directory written to disk by `ekos build`.

**Validation:**
```bash
# Start fixture databases via docker-compose
docker compose -f tests/docker-compose.yml up -d
cargo test -p plugin-postgres --features integration
cargo test -p plugin-sqlserver --features integration
cargo run -p cli -- build   # writes snapshot/ directory
ls snapshot/git/ snapshot/database/ snapshot/files/ snapshot/metadata.json
```

---

- [x] **Plugin: `git` — commits, branches, authors, diffs**
  - *What:* Extend the Phase 3 basic Git Observer into a full plugin. Emit separate artifact types
    for: `CommitArtifact` (sha, author, date, message, stats), `BranchArtifact` (name, tip sha,
    upstream), `DiffArtifact` (changed files, hunks, line counts per commit). Handle repos with
    10k+ commits by streaming rather than loading all history into memory.
  - *Output:* `plugins/git/` crate with full implementation; artifacts cover commits, branches, diffs.
  - *Test/Validate:* Integration test against the EKOS repo: assert commit count > 0, at least one
    branch artifact, diff artifacts contain file paths matching known changed files.

- [x] **Plugin: `filesystem` — directory trees, file metadata**
  - *What:* Extend Phase 3 File Observer. Emit: `FileArtifact` (path, size, sha256, mime_type,
    modified_at), `DirectoryArtifact` (path, child count, total size). Respect `.gitignore` and
    a configurable `exclude_patterns` list. Handle symlinks safely (record target, do not follow).
  - *Output:* `plugins/filesystem/` (or extend `plugins/file/`); directory tree faithfully captured.
  - *Test/Validate:* Run against `tests/fixtures/sample_project/` (checked-in fixture with known
    structure); assert exact file count, total size, and presence of specific file paths in output.

- [x] **Plugin: `postgres` — schemas, tables, columns, constraints, views, functions** —
  **superseded by a different, real, shipped architecture (2026-08-26 audit)**: not a live-database
  connector using `sqlx`/`tokio-postgres` as originally planned — instead, `sql_analyzer.rs` +
  `plugins/sql-dialect-postgres` (RFC 0031's dialect-parser design) parses real Postgres DDL/DML
  text directly (schemas, tables, columns, constraints, views — the same artifact surface this item
  wanted), without needing a live database connection or credentials. A deliberate, real design
  divergence from the original plan, not an unbuilt gap — DDL-file analysis needs no running
  database, matches how every other SQL dialect in this project is handled uniformly, and is what
  the real committed fixtures (`ecommerce.sql`, `northwind.sql`) actually exercise today.
  - *What:* Create `plugins/postgres/` using `sqlx` or `tokio-postgres`. Query information schema
    and pg_catalog to emit: `TableArtifact` (name, schema, columns with types/nullability),
    `ConstraintArtifact` (PK, FK, UNIQUE, CHECK), `ViewArtifact` (name, definition SQL),
    `FunctionArtifact` (name, language, body). Handle multiple schemas.
  - *Output:* `plugins/postgres/` crate; integration test fixture database (Dockerfile).
  - *Test/Validate:* Integration test: start a Postgres container with `tests/fixtures/ecommerce.sql`
    loaded; run connector; assert `orders` table artifact exists with correct column names and types;
    assert FK constraint artifact links `orders.customer_id` to `customers.id`.

- [x] **Plugin: `sqlserver` — same as postgres surface** — **same supersession as `postgres` above**:
  `plugins/sql-dialect-mssql` + `sql_analyzer.rs` covers this via DDL-text parsing, not a live
  `tiberius` connection.
  - *What:* Create `plugins/sqlserver/` using `tiberius`. Emit the same artifact types as the
    Postgres plugin (TableArtifact, ConstraintArtifact, ViewArtifact, FunctionArtifact) but query
    SQL Server's `INFORMATION_SCHEMA` and `sys.*` catalogs. Handle both Windows and SQL auth.
  - *Output:* `plugins/sqlserver/` crate; SQL Server integration test fixture.
  - *Test/Validate:* Integration test with SQL Server Express container: same assertions as Postgres
    fixture — table, constraint, view, and function artifacts all present with correct fields.

- [x] **Output: structured `ObservationPackage` per source written to `snapshot/`**
  - *What:* Update `ekos build` to iterate configured connectors, run each via `Observer::scan()`,
    and write results to `snapshot/<connector-name>/` using `ObservationPackage::write_to_dir()`.
    Write `snapshot/metadata.json` with build timestamp, connector list, and total artifact counts.
  - *Output:* After `ekos build`, the `snapshot/` directory exists with one subdirectory per
    connector and a `metadata.json` at the root.
  - *Test/Validate:* `ekos build` with all four connectors configured; assert `snapshot/git/`,
    `snapshot/database/`, `snapshot/files/` all exist and each contains a `package.json`; assert
    `snapshot/metadata.json` lists all four connectors.

- [x] **`ekos build` drives observation and writes packages to disk**
  - *What:* Wire the `build` subcommand to: load `ekos.toml` connector list, instantiate each
    connector plugin, run them via `Scheduler` (sequentially for now), collect diagnostics, write
    `snapshot/`, print summary. Exit non-zero if any connector returns an error.
  - *Output:* `ekos build` is the single command that runs observation end-to-end.
  - *Test/Validate:* `ekos build --dry-run` (add dry-run flag) prints which connectors would run
    without actually connecting. Full run produces snapshot on disk as above.

---

## Phase 5 — Knowledge Intermediate Representation (KIR)

**Goal:** Define the canonical in-memory and on-disk representation that all compiler passes read and write.

**Context:** KIR is the assembly language of the EKOS compiler — the common format that observation
outputs are promoted into and that all knowledge passes consume and produce. Defining it before writing
any knowledge logic prevents the analysis passes from having conflicting internal representations.
KIR is intentionally minimal: it has exactly four node types and no semantic enrichment of its own.

**Inputs:** RFC 0003 (KIR specification); Phase 2 artifact system (KIR is stored as `KnowledgeArtifact`).

**Outputs:** dedicated `crates/kir` crate with four node types, serialization, and a `KirGraph`
container; no optimization or semantic enrichment. KIR must be its own crate (not a `compiler-core`
module) because `identity` (Phase 7) must be usable standalone without pulling in `compiler-core`.

**Validation:**
```bash
cargo test -p kir
# Manually: serialize a KirGraph with all four node types to JSON, inspect the output
```

---

- [x] **`KirObject` — identity node**
  - *What:* `pub struct KirObject { id: KirId, name: String, kind: ObjectKind, properties:
    HashMap<String, serde_json::Value>, evidence: Vec<KirId> }`. `ObjectKind` is an open enum
    (e.g., `Table`, `Entity`, `Service`, `Api`, `Unknown`). `KirId` is a `Uuid` v4.
  - *Output:* `KirObject` type with serde derives.
  - *Test/Validate:* Unit test: construct a `KirObject` of kind `Table` named `"orders"`, serialize
    to JSON, deserialize, assert all fields are preserved.

- [x] **`KirRelationship` — semantic connection**
  - *What:* `pub struct KirRelationship { id: KirId, kind: RelationshipKind, from: KirId, to: KirId,
    properties: HashMap<String, serde_json::Value>, evidence: Vec<KirId> }`. `RelationshipKind`:
    `ForeignKey`, `Calls`, `Extends`, `DependsOn`, `OwnedBy`, `Unknown`.
  - *Output:* `KirRelationship` type with serde derives.
  - *Test/Validate:* Unit test: construct a `ForeignKey` relationship between two `KirObject` ids;
    serialize and deserialize; assert `from` and `to` ids match.

- [x] **`KirEvent` — immutable change record**
  - *What:* `pub struct KirEvent { id: KirId, kind: EventKind, subject: KirId, timestamp:
    DateTime<Utc>, payload: serde_json::Value, evidence: Vec<KirId> }`. `EventKind`: `Created`,
    `Modified`, `Deleted`, `Migrated`, `Deployed`.
  - *Output:* `KirEvent` type with serde derives.
  - *Test/Validate:* Unit test: construct a `Created` event for a `KirObject`, round-trip through JSON.

- [x] **`KirEvidence` — provenance record**
  - *What:* `pub struct KirEvidence { id: KirId, source_artifact: ArtifactId, location:
    SourceLocation, fragment: String, confidence: f32 }`. `confidence` is [0.0, 1.0]. This is the
    only node type that references a raw artifact — it is the bridge from compiled knowledge back to
    raw observations.
  - *Output:* `KirEvidence` type with serde derives.
  - *Test/Validate:* Unit test: construct evidence with `confidence = 0.95`, serialize, deserialize,
    assert `confidence` is preserved with < 0.001 float tolerance.

- [x] **KIR serialization (JSON)**
  - *What:* Define `pub struct KirGraph { objects: Vec<KirObject>, relationships: Vec<KirRelationship>,
    events: Vec<KirEvent>, evidence: Vec<KirEvidence> }` with a `fn to_json(&self) -> String` and
    `fn from_json(s: &str) -> Result<Self>`. Store `KirGraph` inside a `KnowledgeArtifact`.
  - *Output:* `KirGraph` type; round-trip serialization via `KnowledgeArtifact`.
  - *Test/Validate:* Integration test: write a `KirGraph` with one node of each type as a
    `KnowledgeArtifact`; read it back from the artifact store; assert structural equality.

- [x] **No optimization or semantic enrichment — pure structural representation**
  - *What:* Code review / architecture gate — not a code change. Ensure no business logic has leaked
    into the KIR types. `KirObject.name` is whatever string came from the source; no normalization,
    no synonym resolution, no confidence scoring on objects themselves (only on evidence).
  - *Output:* A checklist review confirming the KIR module has zero dependencies on the LLM layer or
    the identity resolver.
  - *Test/Validate:* `cargo tree -p kir` (or grep `Cargo.toml`) shows no dependency on any LLM
    client crate or the `identity` crate.

---

## Phase 6 — Knowledge Recovery

**Goal:** Extract business meaning from raw observations using compiler passes and LLM assistance.

**Context:** This is the first phase where EKOS produces semantic knowledge, not just structural data.
Each analyzer is a `CompilerPass` that receives an `ObservationArtifact` (or a full
`ObservationPackage`) and emits a `KnowledgeArtifact` containing `KirObject`s, `KirRelationship`s,
and `KirEvidence`s. Deterministic extraction (FK constraints, column names) runs first; the LLM is
invoked only for ambiguous or implicit relationships.

**Inputs:** Phase 5 KIR types; Phase 4 `ObservationPackage`s in `snapshot/`; LLM API key in env.

**Outputs:** `KnowledgeArtifact` files in `.ekos/artifacts/`; `ekos recover` command.

**Validation:**
```bash
ekos recover --source snapshot/database/  # runs SqlAnalyzer
# Inspect output artifact JSON: assert KirObjects for each table, KirRelationships for each FK
cargo test -p compiler-core -- knowledge_recovery   # unit tests with fixture SQL
```

---

- [x] **Compiler pass: `SqlAnalyzer` → Business Entities, Relationships, Evidence from SQL**
  - *What:* Implement `SqlAnalyzer: CompilerPass`. Input: `ObservationArtifact` from the Postgres or
    SQL Server connector. Deterministic extraction: every table → `KirObject(kind=Table)`, every FK
    constraint → `KirRelationship(kind=ForeignKey)`, every column → property on the object. LLM
    extraction: send table names + column names to the LLM with a prompt asking for likely business
    entity names and semantic relationships not expressed by FKs. Emit `KirEvidence` for each claim.
  - *Output:* `KnowledgeArtifact` with fully populated `KirGraph` stored in `.ekos/artifacts/`.
  - *Test/Validate:* Test with `tests/fixtures/ecommerce.sql`: assert `orders` table → `Order` object;
    assert `orders.customer_id` FK → `placed_by` relationship to `Customer`; assert each relationship
    has at least one `KirEvidence` node pointing back to the SQL artifact.

- [x] **Compiler pass: `GitAnalyzer` → change patterns, ownership, coupling**
  - *What:* Implement `GitAnalyzer: CompilerPass`. Input: `ObservationPackage` from the git connector.
    Extract: files that change together frequently → `KirRelationship(kind=CoupledWith)`, authors
    responsible for a path → `KirRelationship(kind=OwnedBy)`, modules that only one author touches
    → `KirObject` with `single_owner: true` property. LLM: interpret commit messages to tag commits
    with semantic labels (feature, bugfix, refactor, breaking-change).
  - *Output:* `KnowledgeArtifact` with ownership and coupling graph.
  - *Test/Validate:* Test against EKOS repo history: assert at least one `OwnedBy` relationship;
    assert commit artifacts are tagged with at least one semantic label.

- [x] **Compiler pass: `ConfluenceAnalyzer` → concepts and relationships from documentation**
  - *What:* Implement `ConfluenceAnalyzer: CompilerPass` (no Confluence connector yet — accept a
    directory of Markdown files as input for now). Parse Markdown, extract headings as candidate
    `KirObject`s, extract links between pages as `KirRelationship(kind=References)`. Use LLM to
    identify business concepts, definitions, and rules mentioned in the text. Emit `KirEvidence`
    citing the paragraph.
  - *Output:* `KnowledgeArtifact` from documentation.
  - *Test/Validate:* Test with `tests/fixtures/sample_docs/` (a few Markdown files). Assert headings
    become objects; assert cross-page links become relationships; assert at least one business rule
    is extracted as a `KirObject(kind=BusinessRule)`.

- [x] **LLM integration layer (provider-agnostic trait, first backend: Anthropic Claude)**
  - *What:* Define `pub trait LlmProvider: Send + Sync { async fn complete(&self, prompt: &str,
    max_tokens: u32) -> Result<String, LlmError>; }` in a new `crates/llm/` crate (or `common`).
    Implement `ClaudeProvider` using the Anthropic API (`claude-sonnet-4-6` model, streaming
    optional). Configuration via `ekos.toml` `[llm]` section: `provider = "claude"`, `api_key_env =
    "ANTHROPIC_API_KEY"`. Retry logic: 3 attempts with exponential backoff on rate-limit errors.
  - *Output:* `LlmProvider` trait; `ClaudeProvider` implementation; `ANTHROPIC_API_KEY` env var used.
  - *Test/Validate:* Unit test with a mock `LlmProvider`. Integration test (gated by
    `--features llm-integration`): send a real prompt to Claude; assert non-empty response string.

- [x] **LLM response cache (determinism + cost control, per RFC 0008)**
  - *What:* Implement `CachedLlmProvider` — a decorator around any `LlmProvider` that stores every
    response as an artifact keyed by SHA-256 of (model id, prompt, params). On cache hit, return
    the stored response without an API call. This is what makes LLM-based passes reproducible
    (warm cache ⇒ identical output) and makes re-runs free during development.
  - *Output:* `CachedLlmProvider` in the `llm` crate; cache artifacts under `.ekos/artifacts/llm/`.
  - *Test/Validate:* Unit test: two identical `complete()` calls hit the inner mock provider exactly
    once (call counter == 1). Changing one character of the prompt busts the cache (counter == 2).

- [x] **Recovery quality eval harness (golden dataset)**
  - *What:* Unit tests prove the code runs; they cannot tell whether the LLM extracted the *right*
    entities. Create `tests/eval/` with fixture inputs and hand-labelled expected KIR (golden
    files). An eval runner compares analyzer output against the labels and computes precision and
    recall for objects and relationships. This is the regression net for every future prompt change.
  - *Output:* `tests/eval/` golden dataset; an eval runner (`ekos eval` or a cargo test target)
    printing per-analyzer precision/recall.
  - *Test/Validate:* Eval on the ecommerce fixture reports ≥ 0.8 precision and recall for entities
    and FK relationships. Results are committed alongside prompt changes so quality drift is visible
    in review.

- [x] **`cli`: `ekos recover` command**
  - *What:* New subcommand that loads configured analyzers from `ekos.toml`, runs them via
    `PassManager` against the `snapshot/` directory produced by `ekos build`, and writes
    `KnowledgeArtifact`s to `.ekos/artifacts/`. Print a summary: passes run, objects discovered,
    relationships discovered, errors.
  - *Output:* `ekos recover` command; `KnowledgeArtifact` files on disk.
  - *Test/Validate:* `ekos build && ekos recover` on a repo with the Postgres fixture: assert
    `.ekos/artifacts/` contains at least one `KnowledgeArtifact`; print summary shows > 0 objects.

---

## Phase 7 — Identity Resolution

**Goal:** Merge synonymous concepts discovered across different sources into single canonical identities.

**Context:** After Phase 6, the knowledge graph contains multiple objects that refer to the same
real-world concept: `Customer` from the database, `Buyer` from Confluence, `client` from Git commit
messages. Without merging, every downstream query returns fragmented results. Identity Resolution is
architecturally separate from the compiler because it is a standalone capability that can be reused
by other systems.

**Inputs:** Phase 6 `KnowledgeArtifact`s containing raw (unresolved) `KirObject`s.

**Outputs:** Updated `KirGraph` where synonymous objects are merged into canonical objects with
provenance; `identity` crate usable as a standalone library.

**Validation:**
```bash
cargo test -p identity
ekos resolve   # new CLI command
# Inspect output: Customer/Buyer/client merged into one canonical object with confidence score
```

---

- [x] **`identity`: resolver trait and algorithm**
  - *What:* Define `pub trait IdentityResolver { fn resolve(&self, graph: &KirGraph) ->
    Result<ResolvedGraph, ResolverError>; }` where `ResolvedGraph` wraps `KirGraph` with an added
    `canonical_map: HashMap<KirId, KirId>` (original id → canonical id). Implement
    `DefaultIdentityResolver` orchestrating the scoring pipeline below.
  - *Output:* `identity` crate at `crates/identity/`; `IdentityResolver` trait.
  - *Test/Validate:* Unit test: pass a `KirGraph` with two identical objects (same name, same kind);
    assert `canonical_map` merges them into one.

- [x] **Similarity scoring (name-based, structural, contextual)**
  - *What:* Implement three scoring functions, each returning `f32` in [0, 1]:
    (1) `name_score`: Levenshtein distance + common synonyms (`customer`/`client`/`buyer`) from a
    configurable synonym map; (2) `structural_score`: overlap in property names and types between
    two `KirObject`s; (3) `contextual_score`: cosine similarity of LLM embeddings of the object's
    name + properties (optional, requires `llm` crate). Final score: weighted average. Add a
    candidate-blocking step before pairwise scoring: bucket objects by normalized-name prefix and
    kind, and only score within buckets — naïve all-pairs comparison is O(n²) and unusable beyond
    ~10k objects.
  - *Output:* Three scoring functions; configurable weights in `ekos.toml` `[identity]` section.
  - *Test/Validate:* Unit tests: `name_score("customer", "client") > 0.7`;
    `name_score("customer", "product") < 0.3`; `structural_score` higher for structurally similar objects.

- [x] **Canonical entity merging: `Customer` + `Buyer` + `Client` → one `KirObject`**
  - *What:* When score exceeds the configured merge threshold (default 0.8), merge the objects:
    canonical name = highest-evidence name, properties = union of all properties (conflicts flagged as
    diagnostics), evidence = union of all evidence from all merged objects. Emit `KirEvent(kind=Merged)`
    recording which ids were merged and the merge confidence.
  - *Output:* `merge(objects: &[&KirObject]) -> KirObject` function; merged object retains all evidence.
  - *Test/Validate:* Unit test: merge `Customer` (DB) and `Buyer` (Confluence); assert merged object
    has `evidence.len() == sum of both`; assert a `Merged` event was emitted; assert the original
    ids map to the canonical id in `canonical_map`.

- [x] **Confidence scoring on merges**
  - *What:* Each entry in `canonical_map` carries a `MergeRecord { canonical_id, source_ids, score:
    f32, merge_reason: String }`. `score` is the weighted similarity score that triggered the merge.
    Merges below threshold but above a `review_threshold` (default 0.6) are flagged as
    `Warning`-severity diagnostics requiring human review.
  - *Output:* `MergeRecord` type; diagnostic warnings for low-confidence merges.
  - *Test/Validate:* Unit test: merge two objects with score 0.65 (between thresholds); assert a
    `Warning` diagnostic is emitted with the object names and score in the message.

- [x] **Conflict detection and reporting**
  - *What:* When two objects being merged have the same property key with different types or
    semantically incompatible values (e.g., `id` is `INT` in DB but `UUID` in API), emit an
    `Error`-severity diagnostic listing the conflict. The merge still proceeds but marks the
    conflicting property as `conflict: true` in the canonical object's properties.
  - *Output:* Conflict diagnostics in `DiagnosticArtifact`; `conflict` flag on merged properties.
  - *Test/Validate:* Unit test: merge two objects where `id` has type `Int` vs `Uuid`; assert one
    `Error` diagnostic with both type names in the message; assert merged object has
    `properties["id"]["conflict"] == true`.

- [x] **Reusable as standalone library**
  - *What:* Ensure `crates/identity/` has zero dependency on `compiler-core` or `cli`. Its only
    dependencies are `crates/kir` (or the KIR module) and optionally `crates/llm`. Publish a
    `README.md` in the crate explaining standalone usage with a minimal code example.
  - *Output:* `crates/identity/README.md`; `cargo package -p identity` succeeds without errors.
  - *Test/Validate:* Write a standalone binary in `examples/identity_standalone.rs` that builds
    a small `KirGraph` and runs the resolver, with no `compiler-core` import. `cargo run --example
    identity_standalone` exits 0 and prints the resolution result.

- [x] **`cli`: `ekos resolve` command**
  - *What:* Subcommand that loads all `KnowledgeArtifact`s, runs the `IdentityResolver`, writes the
    `ResolvedGraph` as a new artifact, and prints a merge summary: merges made, low-confidence merges
    flagged for review, conflicts detected.
  - *Output:* `ekos resolve` subcommand (referenced by this phase's Validation section).
  - *Test/Validate:* After `ekos recover` on the ecommerce fixture, `ekos resolve` exits 0, the
    resolved-graph artifact exists in the store, and the summary reports merge counts.

---

## Phase 8 — Semantic Compiler

**Goal:** Transform resolved KIR into the Canonical Knowledge Model (CKM), the final output of compilation.

**Context:** The CKM is the stable, denormalized representation that downstream consumers (Ledger,
Runtime, AI) depend on. Unlike KIR (which is a mutable intermediate graph), the CKM is a verified,
deduplicated, cross-referenced model ready for permanent storage. This pass is the final compilation
step before the knowledge is committed to the ledger.

**Inputs:** Phase 7 `ResolvedGraph` (identity-resolved KIR); all `KnowledgeArtifact`s.

**Outputs:** CKM as a JSON document; `semantic` crate; CKM schema.

**Validation:**
```bash
cargo test -p semantic
ekos compile   # new CLI command: observation → recovery → identity → semantic → CKM output
cat .ekos/ckm/model.json | jq '.objects | length'   # > 0
```

---

- [x] **`semantic`: `SemanticCompiler` pass**
  - *What:* Implement `SemanticCompiler: CompilerPass`. Input: `ResolvedGraph` from Phase 7. Runs
    three sub-passes: (1) relationship normalisation, (2) cross-source evidence aggregation, (3) CKM
    schema validation. Emits a `KnowledgeArtifact` containing the CKM JSON.
  - *Output:* `SemanticCompiler` struct in `crates/semantic/src/lib.rs`.
  - *Test/Validate:* Unit test: pass a small `ResolvedGraph` with two objects and one relationship;
    assert `SemanticCompiler::run()` returns `Ok(())`; assert CKM artifact exists in store.

- [x] **Transform KIR → CKM (JSON, no binary)**
  - *What:* Define `CkModel { version: u32, compiled_at: DateTime<Utc>, objects: Vec<CkmObject>,
    relationships: Vec<CkmRelationship>, evidence_index: HashMap<KirId, EvidenceRecord> }`.
    `CkmObject` is a flattened, denormalized view of a canonical `KirObject` — no forward references,
    all related evidence embedded. Write to `.ekos/ckm/model.json`.
  - *Output:* `CkModel` type; `model.json` file on disk.
  - *Test/Validate:* `cat .ekos/ckm/model.json | python3 -m json.tool` exits 0 (valid JSON).
    Assert schema version field is `1`.

- [x] **Relationship normalisation and deduplication**
  - *What:* Within the `SemanticCompiler`, after identity resolution, the same relationship may be
    observed multiple times (FK in DB + reference in documentation). Deduplicate by `(from, to, kind)`
    tuple; merge evidence lists. Relationships pointing to non-existent objects are flagged as
    `Warning` diagnostics and dropped from the CKM.
  - *Output:* `normalize_relationships(graph: &ResolvedGraph) -> Vec<CkmRelationship>` function.
  - *Test/Validate:* Unit test: graph with three identical `ForeignKey` relationships; assert
    output contains exactly one with all three evidence entries merged.

- [x] **Cross-source evidence aggregation**
  - *What:* For each `CkmObject`, gather evidence from all source artifacts (DB connector, Git
    connector, Confluence analyzer) and embed as `evidence: Vec<EvidenceRecord>` sorted by confidence
    descending. Highest-confidence evidence fragment is used as the object's `primary_description`.
  - *Output:* `aggregate_evidence(object: &KirObject, artifacts: &ArtifactStore) -> Vec<EvidenceRecord>`
    function; each `CkmObject` has non-empty `evidence`.
  - *Test/Validate:* Unit test: object with evidence from two sources; assert aggregated evidence has
    both entries; assert highest-confidence evidence is `primary_description`.

- [x] **CKM schema definition and validation**
  - *What:* Write `docs/ckm-schema.json` as a JSON Schema document describing the CKM format.
    Implement `fn validate_ckm(model: &CkModel) -> Result<(), Vec<SchemaError>>` that checks:
    all relationship `from`/`to` ids exist as objects, all evidence `source_artifact` ids exist in
    store, no duplicate object ids.
  - *Output:* `docs/ckm-schema.json`; `validate_ckm` function; validation runs at end of `SemanticCompiler::run()`.
  - *Test/Validate:* Unit test: valid CKM passes validation. CKM with a dangling relationship
    (references non-existent object id) returns `Err` with the offending relationship id.

- [x] **`cli`: `ekos compile` command**
  - *What:* Subcommand that runs the full pipeline in order — observation (if snapshot is stale) →
    recovery → identity resolution → semantic compilation — and writes the CKM to
    `.ekos/ckm/model.json`. Prints a stage-by-stage summary. This is the one-command path from raw
    enterprise sources to a validated CKM.
  - *Output:* `ekos compile` subcommand (referenced by this phase's Validation section).
  - *Test/Validate:* `ekos compile` on the ecommerce fixture exits 0 and `.ekos/ckm/model.json`
    passes `validate_ckm`. Running it a second time with no source changes reuses cached stages.

---

## Phase 9 — Knowledge Ledger

**Goal:** Build the permanent, append-only store that is the single source of semantic truth.

**Context:** The ledger is where compiled knowledge lives permanently. Unlike the `.ekos/artifacts/`
cache (which is ephemeral and can be deleted with `ekos clean`), the ledger is never cleared. Every
write is timestamped and sourced. The ledger enables time-travel queries, full audit trails, and
reproducibility — given the same source systems at the same point in time, the same ledger state
must result. NOTE: the tasks below assume snapshot storage on SQLite — both are RFC 0004 decisions.
If the RFC picks event-sourcing or a different backend, adjust these tasks *before* starting the
phase, not during it.

**Inputs:** Phase 8 CKM output; RFC 0004 (ledger design).

**Outputs:** `ledger` crate; append-only store populated by `ekos commit`; `ekos ledger status` CLI.

**Validation:**
```bash
cargo test -p ledger
ekos commit    # new CLI: writes CKM to ledger
ekos ledger status   # prints entry count, last write time
# Attempt to overwrite a ledger entry directly (by editing the file); assert `ekos ledger verify` detects tampering
```

---

- [x] **`ledger`: append-only storage engine (behind `LedgerBackend` trait)**
  - *What:* Implement `Ledger` behind a `LedgerBackend` trait, with SQLite
    (`.ekos/ledger/ledger.db`) as the first — explicitly disposable — backend, per RFC 0004.
    One table: `entries(id TEXT PRIMARY KEY, type TEXT, payload BLOB, written_at INTEGER,
    source_artifact_id TEXT, checksum TEXT)`. Implement `fn append(&self, entry: LedgerEntry) ->
    Result<LedgerEntryId>` that inserts but never updates or deletes. `fn verify_integrity(&self) ->
    Result<(), Vec<IntegrityError>>` checks all checksums. No code outside the `ledger` crate may
    reference SQLite directly — everything goes through the trait, so the v1.0 backend swap touches
    one crate.
  - *Output:* `Ledger` struct in `crates/ledger/src/lib.rs`; SQLite schema.
  - *Test/Validate:* Unit test: `append` 3 entries; `verify_integrity()` returns `Ok(())`. Manually
    corrupt a checksum; assert `verify_integrity()` returns `Err` with the corrupted entry id.

- [x] **Store: Objects, Relationships, Events, Evidence**
  - *What:* Implement `LedgerWriter` with four typed methods: `write_object(obj: &CkmObject)`,
    `write_relationship(rel: &CkmRelationship)`, `write_event(evt: &KirEvent)`,
    `write_evidence(ev: &EvidenceRecord)`. Each serializes to JSON and calls `Ledger::append`. The
    ledger entry `type` field discriminates the four kinds.
  - *Output:* `LedgerWriter` in `crates/ledger/src/writer.rs`.
  - *Test/Validate:* Integration test: write one of each type; query the SQLite DB directly; assert
    four rows exist with correct `type` values.

- [x] **Current-state index**
  - *What:* Maintain a `current_state` table: `(object_id TEXT PRIMARY KEY, latest_entry_id TEXT)`.
    Updated atomically within the same SQLite transaction as the `entries` insert. Enables
    `LedgerReader::current_object(id) -> Option<CkmObject>` without scanning the full entry log.
  - *Output:* `current_state` table; `LedgerReader::current_object()` method.
  - *Test/Validate:* Write an object, then write an updated version with the same id; assert
    `current_object(id)` returns the second version; assert `entries` table has two rows for that id.

- [x] **Historical state index**
  - *What:* All `entries` rows are already the history. Implement `LedgerReader::object_history(id)
    -> Vec<(DateTime<Utc>, CkmObject)>` that returns all versions ordered by `written_at` ascending.
    For time-travel: `object_at(id, timestamp) -> Option<CkmObject>` returns the latest version
    with `written_at <= timestamp`.
  - *Output:* `LedgerReader::object_history()` and `object_at()` methods.
  - *Test/Validate:* Write object at t1, update at t2. Assert `object_at(id, t1)` returns v1,
    `object_at(id, t2)` returns v2, `object_at(id, t0)` returns `None`.

- [ ] **Full audit trail (every write timestamped and sourced)** — **genuine, confirmed gap (2026-08-26
  audit, RFC 0004 written to close the missing-doc half of this finding)**: `written_at` is real
  (every `LedgerEntry` has it), but `source_artifact_id`/artifact-level provenance was never built —
  grepped `crates/ledger/src/*.rs` directly, no such field or method exists anywhere. What shipped
  instead, covering a related but distinct need: `KirEvidence` (RFC 0003) — every semantic
  conclusion cites a real `SourceLocation`/fragment, answering "why do we believe this" at the
  evidence level. Neither backend has ever needed "which pipeline run produced this write" for a
  real feature to date, which is likely why this was never revisited — but it's a real, scoped,
  buildable gap if a future need surfaces (`LedgerEntry` needs a new field, both backends need a
  migration path, every append call site needs a real `ArtifactId` threaded through — see RFC 0004
  for the full writeup).
  - *What:* Every `LedgerEntry` must include `source_artifact_id` (the `ArtifactId` of the
    `KnowledgeArtifact` that produced this knowledge). `written_at` uses wall-clock UTC time.
    Implement `LedgerReader::audit_trail(id) -> Vec<AuditRecord>` returning the full write history
    with artifact provenance.
  - *Output:* `AuditRecord { entry_id, written_at, source_artifact_id, type }`;
    `LedgerReader::audit_trail()`.
  - *Test/Validate:* Write an object twice from two different `KnowledgeArtifact` ids; assert
    `audit_trail()` returns two records with different `source_artifact_id` values.

- [x] **`cli`: `ekos commit` command (idempotent)**
  - *What:* Subcommand that reads the CKM at `.ekos/ckm/model.json` and writes objects,
    relationships, and evidence to the ledger via `LedgerWriter`. Must be idempotent: entry ids
    derive from content hashes and entries already present are skipped, so running `ekos commit`
    twice never duplicates knowledge in the append-only log.
  - *Output:* `ekos commit` subcommand (referenced by this phase's Validation section).
  - *Test/Validate:* Run `ekos commit` twice on the same CKM; assert the ledger entry count is
    identical after the second run and the summary prints "0 new entries".

- [x] **`cli`: `ekos ledger status`**
  - *What:* Subcommand that prints: total entry count, count per entry type, last write timestamp,
    integrity check result (`OK` / `TAMPERED`), and ledger file size.
  - *Output:* Human-readable status report on stdout; exits 0 if integrity check passes.
  - *Test/Validate:* After `ekos commit`, `ekos ledger status` prints non-zero counts for objects
    and relationships. `ekos ledger status` on an empty ledger prints zeros and exits 0.

---

## Phase 10 — Runtime

**Goal:** Build the read-only layer that reconstructs enterprise state from the ledger for query and display.

**Context:** The Runtime is the consumer-facing API of EKOS. AI agents, CLI users, and Knowledge
Services all go through the Runtime — never directly to the ledger. The Runtime's job is reconstruction,
not storage. It must be completely stateless with respect to writes: the Runtime has no `&mut self`
methods that affect the ledger.

**Inputs:** Phase 9 `Ledger`; RFC 0005 (Runtime design).

**Outputs:** `runtime` crate with `load_object`, `load_neighborhood`, `reconstruct_state`,
`reconstruct_state_at`; `ekos query` CLI.

**Validation:**
```bash
cargo test -p runtime
ekos query object <id>             # prints object as JSON
ekos query neighborhood <id> --depth 2   # prints graph
ekos query object <id> --at 2025-01-01  # historical reconstruction
```

---

- [x] **`runtime`: `load_object(id)`**
  - *What:* `fn load_object(&self, id: &KirId) -> Result<Option<CkmObject>, RuntimeError>` calls
    `LedgerReader::current_object(id)`. Returns `None` if the object has never been written to the
    ledger. Runtime is a thin read-only wrapper — no caching yet (Phase 13).
  - *Output:* `Runtime::load_object()` in `crates/runtime/src/lib.rs`.
  - *Test/Validate:* Integration test against a populated test ledger: `load_object(known_id)` returns
    `Some(obj)` with correct name. `load_object(unknown_id)` returns `None`.

- [x] **`runtime`: `load_neighborhood(id, depth)`**
  - *What:* `fn load_neighborhood(&self, id: &KirId, depth: u32) -> Result<KirGraph, RuntimeError>`
    performs a BFS from `id` up to `depth` hops, loading each related object via its relationships.
    Returns a `KirGraph` subgraph. Cycles are handled by tracking visited ids.
  - *Output:* `Runtime::load_neighborhood()`.
  - *Test/Validate:* Integration test: ledger with objects A→B→C (relationship chain). `load_neighborhood(A, 1)` returns A and B only. `load_neighborhood(A, 2)` returns A, B, and C.

- [x] **`runtime`: `reconstruct_state(id)` — current state**
  - *What:* `fn reconstruct_state(&self, id: &KirId) -> Result<ObjectState, RuntimeError>` builds
    an `ObjectState { object: CkmObject, relationships: Vec<CkmRelationship>, evidence: Vec<EvidenceRecord> }`
    by loading the object, all its relationships, and all associated evidence in one coherent view.
  - *Output:* `ObjectState` type; `Runtime::reconstruct_state()`.
  - *Test/Validate:* Integration test: ledger with an object, two relationships, and three evidence
    records. `reconstruct_state(id)` returns all five elements correctly linked.

- [x] **`runtime`: `reconstruct_state_at(id, timestamp)` — historical state**
  - *What:* `fn reconstruct_state_at(&self, id: &KirId, at: DateTime<Utc>) -> Result<Option<ObjectState>, RuntimeError>`
    calls `LedgerReader::object_at(id, at)` and reconstructs relationships and evidence that existed
    at that timestamp (using `written_at` filter on relationship entries).
  - *Output:* `Runtime::reconstruct_state_at()`.
  - *Test/Validate:* Integration test: write object at t1 with one relationship; update at t2 adding
    a second relationship. `reconstruct_state_at(id, t1)` returns one relationship. `at t2` returns two.

- [x] **`runtime`: object name index / text lookup**
  - *What:* Phase 11's ask-pipeline must map question keywords to object ids, but the ledger is only
    addressable by id — without this index, `ekos ask` has no retrieval path. Build a full-text
    index (SQLite FTS5 table over object names, kinds, and property keys, maintained at commit time)
    and expose `Runtime::find_objects(query: &str) -> Vec<(KirId, f32)>` returning ranked matches.
  - *Output:* FTS index in the ledger DB; `Runtime::find_objects()`; `ekos query find "<text>"` subcommand.
  - *Test/Validate:* After committing the ecommerce fixture, `find_objects("order")` returns the
    `Order` object as the top-ranked match; `find_objects("zzz-nonexistent")` returns an empty list.

- [x] **`cli`: `ekos query`**
  - *What:* Subcommand with sub-sub-commands: `ekos query object <id>` (prints `ObjectState` as
    JSON), `ekos query neighborhood <id> [--depth N]` (prints subgraph as JSON), `ekos query object
    <id> --at <ISO8601>` (historical). Add `--format json|table` flag for human vs. machine output.
  - *Output:* `ekos query` command with three modes.
  - *Test/Validate:* After `ekos commit`, `ekos query object <id> --format json | jq '.object.name'`
    prints the object name. `ekos query object <unknown-id>` prints `"Not found"` and exits 1.

---

## Phase 11 — AI Runtime

**Goal:** Enable LLM-powered natural language questions answered by grounded, evidenced knowledge.

**Context:** This is the final assembly of all previous phases. The AI Runtime sits on top of the
Runtime (Phase 10) and the LLM layer (Phase 6). When a user asks a question, the AI Runtime retrieves
relevant context from the ledger via the Runtime, constructs a grounded prompt, and returns an answer
that cites its evidence. The LLM never sees raw enterprise systems — only reconstructed, verified knowledge.

**Inputs:** Phase 10 Runtime; Phase 6 LLM integration layer; a populated ledger.

**Outputs:** `ekos ask` CLI command; AI Runtime that cites evidence in every answer.

**Validation:**
```bash
ANTHROPIC_API_KEY=... ekos ask "What tables does the orders system depend on?"
# Response must: answer the question AND cite at least one KirEvidence with source artifact id
```

---

- [x] **AI layer: question → Runtime context → LLM → answer**
  - *What:* Implement `AiRuntime { runtime: Runtime, llm: Box<dyn LlmProvider> }` with `async fn
    ask(&self, question: &str) -> Result<AiAnswer, AiError>`. Pipeline: (1) match question keywords
    to objects via `Runtime::find_objects` (the Phase 10 name index), (2)
    `Runtime::load_neighborhood` for the top-ranked matches, (3) build a grounded prompt
    including the `ObjectState` JSON and ask the LLM to answer using only that context, (4) parse
    LLM response into `AiAnswer { answer: String, evidence_refs: Vec<KirId> }`.
  - *Output:* `AiRuntime` struct; `AiAnswer` type in `crates/runtime/src/ai.rs`.
  - *Test/Validate:* Integration test with mock LLM: assert the prompt sent to LLM contains object
    context JSON. Assert `AiAnswer.evidence_refs` is non-empty.

- [x] **Provider-agnostic LLM interface (Claude first)**
  - *What:* Reuse the `LlmProvider` trait from Phase 6. Wire `AiRuntime` to accept any `Box<dyn
    LlmProvider>`. The `ClaudeProvider` from Phase 6 is the default. Model: `claude-sonnet-4-6`.
    Prompt template: stored in `ekos.toml` `[ai]` section, overridable without code changes.
  - *Output:* `AiRuntime::new(runtime, llm_provider)` constructor; configurable prompt template.
  - *Test/Validate:* Swap `ClaudeProvider` for a `MockLlmProvider` in tests; assert `AiRuntime`
    behaves identically, proving provider independence.

- [x] **Provenance: every answer cites its evidence**
  - *What:* The LLM prompt explicitly instructs the model to end its response with a JSON block:
    `{"cited_evidence": ["<KirId>", ...]}`. `AiRuntime` parses this block, validates each id exists
    in the ledger, and includes them in `AiAnswer.evidence_refs`. If the LLM omits the block, emit
    a `Warning` diagnostic and return the answer with empty refs.
  - *Output:* Parsed `evidence_refs` in every `AiAnswer`; validation that cited ids are real.
  - *Test/Validate:* Integration test: mock LLM returns a response with a valid citation block;
    assert `evidence_refs` contains the cited id. Mock returns response without citation block;
    assert a `Warning` diagnostic is emitted.

- [x] **`cli`: `ekos ask "<question>"`**
  - *What:* Subcommand: `ekos ask "What is the relationship between orders and customers?"`. Calls
    `AiRuntime::ask()`, prints the answer, then prints a `Sources:` section listing each cited
    evidence with its source artifact and location. `--json` flag returns the full `AiAnswer` JSON.
  - *Output:* `ekos ask` subcommand.
  - *Test/Validate:* With a populated ledger and live API key: `ekos ask "list all tables"` returns
    a non-empty answer with at least one cited source. `ekos ask` with no API key configured prints
    a clear error: `"No LLM provider configured. Set ANTHROPIC_API_KEY and provider = 'claude' in ekos.toml."`.

---

## Phase 12 — Enterprise Knowledge Language (EKL)

**Goal:** Define and implement a domain-specific query language for the EKOS knowledge graph.

**Context:** While `ekos ask` answers natural language questions, power users and integrations need
a precise, composable query language with deterministic semantics. EKL is to EKOS what SQL is to
relational databases — it lets users express exactly what they want from the knowledge graph.

**Inputs:** Phase 10 Runtime API (EKL compiles to Runtime calls); Phase 8 CKM schema.

**Outputs:** EKL RFC; parser; interpreter/query planner; `ekos ekl` CLI.

**Validation:**
```bash
ekos ekl "FIND Object WHERE kind = 'Table' RETURN name, evidence"
ekos ekl "FIND Relationship WHERE kind = 'ForeignKey' FROM 'orders'"
```

---

- [x] **RFC: EKL syntax and semantics**
  - *What:* Write `docs/rfcs/0009-ekl.md` (0008 is taken by the LLM-policy RFC) defining EKL
    grammar (EBNF), statement types (`FIND`,
    `WHERE`, `RETURN`, `LIMIT`, `ORDER BY`), supported predicates (equality, range, contains),
    path expressions (e.g., `orders -> customer_id -> customers`), and the formal semantics mapping
    each construct to Runtime API calls.
  - *Output:* `docs/rfcs/0009-ekl.md` with status `Accepted`.
  - *Test/Validate:* RFC includes 10 example queries with expected outputs for the ecommerce fixture.

- [x] **Parser**
  - *What:* Implement `ekl_parse(input: &str) -> Result<EklAst, ParseError>` using `pest` or
    `nom`. The `EklAst` is a typed AST covering all constructs defined in the RFC. Produce helpful
    parse errors (line, column, expected token).
  - *Output:* `crates/ekl/src/parser.rs`; `EklAst` enum.
  - *Test/Validate:* Unit tests for all grammar constructs defined in the RFC. Fuzzing test: random
    strings must not cause panics (only `ParseError`).

- [x] **Interpreter / query planner against the Runtime**
  - *What:* Implement `EklInterpreter { runtime: Runtime }` with `fn execute(&self, ast: &EklAst)
    -> Result<EklResult, EklError>`. `EklResult` is a table of rows (each row is a `HashMap<String,
    serde_json::Value>`). The interpreter translates AST nodes to Runtime calls (`load_object`,
    `load_neighborhood`) and filters/projects results.
  - *Output:* `EklInterpreter` in `crates/ekl/src/interpreter.rs`; `EklResult` type.
  - *Test/Validate:* Integration test: run `FIND Object WHERE kind = 'Table'` against the ecommerce
    fixture ledger; assert result rows contain `orders`, `customers`, `products`.

---

## Phase 13 — Optimizer

**Goal:** Make the compiler fast enough for large enterprises through incremental compilation, parallelism,
and caching.

**Context:** A compiler that re-processes the entire enterprise from scratch every run is unusable at
scale. The Optimizer adds the same capabilities that `make`, Bazel, and Cargo have for code — only
recompile what changed. This phase does not change what is produced, only how quickly.

**Inputs:** All prior phases (the full compiler pipeline); Phase 2 content-addressable artifact store.

**Outputs:** Incremental builds; parallel pass execution; knowledge diff tool.

**Validation:**
```bash
ekos build && ekos build   # second run should be significantly faster (cache hits)
ekos diff <ledger-state-1> <ledger-state-2>   # prints what changed
time ekos build   # benchmark before/after parallelism
```

---

- [x] **Compact storage (RFC 0015)** — dictionary-zstd ledger v2 (`ekos ledger migrate`, 99→39 MB
  on the live estate), EKOS Pack v1 packed artifact segments (`ekos artifact repack`, 214→31 MB
  on disk), compressed snapshots/CKM with retention, `ekos ledger status --storage` instrument,
  `storage_compaction` bench. Devlog 17.
- [~] **Fact-segment engine (RFC 0016, accepted 2026-07-17)** — Phases 1–6 implemented (fact
  model, segments+watermark, EAVT/AEVT/AVET runs, API parity, tantivy, mmap) plus
  `ekos ledger migrate --v3` and the `KnowledgeStore` backend seam with auto-detection.
  Acceptance gate (devlog 18): functional criteria PASS on the real estate. §7 compression
  fully implemented (dict-zstd batches, prefix-delta binary blocks, slim projections,
  ref-only AVET): 98 → 65 MB — the ≥2× size gate is structurally unreachable (truth +
  indexes + tantivy floor ≈ 50 MB vs v2's 39 MB). Gate AMENDED with measurements
  on the table (recorded in the RFC): ≤2× of v2 at equal-or-better latency → PASSES (1.66×).
  LIVE ESTATE PROMOTED to the fact engine (88,637 versions verified; rollback = delete
  facts/). Post-promotion perf fix: counts via AEVT scan, bulk listings via one EAVT pass
  (status 19 s → 71 ms; 4-call MCP session 88 ms). Fresh workspaces still default to SQLite
  until soak completes; pointer-EAVT remains a documented option.

- [x] **Incremental compilation (re-scan changed sources only)**
  - *What:* Before running an `Observer`, compare the current source fingerprint (Git HEAD sha for
    git, mtimes for filesystem, schema version hash for DB) against the fingerprint stored in the
    previous `ObservationPackage`. If unchanged, skip the observation and reuse the cached artifact.
    Implement `fn source_fingerprint(ctx: &ScanContext) -> Fingerprint` in `observation-sdk`.
  - *Output:* Cache-hit path in `ekos build`; "N connectors skipped (cached)" in build summary.
  - *Test/Validate:* Run `ekos build` twice without changing sources; assert second run takes <10%
    of first run time and prints "0 connectors re-scanned".

- [x] **Parallel pass execution**
  - *What:* Update `Scheduler` to detect passes with no data dependency between them and execute
    them concurrently using `tokio::task::spawn`. Passes that share an output artifact (write to the
    same `ArtifactId`) must not run concurrently — the scheduler enforces this via the dependency DAG.
  - *Output:* `Scheduler` with parallel execution mode; `--parallel` flag on `ekos build`.
  - *Test/Validate:* Run `ekos build --parallel` with three independent passes; instrument each pass
    to record start time; assert all three start times are within 100ms of each other.

- [x] **Artifact cache invalidation strategy**
  - *What:* Define when a cached artifact is invalidated: (1) any transitive input artifact has
    changed (content-hash differs), (2) the pass that produced it has a different version, (3) the
    pass configuration has changed. Implement `fn should_recompute(pass: &dyn CompilerPass, inputs:
    &[ArtifactId], store: &ArtifactStore) -> bool`.
  - *Output:* `should_recompute` function used by `PassManager`; "N passes skipped (cached)" in build summary.
  - *Test/Validate:* Change a pass's config in `ekos.toml`; assert the pass re-runs on next build
    even though its input artifacts have not changed.

- [x] **Knowledge diff (what changed between two ledger states)**
  - *What:* Implement `fn diff_ledger(ledger: &Ledger, from: DateTime<Utc>, to: DateTime<Utc>) ->
    LedgerDiff` where `LedgerDiff { added: Vec<LedgerEntryId>, unchanged: usize }`. (No deletion
    from append-only ledger, so "changed" means a new entry superseded an older one for the same
    object.) Add `ekos diff --from <timestamp> --to <timestamp>` CLI.
  - *Output:* `LedgerDiff` type; `ekos diff` subcommand.
  - *Test/Validate:* Write 3 objects at t1, update 1 at t2. `diff_ledger(t1, t2)` returns
    `added.len() == 1` (the updated entry) and `unchanged == 2`.

- [x] **Knowledge merge and branch**
  - *What:* Allow the ledger to have named "branches" (separate SQLite files at `.ekos/ledger/<branch>.db`).
    `ekos branch create <name>` copies the current ledger. `ekos branch merge <name>` appends entries
    from the branch that are not in the main ledger (by entry id). Conflicts (same object updated in
    both branches after divergence) are flagged as diagnostics.
  - *Output:* `ekos branch` subcommand with `create`, `list`, `merge`, `delete`.
  - *Test/Validate:* Create a branch; write one object to each (main and branch); merge; assert main
    contains both objects. Write the same object to both with different values; assert merge produces
    a conflict diagnostic.

---

## Phase 14 — Enterprise Scale Connectors

**Goal:** Extend EKOS to the major enterprise platforms used in large organisations.

**Context:** Phases 0–13 built and proved the compiler with Git, Postgres, SQL Server, and file
system sources. Phase 14 extends the connector set to the platforms that dominate enterprise IT.
Each connector follows the `Observer` trait contract from Phase 3 and is developed, tested, and
shipped independently.

**Inputs:** Phase 3 Observation SDK; vendor API credentials for integration tests.

**Outputs:** One connector plugin per platform; integration test docker-compose or credential fixtures.

**Validation:** For each connector: `cargo test -p plugin-<name> --features integration` passes
with real or vendor-supplied sandbox credentials.

---

- [~] **SAP connector**
  - *What:* Implement `plugins/sap/` using SAP OData APIs or RFC (Remote Function Call) via the
    `nwrfc` binding. Observe: business objects (BAPIs), table definitions, organizational hierarchy.
    Emit `ObservationArtifact`s per object type.
  - *Output:* `plugins/sap/` crate; integration test with SAP sandbox.
  - *Test/Validate:* Integration test: connect to SAP sandbox; assert at least one BAPI artifact
    and one organizational unit artifact are returned.
  - *Status (RFC 0012):* Scaffolded — `SapClient` trait, OData-based `SapODataClient` (untested
    against a live sandbox), `MockSapClient`, `SapObserver`, unit tests. RFC/`nwrfc` intentionally
    not implemented (proprietary native SDK dependency). Live integration test still outstanding —
    needs a real SAP sandbox credential.

- [~] **Salesforce connector**
  - *What:* Implement `plugins/salesforce/` using the Salesforce REST API. Observe: sObjects schema
    (fields, types, relationships), workflow rules, custom objects. Emit one `ObservationArtifact`
    per sObject with its full field metadata.
  - *Output:* `plugins/salesforce/` crate; integration test with Salesforce developer org.
  - *Test/Validate:* Integration test: observe `Account` and `Contact` sObjects; assert field count
    matches Salesforce developer org schema; assert relationship between Account and Contact is captured.
  - *Status (RFC 0012):* Scaffolded — `SalesforceClient` trait, `SalesforceApiClient` (untested
    against a live org), `MockSalesforceClient`, `SalesforceObserver` (captures reference fields as
    the relationship signal), unit tests including an Account/Contact-shaped mock case. Live
    integration test still outstanding — needs a real developer-org credential.

- [~] **Oracle connector**
  - *What:* Implement `plugins/oracle/` using `oracle` crate (ODPI-C bindings). Same surface as
    Postgres connector: tables, constraints, views, stored procedures. Handle Oracle-specific types
    (VARCHAR2, NUMBER, CLOB).
  - *Output:* `plugins/oracle/` crate; integration test with Oracle XE container.
  - *Test/Validate:* Integration test: load fixture schema into Oracle XE; assert same artifact
    types and counts as equivalent Postgres fixture.
  - *Status (RFC 0012):* Scaffolded — `OracleClient` trait, `MockOracleClient`, `OracleObserver`,
    unit tests. `OracleDbClient` (the real driver) is a documented stub returning `NotImplemented`
    — the `oracle`/ODPI-C crate needs native Oracle Instant Client libraries not installable here;
    wiring a real driver and an Oracle XE integration test are still outstanding.

- [~] **Microsoft Fabric / Snowflake connector**
  - *What:* Implement `plugins/fabric/` using Azure Fabric REST API (workspaces, lakehouses,
    datasets) and `plugins/snowflake/` using the Snowflake JDBC/ODBC REST API. Observe: schemas,
    tables, views, warehouse metadata.
  - *Output:* Two crates; integration tests with Fabric trial and Snowflake trial accounts.
  - *Test/Validate:* Integration test per platform: observe a test warehouse schema; assert table
    and view artifacts are returned.
  - *Status (RFC 0012):* Scaffolded — `FabricClient`/`SnowflakeClient` traits, REST-based
    `FabricApiClient`/`SnowflakeApiClient` (untested against live trial accounts), mock clients,
    observers, unit tests. Live integration tests still outstanding — need real trial-account
    credentials for both platforms.

- [ ] **Kubernetes connector**
  - *What:* Implement `plugins/kubernetes/` using the `kube` crate. Observe: Deployments, Services,
    ConfigMaps, Secrets (names only, no values), Ingresses, CRDs. Emit one artifact per resource kind
    plus one per namespace. Map service-to-deployment relationships.
  - *Output:* `plugins/kubernetes/` crate; integration test against a local `kind` cluster.
  - *Test/Validate:* Integration test: deploy a simple two-service app to `kind`; assert both
    `ServiceArtifact`s are present and the service-to-deployment relationship is captured.

- [ ] **Additional connectors on demand**
  - *What:* Placeholder for connectors requested after Phase 14 ships (Jira, Confluence full
    connector, ServiceNow, etc.). Each follows the same pattern: RFC → SDK impl →
    integration test → docs. dbt's own project metadata (not a live-connector shape — see RFC
    0117 below) is already covered.
  - *Output:* Tracked as individual issues/tickets; this item is the backlog bucket.
  - *Test/Validate:* Each connector added here must ship with a passing integration test before merge.

- [~] **Code knowledge expansion — SQL/Pentaho depth, Python/PySpark, notebooks, Databricks,
      Azure Data Factory, metadata-driven pipelines — RFC 0038**
  - *What:* Six-phase roadmap, sequenced by dependency: (1) [x] close existing SQL/Pentaho gaps
    (stored-procedure control flow, snowflake/databricks SQL dialect plugins — RFC 0039, done),
    (2) [x] a real AST-based Python/PySpark analyzer lowering DataFrame chains into the existing
    Transformation IR (RFC 0040, done — verified against a real Databricks Asset Bundle repo: 83
    files, 57 real nodes recovered), (3) Jupyter notebooks (reuses Phase 2 per code cell), (4) a
    Databricks connector (real Jobs API job/task DAGs), (5) an Azure Data Factory connector (also
    where the parameter/variable IR concept for metadata-driven pipelines finally gets designed,
    against ADF's idiomatic `Lookup`+`ForEach` pattern), (6) generalizing that parameterization
    vocabulary back onto Pentaho/PySpark. See RFC 0038 for full detail; each remaining phase gets
    its own just-in-time RFC before it starts.
  - *Output:* `ekos/docs/rfcs/0038-code-knowledge-expansion-roadmap.md`; six follow-up RFCs as each
    phase starts.
  - *Test/Validate:* Per-phase, detailed in each phase's own RFC.

- [x] **Rust source analyzer — real symbols/imports + real Calls edges — RFC 0041**
  - *What:* A direct user request run alongside the RFC 0038 roadmap (not one of its six numbered
    phases): `plugins/rust` (`RustObserver`) + `crates/recovery/src/rust_analyzer.rs`
    (`RustAnalyzerPass`, real AST parsing via `syn`) recognize `use` imports, fn/struct/enum/
    trait/impl-method definitions, and — the headline capability — real intra-file `Calls` edges,
    the first analyzer in the project to populate `RelationshipKind::Calls` (superseding RFC
    0038 Phase 4's original claim to that title). Real-data testing against this repo's own
    ~50-crate workspace (118 files, 1270 symbols, 715 `Calls` edges) found and fixed a real,
    pre-existing bug in the shared `DefaultResolver` identity-resolution code (silently merging
    distinct same-suffix-named symbols across every analyzer, not just this one) — see devlog_41.
  - *Output:* `ekos/docs/rfcs/0041-rust-source-analyzer.md`; `ekos/plugins/rust/`;
    `ekos/crates/recovery/src/rust_analyzer.rs`; bug fix in `ekos/crates/identity/src/lib.rs`.
  - *Test/Validate:* 9 unit tests (`rust_analyzer.rs`) + 3 unit tests (`plugins/rust`) + 1
    regression test (`crates/identity/src/lib.rs`); real pipeline run against `ekos/` itself,
    spot-checked a real `Calls` edge (`PentahoAnalyzerPass::run` → `parse_kettle_xml`) against
    the actual source.

- [x] **Local document connector (PDF/DOCX, text + tables + image OCR) — RFC 0023**
  - *What:* `ekos-plugin-localdocs` observes `.pdf`/`.docx` files under the workspace, extracting
    prose text, tables (heuristic for PDF, structural for DOCX), and OCR'd embedded-image text
    (via a `tesseract` CLI subprocess, no `unsafe`). `LocalDocAnalyzerPass` maps each into a
    `Document` KIR object plus `Table` child objects with `Contains` edges.
  - *Output:* `plugins/localdocs/` crate; `crates/recovery/src/local_docs_analyzer.rs`; wired into
    `build.rs`/`recover.rs` unconditionally (no credential to gate on).
  - *Test/Validate:* Fixture-driven unit tests (20 in the plugin crate, 5 in the analyzer pass, 3
    built from real book content); end-to-end verified against a real generated PDF (table + JPEG
    scan) and DOCX (table + PNG image); further validated against a real 82-PDF, 955MB library
    (devlog 25) — found and fixed a parser panic on malformed real-world PDFs, a single-space
    text-mangling bug, and a table-heuristic false-positive on justified prose; final run produced
    45 `Document` + 30 `Table` objects, 18 with real OCR'd cover text, committed to the ledger.
    Hardened against document-borne prompt injection (devlog 26): strips zero-width Unicode and
    the Unicode tag block from excerpt/table-cell/OCR text before capture, reporting a nonzero
    removal count on the artifact and in logs.

- [x] **Document section indexing — RFC 0024**
  - *What:* Fixed a real, demonstrated bug — `ekos_search` couldn't find content deep inside long
    PDF/DOCX documents (only a 600-char whole-document excerpt was ever indexed). Decomposes each
    document into `Custom("Section")` objects — one per PDF page (real per-page extraction via
    `pdf-extract`) or DOCX character-budget chunk — each independently indexed. Bundled fix:
    `KirObject::indexed_content()` now also includes `ocr_text` (previously never searchable).
  - *Output:* `plugins/localdocs/src/{lib,pdf,docx}.rs`, `crates/recovery/src/local_docs_analyzer.rs`,
    `crates/kir/src/lib.rs`.
  - *Test/Validate:* Unit tests across all four touched crates, including a genuine PDF round-trip
    (built with `lopdf`'s own writer, parsed by the real `PdfParser`). End-to-end verified against
    the real 82-book library (devlog 27) — found and fixed a *second* bug along the way:
    `ekos-identity`'s `DefaultResolver` was merging nearly every page of a book into one canonical
    object (8,624 raw objects → 120 after resolution), fixed by excluding `Custom("Section")` from
    resolution blocking. After both fixes: `ekos_search(query: "replication")` returns 30 real
    matches, including `Cloud Design Patterns.pdf`'s actual "Data Replication and Synchronization
    Guidance" section (pages 211–216) — previously unreachable.

- [x] **Additional document formats: text/Markdown, HTML, email — RFC 0025**
  - *What:* Extends `ekos-plugin-localdocs` beyond PDF/DOCX to `.txt`/`.md` (`TextParser`),
    `.html`/`.htm` (`HtmlParser`, via `html2text`), and `.eml` (`EmailParser`, via `mail-parser`,
    header block + text/plain-preferred body, falling back to HTML-to-text). `DocumentParser`
    gained a default `supported_extensions()` method so one parser struct can serve two
    extensions. `.msg` and email attachments explicitly out of scope.
  - *Output:* `plugins/localdocs/src/{text,html,email}.rs`; `LocalDocsObserver::with_defaults`
    wiring; fixtures under `plugins/localdocs/tests/fixtures/`.
  - *Test/Validate:* Per-parser unit tests plus a direct regression proving zero downstream
    changes: an artifact with a new `doc_format` produces the same `Document`/`Section` KIR
    shape `LocalDocAnalyzerPass` already produces for PDF, and content past the 600-char
    whole-document excerpt cap is searchable via `indexed_content()` for the new formats too.

- [x] **LLM document-semantics extraction pass — RFC 0026**
  - *What:* Closes the gap RFC 0023 explicitly deferred ("an LLM pass — a different, larger
    mechanism"): `DocumentSemanticsAnalyzerPass` reads `Custom("Section")` objects from
    `LocalDocAnalyzerPass`'s output and calls an LLM (whichever provider is already configured
    via `config.llm.provider` — no new provider selection) to extract `Concept` objects and
    `References`/`Custom`-kind relationship edges, each with evidence, so the same real-world
    concept mentioned across different documents can be found and linked — real semantic memory
    for AI tools, surfaced through the existing MCP tools (no new tool). Opt-in only
    (`[document-semantics] enabled = true`) since it's O(sections) LLM calls, unlike every other
    structural pass in this connector.
  - *Output:* `crates/recovery/src/document_semantics_analyzer.rs`; `crates/recovery/src/llm_json.rs`
    (shared JSON-fence-stripping, factored out of `sql_analyzer.rs` too); `ResolverConfig::
    kind_thresholds` + a minimum-name-length blocking guard in `crates/identity/src/lib.rs`;
    `DocumentSemanticsConfig` in `crates/compiler-core/src/config.rs`; gating in
    `crates/cli/src/commands/recover.rs`.
  - *Test/Validate:* Mock-LLM-driven creation/degradation/idempotency tests mirroring
    `sql_analyzer.rs`'s style; two identity-resolution regression tests proving neither
    degenerate outcome — a genuine cross-document Concept merge succeeds, and generic
    short-name Concepts across unrelated documents do not all collapse into one group (the
    devlog_27 Section over-merge failure shape, deliberately *not* repeated for Concepts, which
    unlike Sections must be allowed to merge). `cargo test --workspace`: 166 tests across the
    five touched crates, 0 failures; `cargo clippy --workspace --all-targets` and `cargo fmt
    --check` clean; zero `unsafe` introduced.

- [x] **Marketing Agent v1 — devlog → tweet → approval → X — RFC 0030**
  - *What:* Auxiliary tooling outside the compiler pipeline (devlogs are release notes, not
    enterprise knowledge — deliberately not a `CompilerPass`/`Observer`): `ekos marketing
    publish [devlog] [--yes] [--dry-run]` reads a `devlog_N.md`, classifies its importance
    (deterministic keyword heuristic — LOW skips before any LLM call), drafts a tweet via the
    already-selected `LlmProvider` (RFC 0008), validates it server-side (≤280 chars, mentions
    EKOS, includes the GitHub link, ≤3 hashtags — one retry on failure), gets interactive Y/N/E
    human approval, and publishes to X via a real RFC 5849 OAuth 1.0a-signed `POST /2/tweets`
    (`TwitterPublisher`) or a `NoopPublisher` for `--dry-run`/disabled. `marketing/posted/
    tweets.json` prevents double-posting a devlog; `marketing/logs/marketing.log` records every
    run. Config is `[marketing]`/`[marketing.twitter]` in `ekos.toml` — not the source design
    doc's standalone `marketing/config.yaml`, since this repo has exactly one config file/format.
  - *Output:* New crate `crates/marketing/` (`devlog.rs`, `importance.rs`, `prompt.rs`,
    `tweet.rs`, `oauth1.rs`, `publisher.rs`, `store.rs`); `crates/cli/src/commands/marketing.rs`;
    `MarketingConfig`/`TwitterConfig` in `crates/compiler-core/src/config.rs`; `marketing/`
    directory (README, template, runtime-created `posted/`/`logs/`).
  - *Test/Validate:* 44 new tests (37 in `ekos-marketing` incl. an RFC 2202 HMAC-SHA1 vector and
    OAuth1 determinism/sensitivity checks, 2 config tests, 5 CLI-orchestration tests); `cargo
    build/test/clippy -D warnings/fmt --check` all pass across the full workspace. Exercised live
    against real devlog content through the actual binary: `ekos marketing publish 28 --dry-run
    --yes` correctly parses and classifies (High) then fails with a clear "no API key" error
    (expected — no credentials in this environment); the LOW-importance skip and duplicate-post
    skip paths were run end-to-end with a synthetic devlog and a seeded `tweets.json`.
    `TwitterPublisher` itself has not been exercised against a live X account — open item in the
    RFC, not silently assumed correct.

---

## Phase 15 — Unified Transformation Semantics

Target scenario: reproducing a legacy Pentaho (Kettle) ETL job's business logic in a new pipeline,
with one rule changed, using EKOS instead of manually reading `.ktr`/`.kjb` XML and Confluence
tribal knowledge. A Pentaho step, a SQL `SELECT`, a `VIEW`, a stored procedure, and a function are
all the same underlying concept — a transformation of data from sources to a sink through
filter/join/aggregate/calculate operations — so every format compiles into one shared
Transformation IR before becoming Object/Relationship/Evidence, rather than N incompatible
per-format semantic models that cannot be diffed against each other. See
`ekos-transformation-semantics-plan.md` (repo root) for the full phase-by-phase implementation
plan.

**Status: all 8 phases complete** (0 RFC → 3 Pentaho → 1 IR → 2 SQL → 5 MCP tools → 6 agents →
4 identity resolution → 7 benchmark), per the plan's own phase order. One follow-up identified by
Phase 7's benchmark, not blocking: canonicalize `Join`/`Calculate` node text across producers so
`ekos_transformation_diff` doesn't report spurious changes there for semantically-identical logic.

- [x] **RFC 0027 — Unified Transformation Semantics**
  - *What:* Defines the `TransformNode`/`TransformGraph` IR (`Source`, `Filter`, `Join`,
    `Aggregate`, `Calculate`, `Sink`, and an explicit `Unmapped` for anything unparseable —
    recorded as evidence, never silently dropped) and how it lowers into
    `KirObject`/`KirRelationship`/`KirEvidence` via `ObjectKind::Custom("TransformNode")`, following
    the `Custom(...)` idiom RFC 0024/0026 already established. Argues deterministic SQL/Pentaho
    parsing is still observation-layer fact collection (same input always produces the same IR,
    zero judgment calls), while labeling *what a step means for the business* is recovery-layer
    interpretation, explicitly out of scope here. Confirms content-addressability (no
    non-deterministic fields) and append-only ledger fit (`Uuid::new_v5`-scoped ids per
    `(source_path, node_index)`, mirroring RFC 0026's `Concept` id scheme, so a later re-parse is a
    new version at a stable id, never a mutation). Flags cross-system identity resolution over
    Transformation-IR objects as its own later phase/RFC, not folded in here.
  - *Output:* `docs/rfcs/0027-unified-transformation-semantics.md`.
  - *Test/Validate:* RFC review per the mandatory workflow; no implementation code in this RFC by
    design (Phase 0 of the plan).
- [x] **Phase 1 — Transformation IR implementation** (`crates/semantic/src/transform_ir.rs`):
  `TransformNode`/`TransformGraph`/`TransformOrigin` types, `lower_to_kir`, one
  deterministic-serialization test per node variant written before the variant (TDD). No
  format-specific parser yet. Every IR node lowers to `ObjectKind::Custom("TransformNode")` with
  a `node_type` property, one evidence record citing the source path, and `Filter.condition`/
  `Calculate.expr` land in `properties["excerpt"]` so they're searchable via `ekos_search`/
  `ekos ask` for free. Deterministic ids for both objects and evidence
  (`transform_node_kir_id`/`transform_evidence_kir_id`, `Uuid::new_v5`-scoped per
  `(source_kind, source_path, node_index)`) — evidence ids had to be made just as deterministic
  as object ids, since `KirEvidence::new`'s default random id otherwise breaks the ledger's
  no-op-on-unchanged-content guarantee (`obj.evidence: Vec<KirId>` is part of what
  `content_signature` hashes). 16 tests, `cargo test --workspace`/`clippy --workspace
  --all-targets`/`fmt --check` all clean.
- [x] **Phase 2 — SQL analysis**: `crates/recovery/src/sql_transform_analyzer.rs`'s
  `SqlTransformAnalyzerPass` (pure structural, no LLM — distinct from `sql_analyzer.rs`'s
  DDL-only, LLM-enriched `SqlAnalyzerPass`) walks `sqlparser` ASTs for `SELECT`/`CREATE VIEW`
  (near-direct AST → IR: `FROM`/`JOIN` → `Source`/`Join`, `WHERE` → `Filter`, `GROUP BY` +
  aggregate projections → `Aggregate`, other computed projections → `Calculate`, a `VIEW`'s name
  → `Sink`) and stored procedures/functions at MVP scope: `CREATE PROCEDURE` bodies are pre-parsed
  by `sqlparser` into `Vec<Statement>` natively (MSSQL), so embedded `SELECT`s become real
  fragments and anything else becomes `Unmapped` with reason `"control flow present, not
  modeled"`; `CREATE FUNCTION` bodies (Postgres `AS $$ ... $$`) are opaque string literals to
  `sqlparser`, so a `;`-split-and-reparse heuristic extracts embedded `SELECT`s the same way.
  Dialects: native `PostgreSqlDialect`/`MsSqlDialect`/`DatabricksDialect` (Databricks coverage
  against real Spark SQL extensions not independently verified, documented not blocking);
  Informix has no dedicated `sqlparser` dialect, falls back to `GenericDialect` with accepted
  incomplete coverage, per the plan's explicit scope. `SqlTransformStats::coverage_percent()` is
  the readiness metric, printed by `ekos recover` as `"Transformation IR nodes (SQL): N total, X%
  mapped"`, registered alongside the existing DDL pass for every `.sql` file found. Dialect
  selection is not yet wired to `ekos.toml` (defaults to `"generic"` in `recover.rs`'s SQL-file
  walk) — flagged as follow-up, not blocking. 14 new tests covering the plan's own golden-example
  list (simple SELECT+WHERE, SELECT+JOIN, SELECT+GROUP BY, VIEW wrapping a multi-table query,
  stored procedure with embedded SELECT + non-SQL statement) plus CTE/dialect/coverage edge cases.
  `cargo test --workspace`/`clippy --workspace --all-targets`/`fmt --check` all clean.
- [x] **Phase 3 — Pentaho plugin**: `plugins/pentaho`'s `PentahoObserver` walks the workspace for
  `.ktr`/`.kjb` files and captures raw XML verbatim (no interpretation — that's fact collection,
  same split as `LocalDocsObserver`/`LocalDocAnalyzerPass`); `crates/recovery/src/
  pentaho_analyzer.rs`'s `PentahoAnalyzerPass` parses that XML via `roxmltree` (new workspace
  dependency — none existed before this phase) and maps steps to `TransformNode`s per the table:
  `TableInput`→`Source`, `FilterRows`→`Filter`, `Calculator`→`Calculate`,
  `DatabaseJoin`/`MergeJoin`→`Join`, `GroupBy`→`Aggregate`, `TableOutput`→`Sink`, anything else→
  `Unmapped` with the raw step XML preserved verbatim as evidence. `.kjb` job entries (orchestration,
  not data transformation) are always `Unmapped`, honestly, rather than forced into the mapping
  table. `PentahoStats::coverage_percent()` is the phase's readiness metric, printed by `ekos
  recover` as "Transformation IR nodes: N total, X% mapped". No real `.ktr`/`.kjb` sample files were
  available — synthetic fixtures per the implementation plan's explicit fallback, XML shapes
  documented as best-effort against Pentaho's known step-metadata conventions, not verified against
  a real export (flagged as follow-up once one is available). 14 new tests (4 observer, 10
  analyzer); `cargo test --workspace`/`clippy --workspace --all-targets`/`fmt --check` all clean.
- [x] **Phase 4 — Cross-system identity resolution**: RFC 0029 written and accepted first. New,
  deliberately separate `find_cross_system_candidates` (`crates/identity/src/cross_system.rs`,
  not a `DefaultResolver` config tweak — that resolver already excludes `Custom("TransformNode")`
  from its own blocking, the opposite posture cross-system matching needs) scores every pair of
  `Table`/`TransformNode` `Source`/`Sink` objects on column-name overlap (Jaccard, factored out to
  `similarity::column_names`/`jaccard`, now shared with RFC 0007's `structural_score`), naming-
  pattern similarity (schema-prefix + ETL-affix stripping, then Jaro-Winkler), and column-type
  compatibility (only when both sides carry typed columns) — signals degrade gracefully when
  absent rather than penalizing the pair. Candidates are written by a new `ekos identity scan` CLI
  command as `unconfirmed` `RelationshipKind::Custom("SameAs")` relationships (never auto-merged,
  never consumed by `DefaultResolver`/`apply_merges`), idempotent against pairs already known. A
  new `ekos_identity_review(relationship_id, decision)` MCP tool — the first write-capable tool,
  explicitly scoped to `SameAs` relationships only — confirms/rejects, writing a `KirEvent` to the
  ledger via new `append_event`/`get_event` surface added to `KnowledgeStore`/`Ledger`/`FactLedger`
  (the first real use of `EntryType::Event`, previously defined but never written anywhere).
  `demo/agents/identity-reviewer.md`'s "not yet wired" status note removed per the RFC's own
  acceptance criterion; new Act 10 in `demo/DEMO.md`. Rehearsed for real: a scratch workspace with
  a real `customers` table and a real `.ktr` job reading `dbo.cust_mstr`, `ekos identity scan`
  found the candidate and wrote it unconfirmed, and a real `ekos_identity_review` JSON-RPC call
  confirmed it end-to-end. 9 new tests in `cross_system.rs` (including the exact three-system
  `cust_mstr`/`customers`/`gold.dim_customer` scenario), 3 in `identity.rs`, 5 in `mcp.rs`, 2
  ledger event round-trip tests. `cargo test --workspace`/`clippy --workspace --all-targets`/`fmt
  --check` all clean.
- [x] **Phase 5 — MCP tools**: RFC 0028 written and accepted first, then implemented in
  `crates/cli/src/commands/mcp.rs`. `ekos_transformation_explain(id, max_hops?)` walks a
  Transformation IR chain upstream from `id` via `Runtime::trace_impact(id,
  ImpactDirection::Dependents, [Custom("FeedsInto")], max_hops)` — reusing `ekos_impact`'s exact
  mechanism, no new graph-walking code — and renders each `Source`/`Filter`/`Join`/`Aggregate`/
  `Calculate`/`Sink`/`Unmapped` node into a human-readable summary with resolved evidence (source
  path + fragment) per step, so every claim is traceable. `ekos_transformation_diff(old_id,
  new_id, max_hops?)` walks both chains and reports added/removed sets per node-type bucket
  (sources, sinks, filters, joins, aggregates, calculates) plus `Unmapped` counts — text-level set
  diffing over each node's rendered value, not a typed expression AST diff, per RFC 0027's own
  Open Question resolution (no consumer exists yet to justify building one). Real bug caught by
  the tests before merge: `ImpactDirection::Dependents` (not `Dependencies`) is the correct
  direction for walking upstream along `FeedsInto` edges — confirmed against `trace_impact`'s
  actual loop, not assumed from the enum variant names. 6 new tests (explain with evidence,
  unknown-object error, diff added/removed, diff-of-identical-chains-is-empty); `tools/list`'s
  asserted tool order updated. `cargo test --workspace`/`clippy --workspace --all-targets`/`fmt
  --check` all clean.
- [x] **Phase 6 — Agents**: `demo/agents/legacy-logic-recoverer.md` (sonnet) explains a
  Pentaho/SQL transformation chain via `ekos_transformation_explain`, citing evidence per step and
  flagging `Unmapped` portions honestly rather than guessing. `demo/agents/identity-reviewer.md`
  (sonnet) is written ahead of its dependency — batches unconfirmed cross-system identity
  hypotheses via `ekos_identity_review`, which is Phase 4 (not yet implemented); the agent
  definition carries an explicit Status note saying so. `impact-analyst`/`estate-architect` reused
  as-is (no changes needed). New Act 9 in `demo/DEMO.md`: recover Pentaho logic → check impact →
  draft new pipeline with a modified rule → diff against the original — rehearsed for real this
  session (not just written): a scratch workspace with one real `.ktr` file and one real SQL
  `CREATE VIEW` replacement, run through the actual `build → recover → resolve → compile → commit`
  pipeline and queried via real `ekos mcp serve` JSON-RPC calls (not a mocked transcript).
  `ekos_transformation_diff` correctly reported exactly one filter changed
  (`status = 'active'` → `status = 'active' AND region = 'EU'`) and every other bucket (sources,
  sinks, joins, aggregates, calculates, unmapped) unchanged. **Found and fixed a real bug live
  during rehearsal**: `ekos resolve` collapsed all 3 nodes of one pipeline into one canonical
  object at confidence 0.99 — the same `Custom("Section")` name-prefix over-merge shape from
  devlog 27/28, now hitting `Custom("TransformNode")` for the identical reason. Fixed by adding
  `Custom("TransformNode")` to `DefaultResolver`'s blanket kind-exclusion list
  (`crates/identity/src/lib.rs`), alongside `Section` — each node is already deterministically
  identified by `(source, node index)`, so no two distinct nodes can legitimately be the same
  entity. New regression test
  `transform_node_objects_are_never_merged_even_with_shared_source_prefix`. Re-verified against a
  full clean rebuild after the fix: zero merge proposals, all 6 nodes across both pipelines stayed
  distinct. `cargo test --workspace`/`clippy --workspace --all-targets`/`fmt --check` all clean.
- [x] **Phase 7 — End-to-end benchmark**: `crates/cli/tests/transformation_benchmark.rs` — a real
  Pentaho job (2 source tables, filter, join, calculated field, sink) and a real SQL `CREATE VIEW`
  redraft with one changed rule, run through the full pipeline, queried exclusively through
  `ekos_ekl`/`ekos_state`/`ekos_transformation_explain`/`ekos_transformation_diff` (no fixture
  file text read after setup — a permanent regression test, not a one-off script). Results: 100%
  coverage (zero `Unmapped` nodes on either side), every explanation step evidenced, the diff
  correctly isolated exactly the one changed filter rule. Real gap found and recorded, not hidden:
  `Join`/`Calculate` node text differs syntactically between the Pentaho and SQL producers for
  identical underlying logic (join-key tuple order reversed; calc-expression syntax differs) — the
  benchmark test deliberately does not assert those two buckets as unchanged, since asserting that
  would currently be false. Decision per the plan's own instruction: Phase 2 needs a small, bounded
  follow-up (canonical join-key ordering + calc-expression rendering) before `joins`/`calculates`
  diffing is trustworthy across producers; Phase 4 needs no further work (this benchmark doesn't
  exercise cross-system naming, already covered separately by Act 10's live rehearsal).
- [x] **Bug fix (opportunistic, unrelated to the IR work): `ekos ask` now honors
  `config.llm.provider`** — `crates/cli/src/commands/ask.rs` previously hardcoded
  `AnthropicProvider` directly, so it failed with "No LLM provider configured" even when
  `[llm] provider = "ollama"` was set and already working for `ekos recover` in the same
  workspace (RFC 0021 added Ollama support to the recovery path only). Fixed by calling
  `recover.rs`'s `build_llm_provider` (now `pub(crate)`) instead of duplicating provider-selection
  logic. Regression test `ask_selects_ollama_provider_when_configured` mirrors
  `recover.rs`'s existing selection tests. `cargo test --workspace`, `cargo clippy --workspace
  --all-targets`, `cargo fmt --check` all clean.

---

## Ongoing / Cross-cutting

These items have no single phase — they must be maintained and grown throughout the entire project lifecycle.

---

- [x] **Benchmark suite (`benchmark/`) — one benchmark per compiler pass**
  - *What:* Use `criterion` crate. One benchmark binary per phase-significant pass: `observation_git`,
    `sql_analyzer`, `identity_resolver`, `semantic_compiler`, `ledger_write`, `runtime_load_neighborhood`.
    Each benchmark uses a fixed fixture dataset so results are comparable across commits. Scope the
    PR-checklist benchmark requirement: benchmarks are mandatory only from Phase 4 onward (once real
    passes exist) and only for performance-relevant changes — scaffolding, CLI plumbing, and docs
    PRs in Phases 0–3 are exempt. A benchmark of an empty pass manager measures nothing.
  - *Output:* `benchmark/benches/*.rs`; `cargo bench` produces HTML reports in `target/criterion/`.
  - *Test/Validate:* `cargo bench` exits 0. CI stores benchmark results as artifacts. Any regression
    > 20% triggers a CI warning comment on the PR.

- [x] **Integration test harness (`tests/`) using real fixture data**
  - *What:* `tests/fixtures/` contains: `ecommerce.sql` (Postgres schema), `sample_project/` (small
    directory tree), `sample_docs/` (Markdown files), `git_fixture/` (a small committed git repo).
    `tests/integration/` contains end-to-end tests that run the full pipeline (build → recover →
    compile → commit → query) against these fixtures without external services.
  - *Output:* `tests/fixtures/`; `tests/integration/` test binaries; `cargo test --test integration` passes.
  - *Test/Validate:* `cargo test --test integration` from a clean clone with no external services
    running exits 0. Every phase's Validation section is covered by at least one integration test.
  - *Status:* Done, with near-real open-source fixtures rather than purely synthetic ones:
    `northwind.sql` (real 13-table Microsoft Northwind schema, MIT-licensed) added alongside
    `ecommerce.sql`; `git_fixture/odoo_utm.bundle` (39 real commits from `odoo/odoo`'s `addons/utm`
    module, LGPL-3.0, path-filtered via `git-filter-repo`, vendored as an offline-cloneable bundle);
    `sample_docs/` added. 3 end-to-end tests in `tests/integration/tests/integration.rs`. This pass
    caught and fixed two real pre-existing bugs the tiny synthetic fixtures never exercised: (1)
    `DefaultResolver`'s name-only similarity scoring falsely merged distinct tables sharing a name
    prefix (`orders`/`order_items`, `Employees`/`EmployeeTerritories`) — fixed by making the
    structural-similarity term use real column-overlap when column data is available; (2)
    `GitAnalyzerPass` read commit fields (`sha`, `author_name`, `files_changed`) at the wrong JSON
    nesting level, so it never produced a real `CoupledWith` relationship against any actual
    repository — fixed the indexing and tightened the two tests that had been silently passing
    despite the bug. Scope note: covers one comprehensive end-to-end test per fixture dataset
    through build→recover→resolve→compile→commit→query, not one test per every phase 0–14
    validation criterion.

- [ ] **Secrets management and sensitive-data policy**
  - [x] *Redaction pass* — RFC 0043 (`ekos/docs/rfcs/0043-secrets-and-pii-redaction.md`,
    devlog_43). A built-in, non-disable-able pattern table (AWS/GitHub/Slack/Google/Stripe token
    shapes, PEM private-key blocks, JWTs, generic `key/secret/password/token = value` assignments)
    redacts matched spans from all observed content before it reaches the artifact store or the
    ledger (`ekos_common::redaction`, wired into `build.rs`'s central artifact loop and
    `recover.rs`'s four direct-file-read blocks); a built-in excluded-file glob list (`.env`,
    `*.pem`, `id_rsa*`, …) drops near-100%-secret files entirely rather than redacting them.
    `[security]` in `ekos.toml` only extends the baseline (`extra-patterns`,
    `extra-excluded-globs`) — no way to disable it. Scope note: this is emails/national-IDs-style
    *generic* PII in free text — email addresses in prose are not yet covered (only credential-
    shaped secrets); structured connector-modeled PII (git commit author name/email) is
    intentionally exempt, see the RFC's Non-goals.
  - [ ] *Env-var-only connector secrets* — not yet done. Connectors need DB passwords and API
    tokens (Postgres, Salesforce, SAP). Standardise: all secrets referenced by env-var name in
    `ekos.toml` (e.g., `password_env = "PG_PASSWORD"`), never as literal values; `ekos doctor`
    verifies referenced vars exist.
  - [ ] *Data retention/erasure RFC* — not yet done. GDPR right-to-erasure vs. the append-only
    ledger guarantee is still an open tension RFC 0043 explicitly did not resolve (redaction stops
    *new* secrets from being stored; it does not provide a way to erase something already
    committed before this RFC shipped).
  - *Test/Validate (remaining):* A config containing a literal `password = "..."` fails validation
    with a clear error (env-var-only secrets, not yet built).

- [ ] **Multi-project/estate-scale follow-ups (RFC 0044)**
  - [x] *Object-identity fix* — `build.rs`'s `File` objects are now project-qualified
    (`"project"` property + id-hash qualification) when `[observe] paths` lists more than one
    entry, closing a real silent-merge bug for two projects sharing a same-relative-path file.
  - [x] *Hierarchical rollups* — `ekos_semantic::rollup` synthesizes one `Rollup` object per
    directory/project group (≥2 members), run in `ekos commit` (not `ekos compile` — see
    devlog_44 for why the first placement produced zero real rollups against this repo's own
    ledger despite passing every unit test). Deterministic, zero-LLM: real member counts +
    boundary-relationship counts + `Contains` links to every member. Surfaced in
    `Architecture.md`'s new `## Subsystems` section (RFC 0042's curated docs).
  - [x] *Analyzer-owned id-collision risk beyond `File`* — closed for four of five, RFC 0079 /
    `devlog_82`. `build.rs` writes a `"project"` field onto every artifact's `data` object at the
    RFC 0043 redaction choke point (absent for the single-path case); a new shared
    `ekos_common::project::project_qualify` helper qualifies id-hash inputs only, never displayed
    names. Wired into `local_docs_analyzer.rs`, `rust_analyzer.rs`, `python_analyzer.rs`,
    `git_analyzer.rs`'s `CoupledWith`. Live-verified: a real two-project fixture with an
    identically-named/shaped Rust file in each produced two distinct `RustSymbol` ids (caught and
    fixed a real bug in the first attempt — see devlog for the live-test failure that found it).
    - *Still open, honestly scoped*: `github_analyzer.rs`'s `file_kir_id` (for `References` edges
      to files mentioned in PR/issue text) is a structurally different problem — a path parsed
      from free text has no single `[observe] paths` entry it naturally belongs to. Investigation
      found it's now *silently wrong*, not just collision-risky, in a multi-project workspace: it
      still computes the bare-path id, which no longer matches `build.rs`'s own project-qualified
      `File` object, so the `References` edge dangles rather than colliding.
  - [ ] *Per-sub-project curated docs* — not yet done. `ekos docs generate` (any layout) reads the
    whole ledger; there's no way to scope curated output to one project within a shared estate
    ledger. Today this requires N separate `ekos.toml`/`.ekos` setups (confirmed this session for
    Databricks/ADF). The `"project"` property RFC 0044 added is exactly what this would key off of.
  - [ ] *Opt-in LLM prose per rollup* — not yet done. Mirror `docs-gen`'s `--prose` (RFC 0035
    Phase 5) exactly: one `AiRuntime::ask`-shaped, citation-validated call per rollup, cost estimate
    shown and confirmed before spending.
  - [ ] *`ekos_summarize` MCP tool* — not yet done. A tool that jumps straight to the nearest
    enclosing rollup for a given object id. Not blocking — rollups are ordinary `KirObject`s, so
    `ekos_search`/`ekos_neighborhood`/EKL already surface them for free.
  - *Test/Validate (remaining):* Same-relatively-named-file collision test extended to a non-`File`
    kind (e.g. two projects each with a `RustSymbol` at the same qualified name) to prove the
    analyzer-owned fix once built; `ekos docs generate --layout curated --project <name>` (or
    equivalent) produces output scoped to one project's objects only.

- [ ] **`docs/rfcs/` — RFC per feature, accepted before implementation**
  - *What:* Maintain the RFC process from Phase -1 throughout the project. New RFCs follow the
    `0000-template.md`. An RFC is merged only when: all open questions are answered, at least one
    review has been completed, and the status is set to `Accepted`. The RFC number is referenced in
    all code and commit messages that implement the feature.
  - *Output:* Every feature in phases 0–14 has a corresponding accepted RFC.
  - *Test/Validate:* Before starting any phase, confirm its RFC file exists in `docs/rfcs/` with
    status `Accepted`. `git log --grep='RFC'` finds references in commit messages for every phase.

- [ ] **Every public API has rustdoc with example**
  - *What:* Every `pub` function, struct, trait, and enum in every crate must have a `///` doc comment
    with at least one sentence and one `# Example` block that compiles (`cargo test --doc`). No
    `#[allow(missing_docs)]` attributes permitted.
  - *Output:* `cargo doc --workspace --no-deps` produces zero warnings. `cargo test --doc` passes.
  - *Test/Validate:* Add `#![deny(missing_docs)]` to each crate's `lib.rs`. CI runs `cargo doc
    --workspace --no-deps 2>&1 | grep warning` and fails if any match is found.

- [ ] **RFC 0045 — hosted demo server (peer-validation MVP)**
  - *What:* A small, fixed two-repo hosted demo (`ekos/crates/demo-server`): pre-baked, pre-rendered
    curated docs for EKOS-self and one clean external repo (`sharkdp/fd`), plus one evidence-cited
    `POST /ask` endpoint (a thin adapter around `AiRuntime::ask`, unmodified). Built to answer a
    strategic question, not a roadmap phase: narrow EKOS to one painful task (making sense of a
    codebase without hitting an LLM's context-window ceiling, devlog_44's framing) and get it in
    front of ~20 peer architects/senior engineers in a 5–10 minute demo. See `devlog_45.md` and
    `ekos/docs/rfcs/0045-hosted-demo-server.md`. Deliberately not general self-serve ingestion — a
    fixed catalog only, see the RFC's Non-goals.
  - *Output:* `ekos/crates/demo-server/` (axum server + `prerender` bake binary); `cli`'s
    `build_llm_provider`/`ai_config` widened `pub(crate)` → `pub` for reuse.
  - *Status:* Implemented and, as of RFC 0046/devlog_46, **live-tested end to end with a real
    OpenAI key** — build/test/clippy/fmt clean across the workspace; boot-time key-check, rate
    limiting, static doc serving, unknown-repo handling, and real `/ask` calls against both baked
    ledgers all confirmed working. A real, reproducible pre-vetted question list now exists (see
    devlog_46's table). **Still not fully demo-ready**: the 5–10 minute script hasn't been
    rehearsed against a real person — that's the one remaining step, and it needs a live human.
  - *Test/Validate (remaining):* Time the full script against someone unfamiliar with EKOS, under
    10 minutes including one live question — use a question from devlog_46's confirmed-good list,
    not an untested one; the same session's live testing found real per-question variance (see the
    two new follow-up items below).

- [ ] **RFC 0046 — OpenAI LLM provider**
  - *What:* `OpenAiProvider` (`ekos/crates/recovery/src/openai.rs`), wired into
    `build_llm_provider`'s existing provider-selection `match` (RFC 0021's extension point) via
    `config.llm.provider = "openai"`. Built specifically to unblock RFC 0045's demo server when no
    `ANTHROPIC_API_KEY` was available. See `ekos/docs/rfcs/0046-openai-llm-provider.md` and
    `devlog_46.md`.
  - *Output:* `recovery/src/openai.rs`; `demo-server`'s `RepoEntry` gained `llm_provider`/
    `llm_api_key_env`/`ai_max_tokens` catalog-level overrides (so the demo can force OpenAI without
    editing EKOS-self's real `ekos.toml`); `.env` loading in `demo-server` matching the existing
    `marketing/.env` pattern.
  - *Status:* Done. Live-verified against both catalog repos with a real key.
  - *Test/Validate:* `OpenAiProvider` unit tests (`model_name`, `temperature: 0`); `build_llm_provider`
    provider-selection test; `demo-server`'s boot-check test extended for the `"openai"` branch;
    full workspace `cargo build/test/clippy/fmt` clean.

- [x] **RFC 0047 — Claims and temporal validity (graph layer)**
  - *What:* The graph-layer slice of `EKOS_World_Engine_Development_Plan.md` (a much larger,
    not-yet-decided proposal to pivot EKOS toward a general knowledge-graph + multi-agent
    simulation platform — see this session's written analysis and `devlog_47.md`). Three additive
    extensions to existing types, no new primitives: `valid_from`/`valid_until` on
    `KirRelationship`; `is_pending_review()` generalized from RFC 0029's hardcoded
    `Custom("SameAs")` to any relationship kind carrying the same `status` convention;
    `object_history`/`relationship_history` on both `KnowledgeStore` backends and `Runtime`. See
    `ekos/docs/rfcs/0047-claims-and-temporal-validity.md`.
  - *Output:* `kir/src/lib.rs`, `ledger/src/lib.rs`, `ledger/src/fact_ledger.rs`,
    `runtime/src/lib.rs`, `runtime/tests/graph_layer_fixture.rs` (5-person/3-org/10-event/
    15-relationship/5-claim fixture, both ledger backends).
  - *Status:* Done. A real bug in the first draft of the generalized `is_pending_review()` was
    caught before merge — narrowing the check to `== "unconfirmed"` would have let a *rejected*
    identity candidate leak back into traversal, since `ekos_identity_review`'s `decision` is
    `"confirmed"` or `"rejected"`, not just `"unconfirmed"`/`"confirmed"`. Fixed to
    `!= "confirmed"`, matching the original's actual (safer) semantics.
  - *Test/Validate:* 16 new unit tests across `kir`/`ledger`/`fact_ledger`/`runtime` plus the
    2-backend integration fixture; full workspace `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* no new
    `KirClaim` type, no claim-review MCP tool, no `valid_from`/`valid_until` on `KirObject`, and no
    World Model / Agent Model / Simulation Engine code — the source document's Phase 3 onward
    remains an open strategic decision, not started.

- [x] **RFC 0048 — World Model**
  - *What:* The next step in `EKOS_World_Engine_Development_Plan.md`'s own recommended
    development order (§44), immediately after RFC 0047's graph-layer/temporal-state work — user
    confirmed continuing toward the World Engine vision, one RFC at a time. `EventKind` gains a
    `Custom(String)` escape hatch (mirroring `ObjectKind`/`RelationshipKind`); a new `World`
    read-model (`runtime/src/lib.rs`, alongside `ObjectState`/`ImpactHop`) plus
    `Runtime::build_world` compute the induced subgraph over a scoped entity set, current or
    historical. Resources and channels are documented `properties`/`Custom("Channel")` conventions,
    not new primitives. See `ekos/docs/rfcs/0048-world-model.md` and `devlog_48.md`.
  - *Output:* `kir/src/lib.rs`, `runtime/src/lib.rs`, `runtime/tests/graph_layer_fixture.rs`
    (extended with a `Channel` object + 2 new World-scoped tests, both ledger backends).
  - *Status:* Done. Deliberately **no new storage** — `World` is a computed projection over
    existing `KnowledgeStore` queries, not a persisted entity, matching the existing
    `ObjectState`/`ImpactHop` read-model pattern rather than the source document's literal
    "world is a stored graph+state structure" framing.
  - *Test/Validate:* 5 new unit tests (`kir` + `runtime`) plus 2 new integration tests extending
    RFC 0047's fixture; full workspace `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* no Agent
    Model / Decision Engine / Action System / Simulation Engine code; no `World` persistence path;
    no `ekos world create`/`ekos simulate` CLI commands; no dedicated `ekos-world` crate.

- [x] **RFC 0049 — Agent Model (definitions, beliefs, knowledge, observation)**
  - *What:* The next step in `EKOS_World_Engine_Development_Plan.md`'s own recommended development
    order (§44), immediately after RFC 0048's World Model — user confirmed continuing toward the
    World Engine vision, one RFC at a time. `ObjectKind::Custom("SimulatedAgent")` (a distinct kind
    from the pre-existing `ObjectKind::Agent`, which means something else — a discovered AI agent
    definition artifact, not a simulation participant) with `role`/`goals`/`fears`/`resources`
    `properties` conventions; beliefs reuse RFC 0047's claim machinery unmodified
    (`Custom("Trusts")`/etc. for beliefs about existing entities, `Custom("Proposition")` +
    `Custom("Believes")` for free-form propositional beliefs — closing a limitation RFC 0047 had
    explicitly named as deferred); knowledge as `RelationshipKind::Custom("Knows")`, a confirmed
    (non-claim) fact; `Runtime::agent_observation` built directly on RFC 0048's `build_world`. See
    `ekos/docs/rfcs/0049-agent-model.md` and `devlog_49.md`.
  - *Output:* `runtime/src/lib.rs` (`World.events` field + `build_world` event fallback,
    `Runtime::agent_observation`), `runtime/tests/graph_layer_fixture.rs` (extended with `Knows`
    edges from two fixture people to different event subsets, both ledger backends).
  - *Status:* Done. Found and fixed a real gap in RFC 0048's own `build_world` while implementing
    this RFC: it only ever tried `get_object`/`object_at` for scope ids, silently dropping any id
    that resolved to an event instead. This RFC's own worked example (agents that `Know` about
    events, not just objects) exercised the gap immediately. Fixed by adding
    `events: Vec<KirEvent>` to `World` and a `get_event` fallback in `build_world`, filtered by
    `occurred_at <= at` when historical.
  - *Test/Validate:* 5 new unit tests (`runtime`) plus 2 new integration tests extending RFC 0047/
    0048's fixture, both ledger backends; full workspace `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* no
    Decision Engine / Action System / Simulation Engine code; no memory-type taxonomy
    (short-term/long-term, belief revision over simulation rounds); no `ekos agent create`/
    `ekos interview` CLI commands; no enforcement of internally-consistent agent beliefs. Also not
    done: a dedicated test constructing `SimulatedAgent`/`Proposition`/`Believes` end-to-end (the
    RFC argues, correctly per RFC 0047's generalization, that these need no new code — but no test
    in this diff proves it beyond argument; noted as still open in `devlog_49.md`).

- [x] **RFC 0050 — Decision Engine, Action System, Simulation Engine**
  - *What:* Phases 5-7 of `EKOS_World_Engine_Development_Plan.md` in one RFC — the user's explicit
    choice, after being told this is the first fork in the continuation with zero prior art
    anywhere in EKOS (unlike RFC 0047-0049, each a small extension of something that already
    existed). New crate `ekos-simulation`: a closed 12-action vocabulary (`ActionKind`, no
    `Custom()` escape hatch — a scope decision, not a taxonomy) with structural validation plus one
    real precondition (`FormAlliance` requires `Trusts` value `> 0.4`, the source document's own
    worked example); a provider-independent `DecisionEngine` trait (mirrors `LlmProvider`'s shape)
    with two deterministic reference engines (`AlwaysDoNothing`, `RuleBasedAgent` — no LLM call
    anywhere in this crate); a round-based `Simulation::run_round` implementing the full
    observe/decide/validate/resolve/execute/persist/update-world/update-memory lifecycle, reusing
    RFC 0048/0049's `Runtime::agent_observation`/`build_world` for every read and writing through
    `KnowledgeStore` directly (not `Runtime`, which stays read-only per RFC 0005 — unchanged by
    this RFC). See `ekos/docs/rfcs/0050-decision-action-simulation-engine.md` and `devlog_50.md`.
  - *Output:* `crates/simulation/` (new workspace member): `action.rs`, `decision.rs`,
    `simulation.rs`, `lib.rs`; `crates/simulation/tests/simulation_fixture.rs` (new, self-contained
    3-agent integration test, both ledger backends).
  - *Status:* Done. The RFC's original Testing section assumed `crates/simulation`'s test could
    "reuse" `runtime/tests/graph_layer_fixture.rs`'s fixture loader — not expressible in Rust
    (a crate's `tests/` code isn't part of its public API, and `ekos-simulation` depends on
    `ekos-runtime`, not the reverse). Caught while writing the test; `simulation_fixture.rs` ships
    its own small fixture instead, and the RFC's Testing/Acceptance-Criteria sections were
    corrected to match.
  - *Test/Validate:* 11 new unit tests (`ekos-simulation`) plus 3 new integration tests (one round
    end-to-end on both backends, plus a same-starting-state determinism check); full workspace
    `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* Phase 8
    (Parallel Agent Execution — this RFC's ordering already produces the same order-independent
    semantics Phase 8 wants); Phase 9 (Conflict Resolution: priority rules, resource constraints,
    `--seed` — nothing in this loop is stochastic yet); any Phase 10+ work (scenarios, replay,
    metrics, turning points, reports, Monte Carlo, counterfactuals, web UI, video); an LLM-backed
    `DecisionEngine`; per-kind action effects beyond `FormAlliance`'s one worked example; an
    `ekos simulate` CLI command; any new `KnowledgeStore` trait methods.

- [x] **RFC 0051 — Scenario Definition**
  - *What:* Phase 10 of `EKOS_World_Engine_Development_Plan.md` — the user's explicit choice over
    deepening the engine further (Phase 9 Conflict Resolution), specifically to close a gap named
    three RFCs running: nothing built in RFC 0047-0050 was runnable by anyone who wasn't editing
    this codebase. `AgentDefinition`/`ScenarioDefinition` YAML schemas (source document §9.1/§15),
    reusing every RFC 0049/0050 convention unmodified; a two-pass `load_scenario`/
    `load_scenario_from_path` loader resolving scenario-local name references (with a fallback to
    existing-ledger-id resolution); a new `ekos simulate <scenario.yaml>` CLI command. One
    safety-relevant decision shaped the RFC: `ekos simulate` writes to a dedicated
    `.ekos/simulations/<scenario-id>/ledger.db` by default, never the real workspace ledger — the
    ledger has no delete/tombstone mechanism anywhere in this codebase (RFC 0043), so fictional
    simulation entities must not become permanent neighbors of real compiled knowledge.
    `--ledger <path>` opts back in explicitly. See
    `ekos/docs/rfcs/0051-scenario-definition.md` and `devlog_51.md`.
  - *Output:* `crates/simulation/src/scenario.rs` (new), `crates/simulation/src/action.rs`
    (`ActionKind::all()`), `crates/simulation/tests/scenario_fixture.rs` (new),
    `crates/cli/src/commands/simulate.rs` (new), `bin/ekos.rs` (`Commands::Simulate`).
  - *Status:* Done. Found and fixed a real gotcha while writing this RFC's own test fixture:
    `RuleBasedAgent`'s `support:<name>`/`oppose:<name>` goal convention matches a target's `name`
    field exactly and case-sensitively, distinct from a scenario's own (often differently-cased)
    lowercase reference id — silently produced `DoNothing` with no error the first time the test
    fixture mismatched case. Fixed the test data and strengthened `RuleBasedAgent`'s own doc
    comment so a scenario author hits documentation, not silent confusion. Also fixed a
    `clippy::large_enum_variant` on `AgentRef` by boxing the inline variant.
  - *Test/Validate:* 12 new unit tests (`scenario.rs`) plus 3 new integration tests (multi-file
    scenario end-to-end, inline agents, unknown-reference error path); a manual CLI smoke test
    against a real 3-file scenario confirming both the decisions produced and that only
    `.ekos/simulations/<id>/ledger.db` was created (never `.ekos/ledger/`); full workspace
    `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:*
    `world.sources` document ingestion (scenario `knowledge:` references stay scoped to
    scenario-local names, a minimal scenario-authored `events:` section, or existing ledger ids);
    Phase 11 (Virtual Social Environment); `--seed`-driven randomness (parses, prints a visible
    no-op notice); per-agent decision-engine selection in YAML (every agent gets `RuleBasedAgent`);
    scenario linting beyond structural/reference errors; a scenario ledger cleanup command.

- [x] **RFC 0052 — Conflict Resolution**
  - *What:* Phase 9 of `EKOS_World_Engine_Development_Plan.md` — the user's explicit choice over
    Phase 11 (Virtual Social Environment) or `world.sources` document ingestion, closing a gap RFC
    0050 named twice: `SimulationConfig` had no seed, and "resolve conflicts" was a documented
    no-op. Tracing the source document's own worked example (`Alice SUPPORT Bob` /
    `Charlie OPPOSE Bob`) through the actual engine found it produces no real collision under the
    current effect model (no shared mutation target) — so this RFC built the one genuine same-round
    collision the engine can currently produce instead: two agents racing for a shared, scarce
    `Custom("Channel")` `resources.capacity`. `SimulationConfig.seed`; priority ordering by
    `Decision.confidence` descending (reusing an existing, previously-unread field) with a seeded,
    reproducible tie-break for equal-confidence ties (the common case); opt-in per-agent
    `resources.energy` costs on every non-`DoNothing` action; `RoundResult.conflict_failures`,
    distinct from `validation_failures` (lost a same-round race vs. was never reasonable);
    `ekos simulate --seed`. See `ekos/docs/rfcs/0052-conflict-resolution.md` and `devlog_52.md`.
  - *Output:* `crates/simulation/src/simulation.rs` (seeded ordering, resource consumption,
    conflict detection), `crates/simulation/src/scenario.rs` (seed wiring),
    `crates/simulation/tests/conflict_fixture.rs` (new), `crates/cli/src/commands/simulate.rs`
    (`--seed`), `bin/ekos.rs` (`Commands::Simulate.seed`).
  - *Status:* Done. Verified live with `ekos simulate scenario.yaml --seed 99` (prints
    `Seed:     99`, runs normally). All 25 pre-RFC-0052 tests continue to pass unmodified — resource
    checks are opt-in (`NoSuchResource` when a `resources` key is absent), so no existing fixture's
    behavior changed; only the two `SimulationConfig` struct literals needed a mechanical `seed`
    field added.
  - *Test/Validate:* 10 new unit tests (`simulation.rs`: priority ordering determinism/tie-break/
    round-variance, `try_consume_resource`'s three outcomes) plus 3 new integration tests
    (`conflict_fixture.rs`: exactly-one-winner, same-seed-same-winner, different-seeds-can-differ —
    the last two seed values, 1 and 2, empirically confirmed to differ, not guessed); full workspace
    `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:*
    per-action-kind differentiated resource costs (one uniform constant for all 11 non-`DoNothing`
    kinds); YAML-authorable costs; richer conflict rules beyond the one worked shared-capacity
    example; Phase 8 Parallel Agent Execution; any randomness in `DecisionEngine` behavior itself
    (the seed governs only tie-break ordering, never what an agent decides); a relationship effect
    for `Support`/`Oppose` (confirmed, not built, that the source document's own literal example
    doesn't need one under the current effect model).

- [x] **RFC 0053 — Virtual Social Environment**
  - *What:* Phase 11 of `EKOS_World_Engine_Development_Plan.md` — the user's explicit next step
    ("go next phase 11"). A `VirtualForum` (`create_channel`/`publish_message`/`like`/`follow`/
    `share`/`read_messages`) built as a direct API over `&dyn KnowledgeStore`, deliberately without
    reopening RFC 0050's closed, escape-hatch-free `ActionKind` vocabulary: checked each of the
    source document's seven capabilities structurally before designing and found none actually
    needs a 13th action kind — `reply` is a message with a parent pointer (`Action.reply_to`,
    additive, not a new kind); `like`/`follow`/`share` are relationship-shaped social facts
    (`Custom("Likes")`/`Custom("Follows")`/`Custom("Shares")`), the same posture RFC 0049 gave
    `Knows`. `read_messages` needed a new `Custom("PostedIn")` relationship index (message event →
    channel) because `KnowledgeStore` has no bulk "every event" query — confirmed by re-reading the
    trait before assuming otherwise — reusing the same "a relationship can point at an event"
    pattern RFC 0049's `Knows` edges already established. See
    `ekos/docs/rfcs/0053-virtual-social-environment.md` and `devlog_53.md`.
  - *Output:* `crates/simulation/src/action.rs` (`Action.reply_to`), `crates/simulation/src/
    forum.rs` (new: `VirtualForum`, `ForumError`), `crates/simulation/src/simulation.rs`
    (`try_consume_resource` made `pub(crate)`, `PostedIn` indexing wired into the round-based
    `PostMessage` path), `crates/simulation/tests/forum_fixture.rs` (new).
  - *Status:* Done. The source document's own worked loop (§16: Alice posts → Bob observes → Bob
    decides → Bob replies) runs end-to-end through the real, unmodified Decision/Action/Simulation
    Engine — Bob's "observing" Alice's post is exactly RFC 0050's existing public-action `Knows`
    fanout, no forum-specific observation logic added.
  - *Test/Validate:* 5 new unit tests (`forum.rs`: channel creation, capacity + indexing, non-
    channel rejection, like/follow/share idempotency, read-order/scoping) plus 1 new integration
    test (`forum_fixture.rs`: the full two-round post-observe-decide-reply loop, plus a
    direct-API-seeded third message proving `read_messages` sees both origin paths against the
    same channel); full workspace `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* round-based
    `Like`/`Follow`/`Share` actions (`ActionKind` unchanged at exactly 12 variants — reaffirmed,
    not reopened); any real platform (X/Reddit) integration, per the source document's own
    instruction; a nested-thread-reconstruction helper (`reply_to` is a flat pointer only); any new
    `KnowledgeStore` trait method; moderation/rate-limiting semantics.

- [x] **RFC 0054 — Event Store (closing Phase 12) and Simulation Replay (Phase 13)**
  - *What:* The user's chosen path after RFC 0053: close Phase 12 honestly, then build Phase 13 on
    top of it. Phase 12 turned out almost entirely satisfied already by the existing
    `ActionExecuted` event shape (id/round/timestamp/actor/action/target/content); the one real gap,
    `observed_by`, closed as a derived query (`observed_by()`) over existing `Knows` edges, not new
    storage. A durable per-ledger `SimulationLog` (`Custom("LoggedIn")` indexing every executed
    action, all 12 kinds including `DoNothing`) was the real, necessary prerequisite Phase 13
    needed — `RoundResult` only ever existed in memory before this, gone once the process exited.
    `Replay` (`open`/`rounds`/`events_in_round`/`jump_to`/`inspect_agent`/`inspect_graph`/
    `observed_by`) reuses RFC 0048/0049's point-in-time reconstruction machinery entirely, adding
    only the "which timestamp is which round" lookup nothing could answer before. New
    `ekos replay <scenario.yaml> [--round N]` CLI command, read-only by construction. See
    `ekos/docs/rfcs/0054-event-store-and-replay.md` and `devlog_54.md`.
  - *Output:* `crates/simulation/src/simulation.rs` (log indexing, `observed_by`),
    `crates/simulation/src/replay.rs` (new), `crates/simulation/tests/replay_fixture.rs` (new),
    `crates/cli/src/commands/replay.rs` (new). Plus, outside the original plan: `crates/ledger/
    src/lib.rs` and `crates/ledger/src/fact_ledger.rs`.
  - *Status:* Done. Found and fixed a real, pre-existing bug in **both** ledger backends while
    writing `Replay`'s own historical-reconstruction test: `relationships_at` (SQLite and
    `FactLedger` alike) only ever reconstructed a relationship's *current* version filtered by
    timestamp, never a genuinely historical one — a documented RFC 0011 limitation, "kept for
    parity" between backends rather than fixed, that nothing before this RFC happened to query in a
    way that exposed it (needs a relationship updated more than once, queried at a point in time
    before its current version). Fixed both backends to match `object_at`'s already-correct
    per-version reconstruction pattern. Also found and corrected a smaller assumption in this RFC's
    own first test draft: `jump_to(round)` resolves to that round's *pre-round* snapshot (matching
    `Simulation::run_round`'s own Observe-step invariant), not a post-round one — doc comments
    updated to state this explicitly.
  - *Test/Validate:* 2 new unit tests (`simulation.rs`: log idempotency, `observed_by` regression
    against RFC 0050's own visibility fanout) plus 4 new unit tests (`replay.rs`) plus 2 new
    integration tests (`replay_fixture.rs`, both ledger backends) plus 2 new regression tests
    directly in `ekos-ledger` (one per backend) pinning the `relationships_at` fix; a live CLI check
    confirming `ekos replay` left the scenario ledger's entry count unchanged (21 before, 21 after);
    full workspace `cargo build/test/clippy/fmt` clean (91 passing test-result blocks).
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:* an
    interactive replay session (`start`/`pause`/`next round` as literal REPL commands — `Replay`'s
    methods are the primitives such a UI would be built from, not the UI); `observed_by` baked into
    the event payload (derived query only); metrics, turning-point detection, or report generation
    (Phases 14-16); video/report rendering of a replay.

- [x] **RFC 0055 — `world.sources` Document Ingestion**
  - *What:* The user's explicit choice, closing the one remaining named fork from RFC 0051: real
    documents seeding a scenario's starting world, instead of only hand-authored `events:` YAML.
    The first RFC in this continuation to reach outside the graph/simulation layer into the real
    compiler pipeline — `ScenarioDefinition.world.sources` wires the actual `ekos-plugin-localdocs`
    connector and `LocalDocAnalyzerPass` (pure structural, no LLM, confirmed by its own module doc)
    into scenario loading, scoped to exactly one connector and one pass, not the full `ekos build`/
    `ekos recover` machinery (SQL/Git/GitHub/Confluence/crypto observers, dialect registries,
    fingerprint caching, `PassManager` DAG scheduling — none of it wired). RFC 0043's redaction
    baseline applies at this entry point with the same choke-point treatment `build.rs` already
    gives every observer artifact. Ingested `Document`/`Section`/`Table` objects join the scenario's
    existing name registry (RFC 0051) by their own path-derived names, so an agent's `knowledge:`
    can reference `world.sources: [reports/report_01.md]` by that same string. See
    `ekos/docs/rfcs/0055-world-sources-document-ingestion.md` and `devlog_55.md`.
  - *Output:* `crates/simulation/src/ingest.rs` (new), `crates/simulation/src/scenario.rs`
    (`WorldDefinition`, `load_scenario`'s `world_objects` parameter), 6 new Cargo dependencies.
  - *Status:* Done. Found and fixed two real issues along the way, neither caught by the first
    green test run: (1) `LocalDocsObserver::scan` computes each artifact's path via
    `strip_prefix(root)`, so pointing `ScanContext` at a single source file directly (the natural
    first design) would have silently produced an empty document name — caught by reading the
    connector's own scan loop before writing ingestion code, fixed by scanning the scenario
    directory once and filtering to the exact requested allowlist. (2) A genuine runtime-nesting
    bug: `ingest_sources`'s original sync-to-async bridge unconditionally built a fresh Tokio
    runtime and called `block_on`, which works from a plain `#[test]` fn (no runtime active) but
    panics ("Cannot start a runtime from within a runtime") when called from `ekos simulate`'s own
    `#[tokio::main]` entry point — invisible to every automated test, since none of them run inside
    an active Tokio runtime; only found by actually running the CLI by hand. Fixed by branching on
    `tokio::runtime::Handle::try_current()`, using `tokio::task::block_in_place` when already inside
    a runtime.
  - *Test/Validate:* 4 new unit tests (`ingest.rs`: real markdown ingestion, missing-source error,
    allowlist exactness, redaction) plus 1 new end-to-end integration test (`scenario_fixture.rs`);
    live-verified against the real `ekos simulate` CLI command after the runtime fix (log line
    confirmed `objects=2 edges=1` real ingestion output); full workspace
    `cargo build/test/clippy/fmt` clean.
  - *Explicitly not done, per the RFC's own Non-goals and the user's confirmed scope:*
    `DocumentSemanticsAnalyzerPass` (LLM-based Concept extraction — real, deferred, opt-in the same
    way it already is upstream); any observer besides `localdocs`; dialect registries, fingerprint
    caching, `PassManager` DAG scheduling; workspace `[security]` redaction extensions (built-in
    baseline only); incremental/cached re-ingestion (every scenario load re-runs the pipeline
    fresh, matching RFC 0051's existing posture for agents/relationships).

- [x] **RFC 0056 — ClickHouse Connector: Compiled Metadata + Live NL-to-SQL Query Engine**
  - *What:* User request: EKOS+AI answering natural-language questions with an LLM-built SQL query
    run live against ClickHouse, grounded in EKOS's own compiled metadata. Checked against EKOS's
    own stated invariant ("AI systems consume knowledge through the Runtime only... they never
    touch raw enterprise systems directly") before designing — no MCP tool, `AiRuntime`, or
    connector in this codebase had ever crossed that line before. Split into two stages: Stage 1
    (`ekos-plugin-clickhouse` + `ClickHouseAnalyzerPass`) compiles ClickHouse table/column metadata
    into real `ObjectKind::Table` KIR objects — closing the exact `build.rs`/`recover.rs` wiring gap
    the RFC 0012 Snowflake/Oracle scaffolds never closed — with zero invariant risk. Stage 2
    (`ekos-clickhouse-query`) is the new auxiliary crate that actually crosses the line: a
    six-stage question -> compiled-schema-context -> LLM-built SQL -> SELECT-only validation
    (`ekos-plugin-sql-dialect-clickhouse`, wrapping the real `sqlparser::dialect::ClickHouseDialect`)
    -> live execution -> redacted, audited dataset pipeline. Exposed via `ekos clickhouse ask`
    (always available) and a gated `ekos_clickhouse_query` MCP tool (off by default, only listed
    once `[clickhouse].enable-mcp-query = true` is set in `ekos.toml`). See
    `ekos/docs/rfcs/0056-clickhouse-connector.md` and `devlog_56.md`.
  - *Output:* New crates `plugins/clickhouse`, `plugins/sql-dialect-clickhouse`,
    `crates/clickhouse-query`; `crates/recovery/src/clickhouse_analyzer.rs`; CLI/MCP wiring;
    `[clickhouse]` config section.
  - *Status:* Done. The key design decision — reusing `ObjectKind::Table` instead of a new
    `Custom("ClickHouseTable")` kind — was found by reading `identity::structural_score`'s actual
    comparison logic before designing: it already compares same-kind objects' `columns` property
    via Jaccard overlap, so reusing `Table` gets real cross-system identity resolution for free
    instead of needing the same blanket-exclusion treatment `Section`/`TransformNode`/etc. needed.
    The MCP tool's sync-to-async bridge reused RFC 0055's exact `Handle::try_current()` +
    `block_in_place` pattern (`ekos mcp serve`'s stdio loop runs inside `#[tokio::main]`, never
    spawned onto its own task) — recognized immediately from the prior devlog rather than
    rediscovered by a live panic.
  - *Test/Validate:* 60+ new tests across the new/touched crates (plugin: 6, dialect: 4, analyzer:
    4, clickhouse-query: 16, config: 2, mcp gating: 3, plus existing-suite regressions), all
    passing; full workspace `cargo build/test/clippy/fmt` clean, including the separate
    `benchmark/` and `tests/integration/` workspaces. **Live verification against a real
    ClickHouse instance done in a follow-up session** (the one acceptance criterion left open
    above): a dedicated `clickhouse/clickhouse-server:24-alpine` container, seeded with real
    tables/rows, exercised end to end — `build`/`recover`/`resolve`/`compile`/`commit` correctly
    compiled both tables with correct columns/evidence and made them `ekos query find`-able;
    `ekos clickhouse ask` (local Ollama `qwen2.5:1.5b`) answered single-table questions correctly
    against the seed data; a weak-model multi-table join hallucination was cleanly rejected by
    ClickHouse and surfaced as a pipeline error rather than a crash; the audit trail showed exactly
    one ledger Event/Evidence pair per successful query and none for failed ones; the MCP gate was
    confirmed in both directions over a real stdio JSON-RPC session (tool listed+callable with
    `enable-mcp-query = true`, absent+rejected-by-name otherwise). See
    `ekos/docs/rfcs/0056-clickhouse-connector.md`'s now-checked Acceptance Criteria.
  - *Explicitly not done, per the RFC's own Non-goals:* write access to ClickHouse; cross-source
    joins in one live query; result streaming/pagination beyond a `LIMIT` cap; a multi-turn
    clarification loop; automatic row-level ledgering; LLM-based business-meaning enrichment of
    ClickHouse table/column names (the `sql_analyzer.rs`-style optional second stage).

- [x] **RFC 0057 — ClickHouse Dialect: Preprocess `CODEC(...)` Before Parsing**
  - *What:* Found live while using EKOS to document a real, unmodified open-source repo's
    (`analytics/`, Plausible Analytics) ClickHouse component for a user: `ekos recover`'s
    `SqlAnalyzerPass`/`SqlTransformAnalyzerPass`, routed to the `"clickhouse"` dialect (RFC 0031),
    failed whole-file on `priv/ingest_repo/structure.sql` — `sql parser error: Expected: ',' or
    ')' after column definition, found: CODEC`. Ruled out a config/stale-binary explanation first
    (a genuinely separate, real bug: `target/release/ekos` predated RFC 0056's dialect
    registration, fixed by rebuilding) before confirming the real root cause by reading
    `sqlparser`'s own source: `CODEC(...)` (ClickHouse's per-column compression clause, used on
    most columns of most real MergeTree tables) has zero support anywhere in the pinned
    `sqlparser = "0.53"` — `MATERIALIZED`/`ALIAS`/`EPHEMERAL` are real ClickHouse-specific column
    options in `parse_optional_column_option`, but `CODEC` was never added, confirmed by a
    zero-hit grep across the whole vendored crate and cross-checked against the current published
    API docs (still no `Codec` variant on `ColumnOption`) — a real, still-open upstream gap, not
    something a version bump would fix.
  - *Output:* `ClickHouseDialectParser::preprocess` (`plugins/sql-dialect-clickhouse/src/lib.rs`)
    now strips well-formed `CODEC(...)` clauses — quote-aware (single-quoted strings with
    backslash-escape, backtick identifiers) and balanced-paren-aware (handles nested forms like
    `CODEC(ZSTD(3))` and multi-arg `CODEC(Delta(4), LZ4)`) — before the SQL reaches `sqlparser`,
    the same architectural slot `MySqlDialectParser` already uses for `DELIMITER` stripping. No new
    dependency (hand-written scanner, matching the `MySqlDialectParser` precedent rather than
    pulling in `regex` for one clause).
  - *Status:* Done for its stated scope. 11 tests (6 new preprocessing cases + a regression test
    using a real excerpt from `analytics/priv/ingest_repo/structure.sql`), full workspace
    `cargo build/test/clippy/fmt` clean. **Live-verified, partially**: rebuilding and rerunning
    `ekos recover` against the real `analytics/` repo confirmed the CODEC failure is gone — the
    reported error moved from line 7 (`CODEC`) to line 49 (`INDEX minmax_timestamp timestamp TYPE
    minmax GRANULARITY 1`, a table-level secondary-index definition — a separate, unrelated gap).
    **`structure.sql` still does not produce `Table`/`Column` KIR objects** — `sqlparser`'s
    `ClickHouseDialect` support for `CREATE TABLE` turned out to be narrower than one clause:
    `INDEX ... TYPE ... GRANULARITY`, `PARTITION BY` (confirmed gated to `BigQueryDialect |
    PostgreSqlDialect | GenericDialect`, ClickHouse excluded, `parser/mod.rs:6236`), and `SETTINGS`
    (no `CREATE TABLE` handling anywhere in the crate) are all still unsupported. Reported to the
    user rather than silently expanded into a much larger effort — see `ekos/docs/rfcs/0057-clickhouse-codec-preprocessing.md`'s Acceptance Criteria for the full accounting.
  - *Explicitly not done, per the RFC's own Non-goals:* forking/patching `sqlparser`; filing the
    real fix upstream (worth doing separately, not controlled by this codebase's timeline);
    modeling codec choice in the KIR (Stage 1's `properties["columns"]` never captured it either,
    even from live introspection); the `INDEX`/`PARTITION BY`/`SETTINGS` gaps found alongside this
    one, deliberately left for their own RFC(s) if the user wants that follow-on work.
    **Closed by RFC 0058 below, the same session.**

- [x] **RFC 0058 — ClickHouse Dialect: Preprocess `INDEX`/`PARTITION BY`/`SAMPLE BY`/`SETTINGS`/`CREATE DICTIONARY`**
  - *What:* User asked to close the `INDEX`/`PARTITION BY`/`SETTINGS` gaps RFC 0057 found and
    reported rather than fixed. Investigating fully before writing code (per the mandated
    workflow) turned up two more gaps in the same file, not named by the user but necessary to
    actually reach "the file parses": `SAMPLE BY` (`Keyword::SAMPLE` doesn't exist anywhere in
    `sqlparser`'s keyword table) and `CREATE DICTIONARY` (an entirely different statement type
    `sqlparser` has zero grammar for — confirmed by a zero-hit `DICTIONARY` grep across the whole
    crate). Folded both in rather than reporting yet another partial fix: `SqlAnalyzerPass` parses
    an entire file in one `Parser::parse_sql` call and discards every table in it if *any*
    statement anywhere fails — stopping at the three named gaps would still have left the real
    file (two `CREATE DICTIONARY` statements, `SAMPLE BY` on both event tables) unparseable, which
    is what "close the gap" actually means operationally.
  - *Output:* Four new functions in `plugins/sql-dialect-clickhouse/src/lib.rs`, chained after RFC
    0057's `strip_codec_clauses` in a new `preprocess_clickhouse_ddl` orchestrator:
    `strip_index_clauses` (removes `INDEX <name> <expr> TYPE <type> GRANULARITY <n>` from the
    column list, plus one adjacent comma so the list stays well-formed); `strip_keyword_expr_clause`
    (a single reusable primitive — not three copies — for `<keyword> [<keyword2>] <expr>` clauses
    terminated by a caller-supplied keyword list, applied to `PARTITION BY`, `SAMPLE BY`, and bare
    `SETTINGS`); `strip_create_dictionary_statements` (removes whole `CREATE DICTIONARY ... ;`
    statements — dictionaries were never modeled in the KIR even by RFC 0056 Stage 1's live
    introspection, so nothing already captured is lost).
  - *Status:* Done, and this time fully live-verified, not partially. 24 tests total in the crate
    (16 new), including the strongest regression available: the entire, unmodified real
    `analytics/priv/ingest_repo/structure.sql` embedded as a fixture
    (`plugins/sql-dialect-clickhouse/tests/fixtures/analytics-structure.sql`), asserted to parse
    into exactly 15 `CREATE TABLE` statements. Full workspace `cargo build/test/clippy/fmt` clean.
    **Live**: rebuilt `target/release/ekos`, reran the full pipeline against `analytics/`.
    `sql-analyzer` reported `objects=15 relationships=0` with zero parse warnings (previously
    `falling back to empty graph`). `ekos query find`/`query object` confirmed real
    `plausible_events_db.sessions_v2`/`events_v2` `Table` objects with all 43 real columns,
    correct types (`LowCardinality(FixedString(2))`, `Array(STRING)` for the `entry_meta.key`/
    `.value` pair, every `ALIAS` column), and 100%-confidence Evidence citing
    `priv/ingest_repo/structure.sql`.
  - *Explicitly not done, per the RFC's own Non-goals:* modeling `INDEX`/`PARTITION
    BY`/`SAMPLE BY`/`SETTINGS`/dictionaries in the KIR (same "nothing already captured is lost"
    reasoning as RFC 0057's `CODEC`); a general ClickHouse-DDL-completeness guarantee (this closes
    every gap the one real file that motivated it actually hits, not every possible ClickHouse
    construct); fixing the upstream `sqlparser` crate itself.

- [x] **`crates/identity`'s `DefaultResolver` over-merges real ClickHouse `imported_*` tables sharing a common base schema — confirmed not `Table`-specific (devlog_60), fixed by RFC 0060 (devlog_61)**
  - *Update, devlog_60:* A full cold whole-repo run against the same `analytics/` workspace shows
    the same 0.85-threshold mechanism hits `Person` and `Document` objects too, not just `Table` —
    a real contributor (`Niklaas Baudet von Gersdorff`, 1 real commit, confirmed via `git log
    --author` to be a genuinely different person from `Niklas Hambüchen`) was merged away and is
    now unfindable under his own name; separately, 27 unrelated documents (real project docs plus
    unrelated test fixtures) merged into one identity at confidence 0.98, the worst single proposal
    seen so far. RFC 0029's separately-coded cross-system resolver (`crates/identity/src/cross_system.rs`)
    independently produced the same class of false positive (`sessions_v2` SameAs `imported_visitors`),
    confirmed and rejected live via the `ekos_identity_review` MCP tool. This is now three
    confirmations of one underlying design gap (the 0.85 default threshold, no per-kind stricter
    override outside `Concept`), not three separate bugs. See `devlog_60.md`.
  - *What:* Found re-analyzing `analytics/` after RFC 0057/0058 fixed ClickHouse DDL parsing —
    the first time real ClickHouse `Table` objects with real `properties["columns"]` existed for
    `crates/identity` to compare. `ekos resolve` merged 6 genuinely distinct tables
    (`imported_visitors`, `imported_operating_systems`, `imported_exit_pages`,
    `imported_entry_pages`, `imported_devices`, `imported_browsers`) into one identity at
    confidence 0.93. Root cause read directly from `crates/identity/src/lib.rs`: `combined = 0.7 *
    name_similarity + 0.3 * structural_score` (`lib.rs:172`) against a default `merge_threshold` of
    `0.85` (`lib.rs:121`), no per-kind override for `Table` (only `Concept` gets `0.95`). All six
    tables share both a name prefix (`imported_*`, high Jaro-Winkler) and a common 8-column
    "spine" (`site_id, date, visitors, visits, visit_duration, bounces, import_id, pageviews`) —
    the *opposite* shape from `structural_score`'s own documented motivating case (`Employees` vs.
    `EmployeeTerritories`: near-zero column overlap despite name similarity). Confirmed real, not
    cosmetic: `ekos query object` on the surviving identity shows only `imported_visitors`'s own 8
    columns and 1 evidence entry — the other 5 tables' distinguishing columns
    (`operating_system`/`exit_page`/`entry_page`/`device`/`browser`) and evidence are gone from
    that identity entirely.
  - *Status:* **Fixed by RFC 0060** — `DEFAULT_MERGE_THRESHOLD` raised 0.85→0.90 (verified against
    17 real pairs read from `analytics/`, not guessed) plus `name_for_similarity` stripping
    `Table`'s schema/database qualifier before comparison (the qualifier was independently
    inflating Jaro-Winkler for every table in the same source). Live-verified: `ekos resolve`
    merge proposals on the real repo dropped from 19 to 8; `imported_browsers` and `Niklaas Baudet
    von Gersdorff` are both directly queryable again under their own names. **Not a complete fix**
    — 3 of the 17 known-wrong pairs and two `Document` over-merge clusters still incorrectly merge
    (documented honestly in the RFC and `DEFAULT_MERGE_THRESHOLD`'s doc comment); the residual
    cases are exactly the class of judgment call RFC 0029's cross-system review flow already
    exists for same-source merges don't yet have. See `ekos/docs/rfcs/0060-identity-resolution-merge-threshold.md`
    and `devlog_61.md`.
  - *Deck:* `docs/presentations/analytics-clickhouse-after.html` documents the original finding
    with real, unedited transcripts; `docs/presentations/analytics-full-loop.html` (devlog_60)
    documents the Person/Document/cross-system confirmations; the fix and live re-verification are
    in `devlog_61.md`.

- [x] **Postgres dialect: `sqlparser` fails whole-file on a real `IDENTITY ... INCREMENT BY` clause — fixed by RFC 0059 (devlog_61)**
  - *What:* Found running the full pipeline cold against `analytics/`'s actual Postgres application
    schema (`priv/repo/structure.sql`, not the ClickHouse `priv/ingest_repo/structure.sql` RFC
    0057/0058 already fixed) — `sql-analyzer` fails whole-file: `sql parser error: Expected: end of
    statement, found: INCREMENT at Line: 116`. `sql-transform-analyzer` degrades to per-statement
    fallback and maps only 1 of 1,282 statements (0.078% coverage). Unrelated to RFC 0057/0058 (a
    different dialect crate, a different clause) and not previously known — this is the first time
    this specific file was recovered against a rebuilt binary with a clean cache. Right now EKOS has
    no structured knowledge of this real repo's actual Postgres schema (`sites`, `api_keys`, and
    every other core application table).
  - *Status:* **Fixed by RFC 0059** — three preprocessing passes added to
    `PostgresDialectParser` (`plugins/sql-dialect-postgres`): whole-statement stripping for
    `CREATE`/`ALTER SEQUENCE` (a real, still-open upstream `sqlparser` clause-ordering bug, not a
    missing grammar rule; sequences were never modeled in the KIR anyway), and two
    information-preserving keyword/clause strips (`UNLOGGED`, `NOT VALID`) found investigating the
    same file, that keep the surrounding `CREATE TABLE`/`ALTER TABLE` statement's real content
    intact rather than dropping it wholesale. Live-verified: `sql-analyzer` now recovers 42 real
    `Table` objects from this file (was 0); `ekos query find "public.sites"` and `"api_keys"` now
    return real compiled `Table` objects. See
    `ekos/docs/rfcs/0059-postgres-sequence-and-ddl-preprocessing.md` and `devlog_61.md`.

- [x] **`ekos ask` retrieval is brittle to full-sentence natural-language phrasing — fixed by RFC 0061 (devlog_61)**
  - *What:* Found testing `ekos ask` against `analytics/`'s compiled ledger — `gather_context`
    (`crates/runtime/src/ai.rs:131`) passes the entire question string verbatim into
    `Runtime::find_objects()`, with no keyword extraction. Full-sentence questions ("Who are the top
    contributors to this repository by commit count?", "Who is Niklas Hambüchen and what did they
    contribute?", "What is Plausible Analytics and how does it track visitors without cookies?")
    consistently retrieved zero context, even though every underlying object is trivially findable
    via `ekos query find` or the `ekos_search` MCP tool using 2-3 keywords (MCP's own tool
    description for `ekos_search` already says so). Reformulating as bare names/keywords
    consistently succeeded.
  - *Status:* **Fixed by RFC 0061** — `AiRuntime::search_for_question` extracts keywords
    (stopword/punctuation-stripped, split on `_` matching FTS5's own tokenizer) and tries an AND
    query for precision, falling back to OR for recall, falling back to the original raw question
    as a last resort. Live-verified: "Who is Niklas Hambüchen and what did they contribute to this
    repository?" now correctly retrieves and answers from real evidence (previously empty).
    **Not fully fixed:** the related `README.md`-ambiguous-filename ranking issue found alongside
    this (a different root cause — relevance ranking, not phrase-escaping) remains open, and
    genuinely aggregate questions ("top contributors by commit count") still correctly retrieve
    nothing, since no keyword search can satisfy them — that class of question needs `ekos_ekl`,
    not `ekos_search`/`ask`. See `ekos/docs/rfcs/0061-ai-runtime-question-keyword-extraction.md`
    and `devlog_61.md`.

- [x] **Publish a first benchmark number — tokens vs. raw grep, real repo**
  - *What:* Every comparable tool in this space (codegraph, codebase-memory-mcp, codemap,
    CoreStory, GitNexus, Code Grapher, KiroGraph, Graphify, Aegis) leads with a published metric;
    EKOS had none. Built a real, reproducible benchmark against `analytics/` (the same real repo
    the full-loop case study uses): two real questions, answered from raw source (three grep
    tiers: best-case targeted, realistic repo-wide, naive full-file-read) vs. from the compiled
    ledger over real MCP calls (`ekos_search` + `ekos_state`, `ekos_ekl`), both sides counted with
    `tiktoken`'s `cl100k_base` encoding — a standard reference tokenizer, not a hand-rolled
    words/4 estimate.
  - *Result:* 67.5-93.4% fewer tokens than the realistic/naive grep tiers across both questions.
    **One honest exception, included rather than hidden:** the best-case tier (agent already knows
    the exact file and line before searching) costs 9.7× *fewer* tokens than EKOS — grep wins when
    it doesn't need to search at all, which isn't a realistic starting condition for "what does
    this table contain?" No comparison against any named competitor's own numbers is claimed —
    none were reproduced or available to test against.
  - *Output:* `docs/presentations/token-benchmark.html` (new deck, same format/convention as every
    other presentation in this repo), backed by real command transcripts, real MCP JSON-RPC
    responses, and the exact token-counting script used, all under
    `docs/presentations/examples/token-benchmark/`. Linked from the site hero (above the fold),
    the "Proven, not promised" stat-grid, `docs/presentations.html`, and `README.md`'s intro.
  - *Status:* Done for this pass. Explicitly scoped as "rough, real, and reproducible," not a
    leaderboard claim — see the deck's own §06 for what it does and doesn't claim (single repo,
    two questions, no latency measurement, no named-competitor comparison). More questions, more
    repos, and a latency benchmark are natural follow-ons, not attempted here.

- [x] **Ship the GitHub connector live, end to end — RFC 0062**
  - *What:* Item 3 of the roadmap: ship one connector "beyond Git/code" against live, real data,
    not a mock. No account for Salesforce/SAP/Oracle/Fabric/Snowflake/Confluence/Jira, but a real
    authenticated `gh` CLI token was available — `ekos/plugins/github`'s `GitHubApiClient` was
    already real HTTP code, just never run live. Fixed two real gaps before running (bare `#N`
    issue references with no closing keyword weren't detected; `GitHubApiClient` had zero
    pagination, silently capped at GitHub's 30-item default), then ran live against
    `github.com/plausible/analytics` (1,600 real issues/PRs, ~23 minutes). That live run
    immediately surfaced a third, far more severe gap — the same `crates/identity` over-merge
    class RFC 0060 fixed for `Table`/`Person`/`Document`/`Pipeline` — collapsing **96% of the real
    items (1,533 of 1,600) into a single identity at confidence 1.00**, because every
    `Custom("Issue")`/`Custom("PullRequest")` object shares the long, uninformative
    `"{owner}/{repo}#{number}: "` name prefix.
  - *Result:* All three fixed the same session. `name_for_similarity` (RFC 0060's extension point)
    given a third case for `Issue`/`PullRequest`; the catastrophic 1,533-object group is gone.
    **Not a complete fix** — 1,439 real merge groups remain post-fix (largest 174), and the
    RFC's own originally-chosen demo PR (#5158) is one of them, its real evidence lost under a
    surviving sibling identity — reported honestly rather than quietly swapped for a
    cleaner-looking example. The published deck's positive chain uses a different real PR (#6421)
    that survived standalone, plus a single real search (`"google analytics import"`) that
    surfaces real external docs, real code, and real GitHub items together without hand-curation.
  - *Output:* `docs/presentations/github-live-cross-system.html`, backed by real transcripts under
    `docs/presentations/examples/github-live-cross-system/`; two real `plausible.io/docs` pages
    vendored as a second observed source (`analytics-docs/`, RFC 0044 multi-project support, no
    new code). 10 new tests (+1 behavior-changed) across `github_analyzer.rs`, `plugins/github`,
    and `crates/identity`.
  - *Status:* Done for this pass. See `ekos/docs/rfcs/0062-github-live-cross-system-verification.md`
    and `devlog_63.md` for the full accounting, including what's still open (the residual
    over-merge, no rate-limit backoff, no request concurrency, full-URL references undetected).

- [x] **Fix real gaps found in an independent Claude+EKOS session transcript**
  - *What:* Analyzed a separate Claude Code session's transcript running EKOS live against the same
    `analytics/` repo. Two real, confirmed gaps: a UTF-8 char-boundary panic in
    `statement_repair.rs`'s `starts_with_keyword()` (the other session's own uncommitted fix kept,
    given a regression test); `ekos resolve` unconditionally hard-stopping on any identity conflict
    with no way to proceed (added `--force`, verified against the real 230-conflict set from the
    global multi-project workspace). A third suspected gap — Postgres `CREATE TYPE ... AS ENUM`
    parsing — was investigated and found **not** to be real: `sqlparser` 0.53.0 already parses it
    correctly, confirmed by running the real statement directly, not just reading source. No code
    change for that one.
  - *Result:* `ekos resolve --force` lets conflicts be printed without blocking `compile`/`commit`;
    the panic fix now has real regression coverage. The dual-ledger confusion (local per-project
    `.ekos/` vs. the global shared workspace connected MCP tools actually query, with
    `ekos_search`/`ekos_ekl` silently returning empty results when a project isn't in the global
    config) is real but reported, not fixed this pass — a bigger design question than the other two.
  - *Output:* `devlog_64.md` has the full accounting, including why the ENUM "gap" turned out to be
    a false alarm.
  - *Status:* Done for this pass. No RFC (both fixes are small, well-scoped changes to existing
    internal helpers/CLI flags, not new capabilities).

- [x] **`ai.rs::extract_citations` can't distinguish "cited nothing" from "nothing to cite"**
  - *What:* Found live-testing RFC 0046 against real OpenAI responses (devlog_46): a
    successfully-parsed but empty `cited_evidence` array produced the same empty-diagnostics shape
    as a genuinely well-cited answer.
  - *Output:* Fixed (devlog_65): new `AI002` diagnostic, distinct from `AI001` (missing/malformed
    block), emitted whenever zero citations survive filtering (empty array, or every id unknown).
  - *Status:* Done — unit-tested (`crates/runtime/src/ai.rs`), full workspace gate green.

- [x] **`gather_context` doesn't bound request size against broad/hub-like search terms**
  - *What:* Found live-testing RFC 0046 (devlog_46): `AiRuntime::gather_context` capped seed-match
    count and hop depth, but not the size of what a single hop pulls in — real
    `context_length_exceeded`/`rate_limit_exceeded` provider failures on broad/hub search terms.
  - *Output:* Fixed (devlog_65): `AiRuntimeConfig::max_context_chars` (default 200k, configurable
    via `[ai].max-context-chars`), `gather_context` stops admitting objects once the budget is
    crossed (always keeping at least one), surfaced as a new `AI003` diagnostic.
  - *Status:* Done — unit-tested (hub-object repro, truncation + budget-respected cases), full
    workspace gate green.

- [x] **Evidence citations show absolute filesystem paths, not repo-relative ones** — root-caused
  and fixed 2026-08-31.
  - *What:* Found rehearsing the RFC 0045 demo end-to-end (Playwright screenshots, real `/ask`
    calls): EKOS-self's citations render clean (`TODO.md`), but `fd`'s render as the full
    workspace path (e.g. `/tmp/claude-.../scratchpad/demo-repo-spike/fd/./src/error.rs` instead of
    `src/error.rs`).
  - *Root cause:* `crates/cli/src/commands/build.rs`'s `File`-object evidence construction built
    `SourceLocation::file` from `base.join(rel_str)` — an absolute path — instead of the plain
    relative `rel_str` every other evidence-producing analyzer (`local_docs_analyzer.rs` included)
    already used. `local_docs_analyzer` reprocesses Markdown/PDF/etc. with its own, correctly
    relative evidence, which is why EKOS-self's citations (mostly `.md` files) looked clean while
    `fd`'s (a Rust codebase — no equivalent reprocessing pass for `.rs` source) exposed the bug:
    the *only* evidence a plain source file had was the base observer's absolute-path one.
    Live-reproduced exactly, down to the reported `/./` (a tiny scratch workspace nested under
    `/tmp`, real `ekos_state` MCP call showed `"path": ".../tinyproj/./src/error.rs"`), then fixed
    and re-verified the same way (`"path": "src/error.rs"`). New regression test
    `file_object_evidence_location_is_relative_not_absolute` builds in a deeply-nested tempdir (the
    real repro shape) and asserts the evidence path is relative and doesn't contain the workspace's
    absolute root.
  - *Output:* Fixed. A citation for any plain source file (not just Markdown/PDF/etc.) now renders
    the clean repo-relative path. The bug applied equally to every workspace, including EKOS-self —
    it just went unnoticed there because the citations that particular rehearsal happened to show
    were for Markdown files, always covered by `local_docs_analyzer`'s separate, always-correct
    evidence. No other call site changed — the fix is one field at the single place the absolute
    path was actually constructed.

- [x] **Identity-resolution over-merge — real hits found against `ripgrep`/`bat`** — the
  `Technology`/`Crate` half fixed at its real source, RFC 0078 / `devlog_81`; the `RustSymbol`/
  `Crate` half honestly re-scoped, still open.
  - *What was actually wrong:* not `identity`'s resolver — `crate_topology_analyzer.rs` fabricated
    a duplicate `Custom("Technology")` object whenever a real internal crate (`ignore`, `pcre2`,
    `bat` itself) was also depended on elsewhere in the same workspace by a bare version string
    instead of `path`/`workspace = true`. The resolver's `SameNameDifferentKind` conflict report
    was a correct read of bad upstream data, not a resolver bug.
  - *Fix:* check the dependency name against every already-known internal crate name
    (`name_to_crate_id`) before fabricating a `Technology` — a real internal `DependsOn` edge
    instead. Verified with a fixture reproducing the exact real `ripgrep`/`bat` shape (an internal
    crate also version-depended-on elsewhere).
  - *Still open, honestly scoped, not guessed at:* the `RustSymbol`/`Crate` half of the same
    finding (a module/type inside a crate's own source sharing that crate's name — normal Rust
    convention, e.g. `pcre2`'s own `mod pcre2` — not a bug) has no existing relationship connecting
    a `Crate` to its own source `File`/`RustModule`/`RustSymbol` objects to structurally
    distinguish legitimate self-naming from a real coincidental collision (Component View, RFC
    0070, only matches `Crate`↔`Rollup` by path-string equality, not a graph relationship).
    Building that missing link is real, separate work.
  - *Test/Validate (remaining):* re-run the RFC 0045 bake against `ripgrep`/`bat` for real
    end-to-end confirmation (not done this session — no live GitHub bake performed, only the
    targeted unit reproduction); expect the `Technology`/`Crate` conflicts gone, the `RustSymbol`
    ones still present until the follow-on above is built.
  - *Note (devlog_65):* the same root-cause class had one more concrete, previously-`#[ignore]`d
    instance — PDF/DOCX-derived `Table` objects — fixed by giving `structural_score` a second real
    signal (`row_cell_tokens`) instead of falling back to its blanket `1.0` floor. The general
    `ripgrep`/`bat` crate/module-name case above is a different concrete manifestation and remains
    open.

- [x] **Reread every devlog for unimplemented gaps; fix the small/well-scoped ones**
  - *What:* User asked for a full reread of all 64 devlogs and a pass at closing whatever gaps
    remained. Compiled a complete list, sorted into small/well-scoped code fixes (fixed below),
    large/systemic design work needing its own RFC, and work blocked on external credentials this
    environment doesn't have.
  - *Fixed (devlog_65):* `OllamaProvider::from_env()` ignoring `[llm].model`; `ekos ask`'s ranking
    picking a same-basename fixture over the real file (bm25 content-length skew —
    `promote_exact_name_matches`); GitHub connector missing full-URL issue/PR references (only
    bare `#N` before); `ekos_transformation_diff` false-positiving on reordered join keys across
    Pentaho vs. SQL producers (`canonical_join_keys`) — plus the two `[ai]` fixes marked done
    above and the PDF-table identity fix noted above.
  - *Investigated, not a real gap:* a suspected multi-project ID-collision extension turned out to
    need a real cross-cutting artifact-schema change (project identity was never plumbed into
    recovery-pass artifacts at all, not just missing from a few analyzers) — re-scoped to Category
    2 (needs its own RFC) rather than forced through; see devlog_65's "Not fixed" section.
  - *Also found, not fixed:* `analytics/`'s local ledger has a corrupted FTS5 index (base DB passes
    `PRAGMA integrity_check`; the virtual table doesn't) — real, physical evidence for the
    storage write-barrier/concurrency gap below ("Storage architecture"), not touched destructively.
  - *Status:* Done for this pass. See `devlog_65.md`.

- [ ] **`examples/` — at least one runnable example per crate**
  - *What:* Each crate under `crates/` has at least one file in its `examples/` directory that
    demonstrates the primary use case with real (non-mock) objects. Examples must compile and run
    to completion with `cargo run --example <name> -p <crate>` producing non-empty output.
  - *Output:* `crates/*/examples/*.rs`; all examples pass `cargo run`.
  - *Test/Validate:* CI step: `for each crate, cargo run --example <primary-example> -p <crate>`
    exits 0. Broken examples block merge.

- [x] **Storage v2→v3 default switch — RFC 0016's own Phase 6, completed (devlog_67)**
  - *What:* Asked for a v2→v3 deprecation timeline to publish; investigation found RFC 0016's fact
    engine was fully implemented (not design-only, correcting a stale memory record) and had
    already soaked a real month on a live multi-project estate — exactly the condition RFC 0016's
    own text set for flipping the default. Shipped the switch itself instead of a timeline for it:
    `open_store()` now defaults a **genuinely fresh** workspace to the fact engine; any
    **pre-existing** SQLite workspace (this repo's own, `analytics/`) is completely unaffected.
  - *Output:* Verified live end to end with a real disposable workspace. See
    `docs/rfcs/0016-fact-segment-engine.md`'s new dated section and `devlog_67.md`.
  - *Status:* Done. Full workspace gate green across all three of this repo's `cargo test`
    surfaces (`ekos/`, `tests/integration/`, `benchmark/` build-checked) — a real regression was
    caught and fixed in two of them (hardcoded `Ledger::open` call sites bypassing the new
    auto-detection), see the devlog's own account.

- [ ] **Storage architecture: six real gaps — plan saved (RFC 0080), Phases 1-3 of 6 implemented**
  - *What:* A real, technically-grounded plan now exists (RFC 0080) — each of the six sub-gaps
    checked against the actual current implementation (not just this summary) and correctly
    attributed to the backend it affects, sequenced by real urgency/dependency:
    - [x] *Phase 1 (highest priority, real live evidence) — concurrency — RFC 0104 / `devlog_121`
      (2026-08-26).* Two distinct real gaps, one per backend, both closed. SQLite `Ledger`'s
      `append`/`append_object`/`append_relationship` (previously 2-4 unwrapped statements each —
      the likely real mechanism behind the corrupted FTS5 table `devlog_65` found live in
      `analytics/`'s ledger) now run inside real `BEGIN IMMEDIATE`/`COMMIT` transactions
      (`in_transaction`); resolved without a supplementary explicit lock — SQLite's own WAL-mode
      locking under `BEGIN IMMEDIATE` already provides real cross-process protection. `FactLedger`
      gets a real, designed `write.lock` file (`fs4`, the same `flock`(2) mechanism tantivy's own
      `IndexWriter` lock already used incidentally, now promoted to a direct dependency and
      acquired first, before `SegmentStore`/`SearchIndex` are touched) — a second writable process
      now fails with a clear `LedgerError::Locked` instead of an eventual tantivy-internal error,
      live-verified with two real racing `ekos commit` processes. **CI flake found and fixed
      2026-08-31**: `acquire_write_lock` originally failed on the very first `try_lock_exclusive`
      attempt (no retry) — proven live under `--test-threads=4` load (traced with acquire/release
      timestamps + thread ids) to sometimes report the lock still held for a few milliseconds
      *after* the previous holder's `File` had already closed, on the same thread, sequentially,
      with nothing else able to run in between — a kernel-level flock scheduling artifact under
      heavy concurrent load, not a leaked handle. A same-thread 200-iteration `build → open` loop
      run alone never failed once; the identical loop run four-wide alongside itself failed within
      tens of iterations, consistently. Fixed with a short bounded retry (≤20 attempts, 5ms apart,
      ≤100ms worst case) — 19/19 stress runs clean afterward vs. 2/8 failing before. A genuine
      second writer is still correctly rejected, just up to ~100ms slower to report. RFC 0016's
      "the manifest lock enforces it" text corrected. The concurrent-read visibility spec turned
      out to be a real, previously-unverified gap: a `FactLedger` handle's view is frozen as of its
      own `open()` call, not automatically refreshed by a separate process's writes — proven with a
      dedicated regression test (`a_long_lived_handle_does_not_see_a_separate_handles_writes_
      until_reopened`), not just documented as an inherited claim. 7 new `ekos-ledger` tests, full
      workspace gate clean, `tests/integration` 3/3.
    - [x] *Phase 2 — WAL recognition + repair tool — RFC 0105 / `devlog_122` (2026-08-26).*
      Confirmed no new WAL needed building — `FactLedger`'s existing segment format (checksummed
      frames, atomic manifest writes) already provides real ledger-level WAL durability; the real
      gap was that nothing surfaced it. New `SegmentStore::verify_sealed_report` checks every
      sealed segment unconditionally (existing `verify_sealed` refactored on top of it, not
      duplicated). New `ekos ledger repair` CLI command opens the ledger (triggering its two
      already-existing free self-heals: torn active-segment tail truncation, stale index-runs
      rebuild), then reports one line per sealed segment — replacing the previously-accurate "the
      only recovery option is a full migration rollback" with a real, precise diagnostic (which
      segment, which transaction range) rather than an automatic fix for the one case
      (genuine bit-rot in a sealed segment) that has no synthesizable fix at all. FactLedger-only,
      matching every prior phase's precedent. 6 new tests (2 `ekos-ledger`, 4 `ekos` CLI), full
      workspace gate clean, `tests/integration` 3/3. Live-verified through the real `ekos` binary
      (the honest "no sealed segments yet" path against a real pipeline-built scratch workspace;
      the corruption-report path verified with real segment files, real on-disk byte corruption,
      and the real `repair()` function via a tiny seal threshold to force real sealed segments
      quickly).
    - [x] *Phase 3 — version-chain checkpoints — RFC 0106 / `devlog_123` (2026-08-26).* Built as a
      pure, purely-additive acceleration structure — periodic per-entity checkpoints
      (`checkpoints.jsonl`, one every 20 versions) let `state_at` (the shared engine behind
      `object_at`/`current_sig`/every point-in-time read) seed its fold from the nearest prior
      checkpoint instead of always genesis, provably equivalent to full replay by construction
      (never consulted for correctness, only speed — a missing/corrupt checkpoint just means a
      slower, still 100% correct fold; a dedicated test appends literal garbage as a trailing
      checkpoint line and confirms reads stay correct). Honest scope check done before shipping,
      not after: `FactIndexes`' EAVT key order means the underlying index scan itself can't be
      tx-bounded cheaply, so the real win is in the fold cost, not scan I/O. 3 new tests, full
      workspace gate clean, `tests/integration` 3/3. Live-verified through the real CLI: 25 real
      revisions of the same real workspace, `checkpoints.jsonl` written for real, `ekos query
      object`/`ekos diff` both correct across the checkpoint boundary.
    - **Real finding from Phase 3, changes how Phase 4 must be scoped — not scheduled until
      resolved**: Phase 4 as originally named means discarding old delta history, directly
      conflicting with `CLAUDE.md`'s own Key Invariant that the ledger is append-only with no
      object-level delete/tombstone mechanism anywhere (deliberate, reviewed, not an oversight).
      Phase 3's checkpoints were deliberately built to *not* need this resolved (purely additive).
      Needs an explicit decision from the user before any Phase 4 design starts: relax the
      invariant (a real, load-bearing architectural change), or re-scope Phase 4 to something that
      doesn't require it (e.g. archival to a separate location rather than in-place deletion — not
      investigated).
    - *Phase 4:* retention/pruning policy — blocked on the finding above, not just a sequencing
      dependency on Phase 3 anymore.
    - **Storage plan paused here by explicit user decision (2026-08-26)**, not for lack of further
      scoped work: asked directly whether to (a) stop, (b) re-scope Phase 4 around archival instead
      of deletion, or (c) discuss relaxing the append-only invariant — chose (a). Phases 1-3 are
      complete, real, shipped increments needing nothing further. Phase 5 still needs its own
      real query-log scoping pass before it's implementation-ready regardless; Phase 6 stays
      blocked on RFC 0111's Phase A (below). Resume at Phase 4 (with a real decision on the
      invariant question above) or Phase 5 (starting with the query-log scoping pass) whenever this
      work picks back up.
    - *Phase 5:* materialized views alongside the EAV fact engine — least-scoped so far, needs a
      pass over real EKL/MCP query logs to find what's actually worth materializing. **Groundwork
      landed 2026-08-31 (RFC 0114)**: that prerequisite didn't exist — the only real query log
      anywhere was RFC 0056's ClickHouse audit trail, scoped to that one live-external-system tool;
      `ekos_ekl` and the other 13 read-only MCP tools had zero persisted call history. New
      `crates/cli/src/commands/query_log.rs` appends one JSON line per call to
      `<workspace>/.ekos/query-log.jsonl` — deliberately **not** RFC 0056's ledger-based
      Evidence/Event pattern (usage telemetry isn't evidence, and a writable `FactLedger` open per
      call would reintroduce the exact lock-contention/latency regression RFC 0097 fixed for the
      13 tools that go through `StoreCache`'s read-only cache). A static, pre-execution heuristic
      (`classify_ekl`/`classify_tool`) flags each call `Cheap`/`Expensive` from its own arguments
      (EKL predicates/LIMIT, `depth`/`max_hops`, diff window size) purely to gate an opportunistic
      **result cache** added to `StoreCache` — an `Expensive` call with identical arguments is
      served from cache while the store's fingerprint hasn't changed, `ekos_clickhouse_query`
      excluded (a live external system the fingerprint knows nothing about). The heuristic doesn't
      have to be accurate for the log to be useful: every call's real measured `duration_ms` is
      recorded regardless of its guessed class — that measured number, not `cost_class`, is what
      the real Phase 5 scoping pass will eventually use. Found and fixed a real bug while wiring
      cache invalidation: the first version only cleared the cache inside `StoreCache::get`, which
      a cache-hit call never reaches — a `refresh()` fingerprint check now runs unconditionally
      before every cache lookup. Test `expensive_tool_call_is_served_from_a_poisoned_cache_when_present`
      deliberately poisons the cache to prove it's actually consulted (not silently bypassed), the
      same technique the RFC 0113 gateway-pruning test used. Live-verified through the real
      `ekos mcp serve`/`ekos ekl` binaries against a real workspace. Phase 5's actual
      materialized-view design still waits for real accumulated log data — this only makes that
      data start existing.
    - *Phase 6:* horizontal distribution — RFC 0034 (single-machine partitioning) and RFC 0110
      (horizontal distribution) were **merged 2026-08-27 into RFC 0111** (Under Review), one
      conformed design, per explicit user direction — both source RFCs are now Withdrawn, kept on
      disk as historical record. RFC 0111 Phase A = single-machine partitioning (RFC 0034's original
      scope: configurable partition dimension, time-bucket tiering, `entity_id → Set<PartitionId>`
      correctness built into the base design). RFC 0111 Phase B = this Phase 6 (RFC 0110's original
      scope: object storage (S3/ADLS Gen2, via `object_store`) as the ledger's single durable copy,
      three services around it — a distributed MPP compile/ingest cluster, a distributed MPP query
      cluster of stateless workers running the existing EAVT/AEVT/AVET fold against cached
      partitions, a single logical load-balanced query gateway — plus distributed search with an
      explicit cross-shard BM25 caveat). **Phase B's dated implementation RFC is RFC 0113** (Draft,
      2026-08-29) — sequences §4/§6/§7 into B1 (`SegmentBackend` seam) → B2 (`ObjectStoreBackend`) →
      B3 (coordinator + Service A) → B4 (Service B/C + `DistributedLedger`) → B5 (distributed
      search). **B1 landed 2026-08-29** — `SegmentBackend` trait + `LocalFsBackend`
      (`crates/ledger/src/backend.rs`), `SegmentStore` routes sealed publish/fetch through it,
      zero behaviour change (all 139 prior ledger tests green). Accept RFC 0113 before B3 (the
      first sub-phase with a network service). **B2 landed 2026-08-29** — `crates/segment-backend`
      extracted (`SegmentBackend` + `LocalFsBackend` + `MemBackend` + `BackendError`, `get`/`get_range`);
      `ObjectStoreBackend` behind the `object-store` feature (`object_store` 0.14, dedicated
      current-thread runtime); `SegmentStore` round-trips on object storage with the cache wiped
      mid-test; a Local `cargo build` never compiles `object_store` (dev-dep only). **B3 landed
      2026-08-29** — `crates/cluster` (`ekos-cluster`): `Coordinator` (catalog + write leases +
      fencing tokens + per-partition tx watermarks + entity→partitions index, persisted as one
      atomic JSON file; leases not persisted, TTL-bounded), `serve` over newline-delimited JSON-RPC
      on TCP (the `ekos mcp serve` pattern — chose it over tonic/gRPC: no protobuf toolchain, small
      request/response only, no segment bytes cross the coordinator), `CoordinatorClient`,
      `CompileWorker`/`LeaseGuard` (Service A's transport+lifecycle half — lease→heartbeat→fenced
      `commit`→release). `ekos coordinator serve`/`status` + `ekos compile-worker run` (thin CLI
      wrappers). Harness (`crates/cluster/tests/harness.rs`): disjoint-shard concurrent commit,
      lease contention (one winner, loser gets "already leased"), expired-lease fencing (stale
      `manifest_commit` rejected, next worker resumes from the committed watermark, no partial/lost
      write), coordinator-restart durability. mTLS deferred (trusted cluster network for v1).
      Binding a lease to a real shard-scoped `build → commit` is B4. **B4 landed 2026-08-30** —
      `crates/distributed` (`ekos-distributed`): `QueryWorker` (Service B) materialises a partition
      on demand (object storage → bounded local cache via `ObjectStoreBackend::from_url`, or a
      co-located local dir used in place), opens it read-only as a `FactLedger`, and serves every
      `KnowledgeStore` read for it over NDJSON/TCP (`spawn_blocking` around every ledger call);
      `ekos query-worker serve`. `DistributedLedger` (Service C) — `impl KnowledgeStore`, fans every
      read across the workers named by the coordinator catalog and merges (newest-partition-wins /
      concat-oldest-first / `PartitionedLedger`'s own `diff` merge), classifies partitions by id
      prefix; `append_*`/`vacuum_into` rejected (`LedgerError::ReadOnly`); never owns a tokio
      `Runtime` (would panic dropped under `#[tokio::main]`). `[storage.distributed]` branch in
      `open_store`/`open_store_read_only`/`store_display`; `StorageDistributedConfig` in
      compiler-core. object_store stays behind `ekos-distributed/object-store` (cli `distributed`
      feature) — stock `cargo build --workspace` still never compiles it. Tests: query-worker reads
      == direct `PartitionedLedger` reads; gateway over 2 workers == in-process `PartitionedLedger`.
      v1 follow-ons: persistent connection pool, parallel fan-out, coordinator-index pruning, a
      command to register an existing Local partitioned workspace with a coordinator. **B5 landed
      2026-08-30** — `FactLedger::find_objects_scored(query, limit)` exposes the BM25 score
      (`find_objects` delegates, behaviour unchanged); `DistributedLedger::search(query, k)` fans
      each object partition's local top-`k` to a worker, merge-sorts by shard-local score, dedups
      by id, truncates to `k`; the `find_objects` trait method rides on it. Scores are shard-local
      — the accepted query-then-fetch approximation (RFC 0111 §7), not a corpus-global ranking; the
      test asserts that caveat explicitly. **Service A real-pipeline binding landed 2026-08-30** —
      `ekos compile-worker run --coordinator <addr> --shard <name> --workspace <dir>` acquires the
      shard lease (heartbeated), runs the real `build → recover → resolve → compile → commit` on a
      blocking thread with its own runtime (executor stays free to heartbeat through a multi-minute
      compile), then registers every partition it wrote (`CatalogRegister` + `RecordEntityPartitions`)
      and `manifest_commit`s the store's monotonic entry count as the generation watermark (fenced).
      Requires a Local `[storage.partition]` workspace (not `[storage.distributed]`); partition roots
      must be on storage the query workers can also reach (shared FS for now). Integration test:
      coordinator + `compile_worker_run` over the ecommerce SQL fixture → 6 Table partitions
      registered, watermark advanced, store really holds the 6 tables. **RFC 0113 Phase B (B1–B5 +
      Service A) is feature-complete at v1 scope**; remaining = gateway v1→v1.1 polish (connection
      pool, parallel fan-out, index pruning). **Partition sealed segments through `SegmentBackend`
      landed 2026-08-30** — `FactLedger::open_with_backend` / `open_read_only_with_backend` +
      `PartitionedLedger::with_segment_backend(resolver)`; `[storage.partition] segment-backend-url
      = "s3://…"` builds an `ObjectStoreBackend` per partition (cli `distributed` feature), so a
      partition's bulk (8 MB sealed segments) lives in object storage while manifest/HEAD/dict/
      search/active stay on the local root. `compile-worker` registers `PartitionLocation::ObjectStore`
      for each partition when the URL is set. **Manifest publishing landed 2026-08-30** —
      `manifest.json` + `dict.bin` now route through the `SegmentBackend` too (new
      `SegmentBackend::publish` for overwriteable metadata, impls on LocalFs/ObjectStore/Mem);
      `SegmentStore` loads them via `exists`/`get`. **A partition is now self-describing in object
      storage** — only `HEAD` + the active segment stay local (writer-only crash-recovery). Test:
      `FactLedger` write, wipe local `manifest.json`/`dict.bin`/`HEAD`/`segments/`, reopen
      read-only → all objects still read from the backend. **`search/` publishing landed
      2026-08-31** — `SegmentStore::publish_aux(rel)`/`fetch_aux(rel)` generically push/pull a flat
      directory's files through the `SegmentBackend` under the same `<rel>/…` keys;
      `FactLedger::open_read_only_with_backend` calls `fetch_aux("search")` when no local `search/`
      exists (an unsynced partition degrades to zero search hits rather than erroring — every other
      read still works); `FactLedger::sync_search_to_backend()` publishes it, and
      `PartitionedLedger::publish_search_indexes()` does so for every catalogued partition;
      `ekos compile-worker run` calls it after each compile, before registering partitions with the
      coordinator. Test: writer publishes a search index through a `MemBackend`, a brand-new reader
      root with nothing local resolves `find_objects` from the backend-fetched index; a second
      reader with no published index still reads objects but gets zero search hits. **Gateway
      v1 → v1.1 landed 2026-08-31** — `DistributedLedger` now pools one connection per coordinator/
      worker address (`ConnSlot`, reconnect-and-retry-once on an I/O error) instead of connecting
      fresh per call; every multi-partition fan-out dispatches concurrently
      (`futures::future::join_all`/`try_join_all` via new `fan_out`/`first_present` helpers)
      instead of sequentially, preserving each method's original merge order; id-scoped reads
      (`get_object`, `object_history`, …) prune to the partitions the coordinator's real
      `entity_id → partitions` index names for an id, falling back to a full class scan only when
      the index has nothing (events/evidence, or a not-yet-recompiled workspace). New
      `PartitionedLedger::partition_entity_ids(key)` lets `ekos compile-worker run` populate that
      index from each partition's actual object/relationship ids — replacing a pre-existing bug
      where it had instead recorded the *shard name* mapped to every partition it produced, a
      placeholder with zero pruning value. A dedicated test
      (`gateway_uses_the_entity_index_to_prune_when_present`) proves pruning is real, not a
      silent fallback, by mis-registering an id against the wrong partition and asserting the
      lookup misses. Fixing the placeholder also surfaced a second, unrelated latent bug caught by
      the existing integration test: its watermark assertion checked
      `watermark(catalog[0].id)` (a physical partition id) when watermarks are actually tracked
      per lease/shard name — always `0` under a partition id, and only ever true by coincidence via
      the `||` against the placeholder entity-index check now removed; both the assertion and the
      underlying index are fixed. **RFC 0113 Phase B is now fully closed at v1 scope** — no
      tracked follow-ons remain.
      **Hardening pass 2026-09-01 (devlog_144, branch `fix/distributed-storage-issues`)** — two
      autonomous end-to-end runs (a Pentaho workspace on `file://`, then the 95-partition
      Plausible/Elixir workspace on a real MinIO container with OpenAI enrichment) found and fixed
      8 defects the unit tests never hit: (1) `ObjectStoreBackend` panicked when built/dropped
      inside `#[tokio::main]` — now runs its `object_store` calls on a dedicated OS-thread runtime
      (`DedicatedRt`), safe from any context; (1b) `parse_url` read no config so `s3://` never
      authenticated to MinIO — now forwards `AWS_*`/`AZURE_*` env vars via `parse_url_opts`, and
      the `object-store` feature bundles `object_store/aws`+`azure`; (2) a dead query worker failed
      every gateway read — new `DistributedLedger::call_worker_failover` rotates to another worker;
      (3) `ekos coordinator status` always showed watermark 0 — new `Request::Watermarks` + a
      shard/generation section; (4) `ekos diff` printed opaque `entry #N` — now resolves touched
      ids to names/kinds; (5) `[llm-description]` sent an OpenAI key to Anthropic — added the
      `openai` branch to `select_llm_provider_for_description`; (6) an unsealed partition
      (i.e. almost every partition under entity-kind partitioning) published only an empty
      `manifest.json` — new `SegmentStore::publish_active` / `PartitionedLedger::publish_active_segments`,
      called by `ekos compile-worker`, plus `open_with_backend` pulls the active segment when
      local is absent; (7) `CompileWorker`'s heartbeat was a fixed 10s regardless of TTL — now
      derived from `lease.expires_at`; (7b) `CoordinatorClient`/`QueryWorkerClient` `call` used
      separate write/read mutexes so concurrent callers on one connection crossed frames — now
      holds the write lock across the round-trip. Also `ekos compile-worker run --force`
      (Service-A equivalent of `ekos resolve --force`). Open follow-ons noted: interrupt-in-flight
      on lease loss; a built-in acquire-retry loop in `ekos compile-worker`; the
      document-semantics analyzer's free-form relationship vocabulary (one partition per bare
      preposition).
    - *Phase A progress (2026-08-29):* being built incrementally against RFC 0111
      directly (that RFC doubles as the Phase A impl RFC, per user direction). Landed:
      `crates/ledger/src/partitioned.rs` — `PartitionedLedger` with all three `PartitionDimension`s
      routing (`SourceScope`/`Composite` via a `with_source_resolver` closure — `KirObject` has no
      source field yet; `UnresolvedSource` on a missing source, never a misroute), configurable
      `TimeBucket` (Daily/Weekly/Monthly), catalog-recorded dimension/bucket with a
      `DimensionMismatch` guard on reopen, `entity_id → Set<PartitionKey>` fan-out (§2), pruned
      scoped reads (`objects_in_kind`, §1), genuine concurrent multi-partition writers
      (`Arc<FactLedger>` per partition), a **persisted `PartitionCatalog`** (`catalog.json`, atomic
      temp+rename, §5), and a **persisted AEVT-style entity index** (`entity-index/run-*.jsonl` —
      append-only pair lines, `merge_runs`-style compaction at `COMPACT_AT`, self-healing scan only
      for ids absent from the index, `rebuild_entity_index()` repair path) so a reopened ledger
      resolves any object/relationship with zero partition scans; **relationships** (RFC 0111
      amendment 2026-08-29 — routed by `"rel:"+kind`; unified `index/run-*.jsonl` `{k,id,p}` lines
      for obj/rel/endpoint/evt/evid; `relationships_for` pruned via the endpoint index, not fanned
      out); **events + evidence** (own `"events"`/`"evidence"` partitions), **point-in-time**
      (`object_at`/`all_objects_at`/`relationships_at`/`all_relationships_at`), **full-text search**
      (hot object partitions, per-partition BM25 merge, cold skipped), **`diff`** (merged
      `LedgerDiff`), **`vacuum_into`** (self-contained copy) — so **`impl KnowledgeStore for
      PartitionedLedger`**, a drop-in for `FactLedger`, tested through `Box<dyn KnowledgeStore>`;
      **cold tiering** (`Tier::Cold`, `mark_cold_before(cutoff)` demotes past-bucket partitions +
      evicts handles, any read rehydrates — RFC §3 policy layer); `compiler-core`
      `[storage.partition]` config parsing; and the **`open_store` wiring** — `open_store` /
      `open_store_read_only` build a `PartitionedLedger` (`.read_only()` opens partitions via
      `FactLedger::open_read_only`) for a fresh workspace opting into `[storage.partition]`
      (`entity-kind` only for now — source-scope/composite need a `KirObject` source field);
      existing SQLite/fact workspaces untouched. **Phase A (Local mode) is functionally complete
      for `entity-kind`.** Remaining: source-scope/composite wiring, per-scope bucket overrides,
      the §3 search-index-drop half of cold tiering. Phase B (distribution) now has its own
      implementation RFC (0113) — start at B1 (`SegmentBackend` seam) once it's Accepted. The
      module is now a `partitioned/` submodule (`mod.rs` routing/index core + `types.rs` +
      `knowledge_store.rs` + `tests.rs`); `mod.rs` is still ~1.4k lines — the `impl PartitionedLedger`
      read/write methods could move to a `reads.rs` if it grows further. See RFC 0111 amendment §4
      + RFC 0113.
  - *Why it matters now, not just eventually:* `devlog_65` found real, physical evidence this is
    already biting — `analytics/`'s local ledger has a corrupted FTS5 virtual table (base DB
    passes `PRAGMA integrity_check`, the FTS index doesn't), now traced to a specific, real,
    plausible mechanism (Phase 1, above) rather than just "concurrency, generally."
  - *Test/Validate:* each phase still needs its own dated implementation RFC before any code, per
    the mandatory workflow — RFC 0080 is the saved plan; Phase 1 is the first phase to graduate to
    a real implementation RFC (0104). Phases 2-5 remain to be scoped and implemented the same way;
    Phase 6 stays explicitly blocked on RFC 0111's Phase A shipping first.

- [x] **Positioning: separate the technical pitch from token materials — README (devlog_68)**
  - *What:* Real risk: README ran three token/crypto headers (raw contract address, pump.fun
    trading link, founder vesting wallet address) back to back near the bottom, duplicating facts
    already canonical in `TOKENOMICS.md`. `docs/index.html` already had the right restrained
    pattern — README didn't match it.
  - *Output:* Consolidated into one `## Token & Community` section (same position, not made more
    prominent), no raw address/pump.fun link inline in README anymore — single link to
    `TOKENOMICS.md` as the source of truth. Moved the one genuinely new fact (vesting wallet
    address) into `TOKENOMICS.md`'s existing vesting section. Deliberately did **not** delete the
    token/contract-address information from public materials — it's also the anti-impersonation-
    scam reference point; the fix was de-duplication and restrained placement, not deletion.
  - *Status:* Done. See `devlog_68.md`.

- [ ] **Positioning: research paper vs. startup track sequencing**
  - *What:* Decide whether/when to pursue an arXiv technical report (problem statement, method,
    baseline comparison against grep/RAG/codegraph, benchmark numbers, honest limitations section)
    alongside continued product development. Not blocking — a published report adds credibility
    and discoverability but doesn't block shipping.
  - *Test/Validate:* a decision, not a build — resolved once Priority-1-equivalent proof points
    (real cold runs, a published benchmark, a live connector) exist to write about, which they now
    do (`analytics-full-loop`, `token-benchmark`, `github-live-cross-system` decks).

### Promoted from RFC Non-Goals (2026-08-21 survey)

A full read-through of every RFC in `docs/rfcs/` (0001-0024) and `ekos/docs/rfcs/` (0025-0062)
found ~40 Non-Goals items that describe genuine deferred work, not permanent scope boundaries —
previously untracked anywhere as backlog. Each RFC's own Non-Goals section carries a one-line
forward-pointer to its entry here; this section is the tracking home, the RFC stays the source of
truth for *why* it was deferred. Items already fulfilled by a later RFC, or already fixed
(RFC 0061's README ranking gap, RFC 0062's full-URL reference gap — both closed in `devlog_65`),
are excluded — see the full exclusion list in the planning history if needed.

- [x] **Runtime/retrieval — 6 of 6 closed (2026-08-26), see the full six-RFC sequence under
  `docs/GAP_ANALYSIS.md` gap-closure plan below** for real implementation detail: EKL `AS OF
  <timestamp>` + `COUNT`/`GROUP BY` (RFC 0096); MCP-scoped read-only ledger caching, `StoreCache`
  (RFC 0097, after an unsafe first attempt was caught and reverted); `ekos ask --stream` (RFC 0098);
  multi-turn `ekos ask --session` history (RFC 0099); `memory/`-path search boost (RFC 0101);
  embedding/semantic search — redesigned by explicit user direction into indexing RFC 0088's
  existing `ai_overview`/`ai_usage` prose instead of new vector infrastructure (RFC 0100). **Async
  `KnowledgeStore`/`Runtime` methods stay deliberately excluded, not closed** — RFC 0005's original
  sync-by-design decision was re-confirmed correct (100% sync, both backends, 33 real call-site
  files), a trade-off not a gap; revisit only if a concrete future consumer (e.g. an async MCP
  transport) needs it. EKL Object+Relationship `JOIN` in one query also stays open — found live to
  be the one extension that actually breaks EKL's flat-clause-type design, deferred as its own
  future RFC.

- [x] **MCP TCP transport** — RFC 0115 (2026-08-31). `ekos mcp serve --tcp <addr>` accepts NDJSON
  JSON-RPC 2.0 connections over plain TCP (the same pattern as `coordinator serve`/`query-worker
  serve`, RFC 0113 B3/B4) alongside the original RFC 0013 stdio transport, which is unchanged and
  stays the default. Lets more than one MCP-speaking tool (PyCharm's AI chat, another agent host, a
  second Claude Code session) connect to one already-running server instead of each needing its own
  spawned `ekos mcp serve` process. `handle_message`'s dispatch core was already transport-agnostic;
  a new shared `serve_messages` loop and `serve_tcp` (one `std::thread::spawn`'d OS thread per
  connection, matching `handle_message`'s own blocking design) are the only new code. Each
  connection gets its **own** `StoreCache` rather than one shared across connections — sharing was
  the original plan but requires `KnowledgeStore: Send`, which the trait doesn't declare and no
  implementor (`Ledger`, `FactLedger`, `PartitionedLedger`, `DistributedLedger`) has been audited
  for; not worth doing as a side effect of a transport RFC. No authentication/TLS — opt-in only,
  loopback/trusted-network use only, same v1 posture RFC 0113's own TCP servers already have.

- [x] **Top-level `ekos status` CLI alias** — RFC 0116 (2026-08-31). `ekos status [--storage]`
  dispatches to the exact same `ledger::status` function `ekos ledger status` already calls — added
  after a real VS Code AI chat, connected over the new MCP TCP transport, recommended running
  `ekos status` (guessing from the `ekos_status` MCP tool name) and hit "unrecognized subcommand."
  `ekos ledger status` is unchanged and stays supported; no relationship-count parity with the MCP
  tool attempted (explicitly declined scope).

- [x] **dbt project metadata analyzer** — RFC 0117 (2026-08-31). `ekos recover` now extracts real
  `Table` objects from a dbt project's own checked-in files — `models/**/*.sql` (one model per
  file, regardless of YAML documentation) and `sources[].tables[]` YAML entries (no `.sql` file
  backs a source) — with `ref()`/`source()` macro calls becoming real `DependsOn` edges. Static
  only: no live warehouse connection, no `manifest.json`/`catalog.json` (both confirmed gitignored
  `dbt/target/` build artifacts on a real project) — dbt itself can point at any database, so the
  only stable, version-controlled source of truth is dbt's own project files. `ObjectKind::Table`
  used deliberately, not a new `Custom(_)` kind, so `DefaultResolver`'s real column-Jaccard scoring
  can fuse a dbt-derived table with an independently-discovered DDL table of the same name — the
  same identity-resolution pattern the SQLAlchemy-ORM-to-`Table` precedent (RFC 0091) already
  established. Live-verified end to end against a real Databricks/dbt project (medallion
  bronze/silver/gold/semantic layers): real `Table` objects for `silver_customer`/`bronze_actor`/
  etc., real `DependsOn` edges matching every `ref()`/`source()` call in the actual SQL (including
  one on line 48 of a 90-line model, not just the obvious top-of-file ones), and RFC 0094's
  `concentration_risks` pass immediately picked up `silver_customer` as a real risk once its
  dependents existed. Column lists from `schema.yml` are honestly partial (only documented/tested
  columns), never fabricated; unresolvable `ref()`s (cross-package, into gitignored
  `dbt_packages/`) are skipped, not guessed at.

- [ ] **MCP / connector infrastructure**: MCP auth + multi-workspace routing, and MCP resources/prompts
  capabilities beyond tools-only (RFC 0013); an HTTP/SSE transport as a *second* transport option
  alongside RFC 0115's plain-TCP one, if a browser-based client ever needs it; generic
  `ScanContext`/`ekos.toml [connectors.X]` config plumbing — confirmed missing for every
  connector, not just crypto (RFC 0017); dynamic/runtime plugin loading (`.so`/WASM) — RFC 0031
  itself calls this "a known limitation, not solved here" (RFC 0006, RFC 0031).

- [ ] **Connector-specific gaps**: GitHub GraphQL client upgrade (RFC 0020); GitHub secondary
  (abuse-detection) rate-limit backoff/retry — accepted real risk from the RFC 0062 live run;
  Confluence cross-space title resolution, LLM-based topic/concept extraction, API v1→v2
  migration (RFC 0022); local-docs cross-document `References` edges and per-image `KirObject`s
  (RFC 0023); ClickHouse cross-source joins in one query and LLM-based business-meaning
  enrichment of table/column names (RFC 0056); a live Databricks Jobs API / ADF management-plane
  connector (RFC 0038); a raw-RPC treasury connector and broader DAO governance platform support
  beyond one (RFC 0032); real-time streaming ingestion for the chat connector (RFC 0033).

- [ ] **Analyzers**: interprocedural/cross-file call-chain tracing — same underlying gap named
  separately for Python (RFC 0040) and Rust (RFC 0041), one cross-language item; Python `.ipynb`
  notebook support, `spark.sql(...)` argument-text parsing, full `.agg(...)` coverage (RFC 0040);
  Rust trait-dispatch resolution (RFC 0041); deep procedural-body parsing (IF/LOOP/cursors) for
  MySQL/Postgres SQL dialects (RFC 0031); semantic/embedding-based synonym matching in identity
  resolution, e.g. "orders" ≈ "purchases" with no string overlap (RFC 0007 — cross-kind matching
  itself already shipped via RFC 0029's `cross_system.rs`, don't re-add that part). **Narrower,
  adjacent case now solved (`devlog_66`, roadmap item 4)**: `cross_system.rs` now also matches a
  `Table`/`TransformNode` concept name against real free text (`File` paths, `Issue`/`PullRequest`
  titles) via fuzzy token containment — verified against 4 real cross-kind pairs from
  `analytics/`. This is *not* the embedding item above — it finds the same word reused across
  systems (`sites` the table vs. `lib/plausible/site` the directory), not true synonyms with no
  shared substring (`orders` vs. `purchases`), which still needs embeddings and remains open.

- [ ] **Docs generation**: HTML output for the curated layout — punted by three separate RFCs
  (RFC 0037, reaffirmed 0042 and 0045), one consolidated item; Docker/Kubernetes/Terraform/
  cloud-config parsing for curated docs (RFC 0042); LLM-based chapter/heading detection for
  document section boundaries (RFC 0024); Transformation IR semantic (business-meaning) diffing
  via a future `TransformSemanticsAnalyzerPass` (RFC 0028).

- [ ] **Multi-project / rollups**: full remediation of every analyzer-owned id scheme for
  multi-project collision-safety (RFC 0044) — the same gap `devlog_65` already investigated and
  found needs a real cross-cutting artifact-schema change, not a per-analyzer copy-paste fix (see
  `devlog_65.md`'s "Not fixed" section for the full reasoning); a dedicated `ekos_summarize` MCP
  tool, per-sub-project curated docs generation, and opt-in LLM-written rollup synthesis (RFC 0044).

- [ ] **Security**: `ArtifactId` is still computed from pre-redaction bytes, and redaction isn't
  applied at each of ~15 individual plugin `data`-construction call sites (RFC 0043) — flag as
  security-relevant, not routine cleanup.

- [ ] **Demo server**: general multi-tenant/self-serve ingestion beyond the fixed two-repo
  catalog, and a no-LLM/ledger-only `/ask` answer mode (RFC 0045).

- [ ] **World Engine — all independently confirmed still-open by the RFC survey**: a claim-review
  MCP tool and `valid_from`/`valid_until` query surface on `KirObject` (RFC 0047); a memory-type
  taxonomy — short-term/long-term, tracked incorrect beliefs (RFC 0049); Phase 8 parallel agent
  execution, an LLM-backed `DecisionEngine`, per-kind action effects beyond the one worked
  `FormAlliance` example, and Phase 14-16 (Metrics, Turning-Point Detection, Report Generation,
  Monte Carlo, Counterfactuals, Web UI, Video Generation) (RFC 0050, reaffirmed RFC 0054);
  per-agent decision-engine selection in scenario YAML (blocked on the `DecisionEngine` item
  above), scenario linting beyond structural/reference errors, and a scenario ledger cleanup
  command (RFC 0051); per-action-kind differentiated resource costs, YAML-authorable resource
  costs, and richer domain conflict rules beyond the one worked example (RFC 0052); round-based
  `Like`/`Follow`/`Share`/`Reply`-as-own-kind actions and a nested-thread reconstruction helper
  (RFC 0053); an interactive replay stepping session and video/report rendering of a replay
  (RFC 0054); a `DocumentSemanticsAnalyzerPass` for world sources, `[security]` extension patterns
  applied to scenario ingestion, and incremental/cached re-ingestion (RFC 0055).

- [x] **Identity resolution**: extend RFC 0029's cross-system `unconfirmed`-until-reviewed flow to
  same-source (`DefaultResolver`) merges too — RFC 0060's own stated residual (3 of 17 known-wrong
  real pairs still clear the 0.90 threshold), restated in RFC 0062 for the GitHub over-merge case.
  Done (RFC 0063 / `devlog_69`): split on exact-vs-fuzzy normalized name (RFC 0060 showed no
  confidence threshold alone separates known-good from known-wrong pairs), not a second threshold.
  Exact matches keep auto-merging; fuzzy matches become `unconfirmed` `SameAs` relationships,
  reviewable via the existing `ekos_identity_review` with zero changes to that tool. Verified live
  against a disposable copy of `analytics/`: all 3 named residual pairs now route to review instead
  of silently merging.
  - *Related, done (`devlog_66`):* roadmap item 4 ("attack identity resolution directly, with a
    narrow test case") picked 4 concrete cross-source pairs in `analytics/`'s real ledger
    (`sites`, `api_keys`, `goals`, `subscriptions` — each a real `Table` + real `File` + real
    `Issue`/`PullRequest`) and got them resolving correctly via `cross_system.rs`'s new
    fuzzy-token-containment pass. Doesn't touch the residual above (that's same-source
    `DefaultResolver` over-merge; this was cross-*kind* under-*matching* — a different bug in a
    different module) — still open, not solved by this. Live verification at full-ledger scale
    also found a real volume problem (27,383 candidates from one real repo, uncapped) and fixed it
    (`MAX_FREE_TEXT_MATCHES_PER_TABLE` cap, down to 2,202) — see `devlog_66.md` for the honest
    account, including a verification gap (the rebuild used to re-check live was missing `File`
    objects for an unrelated reason, see the new item directly below).

- [x] **`ekos build`'s fingerprint cache can silently drop `File` objects on a ledger rebuild** —
  root-caused and fixed, RFC 0077 / `devlog_80`. Root cause: the fingerprint gates both
  re-scanning the filesystem AND constructing/writing `File` `KirObject`s behind one check —
  correct for "did the source change," silent about "does the ledger still have the result."
  Fixed: `ledger.object_count() == 0` distrusts the fingerprint cache entirely for one run when the
  ledger looks freshly cleared, forcing a real rescan; every subsequent run resumes trusting the
  cache normally. Live-verified with a real reproduction test (clear `.ekos/ledger/`, rebuild,
  confirm `File` objects return) and a regression guard the other direction (intact ledger +
  unchanged content still hits the cache, doesn't duplicate). Deliberately doesn't cover a
  hypothetical *partial* File-object loss with everything else intact — not what was found live.

- [x] **Architecture Knowledge Model — reasoning layer, evaluator, MVP agent (RFC 0065/0066/0067)**:
  RFC 0065 Phase 1 (`devlog_70`) shipped the static knowledge model — `Claim`/`ArchitectureGap`
  KIR kinds, `CrateTopologyAnalyzerPass` deterministically populating them, a C4 mapping note + Open
  Questions in `render_architecture`. RFC 0065 Phase 2/3 + RFC 0066's MVP agent (`devlog_71`, RFC
  0067) closed the rest of the real MVP scope both RFCs define: `ArchitectureReasoningPass` (LLM
  role classification, `inference`-type `Claim`s), `evaluate_architecture` (deterministic
  completeness/evidence-coverage scoring), targeted re-collection (a crate's own doc comment), and
  `ekos architecture investigate` orchestrating all of it in RFC 0066 §65's 12-step loop. Live
  end-to-end with a real local Ollama model against this repo's own workspace.

  **Known gaps left untouched — for the next run on this feature:**
  - *Full 3-iteration loop never re-verified end to end with the chunking fix in place.* The
    context-window bug (`MAX_CRATES_PER_CALL`) was fixed and reverified live, but only via a fast
    `ekos recover`-only check (39/44 crates classified in one broad pass) — not a full
    `ekos architecture investigate` run, which would also exercise targeted re-collection against
    the *remaining* ~5 unclassified crates with the fix in place. The two full runs that did
    complete both predate the fix. Cost: ~30-45 min against this repo's real ~35k+-object ledger.
  - *5/44 crates still unclassified even post-fix* (39/44, not 44/44) — real residual quality gap,
    not yet root-caused. Worth checking whether it's model-capability (the LLM skipping a few names
    even within a correctly-sized chunk) or a second, smaller instance of the same class of bug.
  - *`document_semantics_analyzer.rs::collect_sections` has the same latent duplicate-artifact bug*
    `architecture_reasoning.rs::collect_crates` was found to have and fixed (RFC 0015's
    content-addressed, additive artifact store means "read every artifact matching this pass name"
    silently double-/triple-counts after more than one uncached `recover` run in a workspace's
    history). Not fixed here — noted live in `devlog_71`, still real and unaddressed.
  - *Role classifications aren't surfaced in generated docs.* `Claim` objects with `has_role` are
    real, evidence-backed, and queryable via `ekos ekl`/`ekos_search`, but `render_architecture`
    only gained the C4 note + Open Questions (RFC 0065 Phase 1) — no "Role Classifications" section
    reads Phase 2's `has_role` claims back out. A human reading `Architecture.md` currently can't
    see what the LLM concluded about each crate without querying the ledger directly.
  - *`evaluate_architecture` computes 2 of RFC 0065 §34's listed dimensions* (`completeness`,
    `evidence_coverage`) *only* — `consistency`, `cross_view_consistency`, `traceability`, and the
    rest have no real signal to compute yet and were deliberately left unscored rather than faked.
  - *RFC 0066's own Phase 2/3 sections, and RFC 0065's Phase 2 extractors — deliberately not
    started*: persistent checkpointing/resume, concurrency-safety infrastructure, CI/CD exit-code
    matrix + PR-comment workflow, human-review UI, MCP additions, `Assumption`/`Contradiction`-type
    claims (need the reasoning layer to detect a real contradiction first, which hasn't happened on
    real data yet), Terraform/Kubernetes/OpenAPI/SQL extractors. Each is real RFC-sized work in its
    own right — not to be started speculatively ahead of an actual need.

- [ ] **Architecture Documentation Standard — full build-out (RFC 0068)**: filed 2026-08-22 from an
  externally-authored 67-section spec unifying ISO/IEC/IEEE 42010, arc42, C4, and ISO/IEC 25010
  into one target documentation package — the fuller standard RFC 0065 Phase 1-3/RFC 0066's MVP
  agent/RFC 0067 were the first real slice of. **Explicit instruction: build the full feature set
  below, not another trimmed MVP — nothing here is to be cut, only sequenced.** Grouped below by
  RFC 0068's *own* MVP/Phase 2/Phase 3 structure (§61-63) — that sequencing is the source
  document's own, not an invented scope reduction; every item is real, planned build-out, not a
  deferred-indefinitely non-goal the way earlier RFC 0065/0066 items were framed.

  - **Already shipped, real subset** (RFC 0065 Phase 1-3, RFC 0066 MVP agent, RFC 0067 —
    `devlog_70`/`devlog_71`): deterministic crate-topology extraction, one C4-ish container-level
    view (`## Crate & Workspace Topology` + C4 mapping note in `Architecture.md`), LLM-backed role
    classification (`Claim`/`inference`), a deterministic evaluator (`completeness`/
    `evidence_coverage` only), targeted re-collection (crate doc comments), the `ekos architecture
    investigate` orchestrating loop, local Ollama support, Markdown generation. `ekos diff`
    (existing command, RFC 0018-era) already gives ledger-level point-in-time comparison — the real
    primitive RFC 0068 §31-32's "Documentation Drift" needs, not yet wired to an architecture-claim
    comparison though.

  - **RFC 0068 §61 MVP — remaining pieces**:
    - [x] **System Context** (§15) and **basic documentation drift** (§31-32) — done, RFC 0069 /
      `devlog_72`. Drift ended up needing no `ekos diff` extension at all: the real primitive was
      already `KnowledgeStore::object_history` (RFC 0047) plus `append_object`'s existing
      `(id, content_signature)` versioning (RFC 0015) — a role `Claim`'s own version history *is*
      the "documented vs. observed" comparison, not a separately modeled concept. Live-verified
      against this repo's own real ledger: 7 genuine findings from earlier real
      `architecture-reasoning` runs this session, with zero new pipeline run needed to prove it.
    - [x] **Basic Component View** (§18) and **Technology Inventory** — done, RFC 0070 /
      `devlog_73`. The Crate↔File design question resolved with zero new extraction: RFC 0044's
      existing `Rollup` grouping already covers it (`Rollup.name` and `Crate.path` use the same
      directory convention — confirmed against this repo's own real compiled data, not assumed).
      Live-verified: real Component View + Technology Inventory rendered against this repo's own
      already-committed ledger, no new pipeline run needed.
    - [x] **Basic Runtime View** (§20) and **Architecture Summary** (§14) — done, RFC 0071 /
      `devlog_74`. Runtime View links to the already-generated `SequenceDiagrams.md` rather than
      duplicating it (naming *which* sequences are important business scenarios needs LLM
      reasoning or human curation, neither available here — stated explicitly, not invented).
      Architecture Summary populates only real-evidenced fields (component/crate counts, top
      technologies, open-questions count); `Purpose`/`Architecture style`/`Major risks`/
      `Architecture confidence` each say explicitly why they're not computed yet rather than being
      guessed at. Hit the same relationship-duplication bug from RFC 0070 in a new location (raw
      dependent counts) — fixed the same way (dedupe by `(from, to)` before counting), with its own
      regression test — this is now the *second* per-view mitigation for the same untouched root
      cause tracked below.
    - [x] **SVG/diagram generation** — done for System Context, RFC 0073 / `devlog_76`. New generic,
      dependency-free, deterministic `render_graph_svg`/`layer_nodes` primitive (Kahn's-algorithm
      layering, tie-broken by node id for reproducibility; a cycle-fallback layer so no node is ever
      silently dropped) produces a real standalone `system-context.svg` alongside the existing
      Mermaid-in-Markdown block, written by `generate_curated` only when there's real dependency
      data. **All six RFC 0068 §61 MVP view items are now done.** Deliberately scoped to one
      diagram, not all four `graph TD` producers `docs-gen` has (per-object neighborhood via
      `render_mermaid_graph`, Crate & Workspace Topology / per-kind Dependency Graph via
      `render_relationship_kind_graph`) — the primitive is generic and ready for those as real,
      concretely scoped follow-on work, tracked here, not silently narrowed:
      - [x] **RFC 0102 / `devlog_119` (2026-08-26).** Closes all three remaining wiring items in one
        increment:
        - `render_object_neighborhood_svg` (per-object neighborhood diagrams, `--layout objects`) —
          one SVG per significant object with at least one relationship, alongside its `.md`/`.html`
          page.
        - `render_relationship_kind_graph_svg` (per-relationship-kind Dependency Graph,
          `--layout curated`) — `dependency-graph-<kind>.svg` per kind, linked from
          `Architecture.md` right after each kind's Mermaid block. The real decision this needed:
          `MAX_GRAPH_EDGES` (the same 20-edge cap `render_architecture`'s own Markdown loop already
          used to decide "real diagram" vs. "omitted, too large") hoisted to module scope and
          factored into a shared `dependency_graph_groups` function both the Markdown loop and the
          new SVG writer call — so the SVG writer can never independently drift from which kinds the
          Markdown page actually drew a diagram for (the same "logic duplicated across two spots,
          one drifts" shape this project has hit before — `DefaultResolver`'s kind-exclusion list,
          the two ledger backends' indexed-content field lists).
        - `render_er_diagram_svg` (`erDiagram` family, `--layout objects`) — `er-diagram.svg`
          alongside the existing Mermaid `er-diagram.md`/`.html`, linked from `index.md`/`.html`.
          `render_graph_svg`'s plain arrows are a real, named simplification of `erDiagram`'s
          crow's-foot notation (every table/edge still real), not a misrepresentation.
        - `sequenceDiagram` **deliberately still not attempted, a real Non-goal**: a sequence
          diagram is participant lanes over a time axis, a fundamentally different shape from every
          other diagram this primitive draws — forcing it through `layer_nodes`/`render_graph_svg`
          would misrepresent it, not simplify it (unlike the ER diagram case). Needs its own real
          layout primitive; left as a clearly scoped future increment.
        - **Correction to this file, not new work**: the `layer_nodes` wide-layer-wrapping item
          previously listed here as open was already shipped by RFC 0084 / `devlog_87`
          (`wrap_layer_into_rows`, `MAX_NODES_PER_ROW = 8`) — this file was never updated after that
          RFC landed. Found by re-reading the actual code rather than trusting a stale status
          summary (this exact stale claim was independently repeated in a user-pasted status
          document earlier the same session).
        - 9 new `ekos-docs-gen` tests, 3 new/extended `ekos` (CLI) integration tests, full workspace
          gate clean, `tests/integration` 3/3. **Live-verified** against a real scratch workspace (3
          real tables, 2 real compiled `ForeignKey` relationships, through the real
          `init`/`build`/`recover`/`resolve`/`compile`/`commit` pipeline): `docs generate --layout
          objects` wrote 3 real per-table neighborhood SVGs plus `er-diagram.svg`; `docs generate
          --layout curated` wrote `dependency-graph-foreignkey.svg` and `Architecture.md` contains
          the real `[ForeignKey Dependency Graph diagram (SVG)](dependency-graph-foreignkey.svg)`
          link — every SVG confirmed to start with `<svg ` and contain the real table names, not
          placeholders.
        - **Real finding surfaced, not fixed here**: running `docs generate` against this repo's own
          real self-analysis ledger at the repo root failed with `Schema error: 'An index exists but
          the schema does not match.'` — RFC 0101 (this same session, shipped just before this RFC)
          added a new `memory_path` field to `SearchIndex`'s tantivy schema with no migration path
          for an already-built on-disk index; `Index::open_or_create` validates schema rather than
          transparently upgrading. Every pre-existing `FactLedger` workspace is affected. Deliberately
          not fixed here (out of this RFC's scope, and rebuilding/migrating a real production
          ledger's search index needs an explicit user decision, not a unilateral one) — flagged to
          the user directly and tracked here.
        - [x] **Follow-up — RFC 0103 / `devlog_120` (2026-08-26).** A real migration, not a manual
          rebuild instruction (user's explicit choice when asked): `SearchIndex::open_impl` catches
          `TantivyError::SchemaError` specifically and self-heals by wiping and rebuilding the
          on-disk index (safe — it's a documented derived/rebuildable projection of the ledger's
          own facts) whenever the open is writable, forcing the returned marker to `None` so
          `FactLedger`'s existing full-reindex-on-`None`-marker path (the same one a brand-new
          workspace's first open already uses) does the rest with zero other code changes. A
          read-only open still fails clearly rather than mutating anything, matching RFC 0097's own
          "a read-only handle must never be the one doing writing/self-healing work" precedent.
          Genuine corruption (a malformed `meta.json`) is structurally distinguishable from a stale
          schema and still surfaces as a real error, not silently swallowed. 3 new `ekos-ledger`
          tests, full workspace gate clean, `tests/integration` 3/3. **Live-verified against the
          exact real failure that motivated this**: `ekos docs generate` against this repo's own
          real self-analysis ledger at the repo root (previously failing with the schema-mismatch
          error) now opens, self-heals, and produced real output — 5,533 objects rendered, 4,837
          per-object neighborhood SVGs (RFC 0102), a real `dependency-graph-sameas.svg` linked from
          `Architecture.md` — with no manual `.ekos` deletion or separate migration command run.

  - **RFC 0068 §62 Phase 2 — remaining pieces**:
    - [x] **Data Architecture** (§22) — done, RFC 0074 / `devlog_77`. Real Data Stores (every
      compiled `Table`/`Dataset`, with real foreign-key edge counts) and real Transformations/
      Lineage (link-through to `SequenceDiagrams.md`'s Data-Flow Sequences, RFC 0027). Found a
      real, concrete integration gap while building this: `TransformNode` source/sink nodes
      carried table names as *properties*, not a relationship edge to the actual compiled `Table`
      object. Closed in Increment 7, below.
    - [x] **Data Architecture cross-referencing (RFC 0075 / `devlog_78`)** — closes the four
      follow-ons the item above surfaced:
      - [x] `TransformNode` Source/Sink nodes now link to the real `Table`/`Dataset` object they
        name (`ekos_semantic::data_lineage::link_transform_nodes_to_tables`, run from `commit.rs`)
        — unambiguous exact-name match only (case-insensitive), deterministic ids from the start
        (matching RFC 0072's pattern, not repeating its bug). `docs-gen`'s Data Stores section now
        shows real read/write-by-transformation counts per store. Live-verified end to end
        against a disposable fixture, idempotent across a re-commit.
      - [x] Data Domains — real, reusing structure already in the compiled name (schema-qualifier
        prefix, e.g. `sales.orders` → domain `sales`), zero new extraction. Honestly empty for
        both this repo's own committed fixtures (`ecommerce.sql`, `northwind.sql` both use
        unqualified table names) — the grouping logic itself is unit-tested against synthetic
        qualified names, and will activate for real on any workspace whose DDL qualifies table
        names.
      - [x] **Correction, not implementation**: RFC 0074's own Ownership text (repeated in this
        file, just above, before this edit) was factually wrong — `git_analyzer.rs`'s only
        `OwnedBy` edge connects a **commit event** to its **author**, never a `File` object (which
        `git_analyzer.rs` doesn't even emit); RFC 0074 had claimed it landed "onto observed `File`
        objects." Corrected in the rendered Data Architecture text and here. Real, still-open,
        now-correctly-scoped blocker for Ownership: (1) `git_analyzer.rs` needs a new per-file
        top-contributor derivation (it only has commit-event-level `OwnedBy` today, not file-level
        — a real, buildable extension of the same pass's existing per-file `CoupledWith` coupling
        logic, not a redesign); (2) a `Table`/`Dataset` needs the same kind of name/evidence-path
        linkage RFC 0075 just built for `TransformNode`s, but against `File` objects instead of
        `Table` objects. Neither built yet — both concretely scoped now, not vague.
      - [x] Lifecycle — same root blocker as Ownership's item (2) above (no `Table`→`File` link);
        not a separate investigation, confirmed to share the identical missing primitive.
      - [x] Data Quality — checked for a hidden signal (DDL `NOT NULL`/constraint metadata) and
        deliberately didn't use it: a structural constraint is a stated rule, not a measurement of
        actual data, the same requirement-vs-observation distinction RFC 0068 §26 itself draws.
        Confirmed genuinely blocked on RFC 0068 §63 Phase 3 runtime telemetry (row counts, null
        rates, constraint violations against real data) — checked, not assumed.
    - [ ] Terraform/Kubernetes/OpenAPI extractors (same items RFC 0065/0066 already named) —
      genuinely new extraction, no existing analyzer to extend; investigated and explicitly not
      started this increment in favor of Data Architecture (RFC 0074's own investigation section).
    - [ ] **Deployment Architecture** (§21), **Security Architecture** (§24), **Quality
      Architecture** (§26-27) views — each a real named section in the target package, none built
      yet; Deployment Architecture specifically depends on the Terraform/Kubernetes/OpenAPI
      extractors above (no compiled infrastructure data exists to render a real view from yet).
    - [x] **Architecture Diff** (§55) — RFC 0108 / `devlog_124` (2026-08-26). `ekos architecture
      diff --from <ts> --to <ts>`: real architecture-level diff (technologies, crate role
      classifications, risks, open questions) — distinct from raw `ekos diff`'s bare entry-id
      report. Reuses `all_objects_at` (RFC 0096) plus the deterministic `KirId`s every covered
      kind already mints (`technology_kir_id`/`role_claim_kir_id`/`architecture_gap_kir_id`/
      `concentration_risk_kir_id`, confirmed by reading each analyzer directly) — the whole diff is
      a plain id-set comparison per kind, no fuzzy matching, no new ledger primitive. A claim new
      in the later snapshot is deliberately not misreported as a role change (no real "from" role
      to name). 10 new tests (8 `ekos-recovery`, 2 `ekos` CLI), full workspace gate clean, `tests/
      integration` 3/3. Live-verified against a real scratch workspace (re-confirmed independently
      against a second, timing-careful fixture): a real new dependency added between two commits
      correctly reported under `Technologies added`, every other category honestly `0`.
      Relationship-level diff (e.g. a `DependsOn` edge change) and continuous/scheduled drift (§56)
      deliberately left as named follow-ons, not attempted here. **Process note**: this increment
      was built by a subagent that was dispatched for read-only TODO.md auditing only and exceeded
      its scope by implementing this feature unprompted — the resulting code was reviewed,
      verified (full gate + live check), and kept because it was genuinely correct and covers a
      real, wanted gap; the RFC number it picked (0107) collided with a concurrently-written RFC
      and was renumbered to 0108. Flagged as real subagent-scoping feedback, not silently absorbed.
    - [ ] **Architecture Drift** (§56, continuous version of the MVP's one-shot drift check, RFC
      0069).
    - [x] **Human Review** workflow — RFC 0109 / `devlog_127` (2026-08-26). `ekos_architecture_review`,
      a second write-capable MCP tool mirroring RFC 0029's `ekos_identity_review` exactly (kind-check
      discipline, `reviewed_at` timestamp, a real `KirEvent` audit record) — confirm/reject an
      LLM-classified role `Claim` before it's treated as ground truth. A real correctness trap found
      and designed around *before* writing any code: `ArchitectureReasoningPass` is deliberately
      ledger-free and re-derives the same deterministic claim id every `recover`/`commit` cycle — a
      naive "stamp review_status: unconfirmed at creation" design would have made every claim's
      content signature differ from its already-reviewed ledger version on the very next re-run,
      silently reverting every human decision back to unconfirmed. Fixed with two parts: the
      reasoning pass never writes `review_status` at all (read by absence); `commit.rs`'s new
      `preserve_claim_review_status` (the one place that already does real ledger-aware enrichment
      before appending, matching `commit_rollups`/`commit_data_lineage`'s own precedent) carries a
      real review status forward when the role value is unchanged, so an unchanged reviewed claim
      writes no new version at all. A genuinely changed role value does *not* inherit the old status
      — a new assertion needs new review. MCP-only, no CLI equivalent, matching
      `ekos_identity_review`'s own established precedent exactly. 9 new tests (4 `ekos` commit-layer,
      5 `ekos` MCP), full workspace gate clean, `tests/integration` 3/3. **Live-verified end to end,
      not just unit-tested**: a real scratch workspace run through `ekos architecture investigate`
      with a real local Ollama call (`qwen2.5:1.5b`) produced a real role claim; reviewed via the
      real `ekos mcp serve` binary over stdio; a subsequent real `recover`/`compile`/`commit` re-run
      with no source changes reported "Objects skipped: 7 (already in ledger)" — the reviewed claim
      among them, `review_status: "confirmed"` still intact.
    - [ ] **ADR generation** (§28, Architecture Decision Records — `BusinessRule`/`Custom("Claim")`
      kinds are the closest existing KIR shapes to extend).
    - [x] **MCP exposure of architecture query/investigation tools** — RFC 0107 / `devlog_125`
      (2026-08-26). Two new read-only MCP tools alongside the existing RFC 0013 set:
      `ekos_architecture_evaluate` (real completeness/evidence-coverage score, RFC 0065 Phase 3 —
      the same computation `ekos architecture investigate` uses, without running a build) and
      `ekos_architecture_drift` (documentation drift, reusing `architecture.rs::detect_drift`'s
      exact logic). Both call existing, already-tested pure functions verbatim — no new
      evaluation/drift logic, only MCP-protocol wiring. Deliberately does *not* expose
      `investigate`'s own orchestration loop over MCP (write-heavy, potentially LLM-costed —
      fundamentally different in kind from every other tool in this read-only server;
      `ekos_identity_review` stays the one deliberate write exception, and even that only
      confirms/rejects an already-proposed candidate). 4 new tests, full workspace gate clean,
      `tests/integration` 3/3. **Extended same-day**: a third tool, `ekos_architecture_diff`,
      added alongside the other two — thin MCP wiring over RFC 0108's own `diff_architecture`,
      same `from`/`to` RFC 3339 argument shape as the existing `ekos_diff` tool. 2 more tests
      (`tools/list` exhaustive-name test updated to include it).

  - **RFC 0068 §63 Phase 3 — remaining pieces**: runtime telemetry/logs/metrics/traces ingestion;
    continuous drift detection (running the MVP drift check on a schedule/trigger, not just
    on-demand); **Architecture Q&A** (§57 — likely extends `ekos ask`'s existing grounding+citation
    pipeline rather than a new one); Target Architecture / Migration Architecture (a *desired*
    future-state AKM compared against the *current* reconstructed one — genuinely new concept, no
    existing EKOS primitive for a non-observed, aspirational knowledge state); architecture fitness
    checks; architecture governance; architecture evolution analysis (trend over multiple
    baselines, not just two-point diff).

  - **Structural/standards-mapping work spanning all phases, not phase-specific** (§6-13, §41-44):
    ISO 42010's Stakeholders/Concerns/Viewpoints/Views/Model-Kinds framework — no existing EKOS
    concept models "stakeholder concern" or "viewpoint" as first-class filters over the AKM today;
    Cross-View Consistency checking (§41, "does C4 Context agree with C4 Container" — needs at
    least two real views to exist first before this is checkable); Architecture Correspondence
    (§42); Quality-to-Architecture and Architecture-to-Evidence traceability (§43-44, extends the
    existing evidence-linking already on every `Claim`/object, but as an explicit cross-cutting
    report, not implicit); a Glossary section (§39) and Appendices (§40) in generated docs;
    Documentation Quality Gate (§48, a pass/fail gate wired into the investigation loop's DECISION
    step, extending RFC 0066's existing quality-threshold check); Machine-Readable Companion (§53 —
    likely the CKM/ledger JSON already *is* this, needs packaging/documenting as a deliverable
    rather than new data); Architecture Baseline (§54, a named, retrievable ledger snapshot —
    `ekos build`'s existing `.ekos/snapshots/*.json.zst` may already be most of this).

  - *Next step*: **Increment 9** — with Data Architecture (§22, RFC 0075), Architecture Diff (§55,
    RFC 0108), MCP exposure (RFC 0107), and Human Review (RFC 0109) all closed, the remaining
    untouched §62 Phase 2 items are: the Terraform/Kubernetes/OpenAPI extractors (genuinely new
    extraction, no existing analyzer to extend — the biggest remaining item); Deployment/Security/
    Quality Architecture views (Deployment blocked on the extractors above; Security/Quality are
    real, independently startable); continuous Architecture Drift (§56, needs real scheduling
    infrastructure this project doesn't have — a bigger, separate decision, not a small increment);
    ADR generation (§28); the two newly-scoped Ownership/Lifecycle follow-ons from RFC 0075
    (a `git_analyzer.rs` per-file ownership derivation; a `Table`/`Dataset`→`File` link) are real
    candidates too, now that they're concretely designed rather than vague. Its own dated RFC the
    way RFC 0069-0075/0107-0109 were, continuing automatically down the roadmap per the standing
    instruction not to cut anything here.

- [ ] **Real gap found running EKOS end to end on a real, non-EKOS project (2026-08-26):** Python's
  `requirements.txt` (and, by the same reasoning, `pyproject.toml`) has no dependency analyzer at
  all — confirmed by grepping `crates/recovery/src/` directly (no `requirements`/`pip` file
  anywhere), unlike `package_json_analyzer.rs` (npm) and `dependency_analyzer.rs`/
  `crate_topology_analyzer.rs` (Cargo). Found by running the full real pipeline
  (`init`/`build`/`recover`/`resolve`/`compile`/`commit` + `docs generate --layout
  curated`/`solution-architect`) against a real external project (`pdf-reader`: FastAPI Python
  backend + React/TypeScript frontend, `[llm-description] scope = "all"` enabled) — every generated
  `## Technology Inventory`/`## System Context`/`Declared Versions` section only ever showed the 12
  real `package.json` dependencies; all 10 real `backend/requirements.txt` dependencies (`fastapi`,
  `sqlalchemy`, `pymupdf`, `pytesseract`, `openai`, etc.) were completely invisible to every one of
  those views, even though the Python *source code itself* was correctly analyzed in full (55
  `PythonModule`/39 `PythonSymbol` objects, real AI overviews grounding on real source lines). A
  real, concretely-scoped gap for a future increment: a `requirements_analyzer.rs` mirroring
  `package_json_analyzer.rs`'s exact shape (`Custom("Technology")` per declared dependency,
  `DependsOn` edge from the owning `File`) — `requirements.txt`'s `pkg==1.2.3`/`pkg>=1.2.3` line
  format is simpler to parse than `package.json`'s JSON, so this is likely a *smaller* increment
  than its npm sibling, not a bigger one. Two secondary findings from the same live run, both
  real-usage observations rather than code bugs: (1) a small local Ollama model (`qwen2.5:1.5b`)
  was unreliable for RFC 0088's structured-JSON-output description task — 111 of 119 real attempted
  calls failed (presumably malformed JSON the model produced, not caught/logged in detail anywhere
  today — `llm_description.rs`'s `call_and_apply` discards the real error string after counting it,
  a real, separate, smaller gap worth its own fix); the properly-sized `llama3:latest` (the
  project's own configured model) worked correctly on the same task but was too slow on this
  hardware (CPU-bound 8B inference) to finish within a reasonable interactive wait, so the smaller
  model was substituted for the live test — not a silent swap, disclosed here. (2) the LLM-assisted
  `Architecture style` field said "microservices" for a project that's really a single FastAPI
  backend + a separate SPA frontend (arguably not accurate microservices terminology) — the pipeline
  itself worked exactly as designed (the field is honestly labeled "(LLM-assisted, RFC 0088 — see
  the object's own evidence)," not presented as certain), a real example of why that label exists,
  not a bug to fix.

- [x] **`KirRelationship`'s non-deterministic ids let logically-identical relationships
  accumulate as real duplicates across repeated commits** — the concretely observed instance fixed
  at its source, RFC 0072 / `devlog_75`. Found live verifying RFC 0070's Technology Inventory view
  (devlog_73), found *again* independently in RFC 0071's Architecture Summary (devlog_74) — two
  render-time mitigations for the same root cause, both still in place. `crate_topology_analyzer.rs`'s
  `DependsOn` edges (the actual pass responsible for every duplicate seen in either view) now get a
  deterministic id, matching how `Crate`/`Technology`/`Claim`/`ArchitectureGap` already do —
  live-verified end to end (not just unit-level): three independent `build`/`recover`/`compile`/
  `commit` cycles against a real disposable workspace on the real default v3 `FactLedger` backend
  produced the same 2 real relationship ids each time, not a growing count.
  - *Deliberately not a blanket fix* — investigated first, not assumed safe: `grep` found 136
    `KirRelationship::new()` call sites across 32 files, and `sql_analyzer.rs`'s `ForeignKey` edges
    are a real, already-shipped counter-example (two distinct real foreign keys between the same
    two tables via different columns share the same `(from, to, kind)` tuple — a blanket
    `(from,to,kind)`-based id would have silently collapsed them, losing a real fact). Each
    relationship kind needs its own real judgment call about what distinguishes two instances, not
    a mechanical global change.
  - *Still open, real, separate work* — not folded into RFC 0072: the other 134 call sites remain
    exposed to varying degrees (Crate topology Mermaid diagram, MCP tools, EKL queries, and any
    other relationship-reading code); each needs the same kind of case-by-case investigation RFC
    0072 did for `DependsOn`, not a batch fix. Also: this fix does not and cannot retroactively
    clean up duplicate rows already committed to this repo's own real ledger before it shipped (no
    delete/tombstone mechanism exists anywhere in the codebase) — RFC 0070/0071's render-time dedup
    stays in place for exactly that reason and keeps working regardless.
  - **RFC 0072 named `sql_analyzer.rs`'s `ForeignKey` edges as the counter-example proving a
    blanket fix would be wrong — but never checked whether `sql_analyzer.rs`'s own `Table`/
    `ForeignKey` objects had a deterministic id at all.** They didn't. Found live testing a real
    external project (RFC 0076, below) — one of the 134 still-open call sites this note already
    flagged, now closed for real, live-verified data corruption, not just a theoretical gap.

- [x] **Real-project testing (RFC 0076 / `devlog_79`)** — compiled the current build and generated
  documentation for a real, external, non-Rust project (`/home/legion/PycharmProjects/analytics`,
  Plausible Analytics, Elixir, 804MB, 495 files, a real multi-month-old EKOS case-study workspace).
  First time this session tested against pre-existing real ledger state rather than a fresh
  disposable fixture. Six findings, four fixed:
  - [x] `sql_analyzer.rs`'s `Table`/`ForeignKey` objects had no deterministic id — every table in
    the real workspace existed twice (114 rows for 57 real tables, confirmed live via `ekl`). Same
    failure class as the `DependsOn` bug above, one layer deeper. Fixed: `table_kir_id`/
    `foreign_key_kir_id`, live-verified via a completely fresh rebuild of the real workspace, twice
    — 57 tables both times.
  - [x] Elixir's `defp`/`defmodule` (and `defmacro`/`defmacrop`/`defdelegate`) were invisible to
    `plugins/file/src/lib.rs`'s declaration-prefix symbol fallback — checked a real large Elixir
    codebase directly: 1917 `defp` vs. 2509 `def`, 522 `defmodule`, all silently missing. Fixed by
    extending `DECL_PREFIXES`; zero cost for every other already-covered language.
  - [x] `ekos doctor` false-negatived on a correctly-running local Ollama (hardcoded
    `ANTHROPIC_API_KEY` check regardless of configured provider). Fixed: `llm_provider_check`
    extracted and testable, Ollama special-cased (no API key exists for a local server).
  - [x] `ekos compile`'s "Warnings: N (check logs)" pointed nowhere real — every diagnostic only
    ever logged at `tracing::debug!` (invisible at this project's own default `log-level = "info"`)
    and was never persisted; `ekos recover` was worse, not even printing a count. Fixed:
    `DiagnosticSink::emit` now logs at each diagnostic's real severity, and a new
    `write_diagnostics_log` helper persists the full list to `.ekos/diagnostics/<command>.log`,
    wired into both commands. Live-verified: surfaced a real, previously-invisible, actionable
    finding on the first try (`SQL003: ... model 'llama3.1:8b' not found`).
  - [ ] **Not fixed, investigated, not a bug**: low SQL transform coverage (20% mapped) turned out
    to be a real Postgres trigger function's control flow (`IF`/`RAISE`/`RETURN`), genuinely out of
    the Transformation IR's dataflow-only scope (RFC 0027), correctly and honestly reported
    `Unmapped` rather than fabricated. Modeling procedural control flow (`Branch`/`Loop`/
    `Exception` IR node types) would be a real, separate, substantial feature — not attempted, not
    needed to call this "not broken."
  - [ ] **Not fixed, investigated, real fix deferred**: `ekos resolve` took ~5 min against the real
    pre-existing workspace (29.5M pairwise comparisons over 10,178 candidates).
    `DefaultResolver::resolve` already blocks by `(kind, name-prefix)` — not a naive unblocked scan.
    A completely fresh rebuild of the same real workspace produced 5,241 pairs for a structurally
    identical run (≈5,600× fewer) — strong evidence the real driver is candidate-set inflation
    specific to a long-lived, repeatedly-`recover`'d workspace (most likely accumulated
    `KnowledgeArtifact`s from many historical runs all still read as current input by `compile`'s
    `knowledge_artifact_ids`), not the resolver's blocking algorithm. Not fixed: the real fix is
    either an artifact-store lifecycle change (prune/supersede old `KnowledgeArtifact`s per pass) or
    a blocking-key improvement — both real, larger changes with genuine risk of dropping evidence a
    case this session didn't test still needs. A guessed fix here risked a worse regression than the
    performance cost it addresses.
  - *Recurred, confirming the diagnosis*: hit live again during RFC 0081's own verification —
    re-running `recover`/`compile` against the same real analytics workspace after invalidating
    `pass-manifests` (needed to pick up an analyzer code change) produced a real, temporary spike
    to 15,866 CKM-stage objects (nearly double), before `commit`'s content-addressed dedup brought
    the final ledger back to the correct real count. Same root cause, same real workaround
    (`ekos ledger` doesn't yet have a real prune tool — this is exactly Storage Architecture Phase 2
    from RFC 0080, above), not a new bug. Recurred a third time during Phase 2's live verification
    (`SEM002 unknown from-id` warning count: 3379 → 3879 → 6331 across three consecutive cycles) —
    confirmed via direct `ekl` lookup that the specific flagged objects/edges resolve correctly
    post-`commit` each time; still not fixed, still correctly deferred to Storage Architecture
    Phase 2.

- [ ] **`GitObserver::is_git_repo()` false-positive on an unrelated ancestor `.git`** — found live
  while cleaning the analytics project's ledger (before the docs-decomposition plan started), not
  yet fixed. `plugins/git/src/lib.rs` uses `git rev-parse --git-dir`, which walks up to *any*
  ancestor `.git` — so a second `[[observe]] paths` entry with no `.git` of its own (e.g.
  `../analytics-docs`) gets wrongly detected as a git repo if it happens to sit inside a parent
  directory that does have one (`/home/legion/PycharmProjects/.git`). Compounded by
  `recover.rs`'s `collect_git_artifact_ids`, which scans the *whole* artifact store for
  `connector_name == "git"` artifacts with no per-project scoping and unconditionally overwrites
  (`repo_id = Some(id)`, last-one-wins) which "repo" metadata is treated as authoritative — this
  can nondeterministically surface the wrong (tiny, ~1-contributor) commit history instead of the
  real one (~124 contributors), depending on artifact store iteration order. Real fix needs two
  parts: `is_git_repo()` should check for a `.git` directly inside the given path, not walk
  ancestors (or explicitly opt into ancestor discovery only when desired); `collect_git_artifact_ids`
  needs the same per-project scoping RFC 0079 already gave the other multi-project analyzers.

- [x] **Identity resolver: `ElixirModule`/`ElixirSymbol`/`JsModule`/`JsSymbol` missing from
  `DefaultResolver`'s blanket kind-exclusion list** — `devlog_90`. Found live reading a real
  generated entity page (`Plausible.Auth.Password`), not by testing: a real password-hashing
  module had 1,236 real `SameAs` edges to unrelated real modules at confidence=1.00, the exact same
  same-kind-structural-fallback failure `Section`/`TransformNode`/`RustSymbol`/`RustModule`/
  `PythonSymbol`/`PythonModule`/`Crate`/`Claim`/`ArchitectureGap` already hit — RFC 0081/0085 both
  missed adding their new kinds to this list. Fixed (`crates/identity/src/lib.rs`), 3 new tests
  using the exact real names. Verified via a full clean ledger rebuild: 0 bad edges (was 1,236).
- [x] **`docs-generated/` self-referential contamination** — `devlog_90`. Found while chasing why
  the identity fix above didn't seem to take effect: the analytics project's `ekos.toml` never
  excluded its own `docs-generated/` output directory from `[observe] paths`, so every `ekos build`
  after every `ekos docs generate` re-ingested EKOS's own previously generated markdown as real
  project documentation — inflating "Local documents analysed" from 237 to 6,364 and the ledger
  object count to 127,676 at its worst, independent of and compounding RFC 0076 Finding 6. Fixed
  with one `ignore-patterns` line; verified via full clean rebuild: 2,414 real files (not 6,128),
  139 real local documents (not 6,364), 8,787 real CKM objects (not 127,676).

- [x] **Deep Source Decomposition + Production-Grade Architecture Diagrams** — all 6 phases
  shipped and live-verified. Full plan: `/home/legion/.claude/plans/1-prove-the-core-memoized-wren.md`.
  Real motivation: generated `Architecture.md` for the real analytics project and found it
  unprofessional — no backend/frontend/database decomposition, mostly flat lists, the one real
  diagram (System Context) an unreadable 8296×190px single row.
  - [x] **Phase 1 — real Elixir decomposition** — RFC 0081 / `devlog_84`. New
    `ekos-plugin-elixir` observer + `ElixirAnalyzerPass`: real `Custom("ElixirModule")`/
    `Custom("ElixirSymbol")` objects, real `Contains`/`DependsOn` edges (including real
    module-to-module dependencies when both ends are locally defined — the actual "restore links
    and relationships" deliverable). Live-verified against the real analytics project: 1231 files,
    ~1260 modules, ~4800 functions, spot-checked against two real files' actual content. Found and
    fixed a real `API.md` integration gap along the way (two-level `File→Module→Symbol` containment
    wasn't resolved by the existing Rust/Python-shaped grouping logic).
    - *Deferred, not cut*: Phoenix-convention role tagging (controller/LiveView/context) — designed,
      then cut after finding a real dedup-ordering risk in the natural implementation; left for
      Phase 3 as a render-time derivation instead (matching RFC 0075's Data Domains pattern).
  - [x] **Phase 2 — `package.json` dependency extraction** — RFC 0082 / `devlog_85`. New
    `PackageJsonAnalyzerPass`: real `Custom("Technology")` objects + `DependsOn` edges from
    `dependencies`/`devDependencies`, pure JSON parse, no new parser crate. Live-verified against
    the real analytics project: 4 real manifests, 76 real `Technology` objects, 92 real
    `DependsOn` edges; spot-checked `react` directly via `ekl` (real object + real edge from
    `assets/package.json`'s real `File` object, both confirmed in the final committed ledger).
    `Architecture.md`'s `## Technology Inventory` now lists all 76 real packages with real
    per-manifest attribution.
    - *Real anomaly investigated, not a Phase 2 bug*: `compile` logged a growing count of
      transient `SEM002 unknown from-id` warnings (3379 → 3879 → 6331 across three consecutive
      cycles against this same long-lived workspace) — confirmed as a further live recurrence of
      RFC 0076 Finding 6 (already tracked above), not something this phase introduced; the
      specific flagged object/edge both resolve correctly post-`commit`.
  - [x] **Phase 3 — real System Decomposition view (Backend/Frontend/Database layers)** — RFC
    0083 / `devlog_86`. New `layer_classification.rs`: convention-based `classify_path` (backend/
    frontend language extensions, `package.json` as an always-frontend signal), with a real
    `[[architecture.system-decomposition.overrides]]` escape hatch in `ekos.toml` (same
    first-glob-match-wins shape RFC 0031's `[recover.sql.dialect-rules]` already established). New
    `## System Decomposition` section in `render_architecture`, right after `## System Context`,
    reusing `render_graph_svg` (RFC 0073) completely unmodified. Cross-tier edges only drawn when a
    real `DependsOn`/`ReadsFrom`/`WritesTo` relationship justifies one; honestly absent otherwise
    (true for this project today — Phase 6's job). Live-verified against the real analytics
    project: Backend (1232 files), Frontend (324 files), SQL Database (57 tables), rendered as a
    genuinely readable 568×80px SVG — the direct fix for the complaint that started this whole
    plan.
  - [x] **Phase 4 — diagram-quality fixes** — RFC 0084 / `devlog_87`. `layer_nodes`'s topological
    DAG layering left unchanged; new visual-only row-wrapping (`wrap_layer_into_rows`, 8
    nodes/row) so a wide layer becomes multiple rows instead of one unreadable wide row. New
    standalone `crate-topology.svg` (reusing `render_graph_svg` unmodified, same shape as System
    Context's own SVG). Component View's crate-with-no-matching-rollup case now named and counted,
    not silently dropped. Live-verified against EKOS's own self-dogfooded ledger (the analytics
    project has zero `Crate` objects at all — Elixir/Phoenix, no `Cargo.toml` — so these three
    fixes needed a real Rust workspace to exercise): System Context's real 46-node diagram now
    renders as a multi-row 1488×470px SVG instead of the previously-reported unreadable single-row
    8296×190px; `crate-topology.svg` is new and real (44 crates); Component View now names
    `ekos-benchmark, ekos-integration-tests` explicitly instead of silently omitting them.
    - *Deferred, not cut*: standalone SVGs for per-object neighborhood diagrams and `render_api`'s
      per-relationship-kind graphs — a different shape of work (many small per-object/per-kind
      SVGs vs. one whole-workspace SVG per section) with much lower marginal readability payoff
      than the three items shipped; left for a future increment if real usage shows it's needed.
  - [x] **Phase 5 — real JS/TS decomposition** (`javascript_analyzer.rs`) — RFC 0085 / `devlog_88`.
    `oxc_parser` chosen over `swc_ecma_parser` after a real live comparison (crates.io/docs.rs
    metadata fetched, not assumed) — MIT license, single-call API, native TS/JSX/TSX, pinned to
    `=0.133.0` (latest 0.146 needs rustc 1.95, newer than this workspace's 1.93). Real
    `Custom("JsModule")`/`Custom("JsSymbol")` objects, real `Contains`/`DependsOn` edges, flat
    `File → Symbol` containment (no Elixir-style two-level fix needed). Found and fixed a real bug
    live: 18/291 real files failed to parse — all real `.js` files containing real JSX (`.js`
    doesn't get JSX enabled by extension alone); fixed by forcing JSX on for JavaScript source
    types while deliberately leaving TypeScript's `.ts` non-JSX (real `<T>expr` generic-assertion
    ambiguity). Live-verified: 291 real files, 434 real `JsModule`s, 851 real `JsSymbol`s, 99.3%
    parse success after the fix (2 real remaining failures are a real, uninvestigated gap in the
    pinned older `oxc_parser` version's own TS grammar coverage, not an EKOS bug).
  - [x] **Phase 6 — real cross-tier edges (stretch)** — RFC 0086 / `devlog_89`. Backend→Database
    shipped: extended `elixir_analyzer.rs` (not a new pass) to detect real `use Ecto.Repo,
    adapter: Ecto.Adapters.X` declarations, emitting a real `Custom("Technology")` object +
    `DependsOn` edge, reusing `dependency_analyzer.rs`'s own "PostgreSQL" naming convention for
    cross-analyzer identity. `docs-gen` extended with real `Contains`-based one-hop layer
    inheritance (edge resolution only, never inflating displayed file/table counts) and a
    database-adapter-Technology bucket that routes into the same real `layer_sql`/
    `layer_clickhouse` node a matching `Table` would use — honest `"(config only, no tables
    compiled)"` label when a real adapter exists with zero real compiled tables behind it (true
    for this project's ClickHouse side today). Found and fixed a real duplication bug before
    shipping: the real analytics project's 5 separate ClickHouse-adapter Repo modules would have
    each re-pushed a duplicate "ClickHouse" object without extending the existing cross-file dedup
    condition. Live-verified: 6 real ClickHouse-adapter modules and 3 real Postgres-adapter
    modules all resolve to one real object per database (confirmed via `ekl`, no duplication);
    `Architecture.md`'s `## System Decomposition` now draws real `Backend → SQL Database`/
    `Backend → ClickHouse Database` arrows — the plan's first real cross-tier relationship line.
    Frontend→Backend (route/fetch-call matching) stays deliberately unattempted, per the plan's
    own original lower-confidence scoping — not cut, never in this phase's real scope.

  **Plan complete.** All 6 phases (RFC 0081-0086, devlogs 84-89) shipped, tested, and live-verified
  against the real analytics project and/or EKOS's own self-dogfooded ledger. Real backend
  (Elixir) and frontend (JS/TS) decomposition, real npm/database dependency data, a real System
  Decomposition view with real cross-tier edges, and the diagram-readability bugs that made the
  original complaint's System Context view unreadable are all fixed.

- **"Real Descriptions, Purpose, and Links Throughout Generated Documentation" plan**
  - [x] **Phase 1 — real doc-comment extraction (Rust/Python/Elixir/JS-TS)** — RFC 0087 /
    `devlog_91`. All four real decomposition analyzers now extract a real `"description"` property
    from human-written source documentation (`///` doc comments via `syn`'s `#[doc]` attributes,
    PEP 257 docstrings, `@moduledoc`/`@doc`, JSDoc via `oxc_parser`'s comment classification) —
    never fabricated, property absent entirely when the source has none. 18 new tests across the
    four analyzers, all passing.
  - [x] **Phase 2 — entity-page rendering** — RFC 0087 / `devlog_91`. `docs-gen` promotes the real
    `"description"` property into the entity page's Definition section (Markdown + HTML), with an
    honest "Not documented in source" fallback when absent; relationships also regrouped by real
    structural meaning (`"Based on"` for the `Contains` parent, direction-grouped otherwise) rather
    than raw kind. New `docs-gen` render tests, all passing.
    - Full workspace gate (`build`/`test`/`clippy -D warnings`/`fmt --check`) clean. Live-verified
      against the real analytics project with a full clean rebuild: `Plausible.SentryFilter`'s real
      `@moduledoc` text renders correctly on its entity page; `Plausible.Auth.Password` (genuinely
      undocumented in source) renders the honest fallback. Also reconfirmed devlog_90's two bug
      fixes (identity over-merge, `docs-generated/` contamination) still hold on a fresh rebuild.
    - *Process note*: live-verification itself nearly reintroduced devlog_90's contamination bug
      under a new name (`docs generate --output doc` instead of the already-ignored
      `docs-generated`) — caught before any real build re-ran; see `devlog_91`'s Knowledge
      Captured for the generalizable lesson.
  - [ ] **Phase 3+ ("Links")** — not yet scoped: cross-linking between related entities beyond the
    existing relationship list (e.g. inline references from prose/description text to other entity
    pages). Deferred until real usage against a live project shows what's actually missing, per
    this project's own just-in-time RFC convention.
  - [x] **Phase 4 — LLM-backed compile-time descriptions (modules, subsystems, symbols, and
    project-level Purpose/Architecture-style)** — RFC 0088, filed + implemented 2026-08-23
    (`devlog_93`). Real, evidence-grounded `ai_overview`/`ai_usage`/`ai_comment_check` properties,
    persisted at `commit` time (not regenerated at every `docs generate` the way `--prose` is),
    for every `Module`/`Rollup`/`Crate` **and every `Symbol`** with a real compiled `source_span`
    (Rust/Elixir so far — Python/JS deferred, a real, honest, tracked gap, not silently smoothed
    over) — per-symbol scope folded into this RFC's own implementation at the user's explicit
    request. New `ai_comment_check` (`"consistent"`/`"stale"`/`"incomplete"`) is the concrete
    answer to the user's own framing (kept verbatim in the RFC) that a comment being present isn't
    a reason to skip it; RFC 0087's real `description` property is never overwritten, only
    supplemented. Design correction found *before* writing code (reading `semantic`/`ledger`
    source directly, not assumed): this runs as a post-`commit` step against `&dyn KnowledgeStore`
    (`commit_rollups`/`commit_data_lineage`'s own architectural slot), not a `CompilerPass` — this
    pipeline's ledger versions whole objects, not patches, so a bare partial-object write could
    have silently regressed real structural properties another pass wrote. Opt-in
    (`[llm-description]`, `scope` defaults to the cheaper `"modules"`), cost-gated (`ekos commit
    --yes` to skip the confirm prompt), same UX `--prose` already established. Live-verified real,
    zero-API-cost end-to-end against a real local Ollama model (`llama3:latest`): real grounded
    overviews on documented and undocumented symbols alike, real `Architecture style` populated,
    `Purpose` honestly left uncomputed when no real README existed to ground it. Two real bugs
    found and fixed by that live run (neither caught by 17 passing unit tests): a `File`-path
    resolution gap in multi-`[observe] paths` workspaces (the real analytics project's own shape),
    and a pre-existing Ollama model-selection bug (`from_env` vs. `from_env_with_model`) this
    session's own new code had copied from `docs.rs`/`marketing.rs` — both still have it,
    flagged but not fixed (out of this session's scope). Still deferred: the `Risk` KIR kind +
    `## Major risks`, `## Architecture confidence` from a real LLM judgment, Python/JS
    `source_span` capture.

- **Real gaps found live 2026-08-23 analyzing the analytics project's backend, fixed same session**
  (`devlog_92`):
  - [x] `elixir_analyzer.rs`: a multi-target `alias X.{A, B}` form (single-line or wrapped across
    several real lines, `mix format`'s own common shape) was creating one phantom `DependsOn` edge
    to the bare shared prefix (never `defmodule`'d anywhere — a real contentless entity page)
    instead of real edges to each real leaf module. Fixed via a new pre-scan
    (`prescan_multi_alias_targets`), 2 new tests, live-verified against the real analytics backend.
  - [x] `docs-gen`'s `## Component View` always said "no crate directory matched" for any non-Rust
    workspace (zero `Crate` objects ever compile without a `Cargo.toml`) — now falls back to
    listing real compiled `Rollup`s directly when no `Crate`s exist, clearly labeled as a fallback.
  - [x] `## System Decomposition` was summary-only (aggregate file counts per layer, no drill-down)
    — new `### Layer Breakdown` subsection lists which real `Rollup` contributes how many files to
    each layer; a rollup with real members in more than one layer (live-confirmed real case:
    `priv/tracker/js/p.js`, a compiled frontend asset inside an otherwise-backend `priv/`) is
    listed honestly under every layer it actually touches.

- **Real gaps found live 2026-08-23 implementing RFC 0088, fixed only where in scope**
  (`devlog_93`):
  - [x] `llm_description.rs`: `File.name` is relative to its own `[observe] paths` entry, not the
    workspace root, in any multi-path workspace — reading a real source file needs the real
    `"project"` property (RFC 0079) joined back on first. Fixed (`real_file_path`); found by a
    real live run reporting 0 described objects despite real `source_span` data being present.
  - [x] `commit.rs`'s own new Ollama provider selection fixed to use `from_env_with_model`
    (`config.llm.model` was being silently ignored, always falling back to the hard-coded
    `llama3.1:8b` default).
  - [ ] **Not fixed, flagged only** — `docs.rs::select_llm_provider_for_prose` and
    `marketing.rs`'s equivalent have the exact same `from_env` (not `from_env_with_model`) bug;
    `recover.rs` already has the correct fix. `--prose` and `ekos marketing publish` against a
    configured non-default Ollama model silently use the wrong model today. Out of RFC 0088's own
    scope to fix; tracked here so it isn't rediscovered from scratch. **Live-reproduced 2026-08-31**
    demoing real `docs generate --layout solution-architect --prose --yes` against this repo's own
    ledger: with `[llm] model = "llama3:latest"` configured but no locally-pulled
    `llama3.1:8b` (the hardcoded default this bug falls back to), the findings-memo prose call
    failed with a real `api error 404: model 'llama3.1:8b' not found` — handled gracefully (a
    printed warning, deterministic findings kept, not a crash), but confirms the gap is real, not
    theoretical. Setting `OLLAMA_MODEL=llama3:latest` (the env-var override `from_env` does check)
    worked around it for the demo and produced real grounded prose.
  - [x] **A different, adjacent bug found and fixed the same day**: `ekos docs generate --layout
    curated --prose` silently ignored `--prose` entirely — `generate_curated` takes no
    `prose`/`yes` parameters at all, so the flag was accepted but did nothing: byte-identical
    output with or without it, no warning, no error. Only `--layout objects` and `--layout
    solution-architect` ever actually wired `--prose` through. Fixed by rejecting `--prose` for
    `--layout curated` with a clear error (`--prose is not yet supported for --layout curated —
    use --layout objects or --layout solution-architect`) instead of pretending to honor it —
    matching `select_llm_provider_for_prose`'s own stated contract ("a user who asked for it wants
    real output or an honest failure, not silent placeholder prose"). Building real per-page
    prose for curated (project-wide overviews, a different grounding shape than the existing
    per-object case) is a separate, unscoped feature, not attempted here. New test
    `generate_curated_with_prose_errors_clearly_instead_of_silently_ignoring_the_flag`.

- **Real gap found live 2026-08-23 testing RFC 0088 against a real, deliberately small subsystem
  scope (`lib/plausible/auth`, 15 files) — fixed at its actual source, not worked around a second
  time** (`devlog_94`):
  - [x] `build.rs`'s RFC 0079 `project_key` (the mechanism that lets a real disk read reconstruct
    a `File` object's real directory prefix) was only ever written when `[observe] paths` listed
    *more than one* entry — conflating "the truly common `paths = ["."]` case" with "a single
    scoped subdirectory" (`paths = ["lib/plausible/auth"]`, or this repo's own `paths = ["src"]`
    test fixture), where a real, non-empty prefix exists but was silently dropped with no property
    left to recover it. Fixed: `base != cwd` replaces the entry-count check — still empty for the
    real `paths = ["."]` case (no id migration), now also correct for a single non-`"."` entry.
    Found by RFC 0088's own real symbol descriptions reporting 0 described despite real compiled
    `source_span` data existing — the analytics project's own real backend-only config (8 observe
    path entries) never triggered this, only a smaller, more targeted real scope did.

- **Two more real bugs found running RFC 0088 at real, full scale (1,066 real modules, not a small
  test subset) against the real analytics backend** (`devlog_95`):
  - [x] `plugins/file/src/lib.rs`: a single bare *file* (not directory) used as its own
    `[observe] paths` entry — a real, already-used shape (this project's own backend-only config:
    `mix.exs`, `mix.lock`, `README.md`, `CHANGELOG.md`, all four) — got a silently empty
    `name`/`path`, since `WalkDir::new(root)` yields exactly one entry equal to `root` itself when
    `root` is a file, and stripping a path from itself leaves `""`, not an `Err` the existing
    fallback caught. Fixed: falls back to the file's own basename. Found live: `Architecture.md`'s
    new `Purpose` field read as a real but wrong document's content.
  - [x] `llm_description.rs`'s `describe_project`: README detection was a loose
    `.contains("readme")` substring match — matched a real vendored file
    (`ua_inspector/ua_inspector.readme.md`, a real upstream license file this project bundles)
    ahead of the real top-level README. Fixed: `is_real_readme_name` matches only a basename whose
    own stem equals `readme` exactly.
  - Live-verified: the user's own flagged real page, `PlausibleWeb.RequireAccountPlug`, now has a
    real, accurate AI-Assisted Overview (1,062/1,066 real modules described, 4 errors, real local
    `llama3:latest`, zero API cost, ≈2h40m real elapsed time for the full backend at
    `scope = "modules"`).
  - [x] Sibling bug: the `plugins/file` fix above only fixed `File`-kind objects — the real
    `README.md` `Document` object (produced by `plugins/localdocs`, what `describe_project` actually
    reads) was still empty-named afterward. Grepped the whole codebase for the same
    `WalkDir::new(root)`/`abs_path.strip_prefix(root)` pattern and found it independently duplicated
    in **six more** Observer plugins: `localdocs`, `pentaho`, `javascript`, `python`, `elixir`,
    `rust` — none exercised against a bare-file `[observe] paths` entry before now.
    `crates/simulation/src/ingest.rs` already had a comment documenting this exact bug class as a
    known workaround (always scan the whole directory, never a single file) without ever
    root-causing/fixing the underlying plugins. Fixed identically in all six, one new regression
    test each. Full workspace gate re-run clean: `cargo fmt`, `build --workspace`,
    `clippy --workspace -- -D warnings`, `test --workspace` (all green), `tests/integration` (3/3).
    Not yet re-verified live against the real analytics backend end-to-end (next: fresh full
    `[llm-description]` re-run, ~2-3hr real cost, since a fresh ledger drops all cached
    `ai_evidence_hash` hits).

- **Re-verified the sibling-plugin fix cheaply against a small real scope instead of the full
  ~2-3hr backend run** (`analytics/lib/ip`, 2 real Elixir modules + `README.md`) — confirmed
  `README.md` resolves correctly for both `File`/`Document` kinds and `Architecture.md`'s `Purpose`
  reads the real project purpose. This surfaced two more real, previously-undiscovered gaps
  (`devlog_96`/`devlog_97`), both fixed, tested, and pushed:
  - [x] `elixir_analyzer.rs`'s `extract_doc_comments` only matched `@doc` when `def`/`defp` was the
    *literal next source line* — any `@spec` (the standard, near-universal real Elixir convention:
    `@doc` above `@spec` above `def`) or blank line in between silently broke the match. Every
    public function's real doc comment in `lib/ip/tools.ex` was lost before this fix. Fixed: skips
    blank lines and single-line `@spec ...`/`@spec(...)` lines before keying the match. 2 new tests,
    all 261 `ekos-recovery` tests pass.
  - [x] RFC 0089 (new, filed + implemented): symbol/module entity pages never showed which real
    file they're defined in — `"Based on"` only ever renders the *immediate* `Contains` parent (a
    symbol's module, not the file two hops up). Added `resolve_defining_file`/
    `build_contains_parent_map` (real graph walk, zero LLM) and a `**Defined in:** \`file\` (lines
    X–Y)` line under `## Definition`, wired into both `docs generate` layouts. Renders file-only
    when no `source_span` exists (e.g. a multi-clause function whose spannable clause never got
    captured), and nothing at all when neither resolves. 5 new `ekos-docs-gen` tests.
  - Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`) and
    `tests/integration` clean after both fixes; live-verified on the real generated pages.

- **First real analysis of a new project this session (`pdf-reader/backend/app`, real FastAPI/
  PyMuPDF/Tesseract, 15 files) surfaced a real RFC 0088 gap for Python specifically** (`devlog_98`):
  - [x] `python_analyzer.rs` never captured `source_span` (Rust/Elixir only at RFC 0088's own
    launch) — every `PythonSymbol` was silently, honestly skipped by `llm_description.rs` regardless
    of `[llm-description] scope`, with nothing surfacing why. Fixed: `line_number`/`item_span` (byte
    offset via `rustpython_parser::Ranged` → 1-indexed line, since Python has no `syn::LineColumn`
    equivalent) wired into both `add_symbol` call sites (`FunctionDef`/`ClassDef`). 4 new tests, all
    21 `python_analyzer` tests pass, full workspace gate + `tests/integration` clean.
  - Most of `pdf-reader`'s real route-handler/service functions genuinely have no docstring —
    `## Definition` correctly stays "Not documented in source" for those; this fix only unblocks the
    separate, opt-in `## AI-Assisted Overview` section from running for Python symbols at all.

- **RFC 0090 (filed and implemented same-day, 2026-08-24): `--layout solution-architect`, a
  team-facing `docs generate` bundle** — `devlog_99`:
  - [x] `render_dependency_risk_report`/`render_onboarding_guide`/`build_findings_evidence`+
    `render_findings_memo` (new `ekos-docs-gen` functions) and `generate_solution_architect`/
    `enrich_findings_memo` (`crates/cli/src/commands/docs.rs`) — `DependencyRiskReport.md` (real
    `Crate.version`/npm `version_spec`/`dev_dependency` versions, `DependsOn` fan-in concentration
    ranking, an honest "CVE/license data not available" section), `OnboardingGuide.md` (real
    `Crate.path` repository layout, link-through to `Architecture.md` for CI/CD and subsystem
    detail rather than re-listing it), and `FindingsMemo.md` (real `ArchitectureGap` objects +
    undeclared crate versions + doc-comment coverage gaps, grouped by kind; `--prose` layers an
    LLM executive summary *above* the deterministic list, never replacing it — reuses the existing
    `--prose`/`--yes` flow, no new flags). 18 new `ekos-docs-gen` tests + 5 new `crates/cli` tests,
    full workspace gate clean, `tests/integration` 3/3.
  - Live-verified against this repo's own real committed ledger: real crate names/fan-in counts
    (`serde_json` 132 dependents), and a real, honestly-surfaced finding (`1625/1625 RustSymbol`
    objects with no captured `description`) cross-checked against this repo's own already-generated
    `doc/entities/rustsymbol/re/render-readme.md` — confirmed a genuine gap in this ledger snapshot
    (predates or wasn't recommitted since RFC 0087's doc-comment capture), not a bug in the new
    code. `--prose` path verified via `MockLlmProvider` + the no-credentials error path only (no
    `ANTHROPIC_API_KEY` in this environment), matching `--layout objects --prose`'s own existing
    test-coverage convention. See RFC 0090 for the three open-question decisions (Findings
    evidence lives in `docs-gen`+CLI, not `recovery`; one bundled layout reusing existing flags,
    not new `--sections` flags; no speculative `vulnerabilities` field reserved) and explicit
    non-goals (CVE feeds, git churn/hotspot, coverage %).

- **Real redaction false positive found live against `pdf-reader`, 2026-08-24** (`devlog_100`):
  - [x] `ekos_common::redaction`'s generic `api_key|secret|...=value` pattern matched a real,
    legitimate keyword argument referencing a config value (`api_key=settings.azure_openai_api_key`
    in `services/ai_service.py`), truncated its match at the first `.` (outside the old char class),
    and spliced a colon-bearing `[REDACTED:...]` placeholder mid-expression — corrupting the file
    enough that it failed to parse and silently dropped all 8 of its real functions from the ledger,
    with no signal beyond a buried `recover.log` warning line. Fixed: the value char class now
    includes `.`, and a match whose captured value is a dotted chain of plain identifiers (a code
    reference, not a secret literal) is left untouched entirely. 2 new `ekos-common` tests, full
    workspace gate clean.
  - Two more real gaps found while assembling a combined system diagram for `pdf-reader`
    (documented this session, first one since fixed 2026-08-25 — see below): (1) [x]
    `python_analyzer.rs`'s `add_import` never resolved `from package import submodule` to the
    submodule, only the package — so `from app.services import ai_service` compiled as
    `DependsOn → app.services`, coarser than the real source. Fixed, `devlog_105`. (2)
    [x] `RelationshipKind::Extends` (class inheritance) had zero producers across every
    analyzer and zero `docs-gen` consumers — the real blocker behind wanting an auto-generated
    class-level architecture diagram. RFC 0092 filed and implemented 2026-08-25, see below
    (`devlog_108`).

- **RFC 0079's `project_key` fix (2026-08-23, `build.rs`) never propagated to `recover.rs`'s own
  duplicate copies of the same logic — found live, 2026-08-24** (`devlog_101`):
  - [x] `dependency_analyzer.rs` never applied project-id qualification at all (a bare, unqualified
    `file_kir_id(rel_path)`, with `rel_path` relative to `cwd` besides), so every `DependsOn` edge
    it emitted pointed at a `File` id that only ever existed in a `paths = ["."]` workspace —
    silently orphaned relationships (`SEM002: unknown from-id`) and a `## Technology Inventory`
    that could detect a technology but never resolve which file used it. `package_json_analyzer.rs`
    had the same `cwd`-relative-path bug plus a second, independent one: its `recover.rs` collection
    loop still used the pre-fix `observe_paths.len() > 1` condition. New shared
    `ekos_common::project::project_key_for_base` (matching `build.rs`'s real `base != cwd` rule) is
    now the single source of truth `build.rs` itself calls too, plus fixes to both `recover.rs`
    collection loops and `dependency_analyzer.rs`'s id computation. 4 new tests, full workspace gate
    clean. `crate_topology_analyzer.rs`/`cicd_analyzer.rs` had the identical bug class, closed
    2026-08-25 — see below (`devlog_104`).
  - [x] Separately, `dependency_analyzer.rs`'s `PATTERNS` table gained an `OpenAI API` row (no AI-
    provider SDK had a row at all) — named to avoid a real, live-found identity conflict: a bare
    `"OpenAI"` Technology name case-insensitively collides with the `PythonModule` object the same
    `import openai` also produces, and `ekos resolve` correctly refuses to silently merge across
    kinds.
  - Live-verified against `pdf-reader` (`paths = ["backend/app/api"]`): before the fix, `##
    Technology Inventory` showed `used by: _no linked files_`; after, `used by: ai.py`. A real,
    separate discrepancy noted but not chased: `compile.log`'s `SEM002` warnings still fire on ids
    that now resolve correctly via `ekos query object` — `resolve`'s own stage reports 0 conflicts
    and correct counts, so `ekos_semantic`'s compile-time validation appears to check against a
    narrower object set than what actually lands in the ledger. Rendered output is correct either
    way, so left for a future session.

- **Four more real docs-gen gaps found live against `pdf-reader`'s whole-project scope
  (`backend`+`frontend`+`README.md`), fixed same-day, 2026-08-25** (`devlog_102`):
  - [x] `system_context_graph` (`docs-gen/src/lib.rs`) required a `Custom("Crate")`-origin
    `DependsOn` edge — always empty for any non-Rust project regardless of real `Technology`
    data. Fixed: accept any origin when no `Crate` objects exist, matching `## Technology
    Inventory`'s existing behavior; strict Crate-only requirement preserved when Crates do exist.
  - [x] `group_key_for` (`semantic/src/rollup.rs`) treated a `"project"` property as a terminal
    group key, collapsing every file under one `[observe] paths` entry into one flat rollup
    regardless of real subdirectory structure. Fixed to combine `project` + depth-limited `path`.
    **Caught a real off-by-one in the first attempt via live re-verification**: `depth` (default
    3) is calibrated for a workspace-root-relative path; a project-relative path is already one
    level shallower, so `depth - 1` is the correct sub-depth — without it, `take(depth)` on a real
    3-segment project-relative path (`"app/api/ai.py"`) grabbed the filename itself, producing
    zero rollups. A new regression test now uses the real default depth and real path shapes
    specifically because the first test (using a smaller hand-picked depth) passed while the real
    call site still failed.
  - [x] `## Crate & Workspace Topology` had no non-Rust fallback; `## Component View` already did
    (found live once before, 2026-08-23, a different real project) but it was never mirrored into
    this sibling section. Factored the shared fallback into one function both sections call now.
  - [x] `describe_project` (RFC 0088) produced self-referential "purpose" text for a real project
    on a weak local model — mitigated (not guaranteed-fixed) with a real `workspace_name` prompt
    anchor and an explicit anti-self-reference instruction. Live-verified improvement (stopped
    describing EKOS itself), then found a *second* real bug while re-verifying: with two
    legitimate `README.md` files in scope (project root + `frontend`'s unmodified Vite scaffold
    template), a bare `.find()` picked whichever came first in iteration order. Fixed with a
    path-depth preference — then found, re-verifying *that* fix, that both real README.md Document
    objects have zero path separators (one from being observed via its own single-file `[observe]
    paths` entry, one from being the immediate child of the `frontend` entry), so the depth
    preference can't break the tie; only one `Custom("Document")` object named `"README.md"`
    survives in the ledger despite both files being real and processed — misdiagnosed at the time
    as a `local_docs_analyzer.rs` id-collision; [x] root-caused and fixed 2026-08-25, see below
    (`devlog_106`) — it was never an id collision, `DefaultResolver` was missing `Document` from
    its blanket kind-exclusion list (the depth-preference fix is kept regardless — real, harmless
    improvement for the general case).
  - Also found live: 5 real identity conflicts in the whole-project ledger —
    `react`/`vite`/`react-router-dom`/`pdfjs-dist`/`@vitejs/plugin-react` each exist as both a
    `Technology` (`package_json_analyzer.rs`, one per declared npm dependency) and a `JsModule`
    (the JS/TS structural analyzer, one per real import) object. Same cross-kind name-collision
    shape as the earlier `openai` case, but here it's each analyzer's own default behavior
    colliding, not one pattern-table row to rename. [x] RFC 0093 filed and implemented 2026-08-25
    — see below (`devlog_109`).
  - 15 new/updated tests, full workspace gate clean, `tests/integration` 3/3. Live-verified through
    4 full `.ekos/` rebuild cycles against `pdf-reader`'s real whole-project ledger: `## System
    Context` lists all 12 real technologies; `## Subsystems`/`## Component View`/`## Crate &
    Workspace Topology` show 7 real per-directory rollups instead of 2 flat blobs.

- **RFC 0091 (filed and implemented same-day, 2026-08-25): SQLAlchemy ORM model recognition** —
  the last previously-deferred `pdf-reader` gap, resolved on request (`devlog_103`):
  - [x] `python_analyzer.rs`'s existing `ClassDef` handling now also recognizes a real SQLAlchemy
    declarative model (`__tablename__ = "..."` present) and compiles a real `ObjectKind::Table`
    object (real column names + best-effort `data_type` hints + real `ForeignKey` edges resolved
    within the same file) alongside its existing, unchanged `PythonSymbol` object. Reuses
    `sql_analyzer.rs`'s exact `columns`/`ForeignKey` property/id conventions, so a small companion
    `docs-gen::render_data_architecture` fix (real column names were compiled but never rendered,
    for *either* origin) needed zero origin-specific branching. Python/SQLAlchemy only — Django/
    other ORMs/languages are real, deferred extensions, not attempted without a real project to
    verify against. 8 new tests, full workspace gate clean, `tests/integration` 3/3.
  - Live-verified against `pdf-reader`'s real `db/models.py`: 3 new `Table` objects (`documents`,
    `page_cache`, `translation_cache`) with real, correct columns; `## Entity Relationships` now
    renders a real ER diagram matching the real `ForeignKey("documents.file_hash")` in source;
    `translation_cache` correctly shows 0 FK edges (it has none), not force-fit to match its
    siblings. `## Data Architecture`/`## Entity Relationships` were both previously empty/gap-only
    for this project (no raw SQL DDL anywhere in scope) — both now render real content.
  - Caught one real bug via live testing against the actual source rather than a hand-simplified
    fixture: SQLAlchemy allows a bare, uninstantiated type reference (`mapped_column(Integer)`, no
    parens) alongside the called form (`mapped_column(String(64))`) — the first `type_hint`
    implementation only handled the called form.

- **RFC 0079's `project_key` gap closed for `crate_topology_analyzer.rs`/`cicd_analyzer.rs`,
  2026-08-25** (`devlog_104`) — the one item `devlog_101` explicitly deferred for lack of a real
  Cargo/CI-workflow test project:
  - [x] `recover.rs`'s `cargo_manifests`/`cicd_workflows` collection loops, `crate_topology_analyzer.rs`
    (`Crate`/`ArchitectureGap`/`Claim` ids — `technology_kir_id` deliberately left unqualified,
    external crates.io deps are global/shared, not project-scoped), `cicd_analyzer.rs` (`Pipeline`
    ids), and `architecture_reasoning.rs`'s test helper all fixed with the identical
    base-relative-path + `project_key_for_base` pattern `devlog_101` already established. 2 new
    regression tests (real id recomputed and asserted, same shape as `devlog_101`'s precedent), full
    workspace gate clean, `tests/integration` 3/3.
  - Live-verified with a real target since `pdf-reader` has neither Cargo manifests nor CI
    workflows: a scratch `ekos.toml` with `[observe] paths` pointing at two real absolute EKOS crate
    directories (`crates/common`, `crates/kir`) plus the real repo's own `.github/workflows`, run
    through the full pipeline. Independently recomputed both a `Crate` and a `Pipeline` object's
    real id in Python (`uuid.uuid5`) from the qualified-path formula and confirmed byte-for-byte
    matches against the real ledger objects.
  - Same `SEM002` warning-volume discrepancy `devlog_101` flagged reproduced on this run too (still
    not investigated — separate open item below).

- **Python `from package import submodule` now resolves to the submodule, 2026-08-25**
  (`devlog_105`) — second item on the gap-closure list, deferred since `devlog_100`:
  - [x] `python_analyzer.rs`'s `ImportFrom` handling emitted one `DependsOn` edge to the bare base
    module regardless of which names were imported, so `from app.services import ai_service` and
    `from app.services import db_service` collapsed onto the same `app.services` object — losing
    the real distinction the source draws. Fixed: one edge per imported name, qualified
    `<module>.<name>`; a star import (`from pkg import *`) still falls back to the bare module,
    the only real fact available in that case. 1 test updated, 2 new.
  - Live-verified against the exact real line that motivated the gap
    (`backend/app/api/ai.py:7: from app.services import ai_service`, rebuilt whole-project scope):
    `ekos query find "ai_service"` now returns a real `app.services.ai_service` `PythonModule`
    object with a real `DependsOn` edge from `ai.py`'s `File` object; no bare `app.services`
    import-derived object exists anymore.
  - Full workspace gate clean, `tests/integration` 3/3.

- **The "`local_docs_analyzer.rs` id-collision" (`devlog_102`) root-caused and fixed, 2026-08-25**
  (`devlog_106`) — third item on the gap-closure list; it was never an id collision:
  - [x] `DefaultResolver`'s blanket kind-exclusion list (`crates/identity/src/lib.rs`) — already
    covering `Section`/`TransformNode`/`RustSymbol`/`RustModule`/`PythonSymbol`/`PythonModule`/
    `Crate`/`Claim`/`ArchitectureGap`/`ElixirModule`/`ElixirSymbol`/`JsModule`/`JsSymbol`, the
    exact obligation CLAUDE.md's own crate-map names explicitly for every new self-identified
    `Custom(_)` kind — was missing `Custom("Document")`, the ninth kind to hit this exact failure
    shape. Two real, distinct `README.md` files (project root + `frontend`'s Vite scaffold) share
    an *exact* normalized name, so the same-kind 1.0 structural-score fallback pushed them to
    confidence 1.00 — RFC 0063 auto-merges exact matches without review, and `ekos compile`
    silently dropped one of the two real files from the compiled CKM every run. Fixed: `Document`
    added to the list. 1 new regression test, 2 pre-existing tests updated (both had used
    `Custom("Document")` as their own example of a kind the exclusion *doesn't* apply to).
  - Live-verified, including a real methodological trap caught mid-verification: `ekos compile`'s
    own pass-level cache (keyed on upstream content, not the compiling code's version) silently
    served the stale pre-fix CKM after only rebuilding the binary — object counts looked identical
    before/after until specifically cross-checked. `ekos clean` alone then left a second
    inconsistency (`build`'s own re-scan fingerprint isn't cleared with it, so a `clean`+`build` can
    skip re-scanning against a now-empty artifact store). Full `rm -rf .ekos` + `init` was the
    reliable reset. After that: `ekos compile` reports 148 objects (was 147); `ekos query object`
    on both real `README.md` `Document` ids confirms genuinely distinct, correctly-attributed real
    content (the actual project README vs. the actual untouched Vite scaffold text).
  - Full workspace gate clean, `tests/integration` 3/3.

- **`compile.log`'s `SEM002` warning noise root-caused and precisely classified, 2026-08-25**
  (`devlog_107`) — fourth item on the gap-closure list, flagged three prior times
  (`devlog_99`/`devlog_101`/`devlog_104`) without ever being traced:
  - [x] Not a bug in identity resolution — `ekos_semantic`'s CKM validation checks relationships
    against a genuinely narrower object set by architectural design (`File` objects are written
    straight to the ledger by `ekos build`, never through the `KnowledgeArtifact`s the compile
    stage reads; already documented in-line since RFC 0044, just never surfaced in the diagnostic
    text itself). New `CkModel::dangling_relationship_target_ids()` exposes the same set
    `validate()` already computes, as real ids; `compile.rs` (which already has ledger access)
    cross-references them against the ledger's real `File` objects and reports a classified count
    instead of one opaque number. 4 new tests.
  - Live-verified: `pdf-reader`'s 184 raw warnings (23 distinct dangling ids) now report as "22
    expected File-object references ... 1 other(s)". The 1 remaining traced to a *different*,
    already-independently-documented gap (`git_analyzer.rs`'s `OwnedBy` edges point at a synthetic
    commit-subject id, never a real `File` — `docs-gen`'s own `## Ownership` section text already
    names this exact limitation and what a real fix would need) — correctly left unfixed here
    (a scoped, RFC-worthy feature, not a bug), and correctly *not* silently absorbed into
    "expected" by this fix's classification.
  - Full workspace gate clean, `tests/integration` 3/3.

- **RFC 0092 (filed and implemented same-day, 2026-08-25): class inheritance
  (`RelationshipKind::Extends`), Python v1** — fifth item on the gap-closure list (`devlog_108`):
  - [x] `python_analyzer.rs`'s existing `ClassDef` visit (already reused for RFC 0091's
    `__tablename__` detection) now also emits a real `Extends` edge per base class that resolves
    to another real, same-file `PythonSymbol` class (`known_classes` pre-pass, mirrors RFC 0091's
    `known_tables` exactly). An unresolvable base (imported, not locally defined — `BaseModel`,
    `DeclarativeBase`) is honestly left unmapped, same "no fabrication" discipline RFC 0091
    established for `ForeignKey`. Python only — JS/TS `class X extends Y` is the same real shape
    and a legitimate future extension, not attempted without a live target (`pdf-reader`'s
    frontend is entirely functional-component React, no class declarations in scope). 6 new tests.
  - The RFC's first draft wrongly claimed `docs-gen`'s object pages get a dedicated `### Extends`
    section — corrected in place after live verification showed relationships are actually grouped
    into 4 pre-existing structural buckets (`Based on`/`Contains`/`Used in`/`Dependent on`), not
    one section per literal kind; `Extends` lands in `### Dependent on`. The real kind *is* still
    visible with zero `docs-gen` changes, via the same page's Mermaid diagram edge label
    (`Document -->|Extends|-> Base`) — a real, dedicated `### Extends`-style section is left as
    future `docs-gen` work once a second language's worth of real data exists to design it against.
  - Live-verified against `pdf-reader`'s real `db/models.py`: compiled relationship count went
    189 → 192 (the 3 real `Document`/`PageCache`/`TranslationCache` → `Base` edges); `ekos query
    neighbourhood` confirms the real edges and confirms `TranslateRequest(BaseModel)` correctly
    produces no fabricated edge.
  - Full workspace gate clean, `tests/integration` 3/3.

- **RFC 0093 (filed and implemented same-day, 2026-08-25): `Technology`/`JsModule` cross-kind
  conflict false positive** — sixth item on the gap-closure list (`devlog_109`):
  - [x] `DefaultResolver`'s conflict detector flagged every real `Technology`
    (`package_json_analyzer.rs`, declared dependency) that shares a name with a real `JsModule`
    (`javascript_analyzer.rs`, imported specifier) — the expected shape for *every* real JS/TS
    dependency that's both declared and imported, not a genuine ambiguity, and `ekos resolve` (no
    `--force`) refuses to proceed at all when any conflict exists. New
    `is_expected_technology_jsmodule_pair`: excludes exactly a `{Technology, JsModule}` group
    (a third kind still conflicts) where every `JsModule` looks like a real bare package specifier
    (not starting with `.`/`..`/`/`, the same rule Node's own resolution uses) — not a merge, only
    stops the pair being *reported*; a relative-specifier `JsModule` sharing a `Technology`'s name
    still correctly conflicts. 3 new tests.
  - Live-verified against `pdf-reader`'s real whole-project ledger: `ekos resolve` (no `--force`)
    conflict count dropped from 5 to 0 and now exits 0, for the first time all session on this
    project. `compile`/`commit` object/relationship counts unaffected (148/192) — this fix only
    changes conflict *reporting*, not what gets merged or compiled.
  - Full workspace gate clean, `tests/integration` 3/3.

- **RFC 0094 (filed and implemented same-day, 2026-08-25): `Custom("Risk")` KIR kind, Observed
  Concentration Risk v1** — seventh item on the gap-closure list (`devlog_110`):
  - [x] `Architecture.md`'s Executive Summary `**Major risks:**` line had said "not yet computed —
    no `Risk` KIR kind exists yet" since the section was first written. New `Custom("Risk")` kind;
    one v1 rule (an object with 3+ real compiled `DependsOn` dependents — `risk_type: "observed"`
    only, no inference/fabricated severity), computed inside `SemanticCompilerPass::run()`
    (`crates/semantic`, needs the whole-graph `DependsOn` view only available post-resolution),
    kind-agnostic (not `Technology`-only — reused `DependencyRiskReport.md`'s existing render-time
    fan-in computation as a starting point but widened it). 7 new tests.
  - Live-verified against `pdf-reader`'s real whole-project ledger — the positive case turned out
    stronger than planned: widening past `Technology`-only surfaced 11 real `Risk` objects
    (`fastapi.HTTPException`, a shared `app.db.session.get_db` DI helper, a shared frontend
    `../api/client` module, ...), rendered as real content in `Architecture.md`'s Executive
    Summary, no scratch/self-verify scope needed.
  - A real, separate, pre-existing gap found while verifying (not fixed here): `python_analyzer.rs`'s
    `add_import` never attaches evidence to its `DependsOn` edges at all, so a real `Risk` object
    derived from Python-sourced fan-in correctly has zero cited evidence — not a bug in this RFC's
    logic (which correctly forwards whatever evidence exists), a gap in what the underlying edges
    carry. Worth a future session: give `add_import`'s edges the same real evidence citation
    `dependency_analyzer.rs`/`crate_topology_analyzer.rs` already provide theirs.
  - Full workspace gate clean, `tests/integration` 3/3.

- **RFC 0095 (filed and implemented same-day, 2026-08-25): Architecture confidence wired into
  `docs generate`'s Executive Summary** — eighth and final item on the gap-closure list
  (`devlog_111`):
  - [x] `evaluate_architecture` (RFC 0065 Phase 3) already existed and was already used by `ekos
    architecture investigate` — never called from the plain `docs generate` path. Small wiring fix:
    `generate_curated` (`docs.rs`) now calls it and threads a new small local
    `ArchitectureConfidence` struct (`docs-gen`, mirrors `EvaluationReport`, avoids a real
    `ekos-recovery` dependency for this thin rendering crate — matches `LayerOverride`'s own
    precedent) into `render_architecture`. New `evidenced_total` field on `EvaluationReport` lets
    the renderer tell a real score apart from the evaluator's own vacuous `1.0` default (no
    `Crate`/`Claim`/`ArchitectureGap` objects at all — `pdf-reader` today) — renders an honest
    "not meaningfully computed" message instead of a misleading "100% confidence". 7 new tests.
  - **A real, previously-undiscovered bug found live verifying the positive case**: [x]
    `crate_topology_analyzer.rs`'s `dir_to_id` map was keyed by the *bare* manifest directory
    alone, so two crates from *different* `[observe] paths` projects that both have `Cargo.toml`
    at their own entry's root (`dir == ""` — the single most common real shape for a multi-project
    workspace built from standalone crate directories) silently collapsed onto one `Crate` object
    id. A real regression in the RFC 0079 fix `devlog_104` shipped earlier this session — missed
    because that fix's own verification only ever checked one crate's id in isolation, never
    checked whether a *second* crate in the same multi-path scope got a genuinely different one.
    Fixed: `dir_to_id` re-keyed by `(project, dir)`, all 4 use sites updated (including the
    internal path-dependency resolution site, which now correctly pairs a target directory with
    the *declaring* crate's own project — path dependencies never cross a project boundary). 1 new
    regression test reproducing the exact real shape.
  - Live-verified against `pdf-reader` (honest vacuous-case message) and a real scratch 2-crate
    multi-project scope (`crates/kir` + `crates/common`, EKOS's own real crates): after the
    id-collision fix, `Architecture.md` correctly reports "40% (completeness: 0% of 2 crate(s)
    classified..." — 2, not the pre-fix 1.
  - Full workspace gate clean, `tests/integration` 3/3.

- **2026-08-26 (`devlog_112`, no RFC — 4 real bugs found running EKOS's pipeline against its own
  repository for the first time this session)**:
  - [x] **Artifact id computed pre-redaction, data persisted post-redaction** (`build.rs`) — a
    redaction-engine improvement could never retroactively apply to already-observed content, since
    the unchanged raw bytes always re-derive the same (stale) id and `PackArtifactStore::write()`
    is skip-if-exists. Fixed: id now recomputed from the final, already-redacted content right
    before writing. 1 new test.
  - [x] **10 of 11 `recover.rs` artifact-id collectors never deduplicated by target** — only
    `collect_crypto_artifact_ids` had ever been fixed; every sibling (rust, python, elixir,
    javascript, github, clickhouse, confluence, localdocs, pentaho, git) reprocessed every
    historical artifact version forever, a fix-once-not-generalized gap the id-staleness fix above
    made newly visible. Fixed: shared `collect_artifact_ids_for_connector`, recency by real
    `ArtifactMeta.created_at`, all 11 collectors reduced to call it. 2 new tests.
  - [x] **Three independent bugs in `redaction.rs`'s `generic-assigned-secret` pattern**, each found
    only after fixing the previous one exposed the next real parse failure: asymmetric quote
    consumption (a lone trailing `['"]?` could eat a real closing quote with nothing to restore it,
    breaking string-literal syntax), no word-boundary guard (matched as a bare substring inside a
    longer real identifier, e.g. `api_secret` matching mid-token at `secret`), and whole-match
    replacement deleting syntactically-required struct-literal field names/separators. Fixed: a
    symmetric-quote-only alternation, a compound-identifier-aware boundary
    (`(?:[A-Za-z0-9]+[_-])*`), and value-only span replacement. 4 new tests; all 11 pre-existing
    tests unaffected.
  - Live-verified against EKOS's own real, previously-corrupted self-analysis history (not a
    scratch project — the only fix this session that *needed* real accumulated history to surface
    at all): full-repo `redact()` scan across 256+ real `.rs` files (0 broken, was 3), full
    from-scratch `.ekos/` rebuild (687 files, **0 `RUST003` warnings**, was 3). One legacy
    (id, corrupted-data) pair from 2026-08-21 could not self-heal by code fix alone (content-
    addressing's core invariant, once violated for a specific id, is permanent for that id) —
    resolved via a one-time full `.ekos/` reset, not a further code change.
  - Full workspace gate clean (101/101 test groups, 8 new tests), `tests/integration` 3/3.
  - Next: generate full curated + solution-architect documentation for EKOS's own repository now
    that `commit` (with real AI-Assisted Overviews via local Ollama) has run against the fixed,
    freshly-rebuilt ledger.

- **`docs/GAP_ANALYSIS.md` gap-closure plan (2026-08-26): six-RFC sequence for the "Runtime/
  Retrieval" backlog** — user asked to fix the whole Runtime/Retrieval item under "Promoted from RFC
  Non-Goals" above. Planned as RFC A (EKL `AS OF`/`COUNT`/`GROUP BY`) → B (MCP-scoped ledger read
  caching) → C (`ekos ask` streaming) → D (multi-turn `ekos ask` history) → E (`memory/`-path search
  boost) → F (embedding-based semantic search), smallest/most-grounded-first. **Full async
  `KnowledgeStore`/`Runtime` conversion explicitly excluded from this plan** — RFC 0005 already
  evaluated and rejected it for v0 in writing, and re-confirmed still correct (100% sync, both
  backends, 33 real call-site files) rather than blindly redone; revisit only if a concrete future
  consumer (e.g. an async MCP transport) needs it.
  - **Status as of 2026-08-26: 6 of 6 done (A/B/C/D/E/F).** All six are each a real, shipped,
    tested, live-verified increment.
    - [x] **RFC E — RFC 0101, `devlog_118` (2026-08-26).** The "unresolved design gap" the original
      plan flagged (no `memory/` path convention found anywhere in the codebase) turned out to be a
      research gap, not a design gap — the earlier full-repo search only checked `ekos/`'s own
      source tree, never `.claude/skills/memory/SKILL.md` (the real, concrete, already-in-production
      convention: `$WORKSPACE_ROOT/memory`, `<scope>--<type>--<keywords>.md` filenames) or this
      repo's own real, in-production estate-root `/home/legion/PycharmProjects/ekos.toml` (which
      genuinely observes `memory` as its own top-level `[observe] paths` entry today).
      `KirObject::is_under_memory_path` detects both real `[observe] paths` shapes this codebase
      supports (multi-project via RFC 0079's `"project" == "memory"` property; single-path via a
      literal `memory/` prefix on `"path"`) as a real path-segment check, not a substring one.
      `SearchIndex` (tantivy, the RFC 0016 default backend) gets a new `memory_path` field and an
      unconditional 5× boost `Should` clause alongside the existing per-term `Must` clauses — proven
      by a dedicated test to only ever re-rank documents that already matched the query on their own
      merits, never introduce a false positive. Deliberately scoped to `FactLedger` only, not the
      legacy SQLite/FTS5 backend (matches RFC 0097's precedent — real added scope for a backend
      already being phased out by RFC 0016's default-switch policy). A hardcoded convention, not an
      `ekos.toml`-configurable glob, was chosen for v1 — matches the existing architecture of every
      sibling property-key read next to it, and `ekos-kir` has no dependency path back to
      `ekos.toml`'s config types anyway. 6 new tests (4 `ekos-kir`, 2 `ekos-ledger`), full workspace
      gate clean, `tests/integration` 3/3. **Live-verified** against a real scratch workspace
      mirroring this repo's own real estate `ekos.toml` shape: `ekos query find "quadratic"` ranked
      all three memory-derived objects above all three ordinary-project objects, through the real
      CLI.
    - [x] **RFC F — redesigned, then RFC 0100 / `devlog_117` (2026-08-26).** The original
      full-embedding-search plan was replaced, by explicit user direction after a design discussion,
      with a far cheaper first step: RFC 0088 already generates real, evidence-grounded
      `ai_overview`/`ai_usage` prose at commit time — it just was never fed into search.
      `KirObject::indexed_content()` now includes it (zero new infrastructure — no vector store, no
      `EmbeddingProvider` trait, no ANN dependency). **Real bug found and fixed at the same time,
      not filed away**: `FactLedger::index_object` (the RFC 0016 default backend) had its own
      independently-maintained, already-incomplete reimplementation of the indexed-content field
      list — it never included `ocr_text` at all, silently breaking OCR'd-document search on every
      new workspace since RFC 0024 shipped. Fixed at the root by having it deserialize and call the
      real `indexed_content()` instead of a second, drifted copy — the third time this session found
      the identical "logic duplicated across the two ledger backends, one silently stale" bug shape.
      6 new tests, full workspace gate clean, `tests/integration` 3/3. **Live-verified**: a real
      compiled `ai_overview` for a Rust `main` function read "...prints a **greeting message**..." —
      a word absent from the function's own source and doc comment — and `ekos query find
      "greeting"` correctly found it, proving real search-by-concept, not just that the code
      compiles. Full embedding-based search (the original RFC F scope) is not abandoned, just no
      longer attempted first — real usage against this cheaper approach will show whether it's
      needed. A dedicated `search_aliases` LLM property and tantivy's built-in typo-tolerant
      `FuzzyTermQuery` were both named as related, real, smaller future follow-ons and deliberately
      not bundled into this RFC.
    - [ ] **Reframed 2026-09-01 into RFC 0118 — "Compiled-Knowledge Query Engine: SEARCH → QUERY →
      REASON"** (`ekos/docs/rfcs/0118-compiled-knowledge-query-engine.md`, umbrella, Draft). After a
      design discussion the retrieval story was repositioned around EKOS's actual differentiator, the
      Knowledge Compiler: *traditional RAG searches documents; EKOS queries compiled knowledge.*
      Three operations — **SEARCH** (BM25 + vector + entity resolution → RRF-fused ranked objects),
      **QUERY** (direct `fact(entity, attr)` over the existing `FactIndexes` EAV engine + named graph
      ops `dependents`/`path`/… — zero LLM), **REASON** (a Query Planner compiles the NL question into
      a typed `QueryPlan` IR, executes it, assembles a typed `EvidenceSet` with per-item provenance,
      and the LLM *explains* structured evidence instead of interpreting chunks). Design-only; no code.
      Per-phase impl RFCs authored just-in-time: **0119** the `KnowledgeStore::retrieve` seam ·
      **0120** RRF fusion + `ExactName` signal (also improves the RFC 0113 B5 shard-local-IDF merge) ·
      **0121** query understanding (entity resolution + rules-first intent) · **0122** the QUERY
      surface (fact lookup + per-`ObjectKind` fact schema + named graph ops) · **0123** REASON
      (`QueryPlan` + `EvidenceSet` + `AiRuntime` rework) · **0124** surface (EKL `SEMANTIC`, MCP
      `ekos_query`/`ekos_retrieve`, `ekos ask` wiring, `--explain`) · **0125** the vector arm
      (`EmbeddingProvider` + `VectorIndex`) — **gated** on usage data per RFC 0100's stated condition ·
      **0126** eval harness + telemetry (optional). Phases 0–4 are fully offline / zero-LLM. Computed
      staleness/drift (`Custom("Drift")`, code↔doc signature diffing) is deliberately a separate
      future RFC — it was going to be *0127*, but that number was taken by the Web Console umbrella
      (below); the staleness/drift RFC gets a fresh number (0128+) when authored, and these
      cross-references are re-pointed then. (Also noted: commit `e8e1ca3` claims an
      RFC 0117 for the dbt analyzer but no `0117-*.md` was ever filed — backfill needed.)
      - [x] **0119 (Phase 0)** — `KnowledgeStore::retrieve(&RetrievalRequest) -> RankedResults`
        seam, default wraps `find_objects` byte-identically; `Runtime::retrieve`; `AiRuntime` +
        `ekos query find` + MCP + EKL routed through it.
      - [x] **0120 (Phase 1)** — `rrf_fuse` (Cormack RRF, `k=60`) + `ExactName` signal in
        `ledger`; `FactLedger`/`PartitionedLedger`/`DistributedLedger` scored-merge → RRF (also the
        RFC 0113 B5 shard-local-IDF fix). `ekos query find "README"` now promotes the exact match
        on the fact engine, not just SQLite.
      - [x] **0121 (Phase 2)** — `runtime::retrieval::understand()`: mention extraction + fuzzy
        entity resolution (`ResolvedEntity`, Jaro-Winkler ≥ 0.82) + rules-first intent classifier
        (`QueryType` × `StructuralOp`), seeded from `extract_search_terms`.
      - [x] **0122 (Phase 3)** — `KnowledgeStore::fact(entity, attr)` (dotted-path resolver,
        default impl over `get_object`, all backends); `Runtime::fact`/`facts_of`; named graph ops
        `Runtime::dependencies`/`dependents`/`callers`/`related` + `graph_op(StructuralOp, …)`
        dispatch over `trace_impact`/`load_neighborhood`. Fact schema in analyzers +
        `FactIndexes` fast-path deferred (advisory). No CLI/EKL/MCP surface yet (that's 0124).
      - [x] **Full-stack test run + fixes, `devlog_149`** — an autonomous end-to-end run
        (`EKOS_FULL_TEST_PLAN_v2.md`, artifacts `test-runs/run-20260901T160842Z/`) of RFC
        0111/0113 + 0118/0119–0126 + 0013/0115 against a *partitioned* workspace. No BLOCKER; 8
        findings, 3 MEDIUM fixed the same session: **F3** `PartitionedLedger::retrieve` lost
        cross-partition `ExactName` promotion (added the cross-partition arm, matching the gateway);
        **F5** `ekos mcp serve --workspace <dir>` didn't load `<dir>/ekos.toml` (`resolve_config_path`
        helper); **F6** `ekos status`/`ekos ledger status` said "not initialised" on any partitioned
        workspace (added a partitioned/distributed branch). Still open: F2 (`ekos diff` empty for
        very-old `--from`), F4 (`arm_timings` empty on partitioned stores), F7 (inflected
        entity-mention resolution).
      - [x] **0126 (Phase 7), `devlog_148`** — the retrieval eval harness + per-arm telemetry.
        `ekos_runtime::retrieval_eval`: a checked-in graded query set (`reference_queries()`, ~30
        queries × 5 `QueryType`s), a hand-built reference estate (`seed_reference_estate` —
        Northwind tables + FK edges + code modules/symbols with `ai_overview` prose + doc sections)
        and its mock-embedded `VectorIndex` (`seed_reference_vectors`), pure metric fns
        (`recall_at_k`/`reciprocal_rank`/`ndcg_at_k`, unit-tested), `evaluate()` → `EvalReport`
        (Recall@10 / MRR / nDCG@10 overall — over the retrieval-shaped types only — and per type,
        plus intent-classifier accuracy), a `BASELINE` const + `check_regression`. **CI gate:**
        `crates/runtime/tests/retrieval_eval.rs` fails the normal `cargo test` job on a > 2% drop.
        **Scoreboard:** `benchmark/benches/retrieval_eval.rs` prints the table + times
        understand/retrieve (lexical vs hybrid). **Telemetry:** `RankedResults.arm_timings:
        Vec<ArmTiming{source, elapsed_ms, candidates}>`, populated by `FactLedger::retrieve` only
        (bracketed per arm, pure observability — byte-identical hits); surfaced in `ekos_search` /
        `ekos_retrieve` MCP results, lifted into `query_log::LogEntry.arm_timings`, and printed by
        `ekos query find --explain`. Deferred: the optional `contextual_score` identity signal.
      - [x] **0125 (Phase 6), `devlog_147`** — the vector/semantic arm. `EmbeddingProvider` trait
        in `recovery` (`Mock` deterministic-offline / `Ollama` / `OpenAI` / `Cached` disk-cache),
        `build_embedding_provider` mirroring `build_llm_provider`. `ledger::vector::VectorIndex` —
        a `SearchIndex` sibling at `<ledger-dir>/vectors/` (`meta.json`/`ids.bin`/`vectors.f32`/
        `tombstones.bin`/`last_tx`, L2-normalized-at-write brute-force cosine, `f32::from_le_bytes`
        over the bytes — no `bytemuck`/ANN dep), append-on-upsert + `compact()` past 0.3 tombstones,
        `dim`/`model` mismatch self-wipes (RFC 0103). Opt-in `[embeddings]` config (`enabled=false`
        default, same shape as `[llm-description]`); post-`commit` `embed_objects` pass runs last
        (after `run_llm_description`, so it can embed the `ai_overview` prose), incremental by object
        id, single-node only (no-op on SQLite / partitioned). Vector arm in `FactLedger::retrieve` —
        fires only when `req.query_embedding.is_some()` **and** an on-disk index's `dim` matches;
        absent/mismatched → silently skipped, `arms_run.vector=false` (RFC 0119 contract). Surface:
        `ekos query find --mode <lexical|vector|hybrid>`, MCP `ekos_search {mode}` (+ `arms_run` in
        the response), query embedded once in the CLI via `embed_query_blocking`. Distributed:
        `publish_aux("vectors")`/`fetch_aux("vectors")` reuse the `"search"` aux channel — the
        distributed `VectorSearch` RPC and `ekos ask`/EKL `SEMANTIC` vector wiring are deferred
        (RFC 0125b / fast-follow).
      - [x] **0124 (Phase 5), `devlog_146`** — the surface. `ekos ask` compiles the question
        through the REASON planner by default (`--classic` = the old `gather_context` path, implied
        by `--stream`; `--explain` prints the plan + evidence set). MCP `ekos_query` (compiled
        fact/graph answer, no LLM) + `ekos_retrieve` (plan + evidence + understanding, no LLM);
        `ekos_search` gains `limit`. EKL `SEMANTIC 'text' [LIMIT k]` — retrieval as a candidate
        set (rejects `FROM`/`AS OF`/`COUNT`/`Relationship` at parse time). `ekos query find
        --explain`. `AiRuntime::reason_with_history`. Also fixed a pre-existing `tests/integration`
        build break from commit 2896481 (`compile_worker_run` 5th arg).
      - [x] **0123 (Phase 4), `devlog_145`** — `runtime::reason`: the Query Plan IR (`PlanNode`
        Resolve/Search/Fact/Graph/Compose, `QueryPlan`), the offline rules planner (`plan` —
        fact-attribute questions route ahead of the RFC 0121 intent class; `PlannerTier`/`plan_with`
        stub seam for the future LLM tier), the executor (`execute` → typed `EvidenceSet` of atomic
        source-traceable `EvidenceItem`s, item cap 60, `RSN001`–`RSN005` diagnostics), and
        `AiRuntime::{plan, gather_evidence, reason}`. `ask`/`ask_stream`/MCP/EKL unchanged — cutover
        is 0124. Whole RFC 0118 series (0119–0123) fast-forwarded onto `main` this session
        (`30b37cb..c79b189`).
    - [~] **RFC 0127 — Web Console** (`ekos/docs/rfcs/0127-web-console.md`, umbrella, Accepted
      2026-09-02). A browser surface over a compiled workspace: the cross-system impact trace
      (the product's differentiating claim) has no visual form today. Per-increment impl RFCs
      just-in-time (0128+). **Phase 0 contracts landed, `devlog_150`:**
      - [x] **R1** — `ekos graph export`: the first bulk graph-extraction path in EKOS (every
        other read is per-object or `LIMIT 50`). `ekos_runtime::export_graph` — one pure
        read-only fn over `all_objects` + `all_relationships`, kind/rel-kind/min-degree filters,
        `--level aggregate` super-nodes (by kind or path prefix), degree-descending truncation
        reported in the payload. `--format json|ndjson`, deterministic modulo `generated_at`.
      - [x] **R2** — `ekos status --json` / `ekos ledger status --json`: one flat JSON object
        (entries/objects/relationships/evidence, backend tag, storage breakdown, mtime-proxy
        `last_write`). Text output unchanged (RFC 0116 parity kept). Added
        `KnowledgeStore::evidence_count` (real on sqlite/fact/partitioned; `Err` on the
        distributed gateway pending a fan-out RPC) + `Ledger::format_tag`.
      - [x] **R3** — `ekos_graph_export` MCP tool: thin wrapper over R1's fn so the console reads
        the graph over one transport (MCP TCP). Classified `Expensive` → opportunistically cached.
      - [x] **RFC 0128 — Phase 0 (part 2), `devlog_151`** (`ekos/docs/rfcs/0128-web-console-phase-0.md`,
        Accepted 2026-09-03): **R4** — `ekos mcp serve --tcp --tcp-token-file` (or `EKOS_MCP_TOKEN`):
        first line must be an `initialize` carrying `params._meta.token`, hand-rolled constant-time
        compare, `-32001 unauthorized` + close otherwise; token-less `--tcp` unchanged (RFC 0115
        back-compat); stdio never gated. **Python MCP client** — `web/api/app/mcp_client.py`,
        ~150 lines asyncio, raw NDJSON/TCP, no MCP SDK; `EkosMcpClient` + lazy per-workspace
        `ClientPool`; unwraps `{content:[{text}]}` tool results, one reconnect retry. **`web/`
        skeleton** — FastAPI app factory (`create_app`), pydantic-settings, `/api/health` +
        `/api/workspaces` + `/{id}/stats|graph|search` proxied to the MCP tools, static-token
        console auth via `secrets.compare_digest`; `runner.py`/`scheduler.py`/`commands.py`/
        `config_io.py` are Phase 1–4 stubs. Vite + React 18 + TS shell (one page). `docker-compose.yml`
        (api + ui; `ekos` binary + workspace bind-mounted; console spawns the MCP servers itself in
        Phase 1). New `web` CI job: build `ekos` release, ruff + pytest (`EKOS_BIN`-gated live test)
        for `web/api`, tsc + vite build for `web/ui`. E2E verified against this repo's own `.ekos/`.
      - [x] **RFC 0129 — Phase 1: shell + statistics, `devlog_152`** (`ekos/docs/rfcs/0129-web-console-phase-1.md`,
        Accepted 2026-09-03). Auth stays a single static `CONSOLE_TOKEN` (role split → Phase 3);
        `McpSupervisor` is its own module, separate from the Phase 3 job runner.
        - [x] **R5** `ekos doctor --json` — `{schema_version, ok, checks:[{name,status,detail}]}`,
          always exits 0. Text output byte-identical.
        - [x] **R6** `ekos ledger timeline [--json] [--bucket day|week|month] [--since]` —
          cumulative object/relationship counts bucketed by `KirObject::created_at`. Backend-
          agnostic (one `all_objects`+`all_relationships` pass), **no new `KnowledgeStore` method**,
          no per-backend branch. `--since` trims display only.
        - [x] **Logging fix** — `emits_machine_output()` routes `status --json` / `doctor --json` /
          `ekl --json` / `ledger status|timeline --json` / `graph export` logs to stderr (a latent
          R2 bug: tantivy log lines were interleaving with the JSON on stdout).
        - [x] **Console** — SQLite `Workspace` registry (SQLModel, 1 table; `WORKSPACES_JSON` is
          now just a seed); `McpSupervisor` (per-workspace `ekos mcp serve --tcp`, random R4 token,
          readiness probe, exp-backoff restart, graceful teardown); `readproc.py` read-only
          subprocess seam (3-shape allowlist, `cwd=<ws>`, never a shell — NOT the Phase 3 runner);
          `routes/stats.py` — `/stats` `/health` `/stats/{timeline,kinds,queries}`.
        - [x] **UI** — react-router + recharts dashboard (stat tiles, growth area chart, kinds bar,
          storage bar, query-log stats, doctor checklist); workspace register form + server-status
          chips. `types.ts` still a hand-stub (gen wired, not CI-gated).
      - [x] **RFC 0130 — Phase 2: `ekos.toml` config UX, `devlog_153`** (`ekos/docs/rfcs/0130-web-console-phase-2.md`,
        Accepted 2026-09-03). Auth stays the static `CONSOLE_TOKEN` (RFC 0129 §10 Q3 resolved —
        role split does NOT move up; it's Phase 3). Raw editor + validate + preview-scan;
        structured `[observe]` view is read-only. **Found on first run: this repo's `ekos.toml`
        has `*.lock` in ignore-patterns — a no-op, since patterns match dir names not globs.**
        - **R7** `ekos config validate --json` — `{ok, errors, warnings}`; errors from
          `deny_unknown_fields`/TOML syntax, warnings = observe-focused (ignore-pattern-looks-like-
          a-path — matched by dir NAME not glob; observe-path-missing; observe-empty).
        - **R8** `ekos config preview-scan --json` — walks what `build` would observe (same
          `walkdir`+`filter_entry`), counts files + `by_extension` + `ignored_dir_hits`
          (`dirs_skipped: 0` = the pattern matched nothing). Reuses `source_fingerprint`'s walk.
        - **Console**: `config_io.py` (tomlkit read/validate/`.bak` write/observe-diff);
          `routes/config.py` — `GET`/`PUT`/`POST validate`/`POST preview-scan`. `PUT` that narrows
          `paths`/`ignore-patterns` returns an `append_only_warning` (devlog 43 — future builds
          only, wipe+rebuild is the only remedy, a Phase 3 job).
        - **UI**: `/w/:id/config` — textarea editor + Validate + Preview-scan + Save (disabled
          until validate passes) + read-only observe summary.
      - [x] **RFC 0131 — Phase 3: command runner + job runner + OIDC auth, `devlog_154`**
        (`ekos/docs/rfcs/0131-web-console-phase-3.md`, Accepted 2026-09-03). First browser
        mutation → brings the read/write role split. Auth is **OIDC** (Authorization Code + PKCE,
        `authlib`, signed session cookie, a claim → write role) with a **two-static-token
        fallback** when `OIDC_ISSUER` is unset (`CONSOLE_TOKEN` = read, `CONSOLE_WRITE_TOKEN` =
        write). Run logs render in a plain `<pre>` (ANSI stripped server-side), not xterm.js.
        Console-only, no Rust changes.
        - **auth.py** — `Principal{subject,email,role}`, `/api/auth/{login,callback,logout,me}`,
          `require_role("read"|"write")` (401 unauth, 403 wrong role) replaces `require_console_token`.
        - **commands.py** — the real `COMMAND_ALLOWLIST` (doctor/build/recover/resolve/compile/
          commit/pipeline/clean/status/graph export/ledger repair|migrate/artifact repack/docs
          generate/ekl). `is_write` per command; hardcoded argv; never a shell; path params
          resolved against workspace roots.
        - **runner.py** — `JobRunner`: one worker + `asyncio.Lock` per workspace (RFC 0104 — two
          writes on one ws is a guaranteed conflict), bounded queue (429 when full), stream
          stdout+stderr to `.ekos-web/runs/<id>.log`, SIGTERM→SIGKILL cancel, chained `pipeline`
          with per-stage status, startup sweep of stale `running` rows → `interrupted`.
        - **models.Run** table; **routes/{auth,commands,runs}.py**; SSE `/api/runs/{id}/logs`.
        - **UI** — auth gate (Sign in / write-token field), `/w/:id/run` command cards,
          `/runs/:id` streaming log + cancel, `/w/:id/runs` history.
      - [x] **RFC 0132 — Phase 4: scheduled runs, `devlog_155`** (`ekos/docs/rfcs/0132-web-console-phase-4.md`,
        Accepted 2026-09-03). SQLite `Schedule` row = source of truth (APScheduler `AsyncIOScheduler`
        rebuilt from the table on start — no pickle job store). Every schedule has a **required
        `notify_url`** POSTed `{schedule_id, run_id, status, …}` on a non-succeeded run, plus a UI
        last-run chip. `build_trigger` — cron (`CronTrigger.from_crontab`, UTC) or interval,
        validated at create → 422. `JobRunner.submit` gains an `on_done` terminal-status callback
        (the only Phase 3 change). `routes/schedules.py` — `GET` (read) / `POST` / `PATCH` /
        `DELETE` / `POST /{id}/run-now` (write). UI `/schedules` page (write-role only). 68/68
        pytest.
      - [ ] **Next increments (0133+):** Phases 5–6 (graph v1/v2 — `react-force-graph-3d`, LOD,
        impact mode), Phase 7 (hardening). Deferred within R1: `--as-of` graph export (the
        `all_objects_at` primitive exists, scope doesn't), true streaming ndjson, the distributed
        `evidence_count` RPC.
  - [x] **RFC A — RFC 0096, `devlog_113`**: `AS OF <timestamp>` (new bulk
    `all_objects_at`/`all_relationships_at` on `KnowledgeStore`, both backends — the primitive didn't
    exist before, only single-id `object_at`/`relationships_at`, RFC 0047) and `COUNT`/`GROUP BY`
    (reuses the existing flat `Vec<Row>` `EklResult` shape, no new variant needed — a simplification
    found while implementing). `AS OF` + `FROM` explicitly rejected (`EklError::
    AsOfWithFromUnsupported`), not silently degraded — `load_neighborhood`/`trace_impact` have no
    time-aware equivalents yet. Object+Relationship `JOIN` deliberately deferred as its own future
    RFC — the one extension that actually breaks EKL's "six flat clause types" design ethos. 24 new
    tests, full workspace gate clean (103/103 test groups), `tests/integration` 3/3. Live-verified
    against this repo's own real 687-file self-analysis ledger: `COUNT GROUP BY kind` renders real
    per-kind counts, `AS OF '<now>'` matches current-state counts exactly, `AS OF` before this
    workspace's ledger existed correctly returns 0.
  - [x] **RFC B — attempted, found unsafe, deliberately not shipped (2026-08-26).** Built a
    `StoreCache` decorator (mtime-fingerprinted, reopen-on-change) exactly as scoped — full
    workspace gate passed, then a new regression test caught a real, serious problem before it
    shipped: `FactLedger::open` holds tantivy's `IndexWriter` lock for the **whole open handle's
    lifetime**, not just during a write (`SearchIndex` stores the writer as a field,
    `crates/ledger/src/search.rs`). Caching the open handle across MCP calls — the entire premise
    of RFC B — means the server would hold that exclusive lock indefinitely while idle between
    calls, **blocking any real `ekos build`/`commit` running in a separate process** from ever
    acquiring it for as long as the server stays up. Not a hypothetical: reproduced directly with a
    unit test simulating a concurrent external write, which failed with a real `LockBusy` error.
    The original "reopen every `tools/call`, drop immediately after" design was never actually the
    naive choice it looked like — it's the only design that keeps the lock's held duration short
    enough not to starve a concurrent writer. Fixing this properly needs a genuine read-only
    `FactLedger`/`SearchIndex` open path that skips acquiring the writer at all (routing the one
    write-capable MCP tool, `ekos_identity_review`, through a separate write-mode open) — real new
    storage-layer scope, not a caching-layer concern, and the same root cause already named as the
    top-priority item in `docs/GAP_ANALYSIS.md` §11 (Storage Architecture Phase 1: "`FactLedger`
    v3's actual single-writer enforcement is tantivy's own `IndexWriter` lock, an incidental side
    effect, not a designed mechanism"). Code reverted (`git stash`, message "RFC B (StoreCache) -
    abandoned, unsafe write-starvation design" — recoverable, not deleted, since the fingerprinting
    logic itself is real and reusable once a read-only open path exists to cache safely). No RFC
    filed — the design never reached a state worth documenting as accepted.
  - [x] **RFC B, done properly — RFC 0097, `devlog_114` (2026-08-26).** Went to the real root
    instead of patching around it: `SearchIndex::open_read_only` (never calls `Index::writer(..)`,
    so it never acquires tantivy's lock at all — only `IndexReader`, always safe to hold
    indefinitely), `FactLedger::open_read_only` (fails cleanly on a never-built workspace, refuses
    to self-heal corrupt runs read-only, skips the write-requiring search-index catchup — an
    honest, documented limitation: `find_objects` may lag a separate writer, every other read stays
    fully current), new `LedgerError::ReadOnly` write guard, `open_store_read_only`
    (`crates/cli/src/commands/store.rs`, bootstraps a genuinely fresh workspace via a short-lived
    writable open+drop so the existing "empty workspace just works" contract survives). `StoreCache`
    rebuilt on this — the abandoned attempt's fingerprinting logic was salvaged, not thrown away,
    just built on the right primitive this time. `ekos_identity_review` (the one write-capable MCP
    tool) extracted to bypass the cache entirely via its own fresh writable open. 7 new tests
    including the exact regression (a read-only handle staying open never blocks a concurrent
    writable open). Full workspace gate clean, `tests/integration` 3/3. **Live-verified against
    this repo's own real 5529-object ledger**: ran the real `ekos mcp serve` binary, two cached
    `tools/call`s returned identical results (handle reuse confirmed), then a genuine concurrent
    `ekos build` completed successfully while the server's cached handle stayed open — the exact
    scenario that failed with `LockBusy` under the first design.
  - [x] **RFC C — RFC 0098, `devlog_115` (2026-08-26).** Real SSE/NDJSON streaming for all three
    real providers (Anthropic/OpenAI/Ollama) via a new default-implemented `LlmProvider::
    complete_stream` (falls back to `complete` — zero breakage for `MockLlmProvider`/test-only
    implementors), a new dependency-free `stream_lines` primitive (`Response::chunk()`, not
    `bytes_stream()` — no new Cargo feature/dependency needed), `AiRuntime::ask_stream`, and `ekos
    ask --stream` (rejects `--stream --json` together). A real, non-obvious `async_trait` lifetime
    pitfall found live: a borrowed `&mut (dyn FnMut(&str) + Send)` callback fails to borrow-check
    inside `#[async_trait]`'s macro expansion (it collapses elided lifetimes to one shared name,
    breaking the implicit HRTB a borrowed-str callback needs) — fixed by taking an owned `String`
    per chunk instead. `CachedLlmProvider` deliberately bypasses its disk cache for streaming calls.
    The trailing `{"cited_evidence": [...]}` block is honestly NOT hidden during live streaming (a
    named, accepted v1 limitation — a buffering scheme to hide it was investigated and rejected as
    a real correctness risk for a cosmetic gain). MCP streaming explicitly out of scope (no
    progress-notification mechanism in its stdio protocol today; `ekos_ask` isn't even an MCP tool
    yet). 20 new tests (13 offline provider-parsing tests via extracted `StreamAccumulator`/
    `apply_stream_line` pure functions, 1 `ask_stream` test, 1 CLI rejection test, plus provider
    unit tests), full workspace gate clean, `tests/integration` 3/3. Live-verified against a real
    local Ollama daemon twice — a standalone probe (9 real incremental chunks, correct final
    content/token usage) and the full real CLI path (`ekos ask --stream` against a real, freshly
    built EKOS workspace, correct streamed prose + Sources section). Also confirmed the observed
    slowness against this repo's own large (~5,500-object) ledger is pre-existing (reproduced
    identically on the unmodified non-streaming `ask` path), not a regression from this RFC.
  - [x] **RFC D — RFC 0099, `devlog_116` (2026-08-26).** Real conversation memory: `LlmRequest`
    gains `history: &[Message]` (15 call sites across 11 files mechanically updated to
    `history: &[]`, zero behavior change for every pre-existing single-shot caller — a search-
    and-replace near-miss corrupted two unrelated structs that happen to also have a `max_tokens`
    field, `AiRuntimeConfig::default()` and `openai.rs`'s own wire-type definition, caught by
    reading the real `cargo build` output, not the script's own success count); all three real
    providers now build `messages` as `[system] + history + [current turn]` (Anthropic: history +
    current, system stays its own top-level field); `AiRuntime::ask_with_history`/
    `ask_stream_with_history` (`ask`/`ask_stream` now thin empty-history wrappers);
    `ekos ask --session <name>` persisting `.ekos/ask-sessions/<name>.json` (strict
    `[A-Za-z0-9_-]+` name validation — rejects anything that could escape the directory, rather than
    silently sanitizing). `ConversationTurn` deliberately stores the *clean* question/answer, never
    the raw grounded prompt or raw citation-block response, so history doesn't re-inflate every
    later turn with repeated retrieved-context JSON. Retrieval stays turn-local (no cross-turn
    blending) and there's no token-budget cap on accumulated history yet — both named, deliberate
    v1 limitations, not oversights. 18 new tests (provider wire-format placement, cache-key
    extension, real history-threading via a request-recording mock provider — not just "it
    compiles" — session name validation/round-trip), full workspace gate clean, `tests/integration`
    3/3. **Live-verified against a real local Ollama daemon with a real two-turn session**: turn 2
    asked a question with no possible answer from ledger retrieval at all ("what was my previous
    question about?") and the model answered correctly, proof the real conversation history was
    used, not just plumbed through unused code; the session file round-tripped on disk as two clean
    question/answer pairs with no leaked retrieval context or citation JSON.
