//! Devlog parsing (RFC 0030). Targets the *real* structure this repo's devlogs use
//! (`CLAUDE.md`'s Devlog Rule), not the illustrative `## Added`/`## Changed`/`## Fixed`
//! example from the source design doc.
//!
//! `## Summary` is the one section verified present across all 28 existing devlogs, so it
//! is the sole source of prose for tweet generation. Second-level headings vary a lot
//! (`## RFC 0025 — ...`, `## Phase 6 — ...`, `## Bug 2 — ...`) — every one of them except the
//! three universal meta-sections (`Summary`, `Knowledge Captured`, `Files Changed`) is kept as
//! a `section_titles` entry for extra signal.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Second-level headings that are always meta/bookkeeping, never feature content.
const META_SECTIONS: &[&str] = &["Summary", "Knowledge Captured", "Files Changed"];

#[derive(Debug, Error)]
pub enum DevlogParseError {
    #[error("no `# Devlog N — <title>` heading found")]
    MissingTitleHeading,
    #[error("no `## Summary` section found")]
    MissingSummary,
}

/// The parsed, tweet-relevant content of one `devlog_N.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevlogSummary {
    pub number: u32,
    pub title: String,
    /// Raw text of the `**Date:**` line, if present.
    pub date: Option<String>,
    /// Body of the `## Summary` section, trimmed.
    pub summary: String,
    /// Every `## ` heading other than the universal meta-sections.
    pub section_titles: Vec<String>,
}

/// Parse a devlog's raw Markdown. `filename_hint` is used to recover `number` when the
/// `# Devlog N` heading is missing or unparseable (defensive; every real devlog has it).
pub fn parse(text: &str, filename_hint: Option<&str>) -> Result<DevlogSummary, DevlogParseError> {
    let lines: Vec<&str> = text.lines().collect();

    let title_line = lines
        .iter()
        .find(|l| l.trim_start().starts_with("# ") && !l.trim_start().starts_with("## "))
        .ok_or(DevlogParseError::MissingTitleHeading)?;

    let heading = title_line.trim_start().trim_start_matches("# ").trim();
    // "Devlog 28 — RFC 0025/0026: more document formats, ..." — split on the first em-dash
    // (or plain "-") separating the number from the human title.
    let (number_part, title) = split_once_any_dash(heading);
    let number = number_part
        .split_whitespace()
        .find_map(|tok| tok.parse::<u32>().ok())
        .or_else(|| filename_hint.and_then(number_from_filename))
        .ok_or(DevlogParseError::MissingTitleHeading)?;

    let date = lines
        .iter()
        .find(|l| l.trim_start().starts_with("**Date:**"))
        .map(|l| {
            l.trim_start()
                .trim_start_matches("**Date:**")
                .trim()
                .to_string()
        });

    let summary = extract_section(&lines, "Summary").ok_or(DevlogParseError::MissingSummary)?;

    let section_titles = lines
        .iter()
        .filter_map(|l| {
            let trimmed = l.trim_start();
            if trimmed.starts_with("## ") {
                Some(trimmed.trim_start_matches("## ").trim().to_string())
            } else {
                None
            }
        })
        .filter(|title| !META_SECTIONS.contains(&title.as_str()))
        .collect();

    Ok(DevlogSummary {
        number,
        title: title.trim().to_string(),
        date,
        summary,
        section_titles,
    })
}

/// Split "28 — RFC 0025/0026: ..." into ("28 ", "RFC 0025/0026: ...") on the first em-dash,
/// falling back to a plain hyphen if no em-dash is present.
fn split_once_any_dash(heading: &str) -> (&str, &str) {
    if let Some(idx) = heading.find('\u{2014}') {
        let (a, b) = heading.split_at(idx);
        (a, &b['\u{2014}'.len_utf8()..])
    } else if let Some((a, b)) = heading.split_once(" - ") {
        (a, b)
    } else {
        (heading, "")
    }
}

/// Body of a `## <name>` section: everything up to the next `## ` heading or a `---` rule.
fn extract_section(lines: &[&str], name: &str) -> Option<String> {
    let start = lines.iter().position(|l| {
        l.trim_start().trim_start_matches("## ").trim() == name && l.trim_start().starts_with("## ")
    })?;

    let mut body = Vec::new();
    for line in &lines[start + 1..] {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") || trimmed == "---" {
            break;
        }
        body.push(*line);
    }

    let joined = body.join("\n").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

