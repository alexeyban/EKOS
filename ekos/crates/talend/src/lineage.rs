//! Column lineage inside one job (RFC 0264 §"Phase 2").
//!
//! [`job_lineage`] walks back from every column of every target (a database output, a file
//! output, a `tBufferOutput`) to where its value came from, one component at a time, and
//! returns the steps, the origins, the gaps and a formula over the origins. Pure: a database
//! input's SQL is read through a [`QueryReader`] the caller supplies (`ekos-recovery` parses it
//! with sqlparser), so this crate keeps no SQL parser.
//!
//! What a component does to a column is decided only by the rules below; a component without one
//! is a [`Gap`] and the walk stops there. Nothing is guessed: a column that is not in the
//! upstream schema, a tMap reference whose input has no link, a tJavaRow that never assigns the
//! column — each is a gap with its reason.

use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::catalogue::Category;
use crate::java::{qualified_refs, replace_qualified};
use crate::model::{Component, Job, JobletOrigin, Link, LinkKind, Mapper, Param};

/// Longest walk back from one target column, in components.
pub const MAX_DEPTH: usize = 64;
/// Longest formula kept; a longer one is cut and marked `formula_truncated`.
pub const MAX_FORMULA: usize = 400;

/// Reads a database input's SQL into its select list.
pub trait QueryReader {
    /// `None` when the SQL is not one SELECT this reader understands.
    fn read(&self, sql: &str) -> Option<QueryShape>;
}

/// A reader that understands nothing: every database input falls back to column names.
pub struct NoQueries;

impl QueryReader for NoQueries {
    fn read(&self, _: &str) -> Option<QueryShape> {
        None
    }
}

/// A SELECT's projection, item by item, and its WHERE.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryShape {
    pub columns: Vec<SelectColumn>,
    pub filter: Option<String>,
}

/// One select item.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectColumn {
    /// The item as written, normalised by the parser.
    pub expression: String,
    /// Every column it names: `(table, column)`, the table unqualified and `None` when the item
    /// does not say and the query reads more than one table.
    pub sources: Vec<(Option<String>, String)>,
    /// The item is one column and nothing else.
    pub plain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Table,
    File,
    /// `tBufferOutput`: rows handed to the job that called this one.
    Buffer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub kind: TargetKind,
    /// The folded table or file name.
    pub name: String,
    pub schema: Option<String>,
    pub column: String,
    /// The column's place in the target's schema, for a file or a buffer: what another job reads
    /// back is bound by position, not by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
}

/// Where a value enters the job.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    Table {
        table: String,
        schema: Option<String>,
        column: String,
        component: String,
        /// The column was matched by name, not through the parsed query.
        by_name: bool,
    },
    File {
        /// The folded file name; for a file a `tFileList` iterates, its directory and mask
        /// (`/in/ICOM_*_VENTE.csv`).
        file: String,
        column: String,
        component: String,
        /// The `tFileList` whose current file this is.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        iterated_by: Option<String>,
        /// The column's place in the input's schema (a delimited file is read by position).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<usize>,
    },
    /// A row a called job handed back through its `tBufferOutput` (`tRunJob` with an output).
    Job {
        /// The called job, as the `tRunJob` names it.
        job: String,
        column: String,
        component: String,
        /// The column's place in the `tRunJob` schema, which binds to the buffer's by position.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<usize>,
    },
    Generated {
        column: String,
        component: String,
        expression: Option<String>,
    },
}

impl Origin {
    /// How the origin reads in a formula.
    pub fn label(&self) -> String {
        match self {
            Self::Table { table, column, .. } => format!("{table}.{column}"),
            Self::File { file, column, .. } => {
                let base = file.rsplit(['/', '\\']).next().unwrap_or(file);
                format!("{base}.{column}")
            }
            Self::Job { job, column, .. } => format!("{job}.{column}"),
            Self::Generated {
                expression: Some(e),
                ..
            } => e.clone(),
            Self::Generated {
                component, column, ..
            } => format!("{component}.{column}"),
        }
    }
}

/// One component's part in a column's lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub component: String,
    pub component_type: String,
    /// The tMap output, for a tMap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub column: String,
    /// `write`, `map`, `aggregate`, `java_row`, `pass`, `read`, `generate`.
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// The columns this step reads, as `component[.output].column`.
    pub inputs: Vec<String>,
    /// Filters, joins and groupings that decide which rows reach this step.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joblet: Option<JobletOrigin>,
}

/// Where a walk stopped without reaching an origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    pub component: String,
    pub component_type: String,
    pub column: String,
    pub reason: String,
}

/// The lineage of one target column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnLineage {
    pub target: Target,
    /// The target component's unique name.
    pub component: String,
    pub data_action: Option<String>,
    /// A key column an `UPDATE` / `DELETE` matches rows on, rather than a value it writes.
    pub update_key: bool,
    pub formula: String,
    pub formula_truncated: bool,
    pub steps: Vec<Step>,
    pub origins: Vec<Origin>,
    pub gaps: Vec<Gap>,
}

/// A column on a component's output: `output` is the tMap output name, empty elsewhere.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Port {
    component: String,
    output: String,
    column: String,
}

impl Port {
    fn display(&self) -> String {
        if self.output.is_empty() {
            format!("{}.{}", self.component, self.column)
        } else {
            format!("{}.{}.{}", self.component, self.output, self.column)
        }
    }
}

/// What one port is made of.
#[derive(Debug, Clone, Default)]
struct Derived {
    step: Option<Step>,
    inputs: Vec<Port>,
    origins: Vec<Origin>,
    gap: Option<Gap>,
    /// How the port reads in a formula, with `{{n}}` standing for `inputs[n]`.
    template: Template,
}

#[derive(Debug, Clone, Default)]
enum Template {
    /// The single (or first) input, unchanged.
    #[default]
    Pass,
    /// Every input, joined by ` | ` (a union).
    Union,
    /// An expression whose references are replaced by the inputs, keyed `input.column`.
    Expression {
        text: String,
        qualifiers: Vec<String>,
        refs: BTreeMap<(String, String), usize>,
    },
    /// `function(input)`.
    Call(String),
    /// Fixed text (an origin, a gap).
    Text(String),
}

/// Pass-through components: a column keeps its name and value.
const PASS_THROUGH: &[&str] = &[
    "tFilterRow",
    "tUniqRow",
    "tSortRow",
    "tReplicate",
    "tUnite",
    "tConvertType",
    "tLogRow",
    "tFlowMeter",
    "tHashOutput",
    "tBufferOutput",
    "tSampleRow",
    "tFilterColumns",
    // Validates rows; its main output passes them on unchanged.
    "tSchemaComplianceCheck",
    // Logs a message; rows pass through.
    "tWarn",
    "joblet_input",
    "joblet_output",
];

/// The lineage of every target column of a job (expand joblets first: `Project::expand`).
pub fn job_lineage(job: &Job, queries: &dyn QueryReader) -> Vec<ColumnLineage> {
    let mut w = Walker::new(job, queries);
    let mut out = Vec::new();
    for c in &job.components {
        let Some(kind) = target_kind(c) else {
            continue;
        };
        for column in flow_columns(c) {
            out.push(w.lineage(c, kind, column));
        }
    }
    out
}

