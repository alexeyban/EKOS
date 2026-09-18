# Devlog 189 — RFC 0148: compiled .NET and JVM binaries

**Date:** 2026-09-18
**PRs:** (single branch, merged locally)
**Branch:** `feat/binary-analyzer` → `main` (squash-merged)

---

## Summary

EKOS compiles binaries now. RFC 0148 adds `crates/binary` (readers for .NET CLI metadata and JVM
class files), `plugins/binary` (observer), `recovery/src/binary_analyzer.rs` (deterministic
structural analyzer) and `recovery/src/binary_reconstruction.rs` (opt-in LLM business-rule
reconstruction with real confidence scoring), plus the `ekos_binary_explain` MCP tool.

This is the "no readable source" wedge taken to its limit: not a legacy ETL format nobody can
read, but a program whose source is *gone*. Verified end-to-end on two real binaries this repo
does not own — `pdfbox.jar` and wine-mono's `mscorlib.dll` — producing **3,717 types, 33,323
methods, 18,861 fields, 187 external I/O boundaries and 13,077 resolved call edges**, through a
clean `resolve` and a `compile` with zero warnings.

The incoming design called for out-of-process decompiler sidecars. That was rejected on this
project's own precedent and replaced with in-process pure-Rust readers; the reasoning, and what it
costs, is below. Two real defects surfaced during live verification and were fixed in the same
change.

---

## PR — RFC 0148: binary recovery

### Problem / motivation

Compiled .NET and Java binaries are the same failure mode as Pentaho `.ktr` files at much larger
scale: in-house line-of-business apps, acquired-company systems, unmaintained vendor DLLs and
JARs, where the source was lost or never checked in. They routinely encode pricing, eligibility
and tax rules that exist nowhere else. Until now EKOS saw a `.dll` as an opaque `File` object.

### What was built

| Component | Role |
|---|---|
| `crates/binary` (`ekos-binary`) | `DecompiledAst` + `jvm` backend (cafebabe + zip) + `dotnet` backend (hand-written ECMA-335 reader: PE → CLI header → metadata streams → tables → IL bodies) + magic-byte detection + the I/O-boundary classifier |
| `plugins/binary` (`BinaryObserver`) | One artifact per recovered **type**, carrying its `DecompiledAst` as JSON |
| `recovery/src/binary_analyzer.rs` | `BinaryAssembly`/`BinaryType`/`BinaryMethod`/`BinaryField`/`ExternalIoBoundary` objects, `Contains`/`Extends`/`Calls`/`References`/`DependsOn` edges, and one RFC 0027 `TransformGraph` per method |
| `recovery/src/binary_reconstruction.rs` | Opt-in `[binary-reconstruction]` LLM stage: slice → prompt → locator validation → confidence → `accepted`/`unconfirmed` `BinaryRule` objects |
| `ekos_binary_explain` (MCP) | Read-only; keeps deterministic facts and inferred rules in separate, separately-attributed sections |
| `identity::is_expected_binary_declaration_group` | Narrows the cross-kind conflict detector for the mechanically-guaranteed binary declaration collisions |
| Eight registries | `custom_kinds` (+ CI guard), `doc_links`, `llm_description`, three `docs-gen` lists + `render_api` grouping, CLI `build.rs`/`recover.rs` |

### Decisions (alternatives considered, why this choice)

**Sidecars rejected; in-process readers instead.** The incoming design specified a .NET console
app wrapping ICSharpCode.Decompiler and a JVM jar wrapping Vineflower, spawned per file over
stdio JSON. RFC 0147 had already rejected shelling out to `perl` on three grounds, and the first
two apply identically: it needs a toolchain wherever `ekos recover` runs (this machine has no
`dotnet` at all), and it makes ledger facts depend on which decompiler version happened to be
installed, which is the opposite of reproducible builds. A third is CLAUDE.md's own rule that
passes be deterministic and side-effect-free. A fourth is specific to binaries: the draft itself
noted that sandboxing a subprocess reading an untrusted customer DLL needs a security review
before shipping — reading bytes in-process with `#![forbid(unsafe_code)]` and no execution means
there is nothing to sandbox.

**What that costs, named rather than hidden:** no statement-level reconstruction.
`DecompiledAst::fidelity` is an explicit enum (`Structural` today, `Statements` reserved), carried
onto every artifact and told to the LLM in its own prompt, so nothing downstream can quietly
assume it has control flow it was never given. The sidecar remains defined-but-unbuilt behind the
same seam.

**The .NET reader is hand-written because both candidate crates failed on real files.**
`dotnetdll` 0.3 is GPL-3.0+ and cannot be linked into this MIT workspace at all — not a judgement
call. `dotscope` 0.9.1 is Apache-2.0 and otherwise suitable, but **parsed only 77 of 147 real
Mono assemblies**: it aborts the *entire file* inside its custom-attribute loader on one blob it
mis-reads. See Knowledge Captured — that 48% failure rate is the single most important finding of
this session.

