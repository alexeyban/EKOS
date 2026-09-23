//! RFC 0152 — workspace detection: what is actually in this repository.
//!
//! `ekos init` writes a seven-line `ekos.toml` that knows nothing about the repository it is
//! initializing. That has produced the same silent first-run failure five times on real
//! codebases — a missing SQL dialect rule that made an entire 103-table schema vanish behind one
//! buried `SQL001` (devlog_177, RFC 0146), an `[observe] paths` list of subdirectories that made
//! `GitObserver` find zero commits without an error, and a missing `.venv` exclusion that made
//! 50.9% of this project's own compiled objects describe numpy and scipy internals.
//!
//! Every one of those is knowable from the file tree before the first `ekos build` runs. This
//! module reads the tree and says what it found; [`crate::coverage`] later checks what the
//! compiler did with it, using the same [`SourceKind`] vocabulary so the two can be joined.
//!
//! The computational core is pure: [`detect_sources`], [`detect_contaminants`] and
//! [`guess_dialect`] take listings and text, never a filesystem. [`detect_workspace`] is the one
//! function that walks a directory.

use anyhow::Result;
use ekos_compiler_core::EkosConfig;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use walkdir::WalkDir;

/// A kind of thing EKOS can observe and recover.
///
/// Named identically on both the "what is in this workspace" side ([`detect_sources`]) and the
/// "what did the compiler produce" side ([`crate::coverage`]), because the whole point is to
/// join them: inputs present against objects compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SourceKind {
    Sql,
    Git,
    Rust,
    Python,
    JavaScript,
    Elixir,
    Perl,
    Pentaho,
    Dbt,
    Documents,
    CargoManifest,
    PackageJson,
    CiCd,
}

impl SourceKind {
    /// Every kind, in report order.
    pub fn all() -> &'static [SourceKind] {
        use SourceKind::*;
        &[
            Sql,
            Dbt,
            Git,
            Rust,
            Python,
            JavaScript,
            Elixir,
            Perl,
            Pentaho,
            Documents,
            CargoManifest,
            PackageJson,
            CiCd,
        ]
    }

    /// Human label, used in both the detection inventory and the coverage table.
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Sql => "SQL",
            SourceKind::Git => "Git history",
            SourceKind::Rust => "Rust",
            SourceKind::Python => "Python",
            SourceKind::JavaScript => "JavaScript/TypeScript",
            SourceKind::Elixir => "Elixir",
            SourceKind::Perl => "Perl",
            SourceKind::Pentaho => "Pentaho",
            SourceKind::Dbt => "dbt",
            SourceKind::Documents => "Documents",
            SourceKind::CargoManifest => "Cargo manifests",
            SourceKind::PackageJson => "package.json",
            SourceKind::CiCd => "CI/CD workflows",
        }
    }

    /// What to check first when this kind has inputs but compiled nothing.
    ///
    /// One sentence per kind, each naming a real config key or a real, documented defect — not
    /// generic advice. A hint that says "check your configuration" is the same silence this
    /// module exists to end.
    pub fn zero_coverage_hint(self) -> &'static str {
        match self {
            SourceKind::Sql => {
                "check `[recover.sql] default-dialect` and `dialect-rules` in ekos.toml — a wrong \
                 dialect fails the whole-file DDL parse and the entire schema disappears behind \
                 one SQL001 diagnostic. A *correct* dialect is not sufficient: unsupported \
                 syntax (`COMMENT ON ... IS $$...$$`, `INHERITS`) fails the same all-or-nothing \
                 way. See .ekos/diagnostics/recover.log"
            }
            SourceKind::Git => {
                "`[observe] paths` must be `[\".\"]` — GitObserver only looks for a `.git` \
                 directory at the workspace root, so listing subdirectories there yields zero \
                 commits with no error at all"
            }
            SourceKind::Rust
            | SourceKind::Python
            | SourceKind::JavaScript
            | SourceKind::Elixir
            | SourceKind::Perl => {
                "the analyzer for this language runs unconditionally, so zero objects means the \
                 files were excluded before it ran (check `[observe] ignore-patterns` and \
                 `[security] extra-excluded-globs`) or every file failed to parse — see \
                 .ekos/diagnostics/recover.log"
            }
            SourceKind::Pentaho => {
                "Pentaho `.ktr`/`.kjb` files are XML — a zero result usually means the files are \
                 a format version the parser does not recognise; see \
                 .ekos/diagnostics/recover.log"
            }
            SourceKind::Dbt => {
                "dbt recovery reads the project's own checked-in `models/**/*.sql` and \
                 `schema.yml`/`sources.yml` — never `manifest.json`, which is a gitignored build \
                 artifact. Zero objects means the models directory was empty, excluded, or not \
                 under the directory holding `dbt_project.yml`"
            }
            SourceKind::Documents => {
                "local document recovery needs readable text — a zero result on PDFs usually \
                 means they are scanned images and `tesseract` is not on PATH (OCR degrades \
                 silently to no text)"
            }
            SourceKind::CargoManifest => {
                "Cargo manifests compile to `Crate` objects via crate_topology_analyzer; zero \
                 means the manifests were excluded by `[observe] ignore-patterns`"
            }
            SourceKind::PackageJson => {
                "package.json manifests compile to `Technology` dependency objects; zero means \
                 they were excluded by `[observe] ignore-patterns`"
            }
            SourceKind::CiCd => {
                "CI/CD recovery reads `.github/workflows/*.yml`; zero means the directory was \
                 excluded by `[observe] ignore-patterns`"
            }
        }
    }

    /// Does this workspace-relative path count as an input of this kind, ignoring context?
    ///
    /// Context-free on purpose: [`Classifier`] layers the one real exception (a `.sql` file
    /// inside a dbt project belongs to dbt, not to hand-written DDL) on top of this. Both
    /// detection and coverage go through the classifier, so "a Python file" has exactly one
    /// definition and the two sides cannot drift apart.
    pub fn matches_path(self, path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        let file_name = lower.rsplit('/').next().unwrap_or(&lower);
        let ext = file_name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");

        match self {
            // `git:commit:<sha>` / `git:contributors` — git_analyzer's evidence names a
            // pseudo-path, not a file on disk, because a commit is not a file.
            SourceKind::Git => lower.starts_with("git:"),
            SourceKind::Sql => ext == "sql",
            SourceKind::Rust => ext == "rs",
            SourceKind::Python => ext == "py",
            SourceKind::JavaScript => {
                matches!(
                    ext,
                    "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts"
                )
            }
            SourceKind::Elixir => matches!(ext, "ex" | "exs"),
            // `.cgi` is deliberately absent: the Perl observer accepts it only with a real
            // `perl` shebang, which a path cannot tell us. Counting it here would report an
            // input that never reaches the analyzer.
            SourceKind::Perl => matches!(ext, "pl" | "pm" | "psgi") || ext == "t",
            SourceKind::Pentaho => matches!(ext, "ktr" | "kjb"),
            SourceKind::Dbt => file_name == "dbt_project.yml",
            SourceKind::Documents => {
                matches!(ext, "pdf" | "docx" | "md" | "txt" | "html" | "htm" | "eml")
            }
            SourceKind::CargoManifest => file_name == "cargo.toml",
            SourceKind::PackageJson => file_name == "package.json",
            SourceKind::CiCd => {
                lower.starts_with(".github/workflows/") && matches!(ext, "yml" | "yaml")
            }
        }
    }
}

