# RFC 0147 — Perl Connector and Structural Analyzer

**Status:** Accepted
**Date:** 2026-09-18
**Depends on:** RFC 0019 (file observer), RFC 0041 (Rust analyzer — the observer/analyzer split),
RFC 0081 (Elixir analyzer — the hand-written-scanner precedent), RFC 0085 (JavaScript analyzer),
RFC 0088 (`source_span`), RFC 0092 (`Extends` edges from real inheritance), RFC 0135 Part D
(`custom_kinds` registry), RFC 0140 §1 (`KirEvidence` per span-carrying symbol), RFC 0141 §1/§4
(`signature`, `symbol_kind`)

---

## 1. Motivation

Perl is the last major language family in this codebase's own real working set that gets nothing
but `plugins/file`'s crude declaration-prefix symbol scan — bare identifier name strings, no
module structure, no relationships, no spans, no descriptions.

This is not hypothetical. LedgerSMB — the Perl/PostgreSQL ERP this project already compiles, and
the subject of RFC 0146's whole Postgres-dialect effort and the traceability demo built on top of
it — is **a Perl application**. Its 158 tables and 214 foreign keys are compiled and queryable;
the ~700 `.pm`/`.pl` files that actually read and write those tables are not. "Where does payment
status come from?" can today be answered down to the schema and no further, because the layer
between the schema and the user is invisible to the compiler.

Perl is also the single most common language in exactly the kind of estate EKOS exists to
compile: long-lived, business-critical, under-documented systems whose original authors have
moved on.

## 2. Goals / Non-goals

**Goals**

- One `ObservationArtifact` per Perl source file, raw source captured verbatim, no parsing in the
  observer — the `Observer`/analyzer split every language plugin since RFC 0041 has used.
- Real structural recovery: packages, subroutines, dependencies, inheritance, POD documentation,
  source spans, signatures.
- Internal dependency edges that **resolve onto the same real object** as the package's own
  declaration, so `Foo::Bar` used in one file and declared in another is one node, not two.

**Non-goals**

- A call graph. Perl's method dispatch is fully dynamic (`$obj->$method()`, `AUTOLOAD`, symbol
  table manipulation, string eval); a static call graph would be confidently wrong on real code.
  Same scope decision `elixir_analyzer.rs`, `python_analyzer.rs` and `javascript_analyzer.rs` all
  made for their own reasons — module/symbol/dependency structure is what an architecture diagram
  needs.
- Full parsing. **Perl cannot be statically parsed** — the classic result is that only `perl`
  itself can parse Perl, because `BEGIN` blocks and prototypes change the grammar mid-parse. This
  is a bounded structural scanner and says so, in the same honest register RFC 0081 established.

## 3. Why a hand-written scanner

No mature Perl-grammar Rust crate exists (unlike `syn` for Rust, `rustpython-parser` for Python,
`oxc_parser` for JS/TS). The realistic alternatives were:

| Option | Verdict |
|---|---|
| Shell out to `perl -MO=Deparse` / `PPI` | Rejected — requires a Perl toolchain on the machine running `ekos recover`, breaks reproducible builds, and executes `BEGIN` blocks from the observed codebase (arbitrary code execution on untrusted input). |
| Write a full Perl grammar | Rejected — provably not possible statically; would be a fake. |
| Bounded hand-written line scanner | **Chosen** — exactly the RFC 0081 precedent, with the same documented limitations. |

## 4. Observer — `plugins/perl`

`PerlObserver`, mirroring `ElixirObserver` line for line (walk, ignore-patterns, bare-file
`observe_paths` handling, sha256, verbatim `source`).

