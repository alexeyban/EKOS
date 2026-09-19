# RFC 0150 — Statement-level .NET recovery, and assisted migration to Python

**Status:** Accepted — Phases 0–6 implemented in the private `alexeyban/ekos-binary` workspace (2026-09-19); measured results in `devlogs/devlog_192.md`
**Date:** 2026-09-19
**Supersedes:** none
**Related:** RFC 0148 (compiled .NET/JVM binary recovery), RFC 0149 (extension seam — this work
ships in the private `alexeyban/ekos-binary` extension), RFC 0027 (Transformation IR),
RFC 0028 (`ekos_transformation_explain`), RFC 0043 (redaction), RFC 0135 (provenance &
determinism), RFC 0147 (Perl connector — the no-shell-out precedent)

---

## Summary

RFC 0148 recovers a compiled binary's *structure*: types, members, signatures, the call graph,
literals, branch counts and I/O boundaries. That is enough to document what a program touches.
It is not enough to rewrite a program, because it never says what a method *does* — in which
order, under which condition, with which values.

This RFC adds statement-level recovery for .NET method bodies. It is done by an in-process CIL
decoder written in safe Rust, not by a decompiler sidecar. It also adds the surface a migration
needs on top of that: a per-method specification from `ekos_binary_explain`, a
`binary-migration-planner` agent, and a parity check that compares a Python rewrite against the
original binary. Every level of recovery is labelled with the fidelity it actually reached, and
nothing is labelled `Statements` that is not.

The implementation is private (RFC 0149): it lives in `alexeyban/ekos-binary`. This document, like
RFC 0148, is public. The public repository only receives generic changes.

---

## Motivation

The first real application of RFC 0148 (devlog_190) was a lost-source Windows CE barcode-terminal
system, TSD: a handheld client, its desktop sync server, and vendor SDK wrappers. The owner wants
it moved to Python. Structural recovery produced an 18-section system description, and it also
showed where structure stops:

- `ViewProductForm.DoAction` has 57 distinct call targets and a cyclomatic complexity in the
  hundreds. Structure lists every call. It cannot say which run on which key press.
- Six of the terminal's 19 settings are never read. That was provable from the call graph. *What
  the other 13 change* was not.
- The LLM stage can name a type and guess at rules, but it is told, correctly, that it sees no
  statements. A rewrite built on its guesses would reproduce the guesses, not the program.

A migration needs, per method: the conditions, the order of effects, the constants those
conditions compare against, and the exact I/O performed with its arguments. All of that is in the
IL. RFC 0148 read the IL linearly and kept only the call sites, literals and branch opcodes. This
RFC reads it properly.

---

## Non-goals

- **JVM statement recovery.** The TSD work that motivates this is .NET, and the JVM backend
  stays `Structural`. The IR below is backend-agnostic so a JVM decoder can target it later.
- **Compilable C# output.** The target is a specification a person or a model can rewrite
  from, with every statement citing its IL range. It is not a round-trippable decompilation.
- **De-obfuscation.** Obfuscated and packed binaries are detected (Phase 0) and refused by the
  stages that would present their contents as logic. Undoing a protector is out of scope.
- **Automatic translation.** EKOS recovers and checks. The rewrite is written by a developer or
  by Claude Code, from the spec, and then checked against the original.

---

## Phase 0 — gaps the migration depends on (shipped with this RFC)

Five defects or gaps in the RFC 0148 implementation would have undermined everything below.
They are fixed first.