fn target_kind(c: &Component) -> Option<TargetKind> {
    if c.logical.starts_with("db_output") || matches!(c.logical.as_str(), "db_bulk_exec" | "db_scd")
    {
        return Some(TargetKind::Table);
    }
    if c.component_type == "tBufferOutput" {
        return Some(TargetKind::Buffer);
    }
    (c.category == Category::Output && c.component_type.starts_with("tFileOutput"))
        .then_some(TargetKind::File)
}

/// A component's main output columns: the `FLOW` schema, else its first.
fn flow_columns(c: &Component) -> Vec<&crate::model::Column> {
    c.schemas
        .iter()
        .find(|s| s.connector == "FLOW")
        .or_else(|| c.schemas.first())
        .map(|s| s.columns.iter().collect())
        .unwrap_or_default()
}

fn param_text<'a>(c: &'a Component, name: &str) -> Option<&'a str> {
    c.params
        .get(name)
        .and_then(Param::text)
        .filter(|v| !v.is_empty())
}

fn rows<'a>(c: &'a Component, name: &str) -> &'a [BTreeMap<String, String>] {
    c.params.get(name).map(Param::rows).unwrap_or_default()
}

struct Walker<'a> {
    job: &'a Job,
    queries: &'a dyn QueryReader,
    /// Row links into each component (iterate links excluded).
    inbound: HashMap<&'a str, Vec<&'a Link>>,
    memo: HashMap<Port, Derived>,
    rendered: HashMap<Port, String>,
}

impl<'a> Walker<'a> {
    fn new(job: &'a Job, queries: &'a dyn QueryReader) -> Self {
        let mut inbound: HashMap<&str, Vec<&Link>> = HashMap::new();
        for l in job.links.iter().filter(|l| l.kind != LinkKind::Iterate) {
            inbound.entry(l.to.as_str()).or_default().push(l);
        }
        Self {
            job,
            queries,
            inbound,
            memo: HashMap::new(),
            rendered: HashMap::new(),
        }
    }

    fn lineage(
        &mut self,
        c: &Component,
        kind: TargetKind,
        column: &crate::model::Column,
    ) -> ColumnLineage {
        let target = Target {
            kind,
            name: self.folded_name(
                c,
                if kind == TargetKind::File {
                    "FILENAME"
                } else {
                    "TABLE"
                },
            ),
            schema: self.db_schema(c),
            column: column
                .db_name
                .clone()
                .unwrap_or_else(|| column.name.clone()),
            position: (kind != TargetKind::Table)
                .then(|| position_of(c, &column.name))
                .flatten(),
        };
        let data_action = param_text(c, "DATA_ACTION").map(str::to_string);
        let update_key = column.key
            && data_action
                .as_deref()
                .is_some_and(|a| a.contains("UPDATE") || a.contains("DELETE"));
        let inputs = self.pass_inputs(c, &column.name);
        let mut steps = vec![Step {
            component: c.unique_name.clone(),
            component_type: c.component_type.clone(),
            output: None,
            column: target.column.clone(),
            kind: "write".into(),
            expression: None,
            inputs: inputs.iter().map(Port::display).collect(),
            conditions: Vec::new(),
            joblet: c.joblet.clone(),
        }];
        let (mut origins, mut gaps) = (Vec::new(), Vec::new());
        let mut seen = HashSet::new();
        for p in &inputs {
            self.collect(p, 0, &mut seen, &mut steps, &mut origins, &mut gaps);
        }
        if inputs.is_empty() {
            gaps.push(gap(c, &column.name, "no row link into the target"));
        }
        origins.sort();
        origins.dedup();
        let formula = match inputs.first() {
            Some(p) => {
                let mut visiting = HashSet::new();
                self.render(p, &mut visiting)
            }
            None => format!("?({}.{})", c.unique_name, column.name),
        };
        let formula_truncated = formula.chars().count() > MAX_FORMULA;
        let formula = if formula_truncated {
            let cut: String = formula.chars().take(MAX_FORMULA).collect();
            format!("{cut}…")
        } else {
            formula
        };
        ColumnLineage {
            target,
            component: c.unique_name.clone(),
            data_action,
            update_key,
            formula,
            formula_truncated,
            steps,
            origins,
            gaps,
        }
    }

    fn collect(
        &mut self,
        port: &Port,
        depth: usize,
        seen: &mut HashSet<Port>,
        steps: &mut Vec<Step>,
        origins: &mut Vec<Origin>,
        gaps: &mut Vec<Gap>,
    ) {
        if !seen.insert(port.clone()) {
            return;
        }
        if depth >= MAX_DEPTH {
            gaps.push(Gap {
                component: port.component.clone(),
                component_type: String::new(),
                column: port.column.clone(),
                reason: format!("walk deeper than {MAX_DEPTH} steps"),
            });
            return;
        }
        let d = self.derive(port);
        steps.extend(d.step);
        origins.extend(d.origins);
        gaps.extend(d.gap);
        for p in &d.inputs {
            self.collect(p, depth + 1, seen, steps, origins, gaps);
        }
    }

    /// The formula of a port, its inputs substituted. A port already being rendered (a cycle)
    /// reads as `…`.
    fn render(&mut self, port: &Port, visiting: &mut HashSet<Port>) -> String {
        if let Some(r) = self.rendered.get(port) {
            return r.clone();
        }
        if visiting.len() >= MAX_DEPTH || !visiting.insert(port.clone()) {
            return "…".into();
        }
        let d = self.derive(port);
        let mut inputs: Vec<String> = Vec::new();
        for p in &d.inputs {
            inputs.push(self.render(p, visiting));
        }
        visiting.remove(port);
        let r = match &d.template {
            Template::Pass => inputs
                .first()
                .cloned()
                .unwrap_or_else(|| format!("?({})", port.display())),
            Template::Union => inputs.join(" | "),
            Template::Call(f) => format!("{f}({})", inputs.first().map_or("?", |i| strip_outer(i))),
            Template::Text(t) => t.clone(),
            Template::Expression {
                text,
                qualifiers,
                refs,
            } => {
                let q: Vec<&str> = qualifiers.iter().map(String::as_str).collect();
                let whole = refs.len() == 1 && qualified_refs(text, &q).len() == 1 && {
                    let (k, _) = refs.iter().next().expect("one ref");
                    text.trim() == format!("{}.{}", k.0, k.1)
                };
                let replaced = replace_qualified(text, &q, &mut |qual, member| {
                    let i = *refs.get(&(qual.to_string(), member.to_string()))?;
                    let s = inputs.get(i)?;
                    Some(if whole || is_atomic(s) {
                        s.clone()
                    } else {
                        format!("({s})")
                    })
                });
                strip_outer(&replaced).to_string()
            }
        };
        self.rendered.insert(port.clone(), r.clone());
        r
    }

