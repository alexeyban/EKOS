# Devlog 191 — RFC 0149: an extension seam, and the binary decompiler goes private

**Date:** 2026-09-18
**PRs:** (single working tree, committed directly to `main`)
**Branch:** `main` (local, pushed)

---

## Summary

The maintainer wants the core and all documentation to stay public, but the RFC 0148 binary
decompiler to be sold separately as a service. A private crate cannot be an optional dependency of
a public one, so public EKOS gained a generic extension seam (RFC 0149), and the decompiler moved
into the private `alexeyban/ekos-binary` workspace as the seam's first user. The public `ekos`
binary no longer recovers compiled binaries. The private `ekos` binary is the same CLI plus the
decompiler. Verified end to end on the same JAR: public recovers 0 `BinaryType` objects and lists
no `ekos_binary_explain`; private recovers 36 and explains them.

---

## RFC 0149 — the extension seam

### Problem / motivation
Every observer, pass and MCP tool was hardwired into `build.rs`, `recover.rs`, `commit.rs` and
`mcp.rs`. Cargo resolves every git dependency, optional ones included, when it writes
`Cargo.lock`. A public `optional = true` dependency on a private repo would therefore break every
public build. The only workable direction is for the private crate to depend on the public ones,
which needs a place to plug in.

### What was built
| Component | Change |
|---|---|
| `cli/src/extension.rs` | `EkosExtension` (`observers`, `recovery_passes` → `RecoveryContribution { pass, report }`, `after_commit`, `mcp_tools`, `call_mcp_tool`, `logic_version`), `RecoverContext`, `CommitContext`, `Extensions` |
| `cli/src/app.rs` | The old `bin/ekos.rs` moved here as `pub async fn main_with(Extensions)`. `bin/ekos.rs` is now a shim calling it with `Extensions::none()` |
| `build`/`recover`/`commit`/`mcp` | `run_with`/`handle_message_with` variants. The old signatures delegate with `Extensions::none()`, so no public API break |
| `architecture investigate`, cluster compile worker | `InvestigateOptions.extensions`, `compile_worker_run_with`: the pipeline runs extensions there too |
| Build fingerprint | `PIPELINE_LOGIC_VERSION + 1000 × Extensions::logic_version()`. The extension term is 0 with no extensions, so public cache keys are unchanged |
| `commit` | Warns when `[binary-reconstruction]` is enabled but no extension is installed |
| `cli/tests/extension_seam.rs` | A fake extension proves each hook runs, that an unknown tool still errors, and that `none()` lists nothing extra |

### What moved to `alexeyban/ekos-binary`
`crates/binary`, `plugins/binary`, `recovery/src/binary_analyzer.rs`,
`binary_reconstruction.rs`, and `ekos_binary_explain` (definition, handler and test), plus a new
`BinaryExtension` and an `ekos` binary. There are 130 tests in total, all passing, including the
real pdfbox/mscorlib corpus tests.

### What stayed public, deliberately
- The `[binary-reconstruction]` config struct: `EkosConfig` is `deny_unknown_fields`, so removing
  it would break any `ekos.toml` that sets it.
- The `Binary*` custom-kind rows and identity's `is_expected_binary_declaration_group`: `resolve`
  is public and must handle a ledger a private build wrote.
- RFC 0148, devlogs 189/190, and the capabilities documentation.

### Decisions
- **Explicit parameter, not `EkosConfig`.** Config lives in `compiler-core`, below `ledger` and
  `observation-sdk`, so it could only hold extensions as `dyn Any`.
- **No `dlopen`.** Rust has no stable ABI, and it would need `unsafe`.
- **Public history not rewritten** (the maintainer's call). Commits `4c12e5f`/`60ce60e` still
  contain the decompiler and stay MIT-licensed to anyone who obtained them. Only future work is
  private.
- The TSD documentation repo (`alexeyban/tsd-system-documentation`) was made public; the
  documentation is the showcase.

---

## Knowledge Captured

- **`dyn KnowledgeStore` is not `Sync`, so a hook that uses it across `.await` cannot return a
  `Send` future.** The first draft of `EkosExtension` used plain `#[async_trait]`. The public
  fake-extension test passed because its `after_commit` never touched the ledger. The real private
  reconstruction step then failed to compile. The trait is now `#[async_trait(?Send)]`, and the
  test reads the ledger across an `.await` so the requirement is pinned. Make a seam's test fakes
  use the same inputs the real implementations need, or the fakes will hide trait-bound mistakes.
- **Keep old entry-point signatures and add `*_with` variants.** Changing `compile_worker_run`'s
  arity compiled fine in the main workspace and broke only `tests/integration`, a separate Cargo
  workspace that the main `cargo test --workspace` never builds. Always run
  `cd tests/integration && cargo test` after touching a `pub` CLI function.
- The private workspace takes public crates by `git = ".../EKOS", branch = "main"`. Its
  `Cargo.lock` pins the revision, and a commented `[patch]` block switches it to a sibling
  checkout for local development. Cargo finds nested workspace members in a git repo by package
  name, so no path is needed per crate.

---

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0149-extension-seam-open-core.md` | New RFC |
| `ekos/crates/cli/src/extension.rs` | New: the seam |
| `ekos/crates/cli/src/app.rs` (was `bin/ekos.rs`) | `main_with(Extensions)`; threads extensions to every pipeline-running command |
| `ekos/crates/cli/src/bin/ekos.rs` | Three-line shim |
| `ekos/crates/cli/src/commands/{build,recover,commit,mcp,architecture,cluster}.rs` | `*_with` variants and extension hooks; binary wiring removed |
| `ekos/crates/cli/tests/extension_seam.rs` | New seam tests |
| `ekos/crates/recovery/src/{binary_analyzer,binary_reconstruction}.rs`, `ekos/crates/binary/`, `ekos/plugins/binary/` | Removed (moved to the private repo) |
| `ekos/Cargo.toml`, `crates/{cli,recovery}/Cargo.toml` | Members and dependencies removed; `async-trait` added to `cli` |
| `ekos/docs/rfcs/0148-*.md` | Status notes the move |
| `README.md`, `CLAUDE.md`, `docs/generated/ekos-self-documentation.html`, `TODO.md` | Binary recovery described as a separately licensed extension build |
