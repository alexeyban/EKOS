//! Scenario/dataset schema (RFC 0138) — `serde_yaml` deserialization of `evals/datasets/*.yaml`
//! and `evals/datasets/manifest.yaml`. Pure data types; no I/O beyond reading the files handed to
//! [`load_dataset`].

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;

/// Which pipeline a scenario is graded against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// `AiRuntime::reason` — the REASON planner + typed evidence pipeline (RFC 0123/0124).
    #[default]
    Reason,
    /// `AiRuntime::ask` — the classic `gather_context` pipeline (pre-0123).
    Ask,
    /// `Runtime::retrieve` only — no LLM call, graded purely on recall@k.
    Retrieval,
}

/// Words too common in ordinary prose to serve as evidence that an answer *declined* (RFC 0139
/// §2.5). Listed rather than inferred so the rule is auditable: each of these appears in
/// fabricated answers as readily as in refusals.
#[cfg(test)]
const DEGENERATE_REFUSAL_PHRASES: &[&str] = &["not", "no", "never", "none", "n/a", "isn't", "cant"];

fn default_pass_threshold() -> f32 {
    0.7
}

/// One thing a correct answer has to say (RFC 0139 §2.1).
///
/// `Literal` is the original form and every existing dataset line still deserialises as one — the
/// enum is `untagged`, so this was a purely additive schema change. `AnyOf` exists because a
/// deterministic matcher cannot be expected to know that "CKM" means "Canonical Knowledge Model";
/// rather than guess at synonyms, the scenario declares which wordings it accepts:
///
/// ```yaml
/// expected_facts:
///   - any_of: ["Canonical Knowledge Model", "CKM"]
///   - "append-only"
/// ```
///
/// An `AnyOf` counts as **one** slot in the denominator no matter how many alternates it lists —
/// it removes false negatives without inflating the score.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ExpectedFact {
    Literal(String),
    AnyOf { any_of: Vec<String> },
}

impl From<&str> for ExpectedFact {
    fn from(s: &str) -> Self {
        Self::Literal(s.to_string())
    }
}

impl ExpectedFact {
    /// Every accepted wording for this fact.
    pub fn alternates(&self) -> &[String] {
        match self {
            Self::Literal(s) => std::slice::from_ref(s),
            Self::AnyOf { any_of } => any_of,
        }
    }

    /// The canonical wording, for diagnostics and attribution.
    pub fn primary(&self) -> &str {
        match self {
            Self::Literal(s) => s,
            Self::AnyOf { any_of } => any_of.first().map(String::as_str).unwrap_or(""),
        }
    }
}

/// One graded question. See `ekos/docs/rfcs/0138-eval-harness.md` §1 for the full field
/// contract and worked examples.
#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    pub id: String,
    pub question: String,
    /// The `category:` of the file this scenario was loaded from — not part of the YAML itself
    /// (there's no per-scenario field for it), stamped on by [`load_dataset`] so `--category`
    /// filtering and per-category report breakdowns have something to key on.
    #[serde(skip, default)]
    pub category: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub difficulty: Option<String>,
    /// Marks this scenario as intentionally testing hallucination resistance rather than normal
    /// recall — purely descriptive, doesn't change grading on its own (`should_refuse` does).
    #[serde(default)]
    pub adversarial: bool,
    /// The question has no grounded answer in the ledger; a correct answer declines rather than
    /// fabricates. Graded by `evaluators::groundedness`.
    #[serde(default)]
    pub should_refuse: bool,
    /// Extra phrases (beyond the evaluator's builtin list) that count as a valid refusal for this
    /// scenario, e.g. wording specific to the question's phrasing.
    #[serde(default)]
    pub refusal_phrases: Vec<String>,
    /// Facts a correct answer must state. Each entry is either a literal phrase or a set of
    /// accepted alternates (RFC 0139 §2.1) — matched under
    /// [`crate::evaluators::normalize`], so wording and word endings don't decide correctness.
    #[serde(default)]
    pub expected_facts: Vec<ExpectedFact>,
    /// Substrings expected in the fragment/path of at least one *valid* cited evidence entry.
    #[serde(default)]
    pub expected_evidence_contains: Vec<String>,
    /// Real object *names* (not ids — unstable across rebuilds) a good retrieval must surface in
    /// the top-10, graded via `ekos_runtime::retrieval_eval::recall_at_k`.
    #[serde(default)]
    pub expected_objects: Vec<String>,
    /// Optional trajectory check: the REASON planner's `QueryType` this question should route to.
    #[serde(default)]
    pub expected_query_type: Option<String>,
    #[serde(default = "default_pass_threshold")]
    pub pass_threshold: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Dataset {
    pub version: u32,
    pub category: String,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub datasets: BTreeMap<String, ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ManifestEntry {
    pub files: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SchemaError {
    #[error("reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {source}")]
    Yaml {
        path: String,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("no dataset named {0:?} in manifest.yaml, and no file {0:?}.yaml in datasets dir")]
    UnknownDataset(String),
    #[error("datasets dir {0:?} has no *.yaml files")]
    EmptyDatasetsDir(String),
}

fn read_yaml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, SchemaError> {
    let text = std::fs::read_to_string(path).map_err(|source| SchemaError::Io {
        path: path.display().to_string(),
        source,
    })?;
    serde_yaml::from_str(&text).map_err(|source| SchemaError::Yaml {
        path: path.display().to_string(),
        source,
    })
}

/// Every `*.yaml` file in `datasets_dir` except `manifest.yaml`, sorted by filename for a stable
/// default ordering.
fn all_category_files(datasets_dir: &Path) -> Result<Vec<std::path::PathBuf>, SchemaError> {
    let mut files: Vec<_> = std::fs::read_dir(datasets_dir)
        .map_err(|source| SchemaError::Io {
            path: datasets_dir.display().to_string(),
            source,
        })?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension().and_then(std::ffi::OsStr::to_str) == Some("yaml")
                && p.file_name().and_then(std::ffi::OsStr::to_str) != Some("manifest.yaml")
        })
        .collect();
    files.sort();
    Ok(files)
}

