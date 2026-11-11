//! RFC 0257 — a transformation drawn as a chain of steps.
//!
//! Pure over ledger objects: given the `Custom("TransformNode")` objects of one Transformation IR
//! graph (RFC 0027) and their `FeedsInto` edges, [`build`] returns the [`ChainGraph`] that
//! `ekos transformation graph --json` prints, the MCP tool `ekos_transformation_graph` returns
//! and the console draws. Nothing here reads a file or a store; the caller resolves evidence,
//! annotations and the owning definition's source text and passes them in.
//!
//! # The shape (`ekos.transformation-graph/1`)
//!
//! A mart's graph (`lakehouse marts graph`, ekos-lakehouse) is meant to print the same shape: its
//! nodes then carry `term`, `rule` and `fidelity`, which are `null` for a recovered
//! transformation. Every field:
//!
//! * `transformation` — `key` (the graph's id: `<path>#<statement>` for SQL, the file for Kettle),
//!   `kind` (`view`, `routine`, `query`, `kettle-transformation`, `kettle-job`, `python`, `sql`),
//!   `name`, `object` (the owning `View`/`Procedure` id, when there is one), `path`, `recovered`;
//! * `source` — the owning definition's text (redacted, as recovery read it) with `path` and
//!   `start_line`, or `null` when no span is recorded;
//! * `nodes[]` — `id`, `index`, `type` (`Source`, `Filter`, `Join`, `Aggregate`, `Calculate`,
//!   `Sink`, `Unmapped`, or `Statement` for a collapsed group), `label`, `summary` (the template
//!   sentence), `group`, `statement`, `columns_in`, `columns_out`, `columns_used`, `source_text`,
//!   `span`, `term`, `rule`, `fidelity`, `status` (`unmapped` or `null`), `evidence[]`,
//!   `annotations[]`, `members[]` (a collapsed group's node ids);
//! * `edges[]` — `from`, `to`, `kind` (`data` for a `FeedsInto` edge, `order` between collapsed
//!   statements of a routine body, which run in that order);
//! * `spans[]` — the node ↔ source map: `node`, `start`, `end` (UTF-16 offsets into
//!   `source.text`, so a browser can slice it), `start_line`, `end_line` (file lines);
//! * `groups[]` — `id`, `label`, `size`, `collapsed`;
//! * `counts` — `nodes`, `mapped`, `unmapped`, `shown`; `rows` — whether row counts exist.
//!
//! # Large graphs (RFC 0257 open question 4)
//!
//! A routine's nodes are grouped by the body statement they came from. Above [`COLLAPSE_AT`]
//! nodes, every statement is drawn as one `Statement` node unless it is named in
//! [`Expand::Statements`]; [`Expand::All`] draws everything.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ekos_kir::{KirId, KirObject, ObjectKind};
use serde::Serialize;
use serde_json::Value;

/// The format tag of [`ChainGraph`].
pub const FORMAT: &str = "ekos.transformation-graph/1";

/// Above this many nodes a graph with more than one statement is drawn collapsed by statement.
pub const COLLAPSE_AT: usize = 40;

const TRANSFORM_NODE: &str = "TransformNode";

/// Which statements of a large graph to draw in full.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Expand {
    /// Collapse when the graph is larger than [`COLLAPSE_AT`].
    #[default]
    Auto,
    /// Never collapse.
    All,
    /// Collapse every statement except these (0-based body statement numbers).
    Statements(Vec<u32>),
}

/// One evidence record, as the caller resolved it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvidenceView {
    pub path: String,
    pub line: Option<u32>,
    pub fragment: String,
    pub confidence: f32,
}

/// The owning definition's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceText {
    pub path: String,
    pub start_line: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TransformationInfo {
    pub key: String,
    pub kind: String,
    pub name: String,
    pub object: Option<String>,
    pub path: String,
    pub recovered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Span {
    pub node: String,
    pub start: usize,
    pub end: usize,
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChainNode {
    pub id: String,
    pub index: Option<u32>,
    #[serde(rename = "type")]
    pub node_type: String,
    pub label: String,
    pub summary: String,
    pub group: String,
    pub statement: Option<u32>,
    pub columns_in: Vec<String>,
    pub columns_out: Vec<String>,
    pub columns_used: Vec<String>,
    pub source_text: String,
    pub span: Option<Span>,
    /// A mart's term (RFC 0228); `null` for a recovered transformation.
    pub term: Option<String>,
    /// The rule that resolved the term; `null` for a recovered transformation.
    pub rule: Option<String>,
    /// `exact` or `drafted` for a mart; `null` for a recovered transformation.
    pub fidelity: Option<String>,
    /// `unmapped` for a step recovery did not understand.
    pub status: Option<String>,
    pub evidence: Vec<EvidenceView>,
    pub annotations: Vec<Value>,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainEdge {
    pub from: String,
    pub to: String,
    pub kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChainGroup {
    pub id: String,
    pub label: String,
    pub size: usize,
    pub collapsed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub nodes: usize,
    pub mapped: usize,
    pub unmapped: usize,
    pub shown: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowsInfo {
    pub available: bool,
    pub reason: String,
}

/// What `ekos transformation graph --json` prints.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChainGraph {
    pub format: &'static str,
    pub transformation: TransformationInfo,
    pub source: Option<SourceText>,
    pub nodes: Vec<ChainNode>,
    pub edges: Vec<ChainEdge>,
    pub spans: Vec<Span>,
    pub groups: Vec<ChainGroup>,
    pub collapsed: bool,
    pub counts: Counts,
    pub rows: RowsInfo,
}

// ── Selecting a graph ────────────────────────────────────────────────────────

/// Whether `o` is a Transformation IR node.
pub fn is_transform_node(o: &KirObject) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == TRANSFORM_NODE)
}

fn custom_kind(o: &KirObject) -> &str {
    match &o.kind {
        ObjectKind::Custom(k) => k,
        _ => "",
    }
}

fn prop<'a>(o: &'a KirObject, key: &str) -> &'a str {
    o.properties
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn prop_u64(o: &KirObject, key: &str) -> Option<u64> {
    o.properties.get(key).and_then(Value::as_u64)
}

/// The graph a node belongs to: its name without the trailing `:<index>` (`lower_to_kir` names a
/// node `<source_path>:<index>`).
pub fn graph_key(name: &str) -> Option<&str> {
    let (key, index) = name.rsplit_once(':')?;
    (!index.is_empty() && index.bytes().all(|b| b.is_ascii_digit())).then_some(key)
}

fn node_index(o: &KirObject) -> Option<u32> {
    o.name.rsplit_once(':').and_then(|(_, i)| i.parse().ok())
}

/// The file a graph key names (`a.sql#3` → `a.sql`).
fn key_file(key: &str) -> &str {
    key.split_once('#').map_or(key, |(f, _)| f)
}

/// A definition name compared the way SQL compares unquoted names: case and quotes ignored, and
/// an unqualified name matches the last part of a qualified one.
fn same_name(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('"', "").to_ascii_lowercase();
    let (a, b) = (norm(a), norm(b));
    if a == b {
        return true;
    }
    let last = |s: &str| s.rsplit('.').next().unwrap_or_default().to_string();
    (!a.contains('.') || !b.contains('.')) && last(&a) == last(&b)
}

/// The objects of one graph, chosen by [`select`].
#[derive(Debug, Clone)]
pub struct Selection<'a> {
    pub key: String,
    /// Ordered by node index.
    pub nodes: Vec<&'a KirObject>,
    /// The `View` or `Procedure` the graph's statement defines, when there is one.
    pub owner: Option<&'a KirObject>,
}

