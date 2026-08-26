# Devlog 126 — Phase -1 through 13 checkbox audit: four real findings, ~80 stale checkboxes corrected

**Date:** 2026-08-26
**PRs:** none (infra + tracking + one retroactive RFC)
**Branch:** main (direct)

---

## Summary

Asked to continue implementation gaps from `TODO.md`'s original Phase -1 through Phase 13
("Optimizer") planning template, then — after a background subagent's audit attempt malfunctioned
(see `devlog_124`'s Process note and the SendFeedback drafts filed this session) — to check for its
response and, failing that, run a fresh, direct audit myself. Did the full direct audit: every
`- [ ]` item across Phase -1 through Phase 13 (~82 items) checked against the real codebase, not
assumed. The pattern held almost everywhere: the actual EKOS codebase — `compiler-core`
(`Compiler`/`PassManager`/`Scheduler`/`Diagnostics`/`Configuration`), every CLI command, the
artifact system, `observation-sdk`, `ledger`, `identity`, `semantic`, `runtime`, `ekl`, and more —
is a mature, fully-implemented system; the checkboxes were a **stale planning template**, never
updated after the real work shipped (the real completion history lives in the RFC-numbered entries
later in the same file). All confirmed-done items flipped to `[x]` directly in `TODO.md` with a
one-line evidence citation each, not left as a blanket claim.

**Four real, genuine findings surfaced, not glossed over**:
1. **RFC 0004 (Semantic Knowledge Ledger) was never written** — every later ledger RFC references
   it as an established foundation that had no actual document behind it. Written retroactively
   today, describing the design as it was actually built.
2. **`source_artifact_id`/full audit-trail provenance was never implemented** — a real, confirmed
   gap in `crates/ledger`, not superseded by anything (evidence-level provenance via `KirEvidence`
   covers a related but distinct need). Left `[ ]`, documented precisely in both `TODO.md` and
   RFC 0004.
3. **No Dockerfile existed anywhere in the repo** (Phase 0). Built `Dockerfile.dev` +
   `docker-compose.dev.yml`, live-verified by actually building the image and compiling a real
   crate inside the container.
4. **`docs/connector-guide.md` was never written** (Phase 3) — a real, if minor, gap; the substance
   it would document (rustdoc, ~10 real working connector examples) already exists.

One more finding worth naming even though it isn't a gap: the original Phase 4 plan for `postgres`/
`sqlserver` plugins (live-database connectors via `sqlx`/`tiberius`) was never built *as specified*
— but the real capability it wanted (schema/constraint/view extraction) shipped via a different,
deliberate architecture instead: DDL-text parsing (`sql_analyzer.rs` + the `sql-dialect-*` plugin
family, RFC 0031), no live database connection needed. A real design divergence, correctly
classified as superseded, not left miscategorized as either "done as literally specified" or "not
done."

---

## Docker development image (Phase 0)

### Problem / motivation

`TODO.md`'s Phase 0 spec: `Dockerfile.dev` at repo root based on `rust:1.XX-slim`, `build-essential`,
a pinned toolchain version, plus a `docker-compose.dev.yml`/Makefile that mounts the repo and runs
`cargo build --workspace` inside the container — for a machine with no local Rust installation.
Confirmed genuinely missing (no `Dockerfile*`/`docker-compose*` anywhere in the repo) before
building anything.

### What was built

| Component | Change |
|---|---|
| `Dockerfile.dev` | `rust:1.93-slim` base, `build-essential`+`pkg-config`, `rustfmt`+`clippy` components |
| `docker-compose.dev.yml` | Mounts the repo; named volumes for `target/` and the cargo registry |

### Implementation details worth remembering

- **Checked `ekos/Cargo.toml` directly before writing the system-dependency list, rather than
  copying a generic Rust-Docker-image checklist.** Only `zstd-sys`'s C build script actually needs
  a compiler — `rusqlite` uses its `bundled` feature (no system SQLite needed), `reqwest` uses
  `rustls-tls` (no OpenSSL), and nothing in the workspace needs `protobuf`/`onig`/etc. A generic
  "install every common C dependency" image would have worked too, just carried real, checkable-away
  bloat.
- **The Rust version (1.93) matches what this workspace is *actually* built with today
  (`rustc --version`), not a new pin invented for this image.** CI itself still floats on `stable`
  (`.github/workflows/ci.yml`'s `dtolnay/rust-toolchain@stable`) — this image exists for a
  reproducible local dev shell, not to introduce a repo-wide toolchain-pinning policy CI doesn't
  have. A future CI version bump won't silently desync from this image as long as both track
  "current stable" in spirit; revisit only if the project ever adopts a real `rust-toolchain.toml`.
- **Live-verified for real, not just "the image builds."** `docker compose -f
  docker-compose.dev.yml build` succeeded, then `docker compose -f docker-compose.dev.yml run --rm
  ekos cargo build -p ekos-kir` actually compiled a real EKOS crate inside the container end to
  end — real dependency resolution, a real C-backed crate build (`zstd-sys`), a real successful
  link. Scoped to one representative crate rather than the full `cargo build --workspace` inside
  Docker (which the native build had already proven moments earlier) — the container environment
  itself, not workspace-wide build correctness, was the only thing genuinely at risk of being
  wrong.

### Decisions (alternatives considered, why this choice)

- **No dedicated RFC for this.** `CLAUDE.md`'s mandatory RFC workflow is scoped to knowledge-model
  features; build/dev tooling (CI, this Dockerfile) has never individually carried its own RFC
  number anywhere in this project's history. Tracked here and in `TODO.md` instead, matching how
  the original CI pipeline itself was never RFC'd.

---

## Phase -1 through 13 audit — what was actually checked

Direct evidence per phase (representative citations; the full per-item citations now live inline
in `TODO.md` itself, not duplicated here):

- **Phase -1** (RFC process): `docs/rfcs/` — real RFCs 0001-0108 exist (minus 0004, now filled).
- **Phase 0** (bootstrap): workspace/CI/CLI skeleton all real; `tracing_subscriber` init confirmed
  in `crates/cli/src/commands/mod.rs`; Docker was the one real gap, now closed.
- **Phase 1** (`compiler-core`): `Compiler`/`PassManager`/`Scheduler`/`Diagnostics`/`Configuration`
  all present in `crates/compiler-core/src/`.
- **Phase 1.5** (walking skeleton): `crates/cli/tests/skeleton.rs` exists; `ekos query object`
  wired in `crates/cli/src/bin/ekos.rs`.
- **Phase 2** (artifact system): all five artifact types (`ObservationArtifact`/`KnowledgeArtifact`/
  `EvidenceArtifact`/`DiagnosticArtifact`/`IndexArtifact`) confirmed in `crates/artifact/src/lib.rs`.
- **Phase 3** (observation SDK): `Observer` trait real rustdoc confirmed; connector guide doc
  genuinely missing (finding #4 above).
- **Phase 4** (observation compiler): `sql-dialect-{postgres,mysql,mssql,snowflake,databricks,
  clickhouse}` plugins confirmed (the postgres/sqlserver architecture divergence above);
  `build::run` confirmed driving observation.
- **Phase 5** (KIR): all four primitives derive `Serialize`/`Deserialize` in `crates/kir/src/lib.rs`.
- **Phase 6** (recovery): `confluence_analyzer.rs`, `CachedLlmProvider`, real SQL fixtures
  (`northwind.sql`/`ecommerce.sql`/`mysql_hash_comments.sql`) serving the "golden dataset" role.
- **Phase 7** (identity): `IdentityResolver` trait + `DefaultResolver`, real confidence-scoring
  tests (0.99/0.85 thresholds, RFC 0060 finding) confirmed in `crates/identity/src/lib.rs`.
- **Phase 8** (semantic compiler): `validate()` + dangling-relationship test confirmed in
  `crates/semantic/src/lib.rs`.
- **Phase 9** (ledger): current/historical-state indexes confirmed; the audit-trail gap (finding
  #2 above) is this phase's one real miss.
- **Phase 10** (runtime): `pub struct Runtime<'a>` confirmed read-only wrapper.
- **Phase 11** (AI runtime): `pub trait LlmProvider` confirmed provider-agnostic.
- **Phase 12** (EKL): `parser.rs`/`interpreter.rs` confirmed.
- **Phase 13** (optimizer): already mostly `[x]` before this audit; unchanged.

---

## Files Changed

| File | Change summary |
|---|---|
| `Dockerfile.dev` | New |
| `docker-compose.dev.yml` | New |
| `docs/rfcs/0004-semantic-knowledge-ledger.md` | New — retroactive RFC |
| `TODO.md` | ~78 stale checkboxes flipped to `[x]` with evidence; 4 items given nuanced corrections (RFC 0004, audit-trail gap left `[ ]`, SDK docs gap marked `[~]`, postgres/sqlserver marked superseded) |
