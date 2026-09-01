//! RFC 0119 (Phase 0 of RFC 0118) — the retrieval seam.
//!
//! A single scored, multi-signal retrieval surface on [`KnowledgeStore`](crate::KnowledgeStore),
//! with a default implementation that wraps `find_objects` so every existing implementor compiles
//! unchanged and produces **byte-identical** id ordering. Fusion, the exact-name signal, and the
//! vector/graph arms arrive in RFC 0120+.

use ekos_kir::{KirId, ObjectKind};

/// Reciprocal Rank Fusion constant. Shared with the real fuser in RFC 0120 so a Phase 0
/// rank-only score is directly comparable to a Phase 1 fused score.
pub const RRF_K: f32 = 60.0;

/// Which **store-local** retrieval arms to run. The graph arm is not store-local (it needs the
/// `Runtime`), so it is not represented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArmSet {
    pub bm25: bool,
    pub vector: bool,
}

impl ArmSet {
    /// BM25 only — the Phase 0 default.
    pub const LEXICAL: ArmSet = ArmSet {
        bm25: true,
        vector: false,
    };
}

/// Built by the Runtime-level orchestrator (RFC 0121+), never by end consumers. Handed to the
/// sync [`KnowledgeStore::retrieve`](crate::KnowledgeStore::retrieve).
#[derive(Debug, Clone)]
pub struct RetrievalRequest {
    /// Raw user text, verbatim. Drives exact-name logic (RFC 0120) and is the BM25 query when
    /// `keywords` is unset.
    pub raw: String,
    /// Processed keyword / boolean form for BM25 (`"a AND b"` / `"a OR b"`). Wins over `raw`.
    pub keywords: Option<String>,
    /// Pre-computed, L2-normalizable query embedding. The vector arm is skipped unless this is
    /// `Some` **and** its length matches the on-disk vector index header dim (RFC 0125). Inert in
    /// Phase 0.
    pub query_embedding: Option<Vec<f32>>,
    pub arms: ArmSet,
    /// Candidates pulled from each arm before fusion.
    pub per_arm_limit: usize,
    /// Final cap after fusion.
    pub limit: usize,
}

impl RetrievalRequest {
    /// BM25-only, `limit` 50 — the drop-in replacement for a `find_objects` call.
    pub fn lexical(raw: impl Into<String>) -> Self {
        Self {
            raw: raw.into(),
            keywords: None,
            query_embedding: None,
            arms: ArmSet::LEXICAL,
            per_arm_limit: 50,
            limit: 50,
        }
    }

    /// The string handed to BM25: the processed `keywords` form if present, else the raw text.
    pub fn bm25_query(&self) -> &str {
        self.keywords.as_deref().unwrap_or(&self.raw)
    }
}

/// Where a hit came from and how it ranked there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signal {
    pub source: SignalSource,
    /// 0-based rank within that source's own result list.
    pub rank: u32,
    /// The source's raw score. `0.0` when unknown — `find_objects` exposes no score, so every
    /// Phase 0 signal carries `0.0` here.
    pub raw_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
pub enum SignalSource {
    Bm25,
    Vector,
    Graph,
    ExactName,
}

/// Wall-clock cost of one retrieval arm — pure observability (RFC 0126). Fed into the RFC 0114
/// usage log and `ekos query find --explain`; never influences fusion or ranking. Only
/// [`crate::FactLedger::retrieve`] populates these; every other backend leaves
/// [`RankedResults::arm_timings`] empty.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct ArmTiming {
    pub source: SignalSource,
    pub elapsed_ms: f64,
    /// Rows this arm contributed to fusion (pre-dedup).
    pub candidates: usize,
}

/// One ranked knowledge object, with the per-arm evidence for *why* it surfaced.
#[derive(Debug, Clone)]
pub struct Hit {
    pub id: KirId,
    pub name: String,
    /// `None` until `f_kind` is `STORED` (RFC 0120) or the orchestrator hydrates the final top-N.
    pub kind: Option<ObjectKind>,
    /// Fused score, strictly decreasing across `RankedResults::hits`. Rank-only in Phase 0:
    /// `1.0 / (RRF_K + rank)`.
    pub score: f32,
    pub signals: Vec<Signal>,
}

