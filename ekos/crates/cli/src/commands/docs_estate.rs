//! `ekos docs generate --layout estate` — RFC 0214, source-estate documentation.
//!
//! The pages are `ekos_docs_gen::estate` (pure); this file reads the ledger (read-only), the
//! review state (RFC 0213: scope and sign-offs), the committed manifest (provenance) and each
//! source's licence file, then writes the pages with RFC 0208's file rules: manual sections are
//! carried over, an unchanged file is not rewritten, an estate page no longer produced is
//! deleted, and a page without the estate marker (hand-written, or another generator's) is never
//! touched.

use super::store::open_store_read_only;
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_docs_gen::estate::{self, EstateInput, Provenance, ScopeEntry, SubjectLine};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The front-matter line only estate pages carry; deletion keys on it.
const MARKER: &str = "ekos_estate_version:";

/// What a run wrote.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub written: Vec<String>,
    pub unchanged: Vec<String>,
    pub removed: Vec<String>,
}

/// Each source's and database's licence: `[docs-estate] licences` first, then the licence file at
/// a source's root, else `unknown`.
pub fn licences(config: &EkosConfig, cwd: &Path) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for s in &config.sources {
        let root = if s.path.is_absolute() {
            s.path.clone()
        } else {
            cwd.join(&s.path)
        };
        out.insert(
            s.name.clone(),
            licence_at(&root).unwrap_or("unknown").to_string(),
        );
    }
    for d in &config.databases {
        out.insert(d.name.clone(), "unknown".to_string());
    }
    for (k, v) in &config.docs_estate.licences {
        out.insert(k.clone(), v.clone());
    }
    out
}

fn licence_at(root: &Path) -> Option<&'static str> {
    let mut names: Vec<_> = std::fs::read_dir(root)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| {
            let u = n.to_ascii_uppercase();
            u.starts_with("LICENSE") || u.starts_with("LICENCE") || u.starts_with("COPYING")
        })
        .collect();
    names.sort();
    names.iter().find_map(|n| {
        let text = std::fs::read_to_string(root.join(n)).ok()?;
        estate::detect_licence(&text)
    })
}

/// Scope per lower-case table name, from the review's scope items.
fn scope_from(items: &[ekos_semantic::review::Item]) -> BTreeMap<String, ScopeEntry> {
    let mut out = BTreeMap::new();
    for i in items {
        let Some(table) = i.facts.get("table").and_then(Value::as_str) else {
            continue;
        };
        let decided = i
            .decision
            .as_ref()
            .and_then(|d| d.get("scope"))
            .and_then(Value::as_str);
        let verdict = decided.or(i.proposed.as_deref()).unwrap_or("in");
        out.insert(
            table.to_ascii_lowercase(),
            ScopeEntry {
                in_scope: verdict == "in",
                class: i
                    .facts
                    .get("class")
                    .and_then(Value::as_str)
                    .unwrap_or("unclassified")
                    .to_string(),
                decided: decided.is_some(),
            },
        );
    }
    out
}

/// Carry each manual section's body from `old` into `new`.
fn keep_manual(new: &str, old: &str) -> String {
    const OPEN: &str = "<!-- ekos:manual id=\"";
    const CLOSE: &str = "<!-- /ekos:manual -->";
    let sections = |text: &str| -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let mut rest = text;
        while let Some(i) = rest.find(OPEN) {
            let after = &rest[i + OPEN.len()..];
            let Some(q) = after.find("\" -->\n") else {
                break;
            };
            let id = after[..q].to_string();
            let body_start = &after[q + "\" -->\n".len()..];
            let Some(end) = body_start.find(CLOSE) else {
                break;
            };
            out.insert(id, body_start[..end].to_string());
            rest = &body_start[end + CLOSE.len()..];
        }
        out
    };
    let old = sections(old);
    if old.is_empty() {
        return new.to_string();
    }
    let mut out = String::with_capacity(new.len());
    let mut rest = new;
    while let Some(i) = rest.find(OPEN) {
        let after = &rest[i + OPEN.len()..];
        let (Some(q), Some(end)) = (after.find("\" -->\n"), after.find(CLOSE)) else {
            break;
        };
        let id = &after[..q];
        out.push_str(&rest[..i + OPEN.len() + q + "\" -->\n".len()]);
        let body = &after[q + "\" -->\n".len()..end];
        out.push_str(old.get(id).map_or(body, String::as_str));
        out.push_str(CLOSE);
        rest = &after[end + CLOSE.len()..];
    }
    out.push_str(rest);
    out
}

/// Under the state directory: the latest estate generation — where, how many pages, when
/// (RFC 0218). The workflow's step 2 reads it.
pub const RECORD: &str = "docs/estate.json";