**`Calls` edges only to methods recovered in the same run.** A compiled method calls hundreds of
framework methods. Materializing an object for each would bury the real business graph under
`java.lang.StringBuilder.append`, and inventing objects for code EKOS has never read would be
fabrication. Unresolved targets survive as the method's own `call_targets` property — evidence,
without a fake edge. Reported explicitly in `recover` output (13,077 resolved / 92,625 not) so a
low edge count reads as a resolution outcome rather than a parse failure.

**Phases 3 and 4 shipped together, as a hard constraint.** The RFC names "Phase 3 before Phase 4"
as its own top risk: a wrong inferred rule is indistinguishable from a right one without the
machinery that scores it. So the generative stage never landed without its guards.

---

## Knowledge Captured

- **`dotscope` 0.9.1 returns nothing for ~48% of real .NET assemblies, and fails loudly enough to
  look fine.** Measured: 77 of 147 wine-mono managed DLLs parsed; the other 70 returned
  `Err` from one mis-parsed custom-attribute blob, aborting the whole file. This is exactly the
  all-or-nothing parse failure that cost LedgerSMB its entire 103-table schema under RFC 0146,
  and it is why `crates/binary` is written by hand and why its central rule is **degrade per row,
  never per file**. v1 does not read custom attributes at all. If anyone revisits this crate
  later, re-measure on a real corpus before adopting it — the README will not tell you.

- **Most of wine-mono's `4.5` profile is *facade* assemblies, and a facade looks exactly like a
  broken reader.** `System.Xml.dll` there is 135 KB with full metadata (269 types, 2,761 methods)
  and **every method body compiled to `ldnull; throw`** — 1,434 occurrences of the three-byte
  stub. The first version of the .NET test asserted a large call graph against it and failed with
  "only 339 call sites", which is indistinguishable from a broken IL walk until you hexdump one
  body. `mscorlib.dll` (4.6 MB) is the real implementation and yields 1.38 MB of decoded IL,
  79,263 call sites and 37,821 branches. **Pick the implementation assembly, not the facade, for
  any binary-reader test** — and a recovered type count with zero branches is a facade signature,
  not a bug.

- **A `.dll` is usually *not* a .NET assembly, and the difference must be reported distinctly.**
  Detection is by magic bytes and by PE data directory 14 (the CLI header), never by extension.
  `Detected::NativePe` is a separate outcome from `Unknown` precisely so "we skipped 400 native
  DLLs" cannot be mistaken for "we found nothing in 400 assemblies" — on any Windows-adjacent
  tree the native case dominates.

- **ECMA-335 forces you to implement the schema for *every* table, even to read a dozen.**
  Metadata tables are stored back-to-back with no index and no per-table offset, so finding
  `MethodDef` (0x06) requires the exact row size of every present table below it — which depends
  on the `HeapSizes` byte, on other tables' row counts (an index widens from 2 to 4 bytes past
  2^16 rows), and on coded-index tag widths. A missing or wrong row shifts every later table by a
  few bytes and yields *plausible garbage* rather than an error. A CI test asserts every
  spec-defined table id 0x00–0x2C has a schema row, because that is the one mistake that is
  invisible at runtime.

- **Identity: `BinaryMethod` vs `BinaryField` name collisions are mechanically guaranteed, and
  produced 258 conflicts that failed `resolve` by default.** Every C# property compiles to a
  backing field plus `get_`/`set_` methods; `.ctor` normalizes to `ctor` across every type in an
  assembly at once; a field is routinely named after its own type (`private Interop interop;`).
  Fixed by `is_expected_binary_declaration_group`, covering exactly
  `{BinaryType, BinaryMethod, BinaryField}` — the third precedent of this shape after RFC 0093
  (Technology/JsModule) and RFC 0147 (PerlPackage/PerlSymbol). `BinaryAssembly` and
  `ExternalIoBoundary` are deliberately *outside* the set so those collisions still surface.
  **Any new analyzer emitting more than one kind per source unit should expect to write one of
  these.**

- **One artifact per *type*, not per file — a .NET assembly needs splitting just as a jar does.**
  The first end-to-end run produced a single **34 MB artifact** for `mscorlib.dll`: one blob that
  re-hashes in its entirety when a method changes, cannot be diffed, and defeats the
  content-addressed store. `DecompiledAst::split_by_type` makes both backends emit the same unit.
  The JVM side was already correct only because a class file holds exactly one class.

