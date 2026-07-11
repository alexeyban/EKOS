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

- [ ] **Establish `docs/rfcs/` directory and RFC template**
  - *What:* Create `docs/rfcs/` and write `docs/rfcs/0000-template.md` with required sections:
    `Status`, `Motivation`, `Design`, `Alternatives Considered`, `Open Questions`, `Acceptance Criteria`.
  - *Output:* `docs/rfcs/0000-template.md` exists and is committed.
  - *Test/Validate:* `ls docs/rfcs/0000-template.md` exits 0. Template contains all six required
    section headings.

- [ ] **Spike: end-to-end knowledge-recovery prototype (throwaway, do this FIRST)**
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

- [ ] **RFC 0001 — Compiler Core architecture**
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

- [ ] **RFC 0002 — Artifact system and content-addressing scheme**
  - *What:* Define the artifact type hierarchy, the fields every artifact must carry (id, checksum
    algorithm, metadata shape, dependency list, version), the on-disk storage layout under `.ekos/`,
    the cache-hit / cache-miss decision algorithm, and serialization format (JSON v1).
  - *Output:* `docs/rfcs/0002-artifact-system.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: What hashing algorithm for content-addressing? What is the on-disk
    path formula for a given artifact id? How are artifact dependencies expressed?

- [ ] **RFC 0003 — Knowledge Intermediate Representation (KIR)**
  - *What:* Define the four KIR node types (`KirObject`, `KirRelationship`, `KirEvent`,
    `KirEvidence`), their mandatory and optional fields, how ids are assigned, how Evidence links to
    its source artifact, and the JSON schema for serialized KIR. Must define a **single evidence
    model** used across the whole system: `KirEvidence` is the one canonical type; Phase 2's
    `EvidenceArtifact` is its storage wrapper and Phase 8's `EvidenceRecord` is a denormalized
    projection of it — three views of one type, never three independently evolving types.
  - *Output:* `docs/rfcs/0003-kir.md` with status `Accepted`.
  - *Test/Validate:* RFC includes a worked example: one SQL table → one `KirObject` with two
    `KirEvidence` nodes showing exact JSON shape.

- [ ] **RFC 0004 — Semantic Knowledge Ledger**
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

- [ ] **RFC 0006 — Observation SDK and connector contract**
  - *What:* Define the `Observer` trait signature, `ScanContext` contents (config, logger, progress
    sink), `ObservationPackage` output structure, how connectors are discovered and loaded (static
    linking vs. dynamic plugins), and the versioning contract between SDK and plugins.
  - *Output:* `docs/rfcs/0006-observation-sdk.md` with status `Accepted`.
  - *Test/Validate:* RFC answers: Can a connector be written in isolation without depending on
    `compiler-core`? What is the minimal `Cargo.toml` for a new connector crate?

- [ ] **RFC 0007 — Identity Resolution algorithm**
  - *What:* Define the similarity scoring approach (name normalization, structural fingerprint,
    contextual embedding), the merge confidence threshold, how conflicts are surfaced, and the output
    format (canonical `KirObject` with provenance linking back to all merged sources).
  - *Output:* `docs/rfcs/0007-identity-resolution.md` with status `Accepted`.
  - *Test/Validate:* RFC includes a worked example: `Customer` (Postgres), `Buyer` (Confluence),
    `client` (Git commit message) → merged canonical Object with confidence score ≥ 0.85.

- [ ] **RFC 0008 — LLM policy: determinism, caching, model pinning**
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

- [ ] **Initialise Cargo workspace (`ekos/Cargo.toml`)**
  - *What:* Create `ekos/Cargo.toml` as a `[workspace]` manifest listing all planned member crates:
    `crates/compiler-core`, `crates/compiler-sdk`, `crates/scheduler`, `crates/artifact`,
    `crates/observation-sdk`, `crates/cli`, `crates/common`. Set `resolver = "2"` and
    `edition = "2024"` in each member's `Cargo.toml`. Each member has an empty `src/lib.rs`
    (or `src/main.rs` for `cli`).
  - *Output:* `ekos/Cargo.toml` workspace manifest; `ekos/crates/*/Cargo.toml`; `ekos/crates/*/src/lib.rs`.
  - *Test/Validate:* `cargo build --workspace` from `ekos/` exits 0 with no source files beyond
    empty `lib.rs` stubs.

- [ ] **Scaffold crate skeletons: `compiler-core`, `compiler-sdk`, `scheduler`, `artifact`, `observation-sdk`, `cli`, `common`**
  - *What:* For each crate, add a `[package]` section with correct name, version `0.1.0`, edition
    `2024`. Add inter-crate dependencies (e.g., `cli` depends on `compiler-core`). Ensure no circular
    dependencies exist. `cli` gets `src/main.rs` with `fn main() {}`.
  - *Output:* All crates compile individually (`cargo build -p <crate>`) and as a workspace.
  - *Test/Validate:* `for crate in compiler-core compiler-sdk scheduler artifact observation-sdk cli common; do cargo build -p $crate; done` — all exit 0.

- [ ] **GitHub Actions CI: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`**
  - *What:* Create `.github/workflows/ci.yml` with a single job that runs on `push` and
    `pull_request` to `main`. Steps: checkout → install stable Rust toolchain → `cargo build
    --workspace` → `cargo test --workspace` → `cargo clippy --workspace -- -D warnings` → `cargo fmt
    --check`.
  - *Output:* `.github/workflows/ci.yml`; a green CI run on the first push.
  - *Test/Validate:* Push a branch; GitHub Actions shows all steps green. Introduce a `clippy`
    warning intentionally; confirm CI fails on that step.

- [ ] **Docker development image**
  - *What:* Create `Dockerfile.dev` at repo root based on `rust:1.XX-slim`. Install `build-essential`,
    pin the Rust toolchain version. Add a `docker-compose.yml` (or `Makefile` target) that mounts the
    repo and runs `cargo build --workspace` inside the container.
  - *Output:* `Dockerfile.dev`; `docker-compose.dev.yml` (or `Makefile`). Image builds without errors.
  - *Test/Validate:* `docker compose -f docker-compose.dev.yml run ekos cargo build --workspace`
    exits 0 on a machine with no local Rust installation.

- [ ] **`ekos --help` produces output without panicking**
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

- [ ] **`compiler-core`: `Compiler` struct and lifecycle**
  - *What:* Define `pub struct Compiler` in `crates/compiler-core/src/lib.rs`. It holds a
    `PassManager`, a `Configuration`, and a `DiagnosticSink`. Implement `Compiler::new(config) ->
    Self` and `Compiler::run() -> Result<(), CompilerError>` which delegates to `PassManager::run_all()`.
  - *Output:* `Compiler` struct with `new` and `run` methods; `CompilerError` enum.
  - *Test/Validate:* Unit test: `Compiler::new(default_config()).run()` with zero registered passes
    returns `Ok(())`. With a pass that returns `Err`, `run()` propagates the error.