/// The graph `target` names: a `TransformNode` (its own graph), a `View` or a `Procedure` (the
/// graph recovered from its definition). `Err` is a sentence for the person.
pub fn select<'a>(
    objects: &'a [KirObject],
    target: &'a KirObject,
) -> Result<Selection<'a>, String> {
    let key = match custom_kind(target) {
        TRANSFORM_NODE => graph_key(&target.name)
            .ok_or_else(|| format!("`{}` is not a Transformation IR node name", target.name))?
            .to_string(),
        "View" => owned_key(objects, target, "view")?,
        "Procedure" => owned_key(objects, target, "routine")?,
        other => {
            return Err(format!(
                "{} is a {}; a transformation is a TransformNode, View or Procedure",
                target.id.0,
                if other.is_empty() {
                    "built-in kind"
                } else {
                    other
                }
            ));
        }
    };
    let mut nodes: Vec<&KirObject> = objects
        .iter()
        .filter(|o| is_transform_node(o) && graph_key(&o.name) == Some(key.as_str()))
        .collect();
    nodes.sort_by_key(|o| node_index(o));
    let owner = match custom_kind(target) {
        TRANSFORM_NODE => owner_of(objects, &nodes),
        _ => Some(target),
    };
    Ok(Selection { key, nodes, owner })
}

/// Same-named definitions of `def`'s kind in its file, in file order: which of them `def` is.
fn ordinal_in_file(objects: &[KirObject], def: &KirObject) -> u64 {
    let file = prop(def, "source_path");
    let start = prop_u64(def, "span_start").unwrap_or(0);
    objects
        .iter()
        .filter(|o| {
            o.kind == def.kind
                && prop(o, "source_path") == file
                && same_name(&o.name, &def.name)
                && prop_u64(o, "span_start").unwrap_or(0) < start
        })
        .count() as u64
}

fn owned_key(objects: &[KirObject], def: &KirObject, owner_kind: &str) -> Result<String, String> {
    let file = prop(def, "source_path");
    let ordinal = ordinal_in_file(objects, def);
    let mut keys: BTreeMap<u64, &str> = BTreeMap::new();
    for o in objects.iter().filter(|o| is_transform_node(o)) {
        let Some(key) = graph_key(&o.name) else {
            continue;
        };
        if prop(o, "owner_kind") == owner_kind
            && same_name(prop(o, "owner"), &def.name)
            && key_file(key) == file
        {
            keys.insert(prop_u64(o, "owner_ordinal").unwrap_or(0), key);
        }
    }
    if let Some(key) = keys.get(&ordinal).or_else(|| keys.values().next()) {
        return Ok((*key).to_string());
    }
    // A ledger recovered before RFC 0257: a view's graph ends in a `Sink` named after it.
    objects
        .iter()
        .filter(|o| is_transform_node(o) && prop(o, "node_type") == "Sink")
        .filter(|o| same_name(prop(o, "object_name"), &def.name))
        .filter_map(|o| graph_key(&o.name))
        .find(|k| key_file(k) == file)
        .map(str::to_string)
        .ok_or_else(|| {
            format!(
                "no Transformation IR graph was recovered for `{}` (re-run `ekos recover` if the \
                 ledger predates RFC 0257)",
                def.name
            )
        })
}

fn owner_of<'a>(objects: &'a [KirObject], nodes: &[&KirObject]) -> Option<&'a KirObject> {
    let first = nodes.first()?;
    let kind = match prop(first, "owner_kind") {
        "view" => "View",
        "routine" => "Procedure",
        _ => return None,
    };
    let name = prop(first, "owner");
    let file = key_file(graph_key(&first.name)?);
    let ordinal = prop_u64(first, "owner_ordinal").unwrap_or(0);
    let mut defs: Vec<&KirObject> = objects
        .iter()
        .filter(|o| custom_kind(o) == kind && prop(o, "source_path") == file)
        .filter(|o| same_name(&o.name, name))
        .collect();
    defs.sort_by_key(|o| prop_u64(o, "span_start").unwrap_or(0));
    defs.get(ordinal as usize).or(defs.first()).copied()
}

// ── Building the chain ───────────────────────────────────────────────────────

/// Everything [`build`] needs besides the graph itself.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// FeedsInto edges among the selected nodes, `(from, to)`.
    pub edges: Vec<(KirId, KirId)>,
    /// Resolved evidence per node id.
    pub evidence: HashMap<KirId, Vec<EvidenceView>>,
    /// Annotations (RFC 0247, as `ekos annotate list --json` renders them) per target id.
    pub annotations: HashMap<String, Vec<Value>>,
    /// The owning definition's text, when its span is recorded and the file still holds it.
    pub source: Option<SourceText>,
    pub expand: Expand,
}

/// The chain of one selected graph.
pub fn build(selection: &Selection<'_>, ctx: &Context) -> ChainGraph {
    let nodes = &selection.nodes;
    let ids: std::collections::HashSet<KirId> = nodes.iter().map(|o| o.id).collect();
    let edges: Vec<(KirId, KirId)> = ctx
        .edges
        .iter()
        .filter(|(a, b)| ids.contains(a) && ids.contains(b))
        .copied()
        .collect();
    let columns = propagate_columns(nodes, &edges, selection.owner);
    // A join side is named by its table; a side that is an earlier step is named by number.
    let labels: HashMap<u32, String> = nodes
        .iter()
        .filter_map(|o| {
            let i = node_index(o)?;
            let name = match prop(o, "node_type") {
                "Source" => tick(&label(o)),
                _ => format!("the result of step {i}"),
            };
            Some((i, name))
        })
        .collect();
    let spans = span_map(nodes, ctx.source.as_ref());

    let mut full: Vec<ChainNode> = nodes
        .iter()
        .map(|o| {
            let (columns_in, columns_out) = columns.get(&o.id).cloned().unwrap_or_default();
            ChainNode {
                id: o.id.0.to_string(),
                index: node_index(o),
                node_type: prop(o, "node_type").to_string(),
                label: label(o),
                summary: summary(o, &labels),
                group: group_id(o),
                statement: prop_u64(o, "statement").map(|n| n as u32),
                columns_in,
                columns_out,
                columns_used: columns_used(o),
                source_text: source_text(o),
                span: spans.get(&o.id).cloned(),
                term: None,
                rule: None,
                fidelity: None,
                status: (prop(o, "node_type") == "Unmapped").then(|| "unmapped".to_string()),
                evidence: ctx.evidence.get(&o.id).cloned().unwrap_or_default(),
                annotations: ctx
                    .annotations
                    .get(&o.id.0.to_string())
                    .cloned()
                    .unwrap_or_default(),
                members: Vec::new(),
            }
        })
        .collect();
    let mut data_edges: Vec<ChainEdge> = edges
        .iter()
        .map(|(a, b)| ChainEdge {
            from: a.0.to_string(),
            to: b.0.to_string(),
            kind: "data",
        })
        .collect();

    let groups = groups_of(&full);
    let collapse = collapsed_groups(&groups, full.len(), &ctx.expand);
    if !collapse.is_empty() {
        (full, data_edges) = collapse_nodes(full, data_edges, &groups, &collapse);
    }
    let unmapped = nodes
        .iter()
        .filter(|o| prop(o, "node_type") == "Unmapped")
        .count();
    let span_list: Vec<Span> = full.iter().filter_map(|n| n.span.clone()).collect();
    ChainGraph {
        format: FORMAT,
        transformation: info(selection),
        source: ctx.source.clone(),
        counts: Counts {
            nodes: nodes.len(),
            mapped: nodes.len() - unmapped,
            unmapped,
            shown: full.len(),
        },
        nodes: full,
        edges: data_edges,
        spans: span_list,
        groups: groups
            .iter()
            .map(|(id, members)| ChainGroup {
                id: id.clone(),
                label: group_label(id),
                size: members.len(),
                collapsed: collapse.contains(id),
            })
            .collect(),
        collapsed: !collapse.is_empty(),
        rows: RowsInfo {
            available: false,
            reason: "row counts are measured for a mart only (RFC 0257 §3.3): a recovered \
                     transformation's source may not be runnable"
                .into(),
        },
    }
}

