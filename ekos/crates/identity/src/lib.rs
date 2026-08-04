//! Identity Resolution — merges synonymous `KirObject`s across sources.
//!
//! See RFC 0007 for the algorithm design.
//!
//! # Usage
//!
//! ```rust,ignore
//! use ekos_identity::{DefaultResolver, IdentityResolver};
//! let result = DefaultResolver::new().resolve(&kir_graph);
//! for proposal in &result.proposals {
//!     println!("merge {:?} → '{}'", proposal.source_ids, proposal.canonical_name);
//! }
//! ```

pub mod cross_system;
pub mod similarity;

use std::collections::HashMap;

use ekos_kir::{KirGraph, KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

/// Name + structural similarity between two candidates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimilarityScore {
    pub name: f32,
    pub structural: f32,
    pub combined: f32,
}

/// A proposed merge of two or more `KirObject`s into one canonical identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeProposal {
    /// ID of the canonical (authoritative) object — the Union-Find root.
    pub canonical_id: KirId,
    /// Chosen canonical name (taken from the Union-Find root object).
    pub canonical_name: String,
    /// Kind shared by all merged objects.
    pub canonical_kind: ObjectKind,
    /// IDs of all objects in this merge group (includes the canonical).
    pub source_ids: Vec<KirId>,
    /// Highest pairwise similarity score within this group.
    pub confidence: f32,
}

/// Type of identity conflict.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Two objects share the same normalised name but have different `ObjectKind`s.
    SameNameDifferentKind,
}

/// An identity conflict that blocks automatic merging.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictReport {
    pub kind: ConflictKind,
    pub ids: Vec<KirId>,
    pub description: String,
}

/// Aggregated counters from one resolution run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResolutionStats {
    pub candidates_evaluated: usize,
    pub pairs_compared: usize,
    pub merges_proposed: usize,
    pub conflicts_detected: usize,
}

/// The full output of one identity resolution pass.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ResolutionResult {
    pub proposals: Vec<MergeProposal>,
    pub conflicts: Vec<ConflictReport>,
    pub stats: ResolutionStats,
}

// ── Trait ─────────────────────────────────────────────────────────────────────

pub trait IdentityResolver: Send + Sync {
    fn resolve(&self, graph: &KirGraph) -> ResolutionResult;
}

// ── DefaultResolver ──────────────────────────────────────────────────────────

/// Stricter merge threshold applied to `Custom("Concept")` objects (RFC 0026).
///
/// Concepts are LLM-extracted from free prose, so they are the highest-cardinality
/// and most name-collision-prone kind this compiler produces: two documents can
/// name unrelated things almost identically. Unlike `Custom("Section")` they must
/// stay mergeable — cross-document concept linking is the whole point of RFC 0026 —
/// so instead of excluding them from resolution they get a higher bar to clear.
pub const CONCEPT_MERGE_THRESHOLD: f32 = 0.95;

/// A `Custom("Concept")` whose normalised name has fewer words than this is
/// never a blocking candidate — see `MIN_CONCEPT_NAME_CHARS`.
const MIN_CONCEPT_NAME_WORDS: usize = 2;

/// A `Custom("Concept")` whose normalised name is shorter than this is never a
/// blocking candidate. Generic short phrases ("data", "the API", "system") name
/// different things in every document that uses them, so they would collapse into
/// one canonical object on name similarity alone — the devlog_27 failure shape.
const MIN_CONCEPT_NAME_CHARS: usize = 8;

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Minimum combined similarity score to propose a merge. Default: 0.85.
    pub merge_threshold: f32,
    /// Per-`ObjectKind` overrides of `merge_threshold`, keyed on the kind's
    /// `Display` form. The lookup itself is kind-agnostic; only the defaults
    /// seeded below know about specific kinds.
    pub kind_thresholds: HashMap<String, f32>,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            merge_threshold: 0.85,
            kind_thresholds: HashMap::from([("Concept".to_string(), CONCEPT_MERGE_THRESHOLD)]),
        }
    }
}

/// Name-similarity resolver using Jaro-Winkler + blocking (RFC 0007).
pub struct DefaultResolver {
    config: ResolverConfig,
}

impl Default for DefaultResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultResolver {
    pub fn new() -> Self {
        Self {
            config: ResolverConfig::default(),
        }
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.config.merge_threshold = threshold;
        self
    }

