# Devlog 43 — RFC 0043: global secrets/PII redaction, plus fixture-pollution cleanup and Databricks/ADF doc regen

**Date:** 2026-08-09
**PRs:** none — direct commits to `main` this session (no feature-branch/PR workflow used)
**Branch:** `main` → `main`

---

## Summary

Two follow-ups to RFC 0042's curated-docs work, then a new cross-cutting security feature.
`Architecture.md`'s Entity Relationships section was showing a fictional Northwind/ecommerce
schema — real data, but from `tests/fixtures/*.sql` and `docs/presentations/examples/` (content
used to validate other RFCs' analyzers), not EKOS's own architecture. Excluded both from EKOS's
own observation and rebuilt the ledger from scratch (append-only, so a re-scan alone can't remove
already-committed pollution). Regenerated `doc/databricks/` and `doc/adf-pipelines/` (previously
flat RFC-0037-era output) with the RFC-0042-upgraded tooling from their own isolated per-project
`.ekos` workspaces. Then, a new user requirement: EKOS must never scan or store secrets, API
tokens, passwords, or other confidential information — RFC 0043 adds a baseline redaction pass
that cannot be disabled, only extended, wired into both of the pipeline's real raw-content entry
points.

---

## Doc cleanup — excluding test/demo fixtures from EKOS's own curated docs

### Problem

`ekos.toml`'s `[observe]` scans the whole EKOS repo, including `tests/fixtures/{northwind,
ecommerce}.sql` (used via `include_str!` in `sql_analyzer.rs` unit tests) and
`docs/presentations/examples/` (Pentaho/PDF demo content for other RFCs' presentation decks).
Both got compiled into EKOS's own ledger and rendered in `doc/Architecture.md` as if they were
EKOS's real schema/pipelines.

### Fix

Added `"fixtures"` and `"examples"` to `ekos.toml`'s `ignore-patterns` (directory-name match,
confirmed no other legitimately-relevant directory in the repo is named either). Since the ledger
is append-only with no object-level delete/tombstone (`grep`ed the whole `crates/`/`plugins/` tree
for one — only `prune_snapshots`/`prune_empty_dirs` exist, both derived-cache cleanup, not ledger
content), a config change alone doesn't retroactively remove already-committed pollution — wiped
`.ekos/` entirely and reran `build → recover → resolve → compile → commit`. Post-fix,
`Architecture.md`'s `## Entity Relationships` honestly reports "No table foreign-key relationships
compiled" instead of a fake Northwind ER diagram; `ekos ekl "FIND object WHERE kind = 'Table'"`
returns zero rows.

## Databricks + ADF doc regeneration

`doc/databricks/` and `doc/adf-pipelines/` (RFC-0037-era, flat 4-file output, predating RFC 0042)
document two separate real projects (`/home/legion/PycharmProjects/azure-databricks-project`,
`/home/legion/PycharmProjects/adf-pipelines`) that had no `ekos.toml`/`.ekos` of their own — traced
back to a multi-project "estate" config at `/home/legion/PycharmProjects/ekos.toml` (compiles
~40 sibling repos into one shared ledger; referenced obliquely by `demo/`'s "estate-scout" agent
transcripts) as the likely original generation source. Rather than reverse-engineer the estate
config's exact invocation, gave each project its own standalone `ekos.toml` (matching the simple
one-observe-path-per-project template already used elsewhere, e.g. `pih-pentaho/ekos.toml`) and
ran the full pipeline independently for each, then `ekos docs generate --layout curated --output
<EKOS repo>/doc/{databricks,adf-pipelines}`. Both regenerated cleanly with honest empty-state
placeholders where the projects have no matching data (no `Cargo.toml` → no Crate/Technology
section; no `.github/workflows/` → no CI/CD section) — not fabricated to look richer than the
underlying analyzers actually cover.

### Files Changed

| File | Change summary |
|---|---|
| `ekos.toml` | `+"fixtures"`, `+"examples"` in `[observe] ignore-patterns` |
| `doc/**` | Regenerated (fixture-free) after full ledger rebuild |
| `doc/databricks/**`, `doc/adf-pipelines/**` | Regenerated with RFC 0042 tooling from fresh per-project `.ekos` workspaces |
| `/home/legion/PycharmProjects/{azure-databricks-project,adf-pipelines}/ekos.toml` | New (outside this repo, not committed here) |

