//! `evals/bench/<suite>/suite.toml` — what a benchmark suite is made of (RFC 0240 §2.2).
//!
//! A suite names its adapter, its question file and its databases, each either a downloaded
//! archive pinned by SHA-256 or a directory already on disk (the in-repo fixture uses the latter).
//! Everything downloaded or extracted lives under `data/`, which is never checked in.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// One `suite.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Suite {
    /// The suite's name, also its directory name: `bird-mini-dev`.
    pub name: String,
    /// The adapter that reads it: `bird`.
    pub adapter: String,
    /// The benchmark's own release name, recorded in every report (`Mini-Dev 1024`).
    pub version: String,
    /// The SQL engine the questions are written for. Only `sqlite` is run today.
    #[serde(default = "default_dialect")]
    pub dialect: String,
    /// Reports from this suite are marked "New Dev" (BIRD's cleaned dev split, Nov 2025).
    #[serde(default)]
    pub new_dev: bool,
    pub questions: Source,
    pub databases: Source,
    #[serde(default)]
    pub split: SplitSpec,
    /// Where the suite came from, for the report; free text.
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_dialect() -> String {
    "sqlite".into()
}

/// A file or archive the suite needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Source {
    /// File name under `data/archives/` (downloaded) — or, with no `url`, a path relative to the
    /// suite directory (a file for questions, a directory of `<db>/<db>.sqlite` for databases).
    pub file: String,
    /// Where `prepare` downloads `file` from when it is missing.
    #[serde(default)]
    pub url: Option<String>,
    /// Required for anything downloaded; checked on every `prepare`.
    #[serde(default)]
    pub sha256: Option<String>,
    /// Inside a database archive: the directory holding one folder per database.
    #[serde(default)]
    pub prefix: Option<String>,
}

/// The tuning / hold-out split (RFC 0240 §6, first open question).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SplitSpec {
    pub seed: u64,
    /// Share of each (database, difficulty) stratum that goes to the tuning split.
    pub tune_fraction: f64,
}

impl Default for SplitSpec {
    fn default() -> Self {
        Self {
            seed: 240,
            tune_fraction: 0.4,
        }
    }
}

impl Suite {
    pub fn load(suite_dir: &Path) -> Result<Suite> {
        let path = suite_dir.join("suite.toml");
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let suite: Suite =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        for source in [&suite.questions, &suite.databases] {
            if source.url.is_some() && source.sha256.is_none() {
                bail!(
                    "{}: {} is downloaded but has no sha256",
                    path.display(),
                    source.file
                );
            }
        }
        if !(0.0..=1.0).contains(&suite.split.tune_fraction) {
            bail!("{}: split.tune-fraction must be in [0, 1]", path.display());
        }
        Ok(suite)
    }
}

/// The directories one suite reads and writes.
#[derive(Debug, Clone)]
pub struct SuiteDirs {
    pub suite: PathBuf,
    pub data: PathBuf,
}

impl SuiteDirs {
    pub fn new(suite_dir: &Path) -> Self {
        Self {
            suite: suite_dir.to_path_buf(),
            data: suite_dir.join("data"),
        }
    }
    pub fn archives(&self) -> PathBuf {
        self.data.join("archives")
    }
    /// Extracted databases: `<db>/<db>.sqlite` and `<db>/database_description/*.csv` only.
    pub fn databases(&self) -> PathBuf {
        self.data.join("databases")
    }
    /// One source directory per EKOS project — the only files a project is ever given.
    pub fn projects(&self) -> PathBuf {
        self.data.join("projects")
    }
    /// The scratch EKOS home (RFC 0178) holding one project per database.
    pub fn home(&self) -> PathBuf {
        self.data.join("home")
    }
    /// Distinct column values for question-time value lookup.
    pub fn values(&self) -> PathBuf {
        self.data.join("values")
    }
    /// What the last `prepare` built: fingerprints and the leakage scan.
    pub fn prepared(&self) -> PathBuf {
        self.data.join("prepared.json")
    }

    /// Where `source` is on disk: under `data/archives/` when downloaded, else in the suite.
    pub fn locate(&self, source: &Source) -> PathBuf {
        if source.url.is_some() {
            self.archives().join(&source.file)
        } else {
            self.suite.join(&source.file)
        }
    }
}

/// SHA-256 of a file, lower-case hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Fails unless `path` exists and, when the suite pins one, matches its checksum.
pub fn verify(source: &Source, path: &Path) -> Result<()> {
    if !path.exists() {
        match &source.url {
            Some(url) => bail!(
                "{} is missing — download it from {url} (prepare does this unless --offline)",
                path.display()
            ),
            None => bail!("{} is missing", path.display()),
        }
    }
    if let Some(want) = &source.sha256 {
        let got = sha256_file(path)?;
        if !got.eq_ignore_ascii_case(want) {
            bail!(
                "{}: sha256 {got} does not match the suite's {want}",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_downloaded_source_without_a_checksum_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("suite.toml"),
            r#"
name = "x"
adapter = "bird"
version = "1"
[questions]
file = "q.jsonl"
url = "https://example.invalid/q.jsonl"
[databases]
file = "dbs"
"#,
        )
        .unwrap();
        let err = Suite::load(dir.path()).unwrap_err().to_string();
        assert!(err.contains("no sha256"), "{err}");
    }

    #[test]
    fn verify_rejects_a_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        std::fs::write(&path, b"hello").unwrap();
        let mut source = Source {
            file: "a.bin".into(),
            url: Some("https://example.invalid/a.bin".into()),
            sha256: Some(sha256_hex(b"hello")),
            prefix: None,
        };
        verify(&source, &path).unwrap();
        source.sha256 = Some(sha256_hex(b"other"));
        assert!(verify(&source, &path).is_err());
    }
}
