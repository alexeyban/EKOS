# Devlog 59 — Re-analyzing analytics/ after RFC 0057/0058 found a second, different gap

**Date:** 2026-08-20
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Re-ran the full EKOS pipeline against `analytics/` (Plausible Analytics) from a clean cache to
confirm RFC 0057/0058's fix end to end and build a companion "after" presentation deck. Parsing
is now fully correct: `sql-analyzer` reports `objects=15 relationships=0` with zero warnings,
where it previously fell back to an empty graph. Re-analyzing with real data flowing through the
*rest* of the pipeline for the first time surfaced a second, entirely separate finding: identity
resolution (`crates/identity`, untouched by RFC 0057/0058) merges 6 of those 15 real ClickHouse
tables — `imported_visitors`, `imported_operating_systems`, `imported_exit_pages`,
`imported_entry_pages`, `imported_devices`, `imported_browsers` — into a single identity at
confidence 0.93, because they share both a name prefix and a common 8-column base schema. This is
reported, not fixed, in this session — a pre-existing behavior in a different subsystem that
simply had no real ClickHouse column data to operate on until now.

---

## Re-analysis: analytics/ after RFC 0057/0058

### What was done

- `ekos clean` (clears the artifact cache) then a full `ekos build → recover → resolve → compile →
  commit` re-run against `analytics/`, to get an authentic, uncached transcript rather than reusing
  last session's cached pass results.
- `ekos query object`/`ekos ekl "FIND Object WHERE kind = 'Table'"` used to verify not just "no
  warnings" but the actual compiled content — column counts, types, evidence — for `sessions_v2`
  (43 columns) and `events_v2` (39 columns), both fully correct.
- New presentation deck, `docs/presentations/analytics-clickhouse-after.html`, a direct sequel to
  the "cold run" deck from the prior session — same real-output-only convention, backed by 4 new
  real transcript files under `docs/presentations/examples/analytics-clickhouse-after/`.

### The second finding: identity resolution over-merging real Table objects

`ekos resolve`'s merge proposals included:

```
'plausible_events_db.imported_visitors' (Table) — 6 objects merged, confidence 0.93
     • imported_visitors
     • imported_operating_systems
     • imported_exit_pages
     • imported_entry_pages
     • imported_devices
     • imported_browsers
```

Confirmed this isn't a labeling nuance: `ekos query object` on the surviving identity
(`8efeded6-1301-49af-9cae-f00e0691d781`) shows only `imported_visitors`'s own 8 columns
(`site_id`, `date`, `visitors`, `pageviews`, `bounces`, `visits`, `visit_duration`, `import_id`)
and exactly 1 evidence entry — the other five tables' distinguishing columns
(`operating_system`/`exit_page`/`entry_page`/`device`/`browser`) and their own `CREATE TABLE`
evidence are not attached to this identity at all. Querying EKOS about `imported_browsers`'s
schema today returns `imported_visitors`'s columns instead.