fn info(selection: &Selection<'_>) -> TransformationInfo {
    let first = selection.nodes.first();
    let owner_kind = first.map(|o| prop(o, "owner_kind")).unwrap_or_default();
    let file = key_file(&selection.key);
    let kind = match owner_kind {
        "view" | "routine" => owner_kind,
        _ => kind_from_path(&selection.key),
    };
    let name = match (selection.owner, first) {
        (Some(o), _) => o.name.clone(),
        (None, Some(n)) if !prop(n, "owner").is_empty() => prop(n, "owner").to_string(),
        _ => selection.key.clone(),
    };
    TransformationInfo {
        key: selection.key.clone(),
        kind: kind.to_string(),
        name,
        object: selection.owner.map(|o| o.id.0.to_string()),
        path: file.to_string(),
        recovered: true,
    }
}

fn kind_from_path(key: &str) -> &'static str {
    let file = key_file(key).to_ascii_lowercase();
    let has_statement = key.contains('#');
    if file.ends_with(".ktr") {
        "kettle-transformation"
    } else if file.ends_with(".kjb") {
        "kettle-job"
    } else if file.ends_with(".py") {
        "python"
    } else if has_statement {
        "query"
    } else {
        "sql"
    }
}

fn group_id(o: &KirObject) -> String {
    prop_u64(o, "statement").map_or_else(|| "main".to_string(), |k| format!("statement:{k}"))
}

fn group_label(id: &str) -> String {
    match id
        .strip_prefix("statement:")
        .and_then(|k| k.parse::<u32>().ok())
    {
        Some(k) => format!("statement {}", k + 1),
        None => "steps".to_string(),
    }
}

/// Group ids in first-seen order with their member node ids.
fn groups_of(nodes: &[ChainNode]) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for n in nodes {
        match out.iter_mut().find(|(g, _)| *g == n.group) {
            Some((_, members)) => members.push(n.id.clone()),
            None => out.push((n.group.clone(), vec![n.id.clone()])),
        }
    }
    out
}

fn collapsed_groups(
    groups: &[(String, Vec<String>)],
    total: usize,
    expand: &Expand,
) -> BTreeSet<String> {
    if groups.len() < 2 {
        return BTreeSet::new();
    }
    let keep: Vec<String> = match expand {
        Expand::All => return BTreeSet::new(),
        Expand::Auto if total <= COLLAPSE_AT => return BTreeSet::new(),
        Expand::Auto => Vec::new(),
        Expand::Statements(list) => list.iter().map(|k| format!("statement:{k}")).collect(),
    };
    groups
        .iter()
        .map(|(g, _)| g.clone())
        .filter(|g| !keep.contains(g))
        .collect()
}

/// Replaces the nodes of each collapsed group by one `Statement` node; data edges are redirected
/// to it, and consecutive collapsed statements are joined by `order` edges.
fn collapse_nodes(
    nodes: Vec<ChainNode>,
    edges: Vec<ChainEdge>,
    groups: &[(String, Vec<String>)],
    collapse: &BTreeSet<String>,
) -> (Vec<ChainNode>, Vec<ChainEdge>) {
    let mut c = Collapser::default();
    for (group, members) in groups {
        if collapse.contains(group) {
            c.collapsed(statement_node(group, members, &nodes));
        } else {
            c.out
                .extend(nodes.iter().filter(|n| &n.group == group).cloned());
            c.previous = None;
        }
    }
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut new_edges: Vec<ChainEdge> = Vec::new();
    for e in edges {
        let from = c.redirect.get(&e.from).cloned().unwrap_or(e.from);
        let to = c.redirect.get(&e.to).cloned().unwrap_or(e.to);
        if from != to && seen.insert((from.clone(), to.clone())) {
            new_edges.push(ChainEdge {
                from,
                to,
                kind: "data",
            });
        }
    }
    new_edges.extend(c.order);
    (c.out, new_edges)
}

/// The nodes drawn so far while collapsing, and where each hidden node now points.
#[derive(Default)]
struct Collapser {
    out: Vec<ChainNode>,
    redirect: HashMap<String, String>,
    /// The last collapsed statement drawn, when nothing expanded came after it.
    previous: Option<String>,
    order: Vec<ChainEdge>,
}

impl Collapser {
    fn collapsed(&mut self, node: ChainNode) {
        // A run of statements none of which recovery understood is one block: a PL/pgSQL body
        // split by the SQL front end is mostly such statements, one step each.
        if self.previous.is_some()
            && is_opaque(&node)
            && let Some(last) = self.out.last_mut().filter(|l| is_opaque(l))
        {
            for m in &node.members {
                self.redirect.insert(m.clone(), last.id.clone());
            }
            absorb(last, node);
            return;
        }
        for m in &node.members {
            self.redirect.insert(m.clone(), node.id.clone());
        }
        if let Some(prev) = self.previous.replace(node.id.clone()) {
            self.order.push(ChainEdge {
                from: prev,
                to: node.id.clone(),
                kind: "order",
            });
        }
        self.out.push(node);
    }
}

fn steps(n: usize) -> String {
    if n == 1 {
        "1 step".into()
    } else {
        format!("{n} steps")
    }
}

fn is_opaque(n: &ChainNode) -> bool {
    n.node_type == "Statement" && n.status.is_some()
}

