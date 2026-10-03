//! `DbtAnalyzerPass` — structural extraction of `Table` KIR objects from a dbt project's own
//! checked-in metadata (RFC 0117): model `.sql` files (Jinja `ref()`/`source()` macro calls) and
//! `schema.yml`/`sources.yml`-shaped YAML config. Deliberately never reads `manifest.json`/
//! `catalog.json` — both live under `dbt/target/`, a build artifact directory confirmed gitignored
//! on a real inspected project, not checked-in source of truth — and never connects to a live
//! warehouse: dbt itself can point at any database, so the only stable, version-controlled source
//! of truth is dbt's own project files.
//!
//! A `.sql` file under `models/**/` *is* a model regardless of whether any YAML documents it — YAML
//! only adds description/columns on top of a model that already exists. Source tables (`sources:`
//! YAML) have no backing `.sql` file at all — they're pre-existing tables dbt only references, so
//! their only existence signal is the YAML.
//!
//! Uses `ObjectKind::Table`, not a new `Custom(_)` kind, deliberately (RFC 0117): a dbt model is a
//! real table, and letting `DefaultResolver`'s real column-Jaccard structural scoring fuse it with
//! an independently-discovered DDL-based `Table` of the same name is desired, not the over-merge
//! risk `Custom(_)`'s blanket kind-exclusion list (`ekos_identity`) exists to prevent. Mirrors
//! `python_analyzer.rs`'s SQLAlchemy-ORM-to-`Table` precedent (RFC 0091): a distinct id namespace
//! (`"dbt-table:"`) keeps this analyzer's ids from ever colliding with a same-named DDL table's id,
//! while both stay mergeable by real identity resolution.
//!
//! Dependency edges use the built-in `RelationshipKind::DependsOn`, not the Transformation IR's
//! `Custom("FeedsInto")` — this is whole-table-to-whole-table dependency (the same relationship
//! kind `concentration_risks`, RFC 0094, already scans the whole graph for), not step-level lineage
//! within one transformation.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn dbt_table_kir_id(dbt_root: &str, name: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dbt-table:{dbt_root}:{}", name.to_lowercase()).as_bytes(),
    ))
}

/// `(from, to)` alone is a safe id input here, unlike `sql_analyzer.rs`'s FK ids — a dbt model
/// either depends on a target table or it doesn't; repeated `ref()`/`source()` calls to the same
/// target within one model (common — the same upstream table referenced from more than one CTE)
/// are one real dependency fact, not several, so they're deduplicated before this is ever called.
fn dbt_depends_on_kir_id(from: KirId, to: KirId) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("dbt-depends-on:{from}:{to}").as_bytes(),
    ))
}

fn model_name_from_path(rel_path: &str) -> String {
    std::path::Path::new(rel_path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel_path.to_string())
}

fn line_at(content: &str, byte_offset: usize) -> u32 {
    content[..byte_offset.min(content.len())]
        .matches('\n')
        .count() as u32
        + 1
}