/// Path → [`SourceKind`], with the dbt exception applied.
///
/// A dbt model is a `.sql` file. Without this, a dbt-only workspace whose hand-written DDL
/// recovery produced nothing would still show SQL as healthy — exactly the false negative this
/// whole RFC exists to prevent — because the dbt models' own evidence would be counted as SQL
/// coverage.
#[derive(Debug, Clone, Default)]
pub struct Classifier {
    /// Directories containing a `dbt_project.yml`, normalized with a trailing `/`.
    dbt_roots: Vec<String>,
}

impl Classifier {
    pub fn new(dbt_roots: Vec<String>) -> Self {
        Self { dbt_roots }
    }

    /// Derive a classifier from a listing: any directory holding `dbt_project.yml` is a dbt root.
    pub fn from_listing(paths: &[String]) -> Self {
        let roots = paths
            .iter()
            .map(|p| normalize(p))
            .filter(|p| SourceKind::Dbt.matches_path(p))
            .map(|p| match p.rsplit_once('/') {
                Some((dir, _)) => format!("{dir}/"),
                // `dbt_project.yml` at the workspace root: every path is inside it.
                None => String::new(),
            })
            .collect();
        Self::new(roots)
    }

    fn inside_dbt_project(&self, path: &str) -> bool {
        self.dbt_roots
            .iter()
            .any(|root| root.is_empty() || path.starts_with(root.as_str()))
    }

    /// The single kind this path belongs to, or `None` for a path EKOS does not recover from.
    pub fn classify(&self, path: &str) -> Option<SourceKind> {
        let path = normalize(path);
        if SourceKind::Dbt.matches_path(&path) {
            return Some(SourceKind::Dbt);
        }
        if SourceKind::Sql.matches_path(&path) && self.inside_dbt_project(&path) {
            return Some(SourceKind::Dbt);
        }
        SourceKind::all()
            .iter()
            .copied()
            .find(|k| k.matches_path(&path))
    }
}

/// Strip a leading `./` and normalize separators, so a Windows-observed path and a
/// `WalkDir`-produced one classify identically.
fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_string()
}

