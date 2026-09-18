# Devlog 188 — RFC 0147: the Perl connector and structural analyzer

**Date:** 2026-09-18
**PRs:** (single branch, merged locally)
**Branch:** `feat/perl-connector` → `main` (squash-merged)

---

## Summary

EKOS compiles Perl now. RFC 0147 adds `plugins/perl` (observer) and
`recovery/src/perl_analyzer.rs` (structural analyzer), giving Perl the same treatment Rust,
Python, Elixir and JavaScript already had: real packages, subs, dependency and inheritance edges,
POD descriptions, source spans, signatures and evidence — instead of `plugins/file`'s bare
declaration-name strings.

This closes the gap LedgerSMB's own `ekos.toml` had been documenting in a comment for two RFCs:
*"Perl .pm/.pl — File objects and harvested declaration symbols only; EKOS has no Perl AST
analyzer today."* RFC 0146 had compiled LedgerSMB's 158 tables and 214 foreign keys; the ~700 Perl
files that actually read and write them stayed invisible. Verified end-to-end on LedgerSMB's real
`lib/` tree: **272 packages, 1,333 subs, 1,154 `DependsOn` edges, 34 `Extends` edges, 1,313 source
spans, 823 sub descriptions and 272/272 package descriptions — with zero LLM calls.**

One real defect surfaced during that verification and was fixed in the same change: a
cross-kind identity conflict that would have failed `ekos resolve` by default on most real Perl
codebases.

---

## PR — RFC 0147: Perl connector and structural analyzer

### Problem / motivation

Perl was the last major language in this project's own working set getting nothing but the crude
prefix scan. It is also the language of exactly the estate EKOS exists to compile: long-lived,
business-critical, under-documented systems whose authors have moved on. "Where does payment
status come from?" could be answered down to the LedgerSMB schema and no further, because the
layer between the schema and the user was not compiled.

### What was built

| Component | Role |
|---|---|
| `plugins/perl` (`PerlObserver`) | One artifact per `.pl`/`.pm`/`.t`/`.psgi` file, verbatim source, no parsing — plus `.cgi` gated on a real `perl` shebang |
| `recovery/src/perl_analyzer.rs` (`PerlAnalyzerPass`) | Packages, subs, `DependsOn`/`Extends` edges, POD descriptions, `source_span`, `signature`, evidence |
| `ObjectKind::Custom("PerlPackage")` / `("PerlSymbol")` | Both registered `structurally_keyed: true` in `custom_kinds::REGISTRY` |
| `identity::is_expected_perl_package_symbol_pair` | Narrows the cross-kind conflict detector for the one mechanically-expected Perl pair |
| Seven downstream registries | `doc_links`, `llm_description` (module + symbol kinds), `docs-gen` (entity pages, API grouping, doc-bearing kinds), CLI build/recover wiring |

### Implementation details worth remembering

**Perl cannot be statically parsed, and the analyzer says so.** `BEGIN` blocks and prototypes
change the grammar mid-parse — the only complete Perl parser is `perl` itself. Shelling out to
`perl -MO=Deparse`/`PPI` was rejected in the RFC on three independent grounds: it needs a Perl
toolchain wherever `ekos recover` runs, it breaks reproducible builds, and it *executes `BEGIN`
blocks from observed code* — arbitrary code execution on untrusted input. So this is a bounded
hand-written line scanner, the RFC 0081 (Elixir) precedent.

**Pragmas had to be excluded or they would have buried every real edge.** Perl's own convention
reserves all-lowercase module names for pragmas. Every single file starts `use strict; use
warnings;`, so emitting those would have produced ~550 noise edges against two objects and made
the real dependency graph unreadable. `use parent`/`use base` are the deliberate exemptions —
lowercase by that same convention, but carrying real inheritance.

**POD is matched two ways, because real Perl uses two conventions.** Interleaved
(`=head2 foo ... =cut` directly above `sub foo`) and trailing (`=head1 METHODS` at the bottom of
the file documenting every method in one place, adjacent to no code at all). Honouring only the
first would have left the many modules using the second looking undocumented. A name documented
by two headings is left undocumented rather than resolved by guessing — measured coverage was 823
of 1,333 subs, which is the real POD coverage of the codebase, not a ceiling of the matcher.

**Package objects have to absorb, not just dedup.** `elixir_analyzer.rs` keeps the first
occurrence of a module and drops the rest, safe there because every occurrence has the identical
shape. That is false for Perl: only the declaring file's object can carry a POD `description`,
while every `use` reference is thin. First-one-wins would have lost a real module description
purely to artifact iteration order, so `absorb_package` folds later occurrences in, filling gaps
and never overwriting.

### Decisions (alternatives considered, why this choice)

- **No call graph.** Perl's dispatch is fully dynamic (`$obj->$method()`, `AUTOLOAD`, symbol-table
  manipulation, string `eval`). A static call graph would be confidently wrong on real code. Same
  scope decision Elixir/Python/JS each made for their own reasons.