| Gap | Fix | Measured |
|---|---|---|
| The I/O classifier matched owner-type *prefixes*. `System.IO.File` claimed `FileNotFoundException..ctor`; `System.Diagnostics.Process` claimed `GetCurrentProcess`; `DataSet.ReadXml(path)`/`WriteXml(path)` — how TSD persists everything — were invisible. | Rules name an exact type or a namespace, optionally a method set and the first parameter type. The most specific rule wins, and a rule can veto. Exception types are never boundaries. New kinds: `network` (raw sockets) and `device` (serial ports). | TSD client folder: exception constructors (8) and process introspection (2) no longer counted. Newly visible: `DataSet.ReadXml/WriteXml(path)` (6), `XmlDocument.Load/Save` (3), `SmtpClient.Send` (2), `SerialPort.Open/Write` (12), and sockets (6, in the server). |
| `BinaryMethod.call_targets` was cut at 32 entries with no marker, so a "who calls X" check built on it gave false answers. | Call targets get their own cap of 512, which is larger than any real method measured. The largest in TSD has 70. Any capped list now records `<list>_truncated` and `<list>_total`. | 74 TSD methods exceeded the old cap, and none reaches the new one. |
| An obfuscated or packed binary was recovered and documented as if it were clean. | The `obfuscation` module assesses every assembly and archive: protector watermark types, unreadable identifiers, mass one- or two-character renaming, undecodable bodies, and packer stubs. A verdict produces a `BIN_OBFUSCATED`/`BIN_PACKED` warning at `build`, a flag on every derived fact, and a `WARNING:` line from `recover`. Reconstruction skips flagged types, and `ekos_binary_explain` refuses packed binaries. | Calibrated on 3,133 clean binaries (3.14M identifiers), with no verdict on any of them. The unreadable-identifier maximum is 3.0% against a 20% threshold, and the short-name maximum is 20.7% against 40%. Confirmed on two real ProGuard-renamed jars shipped in PyCharm 2025.3.3. |
| The IL walk stopped silently at an undecodable byte. | `BIN_IL_UNDECODABLE` diagnostic; it also feeds the obfuscation assessment. | 0 across the calibration corpus. |
| `ekos_binary_explain` accepted and silently ignored unknown arguments. That is the same failure as `ekos_impact`'s ignored `max_depth` in devlog_190. | `method` is declared up front: it narrows a type to one method by name or locator, and a wrong name lists the valid ones. Any undeclared argument is an error. | — |

Running LLM reconstruction against a real provider for the first time also required four fixes
to `binary_reconstruction.rs`. Each one is pinned by an end-to-end test:
- The hallucination penalty was never applied. The comment said the caller "folds it in", and
  no code did.
- Byte-slicing a Cyrillic condition panicked.
- Rule IDs and the agreement index were keyed by locator alone. A .NET locator is a metadata token
  that recurs in every assembly, so one binary's rules overwrote and "contradicted" another's.
- "Corroboration" raised confidence. Because slices are disjoint, it could only ever count a
  model agreeing with itself.

Confidence now only goes down. The measured precision of that first real run was poor (2 of 29 accepted
rules correct and non-trivial, 22 invented), which led to two more fixes — the recovered statements are now in
the prompt, and methods with no decision, constant or I/O are not sliced — after which invented rules fell to
4 of 30 (`devlog_192`). Both samples were graded by hand, n = 30.

---

## Decision — how method bodies are recovered

### Options