/// Folds the collapsed, not-understood statement `next` into the block `last`.
fn absorb(last: &mut ChainNode, next: ChainNode) {
    last.members.extend(next.members);
    last.annotations.extend(next.annotations);
    let first = last.statement.unwrap_or(0) + 1;
    let to = next.statement.unwrap_or(0) + 1;
    let n = last.members.len();
    last.label = format!("statements {first}–{to} · {n} steps");
    last.summary = format!("{n} step(s) in statements {first}–{to}, none understood by recovery");
    last.span = match (last.span.take(), next.span) {
        (Some(a), Some(b)) => Some(Span {
            node: a.node,
            start: a.start.min(b.start),
            end: a.end.max(b.end),
            start_line: a.start_line.min(b.start_line),
            end_line: a.end_line.max(b.end_line),
        }),
        (a, b) => a.or(b.map(|mut s| {
            s.node = last.id.clone();
            s
        })),
    };
}

fn statement_node(group: &str, members: &[String], nodes: &[ChainNode]) -> ChainNode {
    let inner: Vec<&ChainNode> = nodes.iter().filter(|n| n.group == group).collect();
    let unmapped = inner.iter().filter(|n| n.status.is_some()).count();
    let mut types: Vec<&str> = inner.iter().map(|n| n.node_type.as_str()).collect();
    types.dedup();
    let summary = if unmapped == inner.len() {
        format!("{} step(s), none understood by recovery", inner.len())
    } else {
        format!("{} step(s): {}", inner.len(), types.join(" → "))
    };
    ChainNode {
        id: group.to_string(),
        index: None,
        node_type: "Statement".into(),
        label: format!("{} · {}", group_label(group), steps(inner.len())),
        summary,
        group: group.to_string(),
        statement: group
            .strip_prefix("statement:")
            .and_then(|k| k.parse().ok()),
        columns_in: Vec::new(),
        columns_out: Vec::new(),
        columns_used: Vec::new(),
        source_text: String::new(),
        span: merged_span(group, &inner),
        term: None,
        rule: None,
        fidelity: None,
        status: (unmapped == inner.len()).then(|| "unmapped".to_string()),
        evidence: Vec::new(),
        annotations: inner.iter().flat_map(|n| n.annotations.clone()).collect(),
        members: members.to_vec(),
    }
}

fn merged_span(group: &str, inner: &[&ChainNode]) -> Option<Span> {
    let spans: Vec<&Span> = inner.iter().filter_map(|n| n.span.as_ref()).collect();
    let first = spans.iter().min_by_key(|s| s.start)?;
    let last = spans.iter().max_by_key(|s| s.end)?;
    Some(Span {
        node: group.to_string(),
        start: first.start,
        end: last.end,
        start_line: first.start_line,
        end_line: last.end_line,
    })
}

// ── Columns ──────────────────────────────────────────────────────────────────

fn string_list(v: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = v else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|c| {
            c.as_str()
                .or_else(|| c.get("name").and_then(Value::as_str))
                .map(str::to_string)
        })
        .collect()
}

fn push_unique(out: &mut Vec<String>, items: impl IntoIterator<Item = String>) {
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
}

/// Columns into and out of every node, in node order (recovery emits a node after the nodes that
/// feed it). What a node outputs: a `Source` its recorded columns; `Filter`/`Join` what comes in;
/// an `Aggregate` its groups and aggregates; a `Calculate` what comes in plus its output; a `Sink`
/// its recorded columns, else the owning view's, else what comes in.
fn propagate_columns(
    nodes: &[&KirObject],
    edges: &[(KirId, KirId)],
    owner: Option<&KirObject>,
) -> HashMap<KirId, (Vec<String>, Vec<String>)> {
    let referenced = referenced_columns(nodes);
    let mut out: HashMap<KirId, (Vec<String>, Vec<String>)> = HashMap::new();
    for o in nodes {
        let mut inputs: Vec<String> = Vec::new();
        for (from, _) in edges.iter().filter(|(_, to)| *to == o.id) {
            if let Some((_, produced)) = out.get(from) {
                push_unique(&mut inputs, produced.clone());
            }
        }
        let mut produced = produced_columns(o, &inputs, owner);
        if produced.is_empty() && prop(o, "node_type") == "Source" {
            produced = referenced.get(&o.id).cloned().unwrap_or_default();
        }
        out.insert(o.id, (inputs, produced));
    }
    out
}

/// What a `Source` recorded no columns for is read for: the columns other steps name through
/// its alias or table name (`o.docstatus` for the read aliased `o`, kept qualified so two
/// tables' `id` stay two columns after a join). An unqualified
/// column cannot be placed and is left out, never guessed.
fn referenced_columns(nodes: &[&KirObject]) -> HashMap<KirId, Vec<String>> {
    let used: Vec<String> = nodes.iter().flat_map(|o| columns_used(o)).collect();
    let mut out = HashMap::new();
    for o in nodes.iter().filter(|o| prop(o, "node_type") == "Source") {
        let mut qualifiers = vec![prop(o, "object_name").to_ascii_lowercase()];
        if !prop(o, "alias").is_empty() {
            qualifiers.push(prop(o, "alias").to_ascii_lowercase());
        }
        let mut cols = Vec::new();
        for u in &used {
            let Some((q, _)) = u.rsplit_once('.') else {
                continue;
            };
            if qualifiers.contains(&q.to_ascii_lowercase()) {
                push_unique(&mut cols, [u.clone()]);
            }
        }
        out.insert(o.id, cols);
    }
    out
}