    /// Override the merge threshold for one `ObjectKind`, named by its `Display`
    /// form (e.g. `"Table"`, `"Concept"`).
    pub fn with_kind_threshold(mut self, kind: impl Into<String>, threshold: f32) -> Self {
        self.config.kind_thresholds.insert(kind.into(), threshold);
        self
    }

    /// Threshold that applies to a given object — its kind's override if one is
    /// configured, otherwise the global `merge_threshold`.
    fn threshold_for(&self, obj: &KirObject) -> f32 {
        self.config
            .kind_thresholds
            .get(&format!("{}", obj.kind))
            .copied()
            .unwrap_or(self.config.merge_threshold)
    }

    fn score(&self, a: &KirObject, b: &KirObject) -> SimilarityScore {
        let na = similarity::normalize(&a.name);
        let nb = similarity::normalize(&b.name);
        let name = similarity::jaro_winkler(&na, &nb);
        let structural = structural_score(a, b);
        let combined = 0.7 * name + 0.3 * structural;
        SimilarityScore {
            name,
            structural,
            combined,
        }
    }
}

impl IdentityResolver for DefaultResolver {
    fn resolve(&self, graph: &KirGraph) -> ResolutionResult {
        let objects = &graph.objects;
        let n = objects.len();
        let mut stats = ResolutionStats {
            candidates_evaluated: n,
            ..Default::default()
        };

        if n < 2 {
            return ResolutionResult {
                stats,
                ..Default::default()
            };
        }

        // ── Conflict detection (all objects, cross-kind) ──────────────────────
        let mut by_norm: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, obj) in objects.iter().enumerate() {
            by_norm
                .entry(similarity::normalize(&obj.name))
                .or_default()
                .push(i);
        }

        let mut conflicts = Vec::new();
        for (norm, indices) in &by_norm {
            if indices.len() < 2 {
                continue;
            }
            let first_kind = &objects[indices[0]].kind;
            let has_kind_mismatch = indices[1..].iter().any(|&i| &objects[i].kind != first_kind);
            if has_kind_mismatch {
                let ids: Vec<KirId> = indices.iter().map(|&i| objects[i].id).collect();
                let kinds: Vec<String> = indices
                    .iter()
                    .map(|&i| format!("{}", objects[i].kind))
                    .collect();
                conflicts.push(ConflictReport {
                    kind: ConflictKind::SameNameDifferentKind,
                    ids,
                    description: format!(
                        "'{norm}' appears as multiple kinds: {}",
                        kinds.join(", ")
                    ),
                });
            }
        }

        // ── Build blocks: (kind_str, first 3 chars of normalised name) ────────
        //
        // `Custom("Section")` objects (RFC 0024 — one per document page/chunk,
        // named "{path}: page {n}") are never resolution candidates: each is
        // already deterministically identified by (document, page/index), so
        // no two distinct Section objects can legitimately represent the same
        // real-world entity. Without this exclusion, pages of the same
        // document share a long name prefix ("{path}: page ") that scores
        // high on Jaro-Winkler, and `structural_score`'s same-kind fallback of
        // 1.0 (no `columns` property to compare) adds a flat +0.3 floor on
        // top — collapsing an entire book's worth of distinct pages into one
        // canonical object and defeating RFC 0024's purpose outright (verified
        // against the real 82-book library: 8,624 raw objects fell to 120
        // after resolution, almost all of it Section over-merging — see
        // devlog 27).
        //
        // `Custom("Concept")` objects (RFC 0026 — LLM-extracted from document
        // prose) are the mirror image: two mentions of the same real concept in
        // different documents *should* merge, so they are not excluded by kind.
        // What is excluded is a degenerate *name*: a normalised name that is one
        // word or under `MIN_CONCEPT_NAME_CHARS` characters ("data", "the API")
        // names something different in every document that uses it, and would
        // reproduce the Section over-merge above on name similarity alone. A
        // concrete concept like "Data Replication" blocks normally, and then has
        // to clear the stricter `CONCEPT_MERGE_THRESHOLD` to actually merge.
        let mut blocks: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (i, obj) in objects.iter().enumerate() {
            // `Custom("TransformNode")` objects (RFC 0027/0028) are the same
            // failure shape as Section, discovered live while smoke-testing
            // Phase 6's demo: each node is named "{source_path}:{index}"
            // (`lower_to_kir`, `crates/semantic/src/transform_ir.rs`), so
            // every node parsed from one file shares a long name prefix, and
            // `structural_score`'s same-kind 1.0 fallback (no `columns`
            // property) collapsed a real 3-node Source→Filter→Sink pipeline
            // into one canonical object at confidence 0.99. Each node is
            // already deterministically identified by (source, node index) —
            // no two distinct TransformNode objects can legitimately be the
            // same real-world entity, so — like Section, unlike Concept —
            // this is a blanket kind exclusion, not a threshold/name-length
            // guard.
            if matches!(&obj.kind, ObjectKind::Custom(k) if k == "Section" || k == "TransformNode")
            {
                continue;
            }
            let norm = similarity::normalize(&obj.name);
            if matches!(&obj.kind, ObjectKind::Custom(k) if k == "Concept")
                && (norm.split_whitespace().count() < MIN_CONCEPT_NAME_WORDS
                    || norm.chars().count() < MIN_CONCEPT_NAME_CHARS)
            {
                continue;
            }
            let prefix: String = norm.chars().take(3).collect();
            blocks
                .entry((format!("{}", obj.kind), prefix))
                .or_default()
                .push(i);
        }

