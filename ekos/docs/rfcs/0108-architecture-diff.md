# RFC 0108 — Architecture Diff (RFC 0068 §55)

**Status:** Accepted
**Author:** EKOS team
**Created:** 2026-08-26
**Implemented:** 2026-08-26

---

## Motivation

RFC 0068 §55 asks for a real architecture-level diff — distinct from the existing `ekos diff`
(RFC 0018-era), which reports raw ledger-entry-id counts (`Added: N`, listing bare `entry #N`
lines) with no semantic meaning at all. `TODO.md`'s own §62 tracking already named the distinction:
"needs to diff at the Claim/entity level," not the raw entry level.

RFC 0069's `architecture_drift.rs` already solved an adjacent, narrower problem — comparing one
role `Claim`'s oldest-vs-newest recorded value — but that's a single-object, whole-history
comparison, not a two-point-in-time, whole-architecture one. This RFC generalizes: given two
timestamps, report what changed across every architecturally-meaningful KIR kind this project
already compiles real, evidence-backed objects for.

## Design

### Reused, not rebuilt: `all_objects_at` (RFC 0096) + deterministic ids (RFC 0072/0074/0094/0065)

Every kind this diff covers was already confirmed, by reading each analyzer directly (not
assumed), to mint a **deterministic** `KirId` for the real-world thing it represents:
`Custom("Technology")` (`dependency_analyzer.rs`/`elixir_analyzer.rs`/`package_json_analyzer.rs`'s
own `technology_kir_id`, keyed by name), `Custom("Claim")` role classifications
(`architecture_reasoning::role_claim_kir_id`, keyed by crate manifest dir), `Custom("Risk")`
concentration risks (`concentration_risk_kir_id`, RFC 0094, keyed by the target object), and
`Custom("ArchitectureGap")` (`crate_topology_analyzer::architecture_gap_kir_id`, keyed by crate dir
+ unresolved dependency name). Deterministic ids mean "the same real-world thing" reliably has the
same `KirId` across two points in time — the diff is a plain id-set comparison per kind, not a
fuzzy name-matching problem.

`KnowledgeStore::all_objects_at(at: DateTime<Utc>)` (RFC 0096, already shipped this session) gives
a full object snapshot at any timestamp on both backends. `diff_architecture(before, after)` is a
pure function over two such snapshots — no new ledger primitive needed, no LLM call, deterministic
and reproducible-build-compatible like every other renderer in `docs-gen`/`recovery`.

### What the diff reports

- **Technologies added/removed** — by name, `Custom("Technology")` id-set difference.
- **Role changes** — `Custom("Claim")` objects with `predicate == "has_role"` present in both
  snapshots whose `properties["value"]` differs (the same "documented vs. observed" comparison
  `architecture_drift.rs` already established, generalized from one claim's whole history to every
  claim between two specific points).
- **Risks added/resolved** — `Custom("Risk")` id-set difference. "Resolved" here means the
  underlying condition (real fan-in crossing the concentration-risk threshold, RFC 0094) no longer
  holds as of the later snapshot — an honest, mechanically-derived signal, not a claim that anyone
  addressed it.
- **Architecture gaps added/resolved** — `Custom("ArchitectureGap")` id-set difference; "resolved"
  means the previously-unresolved dependency is now real and compiled (or the crate/dependency was
  removed), not that a human answered an open question.

### `ekos architecture diff --from <ts> --to <ts>`

New subcommand alongside the existing `ekos architecture investigate` (`crates/cli/src/commands/
architecture.rs`, same file — the natural sibling). Opens the store, calls `all_objects_at` twice,
runs `diff_architecture`, prints a real report grouped by category; each category honestly says
"none" rather than being silently omitted when empty, matching this crate's own established
"honest empty state" convention throughout `docs-gen`.

## Non-goals

- **Relationship-level architecture diff** (e.g., a `DependsOn` edge added/removed between two
  crates). Real, valuable follow-on — `all_relationships_at` (RFC 0096) already exists as the
  primitive — deliberately scoped out of this RFC to keep it to the object-kind coverage named
  above; not attempted here to avoid an open-ended "diff everything" surface in one increment.
- **Continuous/scheduled drift detection** (RFC 0068 §56, the separate "Architecture Drift" item).
  This RFC's diff is on-demand, two explicit timestamps — a real prerequisite for §56, not §56
  itself, which would need actual scheduling infrastructure this project doesn't have yet.
- **A UI/report format beyond plain CLI text.** `docs-gen` integration (a rendered
  `ArchitectureDiff.md` alongside the curated doc set) is real, natural follow-on work, not
  attempted here.

## Verification

New `ekos-recovery` unit tests for `diff_architecture`: technology added/removed detected
correctly; a role `Claim` present in both snapshots with a changed `value` reports a `RoleChange`,
an unchanged one reports nothing; a `Claim` present only in `after` is *not* misreported as a role
change (it's a new claim, not a changed one — a real distinction worth its own test); risks/gaps
added and resolved detected correctly via id-set difference; an empty-to-empty diff reports nothing
in every category. New `ekos` (CLI) test: `ekos architecture diff` against a real two-commit
workspace (a `Technology` added between commits) reports it correctly. Full workspace gate clean
(`cargo fmt`, `build --workspace`, `clippy --workspace -D warnings`, `test --workspace`), `tests/
integration` 3/3.

Live-verified against a real scratch workspace: committed once with one dependency, added a second
dependency, committed again — `ekos architecture diff --from <t1> --to <t2>` correctly reported the
real technology addition, with every other category honestly reporting "none."