fn produced_columns(o: &KirObject, inputs: &[String], owner: Option<&KirObject>) -> Vec<String> {
    match prop(o, "node_type") {
        "Source" => string_list(o.properties.get("columns")),
        "Filter" | "Join" => inputs.to_vec(),
        "Aggregate" => {
            let mut cols = string_list(o.properties.get("group_by"));
            push_unique(&mut cols, agg_outputs(o));
            cols
        }
        "Calculate" => {
            let mut cols = inputs.to_vec();
            push_unique(&mut cols, [prop(o, "output").to_string()]);
            cols
        }
        "Sink" => {
            let own = string_list(o.properties.get("columns"));
            let view = owner
                .map(|v| string_list(v.properties.get("columns")))
                .unwrap_or_default();
            [own, view, inputs.to_vec()]
                .into_iter()
                .find(|c| !c.is_empty())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn aggs(o: &KirObject) -> Vec<(String, String, String)> {
    let Some(Value::Array(items)) = o.properties.get("aggs") else {
        return Vec::new();
    };
    items
        .iter()
        .map(|a| {
            let field = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            (field("output"), field("func"), field("arg"))
        })
        .collect()
}

fn agg_outputs(o: &KirObject) -> Vec<String> {
    aggs(o).into_iter().map(|(out, _, _)| out).collect()
}

fn join_keys(o: &KirObject) -> Vec<(String, String)> {
    let Some(Value::Array(items)) = o.properties.get("keys") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|pair| {
            let a = pair.get(0)?.as_str()?.to_string();
            let b = pair.get(1)?.as_str()?.to_string();
            Some((a, b))
        })
        .collect()
}

/// The columns a step reads.
fn columns_used(o: &KirObject) -> Vec<String> {
    let mut out = Vec::new();
    match prop(o, "node_type") {
        "Filter" | "Calculate" => push_unique(&mut out, identifiers(prop(o, "excerpt"))),
        "Join" => {
            for (a, b) in join_keys(o) {
                push_unique(&mut out, [a, b]);
            }
        }
        "Aggregate" => {
            push_unique(&mut out, string_list(o.properties.get("group_by")));
            for (_, _, arg) in aggs(o) {
                push_unique(&mut out, identifiers(&arg));
            }
        }
        _ => {}
    }
    out
}

const KEYWORDS: &[&str] = &[
    "and",
    "or",
    "not",
    "in",
    "is",
    "null",
    "like",
    "ilike",
    "between",
    "case",
    "when",
    "then",
    "else",
    "end",
    "true",
    "false",
    "exists",
    "select",
    "from",
    "where",
    "as",
    "any",
    "all",
    "distinct",
    "some",
    "interval",
    "date",
    "timestamp",
    "on",
    "join",
    "similar",
    "to",
    "escape",
    "current_date",
    "current_timestamp",
    "now",
    "array",
    "row",
    "over",
    "partition",
    "by",
    "order",
    "asc",
    "desc",
    "nulls",
    "first",
    "last",
    "cast",
    "with",
    "without",
    "time",
    "zone",
    "text",
    "integer",
    "int",
    "numeric",
    "boolean",
    "varchar",
    "bigint",
    "isnull",
    "notnull",
];

/// Identifiers in an expression: words outside string literals that are not SQL keywords, not
/// numbers and not function names (a word followed by `(`).
fn identifiers(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i = skip_literal(&chars, i);
            continue;
        }
        if !(c.is_alphabetic() || c == '_' || c == '"') {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_ident_char(chars[i]) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let next = chars[i..].iter().find(|c| !c.is_whitespace());
        if next != Some(&'(') && is_column_word(&word) {
            push_unique(&mut out, [word.replace('"', "")]);
        }
    }
    out
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '$')
}

fn is_column_word(word: &str) -> bool {
    let bare = word.replace('"', "");
    !bare.is_empty()
        && !bare.starts_with('$')
        && !KEYWORDS.contains(&bare.to_ascii_lowercase().as_str())
}

/// Index just past the string literal starting at `i` (`''` is an escaped quote).
fn skip_literal(chars: &[char], mut i: usize) -> usize {
    i += 1;
    while i < chars.len() {
        if chars[i] == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                i += 2;
                continue;
            }
            return i + 1;
        }
        i += 1;
    }
    i
}

// ── Labels and sentences ─────────────────────────────────────────────────────

fn clip(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max - 1).collect::<String>())
    }
}

/// A short label for a node box.
fn label(o: &KirObject) -> String {
    match prop(o, "node_type") {
        "Source" | "Sink" => prop(o, "object_name").to_string(),
        "Filter" => format!("where {}", clip(prop(o, "excerpt"), 24)),
        "Join" => format!("{} join", prop(o, "join_kind").to_ascii_lowercase()),
        "Aggregate" => {
            let groups = string_list(o.properties.get("group_by"));
            if groups.is_empty() {
                "aggregate".into()
            } else {
                format!("group by {}", clip(&groups.join(", "), 20))
            }
        }
        "Calculate" => format!("= {}", clip(prop(o, "output"), 22)),
        "Unmapped" => "not understood".into(),
        other => other.to_string(),
    }
}

/// The source text a node was recovered from, as recovery recorded it.
fn source_text(o: &KirObject) -> String {
    match prop(o, "node_type") {
        "Source" | "Sink" => prop(o, "object_name").to_string(),
        "Filter" | "Calculate" => prop(o, "excerpt").to_string(),
        "Unmapped" => prop(o, "raw").to_string(),
        "Join" => join_keys(o)
            .iter()
            .map(|(a, b)| format!("{a} = {b}"))
            .collect::<Vec<_>>()
            .join(" AND "),
        "Aggregate" => string_list(o.properties.get("group_by")).join(", "),
        _ => String::new(),
    }
}

fn tick(s: &str) -> String {
    format!("`{}`", s.trim())
}

/// The step's template sentence (RFC 0257 §3.2), deterministic per node type.
pub fn summary_of(o: &KirObject) -> String {
    summary(o, &HashMap::new())
}

fn summary(o: &KirObject, labels: &HashMap<u32, String>) -> String {
    match prop(o, "node_type") {
        "Source" => source_sentence(o),
        "Sink" => match prop(o, "owner_kind") {
            "view" => format!("defines the view {}", tick(prop(o, "object_name"))),
            _ => format!("writes {}", tick(prop(o, "object_name"))),
        },
        "Filter" => filter_sentence(prop(o, "excerpt")),
        "Join" => join_sentence(o, labels),
        "Aggregate" => aggregate_sentence(o),
        "Calculate" => format!(
            "computes {} as {}",
            tick(prop(o, "output")),
            tick(prop(o, "excerpt"))
        ),
        "Unmapped" => format!("not understood by recovery: {}", prop(o, "reason")),
        other => format!("a `{other}` step"),
    }
}

fn source_sentence(o: &KirObject) -> String {
    let alias = prop(o, "alias");
    if alias.is_empty() {
        format!("reads {}", tick(prop(o, "object_name")))
    } else {
        format!("reads {} as {}", tick(prop(o, "object_name")), tick(alias))
    }
}

fn side(o: &KirObject, key: &str, labels: &HashMap<u32, String>) -> String {
    prop_u64(o, key)
        .and_then(|i| labels.get(&(i as u32)))
        .cloned()
        .unwrap_or_else(|| "the input".to_string())
}