**A. Decompiler sidecar.** ICSharpCode.Decompiler (ILSpy's engine, MIT) behind a subprocess.

**B. In-process CIL decoder.** Extend the existing safe-Rust ECMA-335 reader with an instruction
decoder, stack simulation, a control-flow graph, and a structuring pass.

| | A — sidecar | B — in-process |
|---|---|---|
| Output quality | Best available: C#-like, mature pattern recovery (LINQ, async, `using`, `foreach`) | Lower ceiling: no async/iterator state-machine un-lowering in v1 |
| Toolchain | .NET SDK on every machine and CI runner that runs `recover` | None |
| Determinism | Output changes with the decompiler version, so ledger facts depend on what is installed | A pure function of the bytes and the reader version |
| Untrusted input | A customer DLL goes to a large C# codebase in a subprocess, which needs a sandbox (no network, read-only FS, CPU/memory/time limits) and a security review | Bytes read with `#![forbid(unsafe_code)]`, bounds-checked, never executed; nothing to sandbox |
| `CompilerPass` contract | Spawning per file is neither deterministic nor side-effect-free | Satisfied |
| Provenance | Facts cite decompiler output, one step removed from the IL | Every statement cites its own IL offset range |
| Precedent | — | RFC 0147 rejected a `perl` shell-out, and RFC 0148 rejected sidecars, for the first three reasons in this table |
| Cost | Days | Weeks |

### Decision

**B for everything that reaches the ledger. A only as a test oracle, outside the ledger path.**

B is the only option that keeps the invariants this project enforces: deterministic,
side-effect-free passes, reproducible builds, provenance down to the IL offset, and no
execution of untrusted input. A's advantage is pattern recovery on modern C#
(async/await, LINQ, pattern matching). That matters less for the binaries that motivate this
work: .NET Compact Framework 2.0/3.5 code from 2007–2014, which uses almost none of those
features.

A is still useful as an **oracle**. It shows what a mature decompiler recovers from the same IL,
so each B gap is a known one. Its use is fenced:

- It runs in CI only, against the fixed benchmark corpus (Phase 6), and is never part of
  `ekos recover`.
- If A is ever used in production, it must: pin the decompiler version; fold that version into
  the extension's `logic_version()` so a change re-observes everything; record
  `extractor: "ilspy-sidecar/<version>"` on every fact it produces; and run inside a sandbox with
  no network, a read-only filesystem, and wall-clock and memory limits. That would need its own
  RFC.

*Status of the oracle on the development machine (2026-09-19):* no .NET SDK is installed, so the
oracle is specified here but not yet running. The Phase 6 benchmark uses **published source** as
ground truth, which is a stricter oracle than any decompiler. ILSpy output is a secondary
comparison, added when a runner with the SDK is available.

---

## Fidelity, honestly labelled

RFC 0148's `Fidelity` had two levels, and nothing produced the second one. This RFC makes fidelity
**per method** and adds the level in between:

| Level | Meaning | Produced when |
|---|---|---|
| `structural` | Facts only: calls, fields, literals, branch counts, I/O | No body, an undecodable body, a body over the work limit, or a JVM method |
| `control_flow` | The body is decoded into basic blocks of expression statements with a control-flow graph, but not fully structured, or it contains unrecovered expressions | Stack simulation succeeded but structuring left `goto`s, or some expression is `Unknown` |
| `statements` | Every block is structured into `if`/`while`/`do`/`switch`/`try` with no `goto`, and every expression is recovered | Only then |

- `DecompiledMethod.fidelity` is the level that method reached. The AST-level `fidelity` is the
  *highest level the reader attempted*, and it is never read as a promise about any one method.
- `BinaryMethod` facts carry `fidelity`. `BinaryAssembly` carries `fidelity_counts`, so "how much
  of this app was recovered at statement level" is a query, not an estimate.
- A method never reports a higher level than it reached. Recovery that is only partial is
  labelled `control_flow`, and the parts that were not recovered stay in the output as
  `Unmapped`/`goto`. They are never dropped.

---

## Phase 2 — CIL decoding and the control-flow graph

**Unit: the method body, never the file.** Every failure is per method. A method that fails at
any step falls back to the previous level with a diagnostic, and the rest of the assembly is
unaffected. This is RFC 0148's degrade-per-item rule, applied one level deeper.

1. **Body header, fully.** Fat headers yield `MaxStack`, the `LocalVarSig` token (decoded through
   `StandAloneSig` into local types), and the extra data sections (small and fat exception-handling
   clauses: `try`/`catch <type>`/`finally`/`fault`/`filter`, with their ranges).
2. **Typed instruction decode.** Each instruction becomes an operation with resolved operands:
   branch targets as absolute offsets; method, field and type tokens as names; `#US` strings;
   argument and local indices. Parameter names come from the `Param` table. Local names do not
   exist without a PDB, so they are `loc0`, `loc1`, …, typed from the signature.
3. **Basic blocks and CFG.** Block leaders are offset 0, every branch target, every instruction
   after a branch/`ret`/`throw`/`leave`/`switch`, and every exception-clause boundary. Edges are
   typed: fallthrough, true/false, switch case *n*, unconditional, `leave`, and exception.
4. **Stack simulation into expressions.** Each block is evaluated symbolically, so
   `ldarg.1; ldc.i4 10000; bgt` becomes `if (amount > 10000)`, and a call becomes
   `Call { target, args }` with argument expressions. Values that cross a block boundary on the
   stack (the C# `?:` and short-circuit `&&`/`||`) are carried as synthetic temporaries, not
   dropped. Anything the simulator cannot type is an `Unknown` expression that cites its offset.
5. **What falls out for free.** String literals, constants, field accesses and call sites now
   each come *with the expression they appear in*. A call site shows its argument values: the path
   passed to `WriteXml`, the SQL passed to a command, the threshold passed to a comparison.

**Hard limits.** Instructions per method, blocks per method, stack depth, and expression depth
are all capped. A hostile or generated body that exceeds a cap falls back to `structural` with a
diagnostic. Every loop in the analyses is bounded, so a crafted CFG cannot hang `recover`.

---

## Phase 3 — structured statements

- **Loops.** A back edge in DFS order whose target dominates its source is a natural loop, and it
  is typed by where its exit test sits: `while` or `do … while`. `break` and `continue` are edges
  to the loop's follow block or header.
- **Conditionals.** A two-way block becomes an `if`/`else` joined at its immediate
  post-dominator. Chains of conditional blocks that share a target are merged into `&&`/`||`
  conditions.
- **`switch`.** It is recovered from the `switch` opcode, with case values and the default target.
- **`try`/`catch`/`finally`.** These come from the exception clauses, not from the CFG, so they
  are exact.
- **Irreducible or unrecognized regions** stay as labelled blocks with `goto`. The method is then
  `control_flow`, not `statements`. They are evidence, never dropped.

**SQL feeds the existing SQL analyzer.** When a constant string (or a concatenation of constants)
reaches `SqlCommand`/`SqlCeCommand`/`OleDbCommand`/`DbCommand` construction or `set_CommandText`,
or an adapter or `Execute*` call on those, it is parsed with the public
`parse_sql_to_transform_graphs` (RFC 0027). The resulting Transformation IR is attached to the
method. SQL assembled at run time from non-constant parts becomes `Unmapped`, with the constant
fragments recorded. It is never guessed.

---

## Phase 4 — documentation and MCP surface

- **`ekos_binary_explain` with `method`** returns a per-method **spec**: the signature with
  parameter names; locals with their types; the structured body rendered as indented C#-like
  pseudo-code, where every line cites its IL range; I/O boundaries *with their argument
  expressions*; constants; SQL with its Transformation IR; exceptions thrown and caught; and the
  method's fidelity. Deterministically recovered content stays in `recovered` and LLM content in
  `reconstructed`, as in RFC 0148.
- **`binary-migration-planner` agent.** In the style of `legacy-logic-recoverer`, it walks a type's
  methods through `ekos_binary_explain`. It orders the migration by the call graph, leaves before
  callers. It flags every `control_flow`/`structural` method as "cannot be migrated from the
  spec alone", and it never fills a gap by guessing.

---

## Phase 5 — the migration loop for Claude Code

The loop runs per type: **read the spec → write Python → check → fix**.

The check is a new MCP tool, `ekos_binary_migration_check`. It is read-only and a pure function
of its arguments plus the ledger. It takes a `BinaryType` id and the rewrite's Python source *as
text*, so it reads no files and stays inside the Runtime-only rule. It parses the Python with
`rustpython-parser`, the parser the public Python analyzer already uses, and compares per method:

- **I/O boundaries**, by kind and operation (a `DataSet.WriteXml(path)` should become a file write,
  and a `SmtpClient.Send` should become an SMTP send), using a Python classifier table that
  mirrors `io_classify`;
- **calls** to the type's other recovered methods, mapped by name;
- **constants and messages** that the original's conditions and outputs use;
- **decision count**, with the cyclomatic complexity of each side reported side by side.

The output lists, per method, what is matched, missing and extra. It is evidence for the developer
to judge, not a pass/fail oracle, because two correct programs can differ in structure.

**Characterization tests** (running the original on sample inputs and recording its outputs) need
a runtime that can load the binary. TSD targets .NET Compact Framework on Windows CE, which cannot
run on a desktop CLR without its platform assemblies. For binaries that can run (desktop .NET
Framework on Mono or .NET), a harness is specified here but not built: it would invoke a public
static method through reflection on recorded inputs, in a sandbox with no network. Until it exists,
parity is checked statically, and every report says so.

---

## Phase 6 — measurement

- **Ground-truth benchmark.** An open-source .NET library whose released DLL and published source
  are for the same version. Metrics, per method present in both:
  - method coverage by fidelity level;
  - structure agreement: counts of `if`/loop/`switch`/`try` recovered vs in the source;
  - call agreement: the set of invoked member names, recovered vs in the source;
  - literal agreement: string and numeric constants, recovered vs in the source.
- **Real-world regression corpus: TSD.** `TSDServer.exe` was built on 2014-01-21, after the last
  edit to any of its sources (2014-01-18), so its source is ground truth for that binary.
  `TSDClient.exe` has sources up to three weeks newer than the binary, so it is used as a
  regression corpus, not as ground truth.
- **Only measured numbers are published.** An estimate is labelled as one, or left out.

---

## Cross-cutting rules

- Every fact carries **fidelity and provenance**. Statements cite IL offset ranges, facts carry
  `extractor` versions, and the extractor version is bumped whenever reader output changes, along
  with the extension's `logic_version()`, so `build`'s fingerprint cache cannot serve stale
  artifacts.
- **Confidence can only go down.** No step raises a score above the evidence that produced it.
- **After any public seam change**, run `cargo update` in the private workspace and re-run its
  tests. The private build pins the public crates by `Cargo.lock`.
- **Licences are checked before any code is sold.** Linked today: `cafebabe` (0BSD), `zip` (MIT),
  `serde`/`serde_json`/`sha2`/`hex`/`thiserror`/`uuid`/`walkdir` (MIT or Apache-2.0), and the
  public EKOS crates (MIT), plus `rustpython-parser` (MIT) for Phase 5. The CI-only oracle,
  ICSharpCode.Decompiler, is MIT and is never linked. `dotnetdll` (GPL-3.0+) remains rejected.
  A licence report (`cargo about`/`cargo deny`) is part of the release checklist.

---

## Security

Nothing changes from RFC 0148's model, and this RFC relies on it. The input is still only read,
never executed. The decoder is safe Rust. Every new analysis has a work bound. Redaction (RFC 0043)
runs on the observer's JSON in `ekos build` for extension observers exactly as for built-in ones.
This matters more now, because argument expressions put more literal text, such as connection
strings, into artifacts.

---

## Phases

| Phase | Content | Exit criteria |
|---|---|---|
| 0 | The five gaps above, plus the first real LLM run | Each fix pinned by a test on real data where real data exists; LLM precision measured and recorded |
| 1 | This RFC | Accepted |
| 2 | Body header + EH clauses, typed decode, blocks, CFG, stack simulation; `control_flow` fidelity | Every mscorlib body decodes or falls back with a diagnostic; zero panics on the truncation sweep; byte-identical re-reads |
| 3 | Structuring; `statements` fidelity; SQL literals into RFC 0027 | Measured share of TSD methods at each level; structure counts agree with TSDServer's source on the measured share |
| 4 | Per-method spec in `ekos_binary_explain`; `binary-migration-planner` agent | A TSD method's spec is sufficient to rewrite it (checked by doing so) |
| 5 | `ekos_binary_migration_check` | A real TSD type migrated to Python through the loop, and the check's report is recorded |
| 6 | Benchmark + TSD numbers | Published numbers, and only measured ones |

---

## Risks and open questions

- **Compiler idioms.** `foreach` over `IEnumerator` with `try/finally` disposal, `using`, `lock`,
  `switch` on strings (a hash table on CF 2.0), and nullable lifting all compile to verbose IL.
  v1 structures them faithfully as the lowered form. Re-sugaring is a readability improvement and
  not needed for correctness, and each one is added only when a measured corpus shows it matters.
- **Async and iterator state machines** do not occur in the CF-era corpus. Modern binaries will
  show them as `control_flow` state-machine `switch`es until un-lowering exists. That is labelled,
  not hidden.
- **Artifact size.** Statement trees are much larger than facts. Per-type artifacts (RFC 0148)
  bound the unit. A per-method statement cap, with fallback, bounds the worst case, and the
  existing `< 4 MB per artifact` test on mscorlib stays in force.
- **The oracle is not running yet** (no SDK on the development machine). Published source is used
  as ground truth in the meantime.