/// Resolve `--dataset <name>` (or `None`) against `datasets_dir` into `(report_name, scenarios)`.
///
/// - `Some(name)` matching a `manifest.yaml` entry: that entry's files, name unchanged.
/// - `Some(name)` matching `<name>.yaml` directly (no manifest, or not listed there): that one
///   file, name unchanged.
/// - `None`: every `*.yaml` file in `datasets_dir` (except `manifest.yaml`), named
///   `ekos-<total scenario count>` — this is where a name like `ekos-100` comes from: it's the
///   real current total, not a fixed magic string (RFC 0138 §1).
pub fn load_dataset(
    name: Option<&str>,
    datasets_dir: &Path,
) -> Result<(String, Vec<Scenario>), SchemaError> {
    let manifest_path = datasets_dir.join("manifest.yaml");
    let manifest: Option<Manifest> = if manifest_path.is_file() {
        Some(read_yaml(&manifest_path)?)
    } else {
        None
    };

    let files: Vec<std::path::PathBuf> = match name {
        Some(n) => {
            if let Some(entry) = manifest.as_ref().and_then(|m| m.datasets.get(n)) {
                entry.files.iter().map(|f| datasets_dir.join(f)).collect()
            } else {
                let direct = datasets_dir.join(format!("{n}.yaml"));
                if direct.is_file() {
                    vec![direct]
                } else {
                    return Err(SchemaError::UnknownDataset(n.to_string()));
                }
            }
        }
        None => all_category_files(datasets_dir)?,
    };

    let mut scenarios = Vec::new();
    for file in &files {
        let dataset: Dataset = read_yaml(file)?;
        let category = dataset.category;
        scenarios.extend(dataset.scenarios.into_iter().map(|mut s| {
            s.category = category.clone();
            s
        }));
    }

    let report_name = match name {
        Some(n) => n.to_string(),
        None => {
            if scenarios.is_empty() {
                return Err(SchemaError::EmptyDatasetsDir(
                    datasets_dir.display().to_string(),
                ));
            }
            format!("ekos-{}", scenarios.len())
        }
    };

    Ok((report_name, scenarios))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write(dir: &std::path::Path, name: &str, content: &str) {
        let mut f = std::fs::File::create(dir.join(name)).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn explicit_dataset_name_loads_manifest_entry() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "architecture.yaml",
            "version: 1\ncategory: architecture\nscenarios:\n  - id: a1\n    question: q1\n",
        );
        write(
            dir.path(),
            "code.yaml",
            "version: 1\ncategory: code\nscenarios:\n  - id: c1\n    question: q2\n",
        );
        write(
            dir.path(),
            "manifest.yaml",
            "version: 1\ndatasets:\n  arch-only:\n    files: [\"architecture.yaml\"]\n",
        );

        let (name, scenarios) = load_dataset(Some("arch-only"), dir.path()).unwrap();
        assert_eq!(name, "arch-only");
        assert_eq!(scenarios.len(), 1);
        assert_eq!(scenarios[0].category, "architecture");
    }

    #[test]
    fn no_dataset_name_loads_everything_and_names_by_count() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "architecture.yaml",
            "version: 1\ncategory: architecture\nscenarios:\n  - id: a1\n    question: q1\n  - id: a2\n    question: q2\n",
        );
        write(
            dir.path(),
            "code.yaml",
            "version: 1\ncategory: code\nscenarios:\n  - id: c1\n    question: q3\n",
        );

        let (name, scenarios) = load_dataset(None, dir.path()).unwrap();
        assert_eq!(name, "ekos-3");
        assert_eq!(scenarios.len(), 3);
    }

    #[test]
    fn direct_filename_stem_works_without_manifest() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "security.yaml",
            "version: 1\ncategory: security\nscenarios:\n  - id: s1\n    question: q1\n",
        );
        let (name, scenarios) = load_dataset(Some("security"), dir.path()).unwrap();
        assert_eq!(name, "security");
        assert_eq!(scenarios[0].category, "security");
    }

    #[test]
    fn unknown_dataset_name_errors() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "architecture.yaml",
            "version: 1\ncategory: architecture\nscenarios: []\n",
        );
        let err = load_dataset(Some("does-not-exist"), dir.path()).unwrap_err();
        assert!(matches!(err, SchemaError::UnknownDataset(_)));
    }

    /// Loads the real, checked-in `evals/datasets/` directory (not a synthetic tempdir) — catches
    /// a YAML syntax error, a duplicate id, or an empty question/id before it ever reaches a real
    /// `ekos eval run`. `CARGO_MANIFEST_DIR` is `ekos/crates/evals`; the real datasets dir is
    /// three levels up (`crates/evals` -> `crates` -> `ekos` -> repo root) then `evals/datasets`.
    #[test]
    fn real_ekos_full_dataset_loads_and_every_scenario_has_a_unique_nonempty_id() {
        let datasets_dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../evals/datasets");
        let (name, scenarios) = load_dataset(Some("ekos-full"), &datasets_dir)
            .unwrap_or_else(|e| panic!("loading real evals/datasets/ekos-full: {e}"));
        assert_eq!(name, "ekos-full");
        assert!(
            scenarios.len() >= 90,
            "expected a real, substantial suite across 7 categories, got {}",
            scenarios.len()
        );

        let mut seen_ids = std::collections::HashSet::new();
        for s in &scenarios {
            assert!(!s.id.is_empty(), "empty scenario id");
            assert!(!s.question.is_empty(), "empty question for {}", s.id);
            assert!(
                seen_ids.insert(s.id.clone()),
                "duplicate scenario id: {}",
                s.id
            );
            // Every scenario must be gradable by at least one signal — a scenario with none of
            // these contributes a silent neutral pass forever (documented in evals/README.md).
            let gradable = !s.expected_facts.is_empty()
                || !s.expected_evidence_contains.is_empty()
                || !s.expected_objects.is_empty()
                || s.expected_query_type.is_some()
                || s.should_refuse;
            assert!(gradable, "{} has no gradable signal at all", s.id);

            // RFC 0139 §2.5 — a refusal phrase short or generic enough to appear in ordinary
            // prose makes its scenario impossible to fail. `adv-014` shipped with a bare `"not"`,
            // which matches almost any English sentence, so a confident fabrication containing a
            // negation scored a perfect refusal. A check that cannot fail measures nothing.
            for phrase in &s.refusal_phrases {
                let p = phrase.trim().to_lowercase();
                assert!(
                    p.len() >= 5 && !DEGENERATE_REFUSAL_PHRASES.contains(&p.as_str()),
                    "{}: refusal phrase {phrase:?} is too generic to ever fail — use wording a \
                     fabricated answer would not contain",
                    s.id
                );
                // A phrase quoted from the question is satisfied by *echoing the question*, which
                // is what a fabrication does. Found the hard way: `adv-014` accepted "new
                // version", its own question says "rather than appending a new version", and an
                // answer confirming the false premise verbatim scored as a correct refusal.
                assert!(
                    !s.question.to_lowercase().contains(&p),
                    "{}: refusal phrase {phrase:?} appears in the question itself, so an answer \
                     that merely echoes the question counts as refusing",
                    s.id
                );
            }
        }

        let categories: std::collections::HashSet<&str> =
            scenarios.iter().map(|s| s.category.as_str()).collect();
        assert_eq!(
            categories,
            std::collections::HashSet::from([
                "architecture",
                "code",
                "dependencies",
                "lineage",
                "history",
                "security",
                "adversarial",
            ]),
            "expected exactly the 7 A-G categories"
        );
    }
}
