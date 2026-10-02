# Devlog 228 — RFC 0163 on LedgerSMB from source: 44/57 → 212/212, and spans that were never right

**Date:** 2026-10-02
**PRs:** none — committed to main, local gates green, `[skip ci]`
**Branch:** main

---

## Summary

The PL/pgSQL parser (RFC 0163, devlog_220) had only been measured on five routines read back
through `pg_get_functiondef`. Run against the routines LedgerSMB actually installs, **read from its
source files**, it recognised 57 of 212 as PL/pgSQL and fully recovered 44. The other 165 were never
parsed: `LANGUAGE plpgsql;` after the body was read as the language `plpgsql;`.

Nine bugs later it recovers **212/212**, and a new corpus test pins that number as a ratchet. The
worst finding was not a gap but a silent mis-parse: a `DROP TABLE IF EXISTS` swallowed the rest of
its routine, `RETURN` included, while the routine was still labelled fully recovered. Along the way,
**every nested span the parser had ever produced turned out to be shifted.** The old test only
checked that a span *contained* the right name.

---

## The bugs, in the order the corpus exposed them

| # | Symptom on LedgerSMB | Cause | Fix |
|---|---|---|---|
| 1 | 165 routines `Signature`, never parsed | `parse_signature` scanned raw text: `LANGUAGE plpgsql;` / `'plpgsql'` after the body, and a body column called `language`, all misread | Signature read from **lexer tokens**; the body is one opaque `Dollar` token. Arguments split at top-level commas (`numeric(10,2)`), `RETURNS SETOF x` / `TABLE (…)` kept whole |
| 2 | `FOR … LOOP` "body not found", IF/ELSE misparsed | An apostrophe in a comment (`-- we don't want…`) opened a "string" in every byte-level keyword scan | `lex::mask_comments` blanks comments to spaces of **equal byte length** once, in `parse_body`; every scan fixed at the same time, spans unchanged |
| 3 | `unrecognized statement starting with ""` | `raise …; -- cause rollback` left a comment-only fragment | Same mask: the fragment is now whitespace and is dropped |
| 4 | `t_id = currval(…)` unrecognised (×12) | PL/pgSQL accepts `=` as assignment | `split_assign` falls back to `=` when the target is exactly a variable reference and the `=` is not part of `<=`/`>=`/`!=`/`:=`/`==` |
| 5 | `IF … ELSE …` lost its `ELSE` | `x := CASE WHEN a THEN 1 ELSE 2 END;` inside a branch: the split used a parenthesis-only scan | Branch, `CASE` and handler splitting use the block-depth scan; a `CASE` branch's / handler's `WHEN` only counts at statement start (not `EXIT WHEN`) |
| 6 | `LOOP without a matching END LOOP` | `GROUP BY …, CASE WHEN … END` then `loop`: the expression's `END` + `LOOP` was taken as one `END LOOP` closer | A stack of open constructs: `END`'s trailing `IF`/`LOOP`/`CASE` belongs to it only if it names what is being closed |
| 7 | 2 routines `Signature` | Pre-8.0 single-quoted bodies, `AS ' … '` with `''` | `quoted_body`: parse the unescaped body, map every span back through the collapsed quotes |
| 8 | `NOTIFY parts_short;` unrecognised | SQL head list incomplete | Every directly executable command: `CALL`, `NOTIFY`, `LISTEN`, `LOCK`, `GRANT`, `REVOKE`, `COPY`, `COMMENT`, … |
| 9 | Routine labelled **complete** with a `RETURN` hidden in SQL text | `DROP TABLE IF EXISTS` counted `IF` as a block opener; depth never closed | `IF` opens a block only at statement start (`at_statement_start`): after `;`, `THEN`, `ELSE`, `LOOP`, `BEGIN`, a label, or at the start |

Plus two lexer defects found by reading rather than by the corpus: string literals were built with
`s[i] as char`, which turned each UTF-8 byte into a separate Latin-1 character (`é` → `Ã©`), and `E'it\'s'` was
not understood, so one such string could make a whole file fail to lex.

---

## Spans: computed, not searched for

`Span` is the parser's citation contract ("a claim about behaviour that cannot point at a span is
not a claim this system makes"). On LedgerSMB, every nested span was wrong, in three compounding
ways:

1. The splitter's span began *before* the leading whitespace its text had been trimmed of.
2. `IF`/`CASE` searched a string that started *after* the keyword, using the statement's own start
   as the base offset. `ELSIF` chains searched fresh copies with no anchor at all.
3. `parent.find(fragment)` returns the **first** match, so `IF a THEN x; ELSE x;` gave the `ELSE`
   branch the `THEN` branch's span. Exception handlers and declarations used the block's start for
   every statement.

The fix replaces the approach rather than the arithmetic. Every fragment is now a `&'a str` subslice
of one comment-masked body, and `Parser::span_of` computes a span as the fragment's address minus
the body's. That is plain address subtraction with no `unsafe`, `debug_assert`ed to lie inside the
body. Exception handlers now cite their own `WHEN … ;` text instead of the enclosing block, and
handler conditions are split on `OR`. The old code split them on `|`, which never appears in
PL/pgSQL, so `WHEN a OR b` became a single condition.