/// A source kind with at least one real input in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedSource {
    pub kind: SourceKind,
    /// Number of input files. For [`SourceKind::Git`] this is the number of repositories (0 or 1).
    pub file_count: usize,
    /// Up to three real paths, so the inventory can show its work.
    pub sample_paths: Vec<String>,
}

/// A directory present in the tree that must not be observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contaminant {
    pub pattern: &'static str,
    pub reason: &'static str,
    pub observed_files: usize,
    /// The enumeration stopped at [`CONTAMINANT_COUNT_CAP`], so `observed_files` is a floor, not
    /// a total. Rendered as `20000+` rather than as a precise-looking number this did not count.
    pub capped: bool,
}

/// The dialect the sampled SQL looks like, with the evidence for the guess.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectGuess {
    pub dialect: &'static str,
    pub markers: Vec<DialectMarker>,
    pub sampled_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialectMarker {
    pub marker: &'static str,
    pub hits: usize,
}

/// Something a human has to decide — never written as live configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub headline: String,
    pub detail: String,
}

/// Everything a scan of the workspace found.
#[derive(Debug, Clone, Default)]
pub struct Detection {
    pub sources: Vec<DetectedSource>,
    pub contaminants: Vec<Contaminant>,
    pub sql_dialect: Option<DialectGuess>,
    pub suggestions: Vec<Suggestion>,
}

impl Detection {
    /// Input count for one kind, 0 when the kind was not found.
    pub fn file_count(&self, kind: SourceKind) -> usize {
        self.sources
            .iter()
            .find(|s| s.kind == kind)
            .map_or(0, |s| s.file_count)
    }

    /// The dbt roots found, for building a [`Classifier`] that matches this detection.
    pub fn classifier(&self) -> Classifier {
        let dbt_paths: Vec<String> = self
            .sources
            .iter()
            .filter(|s| s.kind == SourceKind::Dbt)
            .flat_map(|s| s.sample_paths.clone())
            .collect();
        Classifier::from_listing(&dbt_paths)
    }
}

/// Directories whose contents are never this project's own knowledge.
///
/// Each entry earned its place by contaminating a real ledger. Deliberately absent: the generic
/// names `build`, `dist`, `out` and `coverage` — patterns match a bare path component, so those
/// would prune real source directories in somebody's workspace. A false exclusion is worse than
/// a false inclusion here, because it is invisible.
const KNOWN_CONTAMINANTS: &[(&str, &str)] = &[
    (
        ".venv",
        "third-party Python libraries, not this project's source. Note that .gitignore excludes \
         nothing from EKOS's observation walk",
    ),
    ("venv", "as .venv"),
    (
        "site-packages",
        "installed third-party Python packages, wherever they live",
    ),
    ("node_modules", "third-party JavaScript packages"),
    ("bower_components", "third-party JavaScript packages"),
    ("vendor", "vendored third-party source"),
    (
        "dbt_packages",
        "third-party dbt models installed by `dbt deps`",
    ),
    ("target", "Rust build output"),
    ("__pycache__", "Python bytecode cache"),
    (".pytest_cache", "test-runner cache"),
    (".mypy_cache", "type-checker cache"),
    (".ruff_cache", "linter cache"),
    (
        ".tox",
        "per-environment test installs, each a full dependency tree",
    ),
    (
        ".scannerwork",
        "SonarCloud scanner output — compiles to almost nothing but writes one File object per \
         file, drowning real code in every retrieval",
    ),
    (".next", "Next.js build output"),
    (".nuxt", "Nuxt build output"),
    (".terraform", "downloaded Terraform providers and modules"),
    (".gradle", "Gradle build cache"),
    ("Pods", "third-party CocoaPods source"),
];

/// Dialect markers, strongest signal first within each dialect.
///
/// Scored by how many sampled files contain each marker, not by total occurrences: one file
/// using `SERIAL` forty times is weaker evidence than four files using it once.
const DIALECT_MARKERS: &[(&str, &[&str])] = &[
    (
        "postgres",
        &["serial", "::", "$$", "returning", "inherits", "comment on"],
    ),
    ("mysql", &["auto_increment", "engine=", "`"]),
    (
        "mssql",
        &["[dbo]", "nvarchar", "identity(", "go\n", "getdate()"],
    ),
    ("oracle", &["varchar2", "number(", "nvl(", "dual"]),
    ("snowflake", &["variant", "copy into", "cluster by"]),
    ("clickhouse", &["mergetree", "nullable(", "low_cardinality"]),
    ("databricks", &["using delta", "zorder", "optimize "]),
];

