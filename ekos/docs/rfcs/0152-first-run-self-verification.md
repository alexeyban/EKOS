# RFC 0152 — First-Run Self-Verification: Workspace Detection and Coverage Reporting

**Status:** Accepted
**Date:** 2026-09-23
**Depends on:** RFC 0002 (artifact system), RFC 0003 (KIR — `KirEvidence`/`SourceLocation`),
RFC 0031 (`SqlDialectParser` — the dialect names this detects), RFC 0043 (redaction — the
exclusion list detection must not fight), RFC 0076 (diagnostics log — the existing
"where did the warnings go" precedent), RFC 0117 (dbt analyzer — a detected project kind),
RFC 0135 Part B (`source_artifact_ids`), RFC 0140 §1 (`EvidenceRecord.line`), RFC 0146
(Postgres dialect coverage — the incident that motivates dialect detection)

---

## 1. Motivation

`ekos init` is 38 lines. It creates four directories and writes a seven-line `ekos.toml`:

```toml
[workspace]
root = "."
log-level = "info"
log-format = "pretty"

[observe]
paths = ["."]
ignore-patterns = [".ekos", ".git", "target", "node_modules"]
```

No SQL dialect. No knowledge of what is actually in the repository. `ekos doctor` then checks
the Rust toolchain, the LLM API key and whether the config file exists — it never checks whether
the compiler **found anything**.

The result is a well-documented failure class that this project has now hit five separate times
on real codebases. In every case the pipeline exited `0` and printed a cheerful summary:

| Incident | Symptom | Real cause |
|---|---|---|
| `analytics` (devlog_177, commit 9e097a5) | 0 `Table` objects for a repo with a real schema | no `[recover.sql]` dialect rule; whole-file parse failed, one buried `SQL001` |
| LedgerSMB (RFC 0146) | 0 of 103 tables, on the **correct** `postgres` dialect | `parse_ddl_structural` is whole-file all-or-nothing; `COMMENT ON … IS $$…$$` and `INHERITS` killed the parse |
| git observer (memory: `observe-paths-kills-git-observer`) | 0 commits, silently | `paths` listed subdirectories; `GitObserver` only ever checks `root/.git` |
| first real .NET app (devlog_190) | calls joined 0 of 36k | format-specific join defect, invisible because nothing reported per-format edge counts |
| this repo's own `ekos.toml` | 50.9% of all compiled objects described numpy/scipy/pytest internals | `.venv` was not in `ignore-patterns`; `.gitignore` does not exclude anything from the observation walk |

The last row is the mirror image of the others and matters just as much: the failure is not only
"zero objects", it is "the wrong objects", and both are silent. This repository's own
`ignore-patterns` list is now ~20 entries long, and **every single one was discovered by a
production incident**, each documented in a multi-line comment above it. A new user starts from
the four-entry default and gets to rediscover all of them.

Two facts make this the highest-value work available:

1. Every one of these is a *first-run* failure. A user's first ten minutes decide whether they
   ever run EKOS again, and right now those ten minutes can silently produce nothing — or worse,
   produce a knowledge model that is majority third-party noise.
2. The information needed to catch all of it is **already in the workspace**. The file tree says
   what source kinds exist. The CKM says what was compiled. Nothing joins them.

## 2. Goals / Non-goals

**Goals**

- `ekos init --detect` writes an `ekos.toml` that reflects what the repository actually contains:
  the SQL dialect its schema is written in, the ignore patterns its build tooling requires, and a
  named inventory of the source kinds found.
- A **coverage report** that joins *inputs present* against *objects compiled*, per source kind,
  and treats `files > 0 && objects == 0` as a failure with a named likely cause — never as
  silence.
- The coverage check is reachable three ways: a standalone `ekos coverage`, an automatic summary
  at the end of `ekos compile`, and a `doctor` check.
- A non-zero exit code available (`--strict`) so CI and the RFC 0149 extension builds can gate on
  it.
- Detection and coverage share **one** `SourceKind` vocabulary, so "what is present" and "what
  was compiled" are keyed identically and can actually be joined.

**Non-goals**

- Changing any analyzer. This RFC adds no recovery capability; it reports on what recovery did.
  Fixing `parse_ddl_structural`'s all-or-nothing behaviour is RFC 0146's lineage, not this one.
