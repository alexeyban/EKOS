# Devlog 192 — RFC 0150: statement-level .NET recovery, checked against source

**Date:** 2026-09-19
**PRs:** (working tree; implementation is in the private `alexeyban/ekos-binary` repo, this repo gets the RFC and docs)
**Branch:** `main` (local)

---

## Summary

RFC 0148 recovered a compiled binary's *structure*. RFC 0150 (this session) recovers what a .NET method
*does*: an in-process CIL decoder, stack simulation, CFG and structuring pass produce `if`/loop/`switch`/
`try` statements with the IL offset of every line, labelled with the fidelity they actually reached
(`structural` / `control_flow` / `statements`, per method). On top: a per-method spec in
`ekos_binary_explain`, `ekos_binary_migration_check` (a Python rewrite compared with the original), and a
`binary-migration-planner` agent. Five Phase 0 gaps from devlog_190 were closed first. Every claim below is a
measurement against either the CFG or the published source of the same build.

The implementation is private (RFC 0149). Public changes: RFC 0150 and this devlog.

---

## Phase 0 — five gaps, and four bugs found by finally running the LLM stage

| Gap | Result |
|---|---|
| I/O classifier matched type-name *prefixes* | Rules name exact types/namespaces, methods and first-parameter type; specific beats broad; exception types never count. `DataSet.ReadXml/WriteXml(path)` now file I/O; `SerialPort`/sockets get `device`/`network` kinds |
| `call_targets` cut at 32, no marker | Own cap of 512 (largest real method: 70); any cut list records `<list>_truncated` + `_total` |
| Obfuscated/packed binaries documented as clean | Detector with 4 measured signals; calibrated on **3,133 clean binaries / 3.14M identifiers → 0 false positives**; flags two real ProGuard jars; `packed` is refused by `ekos_binary_explain` |
| `ekos_binary_explain` ignored unknown args | `method` declared; any undeclared argument is an error |
| LLM stage had never met a real provider | See below |

**First real LLM run (llama3 8B via Ollama, TSDServer): 4 code bugs, then a precision measurement.**
The bugs: the hallucination penalty was never applied (a comment said "the caller folds it in"); byte-slicing a
Cyrillic condition panicked; rule ids and the agreement index were keyed by locator alone, and a .NET locator
is a metadata token that recurs in every assembly, so one binary's rules overwrote and "contradicted"
another's; "corroboration" raised confidence though slices are disjoint. Confidence now only goes down.

**Precision, measured by hand against source on a 30-rule random sample (29 accepted): 2 correct
non-trivial rules, 5 trivially true (accessor descriptions), 22 invented.** Every rule scored 1.0. The model
had no statements and was asked for "business rules" of generated plumbing (DataSet designer rows, event
`Invoke`, COM stubs). Fixes: pass the recovered pseudo-code, tell the model to emit nothing for plumbing, and
drop methods with no decision/constant/I/O from slices.

**After the fixes (same model, same binary, 47 slices, new random sample of 30 accepted rules, graded
against the recovered statements):** 4 verified correct and non-trivial (`DataTable.Close`,
`importDocBtn_Click`, `DeleteOldDB`, `downloadBtn_Click`), 5 plausible but unverified, 17 accurate but trivial
descriptions of generated plumbing (typed-DataSet `DBNull` getters, `OnRowDeleted`, constructors), **4 with an
invented or partly wrong condition** (invented rules: 22/29 = 76% → 4/30 = 13%). Confidence is still 1.0 for
most rules; 52 of 158 rules are now `unconfirmed` (was 4 of 196) after the honesty penalty bit on 93
invented locators. The stage is now safe to run but still spends most of its output on plumbing: the
sample is 30, hand-graded by one person, and not a benchmark.

---

## The decision: in-process CIL decoder, not a sidecar

