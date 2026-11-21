//! RFC 0264 §"Transformation IR": a Talend job's data components lowered into the shared
//! Transformation IR (RFC 0027), one graph per job version.
//!
//! Only data components (category input, output, row/set transform, join, flow, elt) become
//! nodes; orchestration, connections, code and logging stay `TalendComponent` objects. A tMap
//! becomes a chain: one `Join` per lookup, then per output an optional `Filter` and one
//! `Calculate` per output column. Nodes carry extra properties (`component`, `column_refs`,
//! table, query, …) that the analyzer adds to the lowered objects by node index.

use std::collections::{BTreeMap, HashMap};

use ekos_semantic::transform_ir::{
    AggExpr, JoinKind, NodeId, TransformGraph, TransformNode, TransformOrigin,
};
use ekos_talend::{Category, Component, Job, LinkKind, Mapper, Param, qualified_refs};
use serde_json::{Value, json};

/// The IR source kind of a Talend job graph.
pub const SOURCE_KIND: &str = "talend-job";

/// One lowered job.
pub struct LoweredJob {
    pub graph: TransformGraph,
    /// Extra properties per node index.
    pub extra: Vec<BTreeMap<String, Value>>,
    pub data_components: usize,
    /// Data components lowered to something other than `Unmapped`.
    pub mapped_components: usize,
}

/// Where rows enter and leave one component's nodes.
#[derive(Default)]
struct Segment {
    /// Incoming link label → nodes the link feeds. `"*"` for every label.
    entries: HashMap<String, Vec<usize>>,
    /// Outgoing connector name (a tMap output, or `"*"`) → nodes rows leave from.
    exits: HashMap<String, Vec<usize>>,
    /// Join node index → which entry label is its left and right input.
    joins: Vec<(usize, Option<String>, String)>,
}

struct Builder<'a> {
    job: &'a Job,
    nodes: Vec<TransformNode>,
    extra: Vec<BTreeMap<String, Value>>,
    edges: Vec<(NodeId, NodeId)>,
    /// (reading component, incoming link label) → the component the link comes from. Keyed by
    /// the reader because two inlined copies of a joblet keep the same labels (RFC 0263 §Joblets).
    label_source: HashMap<(String, String), String>,
}

impl Builder<'_> {
    fn push(
        &mut self,
        component: &Component,
        node: TransformNode,
        extra: BTreeMap<String, Value>,
    ) -> usize {
        let mut props = extra;
        props.insert("component".into(), json!(component.unique_name));
        props.insert("component_type".into(), json!(component.component_type));
        props.insert("line".into(), json!(component.line));
        if let Some(origin) = &component.joblet {
            props.insert("joblet".into(), json!(origin));
        }
        self.nodes.push(node);
        self.extra.push(props);
        self.nodes.len() - 1
    }

    fn edge(&mut self, from: usize, to: usize) {
        let e = (NodeId(from as u32), NodeId(to as u32));
        if !self.edges.contains(&e) {
            self.edges.push(e);
        }
    }

    /// `[{input, column, component}]` for the `qualifier.column` references in `text`. A
    /// qualifier that is a link label resolves to the component the link comes from; any other
    /// (`input_row`) to `upstream`.
    fn column_refs(
        &self,
        reader: &str,
        text: &str,
        qualifiers: &[&str],
        upstream: Option<&str>,
    ) -> Value {
        let refs: Vec<Value> = qualified_refs(text, qualifiers)
            .into_iter()
            .map(|(input, column)| {
                let component = self
                    .label_source
                    .get(&(reader.to_string(), input.clone()))
                    .map(String::as_str)
                    .or(upstream);
                json!({
                    "input": input,
                    "column": column,
                    "component": component,
                })
            })
            .collect();
        Value::Array(refs)
    }
}

