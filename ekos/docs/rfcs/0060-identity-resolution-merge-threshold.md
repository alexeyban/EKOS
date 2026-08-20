# RFC 0060 — Identity Resolution: Raise the Default Merge Threshold, Strip Table Schema Qualifiers

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-20

---

## Motivation

Devlog 59 first found `crates/identity`'s `DefaultResolver` merging 6 genuinely distinct real
ClickHouse `imported_*` tables into one identity. Devlog 60's whole-repo cold run against
`analytics/` (Plausible Analytics) showed this was not a `Table`-specific or ClickHouse-specific
defect: the same 0.85 default `merge_threshold` (RFC 0007) also merged genuinely different real
contributors (`Person`), unrelated real documents (`Document`), and unrelated real CI pipelines
(`Pipeline`) — every kind that reaches `structural_score`'s "no comparable structural data"
fallback of a flat `1.0`. Two tests already in this crate (`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`,
`distinct_pdf_tables_in_one_document_do_not_all_merge`, added 2026-08-03) had already documented
one instance of this exact failure shape and were left `#[ignore]`d pending a fix — this RFC is
that fix, arrived at independently while investigating the newer, `analytics/`-sourced failures.

Before writing any code, real numbers were computed (not guessed) for every real merge proposal
`ekos resolve` produced against `analytics/` — 8 `Person` proposals (3 genuinely the same
contributor under a nickname/username variant, 5 genuinely different real people, cross-checked
against `git log --author`), 5 `Table` proposals (real column data read from the compiled schema),
2 `Document` proposals, 2 `Pipeline` proposals — using the crate's own `similarity::jaro_winkler`/
`jaccard` functions directly, not estimated.

**Finding 1 — the threshold itself is the wrong operating point.** At 0.85, 16 of 17 known-wrong
real merges clear the bar (only `shield_rules_country`/`shield_rules_ip`, the case with the
weakest column overlap, happens to already fail). At 0.90, all 3 known-correct merges stay intact
while 14 of the 17 known-wrong ones are rejected — a large, verified improvement. **No single
threshold on the current two-term formula (`0.7×name + 0.3×structural`) separates every case**:
the known-correct and known-wrong combined scores genuinely interleave above 0.90 (e.g. `Build
Private Images GHCR`/`Build Public Images GHCR`, two real, different pipelines, scores 0.9277 —
higher than one of the three known-correct Person merges). This is stated plainly rather than
tuned further on a 17-example sample; see `DEFAULT_MERGE_THRESHOLD`'s doc comment for the full
numeric accounting.

