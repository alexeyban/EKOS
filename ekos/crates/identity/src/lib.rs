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

pub mod similarity;

use std::collections::{HashMap, HashSet};

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

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    /// Minimum combined similarity score to propose a merge. Default: 0.85.
    pub merge_threshold: f32,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            merge_threshold: 0.85,
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
        let mut blocks: HashMap<(String, String), Vec<usize>> = HashMap::new();
        for (i, obj) in objects.iter().enumerate() {
            let norm = similarity::normalize(&obj.name);
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
                    if score.combined >= self.config.merge_threshold {
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
    match (column_names(a), column_names(b)) {
        (Some(cols_a), Some(cols_b)) if !cols_a.is_empty() && !cols_b.is_empty() => {
            jaccard(&cols_a, &cols_b)
        }
        _ => 1.0,
    }
}

fn column_names(obj: &KirObject) -> Option<HashSet<String>> {
    let cols = obj.properties.get("columns")?.as_array()?;
    Some(
        cols.iter()
            .filter_map(|c| c.get("name")?.as_str().map(|s| s.to_lowercase()))
            .collect(),
    )
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f32 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f32 / union as f32
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
}
