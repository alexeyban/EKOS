//! RFC 0152 — coverage: did each input kind actually compile into anything?
//!
//! `ekos build`, `recover`, `compile` and `commit` all report how much they *read*. None of them
//! reports how much they *produced*, per source kind, and none of them fails when a kind
//! produces nothing. That gap is the single most expensive failure class this project has:
//!
//! | incident | outcome |
//! |---|---|
//! | `analytics`, missing dialect rule (devlog_177) | whole schema gone, one buried `SQL001` |
//! | LedgerSMB on the correct `postgres` dialect (RFC 0146) | 0 of 103 tables |
//! | `paths` listing subdirectories | 0 git commits, no error |
//! | first real .NET app (devlog_190) | calls joined 0 of 36k, unnoticed |
//!
//! Every one exited `0` and printed a cheerful summary. This module joins what
//! [`crate::detect`] found in the tree against what the compiler actually put in the CKM, and
//! treats `files > 0 && objects == 0` as a finding with a named likely cause.

use crate::detect::{Classifier, Detection, SourceKind};
use ekos_semantic::CkModel;
use std::collections::{BTreeMap, BTreeSet};

/// What happened to one source kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    /// Inputs present, objects produced. Normal.
    Ok,
    /// Objects produced, but no relationships at all — the devlog_190 shape, where a .NET
    /// call graph joined 0 of 36k edges while every type and method compiled fine.
    NoEdges,
    /// Inputs present, nothing compiled. The failure this module exists for.
    ZeroCoverage,
    /// No inputs of this kind. Not a finding.
    NoInput,
}

impl CoverageStatus {
    /// Worst first, for ordering findings.
    fn severity(self) -> u8 {
        match self {
            CoverageStatus::ZeroCoverage => 0,
            CoverageStatus::NoEdges => 1,
            CoverageStatus::Ok => 2,
            CoverageStatus::NoInput => 3,
        }
    }

    pub fn is_finding(self) -> bool {
        matches!(self, CoverageStatus::ZeroCoverage | CoverageStatus::NoEdges)
    }