**Finding 2 — real `Table` names carry a shared schema/database qualifier that inflates
Jaro-Winkler independent of the threshold.** SQL-derived `Table` objects are named with their full
qualifier (`plausible_events_db.imported_visitors`, `public.setup_help_emails`) — every table in
one source shares that qualifier, so Jaro-Winkler's prefix-match bonus rewards it as if it were
evidence of similarity. Measured on real data: `plausible_events_db.imported_visitors` vs.
`plausible_events_db.imported_browsers` scores 0.9507 name-similarity fully qualified vs. 0.8905 on
`imported_visitors`/`imported_browsers` alone — enough of a gap to flip whether the pair clears
0.90. This is the same "long shared prefix inflates similarity" shape already named for `Document`
file paths (`unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`'s doc comment: "block on
the file basename rather than the full relative path"), just for SQL's dotted-qualifier convention
instead of filesystem paths.

## Scope

Two changes to `crates/identity/src/lib.rs`:

1. Raise `DEFAULT_MERGE_THRESHOLD` from 0.85 to 0.90 (`ResolverConfig::default`'s
   `merge_threshold`). `CONCEPT_MERGE_THRESHOLD` (0.95, RFC 0026) is untouched — already stricter.
2. Add `name_for_similarity(obj: &KirObject) -> &str`: for `ObjectKind::Table` objects whose name
   contains a `.` and no `/` (the `schema.table` convention, as opposed to a file path), returns
   only the portion after the last `.`. Every other kind, and unqualified table names, are
   unaffected. Called from `DefaultResolver::score` in place of the raw `obj.name` before
   normalization.

## Non-goals

- **Not a complete fix.** Documented honestly in `DEFAULT_MERGE_THRESHOLD`'s doc comment: 3 of the
  17 known-wrong real pairs (`Build Private Images GHCR`/`Build Public Images GHCR`, `Tracker
  CI`/`Tracker script update`, `ua_inspector.readme.md`/`ref_inspector.readme.md`) still incorrectly
  clear 0.90, and the real `analytics/` re-run this RFC verifies against still shows two residual
  `Document` over-merge clusters (reduced from one 27-object cluster to a 5-object and a 22-object
  cluster — smaller, not eliminated). These are exactly the class of judgment call RFC 0029's
  cross-system `unconfirmed`-until-`ekos_identity_review`-reviewed flow already exists for.
  Extending that same review step to same-source (`DefaultResolver`) merges, not just cross-system
  ones, is real follow-on work, not done here — this RFC's job was verified, honest improvement on
  real data, not a claim of correctness.
- **Not touching `structural_score`'s column-Jaccard logic itself.** It already does real,
  useful work (correctly rejects `shield_rules_country`/`shield_rules_ip` on its own, for
  instance) — the bug was in how its output combines with name similarity and the threshold
  applied to that combination, not in the Jaccard computation.
- **Not stripping qualifiers for non-`Table` kinds.** `Document` names are file paths (`/`,
  extensions) with a completely different structure than SQL's `schema.table` convention; a fix
  for that shape already exists (the threshold change — verified below, it independently resolves
  the one `Document` test case that was previously failing).
- **Not adding a review/confirmation step to `DefaultResolver`.** A materially larger change
  (mirroring RFC 0029's `unconfirmed` relationship + `ekos_identity_review` machinery for
  same-source merges) than this RFC's scope; named above as the natural next step if the residual
  cases matter enough to a user to justify it.

## Design

```rust
pub const DEFAULT_MERGE_THRESHOLD: f32 = 0.90; // was 0.85

fn name_for_similarity(obj: &KirObject) -> &str {
    if obj.kind == ObjectKind::Table
        && !obj.name.contains('/')
        && let Some((_, local)) = obj.name.rsplit_once('.')
        && !local.is_empty()
    {
        return local;
    }
    &obj.name
}

fn score(&self, a: &KirObject, b: &KirObject) -> SimilarityScore {
    let na = similarity::normalize(name_for_similarity(a));
    let nb = similarity::normalize(name_for_similarity(b));
    // ... unchanged from here
}
```

The `/`-exclusion guard on `name_for_similarity` exists because no real analyzer produces a
`Table` name shaped like a file path today, but the function must not misread one as a
`schema.table` qualifier if that ever changes — tested directly
(`name_for_similarity_does_not_strip_a_table_name_containing_a_slash`).

Blocking (`(kind, first-3-normalized-chars)`) is deliberately left keyed on the **full** name, not
the qualifier-stripped one — two tables in different schemas with the same leaf name (`public.users`
vs. `staging.users`) still land in different blocks and are never compared under this change, so no
new false-positive risk is introduced. Only pairs that already shared a block (same qualifier, or no
qualifier on either side) have their *score* affected.

## Alternatives Considered

- **Removing `structural_score`'s flat `1.0` "no data" fallback entirely** (i.e. weight name
  similarity at 100% instead of `0.7×name + 0.3×1.0` when no columns exist to compare) — tested
  against the same 17 real pairs; on its own this loses 2 of the 3 known-correct `Person` merges
  (`Adam Rutkowski`/`Adam`, `Vini Brasil`/`Vinicius Brasil`) since the flat bonus was doing real,
  needed work for those legitimate nickname/variant cases, not just inflating the bad ones.
  Rejected in favor of the threshold change, which was verified not to have this regression.
