//! The migration assessment (RFC 0265): findings that need a person, per-job and per-project
//! summaries, from a project's model and its column lineage. Pure and deterministic; no model call.
//!
//! A finding is a hypothesis, never a defect: the report says so, and a person confirms it.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use crate::catalogue::Category;
use crate::java::context_refs;
use crate::lineage::{ColumnLineage, Origin};
use crate::model::{Component, Job, LinkKind, Param, Project};
use crate::resolve::RuntimeContext;

/// One job as the analyzer saw it: the item, the job with its joblets inlined, its lineage, and
/// what the IR made of it.
pub struct JobView<'a> {
    pub job: &'a Job,
    pub expanded: &'a Job,
    pub lineage: &'a [ColumnLineage],
    /// Data components the IR lowered to something other than `Unmapped`.
    pub mapped: usize,
    pub data_components: usize,
    pub reads: &'a [String],
    pub writes: &'a [String],
    pub files_read: &'a [String],
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Evidence {
    pub path: String,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub rule: String,
    /// `probable_defect`, `copy_paste`, `environment`, `dead_item`.
    pub class: String,
    /// What the finding is about: a job, `job.component`, `job.table.column`, … — part of its key.
    pub subject: String,
    /// The job it belongs to, when it belongs to one.
    pub job: Option<String>,
    pub message: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobAssessment {
    pub data_components: usize,
    /// Lowered to the IR; a stand-in for Slice 3's fidelity classes.
    pub mapped: usize,
    /// Expressions by kind: column, arithmetic, routine, java_method, ternary, runtime, literal.
    pub expressions: BTreeMap<String, usize>,
    /// Row-level writes, write-backs, file moves, sequences, commits.
    pub state: Vec<String>,
    pub runtime: Runtime,
    pub idioms: Vec<Idiom>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Runtime {
    /// Context variables the job reads whose value can change at run time.
    pub overridable_context: Vec<String>,
    pub global_map: Vec<String>,
    pub clock: bool,
    pub iterates_files: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Idiom {
    /// `scd2_by_lookup`, `reject_to_table`, `aggregate_then_insert`.
    pub name: String,
    pub components: Vec<String>,
    pub table: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloneCluster {
    pub jobs: Vec<String>,
    /// Component types the jobs share, in order, with how many.
    pub shared: Vec<(String, usize)>,
    /// Smallest edge similarity between two jobs of the cluster (1.0 = the same graph).
    pub similarity: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProjectAssessment {
    pub clone_clusters: Vec<CloneCluster>,
    /// Jobs in waves: a job comes after every job writing a table it reads.
    pub migration_order: Vec<Vec<String>>,
    /// Jobs on a cycle of the table graph, left out of the waves.
    pub cycles: Vec<String>,
    /// Counts of what the translation would produce — a sketch, before any SQL is written.
    pub target_sketch: BTreeMap<String, usize>,
    pub superseded_versions: usize,
    pub findings_by_class: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Assessment {
    pub findings: Vec<Finding>,
    /// By `job@version`.
    pub jobs: BTreeMap<String, JobAssessment>,
    pub project: ProjectAssessment,
}

/// Two jobs whose row-link and trigger edges (between component types) overlap at least this
/// much are clones.
pub const CLONE_SIMILARITY: f64 = 0.7;

/// Talend's own routines: a call to one is a `routine` expression.
const SYSTEM_ROUTINES: &[&str] = &[
    "TalendDate",
    "TalendString",
    "StringHandling",
    "Numeric",
    "Mathematical",
    "Relational",
    "DataOperation",
    "TalendDataGenerator",
];

/// Assess the latest version of every job.
pub fn assess(project: &Project, views: &[JobView<'_>]) -> Assessment {
    let latest: Vec<&JobView> = views.iter().filter(|v| v.job.latest).collect();
    let mut a = Assessment::default();
    let subjects = job_subjects(&latest);
    for v in &latest {
        measure_ignores_named_column(v, &mut a.findings);
        leap_year_by_four(v, &mut a.findings);
        foreign_lookup_name(v, &subjects, &mut a.findings);
        absolute_path(v, &mut a.findings);
        a.jobs.insert(job_key(v.job), job_assessment(v));
    }
    repeated_message(project, &latest, &subjects, &mut a.findings);
    uncalled_job(project, &latest, &mut a.findings);
    a.findings
        .sort_by(|x, y| (&x.rule, &x.subject).cmp(&(&y.rule, &y.subject)));
    a.findings
        .dedup_by(|x, y| x.rule == y.rule && x.subject == y.subject);
    a.project = project_assessment(project, &latest, &a);
    a
}

pub fn job_key(job: &Job) -> String {
    format!("{}@{}", job.name, job.version)
}

fn evidence(job: &Job, c: Option<&Component>) -> Evidence {
    Evidence {
        path: job.path.clone(),
        line: c.map(|c| c.line),
    }
}

fn finding(
    rule: &str,
    class: &str,
    subject: String,
    job: &Job,
    message: String,
    ev: Vec<Evidence>,
) -> Finding {
    Finding {
        rule: rule.into(),
        class: class.into(),
        subject,
        job: Some(job.name.clone()),
        message,
        evidence: ev,
    }
}

/// Upper-case, `_`-separated words.
fn words(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_uppercase)
        .collect()
}

/// `needle`'s words appear in `hay`'s, in order and next to each other.
fn contains_words(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// A measure (an aggregated target column) named after a column the job reads, whose formula
/// never reads it (`NB_QTE_VENTE = count(ID_DIM_PRODUIT)` while the job reads `QTE_VENTE`).
fn measure_ignores_named_column(v: &JobView, out: &mut Vec<Finding>) {
    let mut read: BTreeSet<String> = BTreeSet::new();
    for l in v.lineage {
        for o in &l.origins {
            if let Origin::Table { column, .. } | Origin::File { column, .. } = o {
                read.insert(column.to_ascii_uppercase());
            }
        }
    }
    // Multi-word names only: a single word (`CLIENT`) inside a column name is not evidence.
    let read: Vec<(String, Vec<String>)> = read
        .into_iter()
        .filter(|c| c.contains('_') && c.len() >= 6)
        .map(|c| {
            let w = words(&c);
            (c, w)
        })
        .collect();
    for l in v.lineage.iter().filter(|l| is_aggregate(&l.formula)) {
        let target = l.target.column.to_ascii_uppercase();
        let tw = words(&target);
        let formula = l.formula.to_ascii_uppercase();
        let named: Vec<&String> = read
            .iter()
            .filter(|(c, w)| {
                *c != target && contains_words(&tw, w) && !formula.contains(c.as_str())
            })
            .map(|(c, _)| c)
            .collect();
        let Some(column) = named.first() else {
            continue;
        };
        let c = v.job.component(&l.component);
        out.push(finding(
            "measure_ignores_named_column",
            "probable_defect",
            format!("{}.{}.{}", v.job.name, l.target.name, l.target.column),
            v.job,
            format!(
                "`{}.{}` is named after `{column}`, which the job reads, but its formula is `{}`: \
                 check that the column computes what its name says",
                l.target.name, l.target.column, l.formula
            ),
            vec![evidence(v.job, c)],
        ));
    }
}

/// A formula that is one aggregate: a measure (`count(…)`, `sum(…)`, …).
fn is_aggregate(formula: &str) -> bool {
    let f = formula.trim().to_ascii_lowercase();
    ["count(", "sum(", "avg(", "min(", "max("]
        .iter()
        .any(|a| f.starts_with(a))
}

/// `year / 4 == floor(year / 4)` or `year % 4 == 0` as a leap year, without the 100 / 400 rule.
fn leap_year_by_four(v: &JobView, out: &mut Vec<Finding>) {
    for c in &v.expanded.components {
        for text in c.texts() {
            let t = text.to_ascii_lowercase();
            let year = t.contains("year") || t.contains("annee") || t.contains("année");
            let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            let by_four = [
                "/4)", "/4=", "/4<", "/4>", "%4)", "%4=", "%4<", "%4>", "%4!", "/4!",
            ]
            .iter()
            .any(|p| compact.contains(p))
                || compact.ends_with("%4")
                || compact.ends_with("/4");
            if year && by_four && !compact.contains("100") && !compact.contains("400") {
                out.push(finding(
                    "leap_year_by_four",
                    "probable_defect",
                    format!("{}.{}", v.job.name, c.unique_name),
                    v.job,
                    format!(
                        "`{}` tests a year's divisibility by 4 only; the Gregorian rule also \
                         excludes centuries not divisible by 400 (1900, 2100)",
                        c.unique_name
                    ),
                    vec![evidence(v.job, v.job.component(&c.unique_name).or(Some(c)))],
                ));
                break;
            }
        }
    }
}

/// Each job's subject: its name without the leading `j` and a folder-like prefix (`jOdsVente`
/// in `ODS` → `vente`), lower case, letters and digits only. Five letters at least.
fn job_subjects(latest: &[&JobView]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for v in latest {
        let mut name = v.job.name.as_str();
        if let Some(rest) = name.strip_prefix('j')
            && rest.starts_with(|c: char| c.is_ascii_uppercase())
        {
            name = rest;
        }
        for part in v.job.folder.split('/').filter(|p| !p.is_empty()) {
            if name.len() > part.len() && name[..part.len()].eq_ignore_ascii_case(part) {
                name = &name[part.len()..];
            }
        }
        let s = norm(name);
        if s.len() >= 5 {
            out.insert(v.job.name.clone(), s);
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.to_ascii_lowercase()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// A row-link label that names another job's subject and nothing this job reads.
fn foreign_lookup_name(v: &JobView, subjects: &BTreeMap<String, String>, out: &mut Vec<Finding>) {
    let own = subjects.get(&v.job.name);
    let read: Vec<String> = v
        .reads
        .iter()
        .chain(v.files_read)
        .map(|t| norm(t))
        .collect();
    let mut seen = BTreeSet::new();
    for l in &v.job.links {
        let Some(label) = l.label.as_deref() else {
            continue;
        };
        let n = norm(label);
        let Some((other, _)) = subjects
            .iter()
            .find(|(job, s)| **s == n && **job != v.job.name)
        else {
            continue;
        };
        if own.is_some_and(|o| o.contains(&n)) || read.iter().any(|r| r.contains(&n)) {
            continue;
        }
        if !seen.insert(label.to_string()) {
            continue;
        }
        let c = v.job.component(&l.to);
        out.push(finding(
            "foreign_lookup_name",
            "copy_paste",
            format!("{}.{label}", v.job.name),
            v.job,
            format!(
                "row link `{label}` ({} → {}) is named after `{other}`'s subject, but this job reads \
                 no table or file of that name: probably copied from that job",
                l.from, l.to
            ),
            vec![evidence(v.job, c)],
        ));
    }
}

/// File and directory parameters that fold to an absolute path.
fn absolute_path(v: &JobView, out: &mut Vec<Finding>) {
    let mut seen = BTreeSet::new();
    for c in &v.job.components {
        for (name, p) in &c.params {
            let Param::Value { value, .. } = p else {
                continue;
            };
            let upper = name.to_ascii_uppercase();
            let is_path = [
                "FILENAME",
                "DIRECTORY",
                "FILEPATH",
                "FILE_PATH",
                "PATH",
                "FOLDER",
            ]
            .iter()
            .any(|s| upper.ends_with(s))
                || upper == "FILE";
            if !is_path || value.trim().is_empty() {
                continue;
            }
            let folded = v.job.fold(value).text;
            if !is_absolute(&folded) || !seen.insert(folded.clone()) {
                continue;
            }
            let lower = folded.to_ascii_lowercase().replace('\\', "/");
            let personal = ["/users/", "/home/", "/downloads", "/desktop", "/documents"]
                .iter()
                .any(|s| lower.contains(s));
            out.push(finding(
                "absolute_path",
                "environment",
                format!("{}.{}.{name}", v.job.name, c.unique_name),
                v.job,
                format!(
                    "`{}` reads or writes `{folded}`, an absolute path{}",
                    c.unique_name,
                    if personal {
                        " inside a person's own folders: it exists on one machine"
                    } else {
                        ": it ties the job to one machine's layout"
                    }
                ),
                vec![evidence(v.job, Some(c))],
            ));
        }
    }
}

fn is_absolute(p: &str) -> bool {
    let b = p.as_bytes();
    (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\'))
        || p.starts_with("\\\\")
        || (p.starts_with('/') && !p.starts_with("//") && p.len() > 1)
}

/// The same `tDie` / `tWarn` message literal in several jobs, naming one job's subject (or the
/// job itself) while a job that is not that job and does not call it carries it too. A message
/// shared without that is ordinary logging, not evidence of a copy.
fn repeated_message(
    project: &Project,
    latest: &[&JobView],
    subjects: &BTreeMap<String, String>,
    out: &mut Vec<Finding>,
) {
    let mut by_text: BTreeMap<String, Vec<(&Job, &Component)>> = BTreeMap::new();
    for v in latest {
        for c in &v.job.components {
            if !matches!(c.component_type.as_str(), "tDie" | "tWarn") {
                continue;
            }
            let Some(m) = c.params.get("MESSAGE").and_then(Param::text) else {
                continue;
            };
            let text = m.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.len() >= 12 {
                by_text.entry(text).or_default().push((v.job, c));
            }
        }
    }
    let calls: BTreeMap<&str, BTreeSet<String>> = latest
        .iter()
        .map(|v| {
            let called = project
                .calls(v.job)
                .into_iter()
                .map(|c| c.resolved.map_or(c.child_name, |r| r.name))
                .collect();
            (v.job.name.as_str(), called)
        })
        .collect();
    for (text, uses) in by_text {
        let jobs: BTreeSet<&str> = uses.iter().map(|(j, _)| j.name.as_str()).collect();
        if jobs.len() < 2 {
            continue;
        }
        let words: Vec<String> = text
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .map(norm)
            .collect();
        let named = latest.iter().map(|v| v.job.name.as_str()).find(|j| {
            words.contains(&norm(j)) || subjects.get(*j).is_some_and(|s| words.contains(s))
        });
        let Some(named) = named else {
            continue;
        };
        let strangers: Vec<&str> = jobs
            .iter()
            .copied()
            .filter(|j| *j != named && !calls.get(j).is_some_and(|c| c.contains(named)))
            .collect();
        if strangers.is_empty() {
            continue;
        }
        let shown: String = text.chars().take(120).collect();
        out.push(Finding {
            rule: "repeated_message".into(),
            class: "copy_paste".into(),
            subject: format!(
                "message:{}",
                norm(&text).chars().take(48).collect::<String>()
            ),
            job: Some(uses[0].0.name.clone()),
            message: format!(
                "the message `{shown}` is written in {} jobs ({}); it names `{named}`, yet {} of \
                 them ({}) are not that job and do not call it: probably copied",
                jobs.len(),
                jobs.iter().copied().collect::<Vec<_>>().join(", "),
                strangers.len(),
                strangers.join(", ")
            ),
            evidence: uses.iter().map(|(j, c)| evidence(j, Some(c))).collect(),
        });
    }
}

/// A latest job no job calls and that calls none.
fn uncalled_job(project: &Project, latest: &[&JobView], out: &mut Vec<Finding>) {
    let mut called: BTreeSet<String> = BTreeSet::new();
    let mut calls: BTreeSet<String> = BTreeSet::new();
    for v in latest {
        for call in project.calls(v.job) {
            calls.insert(v.job.name.clone());
            called.insert(
                call.resolved
                    .as_ref()
                    .map_or(call.child_name.clone(), |r| r.name.clone()),
            );
        }
    }
    for v in latest {
        if called.contains(&v.job.name) || calls.contains(&v.job.name) {
            continue;
        }
        out.push(Finding {
            rule: "uncalled_job".into(),
            class: "dead_item".into(),
            subject: v.job.name.clone(),
            job: Some(v.job.name.clone()),
            message: format!(
                "`{}` is not called by any job and calls none: run on its own, by a schedule \
                 outside the project, or not at all",
                v.job.name
            ),
            evidence: vec![evidence(v.job, None)],
        });
    }
}

fn job_assessment(v: &JobView) -> JobAssessment {
    let job = v.expanded;
    let mut expressions: BTreeMap<String, usize> = BTreeMap::new();
    let routines: Vec<&str> = SYSTEM_ROUTINES.to_vec();
    for c in &job.components {
        let mut exprs: Vec<&str> = Vec::new();
        if let Some(m) = &c.mapper {
            exprs.extend(
                m.outputs
                    .iter()
                    .flat_map(|o| &o.entries)
                    .chain(&m.vars)
                    .filter_map(|e| e.expression.as_deref()),
            );
        }
        if let Some(m) = &c.elt {
            exprs.extend(
                m.outputs
                    .iter()
                    .flat_map(|o| &o.entries)
                    .filter_map(|e| e.expression.as_deref()),
            );
        }
        for e in exprs {
            *expressions
                .entry(expression_kind(e, &routines).into())
                .or_default() += 1;
        }
    }
    let mut state = Vec::new();
    for c in &job.components {
        let action = c
            .params
            .get("DATA_ACTION")
            .and_then(Param::text)
            .unwrap_or_default();
        if c.category == Category::Output
            || c.logical == "elt_output"
            || c.logical.starts_with("db_output")
        {
            let table = c
                .params
                .get("TABLE")
                .or_else(|| c.params.get("ELT_TABLE_NAME"))
                .and_then(Param::text)
                .map(|t| job.fold(t).text)
                .unwrap_or_default();
            if action.contains("UPDATE") || action.contains("DELETE") {
                state.push(format!("{action} {table}"));
            }
        }
        match c.component_type.as_str() {
            "tFileDelete" => state.push(format!("deletes a file ({})", c.unique_name)),
            "tFileCopy" if c.params.get("REMOVE_FILE").and_then(Param::text) == Some("true") => {
                state.push(format!("moves a file ({})", c.unique_name));
            }
            t if t.ends_with("Commit") => state.push(format!("commits ({})", c.unique_name)),
            _ => {}
        }
    }
    for t in v.writes.iter().filter(|t| v.reads.contains(t)) {
        state.push(format!("writes back to {t}, which it also reads"));
    }
    let texts = job.texts();
    if texts.iter().any(|t| t.contains("Numeric.sequence")) {
        state.push("uses a sequence (Numeric.sequence)".into());
    }
    state.sort();
    state.dedup();
    let rc = RuntimeContext::of(job);
    let mut overridable: BTreeSet<String> = BTreeSet::new();
    let mut global_map: BTreeSet<String> = BTreeSet::new();
    for t in &texts {
        overridable.extend(context_refs(t).into_iter().filter(|n| rc.overridable(n)));
        let mut rest = *t;
        while let Some(at) = rest.find("globalMap.get(\"") {
            let after = &rest[at + "globalMap.get(\"".len()..];
            if let Some(end) = after.find('"') {
                global_map.insert(after[..end].to_string());
            }
            rest = after;
        }
    }
    let clock = texts.iter().any(|t| {
        [
            "getCurrentDate",
            "TalendDate.getDate(",
            "currentTimeMillis",
            "new Date()",
            "current_date",
            "current_timestamp",
            "sysdate",
        ]
        .iter()
        .any(|p| t.contains(p))
    });
    JobAssessment {
        data_components: v.data_components,
        mapped: v.mapped,
        expressions,
        state,
        runtime: Runtime {
            overridable_context: overridable.into_iter().collect(),
            global_map: global_map.into_iter().collect(),
            clock,
            iterates_files: job
                .components
                .iter()
                .any(|c| c.component_type == "tFileList"),
        },
        idioms: idioms(job),
    }
}

/// What one expression is, the first that applies: runtime, ternary, routine, java_method,
/// arithmetic, column, literal.
pub fn expression_kind(e: &str, routines: &[&str]) -> &'static str {
    let e = e.trim();
    let runtime = [
        "globalMap",
        "context.",
        "TalendDate.getCurrentDate",
        "jobName",
        "Numeric.sequence",
    ];
    if runtime.iter().any(|r| e.contains(r)) {
        return "runtime";
    }
    if e.contains('?') && e.contains(':') {
        return "ternary";
    }
    if routines.iter().any(|r| e.contains(&format!("{r}."))) {
        return "routine";
    }
    // Operators and calls are looked for outside string literals (`"N/A"` is a literal).
    let bare = without_strings(e);
    let e = bare.as_str();
    let bytes = e.as_bytes();
    let method_call = e.match_indices('.').any(|(i, _)| {
        let rest = &e[i + 1..];
        let name_len = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(rest.len());
        name_len > 0 && rest[name_len..].trim_start().starts_with('(')
    });
    if method_call {
        return "java_method";
    }
    let operator = bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| matches!(b, b'+' | b'*' | b'/' | b'%') || (b == b'-' && i > 0));
    if operator || e.contains('(') {
        return "arithmetic";
    }
    if e.contains('.') {
        return "column";
    }
    "literal"
}

/// `e` with the contents of its `"…"` and `'…'` literals removed (quotes kept).
fn without_strings(e: &str) -> String {
    let mut out = String::with_capacity(e.len());
    let (mut quote, mut escaped) = (None::<char>, false);
    for c in e.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                    out.push(c);
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

/// Idioms by structure only (RFC 0265): Slice 3 turns them into dbt constructs.
fn idioms(job: &Job) -> Vec<Idiom> {
    let mut out = Vec::new();
    let action = |c: &Component| {
        c.params
            .get("DATA_ACTION")
            .and_then(Param::text)
            .unwrap_or_default()
            .to_string()
    };
    let table_of = |c: &Component| {
        c.params
            .get("TABLE")
            .and_then(Param::text)
            .map(|t| job.fold(t).text)
    };
    for m in job.components.iter().filter(|c| c.mapper.is_some()) {
        let outs: Vec<&Component> = job
            .links
            .iter()
            .filter(|l| l.from == m.unique_name)
            .filter_map(|l| job.component(&l.to))
            .filter(|c| c.logical.starts_with("db_output"))
            .collect();
        let lookup_tables: BTreeSet<String> = job
            .links
            .iter()
            .filter(|l| l.to == m.unique_name && l.kind == LinkKind::Lookup)
            .filter_map(|l| job.component(&l.from))
            .filter(|c| c.logical == "db_input")
            .filter_map(table_of)
            .collect();
        let mut by_table: BTreeMap<String, Vec<(&Component, String)>> = BTreeMap::new();
        for o in &outs {
            if let Some(t) = table_of(o) {
                by_table.entry(t).or_default().push((o, action(o)));
            }
        }
        for (table, writers) in by_table {
            let inserts = writers.iter().filter(|(_, a)| a.contains("INSERT")).count();
            let updates = writers.iter().any(|(_, a)| a.contains("UPDATE"));
            if lookup_tables.contains(&table) && updates && inserts >= 1 && writers.len() >= 2 {
                let mut components = vec![m.unique_name.clone()];
                components.extend(writers.iter().map(|(c, _)| c.unique_name.clone()));
                out.push(Idiom {
                    name: "scd2_by_lookup".into(),
                    components,
                    table: Some(table),
                });
            }
        }
    }
    for l in &job.links {
        let to = job.component(&l.to);
        let reject_link = matches!(l.kind, LinkKind::Reject | LinkKind::Filter)
            || job
                .component(&l.from)
                .and_then(|c| c.mapper.as_ref())
                .is_some_and(|m| {
                    m.outputs.iter().any(|o| {
                        Some(o.name.as_str()) == l.metaname.as_deref()
                            && (o.reject || o.reject_inner_join)
                    })
                });
        if reject_link && let Some(t) = to.filter(|c| c.logical.starts_with("db_output")) {
            out.push(Idiom {
                name: "reject_to_table".into(),
                components: vec![l.from.clone(), t.unique_name.clone()],
                table: table_of(t),
            });
        }
        let from = job.component(&l.from);
        if from.is_some_and(|c| c.component_type.starts_with("tAggregate"))
            && let Some(t) =
                to.filter(|c| c.logical.starts_with("db_output") && action(c).contains("INSERT"))
        {
            out.push(Idiom {
                name: "aggregate_then_insert".into(),
                components: vec![l.from.clone(), t.unique_name.clone()],
                table: table_of(t),
            });
        }
    }
    out.sort_by(|a, b| (&a.name, &a.components).cmp(&(&b.name, &b.components)));
    out.dedup();
    out
}

/// Edges between component types: (from type, link kind, to type), counted.
type Shape = BTreeMap<(String, String, String), usize>;

fn shape(job: &Job) -> Shape {
    let ty = |n: &str| {
        job.component(n)
            .map_or_else(String::new, |c| c.component_type.clone())
    };
    let mut out: Shape = BTreeMap::new();
    for l in &job.links {
        let kind = match &l.kind {
            LinkKind::Other(o) => o.clone(),
            k => format!("{k:?}"),
        };
        *out.entry((ty(&l.from), kind, ty(&l.to))).or_default() += 1;
    }
    for t in &job.triggers {
        *out.entry((ty(&t.from), format!("{:?}", t.kind), ty(&t.to)))
            .or_default() += 1;
    }
    out
}

fn similarity(a: &Shape, b: &Shape) -> f64 {
    let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
    let (mut inter, mut union) = (0usize, 0usize);
    for k in keys {
        let (x, y) = (
            a.get(k).copied().unwrap_or(0),
            b.get(k).copied().unwrap_or(0),
        );
        inter += x.min(y);
        union += x.max(y);
    }
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// Clone clusters: single linkage over [`similarity`] ≥ [`CLONE_SIMILARITY`], jobs with at least
/// four edges only (a two-component job is like every other).
fn clone_clusters(latest: &[&JobView]) -> Vec<CloneCluster> {
    let shapes: Vec<(&Job, Shape)> = latest
        .iter()
        .map(|v| (v.job, shape(v.job)))
        .filter(|(_, s)| s.values().sum::<usize>() >= 4)
        .collect();
    let n = shapes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    let mut pair_sim: HashMap<(usize, usize), f64> = HashMap::new();
    for i in 0..n {
        for j in i + 1..n {
            let s = similarity(&shapes[i].1, &shapes[j].1);
            pair_sim.insert((i, j), s);
            if s >= CLONE_SIMILARITY {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut out: Vec<CloneCluster> = groups
        .into_values()
        .filter(|g| g.len() >= 2)
        .map(|g| {
            let mut min = 1.0f64;
            for (x, &i) in g.iter().enumerate() {
                for &j in &g[x + 1..] {
                    min = min.min(pair_sim[&(i.min(j), i.max(j))]);
                }
            }
            let mut shared: Option<BTreeMap<String, usize>> = None;
            for &i in &g {
                let mut types: BTreeMap<String, usize> = BTreeMap::new();
                for c in &shapes[i].0.components {
                    *types.entry(c.component_type.clone()).or_default() += 1;
                }
                shared = Some(match shared {
                    None => types,
                    Some(s) => s
                        .into_iter()
                        .filter_map(|(k, n)| types.get(&k).map(|m| (k, n.min(*m))))
                        .collect(),
                });
            }
            let mut jobs: Vec<String> = g.iter().map(|&i| shapes[i].0.name.clone()).collect();
            jobs.sort();
            CloneCluster {
                jobs,
                shared: shared.unwrap_or_default().into_iter().collect(),
                similarity: (min * 100.0).round() / 100.0,
            }
        })
        .collect();
    out.sort_by(|a, b| a.jobs.cmp(&b.jobs));
    out
}

fn project_assessment(project: &Project, latest: &[&JobView], a: &Assessment) -> ProjectAssessment {
    let clone_clusters = clone_clusters(latest);
    // Waves over the table graph: B after A when A writes a table B reads.
    let names: Vec<&str> = latest.iter().map(|v| v.job.name.as_str()).collect();
    let mut after: BTreeMap<&str, BTreeSet<&str>> =
        names.iter().map(|n| (*n, BTreeSet::new())).collect();
    for b in latest {
        for w in latest {
            if w.job.name != b.job.name && w.writes.iter().any(|t| b.reads.contains(t)) {
                after
                    .get_mut(b.job.name.as_str())
                    .unwrap()
                    .insert(w.job.name.as_str());
            }
        }
    }
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut waves: Vec<Vec<String>> = Vec::new();
    loop {
        let wave: Vec<&str> = after
            .iter()
            .filter(|(n, deps)| {
                !done.contains(*n) && deps.iter().all(|d| done.contains(d) || *d == **n)
            })
            .map(|(n, _)| *n)
            .collect();
        if wave.is_empty() {
            break;
        }
        done.extend(&wave);
        waves.push(wave.into_iter().map(str::to_string).collect());
    }
    let cycles: Vec<String> = names
        .iter()
        .filter(|n| !done.contains(*n))
        .map(ToString::to_string)
        .collect();
    let mut sketch: BTreeMap<String, usize> = BTreeMap::new();
    let written: BTreeSet<&String> = latest.iter().flat_map(|v| v.writes).collect();
    let files: BTreeSet<&String> = latest.iter().flat_map(|v| v.files_read).collect();
    let snapshots: BTreeSet<String> = a
        .jobs
        .values()
        .flat_map(|j| &j.idioms)
        .filter(|i| i.name == "scd2_by_lookup")
        .filter_map(|i| i.table.clone())
        .collect();
    sketch.insert("models".into(), written.len());
    sketch.insert("snapshots".into(), snapshots.len());
    sketch.insert("seeds_or_sources_from_files".into(), files.len());
    sketch.insert("macros_for_clone_clusters".into(), clone_clusters.len());
    let mut by_class: BTreeMap<String, usize> = BTreeMap::new();
    for f in &a.findings {
        *by_class.entry(f.class.clone()).or_default() += 1;
    }
    ProjectAssessment {
        clone_clusters,
        migration_order: waves,
        cycles,
        target_sketch: sketch,
        superseded_versions: project.jobs.iter().filter(|j| !j.latest).count(),
        findings_by_class: by_class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expressions_are_classed_by_the_first_kind_that_applies() {
        let r = SYSTEM_ROUTINES;
        assert_eq!(expression_kind("row1.ID", r), "column");
        assert_eq!(expression_kind("row1.QTY * row1.PRICE", r), "arithmetic");
        assert_eq!(expression_kind("TalendDate.getDate(\"Y\")", r), "routine");
        assert_eq!(expression_kind("row1.NAME.trim()", r), "java_method");
        assert_eq!(expression_kind("row1.A == null ? 0 : 1", r), "ternary");
        assert_eq!(expression_kind("context.schema + \"x\"", r), "runtime");
        assert_eq!(expression_kind("\"N/A\"", r), "literal");
        assert_eq!(expression_kind("42", r), "literal");
    }

    #[test]
    fn paths_are_absolute_on_either_system() {
        assert!(is_absolute("C:/Users/pc/Downloads/x.xlsx"));
        assert!(is_absolute("/data/in"));
        assert!(is_absolute("\\\\server\\share"));
        assert!(!is_absolute("in/x.csv"));
        assert!(!is_absolute("{{context.dir}}/x"));
    }

    #[test]
    fn names_match_by_whole_words() {
        assert!(contains_words(&words("NB_QTE_VENTE"), &words("QTE_VENTE")));
        assert!(!contains_words(
            &words("NB_QTE_VENTES"),
            &words("QTE_VENTE")
        ));
    }
}