    fn derive(&mut self, port: &Port) -> Derived {
        if let Some(d) = self.memo.get(port) {
            return d.clone();
        }
        let d = match self.job.component(&port.component) {
            Some(c) => self.derive_on(c, port),
            None => Derived {
                gap: Some(Gap {
                    component: port.component.clone(),
                    component_type: String::new(),
                    column: port.column.clone(),
                    reason: "no such component".into(),
                }),
                template: Template::Text(format!("?({})", port.display())),
                ..Derived::default()
            },
        };
        self.memo.insert(port.clone(), d.clone());
        d
    }

    fn derive_on(&self, c: &Component, port: &Port) -> Derived {
        let col = port.column.as_str();
        if let Some(m) = &c.mapper {
            return self.tmap(c, m, &port.output, col);
        }
        match c.logical.as_str() {
            "db_input" => self.db_input(c, col),
            "tAggregateRow" | "tAggregateSortedRow" => self.aggregate(c, col),
            "tJavaRow" => self.java_row(c, col),
            "tRowGenerator" | "tFixedFlowInput" => generated(c, col),
            "tHashInput" | "tBufferInput" => self.hash_input(c, col),
            _ if c.component_type == "tRunJob" => run_job(c, col),
            _ if c.component_type == "tJoin" => self.join(c, col),
            l if PASS_THROUGH.contains(&l) => self.pass(c, col),
            _ if c.category == Category::Input && c.component_type.starts_with("tFileInput") => {
                self.file_input(c, col)
            }
            _ if target_kind(c).is_some() => self.pass(c, col),
            _ => gapped(
                c,
                col,
                &format!(
                    "{} ({}) has no column rule",
                    c.component_type,
                    c.category.as_str()
                ),
            ),
        }
    }

    /// The upstream port a row link brings `column` from.
    fn upstream(&self, link: &Link, column: &str) -> Port {
        let output = match self
            .job
            .component(&link.from)
            .and_then(|c| c.mapper.as_ref())
        {
            Some(m) => link
                .metaname
                .clone()
                .or_else(|| (m.outputs.len() == 1).then(|| m.outputs[0].name.clone()))
                .unwrap_or_default(),
            None => String::new(),
        };
        Port {
            component: link.from.clone(),
            output,
            column: column.to_string(),
        }
    }

    /// `column` from every row link into `c`.
    fn pass_inputs(&self, c: &Component, column: &str) -> Vec<Port> {
        self.inbound
            .get(c.unique_name.as_str())
            .map(|links| links.iter().map(|l| self.upstream(l, column)).collect())
            .unwrap_or_default()
    }

    fn pass(&self, c: &Component, col: &str) -> Derived {
        let inputs = self.pass_inputs(c, col);
        if inputs.is_empty() {
            return gapped(c, col, "no row link in");
        }
        let missing: Vec<String> = inputs
            .iter()
            .filter(|p| self.has_column(p) == Some(false))
            .map(Port::display)
            .collect();
        if !missing.is_empty() {
            return gapped(
                c,
                col,
                &format!("column not in the upstream schema: {}", missing.join(", ")),
            );
        }
        let mut conditions = Vec::new();
        if c.component_type == "tFilterRow" {
            conditions.extend(filter_condition(c).map(|f| format!("filter: {f}")));
        }
        if c.component_type == "tUniqRow" {
            let keys = unique_keys(c);
            if !keys.is_empty() {
                conditions.push(format!("first row per ({})", keys.join(", ")));
            }
        }
        let template = if inputs.len() > 1 {
            Template::Union
        } else {
            Template::Pass
        };
        Derived {
            step: Some(step(c, None, col, "pass", None, &inputs, conditions)),
            inputs,
            template,
            ..Derived::default()
        }
    }

    /// `tJoin`: a column comes from the lookup when `LOOKUP_COLS` maps it there, else from the
    /// main row under its own name. An inner join decides which rows reach every column; a left
    /// join only the columns read from the lookup.
    fn join(&self, c: &Component, col: &str) -> Derived {
        let links = self
            .inbound
            .get(c.unique_name.as_str())
            .cloned()
            .unwrap_or_default();
        let main = links.iter().find(|l| l.kind != LinkKind::Lookup);
        let lookup = links.iter().find(|l| l.kind == LinkKind::Lookup);
        let flag = |name: &str| param_text(c, name) == Some("true");
        let from_lookup = if flag("USE_LOOKUP_COLS") {
            rows(c, "LOOKUP_COLS")
                .iter()
                .find(|r| r.get("OUTPUT_COLUMN").map(String::as_str) == Some(col))
                .and_then(|r| r.get("LOOKUP_COLUMN").cloned())
        } else {
            None
        };
        let port = match (&from_lookup, main, lookup) {
            (Some(column), _, Some(l)) => self.upstream(l, column),
            (None, Some(m), _) => self.upstream(m, col),
            _ => return gapped(c, col, "no row link brings this column in"),
        };
        let inner = flag("USE_INNER_JOIN");
        let mut conditions = Vec::new();
        if inner || from_lookup.is_some() {
            let on: Vec<String> = rows(c, "JOIN_KEY")
                .iter()
                .filter_map(|r| {
                    Some(format!(
                        "{} = {}",
                        r.get("INPUT_COLUMN")?,
                        r.get("LOOKUP_COLUMN")?
                    ))
                })
                .collect();
            let with = lookup
                .and_then(|l| l.label.clone())
                .unwrap_or_else(|| "lookup".into());
            conditions.push(format!(
                "{} join {with} on {}",
                if inner { "inner" } else { "left" },
                on.join(" and ")
            ));
        }
        let inputs = vec![port];
        Derived {
            step: Some(step(c, None, col, "pass", None, &inputs, conditions)),
            inputs,
            ..Derived::default()
        }
    }

    /// Whether the port's component declares the column; `None` when it declares no schema.
    fn has_column(&self, port: &Port) -> Option<bool> {
        let c = self.job.component(&port.component)?;
        if let Some(m) = &c.mapper {
            let out = m.outputs.iter().find(|o| o.name == port.output)?;
            return Some(out.entries.iter().any(|e| e.name == port.column));
        }
        let cols = flow_columns(c);
        (!cols.is_empty()).then(|| cols.iter().any(|x| x.name == port.column))
    }

    fn hash_input(&self, c: &Component, col: &str) -> Derived {
        let named = (param_text(c, "LINK_WITH") == Some("true"))
            .then(|| param_text(c, "LIST"))
            .flatten();
        let sources: Vec<&Component> = self
            .job
            .components
            .iter()
            .filter(|o| matches!(o.component_type.as_str(), "tHashOutput" | "tBufferOutput"))
            .filter(|o| named.is_none_or(|n| o.unique_name == n))
            .filter(|o| {
                o.component_type.trim_end_matches("Output")
                    == c.component_type.trim_end_matches("Input")
            })
            .collect();
        if sources.is_empty() {
            return gapped(c, col, "no tHashOutput in the job feeds it");
        }
        let inputs: Vec<Port> = sources
            .iter()
            .map(|o| Port {
                component: o.unique_name.clone(),
                output: String::new(),
                column: col.to_string(),
            })
            .collect();
        let template = if inputs.len() > 1 {
            Template::Union
        } else {
            Template::Pass
        };
        Derived {
            step: Some(step(c, None, col, "pass", None, &inputs, Vec::new())),
            inputs,
            template,
            ..Derived::default()
        }
    }

