# Devlog 103 — RFC 0091: SQLAlchemy ORM model recognition

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Resolved the last of the previously-deferred `pdf-reader` documentation gaps on request: no
analyzer recognized an ORM-declared schema, so `## Data Architecture`/`## Entity Relationships`
rendered nothing for a project (like `pdf-reader`) with no raw SQL DDL anywhere. RFC 0091 (filed
and implemented same-day, matching this session's established pattern): `python_analyzer.rs`'s
existing `ClassDef` handling now also recognizes a SQLAlchemy declarative model
(`__tablename__ = "..."` present) and compiles it into a real `ObjectKind::Table` object with real
columns and real foreign keys, alongside its existing `PythonSymbol` object — unchanged.

## What was built

- `extract_tablename`/`extract_orm_columns`/`find_fk_target`/`type_hint` — real AST navigation
  reusing every existing helper in the file (`string_constant`, `positional_string_arg`,
  `keyword_arg`), no new parsing infrastructure.
- `add_orm_table` — builds the `Table` object (same `columns: [{"name","data_type"}]` shape
  `sql_analyzer.rs::columns_json` already uses, so column data reads identically regardless of
  origin) and real `ForeignKey` relationships (same `fk_desc`-in-id-hash scheme
  `sql_analyzer.rs::foreign_key_kir_id` already uses).
- A pre-pass in `parse_python_file` collects every real `__tablename__` in the file before the main
  walk, so a `ForeignKey("other_table.col")` resolves regardless of class declaration order.
- Small companion fix in `docs-gen::render_data_architecture`: real column names were compiled
  (by `sql_analyzer.rs`, for raw SQL DDL) but never rendered anywhere — added one sub-line per
  data store, benefiting existing SQL-DDL-derived tables retroactively, not just the new
  ORM-derived ones.

## Scope decisions

- **Python/SQLAlchemy only.** Django/other ORMs/other languages are real extensions of the same
  idea, explicitly deferred — not attempted without a real project to verify against (this
  session's own repeated lesson: several earlier fixes looked correct by inspection and were only
  caught wrong by live rebuilds).
- **Detection via `__tablename__` presence**, not tracing `bases` back to `declarative_base()`/
  `DeclarativeBase` — the latter is fragile against aliasing/re-exports; the former is
  unambiguous and SQLAlchemy-specific, matching this codebase's established preference for
  concrete syntactic signals (`is_real_readme_name`, `looks_like_code_reference`).
- **Same-file FK resolution only.** A `ForeignKey` target not found among this file's own
  `__tablename__`s gets no edge — an honest gap, not a fabricated edge to a possibly-nonexistent
  id (this session found and fixed several real "dangling relationship" bugs from exactly that
  shortcut).
- **The `Table` object is additional, not a replacement** for the class's existing `PythonSymbol`
  object — two real KirObjects, `Contains`-linked from the same file (matches `add_symbol`'s own
  linking convention). Naming the `Table` by the real `__tablename__` (`"documents"`), not the
  class name (`"Document"`), sidesteps the same-name-different-kind identity-conflict class found
  twice already this session (`devlog_101`, `devlog_102`) in the common case — SQLAlchemy's own
  pluralization convention keeps the two names apart — stated in the RFC as a real, not-guaranteed
  mitigation, not a hard fix.

## Verification

8 new tests (7 `python_analyzer.rs`, 1 `docs-gen`). Full workspace gate clean, `tests/integration`
3/3. Live-verified against `pdf-reader`'s real `db/models.py`: 3 new `Table` objects (`documents`,
`page_cache`, `translation_cache`) with real, correct columns and data types; `## Entity
Relationships` now renders a real ER diagram (`page_cache }o--|| documents : references`) matching
the real `ForeignKey("documents.file_hash")` in source; `translation_cache` correctly shows 0 FK
edges (it has none) rather than being force-fit to match its siblings.

## Knowledge Captured

- **SQLAlchemy allows a bare, uninstantiated type reference as a column-type argument**
  (`mapped_column(Integer)`, no parens) alongside the called form (`mapped_column(String(64))`) —
  the first version of `type_hint` only handled `Expr::Call` and silently produced `"unknown"` for
  every bare-type column, caught immediately by a real test using `pdf-reader`'s own actual
  `page_count: Mapped[int] = mapped_column(Integer)` line rather than a hand-simplified fixture.
  Worth remembering when extracting from any "constructor call OR bare reference" language pattern
  — test with both real shapes, not just the one that happens to come to mind first.
- **`sql_analyzer.rs`'s existing `columns`/`ForeignKey` conventions transferred directly** to a
  completely different extraction source (Python AST vs. SQL DDL parsing) with zero shape changes
  needed — real evidence that "match the existing property/id-scheme convention exactly, even when
  building a new analyzer" pays off immediately: the companion `docs-gen` rendering fix required no
  origin-specific branching at all.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/crates/recovery/src/python_analyzer.rs` | ORM model recognition: `extract_tablename`, `extract_orm_columns`, `find_fk_target`, `type_hint`, `add_orm_table`, `orm_table_kir_id`, `orm_foreign_key_kir_id`; pre-pass in `parse_python_file`; 7 new tests |
| `ekos/crates/docs-gen/src/lib.rs` | `render_data_architecture` renders real `columns` property when present; 1 new test |
| `ekos/docs/rfcs/0091-orm-model-recognition.md` | New RFC — filed and implemented same-day, Accepted with live verification |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against the fix |