- [ ] **`compiler-core`: `PassManager` — registers and sequences compiler passes**
  - *What:* Define `pub trait CompilerPass` with `fn name(&self) -> &str`, `fn dependencies(&self)
    -> &[&str]`, and `fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError>`. Implement
    `PassManager` that holds `Vec<Box<dyn CompilerPass>>`, validates the dependency DAG (reject
    cycles), and returns an execution order via topological sort.
  - *Output:* `CompilerPass` trait; `PassManager::register()`, `PassManager::execution_order()`,
    `PassManager::run_all()`.
  - *Test/Validate:* Unit tests: (1) three passes A→B→C returns order [A, B, C]; (2) cycle A→B→A
    returns `Err(CycleDetected)`; (3) unknown dependency returns `Err(UnknownDependency)`.

- [ ] **`compiler-core`: `Scheduler` — controls pass execution order and dependencies**
  - *What:* `Scheduler` wraps `PassManager` and adds execution policy: sequential (default),
    with hooks for future parallel execution. Exposes `Scheduler::execute(passes, ctx)` which runs
    passes in declared order, collecting all diagnostics rather than stopping at the first error
    (configurable via `FailureMode::FailFast | FailureMode::Collect`).
  - *Output:* `Scheduler` struct; `FailureMode` enum; `ExecutionReport` containing pass outcomes.
  - *Test/Validate:* Unit test: two passes where the second fails — in `Collect` mode both run and
    `ExecutionReport` contains two entries; in `FailFast` mode the second pass does not run.

- [ ] **`compiler-core`: `Diagnostics` — structured error and warning reporting**
  - *What:* Define `Diagnostic { severity: Severity, code: &str, message: String, location: Option<SourceLocation> }` and `DiagnosticSink` (collects diagnostics during a build). `Severity` = `Error | Warning | Info`. Implement `DiagnosticSink::emit()`, `::errors()`, `::has_errors()`.
  - *Output:* `Diagnostic`, `DiagnosticSink`, `Severity`, `SourceLocation` types in `compiler-core`.
  - *Test/Validate:* Unit test: emit two warnings and one error; assert `has_errors()` = true;
    assert `errors().len()` = 1; assert total `diagnostics().len()` = 3.

- [ ] **`compiler-core`: `Configuration` — typed config loading**
  - *What:* Define `EkosConfig` struct with fields for workspace root, artifact cache directory,
    log level, and enabled connectors. Implement loading from `ekos.toml` at the workspace root using
    `toml` crate. Provide `EkosConfig::default()` and `EkosConfig::from_file(path) -> Result<Self>`.
  - *Output:* `EkosConfig` struct; `ekos.toml` example file at repo root; config loading and
    validation logic.
  - *Test/Validate:* Unit test: parse a fixture `ekos.toml` string; assert field values match.
    Pass a malformed TOML; assert `from_file` returns `Err`.

- [ ] **`compiler-core`: `Logging` — structured, levelled, per-crate**
  - *What:* Initialise `tracing` / `tracing-subscriber` in `compiler-core`. Configure log level from
    `EkosConfig`. Each crate uses `tracing::instrument` on its public entry points. Log format:
    structured JSON in CI/production, human-readable in development (controlled by `EKOS_LOG_FORMAT`
    env var).
  - *Output:* `init_logging(config: &EkosConfig)` function in `compiler-core`; `tracing` calls in
    all public functions.
  - *Test/Validate:* `EKOS_LOG=debug cargo run -p cli -- doctor 2>&1 | grep '"level":"DEBUG"'`
    finds at least one structured log line.

- [ ] **`cli`: `ekos init`**
  - *What:* Subcommand that creates a `.ekos/` directory at the current workspace root containing:
    `config/` (empty), `artifacts/` (empty), `ledger/` (empty). Writes a default `ekos.toml` if none
    exists. Idempotent — safe to run twice.
  - *Output:* `.ekos/` directory tree on disk; default `ekos.toml`.
  - *Test/Validate:* Run `ekos init` in an empty directory; assert `.ekos/artifacts/`, `.ekos/ledger/`
    exist. Run again; assert no error and no duplicate files.

- [ ] **`cli`: `ekos build`**
  - *What:* Subcommand that loads `ekos.toml`, constructs a `Compiler`, registers configured passes
    (none yet), runs the compiler, and prints a summary of the `ExecutionReport`. Exit code 0 on
    success, non-zero if any `Error`-severity diagnostic was emitted.
  - *Output:* Running `ekos build` prints `Build complete. 0 passes run, 0 errors.` and exits 0.
  - *Test/Validate:* `cargo run -p cli -- build` exits 0 and prints the summary line. Inject a
    failing pass via test harness; assert non-zero exit and error message on stderr.

- [ ] **`cli`: `ekos clean`**
  - *What:* Subcommand that deletes `.ekos/artifacts/` contents (cached artifacts) but preserves
    `.ekos/ledger/` and `ekos.toml`. Prints count of deleted files.
  - *Output:* Artifact cache cleared; ledger untouched.
  - *Test/Validate:* Create dummy files in `.ekos/artifacts/`; run `ekos clean`; assert artifact
    files gone. Assert `.ekos/ledger/` still exists.

- [ ] **`cli`: `ekos doctor`**
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

- [ ] **Minimal file observer (inline, pre-SDK)**
  - *What:* A hard-coded pass inside `compiler-core` (no `Observer` trait yet) that walks a
    configured directory and emits one observation per file (path, size, sha256). Replaced by the
    real SDK-based connectors in Phases 3–4.
  - *Output:* `ekos build` prints "N files observed" for the fixture directory.
  - *Test/Validate:* Run against `tests/fixtures/sample_project/`; assert observation count equals
    the known fixture file count.

- [ ] **Minimal KIR subset (`KirObject` + `KirEvidence` only)**
  - *What:* Just two of the four node types, with only mandatory fields, defined in `crates/kir`.
    Each observed file becomes a `KirObject(kind=File)` with one `KirEvidence` pointing at the file
    path. Extended to the full four-type model in Phase 5 (the crate and ids carry forward).
  - *Output:* `KirObject` and `KirEvidence` structs in `crates/kir`; build produces one object per file.
  - *Test/Validate:* Unit test: object → JSON → object round-trip. Build output artifact contains
    one object per fixture file, each with exactly one evidence node.