**Root cause, read directly from `crates/identity/src/lib.rs`:** `DefaultResolver` scores every
same-kind object pair as `combined = 0.7 * name_similarity + 0.3 * structural_score` (`lib.rs:172`)
against a **default `merge_threshold` of 0.85** (`lib.rs:121`) with no per-kind override for
`Table` (only `Concept` gets a stricter `0.95`, `CONCEPT_MERGE_THRESHOLD`). `structural_score`
(`lib.rs:391`) computes Jaccard overlap of `properties["columns"]` name sets when both objects
have real column data — its own doc comment explicitly names the safeguard this was built for:
*"two tables with almost no columns in common (e.g. `Employees` vs. `EmployeeTerritories`) score
near 0 here even when their names are similar."* Every ClickHouse `imported_*` table shares a
common 8-column "spine" (`site_id, date, visitors, visits, visit_duration, bounces, import_id,
pageviews`) plus one or two dimension-specific columns — the opposite shape from the safeguard's
motivating example: *most* columns in common, not *almost none*, driving a real Jaccard score high
enough (combined with the shared `imported_` name prefix's high Jaro-Winkler score, weighted 70%)
to clear 0.85.

**This is not a regression introduced by RFC 0057/0058, and not something either RFC's own code
touched.** `structural_score`'s Jaccard-overlap scoring predates both RFCs entirely. It simply had
nothing to operate on for ClickHouse tables until now: every `ClickHouse`-dialect `CREATE TABLE`
compiled to an empty graph before RFC 0057, so `crates/identity` never saw two real ClickHouse
`Table` objects with overlapping `properties["columns"]` to compare. Fixing the parser is what
*produced* the conditions for this to happen — the same "a passing test suite doesn't mean the
feature works end to end, only running it for real does" lesson devlog_54/55/56/57/58 each drew
from a different angle, this time surfacing one directory further down the pipeline than the fix
itself touched.

**Deliberately not fixed this session.** Unlike RFC 0057→0058's tight bugfix-in-one-plugin-crate
shape, a fix here (loosening the safeguard, adding a stricter per-kind `Table` threshold, or
requiring near-total rather than majority column overlap) changes `crates/identity`'s scoring for
*every* same-kind object comparison across the whole estate, not just ClickHouse — a materially
larger blast radius than a preprocessing function scoped to one SQL dialect crate. Reported to the
user with the exact mechanism, rather than either silently patching a estate-wide scoring function
or silently leaving the finding unstated.

---

## Knowledge Captured

- **`crates/identity`'s column-Jaccard safeguard (`structural_score`, RFC 0007-era) is designed
  against the "different tables, similar name, dissimilar columns" failure shape — not the
  opposite one.** Its own doc comment names `Employees` vs. `EmployeeTerritories` as the case it
  protects against (near-zero column overlap despite name similarity). A family of real tables
  that intentionally share a common base schema (the same "spine" of columns, one row per
  dimension per site per day — an extremely common analytics-schema pattern, not specific to this
  repo) produces the inverse shape: high column overlap *and* high name similarity, for objects
  that are still genuinely distinct. The existing safeguard doesn't defend against this because it
  was never the case it was built for.
- **A compiler pass finally succeeding can surface a real bug in a completely different,
  untouched pass — not by introducing one, but by finally producing the input that pass needed to
  misbehave on.** RFC 0057/0058 only ever touched `plugins/sql-dialect-clickhouse`; the bug this
  devlog reports lives entirely in `crates/identity`, unmodified this session. Live
  re-verification after a fix should check the *next* pipeline stage too, not just confirm the
  fixed stage's own output looks right in isolation.
- **"15 objects, zero warnings" and "10 queryable identities" are both true and not
  contradictory** — they're facts about two different compiler stages (`recover` vs. `resolve`).
  Reporting only the first number would have been accurate but incomplete for anyone actually
  trying to query this estate's ClickHouse schema.

---

## Files Changed

| File | Change summary |
|---|---|
| `docs/presentations/analytics-clickhouse-after.html` | New deck: re-analysis after RFC 0057/0058, including the identity-resolution finding |
| `docs/presentations/examples/analytics-clickhouse-after/*.txt` | 4 real, unedited terminal transcripts backing the deck |
| `docs/presentations.html`, `docs/index.html` | New deck listing entries |
| `README.md` | Third ClickHouse deck link, describing both what's fixed and the new finding |
| `TODO.md` | New tracked-but-not-fixed item for the identity-resolution over-merge |
| (in `analytics/`, not this repo) `.ekos/ledger`, `.ekos/ckm` | Re-compiled from a clean cache; now holds 15 structurally-recovered ClickHouse `Table` objects, 10 of them distinctly queryable |

## Still open (tracked, not silently dropped)

- **The `imported_*` over-merge is not fixed.** A real design decision — not a mechanical
  preprocessing fix — is needed: a per-kind stricter threshold for `Table` (mirroring `Concept`'s
  `0.95`), a "near-total overlap, not majority overlap" structural-score shape, or something else.
  Offered to the user as follow-on work rather than assumed in either direction.