        // ── Pairwise scoring within blocks → Union-Find ───────────────────────
        let mut uf = UnionFind::new(n);
        let mut max_score_per_idx = vec![0.0f32; n];

        for indices in blocks.values() {
            if indices.len() < 2 {
                continue;
            }
            for a in 0..indices.len() {
                for b in (a + 1)..indices.len() {
                    let i = indices[a];
                    let j = indices[b];
                    stats.pairs_compared += 1;
                    let score = self.score(&objects[i], &objects[j]);
                    // Blocks are keyed by kind, so both sides share one threshold.
                    if score.combined >= self.threshold_for(&objects[i]) {
                        uf.union(i, j);
                        max_score_per_idx[i] = max_score_per_idx[i].max(score.combined);
                        max_score_per_idx[j] = max_score_per_idx[j].max(score.combined);
                    }
                }
            }
        }

        // ── Collect merge groups ──────────────────────────────────────────────
        let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            groups.entry(uf.find(i)).or_default().push(i);
        }

        let mut proposals = Vec::new();
        for (root, members) in groups {
            if members.len() < 2 {
                continue;
            }
            let canonical = &objects[root];
            let confidence = members
                .iter()
                .map(|&i| max_score_per_idx[i])
                .fold(0.0f32, f32::max);
            proposals.push(MergeProposal {
                canonical_id: canonical.id,
                canonical_name: canonical.name.clone(),
                canonical_kind: canonical.kind.clone(),
                source_ids: members.iter().map(|&i| objects[i].id).collect(),
                confidence,
            });
            stats.merges_proposed += 1;
        }

        stats.conflicts_detected = conflicts.len();
        ResolutionResult {
            proposals,
            conflicts,
            stats,
        }
    }
}

// ── Structural similarity ───────────────────────────────────────────────────

/// Structural similarity between two objects, used as the 30% non-name term in
/// `DefaultResolver::score`.
///
/// Objects of different `ObjectKind` never match. When both objects carry a
/// `properties["columns"]` array (as SQL-derived `KirObject`s from
/// `parse_ddl_structural` do), structural similarity is the Jaccard overlap of
/// their column-name sets — two tables with almost no columns in common (e.g.
/// `Employees` vs. `EmployeeTerritories`) score near 0 here even when their
/// names are similar, which is what keeps `DefaultResolver` from merging
/// genuinely distinct tables that merely share a name prefix. When column data
/// isn't available for one or both objects (e.g. hand-built `KirObject`s in
/// tests, or non-table kinds), this falls back to the same-kind-only signal
/// (1.0) that was this function's entire behavior before column-overlap
/// scoring was added — so name similarity alone still drives merging in that
/// case, exactly as before.
fn structural_score(a: &KirObject, b: &KirObject) -> f32 {
    if a.kind != b.kind {
        return 0.0;
    }
    match (similarity::column_names(a), similarity::column_names(b)) {
        (Some(cols_a), Some(cols_b)) if !cols_a.is_empty() && !cols_b.is_empty() => {
            similarity::jaccard(&cols_a, &cols_b)
        }
        _ => 1.0,
    }
}