    fn tmap(&self, c: &Component, m: &Mapper, output: &str, col: &str) -> Derived {
        let out = m
            .outputs
            .iter()
            .find(|o| o.name == output)
            .or_else(|| (m.outputs.len() == 1).then(|| &m.outputs[0]));
        let Some(out) = out else {
            return gapped(c, col, &format!("tMap has no output {output}"));
        };
        let Some(entry) = out.entries.iter().find(|e| e.name == col) else {
            return gapped(
                c,
                col,
                &format!("tMap output {} has no column {col}", out.name),
            );
        };
        let written = entry.expression.clone().unwrap_or_default();
        let text = inline_vars(&written, m);
        let qualifiers: Vec<String> = m.inputs.iter().map(|i| i.name.clone()).collect();
        let q: Vec<&str> = qualifiers.iter().map(String::as_str).collect();
        let mut inputs = Vec::new();
        let mut refs = BTreeMap::new();
        let mut unresolved = Vec::new();
        for (input, column) in qualified_refs(&text, &q) {
            match self.link_into(c, &input) {
                Some(l) => {
                    refs.insert((input, column.clone()), inputs.len());
                    inputs.push(self.upstream(l, &column));
                }
                None => unresolved.push(format!("{input}.{column}")),
            }
        }
        let used: HashSet<&str> = refs.keys().map(|(i, _)| i.as_str()).collect();
        let mut conditions = join_conditions(m, &used);
        conditions.extend(out.filter.iter().map(|f| format!("filter: {f}")));
        let mut d = Derived {
            step: Some(step(
                c,
                Some(&out.name),
                col,
                "map",
                Some(written.trim()),
                &inputs,
                conditions,
            )),
            inputs,
            template: Template::Expression {
                text,
                qualifiers,
                refs,
            },
            ..Derived::default()
        };
        if !unresolved.is_empty() {
            d.gap = Some(gap(
                c,
                col,
                &format!("no link carries {}", unresolved.join(", ")),
            ));
        }
        d
    }

    /// The row link into `c` whose label is `label`.
    fn link_into(&self, c: &Component, label: &str) -> Option<&'a Link> {
        self.inbound
            .get(c.unique_name.as_str())?
            .iter()
            .copied()
            .find(|l| l.label.as_deref() == Some(label))
    }

    fn main_input(&self, c: &Component) -> Option<&'a Link> {
        let links = self.inbound.get(c.unique_name.as_str())?;
        links
            .iter()
            .copied()
            .find(|l| l.kind == LinkKind::Main)
            .or_else(|| links.first().copied())
    }

    fn aggregate(&self, c: &Component, col: &str) -> Derived {
        let Some(link) = self.main_input(c) else {
            return gapped(c, col, "no row link in");
        };
        let group_by: Vec<String> = rows(c, "GROUPBYS")
            .iter()
            .filter_map(|r| r.get("INPUT_COLUMN").cloned())
            .collect();
        let conditions = if group_by.is_empty() {
            Vec::new()
        } else {
            vec![format!("grouped by {}", group_by.join(", "))]
        };
        let op = rows(c, "OPERATIONS")
            .iter()
            .find(|r| r.get("OUTPUT_COLUMN").map(String::as_str) == Some(col));
        if let Some(op) = op {
            let func = op.get("FUNCTION").cloned().unwrap_or_default();
            let arg = op.get("INPUT_COLUMN").cloned().unwrap_or_default();
            let inputs = vec![self.upstream(link, &arg)];
            let expr = format!("{func}({arg})");
            return Derived {
                step: Some(step(
                    c,
                    None,
                    col,
                    "aggregate",
                    Some(&expr),
                    &inputs,
                    conditions,
                )),
                inputs,
                template: Template::Call(func),
                ..Derived::default()
            };
        }
        let group = rows(c, "GROUPBYS").iter().find(|r| {
            r.get("OUTPUT_COLUMN")
                .or_else(|| r.get("INPUT_COLUMN"))
                .map(String::as_str)
                == Some(col)
        });
        let Some(group) = group else {
            return gapped(c, col, "neither an operation nor a group-by column");
        };
        let arg = group.get("INPUT_COLUMN").map_or(col, String::as_str);
        let inputs = vec![self.upstream(link, arg)];
        Derived {
            step: Some(step(c, None, col, "pass", None, &inputs, conditions)),
            inputs,
            ..Derived::default()
        }
    }

    fn java_row(&self, c: &Component, col: &str) -> Derived {
        let code = param_text(c, "CODE").unwrap_or_default();
        let Some(expr) = java_assignment(code, col) else {
            return gapped(c, col, &format!("CODE assigns no output_row.{col}"));
        };
        let Some(link) = self.main_input(c) else {
            return gapped(c, col, "no row link in");
        };
        let mut inputs = Vec::new();
        let mut refs = BTreeMap::new();
        for (q, column) in qualified_refs(&expr, &["input_row"]) {
            refs.insert((q, column.clone()), inputs.len());
            inputs.push(self.upstream(link, &column));
        }
        Derived {
            step: Some(step(
                c,
                None,
                col,
                "java_row",
                Some(&expr),
                &inputs,
                Vec::new(),
            )),
            inputs,
            template: Template::Expression {
                text: expr,
                qualifiers: vec!["input_row".into()],
                refs,
            },
            ..Derived::default()
        }
    }

    fn db_input(&self, c: &Component, col: &str) -> Derived {
        let table = self.folded_name(c, "TABLE");
        let schema = self.db_schema(c);
        let columns = flow_columns(c);
        let position = columns.iter().position(|x| x.name == col);
        let by_name_column = position
            .and_then(|i| columns[i].db_name.clone())
            .unwrap_or_else(|| col.to_string());
        let shape = param_text(c, "QUERY")
            .map(|q| self.job.fold(q))
            .filter(crate::java::Folded::complete)
            .and_then(|f| self.queries.read(&f.text))
            .filter(|s| s.columns.len() == columns.len());
        let origin = |t: &str, column: &str, by_name: bool| Origin::Table {
            table: t.to_string(),
            schema: schema.clone(),
            column: column.to_string(),
            component: c.unique_name.clone(),
            by_name,
        };
        let (origins, expression, conditions) = match (&shape, position) {
            (Some(s), Some(i)) => {
                let item = &s.columns[i];
                let origins: Vec<Origin> = item
                    .sources
                    .iter()
                    .map(|(t, column)| origin(t.as_deref().unwrap_or(&table), column, false))
                    .collect();
                let expression = (!item.plain).then(|| item.expression.clone());
                let conditions = s.filter.iter().map(|f| format!("where {f}")).collect();
                (origins, expression, conditions)
            }
            _ => (
                vec![origin(&table, &by_name_column, true)],
                None,
                Vec::new(),
            ),
        };
        let text = match (&expression, origins.as_slice()) {
            (None, [o]) => o.label(),
            (Some(e), _) => e.clone(),
            _ => format!("{table}.{by_name_column}"),
        };
        let mut s = step(c, None, col, "read", expression.as_deref(), &[], conditions);
        s.inputs = origins.iter().map(Origin::label).collect();
        Derived {
            step: Some(s),
            origins,
            template: Template::Text(text),
            ..Derived::default()
        }
    }

    fn file_input(&self, c: &Component, col: &str) -> Derived {
        let (file, iterated_by) = match self.iterating_list(c) {
            Some((list, pattern)) => (pattern, Some(list)),
            None => (self.folded_name(c, "FILENAME"), None),
        };
        let o = Origin::File {
            file,
            column: col.to_string(),
            component: c.unique_name.clone(),
            iterated_by,
            position: position_of(c, col),
        };
        let mut s = step(c, None, col, "read", None, &[], Vec::new());
        s.inputs = vec![o.label()];
        Derived {
            step: Some(s),
            template: Template::Text(o.label()),
            origins: vec![o],
            ..Derived::default()
        }
    }

    /// A file name read from `globalMap.get("<tFileList>_CURRENT_FILE…")`: the list's name and
    /// its folded directory joined to each mask (`dir/a*.csv|dir/b*.csv`). The file itself is
    /// only known at run time.
    fn iterating_list(&self, c: &Component) -> Option<(String, String)> {
        let expr = param_text(c, "FILENAME")?;
        let at = expr.find("_CURRENT_FILE")?;
        let start = expr[..at].rfind('"')? + 1;
        let list = self.job.component(&expr[start..at])?;
        let dir = self.folded_name(list, "DIRECTORY");
        let dir = dir.trim_end_matches(['/', '\\']);
        let masks: Vec<String> = rows(list, "FILES")
            .iter()
            .filter_map(|r| r.get("FILEMASK"))
            .map(|m| self.job.fold(m).text)
            .map(|m| {
                if dir.is_empty() {
                    m
                } else {
                    format!("{dir}/{m}")
                }
            })
            .collect();
        let pattern = if masks.is_empty() {
            format!("{dir}/*")
        } else {
            masks.join("|")
        };
        Some((list.unique_name.clone(), pattern))
    }

    /// A table or file name parameter, folded over the job's context (holes kept as `{{…}}`).
    fn folded_name(&self, c: &Component, param: &str) -> String {
        param_text(c, param)
            .map(|e| self.job.fold(e).text)
            .unwrap_or_default()
    }

    /// The schema a database component works in: its own `SCHEMA_DB`, else its connection's.
    fn db_schema(&self, c: &Component) -> Option<String> {
        let own = param_text(c, "SCHEMA_DB");
        let via = param_text(c, "CONNECTION")
            .and_then(|n| self.job.component(n))
            .and_then(|conn| param_text(conn, "SCHEMA_DB"));
        let f = self.job.fold(own.or(via)?);
        (f.complete() && !f.text.is_empty()).then_some(f.text)
    }
}