/// Write `pages` under `out` (RFC 0208's file rules).
pub fn write_pages(out: &Path, pages: &[estate::EstatePage]) -> Result<Written> {
    std::fs::create_dir_all(out).with_context(|| format!("creating {}", out.display()))?;
    let mut w = Written::default();
    let mut produced = BTreeSet::new();
    for p in pages {
        let target = out.join(&p.path);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let old = std::fs::read_to_string(&target).ok();
        let text = match &old {
            Some(o) => keep_manual(&p.body, o),
            None => p.body.clone(),
        };
        if !produced.insert(p.path.clone()) {
            anyhow::bail!("two estate pages share the path {}", p.path);
        }
        if old.as_deref() == Some(text.as_str()) {
            w.unchanged.push(p.path.clone());
            continue;
        }
        std::fs::write(&target, text)?;
        w.written.push(p.path.clone());
    }
    let mut stack = vec![out.to_path_buf()];
    let mut dirs = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let path = e.path();
            if path.is_dir() {
                dirs.push(path.clone());
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(out)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if !rel.ends_with(".md") || produced.contains(&rel) {
                continue;
            }
            let head: String = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .chars()
                .take(400)
                .collect();
            if head.starts_with("---") && head.contains(MARKER) {
                std::fs::remove_file(&path)?;
                w.removed.push(rel);
            }
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        if std::fs::read_dir(&d)
            .map(|mut i| i.next().is_none())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_dir(&d);
        }
    }
    w.written.sort();
    w.unchanged.sort();
    w.removed.sort();
    Ok(w)
}

/// `ekos docs generate --layout estate --output <dir>`
/// RFC 0233: the integrity index as the estate's Findings page shows it — the headline numbers
/// and the error and warning findings (not reachability notes or coverage gaps).
fn integrity_summary(index: &super::check::Index) -> estate::IntegritySummary {
    use ekos_semantic::xref::{DiagnosticKind, Severity};
    let m = &index.report.metrics;
    let pct = |r: Option<f64>| r.map_or_else(|| "—".to_string(), |r| format!("{:.1}%", r * 100.0));
    estate::IntegritySummary {
        computed_at: Some(index.computed_at.format("%Y-%m-%d %H:%M UTC").to_string()),
        resolution: m.resolution_rate,
        cross_system_resolution: m.cross_system_resolution_rate,
        cycles: m.cycles,
        orphans: m.orphans.iter().map(|(k, n)| (k.clone(), *n)).collect(),
        reachability: m
            .reachability
            .iter()
            .map(|(f, r)| {
                let line = match &r.unsupported_because {
                    Some(why) => format!("unsupported: {why}"),
                    None => format!(
                        "{} of {} routines reachable ({}), {} unreachable, {} dynamic ({})",
                        r.reachable,
                        r.symbols,
                        pct(r.reachability_coverage),
                        r.unreachable,
                        r.dynamic,
                        pct(r.dynamic_share)
                    ),
                };
                (f.clone(), line)
            })
            .collect(),
        findings: index
            .report
            .diagnostics
            .iter()
            .filter(|d| d.severity >= Severity::Warning && !d.coverage_gap)
            .filter(|d| d.kind != DiagnosticKind::DynamicReferenceUnknown)
            .take(500)
            .map(|d| estate::IntegrityRow {
                severity: d.severity.as_str().to_string(),
                kind: d.kind.as_str().to_string(),
                target: d.target_spec.written.clone(),
                location: match (&d.source.file, d.source.line) {
                    (Some(f), Some(l)) => format!("{f}:{l}"),
                    (Some(f), None) => f.clone(),
                    _ => format!("{} {}", d.source.object_kind, d.source.object_name),
                },
                reason: d.reason.clone(),
            })
            .collect(),
    }
}