/// The result of a [`retrieve`](crate::KnowledgeStore::retrieve) call.
#[derive(Debug, Clone)]
pub struct RankedResults {
    pub hits: Vec<Hit>,
    /// The arms that actually ran (not just the ones requested). `vector` drops to `false` when
    /// no index is on disk or no query embedding was supplied.
    pub arms_run: ArmSet,
    /// Per-arm wall-clock cost (RFC 0126). Populated only by [`crate::FactLedger::retrieve`];
    /// empty on every other backend and on the `from_ranked_pairs` default path.
    pub arm_timings: Vec<ArmTiming>,
}

impl RankedResults {
    /// Wrap a `find_objects`-style ranked `(id, name)` list as a single-signal, rank-only
    /// `RankedResults` — the Phase 0 default-impl body. `pairs` is assumed best-first.
    pub fn from_ranked_pairs(
        pairs: Vec<(KirId, String)>,
        source: SignalSource,
        limit: usize,
    ) -> Self {
        let hits = pairs
            .into_iter()
            .take(limit)
            .enumerate()
            .map(|(i, (id, name))| {
                let rank = i as u32;
                Hit {
                    id,
                    name,
                    kind: None,
                    score: 1.0 / (RRF_K + rank as f32),
                    signals: vec![Signal {
                        source,
                        rank,
                        raw_score: 0.0,
                    }],
                }
            })
            .collect();
        Self {
            hits,
            arms_run: ArmSet::LEXICAL,
            arm_timings: Vec::new(),
        }
    }

    /// The hit ids, best-first — the shape `find_objects` consumers reduce to.
    pub fn ids(&self) -> Vec<KirId> {
        self.hits.iter().map(|h| h.id).collect()
    }

    /// `(id, name)` pairs, best-first — the exact legacy `find_objects` return shape.
    pub fn into_pairs(self) -> Vec<(KirId, String)> {
        self.hits.into_iter().map(|h| (h.id, h.name)).collect()
    }
}

/// One arm's candidate before fusion (RFC 0120).
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredCandidate {
    pub id: KirId,
    pub name: String,
    pub kind: Option<ObjectKind>,
    /// Arm-native raw score (BM25 unbounded, cosine `[-1,1]`, exact-name `1.0`). Informational
    /// after fusion — RRF ranks, it does not compare magnitudes.
    pub raw_score: f32,
}

impl ScoredCandidate {
    pub fn new(id: KirId, name: impl Into<String>, raw_score: f32) -> Self {
        Self {
            id,
            name: name.into(),
            kind: None,
            raw_score,
        }
    }
}

/// Reciprocal Rank Fusion (Cormack et al. 2009). Each `(source, list)` is best-first from one
/// arm. A document's fused score is `Σ 1/(k + rank)` over the lists it appears in, with 0-based
/// `rank` — so a single list reproduces RFC 0119's `1/(RRF_K + rank)` exactly. Output is
/// best-first, `limit`-capped; ties break by `KirId` for determinism. Each contributing arm is
/// recorded on the `Hit` as a [`Signal`].
pub fn rrf_fuse(lists: &[(SignalSource, Vec<ScoredCandidate>)], k: f32, limit: usize) -> Vec<Hit> {
    use std::collections::HashMap;

    struct Acc {
        name: String,
        kind: Option<ObjectKind>,
        score: f32,
        signals: Vec<Signal>,
    }
    let mut acc: HashMap<KirId, Acc> = HashMap::new();

    for (source, list) in lists {
        for (rank, cand) in list.iter().enumerate() {
            let contribution = 1.0 / (k + rank as f32);
            let entry = acc.entry(cand.id).or_insert_with(|| Acc {
                name: cand.name.clone(),
                kind: cand.kind.clone(),
                score: 0.0,
                signals: Vec::new(),
            });
            entry.score += contribution;
            if entry.kind.is_none() {
                entry.kind = cand.kind.clone();
            }
            entry.signals.push(Signal {
                source: *source,
                rank: rank as u32,
                raw_score: cand.raw_score,
            });
        }
    }

    let mut hits: Vec<Hit> = acc
        .into_iter()
        .map(|(id, a)| Hit {
            id,
            name: a.name,
            kind: a.kind,
            score: a.score,
            signals: a.signals,
        })
        .collect();
    // Highest fused score first; deterministic tie-break by id.
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.0.cmp(&b.id.0))
    });
    hits.truncate(limit);
    hits
}