/// Best-effort `{{ config(materialized='...') }}` extraction — omitted, never guessed, when absent.
fn extract_materialized(sql: &str) -> Option<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re =
        RE.get_or_init(|| regex::Regex::new(r#"materialized\s*=\s*['"]([a-zA-Z_]+)['"]"#).unwrap());
    re.captures(sql).map(|c| c[1].to_string())
}

/// One resolved `ref('model')` or `source('src', 'table')` macro call found in a model's raw SQL.
struct MacroRef {
    /// The name to resolve against `known` — the model name for `ref()`, the table name for
    /// `source()` (dbt addresses source tables by table name, not `source.table`).
    target_name: String,
    byte_offset: usize,
    fragment: String,
}

fn find_macro_refs(sql: &str) -> Vec<MacroRef> {
    static REF_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static SOURCE_RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let ref_re = REF_RE
        .get_or_init(|| regex::Regex::new(r#"ref\(\s*['"]([A-Za-z0-9_]+)['"]\s*\)"#).unwrap());
    let source_re = SOURCE_RE.get_or_init(|| {
        regex::Regex::new(
            r#"source\(\s*['"]([A-Za-z0-9_\-]+)['"]\s*,\s*['"]([A-Za-z0-9_\-]+)['"]\s*\)"#,
        )
        .unwrap()
    });

    let mut refs = Vec::new();
    for m in ref_re.find_iter(sql) {
        let caps = ref_re.captures(m.as_str()).unwrap();
        refs.push(MacroRef {
            target_name: caps[1].to_string(),
            byte_offset: m.start(),
            fragment: m.as_str().to_string(),
        });
    }
    for m in source_re.find_iter(sql) {
        let caps = source_re.captures(m.as_str()).unwrap();
        refs.push(MacroRef {
            target_name: caps[2].to_string(),
            byte_offset: m.start(),
            fragment: m.as_str().to_string(),
        });
    }
    refs
}

/// One `models[].columns[]` or `sources[].tables[]` entry's declared (best-effort, partial —
/// dbt's own `schema.yml` typically only documents tested/described columns, not every column a
/// model actually produces) metadata.
#[derive(Default)]
struct YamlModelDoc {
    columns: Vec<serde_json::Value>,
    description: Option<String>,
}

struct YamlSourceTable {
    name: String,
    source_name: String,
    description: Option<String>,
    columns: Vec<serde_json::Value>,
}

/// RFC 0170 Phase 3 — one documented column: its description, declared type and dbt tests
/// (`tests:` or dbt ≥ 1.8's `data_tests:`) as the same column facts the SQL analyzer records
/// (`not_null`, `unique`), plus `accepted_values` (a declared domain, as normalized literals) and
/// `references` (a `relationships` test: the column's lookup table). `path`/`line` locate the
/// column in its YAML file, for evidence.
fn column_json(
    c: &serde_yaml::Value,
    path: &str,
    text: &str,
    owner: &str,
) -> Option<serde_json::Value> {
    let name = c.get("name").and_then(|n| n.as_str())?;
    let mut col = serde_json::json!({ "name": name });
    if let Some(d) = c.get("description").and_then(|d| d.as_str()) {
        col["description"] = serde_json::json!(d);
        col["description_path"] = serde_json::json!(path);
    }
    if let Some(t) = c.get("data_type").and_then(|d| d.as_str()) {
        col["data_type"] = serde_json::json!(t);
    }
    // The first `- name: <col>` after the owner's own `name:` line.
    let owner_at = text
        .lines()
        .position(|l| l.trim_start().trim_start_matches("- ").trim() == format!("name: {owner}"))
        .unwrap_or(0);
    if let Some(i) = text
        .lines()
        .enumerate()
        .skip(owner_at)
        .find(|(_, l)| l.trim_start().trim_start_matches("- ").trim() == format!("name: {name}"))
        .map(|(i, _)| i)
    {
        col["description_line"] = serde_json::json!(i + 1);
        col["dbt_line"] = serde_json::json!(i + 1);
    }
    col["dbt_path"] = serde_json::json!(path);
    let tests = c
        .get("data_tests")
        .or_else(|| c.get("tests"))
        .and_then(|t| t.as_sequence())
        .cloned()
        .unwrap_or_default();
    let mut names = Vec::new();
    for t in &tests {
        match t {
            serde_yaml::Value::String(s) if s == "not_null" => {
                col["not_null"] = serde_json::json!(true);
                names.push("not_null".to_string());
            }
            serde_yaml::Value::String(s) if s == "unique" => {
                col["unique"] = serde_json::json!(true);
                names.push("unique".to_string());
            }
            serde_yaml::Value::Mapping(m) => {
                for (k, v) in m {
                    let k = k.as_str().unwrap_or_default();
                    // Arguments sit under `arguments:` in dbt ≥ 1.10, directly under the test before.
                    let args = v.get("arguments").unwrap_or(v);
                    match k {
                        "not_null" | "unique" => {
                            col[k] = serde_json::json!(true);
                            names.push(k.to_string());
                        }
                        "accepted_values" => {
                            let quote = args.get("quote").and_then(|q| q.as_bool()).unwrap_or(true);
                            let mut vals: Vec<String> = args
                                .get("values")
                                .and_then(|v| v.as_sequence())
                                .into_iter()
                                .flatten()
                                .filter_map(|v| match v {
                                    serde_yaml::Value::String(s) if quote => {
                                        Some(format!("'{}'", s.replace('\'', "''")))
                                    }
                                    serde_yaml::Value::String(s) => Some(s.clone()),
                                    serde_yaml::Value::Number(n) => Some(n.to_string()),
                                    serde_yaml::Value::Bool(b) => Some(b.to_string()),
                                    _ => None,
                                })
                                .collect();
                            vals.sort();
                            vals.dedup();
                            if !vals.is_empty() {
                                col["accepted_values"] = serde_json::json!(vals);
                                names.push("accepted_values".to_string());
                            }
                        }
                        "relationships" => {
                            let to = args.get("to").and_then(|t| t.as_str()).unwrap_or_default();
                            let field = args
                                .get("field")
                                .and_then(|f| f.as_str())
                                .unwrap_or_default();
                            if let Some(table) = macro_target(to)
                                && !field.is_empty()
                            {
                                col["references"] = serde_json::json!({"table": table, "column": field.to_lowercase()});
                                names.push("relationships".to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if !names.is_empty() {
        col["dbt_tests"] = serde_json::json!(names);
    }
    Some(col)
}

// ── RFC 0170: a model's own filters ─────────────────────────────────────────────────────────

/// A dbt project's `vars:` from `dbt_project.yml`, as text (`acc_ar: "1200"` → `1200`).
pub fn project_vars(dbt_project_yml: &str) -> std::collections::BTreeMap<String, String> {
    let Ok(doc) = serde_yaml::from_str::<serde_yaml::Value>(dbt_project_yml) else {
        return Default::default();
    };
    doc.get("vars")
        .and_then(|v| v.as_mapping())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let k = k.as_str()?.to_string();
                    let v = match v {
                        serde_yaml::Value::String(s) => s.clone(),
                        serde_yaml::Value::Number(n) => n.to_string(),
                        serde_yaml::Value::Bool(b) => b.to_string(),
                        _ => return None,
                    };
                    Some((k, v))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Placeholder for a Jinja value nobody can know statically; any predicate comparing against it
/// is dropped.
const UNKNOWN: &str = "__dbt_unknown__";

/// The SQL a dbt model compiles to, as far as can be known without running dbt: `ref('x')` → `x`,
/// `source('s', 't')` → `t`, `var('n')` → its `dbt_project.yml` value (else its default, else
/// unknown), `this` → the model; `{% … %}` and `{# … #}` blanked. Every replacement keeps the
/// line breaks it covered, so lines still cite the model file. Also returns which literal each
/// substituted var produced (`'1200'` → `acc_ar`): a var's name is a hint at a code's meaning.
pub fn render_model_sql(
    sql: &str,
    model: &str,
    vars: &std::collections::BTreeMap<String, String>,
) -> (String, std::collections::BTreeMap<String, String>) {
    let mut out = String::with_capacity(sql.len());
    let mut used = std::collections::BTreeMap::new();
    let mut rest = sql;
    while let Some(start) = rest.find('{') {
        let (open, close) = match rest[start..].get(..2) {
            Some("{{") => ("{{", "}}"),
            Some("{%") => ("{%", "%}"),
            Some("{#") => ("{#", "#}"),
            _ => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
                continue;
            }
        };
        out.push_str(&rest[..start]);
        let body_start = start + open.len();
        let Some(end) = rest[body_start..].find(close) else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let body = &rest[body_start..body_start + end];
        // `{% if is_incremental() %} … {% endif %}` is a load-time guard, not a business rule:
        // blank everything up to its `endif`.
        if open == "{%"
            && body
                .trim()
                .replace(' ', "")
                .starts_with("ifis_incremental()")
        {
            let after = body_start + end + close.len();
            if let Some(stop) = rest[after..].find("endif") {
                let stop = after + stop;
                let tail = rest[stop..]
                    .find("%}")
                    .map(|e| stop + e + 2)
                    .unwrap_or(rest.len());
                out.push_str(&"\n".repeat(rest[start..tail].matches('\n').count()));
                rest = &rest[tail..];
                continue;
            }
        }
        let newlines = body.matches('\n').count();
        let text = if open == "{{" {
            render_expr(body.trim(), model, vars, &mut used)
        } else {
            String::new()
        };
        out.push_str(&text);
        out.push_str(&"\n".repeat(newlines));
        rest = &rest[body_start + end + close.len()..];
    }
    out.push_str(rest);
    (out, used)
}

fn quoted_args(call: &str) -> Vec<String> {
    let Some(inner) = call
        .split_once('(')
        .and_then(|(_, a)| a.rsplit_once(')'))
        .map(|(a, _)| a)
    else {
        return Vec::new();
    };
    inner
        .split(',')
        .map(|a| a.trim().trim_matches(['\'', '"']).to_string())
        .collect()
}

fn render_expr(
    expr: &str,
    model: &str,
    vars: &std::collections::BTreeMap<String, String>,
    used: &mut std::collections::BTreeMap<String, String>,
) -> String {
    let head = expr.split('(').next().unwrap_or_default().trim();
    let args = quoted_args(expr);
    match head {
        "ref" => args.last().cloned().unwrap_or_else(|| UNKNOWN.into()),
        "source" => args.get(1).cloned().unwrap_or_else(|| UNKNOWN.into()),
        "var" => {
            let Some(name) = args.first() else {
                return UNKNOWN.into();
            };
            match vars.get(name).or(args.get(1)) {
                Some(v) => {
                    used.insert(format!("'{}'", v.replace('\'', "''")), name.clone());
                    used.insert(v.clone(), name.clone());
                    v.clone()
                }
                None => UNKNOWN.into(),
            }
        }
        "this" => model.into(),
        "config" => String::new(),
        _ => UNKNOWN.into(),
    }
}

/// The rendered model, parsed with the first dialect that accepts it.
fn parse_model(rendered: &str) -> Option<Vec<sqlparser::ast::Statement>> {
    use sqlparser::dialect::{ClickHouseDialect, Dialect, GenericDialect, PostgreSqlDialect};
    let dialects: [&dyn Dialect; 3] = [
        &PostgreSqlDialect {},
        &ClickHouseDialect {},
        &GenericDialect {},
    ];
    dialects
        .into_iter()
        .find_map(|d| sqlparser::parser::Parser::parse_sql(d, rendered).ok())
}

/// The predicates of a model's rendered SQL.
pub fn model_predicates(rendered: &str) -> Vec<crate::sql_predicates::PredicateSite> {
    parse_model(rendered)
        .map(|stmts| {
            stmts
                .iter()
                .flat_map(crate::sql_predicates::statement_predicates)
                .filter(|p| !p.values.iter().any(|v| v.contains(UNKNOWN)))
                .collect()
        })
        .unwrap_or_default()
}

/// A model's column lineage — each output column that passes a source column through.
pub fn model_lineage(rendered: &str) -> std::collections::BTreeMap<String, (String, String)> {
    match parse_model(rendered).as_deref() {
        Some([sqlparser::ast::Statement::Query(q)]) => crate::sql_predicates::output_lineage(q),
        _ => Default::default(),
    }
}

/// The table a `ref('x')` / `source('s', 'x')` names.
fn macro_target(s: &str) -> Option<String> {
    let inner = s.trim().strip_suffix(')')?;
    let (_, args) = inner.split_once('(')?;
    let last = args.split(',').next_back()?;
    let name = last.trim().trim_matches(['\'', '"']);
    (!name.is_empty()).then(|| name.to_lowercase())
}

fn parse_yaml_doc(
    doc: &serde_yaml::Value,
    path: &str,
    text: &str,
) -> (HashMap<String, YamlModelDoc>, Vec<YamlSourceTable>) {
    let mut models = HashMap::new();
    if let Some(seq) = doc.get("models").and_then(|v| v.as_sequence()) {
        for entry in seq {
            let Some(name) = entry.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let columns = entry
                .get("columns")
                .and_then(|v| v.as_sequence())
                .map(|cols| {
                    cols.iter()
                        .filter_map(|c| column_json(c, path, text, name))
                        .collect()
                })
                .unwrap_or_default();
            let description = entry
                .get("description")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            models.insert(
                name.to_string(),
                YamlModelDoc {
                    columns,
                    description,
                },
            );
        }
    }

    let mut sources = Vec::new();
    if let Some(seq) = doc.get("sources").and_then(|v| v.as_sequence()) {
        for src in seq {
            let Some(source_name) = src.get("name").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(tables) = src.get("tables").and_then(|v| v.as_sequence()) else {
                continue;
            };
            for table in tables {
                let Some(name) = table.get("name").and_then(|v| v.as_str()) else {
                    continue;
                };
                let description = table
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                let columns = table
                    .get("columns")
                    .and_then(|v| v.as_sequence())
                    .map(|cols| {
                        cols.iter()
                            .filter_map(|c| column_json(c, path, text, name))
                            .collect()
                    })
                    .unwrap_or_default();
                sources.push(YamlSourceTable {
                    name: name.to_string(),
                    source_name: source_name.to_string(),
                    description,
                    columns,
                });
            }
        }
    }

    (models, sources)
}

pub struct DbtAnalyzerPass {
    pass_id: String,
    /// RFC 0079-qualified identifier for this dbt project (its `dbt_project.yml`'s parent
    /// directory) — namespaces every `Table` id this pass mints so two dbt projects in one
    /// workspace never collide on a shared model name, and is stored on each `Table` as
    /// `properties["dbt_project"]` for display.
    dbt_root: String,
    /// (path relative to the dbt project root, raw YAML content) for every YAML file found under
    /// `models/**/` whose top level has a `models:` and/or `sources:` key.
    yml_files: Vec<(String, String)>,
    /// (path relative to the dbt project root, raw SQL content) for every `models/**/*.sql` file
    /// — one dbt model per file, regardless of whether any YAML documents it.
    sql_files: Vec<(String, String)>,
}

impl DbtAnalyzerPass {
    pub fn new(
        dbt_root: impl Into<String>,
        yml_files: Vec<(String, String)>,
        sql_files: Vec<(String, String)>,
    ) -> Self {
        let dbt_root = dbt_root.into();
        Self {
            pass_id: format!("dbt-analyzer:{dbt_root}"),
            dbt_root,
            yml_files,
            sql_files,
        }
    }
}

#[async_trait]
impl CompilerPass for DbtAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    /// `v3` = RFC 0170: a model carries its own filters (`predicates`). `v2` = documented columns
    /// carry description, type and dbt tests.
    fn version(&self) -> &str {
        // Includes `PREDICATES_VERSION`: a model carries `predicates`.
        "v4+predicates/2"
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        let mut all: Vec<&(String, String)> =
            self.yml_files.iter().chain(self.sql_files.iter()).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        for (path, content) in all {
            hasher.update(path.as_bytes());
            hasher.update(content.as_bytes());
        }
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut graph = KirGraph::new();

        // ── Parse every YAML file up front — documentation to merge onto models, plus the full
        //    source-table list (sources have no `.sql` file of their own). ──────────────────────
        let mut model_docs: HashMap<String, YamlModelDoc> = HashMap::new();
        let mut source_tables: Vec<(YamlSourceTable, String)> = Vec::new(); // (table, yml rel_path)
        for (rel_path, content) in &self.yml_files {
            let doc: serde_yaml::Value = match serde_yaml::from_str(content) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("cannot parse {rel_path} as YAML: {e}");
                    continue;
                }
            };
            let (docs, sources) = parse_yaml_doc(&doc, rel_path, content);
            for (name, doc) in docs {
                model_docs.insert(name, doc);
            }
            for source in sources {
                source_tables.push((source, rel_path.clone()));
            }
        }

        let vars = self
            .yml_files
            .iter()
            .find(|(p, _)| p.ends_with("dbt_project.yml"))
            .map(|(_, c)| project_vars(c))
            .unwrap_or_default();

        // ── Models: one `Table` per `.sql` file, regardless of YAML documentation. ──────────────
        let mut known: HashMap<String, KirId> = HashMap::new();
        for (rel_path, _content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            known
                .entry(name)
                .or_insert_with_key(|name| dbt_table_kir_id(&self.dbt_root, name));
        }
        for (rel_path, content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            let Some(&id) = known.get(&name) else {
                continue;
            };
            if graph.objects.iter().any(|o| o.id == id) {
                continue; // a duplicate model filename within this project — keep the first
            }

            let ev = KirEvidence::new(
                SourceLocation::file(rel_path),
                format!("dbt model {rel_path}"),
            );
            let mut obj = KirObject::new(&name, ObjectKind::Table)
                .with_property("dbt_kind", serde_json::json!("model"))
                .with_property("dbt_project", serde_json::json!(self.dbt_root))
                .with_evidence(graph.add_evidence(ev));
            obj.id = id;

            if let Some(materialized) = extract_materialized(content) {
                obj = obj.with_property("materialized", serde_json::json!(materialized));
            }
            // RFC 0170: the model's own filters — a model is a view, and its WHERE defines it.
            let (rendered, used_vars) = render_model_sql(content, &name, &vars);
            let predicates = model_predicates(&rendered);
            let lineage = model_lineage(&rendered);
            if !lineage.is_empty() {
                obj = obj.with_property(
                    "column_lineage",
                    serde_json::json!(
                        lineage
                            .iter()
                            .map(|(k, (r, c))| (k.clone(), serde_json::json!([r, c])))
                            .collect::<serde_json::Map<_, _>>()
                    ),
                );
            }
            obj = obj.with_property("source_path", serde_json::json!(rel_path));
            if !predicates.is_empty() {
                obj = obj.with_property(
                    "predicates",
                    crate::sql_predicates::predicates_json(&predicates, 1),
                );
                if !used_vars.is_empty() {
                    obj = obj.with_property("dbt_var_values", serde_json::json!(used_vars));
                }
            }
            if let Some(doc) = model_docs.get(&name) {
                if !doc.columns.is_empty() {
                    obj = obj.with_property("columns", serde_json::json!(doc.columns));
                }
                if let Some(desc) = &doc.description {
                    obj = obj.with_property("description", serde_json::json!(desc));
                }
            }

            graph.add_object(obj);
        }

        // ── Sources: one `Table` per declared `sources[].tables[]` entry — no `.sql` file backs
        //    these, so YAML is their only existence signal. ─────────────────────────────────────
        for (source, yml_rel_path) in &source_tables {
            if known.contains_key(&source.name) {
                continue; // a source name collided with a model name — keep the model, honestly rare
            }
            let id = dbt_table_kir_id(&self.dbt_root, &source.name);
            known.insert(source.name.clone(), id);

            let ev = KirEvidence::new(
                SourceLocation::file(yml_rel_path),
                format!("dbt source {}.{}", source.source_name, source.name),
            );
            let mut obj = KirObject::new(&source.name, ObjectKind::Table)
                .with_property("dbt_kind", serde_json::json!("source"))
                .with_property("dbt_source", serde_json::json!(source.source_name))
                .with_property("dbt_project", serde_json::json!(self.dbt_root))
                .with_evidence(graph.add_evidence(ev));
            obj.id = id;
            if let Some(desc) = &source.description {
                obj = obj.with_property("description", serde_json::json!(desc));
            }
            if !source.columns.is_empty() {
                obj = obj.with_property("columns", serde_json::json!(source.columns));
            }
            graph.add_object(obj);
        }

        // ── Dependencies: regex-scan each model's raw SQL for `ref()`/`source()` macro calls,
        //    resolved against `known`. Unresolvable refs (cross-package `ref()` into
        //    `dbt_packages/`, itself gitignored) are honestly skipped, never fabricated. ─────────
        for (rel_path, content) in &self.sql_files {
            let name = model_name_from_path(rel_path);
            let Some(&from_id) = known.get(&name) else {
                continue;
            };
            let mut emitted: HashSet<KirId> = HashSet::new();
            for macro_ref in find_macro_refs(content) {
                let Some(&to_id) = known.get(&macro_ref.target_name) else {
                    tracing::debug!(
                        model = %name,
                        target = %macro_ref.target_name,
                        "dbt-analyzer: unresolved ref()/source() — likely a cross-package \
                         reference (dbt_packages/, gitignored) — skipped, not fabricated"
                    );
                    continue;
                };
                if to_id == from_id || !emitted.insert(to_id) {
                    continue;
                }
                let ev = KirEvidence::new(
                    SourceLocation::at(rel_path, line_at(content, macro_ref.byte_offset)),
                    macro_ref.fragment,
                );
                let mut rel = KirRelationship::new(RelationshipKind::DependsOn, from_id, to_id);
                rel.id = dbt_depends_on_kir_id(from_id, to_id);
                rel.evidence.push(graph.add_evidence(ev));
                graph.add_relationship(rel);
            }
        }

        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            tables = knowledge.content.kir.objects.len(),
            dependencies = knowledge.content.kir.relationships.len(),
            "dbt-analyzer complete"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use std::sync::Arc;

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    async fn run_pass(
        yml_files: Vec<(&str, &str)>,
        sql_files: Vec<(&str, &str)>,
    ) -> ekos_kir::KirGraph {
        let (mut c, _dir) = ctx();
        let yml = yml_files
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let sql = sql_files
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string()))
            .collect();
        let mut pass = DbtAnalyzerPass::new("dbt", yml, sql);
        pass.run(&mut c).await.unwrap();

        let ids = c.artifact_store.list().unwrap();
        assert_eq!(ids.len(), 1, "exactly one KnowledgeArtifact expected");
        let json = c.artifact_store.read(&ids[0]).unwrap().unwrap();
        let knowledge: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        knowledge.content.kir
    }

    const SILVER_MODELS_YML: &str = r#"
version: 2
models:
  - name: silver_customer
    columns:
      - name: customer_id
        tests: [not_null, unique]
      - name: is_active
        tests: [not_null]
    description: "Cleaned customer records."
"#;

    const SILVER_SOURCES_YML: &str = r#"
version: 2
sources:
  - name: bronze
    schema: dvdrental
    tables:
      - name: bronze_customer
        description: "Raw customer rows from dvdrental."
"#;

    const SILVER_CUSTOMER_SQL: &str = r#"
{{ config(materialized='incremental', unique_key='customer_id') }}
SELECT * FROM {{ source('bronze', 'bronze_customer') }}
WHERE _is_deleted = false
"#;

    const SEM_CUSTOMER_CONTEXT_SQL: &str = r#"
WITH customer AS (
    SELECT * FROM {{ ref('silver_customer') }}
),
again AS (
    SELECT * FROM {{ ref('silver_customer') }}
)
SELECT * FROM customer
"#;

    #[test]
    fn model_sql_renders_refs_sources_and_vars_keeping_lines() {
        let vars = project_vars("vars:\n  acc_ar: \"1200\"\n  as_of: 2026-06-30\n");
        let sql = "{{ config(materialized='view') }}\nselect o.id\nfrom {{ source('raw', 'oe') }} as o\n\
join {{ ref('accounts') }} a on a.id = o.acc\n{% if is_incremental() %}\nand x = 1\n{% endif %}\n\
where o.oe_class_id = 1 and a.account_number = '{{ var(\"acc_ar\") }}'\n  and o.d > '{{ var(\"nope\") }}'";
        let (rendered, used) = render_model_sql(sql, "m", &vars);
        assert_eq!(rendered.lines().count(), sql.lines().count());
        assert!(rendered.contains("from oe as o"));
        assert!(rendered.contains("join accounts a"));
        assert_eq!(used.get("'1200'"), Some(&"acc_ar".to_string()));
        let c: Vec<(String, u64)> = model_predicates(&rendered)
            .iter()
            .map(|p| (p.canonical(), p.line))
            .collect();
        assert_eq!(
            c,
            vec![
                ("oe.oe_class_id IN (1)".to_string(), 8),
                ("accounts.account_number IN ('1200')".to_string(), 8),
            ],
            "the unknown var's comparison is dropped"
        );
    }

    /// RFC 0170 Phase 3: dbt tests are column facts — a declared domain, keys, references.
    #[test]
    fn dbt_column_tests_become_column_facts() {
        let text = "models:\n  - name: orders\n    columns:\n      - name: id\n        tests: [unique, not_null]\n      - name: status\n        description: \"1=open, 2=closed\"\n        data_tests:\n          - accepted_values:\n              values: ['open', 'closed']\n      - name: customer_id\n        tests:\n          - relationships:\n              to: ref('customers')\n              field: id\n      - name: kind\n        tests:\n          - accepted_values:\n              arguments:\n                values: [1, 3]\n                quote: false\n";
        let doc: serde_yaml::Value = serde_yaml::from_str(text).unwrap();
        let (models, _) = parse_yaml_doc(&doc, "models/schema.yml", text);
        let cols = &models["orders"].columns;
        assert_eq!(cols[0]["unique"], serde_json::json!(true));
        assert_eq!(cols[0]["not_null"], serde_json::json!(true));
        assert_eq!(
            cols[1]["accepted_values"],
            serde_json::json!(["'closed'", "'open'"])
        );
        assert_eq!(cols[1]["description_line"], serde_json::json!(6));
        assert_eq!(
            cols[2]["references"],
            serde_json::json!({"table": "customers", "column": "id"})
        );
        assert_eq!(cols[3]["accepted_values"], serde_json::json!(["1", "3"]));
        assert_eq!(
            macro_target("source('raw', 'Payments')"),
            Some("payments".into())
        );
    }

    #[tokio::test]
    async fn model_without_any_yaml_doc_still_becomes_a_table() {
        let graph = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
        let table = &graph.objects[0];
        assert_eq!(table.name, "silver_customer");
        assert_eq!(table.kind, ObjectKind::Table);
        assert_eq!(table.properties["dbt_kind"], "model");
        assert!(
            !table.properties.contains_key("columns"),
            "no fabricated columns when no YAML documents this model"
        );
        assert_eq!(table.properties["materialized"], "incremental");
    }

    #[tokio::test]
    async fn yaml_documented_columns_and_description_are_merged_onto_the_model() {
        let graph = run_pass(
            vec![("models/silver/_silver_models.yml", SILVER_MODELS_YML)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        let table = graph
            .objects
            .iter()
            .find(|o| o.name == "silver_customer")
            .unwrap();
        let cols = table.properties["columns"].as_array().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(table.properties["description"], "Cleaned customer records.");
    }

    #[tokio::test]
    async fn source_table_has_no_sql_file_but_still_becomes_a_table() {
        let graph = run_pass(
            vec![("models/silver/_silver_sources.yml", SILVER_SOURCES_YML)],
            vec![],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
        let table = &graph.objects[0];
        assert_eq!(table.name, "bronze_customer");
        assert_eq!(table.properties["dbt_kind"], "source");
        assert_eq!(table.properties["dbt_source"], "bronze");
    }

    #[tokio::test]
    async fn source_macro_call_resolves_to_a_real_depends_on_edge() {
        let graph = run_pass(
            vec![("models/silver/_silver_sources.yml", SILVER_SOURCES_YML)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.relationships.len(), 1);
        let rel = &graph.relationships[0];
        assert_eq!(rel.kind, RelationshipKind::DependsOn);
        let from = graph.objects.iter().find(|o| o.id == rel.from).unwrap();
        let to = graph.objects.iter().find(|o| o.id == rel.to).unwrap();
        assert_eq!(from.name, "silver_customer");
        assert_eq!(to.name, "bronze_customer");
    }

    #[tokio::test]
    async fn repeated_ref_to_the_same_model_produces_one_edge_not_two() {
        let graph = run_pass(
            vec![],
            vec![
                ("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL),
                (
                    "models/semantic/sem_customer_context.sql",
                    SEM_CUSTOMER_CONTEXT_SQL,
                ),
            ],
        )
        .await;
        let deps: Vec<_> = graph
            .relationships
            .iter()
            .filter(|r| {
                let from = graph.objects.iter().find(|o| o.id == r.from).unwrap();
                from.name == "sem_customer_context"
            })
            .collect();
        assert_eq!(
            deps.len(),
            1,
            "two ref('silver_customer') calls in one model must dedupe to one DependsOn edge"
        );
    }

    #[tokio::test]
    async fn ref_to_an_undeclared_model_is_honestly_skipped_not_fabricated() {
        const ORPHAN_SQL: &str = "SELECT * FROM {{ ref('some_package_model') }}";
        let graph = run_pass(vec![], vec![("models/gold/gold_thing.sql", ORPHAN_SQL)]).await;
        assert_eq!(
            graph.objects.len(),
            1,
            "gold_thing itself is still a real model"
        );
        assert_eq!(
            graph.relationships.len(),
            0,
            "an unresolvable ref() (e.g. a cross-package reference) must not fabricate an edge"
        );
    }

    #[tokio::test]
    async fn malformed_yaml_is_skipped_without_aborting_the_rest() {
        const BROKEN: &str = "not: [valid yaml: {{{";
        let graph = run_pass(
            vec![("models/silver/_broken.yml", BROKEN)],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph.objects.len(), 1);
    }

    #[tokio::test]
    async fn table_ids_are_deterministic_and_project_namespaced() {
        let graph1 = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        let graph2 = run_pass(
            vec![],
            vec![("models/silver/silver_customer.sql", SILVER_CUSTOMER_SQL)],
        )
        .await;
        assert_eq!(graph1.objects[0].id, graph2.objects[0].id);
        assert_eq!(
            graph1.objects[0].id,
            dbt_table_kir_id("dbt", "silver_customer")
        );
    }

    #[tokio::test]
    async fn nothing_found_emits_nothing() {
        let (mut c, _dir) = ctx();
        let mut pass = DbtAnalyzerPass::new("dbt", vec![], vec![]);
        pass.run(&mut c).await.unwrap();
        assert!(c.artifact_store.list().unwrap().is_empty());
    }
}
