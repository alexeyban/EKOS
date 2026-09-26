# Devlog 220 — RFC 0163: the PL/pgSQL parser, and six bugs the corpus found

**Date:** 2026-09-26
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

`ekos-plpgsql`: a deterministic, in-process PL/pgSQL parser. Dollar-quoting lexer, recursive-descent
statement parser, a procedural IR with spans, and fidelity computed from the IR rather than asserted
by the producer. No LLM anywhere in it.

43 tests. **5 of 5 real routines, read back from PostgreSQL exactly as `pg_get_functiondef` renders
them, recover completely** — with the floor asserted as a ratchet.

This is the piece the whole migration effort has been waiting on. RFC 0164's anti-invention check
requires every generated predicate to map to a source IR node, and against `Unmapped` *everything*
maps: the check passes on invented logic and reports green. That is the failure the entire system
exists to prevent, and it could not be fixed without this.

---

## Why a second IR, not more `TransformNode` variants

`TransformNode` is a **dataflow** graph shared by Pentaho and plain SQL. PL/pgSQL is **imperative**.
Forcing control flow into a dataflow graph either loses the ordering — making the IR wrong — or
gives every existing consumer node kinds meaningless in its own domain.

The procedural layer owns order and condition; the dataflow layer owns what each statement reads and
writes.

**One deviation from the RFC, stated in the crate docs:** RFC 0163 specified
`ProcStmt::Sql { graph: TransformGraph }`. This crate carries the statement's text and span instead
and leaves lowering to RFC 0164. A parser that depends on `ekos-semantic` cannot be tested without
it, and the two change for different reasons. The seam sits one step later than the RFC drew it.

---

## Six bugs, and where each came from

Three were found by the fixture suite and three only by running against real routines.

**1. Slicing a `&str` at a byte index panics on multibyte input.** The splitter, `starts_with_kw`
and `find_kw_op` all did it. A single `¿` in a comment took the process down — unacceptable in a
parser whose entire contract is *local* recovery. Byte comparison throughout, with one `slice()`
helper that snaps to char boundaries.

**2. A leading comment made every commented statement unrecoverable.** A comment does not end a
statement, so it arrives *inside* the statement text, and the first word is `--`.

**3. An inner block's `EXCEPTION` was claimed by the outer block.** `find_kw` tracks parentheses,
which is right for `THEN` and wrong for `BEGIN` and `EXCEPTION`. The outer block ended up owning a
handler belonging to an inner scope — error handling silently reattached to the wrong place.
`find_kw_outer` tracks block depth.

**4. `DECLARE` was counted as a block opener.** It opens nothing; `BEGIN` does. Counting it left the
depth permanently one too high, so the closing `END` never returned to zero and the whole routine
read as one unterminated statement.

**5. …and removing it traded one bug for another.** A `DECLARE` section's semicolons separate
declarations, not statements, so the declaration list split into free-standing fragments that
classified as nothing. `DECLARE` now suppresses splitting until its `BEGIN`.

**6. `END IF` was a close followed by an open.** Matching `END` and then re-matching `IF` on the next
pass decrements and immediately increments, so depth never balanced. `END IF`/`END LOOP`/`END CASE`
are consumed as one closer.

Bugs 4, 5 and 6 are all the same shape: a keyword whose *pairing* was modelled wrongly, invisible
until a routine nested deeply enough for the depth to matter.

---

## The corpus found what fixtures could not

With every unit fixture green, the corpus test reported **4/5**. `cursor_walk` was `Partial` — the
parser had no cursor support, and RFC 0163 lists cursors in scope.

That is the difference between "every construct I thought of parses" and "real routines parse". The
fix was a `ProcStmt::Cursor` variant covering `OPEN`/`FETCH`/`MOVE`/`CLOSE`, including
`FETCH NEXT FROM c INTO v` where a direction precedes the cursor name. 5/5, and the floor was
raised in the same commit so it cannot silently fall back.

---

## Knowledge Captured

**The fidelity label is computed, never asserted.** `ProcedureIr::new` walks the body — *including
nested branches, loops, blocks and exception handlers* — and labels `Statements` only when nothing is
`Unrecovered`. A producer cannot claim the label because there is no way to set it. That is what lets
RFC 0164 refuse reconstruction on `Partial` and mean it.

**Dynamic SQL does not reduce fidelity.** `EXECUTE 'SELECT …' || ident` is a *faithful* recovery of
something genuinely not statically known. Marking it a gap would conflate "we failed to read this"
with "this cannot be read", and the second is a boundary RFC 0164 refuses to cross rather than a
defect in the parser. It is reported through `dynamic_sites()`.

**A wrong span is worse than an obviously broken one.** Sub-parsing initially passed the *parent's*
start as the child offset, so every nested span was shifted by however far into the parent the
fragment sat — still a plausible offset, pointing at real text that was simply the wrong text.
`offset_of` locates the fragment within its parent.

**A routine in an unparsed language gets `Signature`, never an empty body.** An empty body reads as
"there is nothing in it", which is a different and false statement about a C or PL/Python routine.

**A corpus test needs a ratchet, not a pass mark.** The floor is asserted and the count printed, so a
regression fails with the routine named rather than quietly dropping one routine from complete to
partial.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/plpgsql/src/lex.rs` | New — dollar quoting incl. nesting, `$1` vs `$tag$`, quotes and comments, nested block comments, 14 tests |
| `ekos/crates/plpgsql/src/ir.rs` | New — `ProcStmt`, `LoopKind`, `CursorOp`, `Span`, computed `Fidelity`, 7 tests |
| `ekos/crates/plpgsql/src/parse.rs` | New — statement splitter and recursive-descent parser with local recovery |
| `ekos/crates/plpgsql/tests/parse.rs` | 21 tests, one fixture per variant plus a catalogue guard |
| `ekos/crates/plpgsql/tests/corpus.rs` | 3 live tests against routines read back from PostgreSQL, with the ratchet |
| `TODO.md` | RFC 0163 core ticked; analyzer pass, triggers and dataflow lowering remain |