/// Pure: classify a listing into the source kinds it contains.
pub fn detect_sources(paths: &[String]) -> Vec<DetectedSource> {
    let classifier = Classifier::from_listing(paths);
    let mut counts: BTreeMap<SourceKind, (usize, Vec<String>)> = BTreeMap::new();

    for raw in paths {
        let path = normalize(raw);
        let Some(kind) = classifier.classify(&path) else {
            continue;
        };
        let entry = counts.entry(kind).or_insert((0, Vec::new()));
        entry.0 += 1;
        if entry.1.len() < 3 {
            entry.1.push(path);
        }
    }

    SourceKind::all()
        .iter()
        .filter_map(|kind| {
            counts.get(kind).map(|(count, samples)| DetectedSource {
                kind: *kind,
                file_count: *count,
                sample_paths: samples.clone(),
            })
        })
        .collect()
}

/// Pure: which known-contaminating directories this listing actually contains.
pub fn detect_contaminants(paths: &[String]) -> Vec<Contaminant> {
    let mut hits: BTreeMap<&'static str, usize> = BTreeMap::new();

    for raw in paths {
        let path = normalize(raw);
        let components: BTreeSet<&str> = path.split('/').collect();
        for (pattern, _) in KNOWN_CONTAMINANTS {
            if components.contains(pattern) {
                *hits.entry(pattern).or_insert(0) += 1;
            }
        }
    }

    KNOWN_CONTAMINANTS
        .iter()
        .filter_map(|(pattern, reason)| {
            hits.get(pattern).map(|observed_files| Contaminant {
                pattern,
                reason,
                observed_files: *observed_files,
                capped: false,
            })
        })
        .collect()
}

/// Pure: score dialect markers across sampled `(path, text)` SQL.
///
/// Returns `None` when nothing scores — the caller then writes `generic`, which is the honest
/// answer rather than a coin flip between two dialects that tied.
pub fn guess_dialect(samples: &[(String, String)]) -> Option<DialectGuess> {
    if samples.is_empty() {
        return None;
    }

    let lowered: Vec<String> = samples
        .iter()
        .map(|(_, text)| text.to_ascii_lowercase())
        .collect();

    let mut best: Option<DialectGuess> = None;
    let mut best_score = 0usize;
    let mut tied = false;

    for (dialect, markers) in DIALECT_MARKERS {
        let hits: Vec<DialectMarker> = markers
            .iter()
            .filter_map(|marker| {
                let files = lowered.iter().filter(|t| t.contains(marker)).count();
                (files > 0).then_some(DialectMarker {
                    marker,
                    hits: files,
                })
            })
            .collect();
        let score: usize = hits.iter().map(|h| h.hits).sum();
        if score == 0 {
            continue;
        }
        if score > best_score {
            best_score = score;
            tied = false;
            best = Some(DialectGuess {
                dialect,
                markers: hits,
                sampled_files: samples.len(),
            });
        } else if score == best_score {
            tied = true;
        }
    }

    // A tie is not a guess. Saying "generic" and pointing at `ekos coverage` beats picking one
    // of two dialects and being confidently wrong about a whole schema.
    if tied { None } else { best }
}

/// How many `.sql` files to read when guessing a dialect.
const DIALECT_SAMPLE_LIMIT: usize = 50;

/// How many files to enumerate inside one excluded directory before stopping.
///
/// The count is evidence for the exclusion ("3,758 files under .venv"), so it is worth
/// gathering — but this repository's own `target/` holds millions of files and a full walk of it
/// would make `init --detect` appear to hang. The walk prunes at every excluded directory and
/// then counts each one separately, up to this cap.
const CONTAMINANT_COUNT_CAP: usize = 20_000;