- [ ] **Minimal ledger append + read-by-id**
  - *What:* A single SQLite table (`entries`) with `append(entry)` and `get(id)` — no indexes, no
    history, no integrity checking. `ekos build` writes each `KirObject` straight to it. Replaced by
    the full ledger in Phase 9 (same crate, same `LedgerBackend` trait shape).
  - *Output:* `crates/ledger` with the two-function skeleton API; `.ekos/ledger/ledger.db` populated
    by `ekos build`.
  - *Test/Validate:* Unit test: append then get returns the identical entry. After `ekos build`,
    row count in SQLite equals fixture file count.

- [ ] **Minimal `ekos query object <id>`**
  - *What:* CLI subcommand that calls `ledger.get(id)` and prints the object as JSON, including its
    evidence. Widened into the full Runtime-backed query in Phase 10.
  - *Output:* `ekos query object <id>` subcommand.
  - *Test/Validate:* Query an id printed by `ekos build`; assert JSON output contains the file path
    in both `name` and the evidence fragment. Unknown id prints "Not found", exits 1.

- [ ] **End-to-end skeleton test in CI**
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

- [ ] **`artifact`: `ObservationArtifact`**
  - *What:* `pub struct ObservationArtifact` containing: `id: ArtifactId`, `checksum: Checksum`,
    `metadata: ArtifactMeta`, `dependencies: Vec<ArtifactId>`, `version: u32`, `source: SourceRef`
    (connector name + target), `raw_data: serde_json::Value`. `ArtifactId` is a `[u8; 32]` SHA-256
    hash of content.
  - *Output:* `ObservationArtifact` type in `crates/artifact/src/observation.rs`; serializes/deserializes to JSON.
  - *Test/Validate:* Unit test: construct an `ObservationArtifact`, serialize to JSON, deserialize
    back, assert round-trip equality. Assert two artifacts with identical content produce identical ids.

- [ ] **`artifact`: `KnowledgeArtifact`**
  - *What:* `pub struct KnowledgeArtifact` holding compiled KIR output: `id`, `checksum`, `metadata`,
    `dependencies: Vec<ArtifactId>` (points to source `ObservationArtifact`s), `version`, `kir:
    Vec<KirNode>` (placeholder type until Phase 5).
  - *Output:* `KnowledgeArtifact` type in `crates/artifact/src/knowledge.rs`; JSON serializable.
  - *Test/Validate:* Unit test: round-trip serialization. Assert `dependencies` field is preserved.

- [ ] **`artifact`: `EvidenceArtifact`**
  - *What:* `pub struct EvidenceArtifact` holding provenance records: `id`, `checksum`, `metadata`,
    `source_artifact_id: ArtifactId`, `location: SourceLocation` (file, line, column), `fragment: String`
    (the raw text snippet that was the evidence). Links a knowledge claim to its source.
  - *Output:* `EvidenceArtifact` type in `crates/artifact/src/evidence.rs`; JSON serializable.
  - *Test/Validate:* Unit test: construct with a SQL snippet as fragment; serialize and deserialize;
    assert `fragment` and `location` are preserved exactly.

- [ ] **`artifact`: `DiagnosticArtifact`**
  - *What:* `pub struct DiagnosticArtifact` collecting compiler diagnostics for a build run: `id`,
    `checksum`, `metadata`, `diagnostics: Vec<Diagnostic>` (from `compiler-core`). Allows storing
    and diffing diagnostic output across builds.
  - *Output:* `DiagnosticArtifact` type in `crates/artifact/src/diagnostic.rs`; JSON serializable.
  - *Test/Validate:* Unit test: create with two warnings; serialize; deserialize; assert `diagnostics.len() == 2`.

- [ ] **`artifact`: `IndexArtifact`**
  - *What:* `pub struct IndexArtifact` acting as a manifest for a build run: `id`, `checksum`,
    `metadata`, `entries: HashMap<String, ArtifactId>` (logical name → artifact id). Used by the
    compiler to locate artifacts by name without scanning the store.
  - *Output:* `IndexArtifact` type in `crates/artifact/src/index.rs`; JSON serializable.
  - *Test/Validate:* Unit test: insert three entries; serialize; deserialize; assert all three entries
    are present by key lookup.

- [ ] **Each artifact carries: unique id, checksum, metadata, dependencies, version**
  - *What:* Extract shared fields into `pub struct ArtifactMeta { created_at: DateTime<Utc>,
    produced_by: String, schema_version: u32 }` and a blanket `Artifact` trait with `fn id()`,
    `fn checksum()`, `fn meta()`, `fn dependencies()`, `fn version()`. All five types implement this trait.
  - *Output:* `Artifact` trait in `crates/artifact/src/lib.rs`; `ArtifactMeta` struct; all types impl trait.
  - *Test/Validate:* Unit test using the trait object: `let a: &dyn Artifact = &obs_artifact; assert_eq!(a.version(), 1)`.

- [ ] **`compiler-core`: artifact read / write / cache / reuse API**
  - *What:* Add `ArtifactStore` to `compiler-core` with: `fn write<A: Artifact>(&self, artifact: A)
    -> Result<ArtifactId>`, `fn read<A: Artifact>(&self, id: &ArtifactId) -> Result<A>`, `fn
    exists(&self, id: &ArtifactId) -> bool`. `PassContext` gains an `artifact_store: &ArtifactStore`
    field so passes can read inputs and write outputs.
  - *Output:* `ArtifactStore` trait and filesystem implementation in `compiler-core`.
  - *Test/Validate:* Unit test: write an `ObservationArtifact`, read it back by id, assert equality.
    Call `exists()` for a known id (true) and an unknown id (false).

- [ ] **Content-addressable artifact store (local filesystem backend)**
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

- [ ] **Serialization for all artifact types (JSON initially)**
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

- [ ] **`observation-sdk`: `Observer` trait (`fn scan(...) -> ObservationArtifact`)**
  - *What:* Define `pub trait Observer: Send + Sync { fn name(&self) -> &str; fn scan(&self, ctx:
    &ScanContext) -> Result<ObservationPackage, ObserverError>; }`. `ObserverError` is a structured
    error type (not `Box<dyn Error>`). The trait must be object-safe so connectors can be boxed.
  - *Output:* `Observer` trait in `crates/observation-sdk/src/lib.rs`.
  - *Test/Validate:* Write a `NoopObserver` that implements the trait and returns an empty package;
    box it as `Box<dyn Observer>`; call `scan()`; assert `Ok(empty_package)`.

- [ ] **`observation-sdk`: `ScanContext` — passes config and logging into connectors**
  - *What:* `pub struct ScanContext { pub config: ConnectorConfig, pub logger: tracing::Span, pub
    artifact_store: Arc<dyn ArtifactStore>, pub workspace_root: PathBuf }`. `ConnectorConfig` is a
    `HashMap<String, serde_json::Value>` allowing arbitrary connector-specific settings loaded from
    `ekos.toml`. Connectors must not access global state — everything they need comes via `ScanContext`.
  - *Output:* `ScanContext` and `ConnectorConfig` structs in `observation-sdk`.
  - *Test/Validate:* Unit test: build a `ScanContext` with a mock config map; pass to `NoopObserver`;
    assert the connector reads a config value via `ctx.config.get("key")`.