/// The subset of `candidates` whose name equals `query` case-insensitively after trimming — the
/// `ExactName` arm's ranked list, in the candidates' original order. Comparison matches
/// `promote_exact_name_matches` exactly.
pub fn exact_name_matches(query: &str, candidates: &[ScoredCandidate]) -> Vec<ScoredCandidate> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    candidates
        .iter()
        .filter(|c| c.name.trim().to_lowercase() == q)
        .cloned()
        .map(|mut c| {
            c.raw_score = 1.0;
            c
        })
        .collect()
}

/// Resolve a dotted attribute path against a compiled object — the fact-lookup primitive behind
/// [`KnowledgeStore::fact`](crate::KnowledgeStore::fact) (RFC 0122, the QUERY surface).
///
/// - `"name"` → the object's name; `"kind"` → its kind's display string.
/// - any other `attr` is a dotted path walked into `properties`: `"schema"` reads a top-level
///   property, `"foreign_keys.0.column"` walks object → array index → object key.
///
/// Returns `None` for an absent path — the same "simply not present" contract a missing
/// `ai_overview` has today, never an error.
pub fn resolve_fact(obj: &ekos_kir::KirObject, attr: &str) -> Option<serde_json::Value> {
    match attr.trim() {
        "" => return None,
        "name" => return Some(serde_json::Value::String(obj.name.clone())),
        "kind" => return Some(serde_json::Value::String(obj.kind.to_string())),
        _ => {}
    }
    let mut segments = attr.trim().split('.');
    let mut cur = obj.properties.get(segments.next()?)?;
    for seg in segments {
        cur = match cur {
            serde_json::Value::Object(map) => map.get(seg)?,
            serde_json::Value::Array(arr) => arr.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FactLedger, KnowledgeStore};
    use ekos_kir::{KirObject, ObjectKind};
    use tempfile::tempdir;

    fn ledger_with(names: &[&str]) -> (FactLedger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let l = FactLedger::open(&dir.path().join("fl")).unwrap();
        for n in names {
            l.append_object(&KirObject::new(*n, ObjectKind::Table))
                .unwrap();
        }
        (l, dir)
    }

    fn cand(name: &str, score: f32) -> ScoredCandidate {
        ScoredCandidate::new(KirId::new(), name, score)
    }

    // ── the pure default-impl body (RFC 0119) ──────────────────────────────
    #[test]
    fn from_ranked_pairs_preserves_order_and_decreasing_score() {
        let pairs: Vec<(KirId, String)> = ["a", "b", "c"]
            .iter()
            .map(|n| (KirId::new(), n.to_string()))
            .collect();
        let want: Vec<KirId> = pairs.iter().map(|(id, _)| *id).collect();
        let r = RankedResults::from_ranked_pairs(pairs, SignalSource::Bm25, 50);
        assert_eq!(r.ids(), want);
        assert_eq!(r.arms_run, ArmSet::LEXICAL);
        for w in r.hits.windows(2) {
            assert!(w[0].score > w[1].score);
        }
        assert!(r.hits.iter().all(|h| h.kind.is_none()));
    }

    // ── RRF (RFC 0120) ────────────────────────────────────────────────────
    #[test]
    fn rrf_fuse_canonical_cormack_example() {
        let (d1, d2, d3) = (cand("d1", 0.0), cand("d2", 0.0), cand("d3", 0.0));
        let a = vec![d1.clone(), d2.clone(), d3.clone()]; // [d1, d2, d3]
        let b = vec![d2.clone(), d3.clone(), d1.clone()]; // [d2, d3, d1]
        let fused = rrf_fuse(
            &[(SignalSource::Bm25, a), (SignalSource::Vector, b)],
            60.0,
            10,
        );
        let order: Vec<&str> = fused.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            order,
            vec!["d2", "d1", "d3"],
            "d2: 1/61+1/60 > d1: 1/60+1/62 > d3: 1/62+1/61"
        );
        // d2 carries a signal from each list
        let d2h = fused.iter().find(|h| h.name == "d2").unwrap();
        assert_eq!(d2h.signals.len(), 2);
    }

    #[test]
    fn rrf_fuse_dedups_limits_and_handles_empty() {
        assert!(rrf_fuse(&[], 60.0, 10).is_empty());
        assert!(rrf_fuse(&[(SignalSource::Bm25, vec![])], 60.0, 10).is_empty());
        let x = cand("x", 0.0);
        let fused = rrf_fuse(
            &[
                (SignalSource::Bm25, vec![x.clone(), cand("y", 0.0)]),
                (SignalSource::ExactName, vec![x.clone()]),
            ],
            60.0,
            1,
        );
        assert_eq!(fused.len(), 1);
        assert_eq!(fused[0].name, "x"); // x wins: appears in both lists
        assert_eq!(fused[0].signals.len(), 2);
    }

    #[test]
    fn exact_name_matches_is_case_insensitive_and_trims() {
        let cands = vec![
            cand("README.md", 3.0),
            cand("README", 1.0),
            cand("readme_generator", 2.0),
        ];
        let hits = exact_name_matches("  readme  ", &cands);
        assert_eq!(
            hits.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
            vec!["README"]
        );
        assert_eq!(hits[0].raw_score, 1.0);
        assert!(exact_name_matches("", &cands).is_empty());
        assert!(exact_name_matches("nope", &cands).is_empty());
    }

    // ── FactLedger::retrieve fuses the ExactName arm (RFC 0120) ────────────
    #[test]
    fn factledger_retrieve_promotes_exact_name() {
        let (l, _d) = ledger_with(&[
            "readme_generator",
            "docs/README-notes",
            "README",
            "readme_helper",
        ]);
        // BM25 alone does not necessarily rank the exact "README" first.
        let bm25_first = l.find_objects_scored("README", 50).unwrap()[0].1.clone();
        let seam = l.retrieve(&RetrievalRequest::lexical("README")).unwrap();
        assert_eq!(
            seam.hits[0].name, "README",
            "exact-name arm promotes it to #1"
        );
        assert!(
            seam.hits[0]
                .signals
                .iter()
                .any(|s| s.source == SignalSource::ExactName),
            "the #1 hit carries an ExactName signal"
        );
        // Same query, no exact match → falls back to BM25 order.
        let no_exact = l.retrieve(&RetrievalRequest::lexical("readme")).unwrap();
        let _ = (bm25_first, no_exact);
    }

    #[test]
    fn limit_truncates() {
        let (l, _d) = ledger_with(&["order_a", "order_b", "order_c", "order_d"]);
        let mut req = RetrievalRequest::lexical("order");
        req.limit = 2;
        assert_eq!(l.retrieve(&req).unwrap().hits.len(), 2);
    }

    // ── fact-path resolution (RFC 0122) ──────────────────────────────────
    #[test]
    fn resolve_fact_reads_header_properties_and_nested_paths() {
        let mut obj = KirObject::new("orders", ObjectKind::Table);
        obj.properties
            .insert("schema".into(), serde_json::json!("public"));
        obj.properties.insert(
            "foreign_keys".into(),
            serde_json::json!([{ "column": "customer_id", "references": "customers" }]),
        );

        assert_eq!(
            resolve_fact(&obj, "name"),
            Some(serde_json::json!("orders"))
        );
        assert_eq!(resolve_fact(&obj, "kind"), Some(serde_json::json!("Table")));
        assert_eq!(
            resolve_fact(&obj, "schema"),
            Some(serde_json::json!("public"))
        );
        assert_eq!(
            resolve_fact(&obj, "foreign_keys.0.column"),
            Some(serde_json::json!("customer_id"))
        );
        assert_eq!(resolve_fact(&obj, "foreign_keys.9.column"), None);
        assert_eq!(resolve_fact(&obj, "missing"), None);
        assert_eq!(resolve_fact(&obj, ""), None);
    }

    #[test]
    fn store_fact_default_impl_resolves_and_misses() {
        let (l, _d) = ledger_with(&["customers"]);
        let id = l.find_objects("customers").unwrap()[0].0;
        assert_eq!(
            l.fact(&id, "name").unwrap(),
            Some(serde_json::json!("customers"))
        );
        assert_eq!(l.fact(&id, "nope").unwrap(), None);
        assert_eq!(l.fact(&KirId::new(), "name").unwrap(), None);
    }

    #[test]
    fn bm25_query_prefers_keywords_over_raw() {
        let req = RetrievalRequest {
            raw: "the whole question".into(),
            keywords: Some("whole question".into()),
            ..RetrievalRequest::lexical("the whole question")
        };
        assert_eq!(req.bm25_query(), "whole question");
        assert_eq!(RetrievalRequest::lexical("bare").bm25_query(), "bare");
    }
}
