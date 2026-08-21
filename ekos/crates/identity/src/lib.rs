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
    /// `true` only if every member of this group shares the exact same
    /// `name_for_similarity` after normalization (RFC 0063).
    ///
    /// RFC 0060 found that no single confidence threshold on the current scoring formula
    /// separates every real known-good merge from every real known-wrong one — they interleave
    /// (e.g. two distinct real pipelines, `Build Private Images GHCR`/`Build Public Images GHCR`,
    /// score 0.9277, higher than the known-correct `Adam Rutkowski`/`Adam` merge at 0.9000).
    /// Every one of RFC 0060's residual known-wrong pairs is a *fuzzy* name match, but so is
    /// every one of its known-*correct* merges — fuzzy matching itself is the judgment call, not
    /// a threshold band. Exact-vs-fuzzy is the dividing line that is actually safe to automate on:
    /// two objects with the literal same normalized name carry none of that ambiguity. A group is
    /// "exact" only if **every** member matches the canonical exactly — a group that rode in on
    /// one exact and one fuzzy pairwise link (via Union-Find transitivity) is conservatively
    /// treated as fuzzy, since transitivity can chain a safe pair to an unsafe one.
    pub exact_name_match: bool,
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

/// Default merge threshold (RFC 0060). Was 0.85 through RFC 0007; raised here after real,
/// unedited `analytics/` (Plausible Analytics) data showed 0.85 auto-merges genuinely distinct
/// real objects across every kind that reaches `structural_score`'s "no comparable data" fallback
/// (`Table`, `Person`, `Document`, `Pipeline` alike — see `structural_score`'s doc comment) — not
/// a kind-specific defect, a property of the 0.85 operating point itself. Verified against 16 real
/// pairs read directly from `analytics/`'s git history and compiled schema (8 `Person` merge
/// proposals — 3 genuinely the same contributor under different git author names/usernames, 5
/// genuinely different real people; 5 `Table` proposals sharing a common base-schema "spine"; 2
/// `Document` proposals; 2 `Pipeline` proposals): at 0.85, 16 of 17 known-wrong real merges pass
/// (only the one with the lowest structural overlap, `shield_rules_country`/`shield_rules_ip`,
/// happens to already fail); raising to 0.90 keeps all 3 known-correct merges intact while
/// rejecting 14 of the 17 known-wrong ones. **Not a complete fix** — 3 of the 17 (pairs whose
/// names alone score higher than some of the correct merges' names, e.g. `Build Private Images
/// GHCR`/`Build Public Images GHCR`) still incorrectly clear even 0.90, and no single threshold on
/// this two-term formula separates every real case tested (the known-correct and known-wrong
/// combined scores genuinely interleave above 0.90) — documented honestly in RFC 0060 rather than
/// tuned further on a 17-example sample. The residual cases are exactly the class of judgment call
/// RFC 0029's cross-system `unconfirmed`-until-reviewed flow already exists for; extending that
/// review step to same-source merges too is future work, not done here.
pub const DEFAULT_MERGE_THRESHOLD: f32 = 0.90;

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
    /// Minimum combined similarity score to propose a merge. Default: `DEFAULT_MERGE_THRESHOLD`.
    pub merge_threshold: f32,
    /// Per-`ObjectKind` overrides of `merge_threshold`, keyed on the kind's
    /// `Display` form. The lookup itself is kind-agnostic; only the defaults
    /// seeded below know about specific kinds.
    pub kind_thresholds: HashMap<String, f32>,
}