/// Walk `cwd` and detect everything. The only function here that touches the filesystem.
pub fn detect_workspace(cwd: &Path, config: &EkosConfig) -> Result<Detection> {
    let ignore: BTreeSet<&str> = config
        .observe
        .ignore_patterns
        .iter()
        .map(String::as_str)
        .collect();
    let contaminant_names: BTreeSet<&str> = KNOWN_CONTAMINANTS.iter().map(|(p, _)| *p).collect();

    // Directories the walk refused to descend into, recorded by the filter itself — the only
    // way to learn about a subtree that was (deliberately) never enumerated.
    let pruned: RefCell<Vec<std::path::PathBuf>> = RefCell::new(Vec::new());

    let mut clean: Vec<String> = Vec::new();
    for entry in WalkDir::new(cwd)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            if !e.file_type().is_dir() {
                return true;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name == ".ekos" || name == ".git" {
                return false;
            }
            if contaminant_names.contains(name.as_str()) {
                pruned.borrow_mut().push(e.path().to_path_buf());
                return false;
            }
            // Honour what the caller's config already excludes, so re-running detection on a
            // configured workspace does not re-walk what that config exists to skip.
            !ignore.contains(name.as_str())
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(cwd) else {
            continue;
        };
        let rel = normalize(&rel.to_string_lossy());
        if !rel.is_empty() {
            clean.push(rel);
        }
    }

    // Count what each pruned subtree holds, capped. `WalkDir` is lazy, so `.take()` genuinely
    // stops walking rather than enumerating everything and discarding it.
    let mut contaminant_paths: Vec<String> = Vec::new();
    let mut capped_patterns: BTreeSet<String> = BTreeSet::new();
    for root in pruned.borrow().iter() {
        let before = contaminant_paths.len();
        contaminant_paths.extend(
            WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
                .take(CONTAMINANT_COUNT_CAP)
                .filter_map(|e| {
                    e.path()
                        .strip_prefix(cwd)
                        .ok()
                        .map(|r| normalize(&r.to_string_lossy()))
                }),
        );
        if contaminant_paths.len() - before >= CONTAMINANT_COUNT_CAP {
            capped_patterns.insert(
                root.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
            );
        }
    }

    let mut contaminants = detect_contaminants(&contaminant_paths);
    for c in &mut contaminants {
        // A capped count must never be printed as if it were a total — `target` reporting
        // "60000 files" when three separate roots each stopped at the 20,000 cap would be a
        // fabricated number, which is exactly what this whole feature exists to stop.
        c.capped = capped_patterns.contains(c.pattern);
    }
    let mut sources = detect_sources(&clean);

    if cwd.join(".git").is_dir() {
        sources.insert(
            0,
            DetectedSource {
                kind: SourceKind::Git,
                file_count: 1,
                sample_paths: vec![".git".to_string()],
            },
        );
    }

    let classifier = Classifier::from_listing(&clean);
    let mut sql_samples: Vec<(String, String)> = Vec::new();
    for path in &clean {
        if sql_samples.len() >= DIALECT_SAMPLE_LIMIT {
            break;
        }
        if classifier.classify(path) != Some(SourceKind::Sql) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(cwd.join(path)) {
            sql_samples.push((path.clone(), text));
        }
    }

    let sql_dialect = guess_dialect(&sql_samples);
    let suggestions = build_suggestions(cwd, &sources, &sql_samples);

    Ok(Detection {
        sources,
        contaminants,
        sql_dialect,
        suggestions,
    })
}

/// Things detection noticed but must not act on by itself.
fn build_suggestions(
    cwd: &Path,
    sources: &[DetectedSource],
    sql_samples: &[(String, String)],
) -> Vec<Suggestion> {
    let mut out = Vec::new();

    if let Some(remote) = github_remote(cwd) {
        out.push(Suggestion {
            headline: format!("GitHub remote detected ({remote})"),
            detail: "`[github]` in ekos.toml enables issue/PR recovery. Left commented out: it \
                     makes live API calls and needs a token, so turning it on stays an explicit \
                     act."
                .into(),
        });
    }

    if sources.iter().any(|s| s.kind == SourceKind::Sql) && sql_samples.is_empty() {
        out.push(Suggestion {
            headline: "SQL files found but none could be read".into(),
            detail: "The dialect was not guessed. Set `[recover.sql] default-dialect` by hand."
                .into(),
        });
    }

    if sources.iter().any(|s| s.kind == SourceKind::Documents) && which_tesseract().is_none() {
        out.push(Suggestion {
            headline: "Documents found, `tesseract` not on PATH".into(),
            detail: "Scanned-image PDFs will yield no text. OCR degrades silently — install \
                     tesseract if the documents are scans rather than digital text."
                .into(),
        });
    }

    out
}

/// The `origin` remote when it points at GitHub, read from `.git/config` as plain text.
///
/// Deliberately not shelling out to `git`: detection runs before anything else and must work on
/// a machine with no git binary.
fn github_remote(cwd: &Path) -> Option<String> {
    let config = std::fs::read_to_string(cwd.join(".git").join("config")).ok()?;
    config
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("url = "))
        .find(|url| url.contains("github.com"))
        .map(str::to_string)
}

fn which_tesseract() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("tesseract"))
        .find(|candidate| candidate.is_file())
        .map(|p| p.to_string_lossy().to_string())
}

/// The four exclusions every workspace needs regardless of what detection found.
const BASELINE_IGNORES: &[&str] = &[".ekos", ".git", "target", "node_modules"];