fn join_sentence(o: &KirObject, labels: &HashMap<u32, String>) -> String {
    let (l, r) = (side(o, "left", labels), side(o, "right", labels));
    let keys = join_keys(o);
    let on = if keys.is_empty() {
        "a condition recovery did not record as column pairs".to_string()
    } else {
        keys.iter()
            .map(|(a, b)| format!("{} = {}", tick(a), tick(b)))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    match prop(o, "join_kind") {
        "Left" => format!("keeps every row of {l}, adding {r} where {on}"),
        "Right" => format!("keeps every row of {r}, adding {l} where {on}"),
        "Full" => format!("keeps every row of {l} and of {r}, matched where {on}"),
        "Cross" => format!("pairs every row of {l} with every row of {r}"),
        _ => {
            format!("joins {l} and {r} where {on}; rows without a match on both sides are dropped")
        }
    }
}

fn aggregate_sentence(o: &KirObject) -> String {
    let groups: Vec<String> = string_list(o.properties.get("group_by"))
        .iter()
        .map(|g| tick(g))
        .collect();
    let computed: Vec<String> = aggs(o)
        .iter()
        .map(|(out, func, arg)| {
            let call = format!("{func}({arg})");
            if out.eq_ignore_ascii_case(&call) {
                tick(&call)
            } else {
                format!("{} as {}", tick(&call), tick(out))
            }
        })
        .collect();
    match (groups.is_empty(), computed.is_empty()) {
        (true, _) => format!("reduces all rows to one, computing {}", computed.join(", ")),
        (false, true) => format!("keeps one row per {}", groups.join(", ")),
        (false, false) => format!(
            "groups rows by {} and computes {}",
            groups.join(", "),
            computed.join(", ")
        ),
    }
}

/// `keeps rows where …`: each top-level `AND` term in words when it is a comparison, an `IN`
/// list or a null test, and as written otherwise. A condition with a top-level `OR` is quoted
/// whole: splitting it on `AND` would change its meaning.
fn filter_sentence(condition: &str) -> String {
    let condition = strip_parens(condition.trim());
    if split_top(condition, " or ").len() > 1 {
        return format!("keeps rows where {}", tick(condition));
    }
    let parts: Vec<String> = split_top(condition, " and ")
        .iter()
        .map(|t| describe_term(strip_parens(t.trim())))
        .collect();
    format!("keeps rows where {}", parts.join(" and "))
}

/// Splits `s` at `sep` (matched case-insensitively) outside parentheses and string literals.
fn split_top<'a>(s: &'a str, sep: &str) -> Vec<&'a str> {
    let lower = s.to_ascii_lowercase();
    let lower = lower.as_bytes();
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let (mut depth, mut quoted, mut start, mut i) = (0i32, false, 0usize, 0usize);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => quoted = !quoted,
            b'(' if !quoted => depth += 1,
            b')' if !quoted => depth -= 1,
            _ if !quoted && depth == 0 && lower[i..].starts_with(sep.as_bytes()) => {
                out.push(&s[start..i]);
                i += sep.len();
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&s[start..]);
    out
}

/// `(x)` → `x` when the parentheses wrap the whole expression.
fn strip_parens(s: &str) -> &str {
    let t = s.trim();
    if !(t.starts_with('(') && t.ends_with(')')) {
        return t;
    }
    let inner = &t[1..t.len() - 1];
    let mut depth = 0i32;
    for b in inner.bytes() {
        depth += match b {
            b'(' => 1,
            b')' => -1,
            _ => 0,
        };
        if depth < 0 {
            return t;
        }
    }
    if depth == 0 { strip_parens(inner) } else { t }
}

fn value_words(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
        v[1..v.len() - 1].replace("''", "'")
    } else {
        tick(v)
    }
}

/// One `AND` term in words. A term with a top-level `OR` is quoted as written: neither side of
/// the `OR` is a fact about the rows on its own (found on LedgerSMB's `order__search`, whose
/// `(in_date_from IS NULL OR o.transdate >= in_date_from)` read as a comparison).
fn describe_term(term: &str) -> String {
    if split_top(term, " or ").len() > 1 {
        return tick(term);
    }
    describe_null_test(term)
        .or_else(|| describe_in_list(term))
        .or_else(|| describe_comparison(term))
        .unwrap_or_else(|| tick(term))
}

fn describe_null_test(term: &str) -> Option<String> {
    let lower = term.to_ascii_lowercase();
    if let Some(col) = lower.strip_suffix(" is not null") {
        return Some(format!("{} has a value", tick(&term[..col.len()])));
    }
    let col = lower.strip_suffix(" is null")?;
    Some(format!("{} has no value", tick(&term[..col.len()])))
}

fn describe_in_list(term: &str) -> Option<String> {
    let lower = term.to_ascii_lowercase();
    let (pos, negated, sep_len) = match lower.find(" not in (") {
        Some(p) => (p, true, " not in (".len()),
        None => (lower.find(" in (")?, false, " in (".len()),
    };
    let list = term[pos + sep_len..].strip_suffix(')')?;
    if list.trim_start().to_ascii_lowercase().starts_with("select") {
        return None;
    }
    let values: Vec<String> = split_top(list, ",")
        .iter()
        .map(|v| value_words(v))
        .collect();
    let verb = if negated {
        "is not one of"
    } else {
        "is one of"
    };
    Some(format!(
        "{} {verb} {}",
        tick(&term[..pos]),
        values.join(", ")
    ))
}

const COMPARISONS: &[(&str, &str)] = &[
    ("<>", "is not"),
    ("!=", "is not"),
    (">=", "is at least"),
    ("<=", "is at most"),
    ("=", "is"),
    (">", "is greater than"),
    ("<", "is less than"),
];

fn describe_comparison(term: &str) -> Option<String> {
    for (op, words) in COMPARISONS {
        let parts = split_top(term, op);
        if let [left, right] = parts.as_slice() {
            if left.trim().is_empty() || right.trim().is_empty() {
                return None;
            }
            return Some(format!("{} {words} {}", tick(left), value_words(right)));
        }
    }
    None
}

// ── Node ↔ source span map ───────────────────────────────────────────────────

/// Where each node's recorded text appears in the owning definition. Recovery reprints what it
/// parsed, so the match ignores case and runs of whitespace; a text that still is not found has no
/// span (never a guessed one). A node is searched for after the previous node's match first, so
/// a table read twice is placed at its own occurrence when the order allows it.
fn span_map(nodes: &[&KirObject], source: Option<&SourceText>) -> HashMap<KirId, Span> {
    let mut out = HashMap::new();
    let Some(source) = source else {
        return out;
    };
    let folded = Folded::new(&source.text);
    let mut cursor = 0usize;
    for o in nodes {
        let needle = span_needle(o);
        if needle.trim().is_empty() {
            continue;
        }
        let Some((start, end)) = folded
            .find(&needle, cursor)
            .or_else(|| folded.find(&needle, 0))
        else {
            continue;
        };
        cursor = start;
        let line =
            |byte: usize| source.start_line + source.text[..byte].matches('\n').count() as u32;
        out.insert(
            o.id,
            Span {
                node: o.id.0.to_string(),
                start: utf16_len(&source.text[..start]),
                end: utf16_len(&source.text[..end]),
                start_line: line(start),
                end_line: line(end),
            },
        );
    }
    out
}