/// Lowers one job.
pub fn lower_job(job: &Job) -> LoweredJob {
    let mut b = Builder {
        job,
        nodes: Vec::new(),
        extra: Vec::new(),
        edges: Vec::new(),
        label_source: job
            .links
            .iter()
            .filter_map(|l| Some(((l.to.clone(), l.label.clone()?), l.from.clone())))
            .collect(),
    };
    let data: Vec<&Component> = job
        .components
        .iter()
        .filter(|c| c.category.is_data())
        .collect();
    let mut segments: HashMap<&str, Segment> = HashMap::new();
    let mut mapped = 0;
    for c in &data {
        let segment = lower_component(&mut b, c);
        if !segment
            .exits
            .values()
            .flatten()
            .all(|i| matches!(b.nodes[*i], TransformNode::Unmapped { .. }))
        {
            mapped += 1;
        }
        segments.insert(c.unique_name.as_str(), segment);
    }
    wire(&mut b, &segments);
    link_hash_tables(&mut b, &data);
    LoweredJob {
        graph: TransformGraph {
            nodes: b.nodes,
            edges: b.edges,
            origin: TransformOrigin {
                source_path: job.path.clone(),
                source_kind: SOURCE_KIND.to_string(),
                extracted_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
            },
        },
        extra: b.extra,
        data_components: data.len(),
        mapped_components: mapped,
    }
}

fn single(node: usize) -> Segment {
    let mut s = Segment::default();
    s.entries.insert("*".into(), vec![node]);
    s.exits.insert("*".into(), vec![node]);
    s
}

fn lower_component(b: &mut Builder, c: &Component) -> Segment {
    if let Some(mapper) = &c.mapper {
        return lower_tmap(b, c, mapper);
    }
    let (node, extra) = match c.logical.as_str() {
        "db_input" => db_source(b.job, c),
        "db_output" | "db_output_bulk" | "db_output_bulk_exec" | "db_bulk_exec" | "db_scd" => {
            db_sink(b.job, c)
        }
        "tFilterRow" => filter_row(b, c),
        "tAggregateRow" | "tAggregateSortedRow" => aggregate(c),
        "tUniqRow" => uniq_row(c),
        "tRowGenerator" | "tFixedFlowInput" => generated_source(b.job, c),
        "tHashInput" | "tBufferInput" => hash(b.job, c, true),
        "tHashOutput" | "tBufferOutput" => hash(b.job, c, false),
        _ if c.category == Category::Joblet => (
            TransformNode::Unmapped {
                raw: c.component_type.clone(),
                reason: "joblet use not expanded (see TALEND006)".into(),
            },
            BTreeMap::new(),
        ),
        _ if c.category == Category::Input => file_io(b.job, c, true),
        _ if c.category == Category::Output => file_io(b.job, c, false),
        _ => (
            TransformNode::Unmapped {
                raw: c.component_type.clone(),
                reason: format!(
                    "{} ({}) has no IR lowering yet",
                    c.component_type,
                    c.category.as_str()
                ),
            },
            BTreeMap::new(),
        ),
    };
    let index = b.push(c, node, extra);
    single(index)
}

fn param_text<'a>(c: &'a Component, name: &str) -> Option<&'a str> {
    c.params
        .get(name)
        .and_then(Param::text)
        .filter(|v| !v.is_empty())
}

fn schema_columns(c: &Component) -> Vec<String> {
    c.schemas
        .iter()
        .find(|s| s.connector == "FLOW")
        .or_else(|| c.schemas.first())
        .map(|s| s.columns.iter().map(|col| col.name.clone()).collect())
        .unwrap_or_default()
}

/// A Java string parameter folded over the job's context, as `{text, complete, holes, context_used}`.
fn folded(job: &Job, expr: &str) -> (String, Value) {
    let f = job.fold(expr);
    let value = json!({
        "text": f.text,
        "complete": f.complete(),
        "holes": f.holes,
        "context_used": f.context_used,
    });
    (f.text, value)
}

/// The schema a database component works in: its own `SCHEMA_DB`, else the shared connection's.
fn db_schema(job: &Job, c: &Component) -> Option<String> {
    let own = param_text(c, "SCHEMA_DB");
    let via = param_text(c, "CONNECTION")
        .and_then(|name| job.component(name))
        .and_then(|conn| param_text(conn, "SCHEMA_DB"));
    let expr = own.or(via)?;
    let f = job.fold(expr);
    (f.complete() && !f.text.is_empty()).then_some(f.text)
}