/// Render a [`Detection`] as a complete, commented `ekos.toml`.
///
/// Every generated comment names the evidence behind the line it sits above. The alternative —
/// a bare list of patterns — produces a config nobody dares edit because nothing says why any
/// entry is there, which is how this repository's own config ended up needing a five-line
/// comment per exclusion written by hand after each incident.
pub fn render_config(detection: &Detection) -> String {
    let mut out = String::new();
    out.push_str("# EKOS workspace configuration — generated by `ekos init --detect`.\n");
    out.push_str("# Every commented line below records what was detected and why it matters.\n\n");

    out.push_str("[workspace]\nroot = \".\"\nlog-level = \"info\"\nlog-format = \"pretty\"  # \"pretty\" | \"json\"\n\n");

    out.push_str("[observe]\n");
    out.push_str(
        "# Always \".\", never a list of subdirectories: GitObserver only looks for a `.git`\n\
         # directory at the workspace root, so listing subdirectories here yields zero commits\n\
         # with no error at all.\n",
    );
    out.push_str("paths = [\".\"]\n");
    out.push_str("ignore-patterns = [\n");
    for pattern in BASELINE_IGNORES {
        out.push_str(&format!("    \"{pattern}\",\n"));
    }
    for c in &detection.contaminants {
        if BASELINE_IGNORES.contains(&c.pattern) {
            continue;
        }
        out.push_str(&format!(
            "    # {} file(s) found — {}.\n    \"{}\",\n",
            render_count(c),
            c.reason,
            c.pattern
        ));
    }
    out.push_str("]\n\n");

    out.push_str("[recover.sql]\n");
    match &detection.sql_dialect {
        Some(guess) => {
            let markers = guess
                .markers
                .iter()
                .map(|m| format!("{} ({})", m.marker, m.hits))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "# Detected from {} sampled .sql file(s): {}.\n",
                guess.sampled_files, markers
            ));
            out.push_str(
                "# A wrong dialect fails the whole-file DDL parse and the entire schema\n\
                 # disappears behind one SQL001. A *correct* dialect is not sufficient either —\n\
                 # run `ekos coverage` after the first compile to confirm tables were produced.\n",
            );
            out.push_str(&format!("default-dialect = \"{}\"\n", guess.dialect));
        }
        None if detection.file_count(SourceKind::Sql) > 0 => {
            out.push_str(
                "# SQL files were found but no dialect scored clearly (no markers, or a tie).\n\
                 # \"generic\" is the ANSI baseline. If `ekos coverage` reports 0 objects for\n\
                 # SQL, set this to the real dialect: postgres | mysql | mssql | oracle |\n\
                 # snowflake | databricks | clickhouse.\n",
            );
            out.push_str("default-dialect = \"generic\"\n");
        }
        None => {
            out.push_str("# No .sql files found. Left at the ANSI baseline.\n");
            out.push_str("default-dialect = \"generic\"\n");
        }
    }
    out.push('\n');

    out.push_str("# ── Detected inputs ──────────────────────────────────────────────────────\n");
    if detection.sources.is_empty() {
        out.push_str("# Nothing EKOS recovers from was found in this directory.\n");
    }
    for s in &detection.sources {
        out.push_str(&format!(
            "# {:<22} {} file(s)\n",
            s.kind.label(),
            s.file_count
        ));
    }

    if !detection.suggestions.is_empty() {
        out.push_str(
            "\n# ── Not enabled automatically ────────────────────────────────────────────\n",
        );
        for s in &detection.suggestions {
            out.push_str(&format!("# {}\n", s.headline));
            for line in wrap_comment(&s.detail) {
                out.push_str(&format!("#   {line}\n"));
            }
        }
    }

    out.push_str(
        "\n# Next: ekos build && ekos recover && ekos resolve && ekos compile && ekos commit\n\
         # Then: ekos coverage   — confirms every input kind actually produced objects.\n",
    );

    out
}

/// A contaminant's file count, marked as a floor when the enumeration was capped.
pub fn render_count(c: &Contaminant) -> String {
    if c.capped {
        format!("{}+", c.observed_files)
    } else {
        c.observed_files.to_string()
    }
}