- Auto-enabling credentialed connectors (GitHub, Confluence, ClickHouse, treasury). Detection
  *suggests* them as commented-out stanzas; it never writes a live credential reference. Turning
  on a connector that makes network calls stays an explicit human act.
- A quality score. Coverage answers "did this kind produce anything", not "is the output good".
  RFC 0065/0095's `ArchitectureConfidence` already owns evaluative scoring, and this must not
  grow into a second, competing one.
- Threshold-based "partial coverage" verdicts — see §6.

## 3. Interfaces

Both halves live in the `cli` crate as library modules (`crates/cli/src/detect.rs`,
`crates/cli/src/coverage.rs`) rather than a new crate: both read `EkosConfig` and the CKM, both
are CLI-surface concerns, and neither is consumed by a compiler pass. The computational core of
each is a pure function over data, so the tests never touch a filesystem.

### 3.1 The shared vocabulary

```rust
/// A kind of thing EKOS can observe and recover, named identically on both the
/// "what is in this workspace" and "what did the compiler produce" sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    Sql, Git, Rust, Python, JavaScript, Elixir, Perl,
    Pentaho, Dbt, Documents, CargoManifest, PackageJson, CiCd,
}

impl SourceKind {
    /// Every kind, in report order. Exhaustive by construction: a `match` in
    /// `label()` makes a new variant a compile error here too.
    pub fn all() -> &'static [SourceKind];
    /// Human label used in both the detection inventory and the coverage table.
    pub fn label(self) -> &'static str;
    /// Does this workspace-relative path count as an input of this kind?
    pub fn matches_path(self, path: &str) -> bool;
    /// What a human should check first when this kind has inputs but compiled
    /// nothing. One sentence, names a real config key or a real known defect.
    pub fn zero_coverage_hint(self) -> &'static str;
}
```

`matches_path` is the single definition of "a Python file", used by detection to count inputs and
by coverage to attribute an evidence path back to a kind. One definition, two directions.

### 3.2 Detection

```rust
pub struct Detection {
    /// Source kinds with at least one real input file, with counts.
    pub sources: Vec<DetectedSource>,
    /// Directories present in the tree that must not be observed, with the
    /// reason each is excluded (carried into the generated config as a comment).
    pub contaminants: Vec<Contaminant>,
    /// The dialect the sampled SQL looks like, and the evidence for that guess.
    pub sql_dialect: Option<DialectGuess>,
    /// Things a human has to decide — a detected GitHub remote, a Confluence
    /// export, a dbt profile pointing at a live warehouse.
    pub suggestions: Vec<Suggestion>,
}

pub struct DetectedSource { pub kind: SourceKind, pub file_count: usize, pub sample_paths: Vec<String> }
pub struct Contaminant   { pub pattern: String, pub reason: &'static str, pub observed_files: usize }
pub struct DialectGuess  { pub dialect: String, pub markers: Vec<DialectMarker>, pub sampled_files: usize }
pub struct DialectMarker { pub marker: &'static str, pub hits: usize }

/// Pure: classify a listing. No I/O, no clock.
pub fn detect_sources(paths: &[String]) -> Vec<DetectedSource>;
/// Pure: which known-contaminating directories this listing contains.
pub fn detect_contaminants(paths: &[String]) -> Vec<Contaminant>;
/// Pure: score dialect markers across sampled SQL text.
pub fn guess_dialect(samples: &[(String, String)]) -> Option<DialectGuess>;

/// The one function that touches the filesystem: walk, then delegate to the above.
pub fn detect_workspace(cwd: &Path, config: &EkosConfig) -> Result<Detection>;

/// Render a `Detection` as a complete, commented `ekos.toml`.
pub fn render_config(detection: &Detection) -> String;
```

### 3.3 Coverage

