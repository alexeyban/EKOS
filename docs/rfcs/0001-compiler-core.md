# RFC 0001 — Compiler Core Architecture

| Field | Value |
|-------|-------|
| **Status** | Accepted |
| **Author** | alexeyban |
| **Created** | 2026-07-02 |
| **Gating** | Phase 0, Phase 1 |

---

## Motivation

Every subsequent phase of EKOS plugs into the compiler infrastructure. Defining the core
abstractions — `CompilerPass`, `PassManager`, `Scheduler`, `PassContext` — before writing any domain
logic prevents API churn across all future crates and ensures that determinism, dependency ordering,
and error propagation work correctly from day one.

---

## Design

### Concurrency model

**Decision: async throughout, using tokio.**

Rationale: LLM calls (Phase 6) are inherently async HTTP. Parallel pass execution (Phase 13)
requires futures. Retrofitting async onto a synchronous trait hierarchy later means rewriting every
trait, every impl, and every test. Adopting tokio from Phase 0 is cheaper.

All `CompilerPass::run` implementations must be `async`. Blocking operations (filesystem, SQLite)
use `tokio::task::spawn_blocking`.

### `CompilerPass` trait

```rust
#[async_trait]
pub trait CompilerPass: Send + Sync {
    fn name(&self) -> &str;
    fn dependencies(&self) -> &[&str] { &[] }
    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError>;
}
```

Invariants:
- `name()` must be unique within a `PassManager`
- `run()` is deterministic: same inputs → same outputs
- `run()` has no hidden side effects beyond mutating `ctx`

### `PassContext`

Carries everything a pass needs. No global state.

```rust
pub struct PassContext {
    pub config: Arc<EkosConfig>,
    pub diagnostics: DiagnosticSink,
    pub cwd: PathBuf,
}
```

Later phases add fields (artifact store, ledger writer) when needed. Passes must not read
environment variables directly — all config arrives via `ctx.config`.

### `PassManager`

Holds `Vec<Box<dyn CompilerPass>>`. Validates the dependency DAG using Kahn's algorithm (cycle
detection + unknown-dependency detection) and returns a topological execution order. Errors:
`CycleDetected(String)`, `UnknownDependency { pass, dep }`.

### `Scheduler`

Wraps `PassManager` with a `FailureMode`:
- `FailFast` (default): stop after the first failing pass
- `Collect`: run all passes, collecting all errors into `ExecutionReport`

Returns `ExecutionReport { outcomes: Vec<PassOutcome> }` where each `PassOutcome` holds the pass
name and its `Result<(), PassError>`.

### `EkosConfig`

Loaded from `ekos.toml` at the workspace root. All fields have sensible defaults. Validation
(unknown fields, malformed values) is the responsibility of `from_file()` — it returns `Err` with a
descriptive message. No pass may write to `EkosConfig`.

### `DiagnosticSink`

Collects `Diagnostic { severity: Severity, code: String, message: String, location: Option<SourceLocation> }`.
Passed to passes via `PassContext`. The `Compiler::run()` call returns `Err(CompilerError::Failed(n))`
if the sink contains any `Error`-severity diagnostic at the end of the run.

### Error propagation

- A pass returning `Err(PassError)` stops the scheduler in `FailFast` mode; in `Collect` mode subsequent
  passes still run.
- `Compiler::run()` returns `Ok(ExecutionReport)` only if zero `Error` diagnostics were emitted and
  no pass returned `Err`.

---

## Alternatives Considered

**Sync pass trait**: Would avoid the `async_trait` dependency and simplify initial implementation.
Rejected because async is required by Phase 6 LLM calls and Phase 13 parallel scheduling — deferring
the change would mean rewriting every trait and impl.

**`Arc<Mutex<DiagnosticSink>>`**: Would allow passes to emit diagnostics from concurrent tasks.
Not needed until Phase 13; deferred.

---

## Open Questions

All resolved.

---

## Acceptance Criteria

- [x] Concurrency model decided (async / tokio)
- [x] `CompilerPass` trait signature defined
- [x] Topological sort algorithm specified
- [x] `FailureMode` behaviour defined
- [x] Consistent with `ekos.md` compiler model
