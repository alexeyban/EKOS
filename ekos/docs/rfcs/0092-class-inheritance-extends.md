# RFC 0092 — Class Inheritance (`RelationshipKind::Extends`), Python v1

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-25
**Implemented:** 2026-08-25

---

## Motivation

Confirmed real gap, named explicitly in this session's gap-closure list: `RelationshipKind::Extends`
already exists in the KIR enum (`crates/kir/src/lib.rs`) but has zero producers across every
analyzer and, as a direct consequence, zero real data for any `docs-gen` consumer to render — the
blocker behind ever generating a real class-level architecture diagram. A real, common shape,
already present in `pdf-reader`'s own compiled source (`backend/app/db/models.py`,
`backend/app/api/ai.py`):

```python
class Base(DeclarativeBase):
    pass

class Document(Base):
    __tablename__ = "documents"
    ...

class TranslateRequest(BaseModel):
    ...
```

— compiles today into ordinary, disconnected `Custom("PythonSymbol")` class objects with no edge
between `Document` and `Base`, even though `python_analyzer.rs` already parses and visits every
`ast::Stmt::ClassDef` (RFC 0091 reused this same visit for `__tablename__` detection) and the AST
node already carries `bases: Vec<Expr>` — the information is parsed and immediately discarded.

## Design

### Scope: Python only, v1

The only language with a real, live-verifiable inheritance chain available this session —
`pdf-reader`'s `Document(Base)`/`PageCache(Base)`/`TranslationCache(Base)` (all extending a
locally-defined `Base`) and `TranslateRequest(BaseModel)`/`ExplainRequest(BaseModel)` (extending
an external, not-locally-defined class). JS/TS `class X extends Y` is structurally the identical
shape and a real, legitimate future extension (`javascript_analyzer.rs` already visits class
declarations for `JsSymbol`), but not attempted without a real project exercising it —
`pdf-reader`'s frontend is entirely functional-component React, no class declarations anywhere in
scope. Rust has no direct equivalent (`impl Trait for Struct` is a different relationship shape,
composition/interface-satisfaction, not inheritance) and Elixir's closest analogue (`use`/
`__using__` macro injection) is semantically distinct enough not to force into the same kind — both
explicit non-goals, not oversights.

### Resolution scope: same-file only, matching RFC 0091's `ForeignKey` precedent exactly

A base class expression is only turned into a real `Extends` edge when it resolves to a real,
already-known `PythonSymbol` class **defined in the same file** — the identical scoping decision
RFC 0091 made for `ForeignKey` resolution, for the identical reason: no cross-file/cross-module
symbol table exists in this analyzer, and guessing would risk a wrong edge. `Document(Base)`
resolves (both classes are in `db/models.py`); `Base(DeclarativeBase)` and
`TranslateRequest(BaseModel)` do not (`DeclarativeBase`/`BaseModel` are imported from
`sqlalchemy.orm`/`pydantic`, never locally defined) — the base name is still real (visible via
`import` edges already compiled by the pre-existing `ImportFrom` handling), just not promoted to a
fabricated `Extends` edge pointing at a `PythonSymbol` that doesn't exist. Only `ast::Expr::Name`
bases (a bare identifier) are checked against local classes; `ast::Expr::Attribute` bases (a dotted
reference like `orm.DeclarativeBase`) can never refer to a same-file class by construction (a
locally-defined class is always referenced by its bare name within its own file), so those are
skipped without even attempting a lookup — not a partial implementation, a correct one for what
this shape can ever resolve to.

### Implementation

`python_analyzer.rs` gains a pre-pass (mirrors RFC 0091's `known_tables` collection exactly):
`known_classes: HashMap<String, KirId>`, every top-level `ClassDef`'s name mapped to the same
`python-symbol:{path}:{name}` id `add_symbol` already mints, collected before the main walk so a
base class declared later in the file (or, per Python's own resolution order, even one declared
earlier — `known_classes` doesn't care which) still resolves regardless of declaration order.

The `ClassDef` arm, after its existing `add_symbol` call, walks `c.bases` and for each
`ast::Expr::Name` matching an entry in `known_classes`, emits one `RelationshipKind::Extends` edge
from the class's own symbol id to the base's. Id: deterministic, keyed by `(from, to)` — a class
extending a given base is a boolean fact, matching `crate_topology_analyzer.rs`'s
`depends_on_kir_id` precedent (RFC 0070/0071's fix for the exact failure mode a non-deterministic
relationship id causes: unbounded duplicate accumulation across repeated `recover` runs).

### Rendering: none needed

`docs-gen`'s generic `render_object_page` (`--layout objects`) already renders every real
relationship touching an object with zero kind-specific code (`build_object_page_model`,
`crates/docs-gen/src/lib.rs`) — grouped into four existing structural buckets (`Based on`/
`Contains`/`Used in`/`Dependent on`, by `(is_contains, outgoing)`, not one prose section per literal
`RelationshipKind`), so a real `Extends` edge lands in `### Dependent on` alongside any other real
outgoing non-`Contains` edge rather than getting its own `### Extends` heading — an earlier draft
of this RFC claimed otherwise before actually checking `build_object_page_model`'s real grouping
logic, corrected here rather than left standing. What *does* carry the real kind visibly with zero
rendering changes: the same object page's Mermaid diagram (`render_mermaid_graph`) labels every
edge with its real `RelationshipKind` — `Document -->|Extends|-> Base` renders literally, verified
directly (see below). The real data is compiled and visible either way; getting `Extends` its own
prose section (matching e.g. `## Data Architecture`'s dedicated `### Entity Relationships`
treatment for `ForeignKey`) is real, additive `docs-gen` work, deliberately left for a later,
appropriately-scoped pass once a second language's worth of real `Extends` data exists to design
the section against.

### Explicit non-goal: a rendered class-inheritance diagram

A real Mermaid class diagram (the original motivation named in this session's gap list — "the real
blocker behind wanting an auto-generated class-level architecture diagram") is **not** built by
this RFC. Getting real `Extends` data compiled is the actual blocker being removed; a dedicated
diagram-rendering feature consuming it is a separate, later, appropriately-scoped RFC once there's
more than one language's worth of real data to render (JS/TS `extends` support would make the
diagram meaningfully more useful on a typical full-stack project) — building the diagram around
Python-only data now would risk under-scoping the visualization for the shape it will actually need
to handle.

## Verification

- `python_analyzer.rs`: unit tests for `known_classes` collection, `base_class_names` extraction
  (`Name` bases matched, `Attribute` bases skipped without a lookup attempt), a same-file resolved
  case, an unresolved-external-base case (no edge, no fabrication), and a real end-to-end test
  mirroring `pdf-reader`'s exact `Base`/`Document` shape (`Base(DeclarativeBase)` unresolved,
  `Document(Base)` resolved).
- Full workspace gate (`fmt`/`build`/`clippy -D warnings`/`test --workspace`) clean, `tests/integration`
  3/3.
- Live-verified against `pdf-reader`'s real `db/models.py`/`api/ai.py`: real `Extends` edges from
  `Document`/`PageCache`/`TranslationCache` to `Base` (`ekos query neighbourhood` on `Document`'s
  real `PythonSymbol` id shows `Extends 8be8497e... → 440e83d5...`, the real `Base` symbol); the
  generated `Document` object page's `## Relationships` → `### Dependent on` lists the edge, and
  its Mermaid diagram renders the literal edge label `Document -->|Extends|-> Base` — both with
  zero `docs-gen` code changes.