- [ ] **`observation-sdk`: `ObservationPackage` — output format**
  - *What:* `pub struct ObservationPackage { pub observer: String, pub target: String, pub artifacts:
    Vec<ObservationArtifact>, pub metadata: PackageMeta }` where `PackageMeta` includes `scanned_at:
    DateTime<Utc>`, `duration_ms: u64`, `item_count: usize`. The package is itself serializable to
    a directory: `snapshot/<observer-name>/package.json` + individual artifact JSON files.
  - *Output:* `ObservationPackage` type; `fn write_to_dir(&self, dir: &Path) -> Result<()>` method.
  - *Test/Validate:* Unit test: write a package with two artifacts to a temp dir; assert
    `snapshot/<name>/package.json` exists and `artifact_count` in JSON matches 2.

- [ ] **Example connector: File Observer (reference implementation)**
  - *What:* Create `plugins/file/` crate depending only on `observation-sdk`. Implement `Observer`
    to walk a directory tree from `ctx.workspace_root`, emit one `ObservationArtifact` per file with
    fields: `path`, `size_bytes`, `sha256`, `modified_at`. Respects a `ignore_patterns` config field
    (gitignore-style).
  - *Output:* `plugins/file/src/lib.rs`; passes `cargo test -p plugin-file`.
  - *Test/Validate:* Integration test: run the File Observer against `tests/fixtures/sample_project/`;
    assert the returned package contains exactly the expected number of file artifacts, each with
    correct path and non-zero size.

- [ ] **Example connector: Git Observer (basic)**
  - *What:* Create `plugins/git/` crate using `git2` crate. Implement `Observer` to walk the commit
    history of a repo at `ctx.workspace_root`, emitting one `ObservationArtifact` per commit with
    fields: `sha`, `author`, `timestamp`, `message`, `changed_files: Vec<String>`.
  - *Output:* `plugins/git/src/lib.rs`; passes `cargo test -p plugin-git`.
  - *Test/Validate:* Integration test: point the Git Observer at the EKOS repo itself (has at least
    one commit); assert the returned package contains at least one commit artifact with a non-empty
    `sha` and `author`.

- [ ] **SDK documentation and integration guide**
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

- [ ] **Plugin: `git` — commits, branches, authors, diffs**
  - *What:* Extend the Phase 3 basic Git Observer into a full plugin. Emit separate artifact types
    for: `CommitArtifact` (sha, author, date, message, stats), `BranchArtifact` (name, tip sha,
    upstream), `DiffArtifact` (changed files, hunks, line counts per commit). Handle repos with
    10k+ commits by streaming rather than loading all history into memory.
  - *Output:* `plugins/git/` crate with full implementation; artifacts cover commits, branches, diffs.
  - *Test/Validate:* Integration test against the EKOS repo: assert commit count > 0, at least one
    branch artifact, diff artifacts contain file paths matching known changed files.

- [ ] **Plugin: `filesystem` — directory trees, file metadata**
  - *What:* Extend Phase 3 File Observer. Emit: `FileArtifact` (path, size, sha256, mime_type,
    modified_at), `DirectoryArtifact` (path, child count, total size). Respect `.gitignore` and
    a configurable `exclude_patterns` list. Handle symlinks safely (record target, do not follow).
  - *Output:* `plugins/filesystem/` (or extend `plugins/file/`); directory tree faithfully captured.
  - *Test/Validate:* Run against `tests/fixtures/sample_project/` (checked-in fixture with known
    structure); assert exact file count, total size, and presence of specific file paths in output.

- [ ] **Plugin: `postgres` — schemas, tables, columns, constraints, views, functions**
  - *What:* Create `plugins/postgres/` using `sqlx` or `tokio-postgres`. Query information schema
    and pg_catalog to emit: `TableArtifact` (name, schema, columns with types/nullability),
    `ConstraintArtifact` (PK, FK, UNIQUE, CHECK), `ViewArtifact` (name, definition SQL),
    `FunctionArtifact` (name, language, body). Handle multiple schemas.
  - *Output:* `plugins/postgres/` crate; integration test fixture database (Dockerfile).
  - *Test/Validate:* Integration test: start a Postgres container with `tests/fixtures/ecommerce.sql`
    loaded; run connector; assert `orders` table artifact exists with correct column names and types;
    assert FK constraint artifact links `orders.customer_id` to `customers.id`.

- [ ] **Plugin: `sqlserver` — same as postgres surface**
  - *What:* Create `plugins/sqlserver/` using `tiberius`. Emit the same artifact types as the
    Postgres plugin (TableArtifact, ConstraintArtifact, ViewArtifact, FunctionArtifact) but query
    SQL Server's `INFORMATION_SCHEMA` and `sys.*` catalogs. Handle both Windows and SQL auth.
  - *Output:* `plugins/sqlserver/` crate; SQL Server integration test fixture.
  - *Test/Validate:* Integration test with SQL Server Express container: same assertions as Postgres
    fixture — table, constraint, view, and function artifacts all present with correct fields.

- [ ] **Output: structured `ObservationPackage` per source written to `snapshot/`**
  - *What:* Update `ekos build` to iterate configured connectors, run each via `Observer::scan()`,
    and write results to `snapshot/<connector-name>/` using `ObservationPackage::write_to_dir()`.
    Write `snapshot/metadata.json` with build timestamp, connector list, and total artifact counts.
  - *Output:* After `ekos build`, the `snapshot/` directory exists with one subdirectory per
    connector and a `metadata.json` at the root.
  - *Test/Validate:* `ekos build` with all four connectors configured; assert `snapshot/git/`,
    `snapshot/database/`, `snapshot/files/` all exist and each contains a `package.json`; assert
    `snapshot/metadata.json` lists all four connectors.

- [ ] **`ekos build` drives observation and writes packages to disk**
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

- [ ] **`KirObject` — identity node**
  - *What:* `pub struct KirObject { id: KirId, name: String, kind: ObjectKind, properties:
    HashMap<String, serde_json::Value>, evidence: Vec<KirId> }`. `ObjectKind` is an open enum
    (e.g., `Table`, `Entity`, `Service`, `Api`, `Unknown`). `KirId` is a `Uuid` v4.
  - *Output:* `KirObject` type with serde derives.
  - *Test/Validate:* Unit test: construct a `KirObject` of kind `Table` named `"orders"`, serialize
    to JSON, deserialize, assert all fields are preserved.