impl Default for ResolverConfig {
    fn default() -> Self {
        Self {
            merge_threshold: DEFAULT_MERGE_THRESHOLD,
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
        let na = similarity::normalize(name_for_similarity(a));
        let nb = similarity::normalize(name_for_similarity(b));
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
            //
            // `Custom("RustSymbol")`/`Custom("RustModule")` (RFC 0041) and
            // `Custom("PythonSymbol")`/`Custom("PythonModule")` (RFC 0038/0040) are the same
            // failure shape again, discovered live while real-data-testing RFC 0041 against this
            // repo's own ~50-crate workspace: many analyzer passes are literally named
            // `<X>AnalyzerPass` (`ConfluenceAnalyzerPass`, `PentahoAnalyzerPass`,
            // `PythonAnalyzerPass`, ...), so their long shared suffix scores high on Jaro-Winkler,
            // and `structural_score`'s same-kind 1.0 fallback (no `columns` property) pushed
            // several genuinely distinct structs in different files over the 0.85 merge threshold
            // — e.g. `ConfluenceAnalyzerPass` (confluence_analyzer.rs) and `PentahoAnalyzerPass`
            // (pentaho_analyzer.rs) collapsed into one canonical object, silently dropping the
            // other from the ledger even though `resolve`'s cross-kind conflict detector (above)
            // never flagged anything, since these merges are same-kind. Each of these objects is
            // already deterministically identified by (file path, qualified name) — no two
            // distinct source-code symbols/imports can legitimately be the same real-world entity
            // — so this is a blanket kind exclusion, matching Section/TransformNode exactly.
            //
            // `Custom("Crate")` (RFC 0042) is the same failure shape yet again, caught the same
            // way — by running `crate_topology_analyzer` against this repo's own ~40-crate
            // workspace and finding only 1 of 39 `Crate` objects survived `ekos compile`. Crate
            // names share a long common prefix (`ekos-cli`, `ekos-compiler-core`, `ekos-common`,
            // …), and every `Crate` object has the same property shape (`path`/`description`/
            // `version`, no `columns`), so `structural_score`'s same-kind 1.0 fallback pushed
            // nearly every crate pair over threshold and collapsed the whole workspace's crate
            // topology into one canonical object. Each `Crate` is already deterministically
            // identified by its manifest directory — no two distinct crates can legitimately be
            // the same real-world entity.
            //
            // `Custom("Claim")` and `Custom("ArchitectureGap")` (RFC 0065 Phase 1) are added
            // proactively, before any real over-merge was observed, specifically to avoid
            // rediscovering this exact failure shape a sixth time. Both are self-identified by a
            // structural key: a `Claim` by the (subject, predicate, object) triple of the
            // `DependsOn` relationship it was synthesized from, an `ArchitectureGap` by (crate,
            // unresolved dependency name) — many claims/gaps from the same source crate share a
            // long name/statement prefix exactly like `Crate`'s shared `ekos-*` prefix, and every
            // instance of each kind has the same property shape (no `columns`), so the same
            // same-kind 1.0 structural fallback would apply here too. No two distinct claims or
            // gaps can legitimately be the same real-world entity.
            if matches!(&obj.kind, ObjectKind::Custom(k) if k == "Section" || k == "TransformNode" || k == "RustSymbol" || k == "RustModule" || k == "PythonSymbol" || k == "PythonModule" || k == "Crate" || k == "Claim" || k == "ArchitectureGap")
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
            let canonical_norm = similarity::normalize(name_for_similarity(canonical));
            let exact_name_match = members.iter().all(|&i| {
                similarity::normalize(name_for_similarity(&objects[i])) == canonical_norm
            });
            proposals.push(MergeProposal {
                canonical_id: canonical.id,
                canonical_name: canonical.name.clone(),
                canonical_kind: canonical.kind.clone(),
                source_ids: members.iter().map(|&i| objects[i].id).collect(),
                confidence,
                exact_name_match,
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

/// The name text `DefaultResolver::score` compares for the name-similarity term (RFC 0060).
///
/// `Table` objects from SQL DDL recovery are named with their full schema/database qualifier
/// (`plausible_events_db.imported_visitors`, `public.setup_help_emails`) — every table in the
/// same source shares that qualifier, so comparing full names lets Jaro-Winkler's prefix bonus
/// count that shared, uninformative text as if it were evidence of similarity (confirmed on real
/// `analytics/` data: `plausible_events_db.imported_visitors` vs.
/// `plausible_events_db.imported_browsers` scores 0.9507 name-similarity on the full qualified
/// name vs. 0.8905 on `imported_visitors`/`imported_browsers` alone — enough of a gap to flip a
/// merge decision at the 0.90 threshold). The same shape `unrelated_documents_sharing_a_folder_prefix_do_not_all_merge`'s
/// doc comment already named for file paths ("block on the file basename rather than the full
/// relative path") — this is that fix, scoped to `Table`'s dotted schema-qualifier convention
/// specifically, not applied to `Document`/other path-shaped names (which use `/`, not a
/// database-style `schema.table` dot, and where the fix already came from the threshold change).
/// Compares only the portion after the last `.` for `Table` objects with a qualifier; every other
/// kind, and unqualified table names, are unaffected.
pub(crate) fn name_for_similarity(obj: &KirObject) -> &str {
    if obj.kind == ObjectKind::Table
        && !obj.name.contains('/')
        && let Some((_, local)) = obj.name.rsplit_once('.')
        && !local.is_empty()
    {
        return local;
    }
    // `Custom("Issue")`/`Custom("PullRequest")` (RFC 0020) are named
    // `"{owner}/{repo}#{number}: {title}"` (`github_analyzer.rs`) — every
    // item in one repo shares the `"{owner}/{repo}#"` prefix, and unlike
    // `Table`'s schema qualifier this one also swallows the number, so it's
    // proportionally much longer relative to a typical short PR title.
    // Confirmed live (RFC 0062, `analytics/`): comparing full names collapsed
    // 1,533 of 1,600 real, completely unrelated GitHub items — dependency
    // bumps, CI tweaks, unrelated features — into a single identity at
    // confidence 1.00. Strip everything through the first `": "` after the
    // first `#`, leaving just the title (which may itself contain `": "`
    // later, e.g. `"time-on-page: imported_pages new columns"` — only the
    // *first* separator, the number/title boundary, is stripped).
    if matches!(&obj.kind, ObjectKind::Custom(k) if k == "Issue" || k == "PullRequest")
        && let Some(hash_pos) = obj.name.find('#')
        && let Some(sep_pos) = obj.name[hash_pos..].find(": ")
    {
        return &obj.name[hash_pos + sep_pos + 2..];
    }
    &obj.name
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
    if let (Some(cols_a), Some(cols_b)) = (similarity::column_names(a), similarity::column_names(b))
        && !cols_a.is_empty()
        && !cols_b.is_empty()
    {
        return similarity::jaccard(&cols_a, &cols_b);
    }
    // PDF/DOCX-derived `Table` objects (`local_docs_analyzer.rs`) have no `columns` property,
    // but do carry real row content — use it the same way, before falling back to the "no
    // structural signal" floor below.
    if let (Some(rows_a), Some(rows_b)) = (
        similarity::row_cell_tokens(a),
        similarity::row_cell_tokens(b),
    ) {
        return similarity::jaccard(&rows_a, &rows_b);
    }
    1.0
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

    // ── RFC 0060: real over-merges found live against analytics/ (Plausible Analytics) ─────

    #[test]
    fn real_clickhouse_imported_tables_do_not_merge() {
        // Real bug (devlog_59/60): 6 genuinely distinct ClickHouse `imported_*` tables share an
        // 8-column "spine" plus one or two distinguishing columns each. Real column data, real
        // names, read directly from `analytics/priv/ingest_repo/structure.sql`.
        let mut g = KirGraph::new();
        g.add_object(table_with_columns(
            "plausible_events_db.imported_visitors",
            &[
                "site_id",
                "date",
                "visitors",
                "pageviews",
                "bounces",
                "visits",
                "visit_duration",
                "import_id",
            ],
        ));
        g.add_object(table_with_columns(
            "plausible_events_db.imported_browsers",
            &[
                "site_id",
                "date",
                "browser",
                "visitors",
                "visits",
                "visit_duration",
                "bounces",
                "import_id",
                "pageviews",
                "browser_version",
            ],
        ));
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "imported_visitors and imported_browsers are genuinely distinct real tables — got: {:?}",
            result.proposals
        );
    }

    #[test]
    fn real_postgres_email_template_tables_do_not_merge() {
        // Real bug (devlog_60): found the moment RFC 0059 made the real Postgres schema
        // parseable for the first time — `setup_help_emails`/`setup_success_emails` have
        // *identical* real columns (both track when one email type was sent), differing only by
        // name. No column-overlap signal can ever distinguish these; the fix has to come from
        // requiring higher name similarity too, which raising the threshold does here (name
        // similarity between "setup help emails"/"setup success emails" alone isn't high enough
        // to clear 0.90 combined with even a perfect 1.0 structural score).
        let mut g = KirGraph::new();
        g.add_object(table_with_columns(
            "public.setup_help_emails",
            &["id", "site_id", "timestamp"],
        ));
        g.add_object(table_with_columns(
            "public.setup_success_emails",
            &["id", "site_id", "timestamp"],
        ));
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "setup_help_emails and setup_success_emails are genuinely distinct real tables despite \
             identical columns — got: {:?}",
            result.proposals
        );
    }

    #[test]
    fn real_distinct_contributors_with_similar_names_do_not_merge() {
        // Real bug (devlog_60): `Niklas Hambüchen <mail@nh2.me>` and `Niklaas Baudet von
        // Gersdorff <me@niklaas.eu>` are two genuinely different real contributors (confirmed via
        // `git log --author`) that the old 0.85 threshold merged, silently dropping the second
        // person's identity and commit from the ledger under their own name.
        let g = make_graph(&[
            ("Niklas Hambüchen", ObjectKind::Person),
            ("Niklaas Baudet von Gersdorff", ObjectKind::Person),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "two different real contributors with similar names must not merge — got: {:?}",
            result.proposals
        );
    }

    #[test]
    fn real_same_contributor_under_different_git_names_still_merges() {
        // The other side of the same fix: legitimate same-person merges (nickname/nested-name
        // variants of one real git author) found in the same `analytics/` resolve run must keep
        // working after raising the threshold, not just the false positives going away.
        let g = make_graph(&[
            ("RobertJoonas", ObjectKind::Person),
            ("Robert", ObjectKind::Person),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "RobertJoonas and Robert are the same real contributor and should still merge"
        );
    }

    #[test]
    fn exact_name_match_true_for_identical_normalized_names() {
        // Same literal name (case/whitespace variants only) — unambiguous, safe to auto-merge.
        let g = make_graph(&[
            ("Customer", ObjectKind::Table),
            ("customer", ObjectKind::Table),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.proposals.len(), 1);
        assert!(
            result.proposals[0].exact_name_match,
            "identical-after-normalization names must be flagged exact"
        );
    }

    #[test]
    fn exact_name_match_false_for_the_rfc_0060_known_good_fuzzy_merges() {
        // RFC 0060's own known-correct merges (RobertJoonas/Robert, and the same shape for
        // Adam Rutkowski/Adam, Vini Brasil/Vinicius Brasil) are all real, legitimate merges — but
        // none of them share a literal identical normalized name. They must still be *proposed*
        // (this RFC does not change scoring/threshold behavior), just flagged fuzzy rather than
        // exact, so `ekos compile` sends them to review instead of silently auto-merging them.
        let g = make_graph(&[
            ("RobertJoonas", ObjectKind::Person),
            ("Robert", ObjectKind::Person),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(result.proposals.len(), 1);
        assert!(
            !result.proposals[0].exact_name_match,
            "a real nickname/variant merge is not a literal name match and must be flagged fuzzy"
        );
    }

    #[test]
    fn exact_name_match_false_for_rfc_0060_residual_known_wrong_pairs() {
        // RFC 0060's own documented residual: these 3 real pairs still incorrectly clear the 0.90
        // threshold (no confidence cutoff separates them from the known-good fuzzy merges above —
        // that's RFC 0060's core finding). RFC 0063's fix is not to reject these at scoring time
        // (out of scope, unchanged) but to ensure none of them are exact matches, so they get
        // routed to review instead of silently, irreversibly merged.
        for (name_a, name_b, kind) in [
            (
                "Build Private Images GHCR",
                "Build Public Images GHCR",
                ObjectKind::Custom("Pipeline".into()),
            ),
            (
                "Tracker CI",
                "Tracker script update",
                ObjectKind::Custom("Pipeline".into()),
            ),
            (
                // A shared directory prefix (as the real RFC 0060 pair had) is required for
                // both to land in the same `(kind, first-3-normalized-chars)` block — the
                // differing part is only in the basename.
                "docs/localization/ua_inspector.readme.md",
                "docs/localization/ref_inspector.readme.md",
                ObjectKind::Custom("Document".into()),
            ),
        ] {
            let g = make_graph(&[(name_a, kind.clone()), (name_b, kind.clone())]);
            let result = DefaultResolver::new().resolve(&g);
            assert_eq!(
                result.proposals.len(),
                1,
                "'{name_a}'/'{name_b}' expected to still be proposed (RFC 0060's documented \
                 residual, unchanged by this fix)"
            );
            assert!(
                !result.proposals[0].exact_name_match,
                "'{name_a}'/'{name_b}' must not be flagged as an exact match"
            );
        }
    }

    #[test]
    fn exact_name_match_false_for_a_transitively_chained_mixed_group() {
        // Union-Find is transitive: if A-B is exact and B-C is fuzzy, A/B/C can land in one group
        // even though A-C alone might not score above threshold. Such a group must be
        // conservatively treated as fuzzy as a whole — auto-merging it would silently include the
        // unsafe fuzzy link.
        let g = make_graph(&[
            ("Robert", ObjectKind::Person),
            ("robert", ObjectKind::Person), // exact match to "Robert" after normalization
            ("RobertJoonas", ObjectKind::Person), // fuzzy match, chains the group together
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "all three should union into one group"
        );
        assert_eq!(result.proposals[0].source_ids.len(), 3);
        assert!(
            !result.proposals[0].exact_name_match,
            "a group containing any fuzzy pairwise link must not be flagged exact"
        );
    }

    #[test]
    fn name_for_similarity_strips_schema_qualifier_for_tables() {
        let a = KirObject::new("plausible_events_db.imported_visitors", ObjectKind::Table);
        assert_eq!(name_for_similarity(&a), "imported_visitors");
    }

    #[test]
    fn name_for_similarity_leaves_unqualified_table_names_untouched() {
        let a = KirObject::new("orders", ObjectKind::Table);
        assert_eq!(name_for_similarity(&a), "orders");
    }

    #[test]
    fn name_for_similarity_does_not_strip_non_table_kinds() {
        // Document names are file paths ("test/priv/README.md") — the last-`.` heuristic must
        // not fire for them (it would leave just the extension, e.g. "md").
        let a = KirObject::new("test/priv/README.md", ObjectKind::Custom("Document".into()));
        assert_eq!(name_for_similarity(&a), "test/priv/README.md");
    }

    #[test]
    fn name_for_similarity_does_not_strip_a_table_name_containing_a_slash() {
        // Belt and suspenders: a `Table` name that happens to look like a path (defensive —
        // no real analyzer produces this today) must not be misread as `schema.table`.
        let a = KirObject::new("path/to/file.sql", ObjectKind::Table);
        assert_eq!(name_for_similarity(&a), "path/to/file.sql");
    }

    #[test]
    fn name_for_similarity_strips_owner_repo_number_prefix_for_github_items() {
        let pr = KirObject::new(
            "plausible/analytics#5158: time-on-page: `imported_pages` new columns",
            ObjectKind::Custom("PullRequest".into()),
        );
        assert_eq!(
            name_for_similarity(&pr),
            "time-on-page: `imported_pages` new columns"
        );
        let issue = KirObject::new(
            "plausible/analytics#3828: Shield: Country Rules",
            ObjectKind::Custom("Issue".into()),
        );
        assert_eq!(name_for_similarity(&issue), "Shield: Country Rules");
    }

    #[test]
    fn real_github_pull_requests_do_not_all_merge_into_one_identity() {
        // Real bug (RFC 0062): comparing full `"owner/repo#number: title"` names collapsed
        // 1,533 of 1,600 real, completely unrelated GitHub items fetched live from
        // `plausible/analytics` into one identity at confidence 1.00 — every item shares the
        // `"plausible/analytics#"` prefix, inflating Jaro-Winkler regardless of how different
        // the real titles are.
        let g = make_graph(&[
            (
                "plausible/analytics#5158: time-on-page: `imported_pages` new columns",
                ObjectKind::Custom("PullRequest".into()),
            ),
            (
                "plausible/analytics#6527: Bump fast-uri from 3.0.6 to 3.1.4 in /assets",
                ObjectKind::Custom("PullRequest".into()),
            ),
            (
                "plausible/analytics#5469: remove salts logs",
                ObjectKind::Custom("PullRequest".into()),
            ),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "three genuinely unrelated real PRs must not merge just because they share the \
             owner/repo# prefix — got: {:?}",
            result.proposals
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

    /// Real bug, found 2026-08-03 rescanning a mixed-format real-content workspace with RFC
    /// 0025/0026's new mechanics: every file under this test's fixture directory shared the
    /// literal `docs/` path prefix, and `normalize()` never strips path segments — so the
    /// blocking key's "first 3 normalized chars" rule put every `Document` object in this
    /// workspace into one block regardless of the files' actual names, and `structural_score`'s
    /// same-kind 1.0 fallback (Documents have no `columns` property) supplied the same free +0.3
    /// floor devlog_27 already diagnosed for Section — except this hits genuinely unrelated
    /// documents (a PDF, a Markdown file, an HTML file, a plain-text file, an email), not
    /// near-duplicate pages of one book. Live repro: 7 entirely different files collapsed into one
    /// canonical `Document` object at confidence 0.90.
    ///
    /// **Fixed by RFC 0060** (raising `DEFAULT_MERGE_THRESHOLD` from 0.85 to 0.90) — found while
    /// fixing a separate, worse real over-merge (`analytics/`'s Person/Table/Document proposals,
    /// devlog_60): re-running this test with the new threshold now passes without any change to
    /// its own logic. Not a coincidence — this test's 7 documents merged at confidence 0.90 under
    /// the *old* 0.85 threshold precisely because they cleared it; the new threshold sits exactly
    /// at that boundary. The "block on basename" and "no free floor for kinds without columns"
    /// alternatives named below were not needed to close this specific case, though either remains
    /// available if a future real file pushes a similarly-shaped merge just over 0.90 again.
    #[test]
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
    /// previously had no comparable structural signal (`column_names` only
    /// applies to SQL-derived tables), so they got the exact same free
    /// structural-score floor. Live repro: 9 distinct tables from one real
    /// PDF — different content, different rows — collapsed into a single
    /// canonical `Table` object at confidence 0.99. Fixed by giving
    /// `structural_score` a second real signal, `similarity::row_cell_tokens`
    /// (Jaccard over each table's actual cell text, the same real content
    /// `local_docs_analyzer.rs` already stores under `properties["rows"]`) —
    /// this is the identical failure shape RFC 0024 already fixed for
    /// `Custom("Section")`, just on `ObjectKind::Table`, which RFC 0024
    /// deliberately left untouched (SQL-derived tables still need real
    /// fuzzy name dedup across files, so the fix couldn't be "exclude
    /// Table" — it had to distinguish PDF-sourced tables from SQL-sourced
    /// ones by an actual structural signal instead).
    #[test]
    fn distinct_pdf_tables_in_one_document_do_not_all_merge() {
        let mut g = KirGraph::new();
        let tables: &[(&str, &[&[&str]])] = &[
            (
                "docs/120 Data Science Interview Questions.pdf: table 1",
                &[
                    &["Question", "Topic"],
                    &["What is bias-variance tradeoff?", "ML theory"],
                ],
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 2",
                &[&["Chapter", "Page"], &["Statistics", "12"]],
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 3",
                &[
                    &["Term", "Definition"],
                    &["Overfitting", "Memorizing noise"],
                ],
            ),
            (
                "docs/120 Data Science Interview Questions.pdf: table 4",
                &[
                    &["Algorithm", "Use case"],
                    &["Random Forest", "Classification"],
                ],
            ),
        ];
        for (name, rows) in tables {
            let row_values: Vec<Vec<&str>> = rows.iter().map(|r| r.to_vec()).collect();
            let obj = KirObject::new(*name, ObjectKind::Table)
                .with_property("rows", serde_json::json!(row_values));
            g.add_object(obj);
        }
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "four distinct tables extracted from one PDF, with real distinct row content, must \
             not collapse into one canonical table just because they share a name prefix — got: \
             {:?}",
            result.proposals
        );
    }

    /// Two `Table` objects that share the same real row content (e.g. the same PDF re-extracted,
    /// or a genuinely duplicated table) must still be proposed for merge — the fix above must not
    /// make every PDF table look unique regardless of content.
    #[test]
    fn pdf_tables_with_identical_row_content_still_merge() {
        let rows =
            serde_json::json!([["Question", "Topic"], ["What is overfitting?", "ML theory"]]);
        let mut g = KirGraph::new();
        g.add_object(
            KirObject::new("docs/report-v1.pdf: table 1", ObjectKind::Table)
                .with_property("rows", rows.clone()),
        );
        g.add_object(
            KirObject::new("docs/report-v2.pdf: table 1", ObjectKind::Table)
                .with_property("rows", rows),
        );
        let result = DefaultResolver::new().resolve(&g);
        assert_eq!(
            result.proposals.len(),
            1,
            "two tables with identical real row content should still be proposed as one merge \
             group — got: {:?}",
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

    /// Regression test for a real bug found live while real-data-testing RFC 0041 against this
    /// repo's own ~50-crate workspace: `Custom("RustSymbol")` objects sharing a common name
    /// suffix (this repo genuinely has `ConfluenceAnalyzerPass`, `PentahoAnalyzerPass`, and
    /// `GitAnalyzerPass`, each defined in a different file) scored above the merge threshold on
    /// name similarity alone (`structural_score`'s same-kind 1.0 fallback, no `columns` property
    /// to differentiate on) — the identical failure shape as `Section`/`TransformNode`. Before the
    /// fix, distinct structs from different files silently collapsed into one canonical object.
    #[test]
    fn rust_symbol_objects_are_never_merged_even_with_shared_name_suffix() {
        let rust_symbol = ObjectKind::Custom("RustSymbol".to_string());
        let g = make_graph(&[
            ("ConfluenceAnalyzerPass", rust_symbol.clone()),
            ("PentahoAnalyzerPass", rust_symbol.clone()),
            ("GitAnalyzerPass", rust_symbol),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "RustSymbol objects must never be merge candidates, got {:?}",
            result.proposals
        );
    }

    /// Regression test for a real bug found live while regenerating EKOS's own curated docs
    /// (RFC 0042): `Custom("Crate")` objects sharing the workspace's common `ekos-` name prefix
    /// scored above the merge threshold on name similarity alone (`structural_score`'s same-kind
    /// 1.0 fallback, no `columns` property to differentiate on) — the identical failure shape as
    /// `Section`/`TransformNode`/`RustSymbol`. Before the fix, 39 real crates collapsed into 1
    /// canonical object, silently dropping the entire crate/workspace dependency topology.
    #[test]
    fn crate_objects_are_never_merged_even_with_shared_name_prefix() {
        let crate_kind = ObjectKind::Custom("Crate".to_string());
        let g = make_graph(&[
            ("ekos-cli", crate_kind.clone()),
            ("ekos-common", crate_kind.clone()),
            ("ekos-compiler-core", crate_kind),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "Crate objects must never be merge candidates, got {:?}",
            result.proposals
        );
    }

    #[test]
    fn claim_objects_are_never_merged_even_with_shared_statement_prefix() {
        // RFC 0065 Phase 1: claims derived from the same source crate share a long
        // "{crate} depends_on ..." prefix — the same failure shape as Crate's shared `ekos-*`
        // prefix — added to the exclusion list proactively, before any real over-merge occurred.
        let claim_kind = ObjectKind::Custom("Claim".to_string());
        let g = make_graph(&[
            ("ekos-cli depends_on ekos-common", claim_kind.clone()),
            ("ekos-cli depends_on ekos-kir", claim_kind.clone()),
            ("ekos-cli depends_on serde", claim_kind),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "Claim objects must never be merge candidates, got {:?}",
            result.proposals
        );
    }

    #[test]
    fn architecture_gap_objects_are_never_merged_even_with_shared_question_prefix() {
        let gap_kind = ObjectKind::Custom("ArchitectureGap".to_string());
        let g = make_graph(&[
            ("unresolved dependency foo for ekos-cli", gap_kind.clone()),
            ("unresolved dependency bar for ekos-cli", gap_kind.clone()),
            ("unresolved dependency foo for ekos-kir", gap_kind),
        ]);
        let result = DefaultResolver::new().resolve(&g);
        assert!(
            result.proposals.is_empty(),
            "ArchitectureGap objects must never be merge candidates, got {:?}",
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