fn step(
    c: &Component,
    output: Option<&str>,
    column: &str,
    kind: &str,
    expression: Option<&str>,
    inputs: &[Port],
    conditions: Vec<String>,
) -> Step {
    Step {
        component: c.unique_name.clone(),
        component_type: c.component_type.clone(),
        output: output.map(str::to_string),
        column: column.to_string(),
        kind: kind.to_string(),
        expression: expression.filter(|e| !e.is_empty()).map(str::to_string),
        inputs: inputs.iter().map(Port::display).collect(),
        conditions,
        joblet: c.joblet.clone(),
    }
}

fn gap(c: &Component, column: &str, reason: &str) -> Gap {
    Gap {
        component: c.unique_name.clone(),
        component_type: c.component_type.clone(),
        column: column.to_string(),
        reason: reason.to_string(),
    }
}

/// A column's index in a component's main schema.
fn position_of(c: &Component, column: &str) -> Option<usize> {
    flow_columns(c).iter().position(|k| k.name == column)
}

/// A `tRunJob` output column: the called job's `tBufferOutput` column at the same position,
/// linked across jobs by the caller of [`job_lineage`] (the walk stays inside one job).
fn run_job(c: &Component, col: &str) -> Derived {
    let Some(job) = param_text(c, "PROCESS") else {
        return gapped(c, col, "tRunJob names no job");
    };
    let Some(position) = position_of(c, col) else {
        return gapped(c, col, "column not in the tRunJob schema");
    };
    let o = Origin::Job {
        job: job.to_string(),
        column: col.to_string(),
        component: c.unique_name.clone(),
        position: Some(position),
    };
    let mut s = step(c, None, col, "read", None, &[], Vec::new());
    s.inputs = vec![o.label()];
    Derived {
        step: Some(s),
        template: Template::Text(o.label()),
        origins: vec![o],
        ..Derived::default()
    }
}

fn gapped(c: &Component, column: &str, reason: &str) -> Derived {
    Derived {
        gap: Some(gap(c, column, reason)),
        template: Template::Text(format!("?({}.{column})", c.unique_name)),
        ..Derived::default()
    }
}

fn generated(c: &Component, col: &str) -> Derived {
    let expression = ["VALUES", "ARRAY"]
        .iter()
        .flat_map(|p| rows(c, p))
        .find(|r| r.get("SCHEMA_COLUMN").map(String::as_str) == Some(col))
        .and_then(|r| r.get("VALUE").or_else(|| r.get("ARRAY")))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let o = Origin::Generated {
        column: col.to_string(),
        component: c.unique_name.clone(),
        expression: expression.clone(),
    };
    let mut s = step(
        c,
        None,
        col,
        "generate",
        expression.as_deref(),
        &[],
        Vec::new(),
    );
    s.inputs = Vec::new();
    Derived {
        step: Some(s),
        template: Template::Text(o.label()),
        origins: vec![o],
        ..Derived::default()
    }
}

/// `Var.x` replaced by the variable's expression, repeatedly (a variable may use another).
fn inline_vars(expr: &str, m: &Mapper) -> String {
    let mut text = expr.to_string();
    for _ in 0..8 {
        if qualified_refs(&text, &["Var"]).is_empty() {
            break;
        }
        text = replace_qualified(&text, &["Var"], &mut |_, name| {
            let v = m.vars.iter().find(|v| v.name == name)?;
            let e = v.expression.as_deref().unwrap_or_default().trim();
            Some(if is_atomic(e) {
                e.to_string()
            } else {
                format!("({e})")
            })
        });
    }
    text
}

/// The lookups a tMap output depends on: every inner join (it drops rows for all columns), and
/// any other lookup the expression reads.
fn join_conditions(m: &Mapper, used: &HashSet<&str>) -> Vec<String> {
    m.inputs
        .iter()
        .skip(1)
        .filter(|l| l.inner_join || used.contains(l.name.as_str()))
        .map(|l| {
            let keys: Vec<String> = l
                .entries
                .iter()
                .filter_map(|e| {
                    Some(format!(
                        "{}.{} = {}",
                        l.name,
                        e.name,
                        e.expression.as_deref()?.trim()
                    ))
                })
                .collect();
            let kind = if l.inner_join {
                "inner join"
            } else {
                "left join"
            };
            let mut text = format!("{kind} {}", l.name);
            if !keys.is_empty() {
                text.push_str(&format!(" on {}", keys.join(" and ")));
            }
            if let Some(f) = &l.filter {
                text.push_str(&format!(" where {f}"));
            }
            text
        })
        .collect()
}