- [ ] **`KirRelationship` — semantic connection**
  - *What:* `pub struct KirRelationship { id: KirId, kind: RelationshipKind, from: KirId, to: KirId,
    properties: HashMap<String, serde_json::Value>, evidence: Vec<KirId> }`. `RelationshipKind`:
    `ForeignKey`, `Calls`, `Extends`, `DependsOn`, `OwnedBy`, `Unknown`.
  - *Output:* `KirRelationship` type with serde derives.
  - *Test/Validate:* Unit test: construct a `ForeignKey` relationship between two `KirObject` ids;
    serialize and deserialize; assert `from` and `to` ids match.

- [ ] **`KirEvent` — immutable change record**
  - *What:* `pub struct KirEvent { id: KirId, kind: EventKind, subject: KirId, timestamp:
    DateTime<Utc>, payload: serde_json::Value, evidence: Vec<KirId> }`. `EventKind`: `Created`,
    `Modified`, `Deleted`, `Migrated`, `Deployed`.
  - *Output:* `KirEvent` type with serde derives.
  - *Test/Validate:* Unit test: construct a `Created` event for a `KirObject`, round-trip through JSON.

- [ ] **`KirEvidence` — provenance record**
  - *What:* `pub struct KirEvidence { id: KirId, source_artifact: ArtifactId, location:
    SourceLocation, fragment: String, confidence: f32 }`. `confidence` is [0.0, 1.0]. This is the
    only node type that references a raw artifact — it is the bridge from compiled knowledge back to
    raw observations.
  - *Output:* `KirEvidence` type with serde derives.
  - *Test/Validate:* Unit test: construct evidence with `confidence = 0.95`, serialize, deserialize,
    assert `confidence` is preserved with < 0.001 float tolerance.

- [ ] **KIR serialization (JSON)**
  - *What:* Define `pub struct KirGraph { objects: Vec<KirObject>, relationships: Vec<KirRelationship>,
    events: Vec<KirEvent>, evidence: Vec<KirEvidence> }` with a `fn to_json(&self) -> String` and
    `fn from_json(s: &str) -> Result<Self>`. Store `KirGraph` inside a `KnowledgeArtifact`.
  - *Output:* `KirGraph` type; round-trip serialization via `KnowledgeArtifact`.
  - *Test/Validate:* Integration test: write a `KirGraph` with one node of each type as a
    `KnowledgeArtifact`; read it back from the artifact store; assert structural equality.

- [ ] **No optimization or semantic enrichment — pure structural representation**
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

- [ ] **Compiler pass: `SqlAnalyzer` → Business Entities, Relationships, Evidence from SQL**
  - *What:* Implement `SqlAnalyzer: CompilerPass`. Input: `ObservationArtifact` from the Postgres or
    SQL Server connector. Deterministic extraction: every table → `KirObject(kind=Table)`, every FK
    constraint → `KirRelationship(kind=ForeignKey)`, every column → property on the object. LLM
    extraction: send table names + column names to the LLM with a prompt asking for likely business
    entity names and semantic relationships not expressed by FKs. Emit `KirEvidence` for each claim.
  - *Output:* `KnowledgeArtifact` with fully populated `KirGraph` stored in `.ekos/artifacts/`.
  - *Test/Validate:* Test with `tests/fixtures/ecommerce.sql`: assert `orders` table → `Order` object;
    assert `orders.customer_id` FK → `placed_by` relationship to `Customer`; assert each relationship
    has at least one `KirEvidence` node pointing back to the SQL artifact.

- [ ] **Compiler pass: `GitAnalyzer` → change patterns, ownership, coupling**
  - *What:* Implement `GitAnalyzer: CompilerPass`. Input: `ObservationPackage` from the git connector.
    Extract: files that change together frequently → `KirRelationship(kind=CoupledWith)`, authors
    responsible for a path → `KirRelationship(kind=OwnedBy)`, modules that only one author touches
    → `KirObject` with `single_owner: true` property. LLM: interpret commit messages to tag commits
    with semantic labels (feature, bugfix, refactor, breaking-change).
  - *Output:* `KnowledgeArtifact` with ownership and coupling graph.
  - *Test/Validate:* Test against EKOS repo history: assert at least one `OwnedBy` relationship;
    assert commit artifacts are tagged with at least one semantic label.

- [ ] **Compiler pass: `ConfluenceAnalyzer` → concepts and relationships from documentation**
  - *What:* Implement `ConfluenceAnalyzer: CompilerPass` (no Confluence connector yet — accept a
    directory of Markdown files as input for now). Parse Markdown, extract headings as candidate
    `KirObject`s, extract links between pages as `KirRelationship(kind=References)`. Use LLM to
    identify business concepts, definitions, and rules mentioned in the text. Emit `KirEvidence`
    citing the paragraph.
  - *Output:* `KnowledgeArtifact` from documentation.
  - *Test/Validate:* Test with `tests/fixtures/sample_docs/` (a few Markdown files). Assert headings
    become objects; assert cross-page links become relationships; assert at least one business rule
    is extracted as a `KirObject(kind=BusinessRule)`.

- [ ] **LLM integration layer (provider-agnostic trait, first backend: Anthropic Claude)**
  - *What:* Define `pub trait LlmProvider: Send + Sync { async fn complete(&self, prompt: &str,
    max_tokens: u32) -> Result<String, LlmError>; }` in a new `crates/llm/` crate (or `common`).
    Implement `ClaudeProvider` using the Anthropic API (`claude-sonnet-4-6` model, streaming
    optional). Configuration via `ekos.toml` `[llm]` section: `provider = "claude"`, `api_key_env =
    "ANTHROPIC_API_KEY"`. Retry logic: 3 attempts with exponential backoff on rate-limit errors.
  - *Output:* `LlmProvider` trait; `ClaudeProvider` implementation; `ANTHROPIC_API_KEY` env var used.
  - *Test/Validate:* Unit test with a mock `LlmProvider`. Integration test (gated by
    `--features llm-integration`): send a real prompt to Claude; assert non-empty response string.

- [ ] **LLM response cache (determinism + cost control, per RFC 0008)**
  - *What:* Implement `CachedLlmProvider` — a decorator around any `LlmProvider` that stores every
    response as an artifact keyed by SHA-256 of (model id, prompt, params). On cache hit, return
    the stored response without an API call. This is what makes LLM-based passes reproducible
    (warm cache ⇒ identical output) and makes re-runs free during development.
  - *Output:* `CachedLlmProvider` in the `llm` crate; cache artifacts under `.ekos/artifacts/llm/`.
  - *Test/Validate:* Unit test: two identical `complete()` calls hit the inner mock provider exactly
    once (call counter == 1). Changing one character of the prompt busts the cache (counter == 2).