    pub fn label(self) -> &'static str {
        match self {
            CoverageStatus::Ok => "ok",
            CoverageStatus::NoEdges => "no-edges",
            CoverageStatus::ZeroCoverage => "ZERO",
            CoverageStatus::NoInput => "no-input",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageRow {
    pub kind: SourceKind,
    pub files_present: usize,
    pub objects_produced: usize,
    pub relationships_produced: usize,
    pub status: CoverageStatus,
}

#[derive(Debug, Clone, Default)]
pub struct CoverageReport {
    pub rows: Vec<CoverageRow>,
    pub total_objects: usize,
    pub total_relationships: usize,
    /// Objects whose evidence names no path this build recognises, plus objects with no evidence
    /// at all (a concentration `Risk` synthesized in `compile`, RFC 0094). Reported as a
    /// footnote and never hidden — an unattributable object is itself a signal.
    pub unattributed_objects: usize,
}

impl CoverageReport {
    /// Rows that represent a real finding, worst first.
    pub fn findings(&self) -> Vec<&CoverageRow> {
        let mut out: Vec<&CoverageRow> =
            self.rows.iter().filter(|r| r.status.is_finding()).collect();
        out.sort_by_key(|r| (r.status.severity(), r.kind));
        out
    }

    /// True if any kind had inputs and produced nothing — what `--strict` exits non-zero on.
    pub fn has_zero_coverage(&self) -> bool {
        self.rows
            .iter()
            .any(|r| r.status == CoverageStatus::ZeroCoverage)
    }

    /// Rows worth printing by default: everything except kinds with no inputs.
    pub fn visible_rows(&self) -> Vec<&CoverageRow> {
        self.rows
            .iter()
            .filter(|r| r.status != CoverageStatus::NoInput)
            .collect()
    }
}

/// Which source kinds an object's evidence points at.
///
/// An object can legitimately draw evidence from two kinds — a `Table` fused by identity
/// resolution from both hand-written DDL and a dbt model (RFC 0117) is the designed case — so
/// this returns a set and the object is counted once in each row. Rows therefore may sum above
/// `total_objects`; the report says so rather than picking an arbitrary primary kind and
/// under-reporting the other.
fn kinds_of<'a, I>(sources: I, classifier: &Classifier) -> BTreeSet<SourceKind>
where
    I: Iterator<Item = &'a str>,
{
    sources.filter_map(|s| classifier.classify(s)).collect()
}

/// Pure: join a detection against a compiled model.
pub fn compute_coverage(detection: &Detection, model: &CkModel) -> CoverageReport {
    let classifier = detection.classifier();

    let mut objects: BTreeMap<SourceKind, usize> = BTreeMap::new();
    let mut relationships: BTreeMap<SourceKind, usize> = BTreeMap::new();
    let mut unattributed = 0usize;

    for obj in &model.objects {
        let kinds = kinds_of(obj.evidence.iter().map(|e| e.source.as_str()), &classifier);
        if kinds.is_empty() {
            unattributed += 1;
            continue;
        }
        for k in kinds {
            *objects.entry(k).or_insert(0) += 1;
        }
    }

    for rel in &model.relationships {
        let kinds = kinds_of(rel.evidence.iter().map(|e| e.source.as_str()), &classifier);
        for k in kinds {
            *relationships.entry(k).or_insert(0) += 1;
        }
    }

    let rows = SourceKind::all()
        .iter()
        .map(|kind| {
            let files_present = detection.file_count(*kind);
            let objects_produced = objects.get(kind).copied().unwrap_or(0);
            let relationships_produced = relationships.get(kind).copied().unwrap_or(0);
            let status = classify_status(files_present, objects_produced, relationships_produced);
            CoverageRow {
                kind: *kind,
                files_present,
                objects_produced,
                relationships_produced,
                status,
            }
        })
        .collect();

    CoverageReport {
        rows,
        total_objects: model.objects.len(),
        total_relationships: model.relationships.len(),
        unattributed_objects: unattributed,
    }
}

/// The verdict for one row.
///
/// Deliberately three states and no fourth. There is no percentage at which "some objects
/// missing" becomes a warning — RFC 0060 settled this project's position on thresholds when no
/// confidence cutoff could separate correct from incorrect identity merges, and the same holds
/// here. Zero is the one non-arbitrary signal, and zero is what every real incident produced.
fn classify_status(files: usize, objects: usize, relationships: usize) -> CoverageStatus {
    match (files, objects, relationships) {
        (0, _, _) => CoverageStatus::NoInput,
        (_, 0, _) => CoverageStatus::ZeroCoverage,
        // An edge needs two endpoints, so a kind that compiled exactly one object has nothing
        // it could relate — reporting that as a finding would be noise on every small
        // workspace. Two or more objects and nothing relating any of them is the devlog_190
        // shape and worth naming. This is structural, not a tuned threshold: there is no
        // number here to get wrong.
        (_, objects, 0) if objects >= 2 => CoverageStatus::NoEdges,
        _ => CoverageStatus::Ok,
    }
}

/// Why a kind might compile objects and no relationships.
///
/// Deliberately not [`SourceKind::zero_coverage_hint`]: that text explains why *nothing* was
/// produced, and printing it next to "1 object compiled" describes a failure that did not
/// happen — the same misleading-signal problem this module exists to end.
const NO_EDGES_HINT: &str = "objects compiled but nothing relates them. For a call or \
     dependency graph this is the shape devlog_190 found on a real .NET application (calls \
     joined 0 of 36k): the objects are fine and the join that should connect them produced \
     nothing. Check .ekos/diagnostics/recover.log for the analyzer that owns this kind.";

/// The likely cause for a finding row, matched to what actually happened.
pub fn hint_for(row: &CoverageRow) -> &'static str {
    match row.status {
        CoverageStatus::NoEdges => NO_EDGES_HINT,
        _ => row.kind.zero_coverage_hint(),
    }
}

/// Render the report as the text table `ekos coverage` prints.
pub fn render(report: &CoverageReport, show_all: bool) -> String {
    let mut out = String::new();
    out.push_str("Coverage — inputs present vs. objects compiled\n\n");
    out.push_str("  kind                     files   objects   edges   status\n");
    out.push_str("  ------------------------------------------------------------\n");

    let rows: Vec<&CoverageRow> = if show_all {
        report.rows.iter().collect()
    } else {
        report.visible_rows()
    };

    if rows.is_empty() {
        out.push_str("  (no recoverable inputs found in this workspace)\n");
    }

    for r in rows {
        out.push_str(&format!(
            "  {:<22} {:>6}   {:>7}   {:>5}   {}\n",
            r.kind.label(),
            r.files_present,
            r.objects_produced,
            r.relationships_produced,
            r.status.label(),
        ));
    }

    out.push_str(&format!(
        "\n  {} object(s), {} relationship(s) in the model.\n",
        report.total_objects, report.total_relationships
    ));
    out.push_str(
        "  Rows may sum above the model total: one object can carry evidence from two kinds.\n",
    );
    if report.unattributed_objects > 0 {
        out.push_str(&format!(
            "  {} object(s) had no evidence path attributable to a known source kind.\n",
            report.unattributed_objects
        ));
    }

    let findings = report.findings();
    if findings.is_empty() {
        out.push_str("\nNo findings — every input kind produced objects.\n");
        return out;
    }

    out.push_str(&format!("\n{} finding(s):\n", findings.len()));
    for r in findings {
        match r.status {
            CoverageStatus::ZeroCoverage => {
                out.push_str(&format!(
                    "\n  ZERO  {} — {} file(s) present, 0 objects compiled.\n",
                    r.kind.label(),
                    r.files_present
                ));
            }
            CoverageStatus::NoEdges => {
                out.push_str(&format!(
                    "\n  EDGES {} — {} object(s) but 0 relationships.\n",
                    r.kind.label(),
                    r.objects_produced
                ));
            }
            _ => continue,
        }
        let hint = match r.status {
            CoverageStatus::NoEdges => NO_EDGES_HINT,
            _ => r.kind.zero_coverage_hint(),
        };
        for line in wrap(hint, 82) {
            out.push_str(&format!("        {line}\n"));
        }
    }

    out
}

/// One-line-per-finding summary for the tail of `ekos compile`.
///
/// Empty when there is nothing to say: a clean run must stay quiet, or the report becomes noise
/// that people learn to scroll past — which is how a buried `SQL001` failed to warn anyone.
pub fn render_compile_tail(report: &CoverageReport) -> String {
    let findings = report.findings();
    if findings.is_empty() {
        return String::new();
    }

    let mut out = String::from("\n  Coverage findings:\n");
    for r in findings {
        match r.status {
            CoverageStatus::ZeroCoverage => out.push_str(&format!(
                "    ZERO  {}: {} file(s) present, 0 objects compiled\n",
                r.kind.label(),
                r.files_present
            )),
            CoverageStatus::NoEdges => out.push_str(&format!(
                "    EDGES {}: {} object(s), 0 relationships\n",
                r.kind.label(),
                r.objects_produced
            )),
            _ => {}
        }
    }
    out.push_str("    Run `ekos coverage` for the likely cause of each.\n");
    out
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + word.len() + 1 > width {
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
    use crate::detect::DetectedSource;
    use ekos_kir::{KirId, ObjectKind, RelationshipKind};
    use ekos_semantic::{CkmObject, CkmRelationship, EvidenceRecord};

    fn detection(kinds: &[(SourceKind, usize)]) -> Detection {
        Detection {
            sources: kinds
                .iter()
                .map(|(kind, count)| DetectedSource {
                    kind: *kind,
                    file_count: *count,
                    sample_paths: Vec::new(),
                })
                .collect(),
            ..Detection::default()
        }
    }

    fn evidence(source: &str) -> EvidenceRecord {
        EvidenceRecord {
            id: KirId::new(),
            source: source.to_string(),
            line: None,
            fragment: String::new(),
            confidence: 1.0,
        }
    }

    fn object(sources: &[&str]) -> CkmObject {
        CkmObject {
            id: KirId::new(),
            name: "o".into(),
            kind: ObjectKind::Table,
            properties: Default::default(),
            primary_description: None,
            evidence: sources.iter().map(|s| evidence(s)).collect(),
            source_artifact_ids: Vec::new(),
        }
    }

    fn relationship(sources: &[&str]) -> CkmRelationship {
        CkmRelationship {
            id: KirId::new(),
            kind: RelationshipKind::DependsOn,
            from: KirId::new(),
            to: KirId::new(),
            properties: Default::default(),
            evidence: sources.iter().map(|s| evidence(s)).collect(),
            source_artifact_ids: Vec::new(),
        }
    }

    fn model(objects: Vec<CkmObject>, relationships: Vec<CkmRelationship>) -> CkModel {
        CkModel {
            version: 1,
            compiled_at: chrono::Utc::now(),
            objects,
            relationships,
            evidence_index: Default::default(),
        }
    }

    /// The core join: the same workspace, one kind healthy and one kind silently dead.
    #[test]
    fn a_kind_with_inputs_and_no_objects_is_zero_coverage() {
        let d = detection(&[(SourceKind::Sql, 103), (SourceKind::Python, 40)]);
        let m = model(vec![object(&["db/schema.sql"])], vec![]);

        let report = compute_coverage(&d, &m);
        let sql = report
            .rows
            .iter()
            .find(|r| r.kind == SourceKind::Sql)
            .unwrap();
        let py = report
            .rows
            .iter()
            .find(|r| r.kind == SourceKind::Python)
            .unwrap();

        assert_eq!(sql.objects_produced, 1);
        assert_eq!(py.status, CoverageStatus::ZeroCoverage);
        assert_eq!(py.files_present, 40);
        assert!(report.has_zero_coverage());
    }

    #[test]
    fn a_kind_with_no_inputs_is_not_a_finding() {
        let report = compute_coverage(&detection(&[]), &model(vec![], vec![]));
        assert!(!report.has_zero_coverage());
        assert!(report.findings().is_empty());
        assert!(
            report
                .rows
                .iter()
                .all(|r| r.status == CoverageStatus::NoInput),
            "an empty workspace is not a failure"
        );
    }

    /// devlog_190: every .NET type and method compiled, and the call graph joined 0 of 36k.
    #[test]
    fn several_objects_without_relationships_are_reported_as_no_edges() {
        let d = detection(&[(SourceKind::Rust, 10)]);
        let m = model(
            vec![object(&["src/main.rs"]), object(&["src/lib.rs"])],
            vec![],
        );

        let row = compute_coverage(&d, &m)
            .rows
            .into_iter()
            .find(|r| r.kind == SourceKind::Rust)
            .unwrap();
        assert_eq!(row.status, CoverageStatus::NoEdges);
    }

    /// An edge needs two endpoints: one object with no relationships is not a finding, and
    /// reporting it as one would fire on every small workspace until people stopped reading.
    #[test]
    fn a_single_object_with_no_relationships_is_not_a_finding() {
        let d = detection(&[(SourceKind::Rust, 1)]);
        let m = model(vec![object(&["src/main.rs"])], vec![]);

        let report = compute_coverage(&d, &m);
        let row = report
            .rows
            .iter()
            .find(|r| r.kind == SourceKind::Rust)
            .unwrap();
        assert_eq!(row.status, CoverageStatus::Ok);
        assert!(report.findings().is_empty());
    }

    /// The hint must describe what actually happened.
    #[test]
    fn a_no_edges_finding_does_not_print_the_zero_coverage_explanation() {
        let d = detection(&[(SourceKind::Git, 1)]);
        let m = model(
            vec![object(&["git:commit:a"]), object(&["git:commit:b"])],
            vec![],
        );
        let rendered = render(&compute_coverage(&d, &m), false);

        assert!(rendered.contains("EDGES"));
        assert!(
            !rendered.contains("yields zero commits"),
            "printing the zero-coverage cause next to '2 objects compiled' describes a failure              that did not happen: {rendered}"
        );
        assert!(
            rendered.contains("36k"),
            "the real shape should be named: {rendered}"
        );
    }

    #[test]
    fn relationships_are_attributed_by_their_own_evidence() {
        let d = detection(&[(SourceKind::Rust, 10)]);
        let m = model(
            vec![object(&["src/main.rs"])],
            vec![relationship(&["src/main.rs"])],
        );

        let row = compute_coverage(&d, &m)
            .rows
            .into_iter()
            .find(|r| r.kind == SourceKind::Rust)
            .unwrap();
        assert_eq!(row.status, CoverageStatus::Ok);
        assert_eq!(row.relationships_produced, 1);
    }

    /// The overlap the report footer promises is real, and asserted rather than tolerated.
    #[test]
    fn an_object_with_evidence_from_two_kinds_counts_in_both_rows() {
        let d = detection(&[(SourceKind::Sql, 1), (SourceKind::Python, 1)]);
        let m = model(vec![object(&["db/schema.sql", "app/models.py"])], vec![]);

        let report = compute_coverage(&d, &m);
        let sql = report
            .rows
            .iter()
            .find(|r| r.kind == SourceKind::Sql)
            .unwrap();
        let py = report
            .rows
            .iter()
            .find(|r| r.kind == SourceKind::Python)
            .unwrap();

        assert_eq!(sql.objects_produced, 1);
        assert_eq!(py.objects_produced, 1);
        assert_eq!(report.total_objects, 1, "one object, counted in two rows");
        let summed: usize = report.rows.iter().map(|r| r.objects_produced).sum();
        assert!(summed > report.total_objects);
    }

    #[test]
    fn an_object_with_no_usable_evidence_is_counted_as_unattributed() {
        let d = detection(&[(SourceKind::Sql, 1)]);
        let m = model(
            vec![
                object(&["db/schema.sql"]),
                object(&[]),
                object(&["LICENSE"]),
            ],
            vec![],
        );

        let report = compute_coverage(&d, &m);
        assert_eq!(report.unattributed_objects, 2);
        assert_eq!(report.total_objects, 3);
    }

    /// Git evidence is a `git:commit:<sha>` pseudo-path, not a file.
    #[test]
    fn git_evidence_attributes_to_the_git_row() {
        let d = detection(&[(SourceKind::Git, 1)]);
        let m = model(
            vec![object(&["git:commit:abc123"])],
            vec![relationship(&["git:contributors"])],
        );

        let row = compute_coverage(&d, &m)
            .rows
            .into_iter()
            .find(|r| r.kind == SourceKind::Git)
            .unwrap();
        assert_eq!(row.status, CoverageStatus::Ok);
        assert_eq!(row.objects_produced, 1);
    }

    #[test]
    fn findings_are_ordered_worst_first() {
        let d = detection(&[(SourceKind::Sql, 5), (SourceKind::Rust, 5)]);
        // Rust produces objects but no edges; SQL produces nothing at all.
        let m = model(
            vec![object(&["src/main.rs"]), object(&["src/lib.rs"])],
            vec![],
        );

        let report = compute_coverage(&d, &m);
        let findings = report.findings();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].status, CoverageStatus::ZeroCoverage);
        assert_eq!(findings[1].status, CoverageStatus::NoEdges);
    }