fn filter_condition(c: &Component) -> Option<String> {
    if param_text(c, "USE_ADVANCED") == Some("true") {
        return param_text(c, "ADVANCED_COND").map(|s| s.trim().to_string());
    }
    let op = param_text(c, "LOGICAL_OP").unwrap_or("&&");
    let parts: Vec<String> = rows(c, "CONDITIONS")
        .iter()
        .map(|r| {
            let get = |k: &str| r.get(k).map(String::as_str).unwrap_or_default();
            format!(
                "{} {} {}",
                get("INPUT_COLUMN"),
                get("OPERATOR"),
                get("RVALUE")
            )
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(&format!(" {op} ")))
}

fn unique_keys(c: &Component) -> Vec<String> {
    rows(c, "UNIQUE_KEY")
        .iter()
        .filter(|r| r.get("KEY_ATTRIBUTE").map(String::as_str) == Some("true"))
        .filter_map(|r| r.get("SCHEMA_COLUMN").cloned())
        .collect()
}

/// The expression of `output_row.<column> = …;` in a tJavaRow's code (the last one wins).
fn java_assignment(code: &str, column: &str) -> Option<String> {
    let target = format!("output_row.{column}");
    let mut found = None;
    for statement in code.split(';') {
        let s = statement
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let s = s.trim();
        let Some(rest) = s.strip_prefix(&target) else {
            continue;
        };
        let rest = rest.trim_start();
        if let Some(value) = rest.strip_prefix('=')
            && !value.starts_with('=')
        {
            found = Some(value.trim().to_string());
        }
    }
    found
}

/// `s` trimmed, without one pair of parentheses around the whole of it.
fn strip_outer(s: &str) -> &str {
    let t = s.trim();
    if t.starts_with('(') && t.ends_with(')') && balanced_call(&format!("f{t}")) {
        t[1..t.len() - 1].trim()
    } else {
        t
    }
}

/// A formula part that needs no parentheses: one name, a call, or a literal.
fn is_atomic(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return true;
    }
    let plain = s
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '"' | '\'' | '{' | '}'));
    plain || (s.ends_with(')') && balanced_call(s))
}