/// Wrap prose to ~86 columns for a comment block.
fn wrap_comment(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + word.len() + 1 > 86 {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    /// Every variant, positive **and** negative.
    ///
    /// The negative half is the point: a filter that was only ever tested against inputs it
    /// should accept is how `headless.sh` shipped a filter that compared an item to itself
    /// instead of to the requested list and passed everything.
    #[test]
    fn every_source_kind_matches_its_own_paths_and_rejects_others() {
        let cases: &[(SourceKind, &str, &str)] = &[
            (SourceKind::Sql, "db/schema.sql", "db/schema.rb"),
            (SourceKind::Git, "git:commit:abc123", "src/git.rs"),
            (SourceKind::Rust, "src/main.rs", "src/main.rst"),
            (SourceKind::Python, "app/models.py", "app/models.pyc"),
            (SourceKind::JavaScript, "web/ui/app.tsx", "web/ui/app.css"),
            (SourceKind::Elixir, "lib/plausible.ex", "lib/plausible.erl"),
            (SourceKind::Perl, "lib/LedgerSMB.pm", "lib/LedgerSMB.pod"),
            (SourceKind::Pentaho, "etl/load.ktr", "etl/load.xml"),
            (SourceKind::Dbt, "dbt_project.yml", "project.yml"),
            (SourceKind::Documents, "docs/spec.pdf", "docs/spec.pages"),
            (
                SourceKind::CargoManifest,
                "ekos/Cargo.toml",
                "ekos/Cargo.lock",
            ),
            (
                SourceKind::PackageJson,
                "web/ui/package.json",
                "web/ui/package-lock.json",
            ),
            (
                SourceKind::CiCd,
                ".github/workflows/ci.yml",
                "deploy/ci.yml",
            ),
        ];

        for (kind, yes, no) in cases {
            assert!(kind.matches_path(yes), "{:?} must match {yes}", kind);
            assert!(!kind.matches_path(no), "{:?} must NOT match {no}", kind);
        }

        assert_eq!(
            cases.len(),
            SourceKind::all().len(),
            "every SourceKind variant needs a positive and a negative case here"
        );
    }

    /// `.cgi` needs a real `perl` shebang the path cannot reveal, so it must not be counted.
    #[test]
    fn perl_does_not_claim_cgi_from_the_path_alone() {
        assert!(!SourceKind::Perl.matches_path("cgi-bin/login.cgi"));
    }

    #[test]
    fn a_sql_file_inside_a_dbt_project_belongs_to_dbt_not_to_sql() {
        let paths = listing(&[
            "warehouse/dbt_project.yml",
            "warehouse/models/silver_customer.sql",
            "db/schema.sql",
        ]);
        let c = Classifier::from_listing(&paths);

        assert_eq!(
            c.classify("warehouse/models/silver_customer.sql"),
            Some(SourceKind::Dbt),
            "a dbt model is not hand-written DDL — counting it as SQL coverage would hide a \
             total DDL-recovery failure"
        );
        assert_eq!(c.classify("db/schema.sql"), Some(SourceKind::Sql));
    }

    #[test]
    fn a_dbt_project_at_the_root_claims_every_sql_file_under_it() {
        let c = Classifier::from_listing(&listing(&["dbt_project.yml"]));
        assert_eq!(c.classify("models/a.sql"), Some(SourceKind::Dbt));
    }

    #[test]
    fn paths_normalize_across_separators_and_leading_dot_slash() {
        let c = Classifier::default();
        assert_eq!(c.classify("./src/main.rs"), Some(SourceKind::Rust));
        assert_eq!(c.classify(r"src\main.rs"), Some(SourceKind::Rust));
    }

    #[test]
    fn an_unknown_path_classifies_as_nothing() {
        assert_eq!(Classifier::default().classify("LICENSE"), None);
    }

    #[test]
    fn sources_are_counted_with_samples() {
        let found = detect_sources(&listing(&[
            "src/a.rs", "src/b.rs", "src/c.rs", "src/d.rs", "app.py",
        ]));

        let rust = found.iter().find(|s| s.kind == SourceKind::Rust).unwrap();
        assert_eq!(rust.file_count, 4);
        assert_eq!(rust.sample_paths.len(), 3, "samples are capped at three");
        assert_eq!(
            found
                .iter()
                .find(|s| s.kind == SourceKind::Python)
                .unwrap()
                .file_count,
            1
        );
    }

    #[test]
    fn contaminants_are_found_but_generic_build_directories_are_not_flagged() {
        let found = detect_contaminants(&listing(&[
            "web/api/.venv/lib/python3.13/site-packages/numpy/core.py",
            "web/ui/node_modules/react/index.js",
            ".scannerwork/report.txt",
            "test-runs/run-1/workspace/a.sql",
            "build/output.o",
            "dist/bundle.js",
            "src/real.rs",
        ]));
        let patterns: Vec<&str> = found.iter().map(|c| c.pattern).collect();

        assert!(patterns.contains(&".venv"));
        assert!(patterns.contains(&"site-packages"));
        assert!(patterns.contains(&"node_modules"));
        assert!(patterns.contains(&".scannerwork"));
        assert!(
            !patterns.contains(&"build") && !patterns.contains(&"dist"),
            "generic build-output names must never be auto-excluded — patterns match a bare \
             path component, so they would prune real source directories"
        );
    }

    #[test]
    fn a_contaminant_reports_how_many_files_it_holds() {
        let found = detect_contaminants(&listing(&["a/.venv/x.py", "a/.venv/y.py", "src/main.rs"]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].observed_files, 2);
    }

    #[test]
    fn postgres_is_detected_from_the_ledgersmb_marker_shape() {
        let guess = guess_dialect(&[
            (
                "sql/modules/Account.sql".into(),
                "CREATE TABLE account (id SERIAL PRIMARY KEY);\n\
                 COMMENT ON TABLE account IS $$The chart of accounts.$$;\n"
                    .into(),
            ),
            (
                "sql/modules/Person.sql".into(),
                "CREATE TABLE person (id SERIAL) INHERITS (entity);\n".into(),
            ),
        ])
        .expect("postgres markers must score");

        assert_eq!(guess.dialect, "postgres");
        assert_eq!(guess.sampled_files, 2);
        assert!(
            guess
                .markers
                .iter()
                .any(|m| m.marker == "serial" && m.hits == 2),
            "marker hits count FILES, not occurrences: {:?}",
            guess.markers
        );
    }

    #[test]
    fn mysql_is_detected_from_auto_increment_and_engine() {
        let guess = guess_dialect(&[(
            "schema.sql".into(),
            "CREATE TABLE t (id INT AUTO_INCREMENT) ENGINE=InnoDB;".into(),
        )])
        .expect("mysql markers must score");
        assert_eq!(guess.dialect, "mysql");
    }

    #[test]
    fn no_sql_and_no_markers_guess_nothing() {
        assert!(guess_dialect(&[]).is_none());
        assert!(
            guess_dialect(&[("a.sql".into(), "CREATE TABLE t (id INT);".into())]).is_none(),
            "plain ANSI DDL matches no dialect and must not be forced into one"
        );
    }

    /// A tie must produce `generic`, not a coin flip.
    ///
    /// RFC 0060's finding, applied here: when no signal reliably separates two answers, surface
    /// the uncertainty instead of picking. Guessing wrong costs the user their entire schema.
    #[test]
    fn a_tie_between_two_dialects_is_not_a_guess() {
        let guess = guess_dialect(&[(
            "mixed.sql".into(),
            "CREATE TABLE t (id SERIAL);\nCREATE TABLE u (id INT AUTO_INCREMENT);".into(),
        )]);
        assert!(
            guess.is_none(),
            "one postgres marker against one mysql marker is a tie, not a detection"
        );
    }

    #[test]
    fn the_generated_config_parses_back_into_a_real_ekos_config() {
        let detection = Detection {
            sources: vec![DetectedSource {
                kind: SourceKind::Rust,
                file_count: 12,
                sample_paths: vec!["src/main.rs".into()],
            }],
            contaminants: vec![Contaminant {
                pattern: ".venv",
                reason: "third-party Python",
                observed_files: 3758,
                capped: false,
            }],
            sql_dialect: Some(DialectGuess {
                dialect: "postgres",
                markers: vec![DialectMarker {
                    marker: "serial",
                    hits: 31,
                }],
                sampled_files: 47,
            }),
            suggestions: vec![Suggestion {
                headline: "GitHub remote detected".into(),
                detail: "a".repeat(200),
            }],
        };

        let rendered = render_config(&detection);
        let parsed: EkosConfig =
            toml::from_str(&rendered).expect("generated config must be valid ekos.toml");

        assert_eq!(parsed.recover.sql.default_dialect, "postgres");
        assert_eq!(parsed.observe.paths, vec![std::path::PathBuf::from(".")]);
        assert!(
            parsed.observe.ignore_patterns.iter().any(|p| p == ".venv"),
            "a detected contaminant must reach the real parsed config, not just the comments"
        );
        assert!(
            rendered.contains("3758 file(s)"),
            "the reason a pattern is excluded must survive into the file"
        );
    }

    /// A capped enumeration must never print as a total.
    ///
    /// This repository has three `target/` directories; each stops at the cap, so a naive sum
    /// would print "60000 files" — a number nothing counted. Fabricating a precise-looking
    /// figure is exactly the failure this whole feature exists to prevent.
    #[test]
    fn a_capped_contaminant_count_renders_as_a_floor() {
        let capped = Contaminant {
            pattern: "target",
            reason: "Rust build output",
            observed_files: 20_000,
            capped: true,
        };
        assert_eq!(render_count(&capped), "20000+");

        let exact = Contaminant {
            capped: false,
            ..capped.clone()
        };
        assert_eq!(render_count(&exact), "20000");

        // `target` is a baseline ignore, so the generated config lists it without a comment —
        // use a detected-only pattern to check the marker reaches the file.
        let rendered = render_config(&Detection {
            contaminants: vec![Contaminant {
                pattern: ".venv",
                ..capped
            }],
            ..Detection::default()
        });
        assert!(
            rendered.contains("20000+ file(s) found"),
            "the floor marker must survive into the generated config: {rendered}"
        );
    }

    #[test]
    fn a_workspace_with_no_sql_still_renders_a_valid_config() {
        let rendered = render_config(&Detection::default());
        let parsed: EkosConfig = toml::from_str(&rendered).expect("must parse");
        assert_eq!(parsed.recover.sql.default_dialect, "generic");
    }
}
