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

fn default_pass_threshold() -> f32 {
    0.7
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
    /// Keywords/phrases expected to appear (case-insensitive substring) in the answer text.
    #[serde(default)]
    pub expected_facts: Vec<String>,
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
}