fn db_table(job: &Job, c: &Component, extra: &mut BTreeMap<String, Value>) -> Option<String> {
    let expr = param_text(c, "TABLE")?;
    let (text, value) = folded(job, expr);
    extra.insert("table".into(), value);
    (!text.is_empty()).then_some(text)
}

fn db_common(job: &Job, c: &Component) -> (Option<String>, BTreeMap<String, Value>) {
    let mut extra = BTreeMap::new();
    if let Some(engine) = &c.engine {
        extra.insert("engine".into(), json!(engine));
    }
    if let Some(schema) = db_schema(job, c) {
        extra.insert("schema".into(), json!(schema));
    }
    let table = db_table(job, c, &mut extra);
    (table, extra)
}

fn db_source(job: &Job, c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    let (table, mut extra) = db_common(job, c);
    if let Some(query) = param_text(c, "QUERY") {
        extra.insert("query".into(), folded(job, query).1);
    }
    let Some(object_name) = table else {
        return unmapped(c, "database input without a table name", extra);
    };
    (
        TransformNode::Source {
            object_name,
            columns: schema_columns(c),
            alias: None,
        },
        extra,
    )
}

fn db_sink(job: &Job, c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    let (table, mut extra) = db_common(job, c);
    if let Some(action) = param_text(c, "DATA_ACTION") {
        extra.insert("data_action".into(), json!(action));
    }
    if let Some(action) = param_text(c, "TABLE_ACTION") {
        extra.insert("table_action".into(), json!(action));
    }
    extra.insert("keys".into(), json!(output_keys(c)));
    let Some(object_name) = table else {
        return unmapped(c, "database output without a table name", extra);
    };
    (
        TransformNode::Sink {
            object_name,
            columns: schema_columns(c),
        },
        extra,
    )
}

