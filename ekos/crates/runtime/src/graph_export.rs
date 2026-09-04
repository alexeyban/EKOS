//! RFC 0127 R1 — bulk graph extraction from a compiled workspace.
//!
//! Every existing read is per-object (`ekos_neighborhood` / `ekos_impact` / `ekos_dependents`) or
//! rank-capped (`find_objects` at `LIMIT 50`). There is no "give me the whole graph" path. This
//! module is that path: one pure, read-only function ([`export_graph`]) over
//! [`KnowledgeStore::all_objects`] + [`KnowledgeStore::all_relationships`], filtered, optionally
//! aggregated, and truncated **with the truncation reported, never silent**.
//!
//! Both `ekos graph export` (the CLI) and `ekos_graph_export` (the MCP tool) call this one
//! function — the same anti-drift discipline RFC 0102 applied to `dependency_graph_groups`.
//!
//! ## Determinism
//!
//! Two calls against an unchanged store produce byte-identical JSON **except `generated_at`**.
//! Nodes sort by `(kind, name, id)`; edges by `(source_index, target_index, kind_index)`;
//! `kind_index` / `rel_kind_index` sort lexicographically. [`ekos_kir::KirId`] derives no `Ord`,
//! so every id tie-break sorts on the inner `uuid::Uuid` (`id.0`).
//!
//! ## Wire format
//!
//! Short keys (`n`/`k`/`d`/`p`/`s`/`t`/`w`) and an index into `kind_index` / `rel_kind_index`
//! because at 20 000 edges the repeated identifier and key text dominates the payload (RFC 0127
//! §4.4). Node ids stay full [`KirId`] strings so the client can call `ekos_state` /
//! `ekos_neighborhood` / `ekos_impact` with them; aggregate ids are synthetic (`kind:File`,
//! `path:crates/ledger`) and `id_space: "synthetic"` says so.

use crate::RuntimeError;
use chrono::{DateTime, Utc};
use ekos_kir::{KirId, KirObject, ObjectKind, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// The current [`GraphExport`] wire-format version.
pub const SCHEMA_VERSION: u32 = 1;

const DEFAULT_MAX_NODES: usize = 5_000;
const DEFAULT_MAX_EDGES: usize = 20_000;
const DEFAULT_PATH_PREFIX_DEPTH: usize = 2;

// ── Input ────────────────────────────────────────────────────────────────────

/// `--level object` returns real [`KirObject`]s; `--level aggregate` collapses them into
/// super-nodes (one per kind, or one per path prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportLevel {
    Object,
    Aggregate,
}

/// How `--level aggregate` groups objects. `Kind` → one node per [`ObjectKind`]; `PathPrefix` →
/// one node per first-`depth` `/`-segments of the object's `path` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupBy {
    Kind,
    PathPrefix { depth: usize },
}

impl Default for GroupBy {
    fn default() -> Self {
        GroupBy::PathPrefix {
            depth: DEFAULT_PATH_PREFIX_DEPTH,
        }
    }
}

/// Everything [`export_graph`] needs. Build with [`GraphExportOptions::default`] and override.
#[derive(Debug, Clone)]
pub struct GraphExportOptions {
    pub level: ExportLevel,
    /// Absolute workspace path — copied verbatim into the output, never read from.
    pub workspace: PathBuf,
    /// Include-list of object kinds. `None` = every kind.
    pub kinds: Option<Vec<ObjectKind>>,
    /// Include-list of relationship kinds. `None` = every kind (before `exclude_rel_kinds`).
    pub rel_kinds: Option<Vec<RelationshipKind>>,
    /// Relationship kinds to drop, applied after `rel_kinds`. The console passes
    /// `Custom("CoupledWith")` / `Custom("FeedsInto")` here **explicitly** — no built-in silent
    /// exclusion (RFC 0127 §4.8).
    pub exclude_rel_kinds: Vec<RelationshipKind>,
    /// Consulted only when `level == Aggregate`.
    pub group_by: GroupBy,
    pub max_nodes: usize,
    pub max_edges: usize,
    /// Drop object-level nodes whose post-filter degree is below this. Single pass, not iterated
    /// to a fixpoint.
    pub min_degree: usize,
    /// Property keys carried into each node's `p`. Default empty — objects can carry `excerpt` /
    /// `ai_overview` / `symbols`, tens of MB of prose a visualiser never renders.
    pub include_properties: Vec<String>,
    /// RFC 0134 — reconstruct the graph as it stood at this instant, via
    /// [`KnowledgeStore::all_objects_at`] / [`KnowledgeStore::all_relationships_at`]. `None` =
    /// current state. `counts` are then naturally "as of `at`".
    pub as_of: Option<DateTime<Utc>>,
    /// RFC 0134 — stamp each node/edge with its first-seen time (`fs` in the wire format): an
    /// object's / relationship's own `created_at` at object level, the member `min` at aggregate
    /// level. This is what lets a client scrub a monotonic ledger locally without refetching.
    /// Note: `created_at` tracks the *latest* stored version, so `fs` is exact for objects that
    /// never changed content and an upper bound otherwise — `as_of` is the precise path.
    pub include_first_seen: bool,
}