```rust
pub struct CoverageReport {
    pub rows: Vec<CoverageRow>,
    pub total_objects: usize,
    pub total_relationships: usize,
    /// Objects whose evidence names no path this build recognises. Reported as
    /// a footnote, never hidden — an unattributable object is itself a signal.
    pub unattributed_objects: usize,
}

pub struct CoverageRow {
    pub kind: SourceKind,
    pub files_present: usize,
    pub objects_produced: usize,
    pub relationships_produced: usize,
    pub status: CoverageStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    /// Inputs present, objects produced. Normal.
    Ok,
    /// Inputs present, objects produced, but zero relationships — the
    /// devlog_190 ".NET calls joined 0 of 36k" shape.
    NoEdges,
    /// Inputs present, nothing compiled. The failure this RFC exists for.
    ZeroCoverage,
    /// No inputs of this kind. Not a finding; hidden unless `--all`.
    NoInput,
}

/// Pure: join a detection against a compiled model.
pub fn compute_coverage(detection: &Detection, model: &CkModel) -> CoverageReport;

impl CoverageReport {
    /// Rows that represent a real finding, worst first.
    pub fn findings(&self) -> Vec<&CoverageRow>;
    /// True if any row is `ZeroCoverage` — what `--strict` exits non-zero on.
    pub fn has_zero_coverage(&self) -> bool;
}
```