// ── Union-Find ────────────────────────────────────────────────────────────────

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        match self.rank[rx].cmp(&self.rank[ry]) {
            std::cmp::Ordering::Less => self.parent[rx] = ry,
            std::cmp::Ordering::Greater => self.parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                self.parent[ry] = rx;
                self.rank[rx] += 1;
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirGraph, KirObject, ObjectKind};

    fn make_graph(pairs: &[(&str, ObjectKind)]) -> KirGraph {
        let mut g = KirGraph::new();
        for (name, kind) in pairs {
            g.add_object(KirObject::new(*name, kind.clone()));
        }
        g
    }

    #[test]
    fn empty_graph_returns_empty() {
        let g = KirGraph::new();
        let result = DefaultResolver::new().resolve(&g);
        assert!(result.proposals.is_empty());
        assert!(result.conflicts.is_empty());
        assert_eq!(result.stats.candidates_evaluated, 0);
    }

    #[test]
    fn single_object_no_merge() {
        let g = make_graph(&[("Customer", ObjectKind::Table)]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(result.proposals.is_empty());
    }

    #[test]
    fn exact_case_difference_proposes_merge() {
        let g = make_graph(&[
            ("Customer", ObjectKind::Table),
            ("customer", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.proposals.len(), 1, "expected one merge proposal");
        assert_eq!(result.proposals[0].source_ids.len(), 2);
        assert!((result.proposals[0].confidence - 1.0).abs() < 1e-3);
    }

    #[test]
    fn plural_singular_proposes_merge() {
        let g = make_graph(&[("orders", ObjectKind::Table), ("order", ObjectKind::Table)]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "expected merge of 'orders' and 'order'"
        );
        assert!(result.proposals[0].confidence > 0.85);
    }

    #[test]
    fn underscore_variant_proposes_merge() {
        let g = make_graph(&[
            ("customer_table", ObjectKind::Table),
            ("customer", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "expected merge after suffix stripping"
        );
    }

    #[test]
    fn dissimilar_names_no_merge() {
        let g = make_graph(&[
            ("orders", ObjectKind::Table),
            ("products", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "orders and products must not merge"
        );
    }

    fn table_with_columns(name: &str, columns: &[&str]) -> KirObject {
        let cols: Vec<serde_json::Value> = columns
            .iter()
            .map(|c| serde_json::json!({"name": c, "data_type": "text"}))
            .collect();
        KirObject::new(name, ObjectKind::Table)
            .with_property("columns", serde_json::Value::Array(cols))
    }

    #[test]
    fn prefix_sharing_tables_with_disjoint_columns_do_not_merge() {
        // Regression test: "orders" and "order_items" share a name prefix (high
        // Jaro-Winkler score) but are genuinely different tables with almost no
        // column overlap — must not merge. This is the false-positive merge an
        // integration test against a real schema (Northwind: Employees vs.
        // EmployeeTerritories, Customers vs. CustomerDemographics) caught.
        let mut g = KirGraph::new();
        g.add_object(table_with_columns(
            "orders",
            &["id", "customer_id", "order_date"],
        ));
        g.add_object(table_with_columns(
            "order_items",
            &["id", "order_id", "product_id", "quantity"],
        ));
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "orders and order_items share almost no columns and must not merge"
        );
    }

    #[test]
    fn similar_names_with_overlapping_columns_still_merge() {
        // Real near-duplicates (same entity observed from two sources) still merge
        // when their columns substantially overlap, not just their names.
        let mut g = KirGraph::new();
        g.add_object(table_with_columns(
            "customer",
            &["id", "name", "email", "created_at"],
        ));
        g.add_object(table_with_columns(
            "customers",
            &["id", "name", "email", "created_at"],
        ));
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "customer/customers with identical columns should still merge"
        );
    }

    #[test]
    fn different_kind_same_name_conflict() {
        let g = make_graph(&[
            ("customer", ObjectKind::Table),
            ("customer", ObjectKind::Entity),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(
            result.conflicts[0].kind,
            ConflictKind::SameNameDifferentKind
        );
    }

    #[test]
    fn newly_added_object_kind_participates_in_conflict_detection() {
        // AD-001: a new ObjectKind variant (Person) is just as subject to conflict
        // detection as any pre-existing kind — cheap insurance against an
        // exhaustive match being added to this crate later that forgets a variant.
        let g = make_graph(&[("alice", ObjectKind::Person), ("alice", ObjectKind::Table)]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.conflicts.len(), 1);
        assert_eq!(
            result.conflicts[0].kind,
            ConflictKind::SameNameDifferentKind
        );
    }

    /// Real bug, found 2026-08-03 rescanning a mixed-format real-content
    /// workspace with RFC 0025/0026's new mechanics: every file under this
    /// test's fixture directory shared the literal `docs/` path prefix, and
    /// `normalize()` never strips path segments — so the blocking key's
    /// "first 3 normalized chars" rule put every `Document` object in this
    /// workspace into one block regardless of the files' actual names, and
    /// `structural_score`'s same-kind 1.0 fallback (Documents have no
    /// `columns` property) supplied the same free +0.3 floor devlog_27
    /// already diagnosed for Section — except this hits genuinely unrelated
    /// documents (a PDF, a Markdown file, an HTML file, a plain-text file,
    /// an email), not near-duplicate pages of one book. Live repro: 7
    /// entirely different files — two different PDFs, an RFC, a devlog, a
    /// license, a doc-generated HTML page, and an email — collapsed into one
    /// canonical `Document` object at confidence 0.90. `RFC 0024` only
    /// excluded `Custom("Section")` from blocking; `Document` (and, see the
    /// next test, PDF/DOCX-derived `Table`) were never covered and remain
    /// exposed to the exact bug shape devlog_27 already named as
    /// architecturally guaranteed for any kind sharing this structure. This
    /// test currently FAILS — it documents the desired behavior (unrelated
    /// documents in the same folder must not be treated as the same
    /// real-world entity), pending a follow-up fix (candidates: block on the
    /// file basename rather than the full relative path, or don't let
    /// `structural_score` hand out a free floor to kinds with no comparable
    /// structural property at all).
    #[test]
    #[ignore = "known bug, tracked for a follow-up fix — see doc comment above"]
    fn unrelated_documents_sharing_a_folder_prefix_do_not_all_merge() {
        let g = make_graph(&[
            (
                "docs/120 Data Science Interview Questions.pdf",
                ObjectKind::Custom("Document".into()),
            ),
            (
                "docs/41 Essential Machine Learning Interview Questions.pdf",
                ObjectKind::Custom("Document".into()),
            ),
            ("docs/rfc-0026.md", ObjectKind::Custom("Document".into())),
            ("docs/devlog-27.md", ObjectKind::Custom("Document".into())),
            ("docs/license.txt", ObjectKind::Custom("Document".into())),
            (
                "docs/identity-crate-docs.html",
                ObjectKind::Custom("Document".into()),
            ),
            (
                "docs/rollout-note.eml",
                ObjectKind::Custom("Document".into()),
            ),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "seven unrelated documents (different formats, different real content) must not be \
             proposed as one merge group just because they share a `docs/` folder prefix — got: {:?}",
            result.proposals
        );
    }

    /// Same root cause as the test above, one kind over: PDF/DOCX-derived
    /// `Table` objects (`crates/recovery/src/local_docs_analyzer.rs`, named
    /// `"{path}: table {n}"`) share both a long literal path prefix *and*
    /// have no `columns` property (that signal only exists for SQL-derived
    /// tables), so they get the exact same free structural-score floor.
    /// Live repro: 9 distinct tables from one real PDF — different content,
    /// different rows — collapsed into a single canonical `Table` object at
    /// confidence 0.99. This is the identical failure shape RFC 0024 already
    /// fixed for `Custom("Section")`, just on `ObjectKind::Table`, which RFC
    /// 0024 deliberately left untouched because SQL-derived tables need
    /// real fuzzy name dedup across files — so the fix can't be "exclude
    /// Table," it has to distinguish PDF-sourced tables (no `columns`) from
    /// SQL-sourced ones some other way. Currently FAILS; documents the bug.
    #[test]
    #[ignore = "known bug, tracked for a follow-up fix — see doc comment above"]
    fn distinct_pdf_tables_in_one_document_do_not_all_merge() {
        let g = make_graph(&[
            (
                "docs/120 Data Science Interview Questions.pdf: table 1",
                ObjectKind::Table,
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 2",
                ObjectKind::Table,
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 3",
                ObjectKind::Table,
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 4",
                ObjectKind::Table,
            ),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "four distinct tables extracted from one PDF must not collapse into one canonical \
             table just because they share a name prefix and have no `columns` property to \
             compare — got: {:?}",
            result.proposals
        );
    }

    #[test]
    fn three_way_transitivity_single_proposal() {
        let g = make_graph(&[
            ("customer", ObjectKind::Table),
            ("customers", ObjectKind::Table),
            ("customer_table", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "all three should merge into one group"
        );
        assert_eq!(result.proposals[0].source_ids.len(), 3);
    }

    #[test]
    fn stats_counts_pairs_and_candidates() {
        let g = make_graph(&[
            ("orders", ObjectKind::Table),
            ("order", ObjectKind::Table),
            ("products", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.stats.candidates_evaluated, 3);
        assert!(result.stats.pairs_compared >= 1);
    }

    #[test]
    fn custom_threshold_prevents_merge() {
        let g = make_graph(&[("orders", ObjectKind::Table), ("order", ObjectKind::Table)]);
        let result = DefaultResolver::new().with_threshold(0.999).resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "threshold 0.999 should prevent merge"
        );
    }

    #[test]
    fn result_is_serializable() {
        let g = make_graph(&[
            ("Customer", ObjectKind::Table),
            ("customer", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        let json = serde_json::to_string(&result).unwrap();
        let back: ResolutionResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.proposals.len(), result.proposals.len());
    }

    /// Regression test for the real-library finding (RFC 0024, devlog 27):
    /// pages of the same document share a long name prefix ("{path}: page
    /// ") that scores high on Jaro-Winkler even with `structural_score`'s
    /// same-kind 1.0 fallback removed from the picture — `Custom("Section")`
    /// objects must never be merge candidates regardless, since each is
    /// already deterministically identified by (document, page).
    #[test]
    fn section_objects_are_never_merged_even_with_near_identical_names() {
        let section = ObjectKind::Custom("Section".to_string());
        let g = make_graph(&[
            ("Cloud Design Patterns.pdf: page 1", section.clone()),
            ("Cloud Design Patterns.pdf: page 2", section.clone()),
            ("Cloud Design Patterns.pdf: page 213", section),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "Section objects must never be merge candidates, got {:?}",
            result.proposals
        );
    }

    /// Regression test for a real bug found live while smoke-testing Phase 6's
    /// demo (RFC 0027/0028): `Custom("TransformNode")` objects are named
    /// "{source_path}:{index}" (`lower_to_kir`), so every node parsed from one
    /// file shares a long name prefix — the identical failure shape as
    /// `Custom("Section")`. Before the fix, a real 3-node Source→Filter→Sink
    /// pipeline collapsed into one canonical object at confidence 0.99.
    #[test]
    fn transform_node_objects_are_never_merged_even_with_shared_source_prefix() {
        let transform_node = ObjectKind::Custom("TransformNode".to_string());
        let g = make_graph(&[
            ("new_load_customers.sql#0:0", transform_node.clone()),
            ("new_load_customers.sql#0:1", transform_node.clone()),
            ("new_load_customers.sql#0:2", transform_node),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "TransformNode objects must never be merge candidates, got {:?}",
            result.proposals
        );
    }

    /// RFC 0026: the merge this feature exists to produce. The same real-world
    /// concept extracted from two different documents must resolve to one
    /// canonical object, even across case differences — otherwise cross-document
    /// concept linking never happens and the extraction pass is just per-document
    /// noise.
    #[test]
    fn concept_same_real_entity_across_two_documents_merges() {
        let concept = ObjectKind::Custom("Concept".to_string());
        let g = make_graph(&[
            ("Data Replication", concept.clone()),
            ("data replication", concept),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "the same concept named in two documents must merge, got {:?}",
            result.proposals
        );
        assert_eq!(result.proposals[0].source_ids.len(), 2);
    }

    /// RFC 0026, the opposite failure direction (devlog_27's shape applied to
    /// Concepts): a generic short name appears in unrelated documents meaning
    /// unrelated things, and must not collapse every mention into one object.
    /// Phrased as "not all merge" rather than "never merge" — some Concept
    /// merging is the correct outcome, as the test above asserts.
    #[test]
    fn concept_generic_short_names_across_unrelated_documents_do_not_all_merge() {
        let concept = ObjectKind::Custom("Concept".to_string());
        let g = make_graph(&[
            ("the API", concept.clone()),
            ("the API", concept.clone()),
            ("data", concept.clone()),
            ("data", concept),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result
                .proposals
                .iter()
                .all(|p| p.source_ids.len() < g.objects.len()),
            "generic short Concept names must not all collapse into one group, got {:?}",
            result.proposals
        );
        assert!(
            result.proposals.is_empty(),
            "degenerate Concept names are excluded from blocking entirely, got {:?}",
            result.proposals
        );
    }

    /// A non-"Section" `Custom` kind is unaffected by the exclusion — this
    /// pins the fix to the literal string "Section", not `Custom` in
    /// general (e.g. `Custom("Document")`/`Custom("Page")` still resolve
    /// normally).
    #[test]
    fn other_custom_kinds_still_resolve_normally() {
        let doc = ObjectKind::Custom("Document".to_string());
        let g = make_graph(&[("report.pdf", doc.clone()), ("report.pdf", doc)]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "identical-name Documents should still merge"
        );
    }
}