- **A stricter per-kind threshold for `Table` only** (mirroring `Concept`'s existing override),
  leaving the 0.85 default for everything else — rejected once the diagnostic data showed `Person`,
  `Document`, and `Pipeline` all exhibit the identical failure shape; a `Table`-only fix would have
  left 3 of the 4 affected kinds unfixed.
- **Tuning the threshold further (e.g. 0.93+) to also catch the 3 residual known-wrong pairs** —
  rejected: at 0.93, `RobertJoonas`/`Robert` (a known-correct merge, 0.9300) sits right at the edge
  and `Adam Rutkowski`/`Adam` (0.9000) and `Vini Brasil`/`Vinicius Brasil` (0.9245) would both be
  lost — trading 3 known-good merges to catch 3 known-bad ones is not a net improvement, and no
  value beyond 0.90 was found that avoids this trade on the real sample tested.

## Testing

- `name_for_similarity`: qualifier stripped for `Table` (`plausible_events_db.imported_visitors`
  → `imported_visitors`), unqualified table names untouched, non-`Table` kinds untouched (file
  paths like `test/priv/README.md` must not have their extension mistaken for a qualifier), a
  defensively-tested `Table` name containing `/` left untouched.
- **Real regression tests**, each using real names/columns read directly from `analytics/`:
  `real_clickhouse_imported_tables_do_not_merge`, `real_postgres_email_template_tables_do_not_merge`
  (identical columns, name alone must not be enough), `real_distinct_contributors_with_similar_names_do_not_merge`
  (Niklas Hambüchen/Niklaas Baudet von Gersdorff), `real_same_contributor_under_different_git_names_still_merges`
  (RobertJoonas/Robert — the other side of the same fix must keep working).
- Two pre-existing tests un-blocked: `unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`
  was `#[ignore]`d as a known bug since 2026-08-03 — now passes and is un-ignored, with its doc
  comment updated to explain why. `distinct_pdf_tables_in_one_document_do_not_all_merge` remains
  `#[ignore]`d — a different failure shape (deterministically-indexed PDF-table names, `"{path}:
  table {n}"`, scoring 0.99+ on name alone) this RFC's fix does not and was not expected to close;
  left exactly as it was.
- Full workspace gate: `cargo build --workspace && cargo test --workspace && cargo clippy
  --workspace -- -D warnings && cargo fmt --check`.
- Live verification: fresh cold `init/build/recover/resolve/compile/commit` against the real
  `analytics/` repo (both this fix and RFC 0059 present together). `ekos resolve` merge proposals
  dropped from 19 to 8; every one of the 5 known-wrong real `Table` proposals and 5 of the 8
  known-wrong real `Person` proposals are gone; all 3 known-correct `Person` merges remain.
  `ekos query find "imported_browsers"` now returns the real
  `plausible_events_db.imported_browsers` `Table` object directly (previously absent — merged away
  under `imported_visitors`'s identity). `ekos query find "Niklaas"` now returns the real
  `Niklaas Baudet von Gersdorff` `Person` object directly (previously absent).

## Acceptance Criteria

- [x] `DEFAULT_MERGE_THRESHOLD` raised to 0.90; `name_for_similarity` implemented and wired into
      `DefaultResolver::score`.
- [x] 9 new unit tests (4 real-pair regressions, 4 `name_for_similarity` cases, 1 already-fixed
      pre-existing ignored test un-ignored); 49 total in the crate, 1 remaining `#[ignore]`
      (unrelated failure shape, out of scope).
- [x] Full workspace `cargo build/test/clippy/fmt` clean.
- [x] Live: rebuilt `target/release/ekos`, reran the full pipeline against the real `analytics/`
      repo from a genuinely cold state. Merge proposals dropped 19 → 8; `imported_browsers` and
      `Niklaas Baudet von Gersdorff` are both directly queryable again under their own real names.

## Files Changed

| File | Change |
|---|---|
| `ekos/docs/rfcs/0060-identity-resolution-merge-threshold.md` | This RFC |
| `ekos/crates/identity/src/lib.rs` | `DEFAULT_MERGE_THRESHOLD` (0.85→0.90), `name_for_similarity`, 9 new/updated tests |
