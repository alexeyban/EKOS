# Devlog 12 — Near-real open-source test fixtures + integration harness

**Date:** 2026-07-11
**PRs:** —
**Branch:** main

---

## Summary

Built the "Integration test harness" TODO item, sourcing near-real, open-source data instead of
purely synthetic fixtures: a real 13-table Northwind schema (Microsoft, MIT-licensed) alongside the
existing hand-authored `ecommerce.sql`, and 39 real commits from `odoo/odoo`'s `addons/utm` module
(LGPL-3.0, path-filtered via `git-filter-repo`, vendored as an offline `git bundle`). Also upgraded
the four Phase-14 proprietary-connector mocks (Salesforce/SAP/Fabric/Snowflake) to mirror each
vendor's own publicly documented example payloads. The real payoff: testing against realistic data
immediately caught two genuine pre-existing bugs the tiny synthetic fixtures had never exercised —
`DefaultResolver` was falsely merging distinct tables that share a name prefix, and `GitAnalyzerPass`
was reading commit fields from the wrong JSON nesting level and had never once produced a real
`CoupledWith` relationship against any actual repository. Both fixed and regression-tested. All 3 new
end-to-end integration tests pass, full workspace suite (240+ tests) green, clippy clean.

---

## Near-real fixture sourcing

### Problem / motivation

The only test data before this session was `ecommerce.sql` (6 hand-authored tables) and a stub
`sample_project/`. Every git-plugin test built a throwaway 1–20 commit repo via `git init`/`git
commit`. Nothing exercised the full pipeline (build→recover→resolve→compile→commit→query) end to
end, and nothing tested against data resembling a real enterprise's actual schema complexity or git
history shape.

### What was built

| Component | Source / method |
|---|---|
| `tests/fixtures/northwind.sql` | Hand-cleaned ANSI subset of Microsoft's Northwind (`sql-server-samples`, MIT) — 13 real tables, real FK graph |
| `tests/fixtures/git_fixture/odoo_utm.bundle` | 39 real commits from `odoo/odoo`'s `addons/utm`, path-filtered via `git-filter-repo`, vendored as a `git bundle` |
| `tests/fixtures/git_fixture/NOTICE.md` + `LICENSE-LGPL-3.0.txt` | Provenance + vendored license text |
| `tests/fixtures/sample_docs/` | 2 short original Markdown files (no vendoring needed — `FileObserver` treats content as opaque) |
| `plugins/{salesforce,sap,fabric,snowflake}` mocks | Enriched to mirror each vendor's public API reference examples; 6 new unit tests total |
| `crates/recovery/src/sql_analyzer.rs` | 3 new tests against `northwind.sql` |
| `tests/integration/` | New standalone crate, 3 end-to-end tests |

### Implementation details worth remembering

**Sourcing the git fixture took three attempts.** First, a plain `git clone --depth=300` of the
whole `odoo/odoo` monorepo (no blob filter) tried to check out ~40,000 files and stalled past a
5-minute timeout. Second, `--filter=blob:none --no-checkout` cloned fast (23MB) but `git-filter-repo`
failed: it deletes the `origin` remote as a safety measure *before* rewriting history, which broke
partial-clone's ability to lazily fetch the blobs it needed for the surviving path. Third —
what worked — `--no-checkout` (skip materializing the 40K-file working tree) **without** the blob
filter (so all blob data is present locally, no post-hoc fetching needed): 545MB `.git`, then
`git filter-repo --path addons/utm/ --force` in under a second, producing an 888KB repo / 367KB
bundle. Lesson: `--filter=blob:none` and `git-filter-repo` don't compose safely — pick one.

**Module selection mattered.** Within the fetched 3000-commit window (branch `17.0`), most small
addon modules were touched only 1–2 times — too sparse for a "real commit history" fixture. `utm`
(39 touches) and `digest` (46) were the best candidates found; `utm` won for having more substantive
(non-translation) files. Its oldest surviving commit is itself a great fixture artifact: a real
28-file simultaneous commit (models, views, tests, data landing together) — genuine multi-file
coupling, not something a synthetic throwaway repo would produce.

**T-SQL → ANSI cleanup for Northwind was mostly mechanical but had one surprise**: `sqlparser`'s
`ObjectName::to_string()` preserves the original quote style, so `"Order Details"` (double-quoted
because of the space) round-trips as the literal string `"\"Order Details\""` — a test doing
`.name.to_lowercase() == "order details"` fails until the quote characters are stripped first. Noted
inline in the test rather than as a general parser gotcha, since it's specific to quoted-with-space
identifiers.

### Decisions

**Enriched connector mocks based on vendors' own published reference examples** (Salesforce
`describe()` field shapes, SAP's public GWSAMPLE_BASIC demo service, Fabric's public `items`
response shape, Snowflake's public SQL-API statement shape) rather than scraping/embedding literal
API documentation prose — the field *names and types* are public interface specifications, not
copyrightable creative content, so reproducing a representative subset is standard practice and
doesn't carry the same provenance/license concerns as vendoring real git history or a real schema
dump.

**`tests/integration/` is its own standalone Cargo crate**, matching the established convention
(`docs/`, `tests/fixtures/`, `benchmark/` all already live outside the `ekos/` workspace). It
path-deps into `ekos/crates/cli` and calls `ekos::commands::{build,recover,resolve,compile,commit}::run(...)`
directly — no subprocess spawning, no network, matching "without external services" cleanly.

---

## Bug 1 — `DefaultResolver` false-merges tables sharing a name prefix

### How it was found