- **Dangling edges are not free: they cost one `SEM002` warning each.** Emitting `Extends` to
  `java.lang.Comparable` and `DependsOn` to un-observed assemblies produced **2,639 compile
  warnings** on a two-binary workspace. Dropping the edges would have deleted the answer to "what
  implements this interface". The fix is `perl_analyzer`'s established shape: materialize one
  thin, deterministically-keyed object marked `external: true` per referenced entity (+449
  objects, zero warnings).

- **`ConflictingEvidence` does not exist in this codebase.** The incoming draft routed
  low-confidence reconstructions through "the existing `ConflictingEvidence` diagnostic path (on
  the Wave roadmap already)". Checked before building on it: there is no such path anywhere. The
  real convention is `status: "unconfirmed"` (`semantic/src/lib.rs`, for candidate `SameAs`
  relationships). **Verify an inherited design's claims about existing machinery before wiring to
  it** — the RFC was corrected rather than the code bent to fit.

- **Hallucination is defeated by checking, not by prompt wording.** Every reconstructed rule must
  cite `evidence_locators`, and each is checked against the exact set its own prompt contained.
  Invented locators are dropped and counted; a rule left with none is discarded outright. The
  count then feeds confidence, because a model that invented one citation demonstrated it will
  invent, and the rules it got right came from the same generation. Confidence multiplies the
  model's self-report by that honesty penalty and by cross-reference agreement, and **can only
  reduce, never promote** — a confidently wrong model is the failure mode being guarded against.

- **`execute` is not a write verb.** The I/O direction heuristic prefix-matched `execute`, which
  classified JDBC's `executeQuery` (a SELECT) as a `Sink`. Caught by the module's own test.
  `executeUpdate`/`executeNonQuery`/`executeBatch` are named individually instead. A loose prefix
  match over API verbs will misclassify every read in a codebase.

- **`iconst_*`/`ldc.i4.*` literals are noise and are deliberately excluded.** They are loop
  bounds, array indices and boolean returns. The numbers a business rule actually turns on —
  thresholds, rates, limits, codes — do not fit in those opcodes and arrive as
  `bipush`/`sipush`/`ldc`. Emitting the short forms would bury the real constants under thousands
  of 0s and 1s in the searchable `excerpt`.

- **RFC 0043 redaction covers the binary path for free, and it matters more here than elsewhere.**
  `build.rs` runs `redact_json` over the whole artifact `data`, so string constants lifted out of
  IL are redacted before storage — verified by reading the call site, not assumed. Hard-coded
  connection strings are common in exactly the binaries that have lost their source.

- **`main` had four pre-existing `clippy -D warnings` failures** (`runtime/src/ai.rs` ×2,
  `sql_analyzer.rs`, `local_docs_analyzer.rs`) under rustc 1.98's clippy, in files untouched by
  this work. Fixed here so the local gate is green; worth knowing that a clippy version bump
  silently breaks the gate for whoever runs it next.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0148-binary-decompilation-recovery.md` | New RFC — the sidecar rejection and its evidence, fidelity levels, data model, phases, risks |
| `ekos/crates/binary/**` | New crate: `ast.rs`, `detect.rs`, `io_classify.rs`, `jvm.rs`, `dotnet/{mod,pe,metadata,il,sig}.rs` + 90 tests |
| `ekos/plugins/binary/**` | New `BinaryObserver` + 6 tests |
| `ekos/crates/recovery/src/binary_analyzer.rs` | New deterministic analyzer + 12 tests |
| `ekos/crates/recovery/src/binary_reconstruction.rs` | New LLM reconstruction stage + 15 tests |
| `ekos/crates/recovery/src/lib.rs` | Module + re-exports |
| `ekos/crates/kir/src/custom_kinds.rs` | Six new rows (all `structurally_keyed: true`) + guard list |
| `ekos/crates/identity/src/lib.rs` | `is_expected_binary_declaration_group` + 3 tests |
| `ekos/crates/semantic/src/doc_links.rs` | `CODE_KINDS` |
| `ekos/crates/recovery/src/llm_description.rs` | `MODULE_KINDS` / `SYMBOL_KINDS` |
| `ekos/crates/docs-gen/src/lib.rs` | Entity pages, `is_symbol_kind`, doc-bearing kinds, `render_api` grouping |
| `ekos/crates/compiler-core/src/config.rs` | `[binary-reconstruction]` section |
| `ekos/crates/cli/src/commands/{build,recover,commit,mcp}.rs` | Observer, pass + stats, reconstruction step, `ekos_binary_explain` + test |
| `ekos/Cargo.toml`, `crates/{recovery,cli}/Cargo.toml` | Workspace members, `cafebabe`/`zip` deps |
| `ekos/crates/runtime/src/ai.rs`, `recovery/src/{sql,local_docs}_analyzer.rs` | Pre-existing clippy fixes |
