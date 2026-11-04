//! Leakage guards (RFC 0240 §2.3) — tested, not documented.
//!
//! 1. **Allowlist.** A project's source directory holds only the files `prepare` generated from
//!    the database and its dictionary; anything else (a gold SQL file someone dropped there) fails.
//! 2. **Scan.** The text a project is built from, and afterwards every object and relationship in
//!    its ledger, is searched for the normalised text of every question, gold SQL statement and
//!    evidence string of the suite. Any hit fails `prepare`.

use super::BenchScenario;
use anyhow::{Result, bail};
use ekos_kir::{KirObject, KirRelationship};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// Evidence shorter than this (normalised) is not searched for: "female refers to gender = f"
/// is a dictionary fact, not a leaked answer.
pub const MIN_EVIDENCE_CHARS: usize = 40;

/// What one scanned text was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeakKind {
    Question,
    GoldSql,
    Evidence,
}

/// A suite text found where it must not be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeakHit {
    pub question_id: String,
    pub kind: LeakKind,
    /// Where it was found: a file path, or `object <id> (<name>)`.
    pub found_in: String,
}

/// Lower-case, every run of non-alphanumeric characters one space, trimmed.
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else {
            space = true;
        }
    }
    out
}

/// The normalised needles to look for.
pub struct Needles {
    needles: Vec<(String, String, LeakKind)>,
}

impl Needles {
    pub fn from_scenarios(scenarios: &[BenchScenario]) -> Needles {
        let mut needles = Vec::new();
        let mut seen = BTreeSet::new();
        for s in scenarios {
            for (text, kind) in [
                (&s.question, LeakKind::Question),
                (&s.gold_sql, LeakKind::GoldSql),
                (&s.evidence, LeakKind::Evidence),
            ] {
                let n = normalise(text);
                let min = if kind == LeakKind::Evidence {
                    MIN_EVIDENCE_CHARS
                } else {
                    1
                };
                if n.len() >= min && seen.insert((n.clone(), kind as u8)) {
                    needles.push((n, s.id.clone(), kind));
                }
            }
        }
        Needles { needles }
    }

    pub fn len(&self) -> usize {
        self.needles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.needles.is_empty()
    }

    /// Every needle contained in `haystack` (already normalised).
    pub fn find(&self, haystack: &str, found_in: &str) -> Vec<LeakHit> {
        self.needles
            .iter()
            .filter(|(n, _, _)| haystack.contains(n.as_str()))
            .map(|(_, id, kind)| LeakHit {
                question_id: id.clone(),
                kind: *kind,
                found_in: found_in.to_string(),
            })
            .collect()
    }
}

/// Guard 1: every file under `dir` is one `prepare` wrote.
pub fn check_allowlist(dir: &Path, allowed: &BTreeSet<String>) -> Result<()> {
    let mut extra = Vec::new();
    walk(dir, dir, &mut |rel| {
        if !allowed.contains(rel) {
            extra.push(rel.to_string());
        }
    })?;
    if !extra.is_empty() {
        bail!(
            "{} holds files prepare did not write — only the generated schema may reach a \
             project: {}",
            dir.display(),
            extra.join(", ")
        );
    }
    Ok(())
}

fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&str)) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(root, &path, f)?;
        } else {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            f(&rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Guard 2, before the pipeline: the files a project is given.
pub fn scan_files(dir: &Path, needles: &Needles) -> Result<Vec<LeakHit>> {
    let mut hits = Vec::new();
    let mut files = Vec::new();
    walk(dir, dir, &mut |rel| files.push(rel.to_string()))?;
    for rel in files {
        let text = std::fs::read(dir.join(&rel))?;
        let text = normalise(&String::from_utf8_lossy(&text));
        hits.extend(needles.find(&text, &dir.join(&rel).display().to_string()));
    }
    Ok(hits)
}

/// Guard 2, after the pipeline: everything a project's ledger holds.
pub fn scan_ledger(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    needles: &Needles,
) -> Vec<LeakHit> {
    let mut hits = Vec::new();
    for o in objects {
        let text = format!(
            "{} {}",
            o.name,
            serde_json::to_string(&o.properties).unwrap_or_default()
        );
        hits.extend(needles.find(&normalise(&text), &format!("object {} ({})", o.id, o.name)));
    }
    for r in relationships {
        let text = serde_json::to_string(&r.properties).unwrap_or_default();
        hits.extend(needles.find(&normalise(&text), &format!("relationship {}", r.id)));
    }
    hits
}

/// Drops evidence hits whose sentence the benchmark's own dictionary already contains: that text
/// is a shipped input of the benchmark, not a leaked hint. Question and gold-SQL hits always stay.
pub fn without_shipped(
    hits: Vec<LeakHit>,
    scenarios: &[BenchScenario],
    dictionary_text: &str,
) -> Vec<LeakHit> {
    hits.into_iter()
        .filter(|h| {
            !(h.kind == LeakKind::Evidence
                && scenarios
                    .iter()
                    .find(|s| s.id == h.question_id)
                    .is_some_and(|s| dictionary_text.contains(&normalise(&s.evidence))))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;

    fn scenario() -> BenchScenario {
        BenchScenario {
            id: "7".into(),
            db_id: "shop".into(),
            question: "How many customers live in Paris?".into(),
            evidence: "Living in Paris refers to city = 'Paris' in the customers table of record."
                .into(),
            gold_sql: "SELECT COUNT(*) FROM customers WHERE city = 'Paris'".into(),
            difficulty: "simple".into(),
        }
    }

    #[test]
    fn normalising_ignores_case_quotes_and_spacing() {
        assert_eq!(
            normalise("SELECT  COUNT(*)\nFROM `customers`"),
            "select count from customers"
        );
    }

    #[test]
    fn evidence_planted_in_a_ledger_is_found() {
        let needles = Needles::from_scenarios(&[scenario()]);
        let mut o = KirObject::new("customers", ObjectKind::Table);
        o.properties.insert(
            "description".into(),
            serde_json::json!(
                "NOTE: living in paris refers to CITY = 'Paris' in the customers table of record"
            ),
        );
        let hits = scan_ledger(&[o], &[], &needles);
        assert_eq!(hits.len(), 1, "{hits:?}");
        assert_eq!(hits[0].kind, LeakKind::Evidence);
        assert_eq!(hits[0].question_id, "7");
    }

    #[test]
    fn gold_sql_reformatted_inside_a_file_is_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("schema.sql"),
            "-- select count(*)\n-- from customers where CITY='paris'\nCREATE TABLE x (a int);",
        )
        .unwrap();
        let hits = scan_files(dir.path(), &Needles::from_scenarios(&[scenario()])).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, LeakKind::GoldSql);
    }

    #[test]
    fn a_clean_ledger_has_no_hits() {
        let needles = Needles::from_scenarios(&[scenario()]);
        let o = KirObject::new("customers", ObjectKind::Table);
        assert!(scan_ledger(&[o], &[], &needles).is_empty());
    }

    #[test]
    fn a_file_prepare_did_not_write_fails_the_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("schema.sql"), "x").unwrap();
        let allowed: BTreeSet<String> = ["schema.sql".to_string()].into();
        check_allowlist(dir.path(), &allowed).unwrap();
        std::fs::write(dir.path().join("gold.sql"), "SELECT 1").unwrap();
        let err = check_allowlist(dir.path(), &allowed)
            .unwrap_err()
            .to_string();
        assert!(err.contains("gold.sql"), "{err}");
    }
}