The very first integration test run against `ecommerce.sql` (not even Northwind yet) failed: `orders`
and `order_items` got merged into one canonical table at 0.90 confidence. Northwind then showed the
same pattern at greater scale: `Employees`+`EmployeeTerritories`, `Customers`+`CustomerDemographics`+
`CustomerCustomerDemo`. This bug has existed since Phase 7/8 shipped — no test before this session
ever asserted on post-`compile` ledger contents, only on the pre-resolution structural parse.

### Root cause

`DefaultResolver::score` computed `structural` similarity as `1.0` whenever both objects share an
`ObjectKind` (i.e. almost always `1.0` for any two tables) — a constant that contributed zero
discriminating signal. `combined = 0.7 * name_similarity + 0.3 * structural` meant any two tables
with a merely-similar name (Jaro-Winkler boosts shared prefixes) could cross the 0.85 threshold on
name similarity alone, since the structural term was never actually structural.

### Fix

`structural_score` (`crates/identity/src/lib.rs`) now computes real Jaccard column-name overlap when
both objects carry a `properties["columns"]` array (as every SQL-derived `KirObject` does) — falling
back to the old same-kind-only signal only when column data isn't available (preserving all 19
pre-existing unit tests, which use hand-built objects with no columns). `orders`/`order_items` now
score near-zero on the structural term (almost no shared columns) and no longer merge; genuine
near-duplicates with overlapping columns still do. 2 new regression tests added.

## Bug 2 — `GitAnalyzerPass` reads commit fields at the wrong JSON level

### How it was found

The Odoo git-fixture integration test failed with `Objects: 0, Relationships: 0` after `compile`,
despite 39 real commits (with 940+ file pairs that genuinely co-changed ≥2 times) being fed in.
Manual inspection of a real `ObservationArtifact`'s JSON showed `files_changed`/`sha`/`author_name`
live under a nested `data` object — `ObservationArtifact`'s `#[serde(flatten)]` merges
`connector_name`/`target`/`data`/`input_ids` into the top level, but doesn't recurse into `data`
itself.

### Root cause

`GitAnalyzerPass::run` indexed `json["sha"]`, `json["author_name"]`, `json["files_changed"]`, etc.
directly at the top level instead of `json["data"]["sha"]` and so on — every field silently resolved
to `Value::Null`, defaulting to `"unknown"` or an empty array. This pass has never correctly read a
single real commit's metadata or produced a `CoupledWith` relationship against any real repository
since it was written. The two existing unit tests only asserted "`pass.run()` doesn't error" —
trivially true even with every field silently empty — so the bug was invisible to `cargo test` the
entire time.

### Fix

Changed all five field reads to index through `json["data"][...]`. Both existing tests rewritten to
assert on the actual decoded `KirGraph` (event count, real SHAs present, author not `"unknown"`,
exact `CoupledWith` relationship with the correct `co_change_count`) instead of just "no error" —
this class of bug can't hide behind a weak assertion again.

---

## Knowledge Captured

- **Weak assertions ("no error thrown") are worse than no test** — they actively hide bugs by giving
  green CI on code that has never worked correctly. Both `git_analyzer.rs` tests looked like real
  coverage (constructing realistic-looking fixture data, running the real pass) but asserted nothing
  about the actual output. The fix for *this* — not just for the JSON-indexing bug — is to always
  assert on decoded output values, not just absence-of-error, whenever a pass's whole job is to
  transform data.
- **`--filter=blob:none` (partial clone) and `git-filter-repo` don't compose** — filter-repo removes
  the `origin` remote before rewriting history, which breaks partial clone's on-demand blob fetch for
  whatever survives the rewrite. Use a full-blob `--no-checkout` clone instead when the end goal is
  `git filter-repo --path ... `; skip `--filter=blob:none` even though it looks like the more
  bandwidth-efficient choice up front.
- **`ObjectName::to_string()` in `sqlparser` preserves original quote characters** — a
  double-quoted identifier like `"Order Details"` round-trips as the literal 15-character string
  `"\"Order Details\""`, not `Order Details`. Any code comparing parsed table names against a plain
  string needs to strip quote characters first if the source SQL uses quoted identifiers.
- **`SemanticCompilerPass` runs its own internal `DefaultResolver` + `apply_merges`** — it doesn't
  consume `ekos resolve`'s output (which is currently print-only, informational). This means
  `ekos resolve`'s merge-proposal report and `ekos compile`'s actual merge behavior are two
  independent runs of the same resolver against the same data, not producer/consumer — worth knowing
  before assuming `resolve`'s printed proposals are what `compile` will actually apply (they should
  be identical today since both call the same `DefaultResolver::new()`, but that coupling is
  implicit, not structural).

---

## Files Changed

| File | Change summary |
|---|---|
| `tests/fixtures/northwind.sql` | New: real 13-table Northwind schema, MIT-licensed, ANSI-cleaned |
| `tests/fixtures/git_fixture/{odoo_utm.bundle,NOTICE.md,LICENSE-LGPL-3.0.txt}` | New: real Odoo `utm` module history, LGPL-3.0 |
| `tests/fixtures/sample_docs/{README.md,runbook.md}` | New |
| `tests/integration/{Cargo.toml,tests/integration.rs}` | New crate — 3 end-to-end tests |
| `ekos/crates/recovery/src/sql_analyzer.rs` | 3 new Northwind tests |
| `ekos/crates/identity/src/lib.rs` | Fixed false-merge bug (column-overlap structural scoring); 2 new tests |
| `ekos/crates/recovery/src/git_analyzer.rs` | Fixed JSON-nesting bug; 2 existing tests strengthened |
| `ekos/plugins/{salesforce,sap,fabric,snowflake}/src/lib.rs` | Enriched mock fixtures; 6 new tests |
| `.gitignore` | Added `/tests/integration/target/` |
| `TODO.md` | Ticked "Integration test harness" with a detailed status note |
