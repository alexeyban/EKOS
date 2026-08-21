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
    connector, ServiceNow, dbt, etc.). Each follows the same pattern: RFC → SDK impl →
    integration test → docs.
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
  - [ ] *Analyzer-owned id-collision risk beyond `File`* — not yet done. `github_analyzer.rs`,
    `local_docs_analyzer.rs`, `rust_analyzer.rs`/`python_analyzer.rs` (via an embedded `data.path`
    field), and git's `CoupledWith` file-pair ids all derive from the same
    `ScanContext.workspace_root`-relative path convention `plugins/file/src/lib.rs` uses, and share
    the identical multi-project collision risk `build.rs` had. Two viable fixes not yet built:
    qualify `ObservationArtifact.content.target` centrally at RFC 0043's redaction choke point in
    `build.rs`, or fix each analyzer's id derivation individually.
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

- [ ] **Evidence citations show absolute filesystem paths, not repo-relative ones**
  - *What:* Found rehearsing the RFC 0045 demo end-to-end (Playwright screenshots, real `/ask`
    calls): EKOS-self's citations render clean (`TODO.md`), but `fd`'s render as the full
    workspace path (e.g. `/tmp/claude-.../scratchpad/demo-repo-spike/fd/./src/error.rs` instead of
    `src/error.rs`). Both repos went through the identical bake pipeline; the only difference is
    `fd`'s workspace lived under a long `/tmp` scratch path while EKOS-self's is the repo root
    itself — suggests `KirEvidence`/`SourceLocation.path` is (at least sometimes) populated from an
    absolute path rather than the plain within-project relative path RFC 0044's devlog describes
    elsewhere (`content.target`/`abs_path` vs. the display `"path"` property). Not yet root-caused
    to a specific file/line — only confirmed as a real, visible, reproducible symptom.
  - *Output:* Not yet started. A real demo-polish issue: showing a peer a citation with an internal
    `/tmp/...` path instead of a clean repo-relative one undercuts the "polished product"
    impression, independent of whether the underlying answer is correct.
  - *Test/Validate (remaining):* Root-cause which evidence-construction path uses an absolute vs.
    relative path (likely `plugins/*` observers or a recovery analyzer's `SourceLocation::file(...)`
    call site), fix it there, and confirm citations render repo-relative for a workspace at an
    arbitrary (not repo-root) path — `fd`'s baked ledger is a ready-made repro case.

- [ ] **Identity-resolution over-merge — real hits found against `ripgrep`/`bat`, still unfixed**
  - *What:* The repo-selection spike for RFC 0045 (devlog_45) baked `BurntSushi/ripgrep` and
    `sharkdp/bat` end-to-end and both hit the same identity-over-merge failure class CLAUDE.md
    already documents for `Section`/`TransformNode`/`RustSymbol`/`RustModule`/`Crate`: common-word
    crate/module names (`ignore`, `pcre2`, `bat` itself) flagged as multiple kinds via
    name-prefix similarity plus the same-kind structural-score fallback of 1.0. `fd` was chosen for
    the demo specifically because it was the one candidate that baked clean — this is a genuine,
    still-open product gap, not a demo-prep footnote.
  - *Output:* Not yet started. A structural fix to `identity`'s resolver (the recurring root cause),
    not another one-off exclusion-list entry — see `identity/src/cross_system.rs` and
    `DefaultResolver`'s blanket kind-exclusion list (`CLAUDE.md`'s `identity` crate-map entry).
  - *Test/Validate (remaining):* Re-run the RFC 0045 bake against `ripgrep` and `bat` and confirm
    zero identity conflicts, without introducing a new over-merge in the other direction.
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

- [ ] **Storage architecture: none of this is implemented yet**
  - *What:* Seven real gaps, none started: a snapshot+compaction model (periodic consolidation of
    the event chain into a final-state-plus-evidence-pointer snapshot, full history kept
    separately, colder/more compressed); materialized views alongside EAV (RFC 0016) for
    frequently-queried structural patterns, to avoid repeated EAV self-joins; a published v2→v3
    deprecation timeline (when SQLite stops being the default); a write-barrier/concurrency spec
    formalizing the single-writer model with versioned snapshots for concurrent readers; a
    write-ahead log plus a repair tool for corrupted segments (today the only recovery option for
    the v3 fact-segment engine is a full migration rollback); a retention/pruning policy (the
    ledger currently has no upper bound and no archival strategy beyond compaction of existing
    data, RFC 0034); horizontal distribution of storage across multiple machines, beyond RFC
    0034's single-machine vertical partitioning (RFC 0034).
  - *Why it matters now, not just eventually:* `devlog_65` found real, physical evidence this is
    already biting — `analytics/`'s local ledger has a corrupted FTS5 virtual table (base DB
    passes `PRAGMA integrity_check`, the FTS index doesn't) almost certainly from concurrent
    `ekos` processes writing to the same ledger with no write-barrier.
  - *Test/Validate:* each sub-item needs its own RFC before implementation, per the mandatory
    workflow — this entry exists so none of the six silently stays undiscoverable backlog.

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

- [ ] **Runtime/retrieval**: embedding/semantic search for retrieval (restated as open across RFC
  0009/0014/0015/0016 — one consolidated item); streaming LLM responses (RFC 0008/0009); ledger
  read caching (RFC 0005); multi-turn conversation history (RFC 0009); async ledger methods
  (RFC 0005); EKL joins across Object+Relationship in one query, `COUNT`/`GROUP BY` aggregation,
  and `AS OF <timestamp>` historical queries exposed in EKL syntax (RFC 0010 — note
  `object_at`/`relationships_at` already exist in `Runtime`, just not surfaced in EKL grammar);
  structurally boosting `memory/`-path search results beyond bm25 name-weighting (RFC 0014).

- [ ] **MCP / connector infrastructure**: MCP HTTP/SSE transport + auth + multi-workspace routing,
  and MCP resources/prompts capabilities beyond tools-only (RFC 0013); generic
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
  itself already shipped via RFC 0029's `cross_system.rs`, don't re-add that part).

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

- [ ] **Identity resolution**: extend RFC 0029's cross-system `unconfirmed`-until-reviewed flow to
  same-source (`DefaultResolver`) merges too — RFC 0060's own stated residual (3 of 17 known-wrong
  real pairs still clear the 0.90 threshold), restated in RFC 0062 for the GitHub over-merge case.
  One item, two RFC citations — this is the real structural fix the recurring
  Table/Person/Document/GitHub-item over-merge pattern actually needs.