    #[test]
    fn a_zero_coverage_row_renders_its_named_cause_not_generic_advice() {
        let d = detection(&[(SourceKind::Sql, 103)]);
        let rendered = render(&compute_coverage(&d, &model(vec![], vec![])), false);

        assert!(rendered.contains("ZERO"));
        assert!(
            rendered.contains("default-dialect"),
            "the SQL hint must name the actual config key: {rendered}"
        );
        assert!(rendered.contains("SQL001"));
    }

    #[test]
    fn a_clean_run_prints_no_compile_tail_at_all() {
        let d = detection(&[(SourceKind::Rust, 1)]);
        let m = model(
            vec![object(&["src/main.rs"])],
            vec![relationship(&["src/main.rs"])],
        );
        assert_eq!(
            render_compile_tail(&compute_coverage(&d, &m)),
            "",
            "a healthy run must stay silent or the report becomes noise people scroll past"
        );
    }

    #[test]
    fn the_compile_tail_names_each_finding_and_points_at_the_command() {
        let d = detection(&[(SourceKind::Sql, 103)]);
        let tail = render_compile_tail(&compute_coverage(&d, &model(vec![], vec![])));
        assert!(tail.contains("ZERO  SQL"));
        assert!(tail.contains("ekos coverage"));
    }

    /// Hidden rows are hidden, not deleted: `--all` must still show them.
    /// A hint that points at a file which does not exist is the RFC 0076 bug returning: the old
    /// "(check logs)" message named nothing real, so nobody could act on it.
    #[test]
    fn every_hint_naming_a_diagnostics_log_names_the_real_filename() {
        let mut hints: Vec<&str> = SourceKind::all()
            .iter()
            .map(|k| k.zero_coverage_hint())
            .collect();
        hints.push(NO_EDGES_HINT);

        for hint in hints {
            if !hint.contains("diagnostics/") {
                continue;
            }
            assert!(
                hint.contains(".ekos/diagnostics/recover.log"),
                "write_diagnostics_log writes `<command>.log`, so a glob or a different name                  sends the reader somewhere that does not exist: {hint}"
            );
        }
    }

    #[test]
    fn show_all_includes_kinds_with_no_inputs() {
        let report = compute_coverage(&detection(&[]), &model(vec![], vec![]));
        assert!(!render(&report, false).contains("no-input"));
        assert!(render(&report, true).contains("no-input"));
    }
}
