# RFC 0206 — JVM statement recovery behind `JavaDecompiler`

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-10-18
**Builds on:** RFC 0148 (binary recovery), RFC 0150 (statement-level .NET recovery and its
backend-agnostic IR), RFC 0200/0203 (the `JavaDecompiler` seam and compiled Pentaho plugins)
**Code:** private `alexeyban/ekos-binary` (RFC 0149: the design is public, the decompiler is not)

## Goal

Recover JVM method bodies to statements, as RFC 0150 did for .NET, so that (a) a jar's methods
reach `Fidelity::Statements` / `ControlFlow` in the ledger with pseudocode, SQL and I/O facts like
.NET assemblies, and (b) the private build can offer a `JavaDecompiler` (RFC 0200): the decompiled
`processRow` of a compiled Pentaho plugin step goes through RFC 0203's translator instead of
staying a gap.

## Design

Same pipeline as .NET, a new front end:

1. **Decode** — cafebabe's opcodes (constant-pool operands resolved) into an owned instruction
   list: absolute branch targets, member references as `(owner, name, descriptor)`, loadable
   constants rendered once (no floats in artifacts: RFC 0150 determinism).
2. **Block** — leaders at 0, branch and switch targets, after every branch/return/throw, and at
   every exception-table boundary and handler.
3. **Simulate** the operand stack per block into `ir::Stmt`/`ir::Expr`, values carrying their
   JVM category (long/double take two slots, which `dup2`/`pop2`/`dup_x2` depend on); values
   left on the stack across a block boundary become temporaries assigned in every predecessor
   (`?:`, `&&` as a value); `new T; dup; …; invokespecial <init>` folds into `new T(…)`;
   `lcmp`/`fcmp*`/`dcmp*` followed by `if*` fold into one comparison; Java 9+ string
   concatenation (`invokedynamic` to `StringConcatFactory.makeConcatWithConstants`) is rebuilt
   from its recipe; `instanceof` is a new IR expression.
4. **Structure** with RFC 0150's structurer (if/else, loops, switch, try/catch; javac's inlined
   `finally` appears as a catch-all that rethrows).
5. **Render** Java-like source: Java type names, typed local declarations, `LocalVariableTable`
   names when the class carries debug info (else `locN`), `this.f`, `Owner.f`, `new T(…)`,
   `(T) v`, `v instanceof T`.

Gaps, never guesses: `jsr`/`ret` (pre-Java-6 subroutines), `monitorenter`/`monitorexit`
(`synchronized` blocks), lambdas and other `invokedynamic` bootstraps, and stack shapes the
simulator cannot express stay `Unmapped`/`Unknown` with their offsets, and the method is
`ControlFlow` instead of `Statements`.

## `JavaDecompiler`

`BinaryExtension::java_decompiler()` returns a decompiler that reads a jar or class file (the
RFC 0148 limits: member size, member count, no nested jars, nothing executed), finds the class by
binary name, lifts every method and returns `DecompiledClass` with each method's Java-like source,
fidelity and gaps. A private combined binary can register both `BinaryExtension` and
`LakehouseExtension`, which is how compiled Kettle plugins become translatable.

## Measured

pdfbox.jar (RFC 0148's JVM corpus, 652 classes): the share of methods reaching `Statements` and
`ControlFlow`, the gap reasons, and parse/lift time — in the ekos-binary devlog that ships this.
Golden tests on small classes compiled once and committed as bytes, with their sources beside them.

## Non-goals

Recovering generics from signatures beyond what `LocalVariableTypeTable` states, lambdas as
lambdas, Kotlin/Scala idioms, or obfuscated control flow beyond what RFC 0148's detector reports.