- **`.cgi` by shebang, not by extension.** `.cgi` was the generic CGI extension (Python, shell, C
  binaries too), but legacy Perl web estates are full of real `.cgi` entry points. Requiring the
  real `#!...perl` line is a deterministic, evidence-based test instead of a guess. Extensionless
  `bin/` scripts with a Perl shebang are a documented gap, not a silent one.
- **No `PerlSymbol` for `my $var`.** Matches `javascript_analyzer.rs`'s judgment that non-function
  top-level bindings are data-constant noise.

---

## Knowledge Captured

- **A new language kind touches eight registries, not one.** `custom_kinds::REGISTRY` (+ its
  over-merge guard list), `semantic::doc_links::CODE_KINDS`, `llm_description::MODULE_KINDS` and
  `SYMBOL_KINDS`, and three separate lists in `docs-gen` (`is_entity_page_kind`, `is_symbol_kind`,
  `DOC_BEARING_CUSTOM_KINDS`) plus `render_api`'s container grouping, then the CLI's `build.rs`
  observer list and `recover.rs` pass/stats wiring. Only the first is CI-enforced
  (`every_pipeline_custom_kind_is_registered`); a miss in any of the other seven is a silent
  downstream hole. **EKL is not one of them** — it has only `Object`/`Relationship` entities and
  no per-kind registry, so `FIND PerlPackage ...` fails but `FIND Object WHERE kind = 'PerlPackage'`
  works. That looked like a missed integration point and was not.

- **`similarity::normalize` lowercases, which makes case-only cross-kind collisions
  systematic for Perl.** `use Template;` (Template Toolkit, one of the most widely used CPAN
  modules) produces a `PerlPackage` named `Template`; `sub template` produces a `PerlSymbol`.
  Both normalize to `template`, and `ekos resolve` refuses to proceed when *any* conflict exists.
  This was the single conflict in a 272-package run and would have failed by default on most real
  Perl codebases — training users to reach for `--force`, the one outcome that makes the detector
  worthless. A namespace and a function inside one are categorically different entities, so the
  conflict can never be actionable. Fixed by `is_expected_perl_package_symbol_pair`, mirroring
  RFC 0093's `is_expected_technology_jsmodule_pair` exactly: narrows the specific two-kind pair,
  and a third kind in the group still conflicts.

- **`$#array` will eat your line if you strip `#` comments naively.** `$#rows` is real, common
  Perl for "last index of `@rows`". A quote-aware comment stripper that doesn't also check for a
  preceding `$` truncates the line at a meaningless point — which, since the closing brace is
  usually after it, desynchronizes brace-depth tracking for the rest of the file and silently
  destroys every subsequent `source_span`. Guarded and tested.

- **POD directives must be recognized at column 0 only.** `perlpod` requires it, and the reason
  matters: treating an indented `=` as a directive makes `my $x = 1;` inside a sub body open a POD
  block that swallows the rest of the file. Tested both directions.

- **Verify recovered structure by counting the source, not by trusting the total.** The 34
  `Extends` edges were checked against 33 real `use parent`/`use base` lines — the discrepancy is
  `use parent qw( Locale::Maketext Exporter )`, one line naming two superclasses, and both edges
  are correct. A bare total of "34" would have looked equally plausible if it were wrong.

- **The `ekos.toml` of an observed workspace is a real record of what EKOS could not yet do.**
  LedgerSMB's config had carried an accurate prose description of this exact gap since RFC 0146.
  Worth reading those comments before picking work — it should be updated now.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0147-perl-connector.md` | New RFC — motivation, scope, the eight integration points, alternatives |
| `ekos/plugins/perl/Cargo.toml`, `src/lib.rs` | New `PerlObserver` + 6 tests (extensions, `.cgi` shebang both ways, bare-file path, hash stability) |
| `ekos/crates/recovery/src/perl_analyzer.rs` | New `PerlAnalyzerPass` + 39 tests |
| `ekos/crates/recovery/src/lib.rs` | `perl_analyzer` module + `PerlAnalyzerPass`/`PerlStats` re-export |
| `ekos/crates/kir/src/custom_kinds.rs` | `PerlPackage`/`PerlSymbol` rows (`structurally_keyed: true`) + guard list |
| `ekos/crates/identity/src/lib.rs` | `is_expected_perl_package_symbol_pair` + 3 tests |
| `ekos/crates/semantic/src/doc_links.rs` | `CODE_KINDS` |
| `ekos/crates/recovery/src/llm_description.rs` | `MODULE_KINDS` / `SYMBOL_KINDS` |
| `ekos/crates/docs-gen/src/lib.rs` | Entity pages, `is_symbol_kind`, doc-bearing kinds, `render_api` package grouping |
| `ekos/crates/cli/Cargo.toml`, `commands/build.rs`, `commands/recover.rs` | Observer registration, pass wiring, stats output |
| `ekos/Cargo.toml` | `plugins/perl` workspace member |