---

## RFC 0043 — Global Secrets/PII Redaction

### Problem / motivation

The ledger is append-only by core design invariant — there is no delete path for a committed
object. Combined with EKOS observing raw file content from every connected system, this means a
stray `.env` file, a hardcoded AWS key, or a checked-in private key gets copied into
`ObservationArtifact`s, `KirObject` properties, and the ledger **permanently**. The user asked for
a global limitation: never scan/save secrets, tokens, passwords, or PII — on by default, not
something a workspace can turn off.

### Investigation

Found exactly two real raw-content entry points, not the many it might first appear to be:

1. `build.rs:158-171` — **every** `Observer` (all ~15 connector plugins, present and future)
   funnels its artifacts through one loop before `artifact_store.write(...)`; a second loop at
   `184-214` in the same file copies `excerpt`/`symbols` again into `KirObject` properties.
2. `recover.rs:89,178,353,408` — four separate blocks (SQL dialect scan, RFC 0019
   dependency-scan, this session's `crate_topology_analyzer`/`cicd_analyzer` file collection) call
   `std::fs::read_to_string` directly, bypassing the artifact store entirely.

`crypto_analyzer.rs`'s name suggested a possible existing secret scanner — checked directly, it's
unrelated (RFC 0017 DeFi/cryptocurrency export data mapping). No secret-detection code existed
anywhere to reuse; `dependency_analyzer.rs`'s pattern-table shape (RFC 0019, `const PATTERNS:
&[(&str, &str)]`) was the template followed instead.

### What was built

| Component | Location |
|---|---|
| Redaction module | `ekos/crates/common/src/redaction.rs` (new) |
| `[security]` config | `ekos/crates/compiler-core/src/config.rs` (`SecurityConfig`, `SecretPatternConfig`) |
| Wiring | `ekos/crates/cli/src/commands/build.rs`, `ekos/crates/cli/src/commands/recover.rs` |
| RFC | `ekos/docs/rfcs/0043-secrets-and-pii-redaction.md` (new) |

`ekos_common::redaction` exposes `redact(content, config)`, `redact_json(value, config)` (recurses
through a `serde_json::Value` — needed since `ObservationContent.data` and harvested symbol lists
are JSON, not a single string), and `is_excluded_path(rel_path, config)`. A built-in, always-on
pattern table (compiled once via `OnceLock<Vec<(&str, Regex)>>`) covers AWS access key IDs, GitHub/
Slack/Google/Stripe token prefixes, PEM private-key blocks, JWTs, and a generic case-insensitive
`key/secret/password/token = value` assignment shape — each match becomes `[REDACTED:<label>]`. A
built-in excluded-file glob list (`.env`, `.env.*`, `*.pem`, `*.key`, `id_rsa*`, `id_ed25519*`,
`credentials(.json)`, `.npmrc`, `.netrc`, `.pgpass`, `*.jks`, `*.keystore`, …) causes those files to
be skipped entirely rather than redacted-and-kept, since they're near-100% secret material. `[security]`
in `ekos.toml` (`extra-patterns`, `extra-excluded-globs`) is additive-only — no `enabled` flag
exists anywhere, matching "global limitation, not opt-in feature."

Wired in exactly at the two entry points found: `build.rs` gets one central pass right after
`observer.scan()` returns (`package.artifacts.retain(...)` for exclusions, then `redact_json` over
each remaining artifact's `content.data`) — covering every connector automatically, including any
written after this RFC, without relying on a plugin author remembering to call a helper.
`recover.rs`'s four direct-read blocks each gained an `is_excluded_path` check before reading and a
`redact()` call immediately after.

### Decisions (alternatives considered, why this choice)

- **Central `build.rs` choke point over per-plugin redaction** — chosen specifically because it
  can't be forgotten by a future connector author; the one accepted tradeoff (documented explicitly
  in the RFC's Non-goals) is that each plugin's `ObservationArtifact::new(...)` already computed a
  content-addressed `ArtifactId` hash from the pre-redaction bytes by the time `build.rs`'s pass
  runs. The **data** is fully redacted before persistence; only an opaque hash technically derives
  from the original bytes — low severity, since a cryptographic hash doesn't reversibly leak its
  input (the same property a git blob SHA already has).
- **Fixed pattern table over entropy-based/statistical scanning** — matches
  `dependency_analyzer.rs`'s own "not exhaustive, cheap, transparent" precedent rather than scope
  creep into a heavier DLP-style approach for v1.
- **No `enabled = false` escape hatch** — rejected per explicit user direction.
- Structured, intentionally-modeled personal data connectors already extract on purpose — git
  commit author name/email (`git_analyzer.rs`, RFC 0007's `Person` object) — is **not** redacted;
  it's a deliberate, labeled property, not incidental leakage of raw text, and redacting it would
  break real contributor-attribution functionality. Redaction only touches raw free-text content
  fields (`excerpt`, `symbols`, file bodies read directly), never already-structured connector
  metadata.

---

## Knowledge Captured

- **The ledger's append-only invariant cuts both ways.** It's the project's core durability
  guarantee, but it also means a config-only fix (adding an ignore-pattern, fixing an
  over-aggressive identity merge) never retroactively cleans already-committed data — the *only*
  remedy is a full `.ekos/` wipe and rebuild from source. Any future "stop compiling X" change
  needs this same two-step: config fix + full rebuild, not just the config fix.
- **`WalkDir`'s `filter_entry` ignore-pattern matching in this codebase is directory-*name*
  equality, not a path-prefix or glob match** (`ekos/crates/observation-sdk/src/lib.rs:82,112`,
  `ekos/plugins/file/src/lib.rs:49`). Adding `"fixtures"` to `ignore-patterns` excludes *any*
  directory literally named `fixtures` anywhere in the tree — verified no false-positive
  collisions existed in this repo before relying on that behavior, since a bare `"fixtures"`
  pattern would have silently swallowed something unrelated on a repo where that name is reused
  for a different purpose.
- **A per-project `ekos.toml`/`.ekos` is the normal unit of EKOS usage**, not a special case — every
  real example in `~/PycharmProjects/*/ekos.toml` (`pih-pentaho`, `etl_adventureworks_...`, this
  session's new `azure-databricks-project`/`adf-pipelines` ones) follows the same `root = "."`,
  `paths = ["."]` template. A separate "estate" config at the parent directory compiling ~40
  sibling repos into one shared ledger also exists and is what backs the `demo/`'s cross-project
  "estate-scout" agent capability — a different, coarser unit of composition layered on top of the
  per-project default, not a replacement for it.
- **YAML block scalars matter for fixture content containing a literal `:`.** A test fixture's
  `run: curl -H "Authorization: AKIA..." https://...` broke `serde_yaml` parsing ("mapping values
  are not allowed in this context") because the inner `Authorization: ` colon-space sequence reads
  as a nested mapping key when the `run:` value isn't a block scalar (`run: |`) or fully quoted.
  Not an EKOS bug — a reminder that realistic-secret test fixtures need YAML-safe framing.

---

## Files Changed

| File | Change summary |
|---|---|
| `ekos/docs/rfcs/0043-secrets-and-pii-redaction.md` | New RFC |
| `ekos/crates/common/src/redaction.rs` | New: `RedactionConfig`, `redact`, `redact_json`, `is_excluded_path`, built-in tables + unit tests |
| `ekos/crates/common/src/lib.rs` | `+pub mod redaction;` |
| `ekos/crates/common/Cargo.toml`, `ekos/Cargo.toml` | `+regex`, `+glob` deps for `ekos-common` |
| `ekos/crates/compiler-core/src/config.rs` | `+SecurityConfig`, `+SecretPatternConfig`, `+EkosConfig.security`, `+EkosConfig::redaction_config()` |
| `ekos/crates/cli/src/commands/build.rs` | Central redaction pass right after `observer.scan()` |
| `ekos/crates/cli/src/commands/recover.rs` | Exclusion check + redaction at all four direct-read blocks |
| `ekos/crates/cli/tests/skeleton.rs` | 3 new integration tests: excerpt redaction, `.env` exclusion, CI/CD workflow step redaction |
| `README.md` | `+` redaction bullet under Key Invariants |