- [ ] **Recovery quality eval harness (golden dataset)**
  - *What:* Unit tests prove the code runs; they cannot tell whether the LLM extracted the *right*
    entities. Create `tests/eval/` with fixture inputs and hand-labelled expected KIR (golden
    files). An eval runner compares analyzer output against the labels and computes precision and
    recall for objects and relationships. This is the regression net for every future prompt change.
  - *Output:* `tests/eval/` golden dataset; an eval runner (`ekos eval` or a cargo test target)
    printing per-analyzer precision/recall.
  - *Test/Validate:* Eval on the ecommerce fixture reports ≥ 0.8 precision and recall for entities
    and FK relationships. Results are committed alongside prompt changes so quality drift is visible
    in review.

- [ ] **`cli`: `ekos recover` command**
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

- [ ] **`identity`: resolver trait and algorithm**
  - *What:* Define `pub trait IdentityResolver { fn resolve(&self, graph: &KirGraph) ->
    Result<ResolvedGraph, ResolverError>; }` where `ResolvedGraph` wraps `KirGraph` with an added
    `canonical_map: HashMap<KirId, KirId>` (original id → canonical id). Implement
    `DefaultIdentityResolver` orchestrating the scoring pipeline below.
  - *Output:* `identity` crate at `crates/identity/`; `IdentityResolver` trait.
  - *Test/Validate:* Unit test: pass a `KirGraph` with two identical objects (same name, same kind);
    assert `canonical_map` merges them into one.

- [ ] **Similarity scoring (name-based, structural, contextual)**
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

- [ ] **Canonical entity merging: `Customer` + `Buyer` + `Client` → one `KirObject`**
  - *What:* When score exceeds the configured merge threshold (default 0.8), merge the objects:
    canonical name = highest-evidence name, properties = union of all properties (conflicts flagged as
    diagnostics), evidence = union of all evidence from all merged objects. Emit `KirEvent(kind=Merged)`
    recording which ids were merged and the merge confidence.
  - *Output:* `merge(objects: &[&KirObject]) -> KirObject` function; merged object retains all evidence.
  - *Test/Validate:* Unit test: merge `Customer` (DB) and `Buyer` (Confluence); assert merged object
    has `evidence.len() == sum of both`; assert a `Merged` event was emitted; assert the original
    ids map to the canonical id in `canonical_map`.

- [ ] **Confidence scoring on merges**
  - *What:* Each entry in `canonical_map` carries a `MergeRecord { canonical_id, source_ids, score:
    f32, merge_reason: String }`. `score` is the weighted similarity score that triggered the merge.
    Merges below threshold but above a `review_threshold` (default 0.6) are flagged as
    `Warning`-severity diagnostics requiring human review.
  - *Output:* `MergeRecord` type; diagnostic warnings for low-confidence merges.
  - *Test/Validate:* Unit test: merge two objects with score 0.65 (between thresholds); assert a
    `Warning` diagnostic is emitted with the object names and score in the message.

- [ ] **Conflict detection and reporting**
  - *What:* When two objects being merged have the same property key with different types or
    semantically incompatible values (e.g., `id` is `INT` in DB but `UUID` in API), emit an
    `Error`-severity diagnostic listing the conflict. The merge still proceeds but marks the
    conflicting property as `conflict: true` in the canonical object's properties.
  - *Output:* Conflict diagnostics in `DiagnosticArtifact`; `conflict` flag on merged properties.
  - *Test/Validate:* Unit test: merge two objects where `id` has type `Int` vs `Uuid`; assert one
    `Error` diagnostic with both type names in the message; assert merged object has
    `properties["id"]["conflict"] == true`.

- [ ] **Reusable as standalone library**
  - *What:* Ensure `crates/identity/` has zero dependency on `compiler-core` or `cli`. Its only
    dependencies are `crates/kir` (or the KIR module) and optionally `crates/llm`. Publish a
    `README.md` in the crate explaining standalone usage with a minimal code example.
  - *Output:* `crates/identity/README.md`; `cargo package -p identity` succeeds without errors.
  - *Test/Validate:* Write a standalone binary in `examples/identity_standalone.rs` that builds
    a small `KirGraph` and runs the resolver, with no `compiler-core` import. `cargo run --example
    identity_standalone` exits 0 and prints the resolution result.

- [ ] **`cli`: `ekos resolve` command**
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

- [ ] **`semantic`: `SemanticCompiler` pass**
  - *What:* Implement `SemanticCompiler: CompilerPass`. Input: `ResolvedGraph` from Phase 7. Runs
    three sub-passes: (1) relationship normalisation, (2) cross-source evidence aggregation, (3) CKM
    schema validation. Emits a `KnowledgeArtifact` containing the CKM JSON.
  - *Output:* `SemanticCompiler` struct in `crates/semantic/src/lib.rs`.
  - *Test/Validate:* Unit test: pass a small `ResolvedGraph` with two objects and one relationship;
    assert `SemanticCompiler::run()` returns `Ok(())`; assert CKM artifact exists in store.

- [ ] **Transform KIR → CKM (JSON, no binary)**
  - *What:* Define `CkModel { version: u32, compiled_at: DateTime<Utc>, objects: Vec<CkmObject>,
    relationships: Vec<CkmRelationship>, evidence_index: HashMap<KirId, EvidenceRecord> }`.
    `CkmObject` is a flattened, denormalized view of a canonical `KirObject` — no forward references,
    all related evidence embedded. Write to `.ekos/ckm/model.json`.
  - *Output:* `CkModel` type; `model.json` file on disk.
  - *Test/Validate:* `cat .ekos/ckm/model.json | python3 -m json.tool` exits 0 (valid JSON).
    Assert schema version field is `1`.

- [ ] **Relationship normalisation and deduplication**
  - *What:* Within the `SemanticCompiler`, after identity resolution, the same relationship may be
    observed multiple times (FK in DB + reference in documentation). Deduplicate by `(from, to, kind)`
    tuple; merge evidence lists. Relationships pointing to non-existent objects are flagged as
    `Warning` diagnostics and dropped from the CKM.
  - *Output:* `normalize_relationships(graph: &ResolvedGraph) -> Vec<CkmRelationship>` function.
  - *Test/Validate:* Unit test: graph with three identical `ForeignKey` relationships; assert
    output contains exactly one with all three evidence entries merged.

- [ ] **Cross-source evidence aggregation**
  - *What:* For each `CkmObject`, gather evidence from all source artifacts (DB connector, Git
    connector, Confluence analyzer) and embed as `evidence: Vec<EvidenceRecord>` sorted by confidence
    descending. Highest-confidence evidence fragment is used as the object's `primary_description`.
  - *Output:* `aggregate_evidence(object: &KirObject, artifacts: &ArtifactStore) -> Vec<EvidenceRecord>`
    function; each `CkmObject` has non-empty `evidence`.
  - *Test/Validate:* Unit test: object with evidence from two sources; assert aggregated evidence has
    both entries; assert highest-confidence evidence is `primary_description`.

