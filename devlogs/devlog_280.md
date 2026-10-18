# Devlog 280 — JVM statement recovery, compiled Pentaho plugins, community verification

**Date:** 2026-10-18
**PRs:** none (local `main` in all three repos, not pushed)
**Branch:** EKOS main (this commit); ekos-binary 0e3619e (private); ekos-lakehouse da7f632, ca3431f, 3639385
**Design:** RFC 0206 (new, design public / code private), RFC 0204 §M6 (handed to the community), RFC 0205 bench

---

## Summary

Follow-up to devlog_279, on the maintainer's three decisions. **M7 shipped:** the private
ekos-binary build recovers JVM bytecode to statements (93.8% of pdfbox.jar's method bodies)
and offers a `JavaDecompiler`, so a compiled Kettle plugin step now translates through the
public lakehouse translator — tested end to end with a real compiled class. **M6** (running the
original transformations in PDI against the dbt result) is written up for the community to run,
with an issue template and an X post drafted. **The model bake-off** for RFC 0205 is built and
ready but did not run: the OpenCode key had expired (401 on the first call, $0 spent).

---

## RFC 0206 — JVM statement recovery (ekos-binary 0e3619e)

### What was built
| Part | What |
|---|---|
| `jvm_lift.rs` | cafebabe opcodes → owned instructions → blocks → stack simulation into RFC 0150's IR, then the shared structurer |
| `render_java.rs` | Java-like source: calls on `this` without a receiver, typed declarations, catch variables and temporaries declared once |
| `jvm.rs` | every JVM method with code gets IR and `Statements`/`ControlFlow` fidelity (logic version 4) |
| `jvm_decompiler.rs` | `JavaDecompiler` (jar or class file → `DecompiledClass`); `BinaryExtension::java_decompiler()` offers it |
| IR | `Expr::InstanceOf`, `Expr::map`, `Stmt::map_exprs` |

### Measured
pdfbox.jar (652 classes): 5,384 of 5,741 bodies `Statements` (93.8%), 355 `ControlFlow` (top
reason: gotos the structurer did not reduce), 2 not lifted; 0.31 s for the jar in release.
Golden fixtures compiled with javac 21 (`--release 11 -g` and `--release 8 -g:none`) are
committed with their sources; `AcmeDoubler` (a Kettle-shaped plugin against stub API classes)
decompiles to exactly the UDJC idioms RFC 0203 executes.

### Implementation details worth remembering
- **`fcmpl` vs `fcmpg` decide NaN.** The branch after them is taken on NaN when `nan <op> 0`;
  such a `<`/`<=`/`>`/`>=` is an unordered compare, an ordered one is emitted as
  `!(l <inverse>.un r)` so the structurer's later negation stays exact. `==`/`!=` need nothing:
  Java's own operators already are the ordered/unordered pair. The IR's `negate` judges
  float-ness from operand *shape*, and a `float` local has none — the lifter must decide.
- **A body with `goto` left must not be `Statements`**, whatever the structurer reports:
  `COSArray.indexOfObject` came back with labels and an empty gap list. Guarded in the lifter.
- **Booleans arrive as 0/1** through `?:` merges the lifter cannot see; a post-pass fixes
  returns of `boolean` methods and folds `c ? true : false`.
- **Java 8 concatenation is `StringBuilder` chains**; folded to `+` so both targets render the
  same (and RFC 0203 can read it). `append('!')` needs a `(char)` conversion, or it reads 33.
- **The JVM records only a handler's start.** Extent = blocks reachable from the handler entry
  but not from the method entry, contiguous in layout order (javac's try / goto / handler).

## ekos-lakehouse 3639385 — the executor reads decompiled code
Decompiled `processRow` declares locals first, assigns `r = getRow()`, inverts the guard
(`if (r != null) { … } setOutputDone(); return false;`), spells autoboxing out and starts a
concatenation with `""`. All handled now. Two real executor bugs surfaced: an empty literal in a
Java concatenation was read as Kettle's empty-is-null (printing `"null"`), and a local named
like a field shadowed `get(Fields.In, "f")`. A plugin's output fields come from its code (its
Meta class), so fields the code adds are accepted for plugin steps, with a note.

## M6 handed to the community (ekos-lakehouse ca3431f)
`docs/pentaho-verify.md`: translate → `pan.sh` into a scratch schema → `dbt build` → count and
`EXCEPT ALL` both ways in Trino; `.github/ISSUE_TEMPLATE/pentaho-verification.md` asks for
counts and labels, never row values. X post draft: `marketing/drafts/2026-10-18-pentaho-verify.md`
(links point at GitHub `main`: post after pushing).

## RFC 0205 bench (ekos-lakehouse da7f632)
`examples/draft_bench.rs`: set A = the corpora's 32 gap code steps (drafted? Trino plans it?),
set B = 15 steps the translator handles, drafted anyway and compared row by row with the
deterministic SQL on Trino over synthetic edge rows (`examples/bench-gold/gold.ktr`: JS
replace-first, float division, null concatenation, skipped rows, UDJC, Janino). Candidates:
Claude Haiku 5.5, MiniMax M3, GPT-6 Luna, $1.00 cap via the router's budget. **Not run:** the
OpenCode key expired.

---

## Knowledge Captured

- **`grep | head` after a gate always "succeeds"** — gate on the command's own exit status.
- **Patching EKOS in a downstream workspace means every EKOS crate**: ekos-binary needed the
  lakehouse's full list once it depended on lakehouse crates (mixed copies of shared types).
- **javac is available** only from PyCharm's JBR (`…/pycharm-2025.3.3/jbr/bin/javac`, 21.0.10).
- Synthetic-row agreement against the deterministic translation is a cheap correctness oracle
  for model drafts — use it before trusting a "passes the checks" rate.

---

## Files Changed

| Repo / file | Change |
|---|---|
| EKOS `ekos/docs/rfcs/0206-jvm-statement-recovery.md` | the design |
| EKOS `devlogs/devlog_280.md`, `TODO.md`, capabilities page | this record |
| EKOS `marketing/drafts/2026-10-18-pentaho-verify.md` | the X post draft |
| ekos-binary `crates/binary/src/{jvm_lift,render_java}.rs`, `jvm.rs`, `ir.rs`, `render.rs`, `structure.rs` | lifter, renderer, wiring, IR |
| ekos-binary `crates/binary/tests/{jvm_golden.rs,fixtures/jvm/*}`, `examples/{jvm_fidelity,jvm_class}.rs` | golden tests, measurement |
| ekos-binary `crates/binary-recovery/src/{jvm_decompiler,extension}.rs`, `tests/pentaho_plugin.rs`, `Cargo.toml` | decompiler, hook, end-to-end test, full `[patch]` |
| ekos-lakehouse `crates/kettle-expr/src/{script,java}.rs`, `kettle-rel/src/steps.rs` | executor fixes, plugin outputs |
| ekos-lakehouse `docs/pentaho-verify.md`, `.github/ISSUE_TEMPLATE/…`, `examples/draft_bench.rs` | M6 write-up, bench |