**Attribution rule.** For each `CkmObject`, every `EvidenceRecord.source` path is classified with
`SourceKind::matches_path`; the object is counted toward each distinct kind it has evidence from.
An object compiled from two kinds (a `Table` recovered from both DDL and a dbt model — exactly
what RFC 0117's identity fusion is designed to produce) counts once in each row, so **rows may sum
to more than `total_objects`**. The report states this in its own footer rather than picking an
arbitrary primary kind and under-reporting the other. Objects with no evidence at all (a
concentration `Risk` synthesized in `compile`, RFC 0094) are counted in `unattributed_objects`.

## 4. CLI surface

```
ekos init --detect [--dry-run] [--force]
ekos coverage [--json] [--strict] [--all]
```

`ekos init --detect` refuses to overwrite an existing `ekos.toml` unless `--force`, and
`--dry-run` prints the config it would write without writing it. Plain `ekos init` is unchanged
and still writes the seven-line default — detection is opt-in, because a detected config makes
real claims about a repository and a user must be able to get the inert one.

`ekos compile` gains an automatic tail: nothing at all when every row is `Ok`, and otherwise one
line per finding plus a pointer to `ekos coverage`. Compile's exit code is unchanged — a silent
pipeline is the bug being fixed, not a pipeline that stops.

`ekos doctor` gains a `coverage` check that reads the CKM when one exists and reports the finding
count. Absent CKM is `ok` with "not compiled yet", not a failure: `doctor` is routinely run
before the first build.

## 5. Generated config

`render_config` emits the four-entry baseline plus every detected contaminant, each with the
reason inline — the same shape this repository's own `ekos.toml` reached through five incidents,
now available on the first run:

```toml
[observe]
# Always ".", never a list of subdirectories: GitObserver only looks for `root/.git`, so
# listing subdirectories here silently yields zero commits with no error.
paths = ["."]
ignore-patterns = [
    ".ekos", ".git", "target", "node_modules",
    # 3758 Python files under .venv/ — third-party library code, not this project's.
    # Note: .gitignore does NOT exclude anything from the observation walk.
    ".venv",
    ...
]

# Detected from 47 sampled .sql files: SERIAL (31), $$ (12), RETURNING (8), INHERITS (2).
# A wrong dialect makes the whole-file DDL parse fail and the entire schema disappear with
# only a buried SQL001 — check this line first if `ekos coverage` reports 0 tables.
[recover.sql]
default-dialect = "postgres"
```

The dialect comment naming its own markers is deliberate: the LedgerSMB incident proved the
dialect can be *right* and the parse still fail, so the generated config has to point at the
verification step rather than imply the problem is solved.

## 6. Alternatives considered

| Alternative | Verdict |
|---|---|
| A percentage/threshold "partial coverage" verdict (e.g. "only 40% of SQL files produced tables — warn") | **Rejected.** RFC 0060 established this project's position on confidence thresholds: no threshold reliably separated correct from incorrect fuzzy identity merges, and the answer was to stop guessing and surface the candidate instead. The same reasoning applies — there is no defensible number at which "some tables missing" becomes a warning, and a wrong threshold trains users to ignore the report. Zero is the one unambiguous, non-arbitrary signal, and it is the one every real incident actually produced. |
| Attribute objects via `source_artifact_ids` (RFC 0135 Part B) instead of evidence paths | Rejected as the primary mechanism. It is the more *precise* provenance link, but it is empty for pre-0135 models and for objects synthesized in `compile`, and it names an artifact id rather than a path — so mapping back to a source kind needs an extra artifact-store round trip per object. Evidence paths are present on every object that has any provenance at all, and RFC 0140 §1 already guarantees they carry real locations. |
| Make detection a compiler pass | Rejected. Passes must be deterministic and side-effect-free over a `PassContext`; detection writes a config file *before* any pass context exists, and coverage reads a CKM *after* the pass pipeline has finished. Neither is a pass. |
| Put `SourceKind` in `kir` so analyzers could report their own coverage | Rejected for now. It would be the more principled home, but it makes every analyzer crate responsible for a reporting concern, and the evidence-path join already works without touching them. Revisit if a future analyzer needs to declare coverage the file tree cannot infer. |
| Fail `ekos compile` on zero coverage | Rejected. A workspace can legitimately contain one `.sql` file that is a migration fragment producing no tables. The default is loud, not fatal; `--strict` exists for the caller who wants fatal. |

## 6a. Two corrections the first real run forced

Recorded here rather than silently fixed, because both were cases of this RFC committing the
error it exists to prevent.

1. **`NoEdges` fired on kinds with a single object.** A repository with one commit and one Python
   class produced two "0 relationships" findings, which is meaningless: an edge needs two
   endpoints. The rule is now `objects >= 2`, which is structural rather than a tuned threshold —
   there is no number here to get wrong.
2. **The `NoEdges` finding printed the zero-coverage explanation.** Next to "1 object compiled",
   the git hint read *"yields zero commits with no error at all"* — describing a failure that had
   not happened. `NoEdges` now has its own text. A report that misdescribes what it found is
   worse than no report.

A third, smaller one: a capped enumeration must not print as a total. This repository has three
`target/` directories, each stopping at the 20,000-file cap, and the first version reported
"60000 files" — a number nothing had counted. Capped counts now render as `20000+`.

## 7. Acceptance criteria

1. `SourceKind::matches_path` has a unit test per variant, including at least one negative case
   per variant — the `headless.sh` filter bug (memory: `feedback-filter-self-comparison-bug`) was
   a filter that was never tested against a case it should reject.
2. `guess_dialect` correctly identifies `postgres` on the LedgerSMB core schema markers
   (`SERIAL`, `$$`, `INHERITS`, `COMMENT ON`) and `mysql` on `AUTO_INCREMENT`/`ENGINE=`, from
   fixture text, with no filesystem access.
3. `detect_contaminants` finds `.venv`, `site-packages`, `.scannerwork`, `test-runs` and
   `node_modules` in a synthetic listing, and does **not** flag a real source directory named
   `build/` (the generic-name trap this repository's own config comment documents).
4. `compute_coverage` on a model with SQL evidence and no Python evidence, against a detection
   with both kinds present, yields `Ok` for `Sql` and `ZeroCoverage` for `Python`.
5. `compute_coverage` yields `NoEdges` for a kind with objects but no relationships.
6. Rows summing above `total_objects` is asserted, not merely tolerated, by a test with an object
   carrying evidence from two kinds.
7. `ekos coverage --strict` exits non-zero when any row is `ZeroCoverage`, zero otherwise.
8. `ekos init --detect` on this repository completes in seconds despite a 142GB `target/` tree,
   and reports Rust, Python, JavaScript, documents, Cargo manifests, `package.json` and CI/CD
   inputs plus the `.venv`/`site-packages`/`.scannerwork`/`__pycache__` contaminants. (Measured
   2026-09-23: 3.0s. It reports **no** SQL, Perl or dbt inputs — this repository's own
   `ignore-patterns` excludes `fixtures/`, and it has no Perl or dbt of its own. An earlier draft
   of this criterion asserted Perl and dbt would be found; that was a guess and it was wrong.)
9. `ekos init --detect` refuses to overwrite an existing `ekos.toml` without `--force`.
10. The generated config round-trips: `render_config`'s output parses as a valid `EkosConfig`.