/// Extract `N` from a `devlog_N.md`-shaped filename (path or bare name).
pub fn number_from_filename(name: &str) -> Option<u32> {
    let stem = Path::new(name).file_stem()?.to_str()?;
    stem.strip_prefix("devlog_")?.parse().ok()
}

/// Find the highest-numbered `devlog_N.md` directly inside `dir`.
pub fn find_latest(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?.to_string();
            let n = number_from_filename(&name)?;
            Some((n, path))
        })
        .max_by_key(|(n, _)| *n)
        .map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real structure per `CLAUDE.md`'s Devlog Rule and `devlog_28.md`.
    const FIXTURE: &str = r#"# Devlog 28 — RFC 0025/0026: more document formats, and real semantic memory for AI tools

**Date:** 2026-08-03
**PRs:** worked on `main` (single session, follow-up to devlog 25-27 — RFC 0023/0024)
**Branch:** main

---

## Summary

The user's ask was blunt: "extract semantics from PDF and other text documents to provide memory
for AI tools." This session closed both real gaps: RFC 0025 extends document parsing, RFC 0026
is the deferred semantic mechanism. 166 tests across five crates, all green.

---

## RFC 0025 — Additional Document Formats

### What was built

Some content here.

---

## RFC 0026 — LLM Document-Semantics Extraction Pass

### What was built

More content.

---

## Knowledge Captured

Some knowledge.

---

## Files Changed

| File | Change |
|---|---|
"#;

    #[test]
    fn parses_number_title_date_from_real_format() {
        let d = parse(FIXTURE, Some("devlog_28.md")).unwrap();
        assert_eq!(d.number, 28);
        assert_eq!(
            d.title,
            "RFC 0025/0026: more document formats, and real semantic memory for AI tools"
        );
        assert_eq!(d.date.as_deref(), Some("2026-08-03"));
    }

    #[test]
    fn extracts_summary_body_only() {
        let d = parse(FIXTURE, None).unwrap();
        assert!(d.summary.starts_with("The user's ask was blunt"));
        assert!(d.summary.contains("166 tests across five crates"));
        // Must stop before the next heading, not swallow the whole file.
        assert!(!d.summary.contains("RFC 0025 — Additional Document Formats"));
    }

    #[test]
    fn section_titles_excludes_meta_sections_only() {
        let d = parse(FIXTURE, None).unwrap();
        assert_eq!(
            d.section_titles,
            vec![
                "RFC 0025 — Additional Document Formats".to_string(),
                "RFC 0026 — LLM Document-Semantics Extraction Pass".to_string(),
            ]
        );
        assert!(!d.section_titles.iter().any(|t| t == "Summary"));
        assert!(!d.section_titles.iter().any(|t| t == "Knowledge Captured"));
        assert!(!d.section_titles.iter().any(|t| t == "Files Changed"));
    }

    #[test]
    fn missing_summary_is_an_error() {
        let text = "# Devlog 1 — Title\n\n**Date:** 2026-01-01\n\n## Files Changed\n\nnone\n";
        assert!(matches!(
            parse(text, None),
            Err(DevlogParseError::MissingSummary)
        ));
    }

    #[test]
    fn missing_title_heading_is_an_error() {
        let text = "## Summary\n\nsomething happened\n";
        assert!(matches!(
            parse(text, None),
            Err(DevlogParseError::MissingTitleHeading)
        ));
    }

    #[test]
    fn number_from_filename_handles_path_and_bare_name() {
        assert_eq!(number_from_filename("devlog_28.md"), Some(28));
        assert_eq!(number_from_filename("/repo/root/devlog_5.md"), Some(5));
        assert_eq!(number_from_filename("README.md"), None);
    }

    #[test]
    fn find_latest_picks_highest_numbered_devlog() {
        let dir = tempfile::tempdir().unwrap();
        for n in [1, 5, 28, 9] {
            std::fs::write(dir.path().join(format!("devlog_{n}.md")), "content").unwrap();
        }
        std::fs::write(dir.path().join("README.md"), "not a devlog").unwrap();

        let latest = find_latest(dir.path()).unwrap();
        assert_eq!(latest.file_name().unwrap(), "devlog_28.md");
    }

    #[test]
    fn find_latest_returns_none_when_no_devlogs_exist() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_latest(dir.path()).is_none());
    }
}