impl Default for GraphExportOptions {
    fn default() -> Self {
        Self {
            level: ExportLevel::Object,
            workspace: PathBuf::new(),
            kinds: None,
            rel_kinds: None,
            exclude_rel_kinds: Vec::new(),
            group_by: GroupBy::Kind,
            max_nodes: DEFAULT_MAX_NODES,
            max_edges: DEFAULT_MAX_EDGES,
            min_degree: 0,
            include_properties: Vec::new(),
            as_of: None,
            include_first_seen: false,
        }
    }
}

/// An edge in flight through [`export_graph`]: endpoints, kind, and the relationship's
/// `created_at` (carried so `include_first_seen` / aggregate `min` can use it).
type EdgeRow = (KirId, KirId, RelationshipKind, DateTime<Utc>);

/// Aggregate super-node accumulator, keyed by group id → (label, member count, min `created_at`).
type GroupAcc = BTreeMap<String, (String, usize, Option<DateTime<Utc>>)>;

/// Aggregate group-edge accumulator, keyed by (min group, max group, kind) → (weight, min
/// `created_at`).
type CollapsedEdges = BTreeMap<(String, String, String), (usize, Option<DateTime<Utc>>)>;

// ── Output ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GraphExport {
    pub schema_version: u32,
    pub workspace: String,
    /// The one non-deterministic field — excluded from the determinism test.
    pub generated_at: DateTime<Utc>,
    pub level: &'static str,
    /// RFC 0134 — the instant this graph was reconstructed at; `None` = current state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub as_of: Option<DateTime<Utc>>,
    /// `"kir"` (node ids are real [`KirId`]s, safe for `ekos_state`) or `"synthetic"` (aggregate
    /// ids like `kind:File` — **never** pass these to a per-object tool).
    pub id_space: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<&'static str>,
    pub counts: GraphCounts,
    pub truncated: Truncation,
    pub filters: AppliedFilters,
    /// Lexicographically sorted; node `k` indexes this.
    pub kind_index: Vec<String>,
    /// Lexicographically sorted; edge `k` indexes this.
    pub rel_kind_index: Vec<String>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphCounts {
    pub total_objects: usize,
    pub total_relationships: usize,
    pub objects_after_filter: usize,
    pub relationships_after_filter: usize,
    pub returned_nodes: usize,
    pub returned_edges: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Truncation {
    pub nodes: bool,
    pub node_limit: usize,
    pub edges: bool,
    pub edge_limit: usize,
    /// How the retained set was chosen when truncation happened.
    pub selection: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppliedFilters {
    pub kinds: Option<Vec<String>>,
    pub rel_kinds: Option<Vec<String>>,
    pub exclude_rel_kinds: Vec<String>,
    pub min_degree: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Full `KirId` string at object level; synthetic (`kind:File`) at aggregate level.
    pub id: String,
    #[serde(rename = "n")]
    pub name: String,
    /// Index into [`GraphExport::kind_index`].
    #[serde(rename = "k")]
    pub kind: usize,
    /// Degree over the **post-filter** edge set.
    #[serde(rename = "d")]
    pub degree: usize,
    /// `include_properties` keys present on the object. `BTreeMap` so key order is deterministic
    /// regardless of `serde_json`'s `preserve_order` feature.
    #[serde(rename = "p", skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, serde_json::Value>,
    /// Members collapsed into this super-node — aggregate level only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    /// RFC 0134 — first-seen time; present only when `include_first_seen`. Object level: the
    /// object's `created_at`. Aggregate level: the `min` over the super-node's members.
    #[serde(rename = "fs", skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    #[serde(rename = "s")]
    pub source: usize,
    #[serde(rename = "t")]
    pub target: usize,
    #[serde(rename = "k")]
    pub kind: usize,
    /// Underlying object-level relationships collapsed into this group edge — aggregate only.
    #[serde(rename = "w", skip_serializing_if = "Option::is_none")]
    pub weight: Option<usize>,
    /// RFC 0134 — first-seen time; present only when `include_first_seen`. Object level: the
    /// relationship's `created_at`. Aggregate level: the `min` over the collapsed edges.
    #[serde(rename = "fs", skip_serializing_if = "Option::is_none")]
    pub first_seen: Option<DateTime<Utc>>,
}

// ── The function ─────────────────────────────────────────────────────────────

/// Extract the whole compiled graph, filtered and (optionally) aggregated.
///
/// Read-only (RFC 0005). Does **not** serialize — the caller (`ekos graph export` or the
/// `ekos_graph_export` MCP tool) turns the returned struct into JSON / NDJSON.
pub fn export_graph(
    store: &dyn KnowledgeStore,
    opts: &GraphExportOptions,
) -> Result<GraphExport, RuntimeError> {
    // RFC 0134 — `as_of` swaps the unbounded reads for the point-in-time bulk primitives; every
    // downstream step (filter, degree, min_degree, truncation, aggregation) is unchanged.
    let (objects, relationships) = match opts.as_of {
        Some(at) => (store.all_objects_at(at)?, store.all_relationships_at(at)?),
        None => (store.all_objects()?, store.all_relationships()?),
    };
    let total_objects = objects.len();
    let total_relationships = relationships.len();

    // 1. node filter by kind include-list.
    let mut surviving: HashMap<KirId, KirObject> = objects
        .into_iter()
        .filter(|o| match &opts.kinds {
            Some(ks) => ks.contains(&o.kind),
            None => true,
        })
        .map(|o| (o.id, o))
        .collect();

    // 2. edge filter: rel-kind include ∧ not excluded ∧ both endpoints survived.
    let edge_kind_ok = |k: &RelationshipKind| {
        opts.rel_kinds.as_ref().is_none_or(|ks| ks.contains(k))
            && !opts.exclude_rel_kinds.contains(k)
    };
    let mut edges: Vec<EdgeRow> = relationships
        .into_iter()
        .filter(|r| {
            edge_kind_ok(&r.kind)
                && surviving.contains_key(&r.from)
                && surviving.contains_key(&r.to)
        })
        .map(|r| (r.from, r.to, r.kind, r.created_at))
        .collect();

    // 3. degree over the post-filter edge set. A self-loop adds 2 to its one endpoint.
    let degree_of = |edges: &[EdgeRow]| -> HashMap<KirId, usize> {
        let mut d: HashMap<KirId, usize> = HashMap::new();
        for (s, t, _, _) in edges {
            *d.entry(*s).or_insert(0) += 1;
            *d.entry(*t).or_insert(0) += 1;
        }
        d
    };
    let mut degree = degree_of(&edges);

    // 4. min_degree — single pass (documented; not iterated to a fixpoint).
    if opts.min_degree > 0 {
        surviving.retain(|id, _| degree.get(id).copied().unwrap_or(0) >= opts.min_degree);
        edges.retain(|(s, t, _, _)| surviving.contains_key(s) && surviving.contains_key(t));
        degree = degree_of(&edges);
    }

    let objects_after_filter = surviving.len();
    let relationships_after_filter = edges.len();

    let filters = AppliedFilters {
        kinds: opts
            .kinds
            .as_ref()
            .map(|ks| ks.iter().map(|k| k.to_string()).collect()),
        rel_kinds: opts
            .rel_kinds
            .as_ref()
            .map(|ks| ks.iter().map(|k| k.to_string()).collect()),
        exclude_rel_kinds: opts
            .exclude_rel_kinds
            .iter()
            .map(|k| k.to_string())
            .collect(),
        min_degree: opts.min_degree,
    };

    match opts.level {
        ExportLevel::Object => Ok(build_object_level(
            opts,
            surviving,
            edges,
            &degree,
            total_objects,
            total_relationships,
            objects_after_filter,
            relationships_after_filter,
            filters,
        )),
        ExportLevel::Aggregate => Ok(build_aggregate_level(
            opts,
            surviving,
            edges,
            total_objects,
            total_relationships,
            objects_after_filter,
            relationships_after_filter,
            filters,
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_object_level(
    opts: &GraphExportOptions,
    surviving: HashMap<KirId, KirObject>,
    edges: Vec<EdgeRow>,
    degree: &HashMap<KirId, usize>,
    total_objects: usize,
    total_relationships: usize,
    objects_after_filter: usize,
    relationships_after_filter: usize,
    filters: AppliedFilters,
) -> GraphExport {
    // 5/6. node truncation by (degree desc, id asc).
    let mut kept: Vec<KirObject> = surviving.into_values().collect();
    kept.sort_by_key(|a| a.id.0);
    kept.sort_by_key(|o| std::cmp::Reverse(degree.get(&o.id).copied().unwrap_or(0)));
    let nodes_truncated = kept.len() > opts.max_nodes;
    kept.truncate(opts.max_nodes);
    let kept_ids: HashSet<KirId> = kept.iter().map(|o| o.id).collect();

    // 7. restrict edges to surviving endpoints, then truncate by (max endpoint degree desc, …).
    let mut kept_edges: Vec<EdgeRow> = edges
        .into_iter()
        .filter(|(s, t, _, _)| kept_ids.contains(s) && kept_ids.contains(t))
        .collect();
    kept_edges.sort_by(|(s1, t1, k1, _), (s2, t2, k2, _)| {
        s1.0.cmp(&s2.0)
            .then(t1.0.cmp(&t2.0))
            .then(k1.to_string().cmp(&k2.to_string()))
    });
    kept_edges.sort_by_key(|(s, t, _, _)| {
        let ds = degree.get(s).copied().unwrap_or(0);
        let dt = degree.get(t).copied().unwrap_or(0);
        std::cmp::Reverse(ds.max(dt))
    });
    let edges_truncated = kept_edges.len() > opts.max_edges;
    kept_edges.truncate(opts.max_edges);

    // 8. indices.
    let kind_index = sorted_unique(kept.iter().map(|o| o.kind.to_string()));
    let rel_kind_index = sorted_unique(kept_edges.iter().map(|(_, _, k, _)| k.to_string()));

    // 9. deterministic node order, then position map.
    kept.sort_by(|a, b| {
        a.kind
            .to_string()
            .cmp(&b.kind.to_string())
            .then(a.name.cmp(&b.name))
            .then(a.id.0.cmp(&b.id.0))
    });
    let pos: HashMap<KirId, usize> = kept.iter().enumerate().map(|(i, o)| (o.id, i)).collect();
    let kind_pos = index_lookup(&kind_index);
    let rel_kind_pos = index_lookup(&rel_kind_index);

    let nodes: Vec<Node> = kept
        .iter()
        .map(|o| Node {
            id: o.id.0.to_string(),
            name: o.name.clone(),
            kind: kind_pos[&o.kind.to_string()],
            degree: degree.get(&o.id).copied().unwrap_or(0),
            properties: opts
                .include_properties
                .iter()
                .filter_map(|k| o.properties.get(k).map(|v| (k.clone(), v.clone())))
                .collect(),
            count: None,
            first_seen: opts.include_first_seen.then_some(o.created_at),
        })
        .collect();

    let mut edges_out: Vec<Edge> = kept_edges
        .iter()
        .map(|(s, t, k, ts)| Edge {
            source: pos[s],
            target: pos[t],
            kind: rel_kind_pos[&k.to_string()],
            weight: None,
            first_seen: opts.include_first_seen.then_some(*ts),
        })
        .collect();
    edges_out.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.kind.cmp(&b.kind))
    });

    GraphExport {
        schema_version: SCHEMA_VERSION,
        workspace: opts.workspace.display().to_string(),
        generated_at: Utc::now(),
        level: "object",
        as_of: opts.as_of,
        id_space: "kir",
        group_by: None,
        counts: GraphCounts {
            total_objects,
            total_relationships,
            objects_after_filter,
            relationships_after_filter,
            returned_nodes: nodes.len(),
            returned_edges: edges_out.len(),
        },
        truncated: Truncation {
            nodes: nodes_truncated,
            node_limit: opts.max_nodes,
            edges: edges_truncated,
            edge_limit: opts.max_edges,
            selection: "degree_desc",
        },
        filters,
        kind_index,
        rel_kind_index,
        nodes,
        edges: edges_out,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_aggregate_level(
    opts: &GraphExportOptions,
    surviving: HashMap<KirId, KirObject>,
    edges: Vec<EdgeRow>,
    total_objects: usize,
    total_relationships: usize,
    objects_after_filter: usize,
    relationships_after_filter: usize,
    filters: AppliedFilters,
) -> GraphExport {
    // group each surviving object → (group id, group label).
    let group_of = |o: &KirObject| -> (String, String) {
        match opts.group_by {
            GroupBy::Kind => {
                let k = o.kind.to_string();
                (format!("kind:{k}"), k)
            }
            GroupBy::PathPrefix { depth } => {
                match o.properties.get("path").and_then(|v| v.as_str()) {
                    Some(p) if !p.is_empty() => {
                        let joined = p
                            .split('/')
                            .filter(|s| !s.is_empty())
                            .take(depth.max(1))
                            .collect::<Vec<_>>()
                            .join("/");
                        let joined = if joined.is_empty() {
                            "<unpathed>".to_string()
                        } else {
                            joined
                        };
                        (format!("path:{joined}"), joined)
                    }
                    _ => ("path:<unpathed>".to_string(), "<unpathed>".to_string()),
                }
            }
        }
    };

    // RFC 0134 — a super-node / group-edge's first-seen is the `min` `created_at` over its members.
    let earlier = |a: Option<DateTime<Utc>>, b: DateTime<Utc>| Some(a.map_or(b, |x| x.min(b)));

    let mut member_of: HashMap<KirId, String> = HashMap::new();
    // id -> (label, count, min created_at)
    let mut groups: GroupAcc = BTreeMap::new();
    for o in surviving.values() {
        let (gid, label) = group_of(o);
        member_of.insert(o.id, gid.clone());
        let e = groups.entry(gid).or_insert((label, 0, None));
        e.1 += 1;
        e.2 = earlier(e.2, o.created_at);
    }

    // collapse edges: (min group, max group, kind) -> (weight, min created_at).
    let mut collapsed: CollapsedEdges = BTreeMap::new();
    for (s, t, k, ts) in &edges {
        let (gs, gt) = (&member_of[s], &member_of[t]);
        let (a, b) = if gs <= gt { (gs, gt) } else { (gt, gs) };
        let e = collapsed
            .entry((a.clone(), b.clone(), k.to_string()))
            .or_insert((0, None));
        e.0 += 1;
        e.1 = earlier(e.1, *ts);
    }

    // group degree = distinct *other* groups connected (self-edge excluded).
    let mut neighbours: HashMap<String, HashSet<String>> = HashMap::new();
    for (a, b, _) in collapsed.keys() {
        if a != b {
            neighbours.entry(a.clone()).or_default().insert(b.clone());
            neighbours.entry(b.clone()).or_default().insert(a.clone());
        }
    }

    // node kind index: for Kind grouping it's the sorted group labels; for PathPrefix a single
    // synthetic entry so the wire shape (a `k` per node) is uniform.
    let kind_index: Vec<String> = match opts.group_by {
        GroupBy::Kind => sorted_unique(groups.values().map(|(l, _, _)| l.clone())),
        GroupBy::PathPrefix { .. } => vec!["path-prefix".to_string()],
    };
    let kind_pos = index_lookup(&kind_index);

    let mut nodes: Vec<Node> = groups
        .iter()
        .map(|(gid, (label, count, first))| Node {
            id: gid.clone(),
            name: label.clone(),
            kind: match opts.group_by {
                GroupBy::Kind => kind_pos[label],
                GroupBy::PathPrefix { .. } => 0,
            },
            degree: neighbours.get(gid).map(|s| s.len()).unwrap_or(0),
            properties: BTreeMap::new(),
            count: Some(*count),
            first_seen: if opts.include_first_seen {
                *first
            } else {
                None
            },
        })
        .collect();
    nodes.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    let node_pos: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id.clone(), i))
        .collect();

    let rel_kind_index = sorted_unique(collapsed.keys().map(|(_, _, k)| k.clone()));
    let rel_kind_pos = index_lookup(&rel_kind_index);

    let mut edges_out: Vec<Edge> = collapsed
        .iter()
        .map(|((a, b, k), (w, first))| Edge {
            source: node_pos[a],
            target: node_pos[b],
            kind: rel_kind_pos[k],
            weight: Some(*w),
            first_seen: if opts.include_first_seen {
                *first
            } else {
                None
            },
        })
        .collect();
    edges_out.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then(a.target.cmp(&b.target))
            .then(a.kind.cmp(&b.kind))
    });

    GraphExport {
        schema_version: SCHEMA_VERSION,
        workspace: opts.workspace.display().to_string(),
        generated_at: Utc::now(),
        level: "aggregate",
        as_of: opts.as_of,
        id_space: "synthetic",
        group_by: Some(match opts.group_by {
            GroupBy::Kind => "kind",
            GroupBy::PathPrefix { .. } => "path_prefix",
        }),
        counts: GraphCounts {
            total_objects,
            total_relationships,
            objects_after_filter,
            relationships_after_filter,
            returned_nodes: nodes.len(),
            returned_edges: edges_out.len(),
        },
        truncated: Truncation {
            nodes: false,
            node_limit: opts.max_nodes,
            edges: false,
            edge_limit: opts.max_edges,
            selection: "none",
        },
        filters,
        kind_index,
        rel_kind_index,
        nodes,
        edges: edges_out,
    }
}

fn sorted_unique(it: impl Iterator<Item = String>) -> Vec<String> {
    let mut v: Vec<String> = it.collect::<HashSet<_>>().into_iter().collect();
    v.sort();
    v
}

fn index_lookup(v: &[String]) -> HashMap<String, usize> {
    v.iter().enumerate().map(|(i, s)| (s.clone(), i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirEvidence, KirRelationship, SourceLocation};
    use ekos_ledger::{FactLedger, Ledger};

    fn obj(name: &str, kind: ObjectKind) -> KirObject {
        KirObject::new(name, kind)
    }

    /// A small fixture: 3 kinds, 4 rel kinds (built-in `CoupledWith` + a genuinely-custom
    /// `derived_from` — note a `Custom("CoupledWith")` would round-trip to the built-in variant
    /// through serde's untagged fallthrough, which is the real system behaviour), one object with
    /// a `path` property, one isolated object, one self-loop.
    fn seed(store: &dyn KnowledgeStore) -> Vec<KirId> {
        let a = obj("orders", ObjectKind::Table);
        let b = obj("customers", ObjectKind::Table);
        let c = KirObject::new("sql/orders.sql", ObjectKind::File)
            .with_property("path", serde_json::json!("sql/orders.sql"))
            .with_property("excerpt", serde_json::json!("SELECT * FROM orders"));
        let d = obj("alice", ObjectKind::Person);
        let e = obj("island", ObjectKind::Table); // isolated
        for o in [&a, &b, &c, &d, &e] {
            store.append_object(o).unwrap();
        }
        for r in [
            KirRelationship::new(RelationshipKind::ForeignKey, a.id, b.id),
            KirRelationship::new(RelationshipKind::Contains, c.id, a.id),
            KirRelationship::new(RelationshipKind::CoupledWith, a.id, c.id),
            KirRelationship::new(RelationshipKind::OwnedBy, a.id, d.id),
            KirRelationship::new(RelationshipKind::Custom("derived_from".into()), a.id, a.id),
        ] {
            store.append_relationship(&r).unwrap();
        }
        vec![a.id, b.id, c.id, d.id, e.id]
    }

    fn sqlite() -> (tempfile::TempDir, Box<dyn KnowledgeStore>) {
        let dir = tempfile::tempdir().unwrap();
        let l = Ledger::open(&dir.path().join("l.db")).unwrap();
        (dir, Box::new(l))
    }
    fn fact() -> (tempfile::TempDir, Box<dyn KnowledgeStore>) {
        let dir = tempfile::tempdir().unwrap();
        let l = FactLedger::open(&dir.path().join("facts")).unwrap();
        (dir, Box::new(l))
    }

    fn drop_generated_at(mut v: serde_json::Value) -> serde_json::Value {
        v.as_object_mut().unwrap().remove("generated_at");
        v
    }

    #[test]
    fn counts_match_direct_all_objects_and_relationships() {
        for (_d, store) in [sqlite(), fact()] {
            seed(&*store);
            let g = export_graph(&*store, &GraphExportOptions::default()).unwrap();
            assert_eq!(g.counts.total_objects, store.all_objects().unwrap().len());
            assert_eq!(
                g.counts.total_relationships,
                store.all_relationships().unwrap().len()
            );
            assert_eq!(g.counts.objects_after_filter, 5);
            assert_eq!(g.counts.returned_nodes, 5);
            assert_eq!(g.counts.returned_edges, 5);
        }
    }

    #[test]
    fn kind_filter_removes_exactly_the_excluded_kinds() {
        let (_d, store) = sqlite();
        seed(&*store);
        let opts = GraphExportOptions {
            kinds: Some(vec![ObjectKind::Table]),
            ..Default::default()
        };
        let g = export_graph(&*store, &opts).unwrap();
        assert_eq!(g.counts.objects_after_filter, 3); // orders, customers, island
        assert!(g.nodes.iter().all(|n| g.kind_index[n.kind] == "Table"));
        // FK (orders→customers) + the self-loop (orders→orders) both have Table endpoints.
        assert_eq!(g.counts.relationships_after_filter, 2);
    }

    #[test]
    fn rel_kind_include_and_exclude() {
        let (_d, store) = sqlite();
        seed(&*store);
        let inc = GraphExportOptions {
            rel_kinds: Some(vec![RelationshipKind::ForeignKey]),
            ..Default::default()
        };
        assert_eq!(
            export_graph(&*store, &inc)
                .unwrap()
                .counts
                .relationships_after_filter,
            1
        );
        let exc = GraphExportOptions {
            exclude_rel_kinds: vec![RelationshipKind::CoupledWith],
            ..Default::default()
        };
        let g = export_graph(&*store, &exc).unwrap();
        assert!(!g.rel_kind_index.iter().any(|k| k == "CoupledWith"));
        assert_eq!(g.counts.relationships_after_filter, 4);
    }

    #[test]
    fn degree_is_computed_post_filter() {
        let (_d, store) = sqlite();
        let ids = seed(&*store);
        let full = export_graph(&*store, &GraphExportOptions::default()).unwrap();
        let deg_a_full = full
            .nodes
            .iter()
            .find(|n| n.id == ids[0].0.to_string())
            .unwrap()
            .degree;
        // orders: FK→customers, Contains←sql, CoupledWith→sql, OwnedBy→alice, self-loop(+2) = 6
        assert_eq!(deg_a_full, 6);
        let filtered = export_graph(
            &*store,
            &GraphExportOptions {
                exclude_rel_kinds: vec![
                    RelationshipKind::CoupledWith,
                    RelationshipKind::Custom("derived_from".into()),
                ],
                ..Default::default()
            },
        )
        .unwrap();
        let deg_a = filtered
            .nodes
            .iter()
            .find(|n| n.id == ids[0].0.to_string())
            .unwrap()
            .degree;
        assert_eq!(deg_a, 3); // FK + Contains + OwnedBy
    }

    #[test]
    fn min_degree_drops_isolated_nodes() {
        let (_d, store) = sqlite();
        seed(&*store);
        let g = export_graph(
            &*store,
            &GraphExportOptions {
                min_degree: 1,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(g.counts.objects_after_filter, 4); // "island" gone
        assert!(!g.nodes.iter().any(|n| n.name == "island"));
    }

    #[test]
    fn determinism_modulo_generated_at() {
        for level in [ExportLevel::Object, ExportLevel::Aggregate] {
            let (_d, store) = sqlite();
            seed(&*store);
            let opts = GraphExportOptions {
                level,
                ..Default::default()
            };
            let a = serde_json::to_value(export_graph(&*store, &opts).unwrap()).unwrap();
            let b = serde_json::to_value(export_graph(&*store, &opts).unwrap()).unwrap();
            assert_eq!(drop_generated_at(a), drop_generated_at(b));
        }
    }

    #[test]
    fn truncation_returns_top_degree_set() {
        let (_d, store) = sqlite();
        // hub + 6 spokes; hub degree 6, each spoke degree 1.
        let hub = obj("hub", ObjectKind::Table);
        store.append_object(&hub).unwrap();
        let mut spokes = vec![];
        for i in 0..6 {
            let s = obj(&format!("spoke{i}"), ObjectKind::Table);
            store.append_object(&s).unwrap();
            store
                .append_relationship(&KirRelationship::new(
                    RelationshipKind::DependsOn,
                    s.id,
                    hub.id,
                ))
                .unwrap();
            spokes.push(s.id);
        }
        let g = export_graph(
            &*store,
            &GraphExportOptions {
                max_nodes: 3,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(g.counts.returned_nodes, 3);
        assert!(g.truncated.nodes);
        assert!(g.nodes.iter().any(|n| n.id == hub.id.0.to_string()));
        // every returned edge index is in range.
        assert!(g.edges.iter().all(|e| e.source < 3 && e.target < 3));
    }

    #[test]
    fn aggregate_by_kind_sums_match_object_level() {
        for (_d, store) in [sqlite(), fact()] {
            seed(&*store);
            let obj_lvl = export_graph(&*store, &GraphExportOptions::default()).unwrap();
            let agg = export_graph(
                &*store,
                &GraphExportOptions {
                    level: ExportLevel::Aggregate,
                    group_by: GroupBy::Kind,
                    ..Default::default()
                },
            )
            .unwrap();
            let sum_count: usize = agg.nodes.iter().map(|n| n.count.unwrap()).sum();
            let sum_w: usize = agg.edges.iter().map(|e| e.weight.unwrap()).sum();
            assert_eq!(sum_count, obj_lvl.counts.objects_after_filter);
            assert_eq!(sum_w, obj_lvl.counts.relationships_after_filter);
        }
    }

    #[test]
    fn aggregate_by_path_prefix_buckets_unpathed() {
        let (_d, store) = sqlite();
        seed(&*store); // only sql/orders.sql has a path; 4 others don't
        let g = export_graph(
            &*store,
            &GraphExportOptions {
                level: ExportLevel::Aggregate,
                group_by: GroupBy::PathPrefix { depth: 1 },
                ..Default::default()
            },
        )
        .unwrap();
        let unpathed = g.nodes.iter().find(|n| n.id == "path:<unpathed>").unwrap();
        assert_eq!(unpathed.count, Some(4));
        assert!(g.nodes.iter().any(|n| n.id == "path:sql"));
        assert!(
            g.nodes
                .iter()
                .all(|n| g.kind_index[n.kind] == "path-prefix")
        );
    }

    #[test]
    fn include_property_carries_only_named_keys() {
        let (_d, store) = sqlite();
        seed(&*store);
        let g = export_graph(
            &*store,
            &GraphExportOptions {
                include_properties: vec!["path".to_string()],
                ..Default::default()
            },
        )
        .unwrap();
        let sql = g.nodes.iter().find(|n| n.name == "sql/orders.sql").unwrap();
        assert_eq!(sql.properties.keys().collect::<Vec<_>>(), vec!["path"]);
        assert!(!sql.properties.contains_key("excerpt"));
        let orders = g.nodes.iter().find(|n| n.name == "orders").unwrap();
        assert!(orders.properties.is_empty());
    }

    #[test]
    fn kind_index_sorted_and_referenced() {
        let (_d, store) = sqlite();
        seed(&*store);
        let g = export_graph(&*store, &GraphExportOptions::default()).unwrap();
        let mut sorted = g.kind_index.clone();
        sorted.sort();
        assert_eq!(g.kind_index, sorted);
        assert!(g.nodes.iter().all(|n| n.kind < g.kind_index.len()));
        assert!(g.edges.iter().all(|e| e.kind < g.rel_kind_index.len()));
    }

    #[test]
    fn empty_store_produces_valid_empty_export() {
        let (_d, store) = fact();
        let g = export_graph(&*store, &GraphExportOptions::default()).unwrap();
        assert_eq!(g.counts.returned_nodes, 0);
        assert_eq!(g.counts.returned_edges, 0);
        assert!(!g.truncated.nodes && !g.truncated.edges);
        serde_json::to_string(&g).unwrap();
    }

    #[test]
    fn evidence_is_ignored_by_the_export() {
        // a stray evidence record must not affect node/edge counts.
        let (_d, store) = sqlite();
        seed(&*store);
        store
            .append_evidence(&KirEvidence::new(
                SourceLocation::at("x.sql", 1),
                "CREATE TABLE orders",
            ))
            .unwrap();
        let g = export_graph(&*store, &GraphExportOptions::default()).unwrap();
        assert_eq!(g.counts.returned_nodes, 5);
    }

    // ── RFC 0134 — time-travel ───────────────────────────────────────────────

    /// Seed two objects + one edge, snapshot the instant, then seed a third object + a second
    /// edge. `as_of = mid` sees only the first wave; `as_of = now` equals the unbounded export.
    #[test]
    fn as_of_returns_only_the_graph_that_existed_then() {
        for (_d, store) in [sqlite(), fact()] {
            let a = obj("orders", ObjectKind::Table);
            let b = obj("customers", ObjectKind::Table);
            store.append_object(&a).unwrap();
            store.append_object(&b).unwrap();
            store
                .append_relationship(&KirRelationship::new(
                    RelationshipKind::ForeignKey,
                    a.id,
                    b.id,
                ))
                .unwrap();
            let mid = Utc::now();
            std::thread::sleep(std::time::Duration::from_millis(8));

            let c = obj("shipments", ObjectKind::Table);
            store.append_object(&c).unwrap();
            store
                .append_relationship(&KirRelationship::new(
                    RelationshipKind::ForeignKey,
                    c.id,
                    a.id,
                ))
                .unwrap();

            let then = export_graph(
                &*store,
                &GraphExportOptions {
                    as_of: Some(mid),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(then.counts.returned_nodes, 2);
            assert_eq!(then.counts.returned_edges, 1);
            assert_eq!(then.as_of, Some(mid));
            assert!(!then.nodes.iter().any(|n| n.name == "shipments"));

            let now = export_graph(
                &*store,
                &GraphExportOptions {
                    as_of: Some(Utc::now()),
                    ..Default::default()
                },
            )
            .unwrap();
            let unbounded = export_graph(&*store, &GraphExportOptions::default()).unwrap();
            assert_eq!(now.counts.returned_nodes, unbounded.counts.returned_nodes);
            assert_eq!(now.counts.returned_edges, unbounded.counts.returned_edges);
        }
    }

    #[test]
    fn include_first_seen_stamps_nodes_and_edges_only_when_asked() {
        let (_d, store) = sqlite();
        seed(&*store);

        let off = export_graph(&*store, &GraphExportOptions::default()).unwrap();
        assert!(off.nodes.iter().all(|n| n.first_seen.is_none()));
        assert!(off.edges.iter().all(|e| e.first_seen.is_none()));

        let on = export_graph(
            &*store,
            &GraphExportOptions {
                include_first_seen: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(on.nodes.iter().all(|n| n.first_seen.is_some()));
        assert!(on.edges.iter().all(|e| e.first_seen.is_some()));
        // serde surfaces it under the short key `fs`.
        let v = serde_json::to_value(&on).unwrap();
        assert!(v["nodes"][0].get("fs").is_some());
    }

    #[test]
    fn aggregate_first_seen_is_the_member_minimum() {
        let (_d, store) = sqlite();
        let early = obj("early", ObjectKind::Table);
        store.append_object(&early).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(8));
        let late = obj("late", ObjectKind::Table);
        store.append_object(&late).unwrap();

        let g = export_graph(
            &*store,
            &GraphExportOptions {
                level: ExportLevel::Aggregate,
                group_by: GroupBy::Kind,
                include_first_seen: true,
                ..Default::default()
            },
        )
        .unwrap();
        let table = g.nodes.iter().find(|n| n.name == "Table").unwrap();
        assert_eq!(table.count, Some(2));
        assert_eq!(
            table.first_seen.unwrap(),
            early.created_at.min(late.created_at)
        );
    }

    #[test]
    fn determinism_holds_with_a_fixed_as_of() {
        let (_d, store) = sqlite();
        seed(&*store);
        let opts = GraphExportOptions {
            as_of: Some(Utc::now()),
            include_first_seen: true,
            ..Default::default()
        };
        let a = serde_json::to_value(export_graph(&*store, &opts).unwrap()).unwrap();
        let b = serde_json::to_value(export_graph(&*store, &opts).unwrap()).unwrap();
        assert_eq!(drop_generated_at(a), drop_generated_at(b));
    }
}