**File selection.** By extension: `.pl`, `.pm`, `.t` (Perl's universal test-file extension),
`.psgi` (Plack/PSGI application entry points). Plus `.cgi` **only when** the file's first line is
a `#!` shebang naming `perl` — `.cgi` is not Perl-exclusive, and legacy Perl web estates
(LedgerSMB's own `old/bin/*.pl` + dispatcher layout included) are full of them. Guessing by
extension alone would misfile another language's CGI scripts; requiring the real shebang is a
deterministic, evidence-based test.

Extensionless files carrying a Perl shebang (`bin/` scripts) are **not** collected — that would
require sniffing every extensionless file in the tree. Documented gap, not a silent one.

## 5. Analyzer — `PerlAnalyzerPass`

`crates/recovery/src/perl_analyzer.rs`. Emits two new `ObjectKind::Custom` kinds:

| Kind | Identity | Registry flag |
|---|---|---|
| `PerlPackage` | qualified package name (`Foo::Bar`) | `structurally_keyed: true` |
| `PerlSymbol` | (owning package/file id, sub name) | `structurally_keyed: true` |

Both **must** be registered in `ekos_kir::custom_kinds::REGISTRY` as structurally keyed — they
are the exact `ElixirModule`/`ElixirSymbol` shape, and would otherwise hit the same
name-prefix + same-kind-`1.0`-fallback over-merge that RFC 0135 Part D's registry exists to
prevent (`LedgerSMB::Payment`, `LedgerSMB::Report`, … collapsing into one object).

### 5.1 What is recovered

- **`package Foo::Bar;`** and the block form **`package Foo::Bar { ... }`** → `PerlPackage` object
  + `Contains` edge from the owning `File`. The statement form's scope runs to the end of the
  enclosing block or file, which is how the scanner attributes later `sub`s.
- **`sub name { ... }`** → `PerlSymbol` (`symbol_kind: "sub"`), `Contains` edge from the owning
  package — or from the `File` directly when the file declares no package (a plain script, very
  common for `.pl`/`.t`). `visibility`: `"private"` when the name starts with `_` (the real,
  near-universal Perl convention, and the only visibility signal Perl actually has — there is no
  keyword), `"public"` otherwise.
- **`use Foo::Bar;` / `require Foo::Bar;`** → `DependsOn` from the owning package (or `File`) to a
  `PerlPackage` target built with the **same** `perl_package_kir_id` the declaration uses, so
  internal dependencies resolve onto the real declared object.
  - **Pragmas are excluded.** Perl's own convention reserves all-lowercase module names for
    pragmas (`strict`, `warnings`, `utf8`, `lib`, `constant`, `vars`, `feature`, `overload`…).
    An all-lowercase `use` target and a bare version (`use v5.36;`, `use 5.010;`) produce no edge —
    they are compiler directives, not dependencies, and would bury every real edge under
    `strict`/`warnings` noise on every single file.
- **Inheritance** → `RelationshipKind::Extends`, from three real forms:
  - `use parent 'Foo::Bar';` / `use parent -norequire, 'Foo::Bar';`
  - `use base qw(Foo::Bar Baz);`
  - `our @ISA = ('Foo::Bar');` / `push @ISA, 'Foo::Bar';`

  `parent`/`base` are the two lowercase `use` targets deliberately exempted from the pragma
  filter above, because they carry a real structural relationship rather than a directive. This
  mirrors RFC 0092's Python `Extends` recovery, and is the same-file-independent case: the target
  id scheme is shared, so cross-file inheritance resolves correctly (unlike Python's documented
  same-file-only limitation).
- **POD documentation** → `description`, from real POD only:
  - A `=head1 DESCRIPTION` (or `=head1 NAME`) block's body → the description of the **first**
    package declared in the file, Perl's universal module-header convention.
  - A `=head2 name` / `=item name` / `=head3 name` block terminated by `=cut` and immediately
    followed (blank lines skipped) by `sub name` → that sub's description.
  - Nothing is fabricated: a sub with no preceding POD gets no `description` property at all.
- **`source_span`** (RFC 0088) per sub, via brace-depth tracking from the sub's opening `{` to its
  matching `}`; **`KirEvidence`** attached from it via `source_evidence::attach` (RFC 0140 §1).
- **`signature`** (RFC 0141 §1): the declaration's own text — including a real Perl signature
  (`sub area($w, $h)`) or prototype when present.

### 5.2 Documented limitations

The scanner strips `#` comments outside quotes and skips POD blocks (`^=\w+` … `^=cut`) and
`__END__`/`__DATA__` sections before structural scanning. It is **not** aware of heredocs,
`q{}`/`qq{}`/`qw()` bracket-quoting with embedded braces, regex literals containing braces, or
string `eval`. A brace inside one of those constructs can desynchronize depth tracking for the
remainder of that one file — the same accepted, documented tradeoff `elixir_analyzer.rs` states
for its own `do`/`end` tracking. This degrades one file's spans; it never fails the pass.

## 6. Integration points

Adding a language kind touches seven registries. All are required — a missed one is a silent
downstream hole, which is exactly the failure mode RFC 0135 Part D's CI guard was written for.

| Location | Change |
|---|---|
| `ekos/Cargo.toml` | `plugins/perl` workspace member |
| `crates/cli/Cargo.toml` + `commands/build.rs` | register `PerlObserver` |
| `crates/cli/src/commands/recover.rs` | collect `perl` artifacts, register the pass, print stats |
| `crates/kir/src/custom_kinds.rs` | `PerlPackage`/`PerlSymbol`, `structurally_keyed: true` + the over-merge guard test list |
| `crates/semantic/src/doc_links.rs` | `CODE_KINDS` — so a backticked `` `LedgerSMB::Payment` `` in a doc links to the real package |
| `crates/recovery/src/llm_description.rs` | `MODULE_KINDS` / `SYMBOL_KINDS` |
| `crates/docs-gen/src/lib.rs` | `is_entity_page_kind`, `is_symbol_kind`, `DOC_BEARING_CUSTOM_KINDS`, and `render_api`'s package grouping (Perl groups by package like Elixir, not by file like Rust/Python) |

## 7. Testing

- Observer: extension selection, the `.cgi`-shebang rule (positive **and** negative), bare-file
  `observe_paths`, hash stability across runs.
- Analyzer: package (both forms), sub + visibility, pragma exclusion, real dependency edges,
  all three inheritance forms, POD for package and sub, `source_span`, `signature`, POD/`__END__`
  skipping, project-qualified ids, and a malformed file not panicking.
- Registry: the existing `every_pipeline_custom_kind_is_registered` guard covers the new kinds
  automatically.

## 8. Alternatives considered

- **Extend `plugins/file`'s prefix scan with `sub `.** Rejected: gives bare name strings with no
  package, no edges, no spans — the exact poverty RFC 0081 was written to end.
- **One `PerlSymbol` per `sub` *and* per `my $var`.** Rejected: matches
  `javascript_analyzer.rs`'s judgment that non-function top-level bindings are data-constant
  noise.
- **Resolve `Foo::Bar->method()` into `Calls` edges.** Rejected — see §2 non-goals.