- [ ] **CKM schema definition and validation**
  - *What:* Write `docs/ckm-schema.json` as a JSON Schema document describing the CKM format.
    Implement `fn validate_ckm(model: &CkModel) -> Result<(), Vec<SchemaError>>` that checks:
    all relationship `from`/`to` ids exist as objects, all evidence `source_artifact` ids exist in
    store, no duplicate object ids.
  - *Output:* `docs/ckm-schema.json`; `validate_ckm` function; validation runs at end of `SemanticCompiler::run()`.
  - *Test/Validate:* Unit test: valid CKM passes validation. CKM with a dangling relationship
    (references non-existent object id) returns `Err` with the offending relationship id.

- [ ] **`cli`: `ekos compile` command**
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

- [ ] **`ledger`: append-only storage engine (behind `LedgerBackend` trait)**
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

- [ ] **Store: Objects, Relationships, Events, Evidence**
  - *What:* Implement `LedgerWriter` with four typed methods: `write_object(obj: &CkmObject)`,
    `write_relationship(rel: &CkmRelationship)`, `write_event(evt: &KirEvent)`,
    `write_evidence(ev: &EvidenceRecord)`. Each serializes to JSON and calls `Ledger::append`. The
    ledger entry `type` field discriminates the four kinds.
  - *Output:* `LedgerWriter` in `crates/ledger/src/writer.rs`.
  - *Test/Validate:* Integration test: write one of each type; query the SQLite DB directly; assert
    four rows exist with correct `type` values.

- [ ] **Current-state index**
  - *What:* Maintain a `current_state` table: `(object_id TEXT PRIMARY KEY, latest_entry_id TEXT)`.
    Updated atomically within the same SQLite transaction as the `entries` insert. Enables
    `LedgerReader::current_object(id) -> Option<CkmObject>` without scanning the full entry log.
  - *Output:* `current_state` table; `LedgerReader::current_object()` method.
  - *Test/Validate:* Write an object, then write an updated version with the same id; assert
    `current_object(id)` returns the second version; assert `entries` table has two rows for that id.

- [ ] **Historical state index**
  - *What:* All `entries` rows are already the history. Implement `LedgerReader::object_history(id)
    -> Vec<(DateTime<Utc>, CkmObject)>` that returns all versions ordered by `written_at` ascending.
    For time-travel: `object_at(id, timestamp) -> Option<CkmObject>` returns the latest version
    with `written_at <= timestamp`.
  - *Output:* `LedgerReader::object_history()` and `object_at()` methods.
  - *Test/Validate:* Write object at t1, update at t2. Assert `object_at(id, t1)` returns v1,
    `object_at(id, t2)` returns v2, `object_at(id, t0)` returns `None`.

- [ ] **Full audit trail (every write timestamped and sourced)**
  - *What:* Every `LedgerEntry` must include `source_artifact_id` (the `ArtifactId` of the
    `KnowledgeArtifact` that produced this knowledge). `written_at` uses wall-clock UTC time.
    Implement `LedgerReader::audit_trail(id) -> Vec<AuditRecord>` returning the full write history
    with artifact provenance.
  - *Output:* `AuditRecord { entry_id, written_at, source_artifact_id, type }`;
    `LedgerReader::audit_trail()`.
  - *Test/Validate:* Write an object twice from two different `KnowledgeArtifact` ids; assert
    `audit_trail()` returns two records with different `source_artifact_id` values.