/// The key columns an output updates by: the field options when they are used, else the
/// schema's key columns.
fn output_keys(c: &Component) -> Vec<String> {
    if param_text(c, "USE_FIELD_OPTIONS") == Some("true") {
        return c
            .params
            .get("FIELD_OPTIONS")
            .map(Param::rows)
            .unwrap_or_default()
            .iter()
            .filter(|r| r.get("UPDATE_KEY").map(String::as_str) == Some("true"))
            .filter_map(|r| r.get("SCHEMA_COLUMN").cloned())
            .collect();
    }
    c.schemas
        .iter()
        .find(|s| s.connector == "FLOW")
        .map(|s| {
            s.columns
                .iter()
                .filter(|col| col.key)
                .map(|col| col.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn file_io(job: &Job, c: &Component, input: bool) -> (TransformNode, BTreeMap<String, Value>) {
    let mut extra = BTreeMap::new();
    let Some(expr) = param_text(c, "FILENAME") else {
        return unmapped(c, "file component without a file name", extra);
    };
    let (text, value) = folded(job, expr);
    extra.insert("file".into(), value);
    if let Some(list) = iterating_file_list(job, expr) {
        extra.insert("iterated_by".into(), list);
    }
    let columns = schema_columns(c);
    let node = if input {
        TransformNode::Source {
            object_name: text,
            columns,
            alias: None,
        }
    } else {
        TransformNode::Sink {
            object_name: text,
            columns,
        }
    };
    (node, extra)
}

/// A file name read from `globalMap.get("<tFileList>_CURRENT_FILE…")`: the `tFileList` that
/// iterates it, with its folded directory and mask. The file itself is only known at run time.
fn iterating_file_list(job: &Job, expr: &str) -> Option<Value> {
    let at = expr.find("_CURRENT_FILE")?;
    let start = expr[..at].rfind('"')? + 1;
    let list = job.component(&expr[start..at])?;
    let fold_param = |name: &str| param_text(list, name).map(|e| folded(job, e).1);
    let masks: Vec<String> = list
        .params
        .get("FILES")
        .map(Param::rows)
        .unwrap_or_default()
        .iter()
        .filter_map(|r| r.get("FILEMASK").cloned())
        .collect();
    Some(json!({
        "component": list.unique_name,
        "directory": fold_param("DIRECTORY"),
        "masks": masks,
    }))
}

fn generated_source(job: &Job, c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    (
        TransformNode::Source {
            object_name: format!("{}.{}", job.name, c.unique_name),
            columns: schema_columns(c),
            alias: None,
        },
        BTreeMap::from([("generated_rows".to_string(), json!(true))]),
    )
}

/// `tHashOutput` / `tHashInput`: an in-memory table inside the job, named after the job.
fn hash(job: &Job, c: &Component, input: bool) -> (TransformNode, BTreeMap<String, Value>) {
    let object_name = format!("hash:{}", job.name);
    let columns = schema_columns(c);
    let extra = BTreeMap::from([("in_memory".to_string(), json!(true))]);
    let node = if input {
        TransformNode::Source {
            object_name,
            columns,
            alias: None,
        }
    } else {
        TransformNode::Sink {
            object_name,
            columns,
        }
    };
    (node, extra)
}

fn filter_row(b: &Builder, c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    let condition = if param_text(c, "USE_ADVANCED") == Some("true") {
        param_text(c, "ADVANCED_COND")
            .map(strip_line_comments)
            .unwrap_or_default()
    } else {
        simple_conditions(c)
    };
    let upstream = b
        .job
        .links
        .iter()
        .find(|l| l.to == c.unique_name && l.kind == LinkKind::Main)
        .map(|l| l.from.as_str());
    let mut extra = BTreeMap::new();
    extra.insert(
        "column_refs".into(),
        b.column_refs(&c.unique_name, &condition, &["input_row"], upstream),
    );
    (TransformNode::Filter { condition }, extra)
}

fn strip_line_comments(code: &str) -> String {
    code.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The condition table: `column function operator value`, joined by the logical operator.
fn simple_conditions(c: &Component) -> String {
    let op = param_text(c, "LOGICAL_OP").unwrap_or("&&");
    c.params
        .get("CONDITIONS")
        .map(Param::rows)
        .unwrap_or_default()
        .iter()
        .map(|r| {
            let get = |k: &str| r.get(k).map(String::as_str).unwrap_or_default();
            let function = get("FUNCTION");
            let column = if function.is_empty() || function == "EMPTY" {
                format!("input_row.{}", get("INPUT_COLUMN"))
            } else {
                format!("{function}(input_row.{})", get("INPUT_COLUMN"))
            };
            format!("{column} {} {}", get("OPERATOR"), get("RVALUE"))
        })
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

fn aggregate(c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    let rows = |name: &str| {
        c.params
            .get(name)
            .map(Param::rows)
            .unwrap_or_default()
            .to_vec()
    };
    let group_by = rows("GROUPBYS")
        .iter()
        .filter_map(|r| {
            r.get("INPUT_COLUMN")
                .or_else(|| r.get("OUTPUT_COLUMN"))
                .cloned()
        })
        .collect();
    let aggs = rows("OPERATIONS")
        .iter()
        .map(|r| AggExpr {
            output: r.get("OUTPUT_COLUMN").cloned().unwrap_or_default(),
            func: r.get("FUNCTION").cloned().unwrap_or_default(),
            arg: r.get("INPUT_COLUMN").cloned().unwrap_or_default(),
        })
        .collect();
    (TransformNode::Aggregate { group_by, aggs }, BTreeMap::new())
}

fn uniq_row(c: &Component) -> (TransformNode, BTreeMap<String, Value>) {
    let keys: Vec<String> = c
        .params
        .get("UNIQUE_KEY")
        .map(Param::rows)
        .unwrap_or_default()
        .iter()
        .filter(|r| r.get("KEY_ATTRIBUTE").map(String::as_str) == Some("true"))
        .filter_map(|r| r.get("SCHEMA_COLUMN").cloned())
        .collect();
    let condition = format!("first row per ({})", keys.join(", "));
    (
        TransformNode::Filter { condition },
        BTreeMap::from([("unique_keys".to_string(), json!(keys))]),
    )
}

fn unmapped(
    c: &Component,
    reason: &str,
    extra: BTreeMap<String, Value>,
) -> (TransformNode, BTreeMap<String, Value>) {
    (
        TransformNode::Unmapped {
            raw: c.component_type.clone(),
            reason: reason.to_string(),
        },
        extra,
    )
}

/// A tMap: `Join` per lookup, then per output an optional `Filter` and a `Calculate` per column.
fn lower_tmap(b: &mut Builder, c: &Component, mapper: &Mapper) -> Segment {
    let mut segment = Segment::default();
    let inputs: Vec<&str> = mapper.inputs.iter().map(|i| i.name.as_str()).collect();
    let mut qualifiers = inputs.clone();
    qualifiers.push("Var");
    let main = mapper.inputs.first().map(|i| i.name.clone());
    // Joins: J1 = main ⋈ lookup1, Ji = J(i-1) ⋈ lookup i.
    let mut core: Option<usize> = None;
    for lookup in mapper.inputs.iter().skip(1) {
        let keys: Vec<(String, String)> = lookup
            .entries
            .iter()
            .filter_map(|e| Some((e.expression.clone()?, format!("{}.{}", lookup.name, e.name))))
            .collect();
        let key_text = keys
            .iter()
            .map(|(l, _)| l.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let mut extra = BTreeMap::new();
        extra.insert("lookup".into(), json!(lookup.name));
        extra.insert("matching_mode".into(), json!(lookup.matching_mode));
        extra.insert("lookup_mode".into(), json!(lookup.lookup_mode));
        extra.insert(
            "column_refs".into(),
            b.column_refs(&c.unique_name, &key_text, &qualifiers, None),
        );
        if let Some(f) = &lookup.filter {
            extra.insert("lookup_filter".into(), json!(f));
        }
        let kind = if lookup.inner_join {
            JoinKind::Inner
        } else {
            JoinKind::Left
        };
        // left/right are filled in by `wire` once upstream nodes are known.
        let node = TransformNode::Join {
            left: NodeId(0),
            right: NodeId(0),
            keys,
            kind,
        };
        let index = b.push(c, node, extra);
        let left_label = match core {
            Some(prev) => {
                b.edge(prev, index);
                segment.joins.push((index, None, lookup.name.clone()));
                None
            }
            None => main.clone(),
        };
        if let Some(label) = &left_label {
            segment
                .entries
                .entry(label.clone())
                .or_default()
                .push(index);
            segment
                .joins
                .push((index, Some(label.clone()), lookup.name.clone()));
        }
        segment
            .entries
            .entry(lookup.name.clone())
            .or_default()
            .push(index);
        core = Some(index);
    }
    for output in &mapper.outputs {
        let exits = lower_tmap_output(
            b,
            c,
            output,
            &qualifiers,
            core,
            main.as_deref(),
            &mut segment,
        );
        segment.exits.insert(output.name.clone(), exits);
    }
    segment
}

fn lower_tmap_output(
    b: &mut Builder,
    c: &Component,
    output: &ekos_talend::MapperOutput,
    qualifiers: &[&str],
    core: Option<usize>,
    main: Option<&str>,
    segment: &mut Segment,
) -> Vec<usize> {
    let mut flags = BTreeMap::new();
    flags.insert("tmap_output".to_string(), json!(output.name));
    if output.reject {
        flags.insert("reject".into(), json!(true));
    }
    if output.reject_inner_join {
        flags.insert("reject_inner_join".into(), json!(true));
    }
    let head = output.filter.as_ref().map(|condition| {
        let mut extra = flags.clone();
        extra.insert(
            "column_refs".into(),
            b.column_refs(&c.unique_name, condition, qualifiers, None),
        );
        b.push(
            c,
            TransformNode::Filter {
                condition: condition.clone(),
            },
            extra,
        )
    });
    let feed = |b: &mut Builder, segment: &mut Segment, target: usize| match (head, core) {
        (Some(h), _) if h != target => b.edge(h, target),
        (Some(_), Some(j)) | (None, Some(j)) => b.edge(j, target),
        (_, None) => {
            if let Some(m) = main {
                segment
                    .entries
                    .entry(m.to_string())
                    .or_default()
                    .push(target);
            }
        }
    };
    if let Some(h) = head {
        feed(b, segment, h);
    }
    let mut exits = Vec::new();
    for entry in &output.entries {
        let expr = entry.expression.clone().unwrap_or_default();
        let mut extra = flags.clone();
        extra.insert(
            "column_refs".into(),
            b.column_refs(&c.unique_name, &expr, qualifiers, None),
        );
        let node = TransformNode::Calculate {
            output: format!("{}.{}", output.name, entry.name),
            expr,
        };
        let index = b.push(c, node, extra);
        feed(b, segment, index);
        exits.push(index);
    }
    if exits.is_empty() {
        exits.extend(head);
    }
    exits
}

/// Row links between data components become edges; each tMap join learns its inputs.
fn wire(b: &mut Builder, segments: &HashMap<&str, Segment>) {
    let mut join_inputs: HashMap<usize, (Option<usize>, Option<usize>)> = HashMap::new();
    let links: Vec<_> = b
        .job
        .links
        .iter()
        .filter(|l| l.kind != LinkKind::Iterate)
        .cloned()
        .collect();
    for link in &links {
        let (Some(from), Some(to)) = (
            segments.get(link.from.as_str()),
            segments.get(link.to.as_str()),
        ) else {
            continue;
        };
        let exits = link
            .metaname
            .as_ref()
            .and_then(|m| from.exits.get(m))
            .or_else(|| from.exits.get("*"))
            .cloned()
            .unwrap_or_default();
        let entries = link
            .label
            .as_ref()
            .and_then(|l| to.entries.get(l))
            .or_else(|| to.entries.get("*"))
            .cloned()
            .unwrap_or_default();
        for &e in &exits {
            for &t in &entries {
                b.edge(e, t);
            }
        }
        record_join_inputs(
            to,
            link.label.as_deref(),
            exits.last().copied(),
            &mut join_inputs,
        );
    }
    for segment in segments.values() {
        for (join, left_label, _) in &segment.joins {
            if left_label.is_none() {
                // A chained join's left input is the previous join, which fed it directly.
                let prev = b.edges.iter().find(|(f, t)| {
                    t.0 as usize == *join
                        && matches!(b.nodes[f.0 as usize], TransformNode::Join { .. })
                });
                if let Some((f, _)) = prev {
                    join_inputs.entry(*join).or_default().0 = Some(f.0 as usize);
                }
            }
        }
    }
    for (join, (left, right)) in join_inputs {
        if let TransformNode::Join {
            left: l, right: r, ..
        } = &mut b.nodes[join]
        {
            *l = NodeId(left.unwrap_or(join) as u32);
            *r = NodeId(right.unwrap_or(join) as u32);
        }
    }
}

fn record_join_inputs(
    to: &Segment,
    label: Option<&str>,
    upstream: Option<usize>,
    join_inputs: &mut HashMap<usize, (Option<usize>, Option<usize>)>,
) {
    let (Some(label), Some(upstream)) = (label, upstream) else {
        return;
    };
    for (join, left_label, right_label) in &to.joins {
        let slot = join_inputs.entry(*join).or_default();
        if left_label.as_deref() == Some(label) {
            slot.0 = Some(upstream);
        }
        if right_label == label {
            slot.1 = Some(upstream);
        }
    }
}

/// `tHashOutput` → `tHashInput` inside one job: an in-memory hand-over.
fn link_hash_tables(b: &mut Builder, data: &[&Component]) {
    let index_of = |b: &Builder, name: &str| {
        b.extra
            .iter()
            .position(|e| e.get("component").and_then(Value::as_str) == Some(name))
    };
    let outputs: Vec<(usize, String)> = data
        .iter()
        .filter(|c| matches!(c.component_type.as_str(), "tHashOutput" | "tBufferOutput"))
        .filter_map(|c| Some((index_of(b, &c.unique_name)?, c.unique_name.clone())))
        .collect();
    // The tHashOutput a tHashInput names (`LINK_WITH` + `LIST`), else every one in the job.
    let inputs: Vec<(usize, Option<String>)> = data
        .iter()
        .filter(|c| matches!(c.component_type.as_str(), "tHashInput" | "tBufferInput"))
        .filter_map(|c| {
            let linked = param_text(c, "LINK_WITH") == Some("true");
            let list = linked
                .then(|| param_text(c, "LIST").map(str::to_string))
                .flatten();
            Some((index_of(b, &c.unique_name)?, list))
        })
        .collect();
    for &(o, ref name) in &outputs {
        for &(i, ref list) in &inputs {
            if list.as_ref().is_none_or(|l| l == name) {
                b.edge(o, i);
            }
        }
    }
}
