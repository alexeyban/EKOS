# Devlog 108 — RFC 0092: real class inheritance (`RelationshipKind::Extends`), Python v1

**Date:** 2026-08-25
**PRs:** (uncommitted at time of writing)
**Branch:** main (direct)

---

## Summary

Fifth item on the gap-closure list: `RelationshipKind::Extends` already existed in the KIR enum
but had zero producers across every analyzer — the blocker behind ever generating a real
class-level inheritance view. Filed and implemented RFC 0092 (Python v1, same discipline RFC 0091
established: scoped to the one real, live-verifiable language available this session). Live-verified
against `pdf-reader`'s real `db/models.py`: real `Extends` edges from `Document`/`PageCache`/
`TranslationCache` to `Base`, correctly honest about the one it can't resolve (`Base` itself extends
an imported `DeclarativeBase`, never locally defined).

## What was built

`python_analyzer.rs`'s `ClassDef` handling already visited every class (RFC 0091 reused the same
visit for `__tablename__` detection) — this extends it: a `known_classes: HashMap<String, KirId>`
pre-pass (mirrors RFC 0091's `known_tables` exactly, same reason — a base class can legitimately be
declared after its subclass in real source), then a real `Extends` edge per base expression that's
an `ast::Expr::Name` matching a same-file class. `ast::Expr::Attribute` bases (`orm.DeclarativeBase`)
are skipped without even attempting a lookup — a dotted reference can never refer to a same-file
class by construction, not a partial implementation. An `Extends` edge to a base that isn't
locally defined (`BaseModel`, imported from `pydantic`) is honestly not emitted — the identical
"resolve within-file only, no fabrication" discipline RFC 0091 established for `ForeignKey`.
Relationship id: deterministic, keyed by `(from, to)`, matching `crate_topology_analyzer.rs`'s
`depends_on_kir_id` precedent (RFC 0070/0071's fix for the unbounded-duplicate-accumulation failure
mode a non-deterministic relationship id causes across repeated `recover` runs).

6 new tests: same-file resolution (both declaration orders), an unresolvable-external-base case
(no edge, no fabrication), an `Attribute`-form base never mistakenly matching a same-named local
class, a real end-to-end shape mirroring `pdf-reader`'s exact `Base(DeclarativeBase)` → `Document(Base)`
chain (one resolved edge, one correctly skipped, in the same file), and a determinism check.

## A real correction made mid-session, not left standing

The RFC's first draft claimed `docs-gen`'s generic object-page renderer groups relationships one
section per literal `RelationshipKind` (`### Extends`) — written from a comment, not from actually
reading `build_object_page_model`. Live verification against the real generated `Document` page
showed otherwise: relationships are grouped into four *structural* buckets (`Based on`/`Contains`/
`Used in`/`Dependent on`, by `(is_contains, outgoing)` — pre-existing design, not something this
session touched), so `Extends` lands in `### Dependent on` alongside any other outgoing non-`Contains`
edge, not its own heading. What *does* carry the real kind visibly: the same page's Mermaid diagram
renders the literal edge label — `Document -->|Extends|-> Base`. Corrected the RFC text to say this
precisely, rather than leave an inaccurate claim in an "Accepted" RFC because it happened to still
be technically true that "the data is visible" — the *how* was wrong and worth fixing on sight.

## Live verification

Rebuilt `pdf-reader`'s `.ekos/` fully fresh. `ekos compile` relationship count went from 189 → 192
(exactly the 3 real `Document`/`PageCache`/`TranslationCache` → `Base` edges). `ekos query
neighbourhood` on `Document`'s real `PythonSymbol` id (`8be8497e-...`) shows the real edge:
`Extends 8be8497e-... → 440e83d5-...` (the real `Base` symbol). `ekos query neighbourhood` on
`TranslateRequest` (extends `BaseModel`, never locally defined) confirms no fabricated edge — only
the real `Contains` edge from its file. Generated the `objects` layout and read the real
`Document` page directly: `## Relationships` → `### Dependent on` lists `Base`; the page's Mermaid
diagram renders `Document -->|Extends|-> Base` literally.

Full workspace gate clean: `cargo fmt`, `cargo build --workspace`, `cargo clippy --workspace -- -D
warnings`, `cargo test --workspace` (101/101 test groups). `tests/integration` 3/3.

## Knowledge Captured

- **A design claim in an RFC about a rendering layer's actual behavior needs to be checked against
  the real render function, not inferred from a nearby comment** — the comment describing
  `## Relationships`' shape (`"Based on"`/`"Dependent on"`/etc.) was accurate on its own, but
  reading it in isolation led to a wrong inference about *how* kinds get their own sections. The
  fix cost nothing once live verification surfaced it (the RFC was corrected in place, same
  session, before calling the RFC done) — but the lesson generalizes: a rendering claim in a design
  doc is exactly the kind of statement that should be verified against a real generated page before
  the RFC is marked Accepted, not just asserted from the surrounding code's comments.

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0092-class-inheritance-extends.md` | New RFC, Accepted; corrected mid-session after live verification of its rendering claim |
| `ekos/crates/recovery/src/python_analyzer.rs` | `known_classes` pre-pass; `python_symbol_kir_id`/`extends_kir_id` helpers; real `Extends` edge emission in `ClassDef` handling; 6 new tests |
| `pdf-reader/.ekos/` (external project) | Rebuilt fresh against the fix |