ICSharpCode.Decompiler gives better output but needs a .NET SDK on every runner, makes ledger facts depend
on the decompiler version, and needs a sandbox for untrusted DLLs. The in-process reader is deterministic,
has nothing to sandbox and cites IL offsets. Decision: in-process for the ledger; ILSpy only as a CI oracle
(no SDK on this machine, so not running — published source is used as ground truth instead).

## What was built (private repo)

`dotnet/body.rs` (full headers, EH clauses) · `dotnet/lift.rs` (typed decode, blocks, stack simulation into
expressions; enums from `Constant` rows; `MethodSpec`/`TypeSpec` owners) · `structure.rs` (short-circuit
merging, dominators/post-dominators per scope, loops, `try` from EH regions, `while`/`do`/`?:`/`return a && b`
canonicalization) · `verify.rs` (see below) · `render.rs` (C#-like pseudo-code, C# precedence) · `ir.rs`.

### Two invariants, checked over whole corpora rather than samples

1. **Equivalence:** every `statements` method executes like its CFG under random branch decisions
   (finally-on-leave included).
2. **Coverage:** no method of *any* fidelity loses a statement, `return` or `throw` ("never dropped").

| Corpus | Bodies | `statements` | diverge | dropped code |
|---|---|---|---|---|
| wine-mono mscorlib | 24,526 | 23,556 (96.0%) | 0 | 0 |
| Newtonsoft.Json net20 | 3,707 | 3,540 (95.5%) | 0 | 0 |
| Newtonsoft.Json netstandard2.0 | 4,064 | 3,815 (93.9%) | 0 | 0 |
| TSDServer.exe | 817 | 798 (97.7%) | 0 | 0 |
| TSDClient.exe | 1,101 | 1,035 (94.0%) | 0 | 0 |

These checks earned their keep: they found a `try` moved out of its loop, a loop inside a `finally` losing its
exit, a `return` copied across a `leave` ahead of its `finally`, and a `throw` dropped behind a dangling
`goto`. None was visible in the output unless you already knew the source.

## Ground-truth benchmark (published source of the same build)

Newtonsoft.Json 13.0.3 — the DLL's embedded commit `0a2e291c…` equals the `13.0.3` tag's commit; source is
preprocessed with each target's own `DefineConstants`. TSDServer's binary (2014-01-21) postdates every one of
its sources (2014-01-18). Methods are matched by (type, name, arity); overload-ambiguous, async/iterator and
source-only methods are skipped and counted.

| | matched | at `statements` | decision points within ±1 of source | call recall | string recall |
|---|---|---|---|---|---|
| Newtonsoft.Json net20 | 1,904 | 93.5% | 98.0% | 94.1% | 99.6% |
| Newtonsoft.Json netstandard2.0 | 2,127 | 93.9% | 98.2% | 93.9% | 99.6% |
| TSDServer | 715 | 97.8% | 99.6% | 96.8% | 100% (576/576) |
| TSDUtils | 68 | 98.5% | 100% | 94.4% | 100% (9/9) |

Exact `if` count agrees in 81–98% of methods; the rest are named lowering (`?:`, `??`, `?.`, `using`/`foreach`
/`lock`, string `switch` as an if-chain, the cached-delegate null check). 66–71 Newtonsoft methods have no
named feature and are unexplained — not investigated further. Recovered `switch` count is 46 vs 88 in the
source; that is consistent with the compiler lowering string and small switches to if-chains, but that cause
was not separately verified.

## Phase 5 — the migration loop

`ekos_binary_migration_check` takes a `BinaryType` id and the Python **as text** (no file reads; stays
Runtime-only), parses it with `rustpython-parser`, and compares per method: I/O kinds, SQL tables, messages and
constants, calls to sibling methods, exceptions, cyclomatic complexity. It reports evidence, never a score.

**Run on a real type:** `TSDServer.Compressor` (4 methods, all `statements`). The planner's inputs came from the
real MCP server over stdio: `migration_order` = Compress, Compress, DeCompressBytes, DeCompress (no calls among
them, so source order); I wrote `bench/migration/compressor.py` from the specs alone. The check reported
**4 matches, 0 differs, 0 missing, 1 Python-only function** (`compress_bytes`, because both .NET `Compress`
overloads map to one Python name — `method_map` is by name and cannot separate overloads).

Two honest limits. (1) The first run said `differs` for both decompress methods: my rewrite hoisted `65535` to a
module constant and the check only looked inside each function. That was a checker false positive, fixed
(module-level constants are in scope). (2) The check cannot see that Python's `pickle` is not .NET's
`BinaryFormatter` — the two produce incompatible bytes. A clean report means nothing this check compares
differs, not that the rewrite is behaviourally equivalent; characterization tests would catch it and do not
exist yet.

---

## Knowledge Captured

- **A verification corpus with statement counts can hide dropped code; a coverage invariant cannot.** The
  equivalence check only ran on fully structured methods, so two bugs that lost a `throw` from `control_flow`
  methods were invisible until `verify::covers` checked every body.
- **`rustpython-ast` 0.4's generated `Visitor` does not descend into `with` items, keyword arguments,
  comprehensions or `match` cases** (their `generic_visit_*` bodies are empty). `with open(p) as f:` and
  `requests.post(url, json=x)` go unseen unless you override them.
- **tree-sitter-c-sharp emits `escape_sequence` as a separate node** inside `string_literal`; joining only
  `string_literal_content` silently drops every `\n` and made a real 100%-recall measurement read 84%.
- **`[Conditional("DEBUG")]` calls vanish from Release binaries**, arguments and all — a benchmark against
  source must drop them or it counts compiler-removed strings as recovery misses.
- **An LLM told to find "business rules" will find them in getters.** Confidence 1.0 on 22 of 29 invented
  rules; prompt wording did not prevent it, feeding statements and removing logic-free methods does.
- **A constructor metadata name is `.ctor`, not `..ctor`.** A rule table written with the display spelling
  matched nothing while tests written with the same wrong spelling passed; pinned against real call sites.
- **`sqlparser` 0.53: `Insert.table_name`, `Delete.from` is `FromTable::{WithFromKeyword,WithoutKeyword}`.**
- **Licences:** `malachite`/`malachite-bigint` (**LGPL-3.0**) is in the private binary today via the *public*
  `ekos-recovery` Python analyzer (`rustpython-parser`). Not introduced by this work, but it must be resolved
  before a proprietary build is sold. All 4 `LicenseRef-Proprietary` and the public-workspace `UNKNOWN`
  licences are our own crates.
- Structuring is bounded: `MAX_BLOCKS` 2,000, nesting depth 128, an emission budget; a hostile graph falls
  back to `control_flow`, verified on a 300-block dense graph.

## Not done / open

- Characterization tests against the running original: specified, not built (TSD is .NET Compact Framework
  on Windows CE and cannot run on a desktop CLR).
- The ILSpy oracle is not running (no SDK here).
- `async`/iterator state machines are not un-lowered; the benchmark skips them.
- JVM statement recovery is a non-goal of this RFC.
- 66–71 unexplained `if` mismatches in Newtonsoft.

## Files Changed
| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0150-statement-level-dotnet-recovery-and-migration.md` | New RFC |
| `devlogs/devlog_192.md` | This file |
| (private) `crates/binary/src/{ir,render,structure,verify,obfuscation,io_classify}.rs`, `dotnet/{body,lift,mod}.rs` | Statement recovery, checks, classifier, obfuscation detector |
| (private) `crates/binary-recovery/src/{statements,migration_check,explain,binary_analyzer,binary_reconstruction}.rs` | Spec, parity check, LLM fixes |
| (private) `bench/compare.py`, `agents/binary-migration-planner.md`, examples | Benchmark, agent, tooling |
