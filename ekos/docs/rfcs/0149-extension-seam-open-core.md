# RFC 0149 — Extension seam: out-of-tree connectors and the private binary decompiler

**Status:** Accepted
**Date:** 2026-09-18
**Supersedes:** none
**Related:** RFC 0148 (compiled .NET/JVM binary recovery), RFC 0013 (MCP server),
RFC 0135 Part A (build fingerprint logic version), RFC 0088 (the post-`commit` LLM slot)

---

## Summary

Adds a generic, public **extension seam** to the `ekos` CLI crate. An out-of-tree crate can
contribute observers to `ekos build`, passes to `ekos recover`, a post-`commit` step and MCP tools.
A binary built with `ekos::app::main_with(extensions)` gets all of them, and the public `ekos`
binary is simply `main_with(Extensions::none())`.

The first user is RFC 0148's binary decompiler. It moves out of this repository into the private
`alexeyban/ekos-binary` repo, so that the decompiler can be sold as a service independently of
the open-source core. Everything else (the compiler pipeline, every other connector, the ledger,
the runtime, MCP and all documentation, RFC 0148 included) stays public.

## Motivation

The maintainer wants the core and its documentation public, but wants the compiled-binary
decompiler (the .NET/JVM readers, the binary analyzer pass, the LLM business-logic reconstruction
and `ekos_binary_explain`) to be a separately sold capability.

A private crate cannot be an optional dependency of a public one. Cargo resolves *every* git
dependency, optional ones included, when it writes `Cargo.lock`, so a public workspace naming a
private repository fails to build for everyone without access. The dependency has to point the
other way: the private crate depends on the public crates, and the public binary must offer a
place to plug it in. Today there is no such place, because every observer, pass and MCP tool is
hardwired into `build.rs`, `recover.rs`, `commit.rs` and `mcp.rs`.

## Design

### The trait (`ekos::extension`)

```rust
#[async_trait]
pub trait EkosExtension: Send + Sync {
    /// Stable identifier, shown in diagnostics.
    fn name(&self) -> &'static str;
    /// Folded into `ekos build`'s fingerprint cache key (RFC 0135 Part A). Bump it whenever the
    /// extension's observer output changes, exactly like `PIPELINE_LOGIC_VERSION`.
    fn logic_version(&self) -> u32 { 0 }
    /// Observers appended after the built-in ones in `ekos build`.
    fn observers(&self, config: &EkosConfig) -> Vec<Box<dyn Observer>> { vec![] }
    /// Passes for `ekos recover`, built from the artifact store the observers wrote.
    fn recovery_passes(&self, ctx: &RecoverContext<'_>) -> Vec<RecoveryContribution> { vec![] }
    /// Runs in `ekos commit` after `[llm-description]` and before `[embeddings]`, i.e. the slot
    /// RFC 0148's reconstruction occupied, over the fully committed ledger.
    async fn after_commit(&self, ctx: &CommitContext<'_>) -> anyhow::Result<Vec<String>> { Ok(vec![]) }
    /// Extra MCP tool definitions for `tools/list`.
    fn mcp_tools(&self, config: &EkosConfig) -> Vec<serde_json::Value> { vec![] }
    /// `Some` if this extension owns `name`. Read-only: gets the same cached store every other
    /// read tool uses.
    fn call_mcp_tool(&self, name: &str, args: &Value, ledger: &dyn KnowledgeStore)
        -> Option<anyhow::Result<Value>> { None }
}
```

- `RecoveryContribution` pairs a `Box<dyn CompilerPass>` with a report closure. After the pass
  manager runs, `recover` prints the closure's lines alongside the built-in passes' own summary
  lines. This is how a pass reports counts it accumulated behind a stats handle, the way every
  built-in analyzer does.
- `Extensions` is a cheap `Clone` newtype over `Arc<[Arc<dyn EkosExtension>]>`. It is threaded
  explicitly as a parameter (dependency injection, not global state), so it follows the same
  paths the config already does.

### Threading

- `build::run`, `recover::run`, `commit::run`, `mcp::run` and `mcp::handle_message` keep their
  signatures and delegate to new `*_with(..., &Extensions)` variants, passing
  `Extensions::none()`. Every existing caller, including the integration tests, is unchanged.
- The pipeline-running commands (`architecture`, `cluster` compile-worker) and `bin/ekos.rs`
  thread the real set through, so a private build's decompiler also runs inside them.
- `bin/ekos.rs`'s CLI definition and dispatch move into `ekos::app` as `main_with`. The binary
  becomes a three-line shim.

### What stays public

- `[binary-reconstruction]` in `EkosConfig`. The struct is a config schema, not an algorithm, and
  `EkosConfig` is `deny_unknown_fields`, so removing it would break every existing `ekos.toml`
  that sets it. The public build warns when it is enabled but no extension consumes it.
- The `Binary*` rows in `ekos_kir::custom_kinds::REGISTRY` and identity's
  `is_expected_binary_declaration_group`. They are object-kind names and a merge-policy rule over
  them, and `ekos resolve` is a public stage that must handle a ledger a private build wrote.
- RFC 0148, devlogs 189/190 and the capabilities documentation.

### What moves to `alexeyban/ekos-binary` (private)

| From (public) | To (private) |
|---|---|
| `ekos/crates/binary` (`ekos-binary`, the ECMA-335 and class-file readers) | `crates/binary` |
| `ekos/plugins/binary` (`ekos-plugin-binary`, `BinaryObserver`) | `crates/plugin-binary` |
| `recovery/src/binary_analyzer.rs` (`BinaryAnalyzerPass`) | `crates/binary-recovery` |
| `recovery/src/binary_reconstruction.rs` | `crates/binary-recovery` |
| `ekos_binary_explain` (definition, handler, tests) in `cli/src/commands/mcp.rs` | `crates/binary-recovery` |
| — | `BinaryExtension: EkosExtension` plus an `ekos` binary calling `main_with` |

The private workspace depends on the public crates by git URL at a pinned revision, recorded in
its `Cargo.lock`, with a commented `[patch]` block for local path development against a sibling
checkout.

## Alternatives considered

- **Cargo feature on the public CLI with an optional private git dependency.** Rejected: Cargo
  resolves optional git dependencies for the lockfile, so every public build would fail to fetch
  the private repo.
- **Dynamic loading (`dlopen` of a `cdylib`).** Rejected: Rust has no stable ABI, it would
  need `unsafe` (the coding rules forbid it without an RFC justifying it), and it buys nothing
  over static linking into a second binary.
- **Carrying the extensions inside `EkosConfig`.** Rejected: `EkosConfig` lives in
  `compiler-core`, below `ledger` and `observation-sdk` in the dependency graph, so it could hold
  them only as `dyn Any`. It also mixes runtime wiring into a serializable config.
- **Rewriting public git history to purge the already-published decompiler.** The maintainer
  decided against it. Versions already pushed (commits `4c12e5f` and `60ce60e`) stay retrievable
  and MIT-licensed to anyone who obtained them. Only future work is private.

## Testing

- The public workspace builds, and all its tests pass, with no binary code present. The MCP
  tool-list test no longer expects `ekos_binary_explain`.
- New unit tests with a fake extension check that its observer runs in `build`, its pass and
  report lines run in `recover`, its `after_commit` runs in `commit`, and its MCP tool is listed
  and dispatched. An unknown tool still errors.
- The private workspace carries every moved test, plus an end-to-end check that its `ekos`
  binary recovers `BinaryType` objects from a real assembly.