pub fn generate(config: &EkosConfig, cwd: &Path, output: &Path) -> Result<()> {
    let ledger = open_store_read_only(config, cwd).map_err(|e| {
        anyhow::anyhow!("{e}\nRun the pipeline (`ekos build` … `ekos commit`) first.")
    })?;
    let review = super::review::review_state(config, cwd, &*ledger)?;
    let objects = ledger.all_objects()?;
    let relationships = ledger.all_relationships()?;
    let scope = scope_from(review.items(ekos_semantic::review::Subject::Scope));
    let subjects: Vec<SubjectLine> = review
        .statuses
        .iter()
        .map(|s| SubjectLine {
            subject: s.subject.clone(),
            items: s.items,
            blocking: s.blocking,
            state: s.signoff.name().to_string(),
            signed_by: match &s.signoff {
                ekos_semantic::review::SignState::Valid { signed_by, .. }
                | ekos_semantic::review::SignState::Stale { signed_by, .. } => {
                    Some(signed_by.clone())
                }
                _ => None,
            },
        })
        .collect();
    let committed = crate::freshness::read_manifest(
        &config
            .ekos_dir(cwd)
            .join(crate::freshness::COMMITTED_MANIFEST),
    );
    let provenance = Provenance {
        project: config
            .location
            .as_ref()
            .map(|l| l.project.clone())
            .or_else(|| cwd.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "project".into()),
        committed_at: committed
            .as_ref()
            .and_then(|m| m.committed_at)
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string()),
        git_head: committed
            .as_ref()
            .and_then(|m| m.git_head.clone())
            .map(|h| h[..h.len().min(12)].to_string()),
    };
    let licences = licences(config, cwd);
    let conflicts = super::conflicts::current_ids(config, cwd);
    let integrity = super::check::read_index(config, cwd).map(|i| integrity_summary(&i));
    let pages = estate::build(&EstateInput {
        objects: &objects,
        relationships: &relationships,
        scope: &scope,
        subjects: &subjects,
        licences: &licences,
        provenance: &provenance,
        current_conflicts: conflicts.as_ref(),
        max_er_tables: config.docs_estate.max_er_tables.unwrap_or(30).max(2),
        integrity: integrity.as_ref(),
    });
    let w = write_pages(output, &pages)?;
    let record = serde_json::json!({
        "output": crate::home::canonical_dir(output)
            .unwrap_or_else(|_| output.to_path_buf())
            .display()
            .to_string(),
        "pages": pages.len(),
        "written": w.written.len(),
        "unchanged": w.unchanged.len(),
        "removed": w.removed.len(),
        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "committed_at": provenance.committed_at,
    });
    // RFC 0218: the workflow's step 2 reads this record; losing it only hides the step's state.
    let record_path = config.ekos_dir(cwd).join(RECORD);
    if let Some(dir) = record_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&record_path, format!("{record:#}\n")) {
        eprintln!("warning: {}: {e}", record_path.display());
    }
    ekos_common::run_log::emit("docs", "estate pages written", &record);
    println!(
        "estate docs: {} page(s) — {} written, {} unchanged, {} removed → {}",
        pages.len(),
        w.written.len(),
        w.unchanged.len(),
        w.removed.len(),
        output.display()
    );
    for (s, l) in &licences {
        if !estate::licence_allows_text(l) {
            println!(
                "  {s}: licence {l} — its text is left out (set [docs-estate] licences.{s} to use it)"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(path: &str, body: &str) -> estate::EstatePage {
        estate::EstatePage {
            path: path.into(),
            title: path.into(),
            body: format!("---\ntitle: \"x\"\n{MARKER} 1\n---\n\n{body}"),
        }
    }

    #[test]
    fn regeneration_keeps_notes_and_hand_pages_and_removes_stale_estate_pages() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path();
        let note = |b: &str| format!("<!-- ekos:manual id=\"n\" -->\n{b}\n<!-- /ekos:manual -->\n");
        let first = vec![
            page("index.md", &note("_default_")),
            page("data-model/c/c_order.md", "order"),
        ];
        let w = write_pages(out, &first).unwrap();
        assert_eq!(w.written.len(), 2);

        // A person writes a note and adds a page of their own.
        let idx = out.join("index.md");
        let text = std::fs::read_to_string(&idx)
            .unwrap()
            .replace("_default_", "Our ERP.");
        std::fs::write(&idx, text).unwrap();
        std::fs::write(out.join("data-model/hand.md"), "# mine\n").unwrap();

        // The next run no longer produces c_order.
        let second = vec![page("index.md", &note("_default_"))];
        let w = write_pages(out, &second).unwrap();
        assert_eq!(w.removed, vec!["data-model/c/c_order.md".to_string()]);
        assert_eq!(
            w.unchanged,
            vec!["index.md".to_string()],
            "the note is kept, nothing else changed"
        );
        assert!(std::fs::read_to_string(&idx).unwrap().contains("Our ERP."));
        assert!(
            out.join("data-model/hand.md").exists(),
            "a hand-written page is never touched"
        );
        assert!(
            !out.join("data-model/c").exists(),
            "an emptied directory goes"
        );
    }

    #[test]
    fn licences_come_from_the_config_then_the_licence_file() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(
            a.join("LICENSE.md"),
            "GNU General Public License\nVersion 2",
        )
        .unwrap();
        let mut config: EkosConfig = toml::from_str(&format!(
            "[[sources]]\nname = \"a\"\npath = \"{}\"\n[[sources]]\nname = \"b\"\npath = \"{}\"\n\
             [docs-estate]\nlicences = {{ c = \"MIT\" }}\n",
            a.display(),
            b.display()
        ))
        .unwrap();
        config.databases.clear();
        let l = licences(&config, dir.path());
        assert_eq!(l["a"], "GPL-2.0");
        assert_eq!(l["b"], "unknown");
        assert_eq!(l["c"], "MIT");
    }
}