/// `name(…)` whose outer parentheses close at the end.
fn balanced_call(s: &str) -> bool {
    let Some(open) = s.find('(') else {
        return false;
    };
    if !s[..open]
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        return false;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices().skip(open) {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return i == s.len() - 1;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectFile, read_project};

    fn node(kind: &str, name: &str, extra: &str) -> String {
        format!(
            r#"<node componentName="{kind}"><elementParameter field="TEXT" name="UNIQUE_NAME" value="{name}"/>{extra}</node>"#
        )
    }

    fn meta(cols: &[&str]) -> String {
        let cols: String = cols
            .iter()
            .map(|c| format!(r#"<column name="{c}" type="id_String"/>"#))
            .collect();
        format!(r#"<metadata connector="FLOW" name="x">{cols}</metadata>"#)
    }

    fn link(label: &str, from: &str, to: &str, extra: &str) -> String {
        format!(
            r#"<connection connectorName="FLOW" label="{label}" source="{from}" target="{to}" {extra}/>"#
        )
    }

    fn table(name: &str, rows: &[&[(&str, &str)]]) -> String {
        let values: String = rows
            .iter()
            .flat_map(|r| *r)
            .map(|(k, v)| format!(r#"<elementValue elementRef="{k}" value="{v}"/>"#))
            .collect();
        format!(r#"<elementParameter field="TABLE" name="{name}">{values}</elementParameter>"#)
    }

    /// Synthetic, in the shape of fixture B's fact job: a sales read with a lookup, a tMap
    /// computing an amount, an in-memory hand-over, an aggregate, a second tMap, a table write.
    fn job() -> crate::Job {
        let xml = format!(
            r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xmlns:TalendMapper="http://www.talend.org/mapper" defaultContext="Default">
<context name="Default"><contextParameter name="ods" type="id_String" value="ODS"/></context>
{sales}{product}{tmap1}{hout}{hin}{agg}{tmap2}{out}
{l1}{l2}{l3}{l4}{l5}{l6}
</talendfile:ProcessType>"#,
            sales = node(
                "tPostgresqlInput",
                "tDBInput_1",
                &format!(
                    r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;SALES&quot;"/><elementParameter field="MEMO_SQL" name="QUERY" value="&quot;SELECT ID, CD, QTY, PRICE FROM &quot;+context.ods+&quot;.SALES&quot;"/>{}"#,
                    meta(&["ID", "CD", "QTY", "PRICE"])
                )
            ),
            product = node(
                "tPostgresqlInput",
                "tDBInput_2",
                &format!(
                    r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;DIM_PRODUCT&quot;"/>{}"#,
                    meta(&["ID_DIM", "CD"])
                )
            ),
            tmap1 = node(
                "tMap",
                "tMap_1",
                r#"<nodeData xsi:type="TalendMapper:MapperData"><varTables name="Var"><mapperTableEntries name="amount" expression="row1.QTY * row1.PRICE"/></varTables><outputTables name="toHash"><mapperTableEntries name="ID_DIM" expression="row2.ID_DIM"/><mapperTableEntries name="AMOUNT" expression="Var.amount"/></outputTables><inputTables name="row1"/><inputTables name="row2" innerJoin="true"><mapperTableEntries name="CD" expression="row1.CD"/></inputTables></nodeData>"#
            ),
            hout = node("tHashOutput", "tHashOutput_1", &meta(&["ID_DIM", "AMOUNT"])),
            hin = node(
                "tHashInput",
                "tHashInput_1",
                &format!(
                    r#"<elementParameter field="CHECK" name="LINK_WITH" value="true"/><elementParameter field="COMPONENT_LIST" name="LIST" value="tHashOutput_1"/>{}"#,
                    meta(&["ID_DIM", "AMOUNT"])
                )
            ),
            agg = node(
                "tAggregateRow",
                "tAggregateRow_1",
                &format!(
                    "{}{}{}",
                    table(
                        "GROUPBYS",
                        &[&[("OUTPUT_COLUMN", "ID_DIM"), ("INPUT_COLUMN", "ID_DIM")]]
                    ),
                    table(
                        "OPERATIONS",
                        &[
                            &[
                                ("OUTPUT_COLUMN", "TOTAL"),
                                ("FUNCTION", "sum"),
                                ("INPUT_COLUMN", "AMOUNT")
                            ],
                            &[
                                ("OUTPUT_COLUMN", "N"),
                                ("FUNCTION", "count"),
                                ("INPUT_COLUMN", "ID_DIM")
                            ]
                        ]
                    ),
                    meta(&["ID_DIM", "TOTAL", "N"])
                )
            ),
            tmap2 = node(
                "tMap",
                "tMap_2",
                r#"<nodeData xsi:type="TalendMapper:MapperData"><outputTables name="ins"><mapperTableEntries name="ID_DIM" expression="row6.ID_DIM"/><mapperTableEntries name="TOTAL" expression="row6.TOTAL"/><mapperTableEntries name="NB" expression="row6.N"/><mapperTableEntries name="LOADED" expression="TalendDate.getCurrentDate()"/></outputTables><inputTables name="row6"/></nodeData>"#
            ),
            out = node(
                "tPostgresqlOutput",
                "tDBOutput_1",
                &format!(
                    r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;FACT&quot;"/><elementParameter field="CLOSED_LIST" name="DATA_ACTION" value="INSERT"/>{}"#,
                    meta(&["ID_DIM", "TOTAL", "NB", "LOADED"])
                )
            ),
            l1 = link("row1", "tDBInput_1", "tMap_1", ""),
            l2 = link("row2", "tDBInput_2", "tMap_1", r#"lineStyle="8""#),
            l3 = link("toHash", "tMap_1", "tHashOutput_1", r#"metaname="toHash""#),
            l4 = link("row5", "tHashInput_1", "tAggregateRow_1", ""),
            l5 = link("row6", "tAggregateRow_1", "tMap_2", ""),
            l6 = link("ins", "tMap_2", "tDBOutput_1", r#"metaname="ins""#),
        );
        let p = read_project(
            "",
            &[ProjectFile {
                path: "process/jFact_0.1.item".into(),
                text: xml,
            }],
        );
        assert!(
            p.diagnostics
                .iter()
                .all(|d| d.code == "TALEND003" || d.code == "TALEND004")
        );
        p.jobs[0].clone()
    }

    /// A reader for the one query in [`job`]: `SELECT ID, CD, QTY, PRICE FROM ODS.SALES`.
    struct Plain;
    impl QueryReader for Plain {
        fn read(&self, sql: &str) -> Option<QueryShape> {
            let list = sql.strip_prefix("SELECT ")?.split(" FROM ").next()?;
            Some(QueryShape {
                columns: list
                    .split(", ")
                    .map(|c| SelectColumn {
                        expression: c.into(),
                        sources: vec![(Some("SALES".into()), c.into())],
                        plain: true,
                    })
                    .collect(),
                filter: None,
            })
        }
    }

    fn by_column<'a>(all: &'a [ColumnLineage], column: &str) -> &'a ColumnLineage {
        all.iter().find(|l| l.target.column == column).unwrap()
    }

    #[test]
    fn a_measure_reads_back_through_map_hash_and_aggregate_to_the_source_columns() {
        let all = job_lineage(&job(), &Plain);
        assert_eq!(all.len(), 4, "one per target column");
        let total = by_column(&all, "TOTAL");
        assert_eq!(total.target.name, "FACT");
        assert_eq!(total.formula, "sum(SALES.QTY * SALES.PRICE)");
        assert!(total.gaps.is_empty(), "{:?}", total.gaps);
        let tables: Vec<String> = total.origins.iter().map(Origin::label).collect();
        assert_eq!(tables, ["SALES.PRICE", "SALES.QTY"]);
        let kinds: Vec<&str> = total.steps.iter().map(|s| s.kind.as_str()).collect();
        assert_eq!(
            kinds,
            [
                "write",
                "map",
                "aggregate",
                "pass",
                "pass",
                "map",
                "read",
                "read"
            ]
        );
        let map = total
            .steps
            .iter()
            .find(|s| s.component == "tMap_1")
            .unwrap();
        assert_eq!(map.expression.as_deref(), Some("Var.amount"));
        assert!(
            map.conditions
                .iter()
                .any(|c| c == "inner join row2 on row2.CD = row1.CD"),
            "{:?}",
            map.conditions
        );
        let agg = total.steps.iter().find(|s| s.kind == "aggregate").unwrap();
        assert_eq!(agg.conditions, ["grouped by ID_DIM"]);
    }

    #[test]
    fn a_count_is_shown_as_a_count_of_what_it_counts() {
        // Fixture B's NB_QTE_VENTE shape: the "quantity" counts dimension keys.
        let all = job_lineage(&job(), &Plain);
        assert_eq!(by_column(&all, "NB").formula, "count(DIM_PRODUCT.ID_DIM)");
    }

    #[test]
    fn without_a_query_reader_columns_match_by_name_and_say_so() {
        let all = job_lineage(&job(), &NoQueries);
        let total = by_column(&all, "TOTAL");
        assert!(
            total
                .origins
                .iter()
                .all(|o| matches!(o, Origin::Table { by_name: true, .. }))
        );
        assert_eq!(total.formula, "sum(SALES.QTY * SALES.PRICE)");
    }

    #[test]
    fn a_value_computed_in_the_map_has_no_origin_and_no_gap() {
        let all = job_lineage(&job(), &Plain);
        let loaded = by_column(&all, "LOADED");
        assert!(loaded.origins.is_empty());
        assert!(loaded.gaps.is_empty());
        assert_eq!(loaded.formula, "TalendDate.getCurrentDate()");
    }

    #[test]
    fn a_component_without_a_rule_is_a_gap_not_a_guess() {
        let mut j = job();
        let agg = j
            .components
            .iter_mut()
            .find(|c| c.unique_name == "tAggregateRow_1")
            .unwrap();
        agg.component_type = "tSomethingNew".into();
        agg.logical = "tSomethingNew".into();
        agg.category = Category::Unknown;
        let all = job_lineage(&j, &Plain);
        let total = by_column(&all, "TOTAL");
        assert!(total.origins.is_empty());
        assert_eq!(total.gaps.len(), 1);
        assert!(total.gaps[0].reason.contains("no column rule"));
        assert_eq!(total.formula, "?(tAggregateRow_1.TOTAL)");
    }

    #[test]
    fn a_file_a_file_list_iterates_is_named_by_its_directory_and_mask() {
        let xml = format!(
            r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default"><context name="Default"><contextParameter name="dir" type="id_String" value="/data/in/"/></context>{}{}{}{}</talendfile:ProcessType>"#,
            node(
                "tFileList",
                "tFileList_1",
                &format!(
                    r#"<elementParameter field="DIRECTORY" name="DIRECTORY" value="context.dir"/>{}"#,
                    table("FILES", &[&[("FILEMASK", "&quot;SALES_*.csv&quot;")]])
                )
            ),
            node(
                "tFileInputDelimited",
                "tFileInputDelimited_1",
                &format!(
                    r#"<elementParameter field="FILE" name="FILENAME" value="((String)globalMap.get(&quot;tFileList_1_CURRENT_FILEPATH&quot;))"/>{}"#,
                    meta(&["QTY"])
                )
            ),
            node(
                "tPostgresqlOutput",
                "tDBOutput_1",
                &format!(
                    r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;ODS&quot;"/>{}"#,
                    meta(&["QTY"])
                )
            ),
            link("row1", "tFileInputDelimited_1", "tDBOutput_1", ""),
        );
        let p = read_project(
            "",
            &[ProjectFile {
                path: "process/jOds_0.1.item".into(),
                text: xml,
            }],
        );
        let all = job_lineage(&p.jobs[0], &NoQueries);
        assert_eq!(all[0].formula, "SALES_*.csv.QTY");
        assert!(matches!(
            &all[0].origins[0],
            Origin::File { file, iterated_by: Some(l), .. }
                if file == "/data/in/SALES_*.csv" && l == "tFileList_1"
        ));
    }

    /// A job from `nodes` and `links`, read alone.
    fn one_job(path: &str, body: &str) -> crate::Job {
        let xml = format!(
            r#"<talendfile:ProcessType xmlns:talendfile="platform:/resource/org.talend.model/model/TalendFile.xsd" defaultContext="Default"><context name="Default"/>{body}</talendfile:ProcessType>"#
        );
        let p = read_project(
            "",
            &[ProjectFile {
                path: path.into(),
                text: xml,
            }],
        );
        p.jobs[0].clone()
    }

    #[test]
    fn a_buffer_and_a_file_target_record_positions_and_a_run_job_is_an_origin() {
        // Child: a table read handed back through `tBufferOutput` and written to a file.
        let child = one_job(
            "process/jChild_0.1.item",
            &format!(
                "{}{}{}{}{}",
                node(
                    "tPostgresqlInput",
                    "tDBInput_1",
                    &format!(
                        r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;SRC&quot;"/>{}"#,
                        meta(&["A", "B"])
                    )
                ),
                node("tBufferOutput", "tBufferOutput_1", &meta(&["A", "B"])),
                node(
                    "tFileOutputDelimited",
                    "tFileOutputDelimited_1",
                    &format!(
                        r#"<elementParameter field="FILE" name="FILENAME" value="&quot;/out/x.csv&quot;"/>{}"#,
                        meta(&["A", "B"])
                    )
                ),
                link("row1", "tDBInput_1", "tBufferOutput_1", ""),
                link("row2", "tBufferOutput_1", "tFileOutputDelimited_1", ""),
            ),
        );
        let lineages = job_lineage(&child, &NoQueries);
        let buffer: Vec<_> = lineages
            .iter()
            .filter(|l| l.target.kind == TargetKind::Buffer)
            .map(|l| (l.target.column.as_str(), l.target.position))
            .collect();
        assert_eq!(buffer, [("A", Some(0)), ("B", Some(1))]);
        let file = lineages
            .iter()
            .find(|l| l.target.kind == TargetKind::File && l.target.column == "B")
            .unwrap();
        assert_eq!(
            (file.target.name.as_str(), file.target.position),
            ("/out/x.csv", Some(1))
        );
        assert_eq!(file.formula, "SRC.B");

        // Parent: the called job's rows, under the parent's own column names, into a table.
        let parent = one_job(
            "process/jParent_0.1.item",
            &format!(
                "{}{}{}",
                node(
                    "tRunJob",
                    "tRunJob_1",
                    &format!(
                        r#"<elementParameter field="PROCESS_TYPE" name="PROCESS" value="jChild"/>{}"#,
                        meta(&["X", "Y"])
                    )
                ),
                node(
                    "tPostgresqlOutput",
                    "tDBOutput_1",
                    &format!(
                        r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;DST&quot;"/>{}"#,
                        meta(&["X", "Y"])
                    )
                ),
                link("row1", "tRunJob_1", "tDBOutput_1", ""),
            ),
        );
        let y = job_lineage(&parent, &NoQueries)
            .into_iter()
            .find(|l| l.target.column == "Y")
            .unwrap();
        assert!(y.gaps.is_empty(), "{:?}", y.gaps);
        assert_eq!(y.formula, "jChild.Y");
        assert_eq!(
            y.origins,
            [Origin::Job {
                job: "jChild".into(),
                column: "Y".into(),
                component: "tRunJob_1".into(),
                position: Some(1),
            }]
        );
    }

    #[test]
    fn a_join_takes_lookup_columns_from_the_lookup_and_says_how_it_joins() {
        let job = one_job(
            "process/jJoin_0.1.item",
            &format!(
                "{}{}{}{}{}{}{}",
                node(
                    "tPostgresqlInput",
                    "tDBInput_1",
                    &format!(
                        r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;ORDERS&quot;"/>{}"#,
                        meta(&["ID", "CUST"])
                    )
                ),
                node(
                    "tPostgresqlInput",
                    "tDBInput_2",
                    &format!(
                        r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;CUSTOMERS&quot;"/>{}"#,
                        meta(&["CUST_ID", "NAME"])
                    )
                ),
                node(
                    "tJoin",
                    "tJoin_1",
                    &format!(
                        r#"<elementParameter field="CHECK" name="USE_INNER_JOIN" value="true"/><elementParameter field="CHECK" name="USE_LOOKUP_COLS" value="true"/>{}{}{}"#,
                        table(
                            "JOIN_KEY",
                            &[&[("INPUT_COLUMN", "CUST"), ("LOOKUP_COLUMN", "CUST_ID")]]
                        ),
                        table(
                            "LOOKUP_COLS",
                            &[&[("OUTPUT_COLUMN", "CUSTOMER"), ("LOOKUP_COLUMN", "NAME")]]
                        ),
                        meta(&["ID", "CUSTOMER"])
                    )
                ),
                node(
                    "tPostgresqlOutput",
                    "tDBOutput_1",
                    &format!(
                        r#"<elementParameter field="DBTABLE" name="TABLE" value="&quot;DST&quot;"/>{}"#,
                        meta(&["ID", "CUSTOMER"])
                    )
                ),
                link("row1", "tDBInput_1", "tJoin_1", ""),
                link("row2", "tDBInput_2", "tJoin_1", r#"lineStyle="8""#),
                link("row3", "tJoin_1", "tDBOutput_1", ""),
            ),
        );
        let all = job_lineage(&job, &NoQueries);
        let get = |c: &str| all.iter().find(|l| l.target.column == c).unwrap();
        assert_eq!(get("CUSTOMER").formula, "CUSTOMERS.NAME");
        assert_eq!(get("ID").formula, "ORDERS.ID");
        for c in ["ID", "CUSTOMER"] {
            let l = get(c);
            assert!(l.gaps.is_empty(), "{:?}", l.gaps);
            assert!(
                l.steps
                    .iter()
                    .any(|s| s.conditions == ["inner join row2 on CUST = CUST_ID"]),
                "{:?}",
                l.steps
            );
        }
    }

    #[test]
    fn a_java_row_assignment_is_read() {
        assert_eq!(
            java_assignment(
                "// x\noutput_row.A = input_row.B.trim();\noutput_row.C = 1;",
                "A"
            )
            .as_deref(),
            Some("input_row.B.trim()")
        );
        assert_eq!(java_assignment("if (output_row.A == null) {}", "A"), None);
    }

    #[test]
    fn atomic_parts_are_not_parenthesised() {
        assert!(is_atomic("SALES.QTY"));
        assert!(is_atomic("sum(a * b)"));
        assert!(!is_atomic("a * b"));
        assert!(!is_atomic("f(a) + g(b)"));
    }
}