`every_nested_span_starts_at_its_own_statement` checks, at every depth, that a span starts with its
own statement's keyword and carries no surrounding whitespace. The corpus test checks the same thing
on all 212 routines.

---

## Verification

- **45 → 63 tests in the crate** (34 fixture, 24 lexer/IR unit, 3 live, 2 corpus). Each new behaviour
  has its own regression test.
- **Mutation-checked:** with each fix reverted on its own (any-`IF` opener, any-tail `END`, no comment
  mask, a one-byte span shift, parenthesis-only `ELSE` split, no quoted-body remap), at least one
  named test fails.
- `EKOS_LEDGERSMB_DIR=… cargo test -p ekos-plpgsql --test ledgersmb_corpus`: **212/212**, spans exact,
  second parse byte-identical, against LedgerSMB `544bcd947`.
- `EKOS_MIGRATE_LIVE=1 … --test corpus`: still 5/5. This is the `pg_get_functiondef` form, with
  `LANGUAGE` before `AS`, which is the other half of the signature rewrite.
- `cargo clippy -p ekos-plpgsql --all-targets -- -D warnings`, `cargo fmt --check`: clean.

---

## Knowledge Captured

- **`pg_get_functiondef` output is a flattering corpus.** It normalizes exactly what hand-written
  source does not: clause order, quoting, body delimiters. Five routines from it said "5/5". The same
  parser on source files said 44/57, with 165 routines unseen. `recover` reads repositories, so source
  files are the corpus that matters, and a live readback only checks the normalized form.
- **A computed fidelity label is only as honest as the IR under it.** Bug 9 produced zero
  `Unrecovered` nodes, so the label was correctly computed and still wrong. What caught it was a
  *structural* assertion: a leaf statement's span never ends in `;`. RFC 0164's lowering should
  reject an `Sql` text containing a top-level `;` as a parser defect.
- **Count only what upstream loads.** `Business_Dates.sql` is commented out of LedgerSMB's
  `LOADORDER`, and its three routines are invalid (untyped arguments, `end if` without `;`).
  PostgreSQL would reject them. Counting them as parser failures would have meant tuning the parser
  to accept invalid code. Its corpus test reads `LOADORDER`.
- **`IF EXISTS` is two things.** `IF EXISTS (SELECT …) THEN` is a PL/pgSQL statement, while
  `DROP TABLE IF EXISTS` is DDL. The previous word decides which, and a keyword list cannot.
- **`END` + newline + `LOOP` may be two constructs.** Pairing needs a stack, not a depth counter. This
  is the fourth keyword-pairing bug in this parser (devlog_220 had three) and the first that needed
  memory of *what* was opened.
- **Masking beats teaching every scanner.** Seven byte-level scanners each handled quotes slightly
  differently, and none handled comments. Blanking comments to equal-length spaces once removed a
  whole class of bugs without touching span arithmetic. Quote handling is now one function too
  (`skip_quoted`: `'…'`, `"…"`, `E'…'`, `$tag$…$tag$`).
- **Generic recursion over closures overflows instantiation.** A recursive `fn spans_mut(&mut self,
  f: &mut impl FnMut)` that wraps `f` in a new closure hits rustc's recursion limit. Use `&mut dyn
  FnMut`.

---

## Still open (RFC 0163)

- `PlPgSqlAnalyzerPass` in `recovery`, with `Procedure`/`ProcedureStatement` REGISTRY rows. The
  parser is still not consumed by anything.
- Trigger recovery and structural classification.
- Pagila corpus floor.
- Lowering `ProcStmt::Sql` text into `TransformGraph` (RFC 0164).

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/plpgsql/src/parse.rs` | Slice-based parser with computed spans; token-based `parse_signature`; `quoted_body`; `at_statement_start`; opener stack in both depth scanners; `skip_quoted`; `split_at_statement_kw`; label-aware `strip_end`; `=` assignment; SQL head list; handler conditions split on `OR`, handler spans |
| `ekos/crates/plpgsql/src/lex.rs` | `mask_comments`; UTF-8-correct string and quoted-identifier values; `E'…'` escapes; 3 tests |
| `ekos/crates/plpgsql/src/ir.rs` | `ProcStmt::spans_mut` (crate-private), for remapping quoted-body spans |
| `ekos/crates/plpgsql/tests/parse.rs` | 13 regression tests, including the nested-span property test |
| `ekos/crates/plpgsql/tests/ledgersmb_corpus.rs` | New: LedgerSMB `LOADORDER` corpus from source, 212 floor, exact spans, determinism (`EKOS_LEDGERSMB_DIR`) |
| `ekos/docs/rfcs/0163-plpgsql-procedural-ir.md` | Acceptance criteria updated; *Amendment 2026-10-02* |
| `TODO.md` | Corpus ratchet ticked; Pagila floor added |