fn span_needle(o: &KirObject) -> String {
    match prop(o, "node_type") {
        "Join" => join_keys(o)
            .first()
            .map(|(a, b)| format!("{a} = {b}"))
            .unwrap_or_default(),
        "Aggregate" => string_list(o.properties.get("group_by"))
            .first()
            .cloned()
            .unwrap_or_default(),
        _ => source_text(o),
    }
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// A text with whitespace runs folded to one space and ASCII case folded, keeping the byte
/// offset in the original of every folded byte.
struct Folded {
    text: String,
    origin: Vec<usize>,
    len: usize,
}

impl Folded {
    fn new(original: &str) -> Self {
        let mut text = String::new();
        let mut origin = Vec::new();
        let mut in_space = false;
        for (i, c) in original.char_indices() {
            if c.is_whitespace() {
                if !in_space {
                    text.push(' ');
                    origin.push(i);
                }
                in_space = true;
                continue;
            }
            in_space = false;
            let lower = c.to_ascii_lowercase();
            for _ in 0..lower.len_utf8() {
                origin.push(i);
            }
            text.push(lower);
        }
        Self {
            text,
            origin,
            len: original.len(),
        }
    }

    /// Byte range in the original of the first match of `needle` at or after original byte `from`.
    fn find(&self, needle: &str, from: usize) -> Option<(usize, usize)> {
        let needle = Folded::new(needle.trim()).text;
        let start_at = self.origin.iter().position(|&o| o >= from)?;
        let at = self.text.get(start_at..)?.find(&needle)? + start_at;
        let start = self.origin[at];
        let end = self
            .origin
            .get(at + needle.len())
            .copied()
            .unwrap_or(self.len);
        Some((start, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn node(path: &str, index: u32, props: Value) -> KirObject {
        let mut o = KirObject::new(
            format!("{path}:{index}"),
            ObjectKind::Custom(TRANSFORM_NODE.into()),
        );
        o.id = KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("{path}:{index}").as_bytes(),
        ));
        o.properties = props.as_object().unwrap().clone().into_iter().collect();
        o
    }

    fn view_graph() -> Vec<KirObject> {
        let owner = json!({"owner": "open_orders", "owner_kind": "view", "owner_ordinal": 0});
        let with = |mut v: Value| {
            v.as_object_mut()
                .unwrap()
                .extend(owner.as_object().unwrap().clone());
            v
        };
        vec![
            node(
                "db/v.sql#0",
                0,
                with(
                    json!({"node_type": "Source", "object_name": "oe", "columns": [], "alias": "o"}),
                ),
            ),
            node(
                "db/v.sql#0",
                1,
                with(json!({"node_type": "Source", "object_name": "entity", "columns": []})),
            ),
            node(
                "db/v.sql#0",
                2,
                with(
                    json!({"node_type": "Join", "left": 0, "right": 1, "keys": [["o.entity_id", "entity.id"]], "join_kind": "Left"}),
                ),
            ),
            node(
                "db/v.sql#0",
                3,
                with(
                    json!({"node_type": "Filter", "excerpt": "o.docstatus IN ('CO', 'CL') AND o.closed IS NULL"}),
                ),
            ),
            node(
                "db/v.sql#0",
                4,
                with(json!({"node_type": "Sink", "object_name": "open_orders", "columns": []})),
            ),
        ]
    }

    fn view_object() -> KirObject {
        let mut v = KirObject::new("open_orders", ObjectKind::Custom("View".into()));
        v.properties.insert("source_path".into(), json!("db/v.sql"));
        v.properties.insert("span_start".into(), json!(0));
        v.properties.insert("columns".into(), json!(["id", "name"]));
        v
    }

    fn edges(nodes: &[KirObject]) -> Vec<(KirId, KirId)> {
        vec![
            (nodes[0].id, nodes[2].id),
            (nodes[1].id, nodes[2].id),
            (nodes[2].id, nodes[3].id),
            (nodes[3].id, nodes[4].id),
        ]
    }

    const VIEW_SQL: &str = "CREATE VIEW open_orders AS\nSELECT o.id, e.name\n  FROM oe o\n  LEFT JOIN entity ON o.entity_id = entity.id\n WHERE o.docstatus IN ('CO','CL') AND o.closed IS NULL;";

    #[test]
    fn filter_sentences_follow_the_template() {
        assert_eq!(
            filter_sentence("docstatus IN ('CO', 'CL')"),
            "keeps rows where `docstatus` is one of CO, CL"
        );
        assert_eq!(
            filter_sentence("(a.x = 'y') AND b IS NOT NULL AND c <> 3"),
            "keeps rows where `a.x` is y and `b` has a value and `c` is not `3`"
        );
        assert_eq!(
            filter_sentence("a = 1 OR b = 2"),
            "keeps rows where `a = 1 OR b = 2`"
        );
        assert_eq!(
            filter_sentence("id NOT IN (SELECT id FROM x)"),
            "keeps rows where `id NOT IN (SELECT id FROM x)`"
        );
        assert_eq!(
            filter_sentence("a = 1 AND (d IS NULL OR t >= d)"),
            "keeps rows where `a` is `1` and `d IS NULL OR t >= d`"
        );
        assert_eq!(
            filter_sentence("name = 'it''s'"),
            "keeps rows where `name` is it's"
        );
    }

    #[test]
    fn identifiers_skip_literals_keywords_and_functions() {
        assert_eq!(
            identifiers("lower(e.name) = 'and x' AND o.closed IS NULL AND $1 > amount"),
            vec!["e.name", "o.closed", "amount"]
        );
    }

    #[test]
    fn a_view_selects_its_own_graph_and_a_node_finds_its_view() {
        let mut objects = view_graph();
        objects.push(node(
            "db/v.sql#1",
            0,
            json!({"node_type": "Source", "object_name": "other"}),
        ));
        objects.push(view_object());
        let view = objects.last().unwrap().clone();
        let by_view = select(&objects, &view).unwrap();
        assert_eq!(by_view.key, "db/v.sql#0");
        assert_eq!(by_view.nodes.len(), 5);
        let by_node = select(&objects, &objects[3]).unwrap();
        assert_eq!(by_node.key, "db/v.sql#0");
        assert_eq!(by_node.owner.map(|o| o.name.as_str()), Some("open_orders"));
        let table = KirObject::new("oe", ObjectKind::Table);
        assert!(select(&objects, &table).is_err());
    }

    #[test]
    fn an_overloaded_routine_picks_its_own_definition() {
        let mk = |stmt: &str, ordinal: u32| {
            node(
                &format!("db/f.sql#{stmt}"),
                0,
                json!({"node_type": "Source", "object_name": format!("t{ordinal}"),
                       "owner": "f", "owner_kind": "routine", "owner_ordinal": ordinal}),
            )
        };
        let proc = |start: u64| {
            let mut p = KirObject::new("public.f", ObjectKind::Custom("Procedure".into()));
            p.id = KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("p{start}").as_bytes(),
            ));
            p.properties.insert("source_path".into(), json!("db/f.sql"));
            p.properties.insert("span_start".into(), json!(start));
            p
        };
        let objects = vec![mk("0", 0), mk("1", 1), proc(10), proc(500)];
        assert_eq!(select(&objects, &objects[3]).unwrap().key, "db/f.sql#1");
        assert_eq!(select(&objects, &objects[2]).unwrap().key, "db/f.sql#0");
        let owner = select(&objects, &objects[1]).unwrap().owner.unwrap();
        assert_eq!(owner.id, objects[3].id);
    }

    #[test]
    fn a_ledger_without_owner_properties_still_finds_a_views_sink() {
        let objects: Vec<KirObject> = view_graph()
            .into_iter()
            .map(|mut o| {
                o.properties.remove("owner");
                o.properties.remove("owner_kind");
                o
            })
            .chain([view_object()])
            .collect();
        let view = objects.last().unwrap();
        assert_eq!(select(&objects, view).unwrap().key, "db/v.sql#0");
    }

    #[test]
    fn the_chain_has_sentences_columns_spans_and_no_mart_fields() {
        let mut objects = view_graph();
        objects.push(view_object());
        let selection = select(&objects, objects.last().unwrap()).unwrap();
        let ctx = Context {
            edges: edges(&objects),
            source: Some(SourceText {
                path: "db/v.sql".into(),
                start_line: 10,
                text: VIEW_SQL.into(),
            }),
            ..Context::default()
        };
        let g = build(&selection, &ctx);
        assert_eq!(g.format, FORMAT);
        assert_eq!(g.transformation.kind, "view");
        assert_eq!(g.nodes.len(), 5);
        assert_eq!(g.edges.len(), 4);
        assert!(!g.collapsed);
        let filter = &g.nodes[3];
        assert_eq!(
            filter.summary,
            "keeps rows where `o.docstatus` is one of CO, CL and `o.closed` has no value"
        );
        assert_eq!(filter.columns_used, vec!["o.docstatus", "o.closed"]);
        assert!(
            g.nodes[2]
                .summary
                .starts_with("keeps every row of `oe`, adding `entity`")
        );
        assert_eq!(g.nodes[4].columns_out, vec!["id", "name"]);
        assert_eq!(
            g.nodes[0].columns_out,
            vec!["o.entity_id", "o.docstatus", "o.closed"]
        );
        assert_eq!(g.nodes[1].columns_out, vec!["entity.id"]);
        assert_eq!(g.nodes[4].summary, "defines the view `open_orders`");
        assert!(
            g.nodes
                .iter()
                .all(|n| n.term.is_none() && n.fidelity.is_none())
        );
        // The join's keys and the source tables are found in the text; the reprinted filter is
        // not (recovery wrote `'CO', 'CL'`, the file `'CO','CL'`), so it has no span.
        let join = g.nodes[2].span.as_ref().unwrap();
        let text: Vec<u16> = VIEW_SQL.encode_utf16().collect();
        assert_eq!(
            String::from_utf16(&text[join.start..join.end]).unwrap(),
            "o.entity_id = entity.id"
        );
        assert_eq!(join.start_line, 13);
        assert!(g.nodes[3].span.is_none());
        assert_eq!(
            g.spans.len(),
            g.nodes.iter().filter(|n| n.span.is_some()).count()
        );
        assert!(!g.rows.available);
    }

    fn routine(statements: u32, per: u32) -> Vec<KirObject> {
        let mut out = Vec::new();
        for s in 0..statements {
            for k in 0..per {
                let i = s * per + k;
                out.push(node(
                    "db/r.sql#0",
                    i,
                    json!({"node_type": if k == 0 { "Unmapped" } else { "Filter" },
                           "excerpt": format!("x = {i}"), "raw": "IF", "reason": "control flow",
                           "owner": "r", "owner_kind": "routine", "owner_ordinal": 0, "statement": s}),
                ));
            }
        }
        out
    }

    #[test]
    fn a_large_routine_collapses_by_statement_and_expands_on_request() {
        let objects = routine(5, 10);
        let mut ctx = Context {
            edges: (1..10)
                .map(|k| (objects[k - 1].id, objects[k].id))
                .chain([(objects[9].id, objects[10].id)])
                .collect(),
            ..Context::default()
        };
        let selection = select(&objects, &objects[0]).unwrap();
        let g = build(&selection, &ctx);
        assert!(g.collapsed);
        assert_eq!(g.nodes.len(), 5);
        assert!(
            g.nodes
                .iter()
                .all(|n| n.node_type == "Statement" && n.members.len() == 10)
        );
        let data: Vec<_> = g.edges.iter().filter(|e| e.kind == "data").collect();
        assert_eq!(
            data.len(),
            1,
            "statement 1 feeds statement 2: {:?}",
            g.edges
        );
        assert_eq!(g.edges.iter().filter(|e| e.kind == "order").count(), 4);
        assert_eq!(g.counts.nodes, 50);
        assert_eq!(g.counts.unmapped, 5);

        ctx.expand = Expand::Statements(vec![1]);
        let g = build(&selection, &ctx);
        assert_eq!(g.nodes.len(), 4 + 10);
        assert!(
            g.groups
                .iter()
                .any(|gr| gr.id == "statement:1" && !gr.collapsed)
        );

        ctx.expand = Expand::All;
        assert_eq!(build(&selection, &ctx).nodes.len(), 50);

        let small = routine(2, 5);
        let sel = select(&small, &small[0]).unwrap();
        assert!(!build(&sel, &Context::default()).collapsed);
    }

    /// A PL/pgSQL body split by the SQL front end: one not-understood step per statement. A run
    /// of them is one block, so collapsing actually makes the routine smaller.
    #[test]
    fn a_run_of_not_understood_statements_is_one_block() {
        let mut objects: Vec<KirObject> = (0..45)
            .map(|s| {
                node(
                    "db/p.sql#0",
                    s,
                    json!({"node_type": "Unmapped", "raw": "IF x THEN", "reason": "control flow",
                           "owner": "p", "owner_kind": "routine", "owner_ordinal": 0, "statement": s}),
                )
            })
            .collect();
        for k in 0..3 {
            objects.push(node(
                "db/p.sql#0",
                45 + k,
                json!({"node_type": "Source", "object_name": "t", "owner": "p",
                       "owner_kind": "routine", "owner_ordinal": 0, "statement": 45}),
            ));
        }
        objects.push(node(
            "db/p.sql#0",
            48,
            json!({"node_type": "Unmapped", "raw": "RETURN", "reason": "control flow",
                   "owner": "p", "owner_kind": "routine", "owner_ordinal": 0, "statement": 46}),
        ));
        let selection = select(&objects, &objects[0]).unwrap();
        let g = build(&selection, &Context::default());
        assert!(g.collapsed);
        let labels: Vec<&str> = g.nodes.iter().map(|n| n.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "statements 1–45 · 45 steps",
                "statement 46 · 3 steps",
                "statement 47 · 1 step"
            ]
        );
        assert_eq!(g.nodes[0].members.len(), 45);
        assert_eq!(g.edges.iter().filter(|e| e.kind == "order").count(), 2);
    }

    #[test]
    fn annotations_are_shown_on_their_step() {
        let objects = view_graph();
        let selection = select(&objects, &objects[0]).unwrap();
        let mut ctx = Context {
            edges: edges(&objects),
            ..Context::default()
        };
        ctx.annotations.insert(
            objects[3].id.0.to_string(),
            vec![json!({"kind": "comment", "text": "cancelled orders too?"})],
        );
        let g = build(&selection, &ctx);
        assert_eq!(g.nodes[3].annotations.len(), 1);
        assert!(g.nodes[2].annotations.is_empty());
    }

    #[test]
    fn graph_keys_and_kinds() {
        assert_eq!(graph_key("/a/b.sql#3:12"), Some("/a/b.sql#3"));
        assert_eq!(graph_key("jobs/x.ktr:0"), Some("jobs/x.ktr"));
        assert_eq!(graph_key("C:"), None);
        assert_eq!(kind_from_path("jobs/x.ktr"), "kettle-transformation");
        assert_eq!(kind_from_path("q.sql#2"), "query");
    }
}