- [ ] **`cli`: `ekos commit` command (idempotent)**
  - *What:* Subcommand that reads the CKM at `.ekos/ckm/model.json` and writes objects,
    relationships, and evidence to the ledger via `LedgerWriter`. Must be idempotent: entry ids
    derive from content hashes and entries already present are skipped, so running `ekos commit`
    twice never duplicates knowledge in the append-only log.
  - *Output:* `ekos commit` subcommand (referenced by this phase's Validation section).
  - *Test/Validate:* Run `ekos commit` twice on the same CKM; assert the ledger entry count is
    identical after the second run and the summary prints "0 new entries".

- [ ] **`cli`: `ekos ledger status`**
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

- [ ] **Incremental compilation (re-scan changed sources only)**
  - *What:* Before running an `Observer`, compare the current source fingerprint (Git HEAD sha for
    git, mtimes for filesystem, schema version hash for DB) against the fingerprint stored in the
    previous `ObservationPackage`. If unchanged, skip the observation and reuse the cached artifact.
    Implement `fn source_fingerprint(ctx: &ScanContext) -> Fingerprint` in `observation-sdk`.
  - *Output:* Cache-hit path in `ekos build`; "N connectors skipped (cached)" in build summary.
  - *Test/Validate:* Run `ekos build` twice without changing sources; assert second run takes <10%
    of first run time and prints "0 connectors re-scanned".

- [ ] **Parallel pass execution**
  - *What:* Update `Scheduler` to detect passes with no data dependency between them and execute
    them concurrently using `tokio::task::spawn`. Passes that share an output artifact (write to the
    same `ArtifactId`) must not run concurrently — the scheduler enforces this via the dependency DAG.
  - *Output:* `Scheduler` with parallel execution mode; `--parallel` flag on `ekos build`.
  - *Test/Validate:* Run `ekos build --parallel` with three independent passes; instrument each pass
    to record start time; assert all three start times are within 100ms of each other.

- [ ] **Artifact cache invalidation strategy**
  - *What:* Define when a cached artifact is invalidated: (1) any transitive input artifact has
    changed (content-hash differs), (2) the pass that produced it has a different version, (3) the
    pass configuration has changed. Implement `fn should_recompute(pass: &dyn CompilerPass, inputs:
    &[ArtifactId], store: &ArtifactStore) -> bool`.
  - *Output:* `should_recompute` function used by `PassManager`; "N passes skipped (cached)" in build summary.
  - *Test/Validate:* Change a pass's config in `ekos.toml`; assert the pass re-runs on next build
    even though its input artifacts have not changed.

- [ ] **Knowledge diff (what changed between two ledger states)**
  - *What:* Implement `fn diff_ledger(ledger: &Ledger, from: DateTime<Utc>, to: DateTime<Utc>) ->
    LedgerDiff` where `LedgerDiff { added: Vec<LedgerEntryId>, unchanged: usize }`. (No deletion
    from append-only ledger, so "changed" means a new entry superseded an older one for the same
    object.) Add `ekos diff --from <timestamp> --to <timestamp>` CLI.
  - *Output:* `LedgerDiff` type; `ekos diff` subcommand.
  - *Test/Validate:* Write 3 objects at t1, update 1 at t2. `diff_ledger(t1, t2)` returns
    `added.len() == 1` (the updated entry) and `unchanged == 2`.

- [ ] **Knowledge merge and branch**
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

- [ ] **SAP connector**
  - *What:* Implement `plugins/sap/` using SAP OData APIs or RFC (Remote Function Call) via the
    `nwrfc` binding. Observe: business objects (BAPIs), table definitions, organizational hierarchy.
    Emit `ObservationArtifact`s per object type.
  - *Output:* `plugins/sap/` crate; integration test with SAP sandbox.
  - *Test/Validate:* Integration test: connect to SAP sandbox; assert at least one BAPI artifact
    and one organizational unit artifact are returned.

- [ ] **Salesforce connector**
  - *What:* Implement `plugins/salesforce/` using the Salesforce REST API. Observe: sObjects schema
    (fields, types, relationships), workflow rules, custom objects. Emit one `ObservationArtifact`
    per sObject with its full field metadata.
  - *Output:* `plugins/salesforce/` crate; integration test with Salesforce developer org.
  - *Test/Validate:* Integration test: observe `Account` and `Contact` sObjects; assert field count
    matches Salesforce developer org schema; assert relationship between Account and Contact is captured.

- [ ] **Oracle connector**
  - *What:* Implement `plugins/oracle/` using `oracle` crate (ODPI-C bindings). Same surface as
    Postgres connector: tables, constraints, views, stored procedures. Handle Oracle-specific types
    (VARCHAR2, NUMBER, CLOB).
  - *Output:* `plugins/oracle/` crate; integration test with Oracle XE container.
  - *Test/Validate:* Integration test: load fixture schema into Oracle XE; assert same artifact
    types and counts as equivalent Postgres fixture.

- [ ] **Microsoft Fabric / Snowflake connector**
  - *What:* Implement `plugins/fabric/` using Azure Fabric REST API (workspaces, lakehouses,
    datasets) and `plugins/snowflake/` using the Snowflake JDBC/ODBC REST API. Observe: schemas,
    tables, views, warehouse metadata.
  - *Output:* Two crates; integration tests with Fabric trial and Snowflake trial accounts.
  - *Test/Validate:* Integration test per platform: observe a test warehouse schema; assert table
    and view artifacts are returned.

- [ ] **Kubernetes connector**
  - *What:* Implement `plugins/kubernetes/` using the `kube` crate. Observe: Deployments, Services,
    ConfigMaps, Secrets (names only, no values), Ingresses, CRDs. Emit one artifact per resource kind
    plus one per namespace. Map service-to-deployment relationships.
  - *Output:* `plugins/kubernetes/` crate; integration test against a local `kind` cluster.
  - *Test/Validate:* Integration test: deploy a simple two-service app to `kind`; assert both
    `ServiceArtifact`s are present and the service-to-deployment relationship is captured.

- [ ] **Additional connectors on demand**
  - *What:* Placeholder for connectors requested after Phase 14 ships (Jira, Confluence full
    connector, ServiceNow, Databricks, dbt, etc.). Each follows the same pattern: RFC → SDK impl →
    integration test → docs.
  - *Output:* Tracked as individual issues/tickets; this item is the backlog bucket.
  - *Test/Validate:* Each connector added here must ship with a passing integration test before merge.

---

## Ongoing / Cross-cutting

These items have no single phase — they must be maintained and grown throughout the entire project lifecycle.

---

- [ ] **Benchmark suite (`benchmark/`) — one benchmark per compiler pass**
  - *What:* Use `criterion` crate. One benchmark binary per phase-significant pass: `observation_git`,
    `sql_analyzer`, `identity_resolver`, `semantic_compiler`, `ledger_write`, `runtime_load_neighborhood`.
    Each benchmark uses a fixed fixture dataset so results are comparable across commits. Scope the
    PR-checklist benchmark requirement: benchmarks are mandatory only from Phase 4 onward (once real
    passes exist) and only for performance-relevant changes — scaffolding, CLI plumbing, and docs
    PRs in Phases 0–3 are exempt. A benchmark of an empty pass manager measures nothing.
  - *Output:* `benchmark/benches/*.rs`; `cargo bench` produces HTML reports in `target/criterion/`.
  - *Test/Validate:* `cargo bench` exits 0. CI stores benchmark results as artifacts. Any regression
    > 20% triggers a CI warning comment on the PR.

- [ ] **Integration test harness (`tests/`) using real fixture data**
  - *What:* `tests/fixtures/` contains: `ecommerce.sql` (Postgres schema), `sample_project/` (small
    directory tree), `sample_docs/` (Markdown files), `git_fixture/` (a small committed git repo).
    `tests/integration/` contains end-to-end tests that run the full pipeline (build → recover →
    compile → commit → query) against these fixtures without external services.
  - *Output:* `tests/fixtures/`; `tests/integration/` test binaries; `cargo test --test integration` passes.
  - *Test/Validate:* `cargo test --test integration` from a clean clone with no external services
    running exits 0. Every phase's Validation section is covered by at least one integration test.

- [ ] **Secrets management and sensitive-data policy**
  - *What:* Connectors need DB passwords and API tokens (Postgres, Salesforce, SAP). Standardise:
    all secrets are referenced by env-var name in `ekos.toml` (e.g., `password_env = "PG_PASSWORD"`),
    never as literal values; `ekos doctor` verifies referenced vars exist. Separately — and more
    seriously — evidence fragments can capture sensitive data (PII in SQL rows, secrets in commit
    diffs) into an append-only ledger that by design never deletes. Define a redaction pass
    (configurable patterns: emails, credentials, national IDs) applied before evidence is written,
    and write an RFC covering data retention and erasure obligations (e.g., GDPR right-to-erasure
    vs. the append-only guarantee).
  - *Output:* Env-var-only secret loading in `observation-sdk`; a redaction pass in the pipeline;
    an accepted RFC on data retention/erasure.
  - *Test/Validate:* A config containing a literal `password = "..."` fails validation with a clear
    error. A fixture SQL dump containing an email address produces stored evidence with the address
    redacted.

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

- [ ] **`examples/` — at least one runnable example per crate**
  - *What:* Each crate under `crates/` has at least one file in its `examples/` directory that
    demonstrates the primary use case with real (non-mock) objects. Examples must compile and run
    to completion with `cargo run --example <name> -p <crate>` producing non-empty output.
  - *Output:* `crates/*/examples/*.rs`; all examples pass `cargo run`.
  - *Test/Validate:* CI step: `for each crate, cargo run --example <primary-example> -p <crate>`
    exits 0. Broken examples block merge.
