//! RFC 0211 §3.5 — `OracleCatalogPass`: the live Oracle catalog (`ekos-plugin-oracle`'s
//! artifacts) into KIR.
//!
//! - `Table` per table, in `SqlAnalyzerPass`'s property shape (`columns` with `not_null` /
//!   `primary_key` / `unique`, `check_constraints` with predicates), plus `dialect: oracle`,
//!   `origin: live`, `database`, `owner`, `status`, statistics and the dictionary comment;
//! - `ForeignKey` relationships between them;
//! - `View` per view, read by RFC 0169's view reader from the dictionary text;
//! - `Procedure` / `ProcedureStatement` / `Trigger` per routine, read by RFC 0210's PL/SQL reader
//!   — the same objects a `db/oracle/*.sql` file yields, keyed by the catalog location instead;
//! - one `OracleCatalog` per (database, owner): census, view family, write probe, privileges.
//!
//! Every object carries `origin: live`. Identity never merges such an object, and name linking
//! uses it only for names no file-derived object has (`semantic::procedure_lineage::NameIndex`).
//! Pure structural mapping; no LLM, no connection (the observer already read everything).

use crate::plsql_analyzer::{PlSqlStats, recover_plsql};
use crate::view_analyzer::recover_views;
use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_sql_dialect_sdk::SqlDialectParser;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Bump when the mapping's output changes.
pub const LOGIC_VERSION: &str = "oracle-catalog/4";

/// The custom kind of the per-owner summary object.
pub const CATALOG_KIND: &str = "OracleCatalog";

/// What one run recovered, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OracleCatalogStats {
    pub catalogs: usize,
    pub tables: usize,
    pub foreign_keys: usize,
    pub foreign_keys_unresolved: usize,
    pub views: usize,
    pub views_unparsed: usize,
    pub routines: PlSqlStats,
    pub invalid_objects: usize,
    /// RFC 0211 §3.4: the profile tier each catalog was read at, tables a scan profiled or
    /// failed on, and the archive-log mode (`NOARCHIVELOG`, …, or `unknown`).
    pub profile_tiers: Vec<String>,
    pub tables_profiled: usize,
    pub tables_profile_failed: usize,
    pub log_modes: Vec<String>,
    /// Object artifacts left from an earlier read that the newest catalog no longer lists: an
    /// object dropped from the database. Not recovered.
    pub stale_objects: usize,
    /// RFC 0212: tables the application dictionary listed (0 when it was not read).
    pub dictionary_tables: usize,
}

fn live_id(prefix: &str, key: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("oracle-live-{prefix}:{key}").as_bytes(),
    ))
}

/// One catalog object artifact, as the observer wrote it.
struct Item {
    target: String,
    database: String,
    kind: String,
    owner: String,
    name: String,
    status: Option<String>,
    detail: Value,
}

fn item(target: &str, data: &Value) -> Option<Item> {
    let o = data.get("object")?;
    Some(Item {
        target: target.to_string(),
        database: data.get("database")?.as_str()?.to_string(),
        kind: o.get("kind")?.as_str()?.to_string(),
        owner: o.get("owner")?.as_str()?.to_string(),
        name: o.get("name")?.as_str()?.to_string(),
        status: o.get("status").and_then(Value::as_str).map(str::to_string),
        detail: o.get("detail").cloned().unwrap_or(Value::Null),
    })
}

/// Stamp the properties every live object carries.
fn stamp(o: &mut KirObject, database: &str, owner: &str) {
    for (k, v) in [
        ("origin", json!("live")),
        ("dialect", json!("oracle")),
        ("database", json!(database)),
        ("owner", json!(owner)),
    ] {
        o.properties.insert(k.into(), v);
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Constraints of one table, collected before the table object is built.
#[derive(Default)]
struct TableConstraints {
    primary_key: Vec<String>,
    unique: Vec<Vec<String>>,
    checks: Vec<Value>,
}

/// The pure mapping: every artifact `(target, data)` of one recover run → KIR.
pub fn recover_catalog(artifacts: &[(String, Value)]) -> (KirGraph, OracleCatalogStats) {
    let mut graph = KirGraph::new();
    let mut stats = OracleCatalogStats::default();
    let mut items: Vec<Item> = Vec::new();
    // The objects each newest catalog read lists, by its target. The artifact store keeps every
    // version of every object, so an object dropped from the database would otherwise live on.
    let mut listed: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    // RFC 0212: (database, owner, TABLE) → the application dictionary's flags.
    let mut dictionary: Dictionary = BTreeMap::new();
    for (target, data) in artifacts {
        match data.get("kind").and_then(Value::as_str) {
            Some("catalog") => {
                add_catalog(&mut graph, target, data);
                if let Some(objects) = data.get("objects").and_then(Value::as_array) {
                    listed.insert(
                        target.as_str(),
                        objects.iter().filter_map(Value::as_str).collect(),
                    );
                }
                count_catalog(&mut stats, data);
            }
            Some("object") => items.extend(item(target, data)),
            Some("dictionary") => read_dictionary(&mut dictionary, &mut stats, data),
            _ => {}
        }
    }
    stats.stale_objects = drop_stale(&mut items, &listed);
    items.sort_by(|a, b| a.target.cmp(&b.target));
    stats.invalid_objects = items
        .iter()
        .filter(|i| {
            i.status
                .as_deref()
                .is_some_and(|s| s != "VALID" && s != "ENABLED")
        })
        .count();

    let mut constraints: BTreeMap<(String, String, String), TableConstraints> = BTreeMap::new();
    for i in &items {
        let table = i.detail["table"].as_str().unwrap_or_default().to_string();
        let key = (i.database.clone(), i.owner.clone(), table.clone());
        let columns = strings(&i.detail["columns"]);
        match i.kind.as_str() {
            "primary_key" => constraints.entry(key).or_default().primary_key = columns,
            "unique_constraint" => constraints.entry(key).or_default().unique.push(columns),
            "check_constraint" => {
                let expression = i.detail["condition"].as_str().unwrap_or_default();
                let mut c = json!({ "name": i.name, "path": i.target, "expression": expression });
                if let Some(sites) = check_sites(&table, expression) {
                    c["predicates"] = crate::sql_predicates::predicates_json(&sites, 1);
                }
                constraints.entry(key).or_default().checks.push(c);
            }
            _ => {}
        }
    }

    let mut tables: BTreeMap<(String, String, String), KirId> = BTreeMap::new();
    for i in items.iter().filter(|i| i.kind == "table") {
        let key = (i.database.clone(), i.owner.clone(), i.name.clone());
        let id = add_table(&mut graph, i, constraints.get(&key));
        tables.insert(key, id);
        stats.tables += 1;
    }
    for i in items.iter().filter(|i| i.kind == "foreign_key") {
        if add_foreign_key(&mut graph, i, &tables) {
            stats.foreign_keys += 1;
        } else {
            stats.foreign_keys_unresolved += 1;
        }
    }
    for i in items.iter().filter(|i| i.kind == "view") {
        if add_view(&mut graph, i) {
            stats.views += 1;
        } else {
            stats.views_unparsed += 1;
        }
    }
    for i in &items {
        if let Some(s) = add_routine(&mut graph, i) {
            let (t, r) = (&mut stats.routines, &s.routines);
            t.routines.routines += r.routines;
            t.routines.complete += r.complete;
            t.routines.partial += r.partial;
            t.routines.statements += r.statements;
            t.routines.unrecovered += r.unrecovered;
            t.triggers += s.triggers;
            t.skipped += s.skipped;
            t.unterminated += s.unterminated;
        }
    }
    apply_dictionary(&mut graph, &dictionary);
    (graph, stats)
}

/// (database, owner, TABLE) → the application dictionary's flags (RFC 0212).
type Dictionary = BTreeMap<(String, String, String), Value>;

/// One catalog artifact's counts: the profile tier and scans, and the log mode.
fn count_catalog(stats: &mut OracleCatalogStats, data: &Value) {
    stats.catalogs += 1;
    let p = &data["profile"];
    let n = |k: &str| p[k].as_u64().unwrap_or(0) as usize;
    stats
        .profile_tiers
        .push(p["tier"].as_str().unwrap_or("p0").to_string());
    stats.tables_profiled += n("tables_profiled");
    stats.tables_profile_failed += n("tables_failed") + n("tables_skipped");
    stats.log_modes.push(
        data["log_state"]["log_mode"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
    );
}

/// RFC 0212: one dictionary artifact's table flags.
fn read_dictionary(dictionary: &mut Dictionary, stats: &mut OracleCatalogStats, data: &Value) {
    let database = data["database"].as_str().unwrap_or_default();
    let owner = data["owner"].as_str().unwrap_or_default();
    let tables = data["tables"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    stats.dictionary_tables += tables.len();
    for f in tables {
        if let Some(name) = f["table_name"].as_str() {
            dictionary.insert(
                (database.to_string(), owner.to_string(), name.to_uppercase()),
                json!({ "deleteable": f["deleteable"], "change_log": f["change_log"],
                        "table_name": name }),
            );
        }
    }
}

/// Drop objects an earlier read left behind that the newest catalog no longer lists; how many.
fn drop_stale(items: &mut Vec<Item>, listed: &BTreeMap<&str, BTreeSet<&str>>) -> usize {
    let before = items.len();
    items.retain(|i| {
        let catalog = format!("oracle://{}/{}", i.database, i.owner);
        match listed.get(catalog.as_str()) {
            Some(objects) => i
                .target
                .strip_prefix(&format!("{catalog}/"))
                .is_some_and(|entry| objects.contains(entry)),
            // A catalog written before the list existed: keep everything, as before.
            None => true,
        }
    });
    before - items.len()
}

/// RFC 0212: put each live table's dictionary flags on it, by name.
fn apply_dictionary(graph: &mut KirGraph, dictionary: &Dictionary) {
    if dictionary.is_empty() {
        return;
    }
    for o in graph
        .objects
        .iter_mut()
        .filter(|o| o.kind == ObjectKind::Table)
    {
        let key = |k: &str| {
            o.properties
                .get(k)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string()
        };
        let k = (key("database"), key("owner"), o.name.to_uppercase());
        if let Some(flags) = dictionary.get(&k) {
            o.properties.insert("dictionary".into(), flags.clone());
        }
    }
}

fn check_sites(table: &str, expression: &str) -> Option<Vec<ekos_kir::predicates::PredicateSite>> {
    let dialect = sqlparser::dialect::GenericDialect {};
    let expr = sqlparser::parser::Parser::new(&dialect)
        .try_with_sql(expression)
        .ok()?
        .parse_expr()
        .ok()?;
    Some(crate::sql_predicates::check_predicates(
        &table.to_lowercase(),
        &expr,
    ))
}

fn add_catalog(graph: &mut KirGraph, target: &str, data: &Value) {
    let database = data["database"].as_str().unwrap_or_default();
    let owner = data["owner"].as_str().unwrap_or_default();
    let ev = graph.add_evidence(KirEvidence::new(
        SourceLocation::file(target),
        format!("catalog of {owner} in database {database}, reconciled against the dictionary"),
    ));
    let mut o = KirObject::new(
        format!("{database}/{owner}"),
        ObjectKind::Custom(CATALOG_KIND.into()),
    )
    .with_evidence(ev);
    o.id = live_id("catalog", &format!("{database}:{owner}"));
    for k in [
        "family",
        "census",
        "listed",
        "columns",
        "folded_not_null",
        "other_constraints",
        "write_check",
        "any_privileges",
        "profile",
        "log_state",
    ] {
        if let Some(v) = data.get(k) {
            o.properties.insert(k.into(), v.clone());
        }
    }
    stamp(&mut o, database, owner);
    graph.add_object(o);
}

fn add_table(graph: &mut KirGraph, i: &Item, c: Option<&TableConstraints>) -> KirId {
    let single_unique =
        |col: &str| c.is_some_and(|c| c.unique.iter().any(|u| u.len() == 1 && u[0] == col));
    let pk_single = c
        .filter(|c| c.primary_key.len() == 1)
        .map(|c| c.primary_key[0].clone());
    let columns: Vec<Value> = i.detail["columns"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|col| {
            let name = col["name"].as_str().unwrap_or_default();
            let mut out = json!({ "name": name, "data_type": col["data_type"] });
            let pk = pk_single.as_deref() == Some(name);
            for (k, v) in [
                ("not_null", col["nullable"] == false || pk),
                ("primary_key", pk),
                ("unique", single_unique(name)),
            ] {
                if v {
                    out[k] = true.into();
                }
            }
            for (from, to) in [
                ("default", "default"),
                ("comment", "description"),
                ("profile", "profile"),
            ] {
                if let Some(v) = col.get(from) {
                    out[to] = v.clone();
                }
            }
            out
        })
        .collect();
    let ev = graph.add_evidence(KirEvidence::new(
        SourceLocation::file(&i.target),
        format!(
            "table {}.{} in the catalog of {}",
            i.owner, i.name, i.database
        ),
    ));
    let mut o = KirObject::new(&i.name, ObjectKind::Table).with_evidence(ev);
    o.id = live_id("table", &format!("{}:{}.{}", i.database, i.owner, i.name));
    o.properties.insert("columns".into(), Value::Array(columns));
    if let Some(c) = c.filter(|c| !c.checks.is_empty()) {
        o.properties
            .insert("check_constraints".into(), Value::Array(c.checks.clone()));
    }
    if let Some(c) = c.filter(|c| !c.primary_key.is_empty()) {
        o.properties
            .insert("primary_key".into(), json!(c.primary_key));
    }
    for (from, to) in [
        ("comment", "description"),
        ("num_rows", "num_rows"),
        ("last_analyzed", "last_analyzed"),
        ("profile", "profile"),
        ("temporary", "temporary"),
        ("partitioned", "partitioned"),
        ("iot_type", "iot_type"),
    ] {
        if let Some(v) = i.detail.get(from) {
            o.properties.insert(to.into(), v.clone());
        }
    }
    if let Some(s) = &i.status {
        o.properties.insert("status".into(), json!(s));
    }
    stamp(&mut o, &i.database, &i.owner);
    graph.add_object(o)
}

fn add_foreign_key(
    graph: &mut KirGraph,
    i: &Item,
    tables: &BTreeMap<(String, String, String), KirId>,
) -> bool {
    let from = i.detail["table"].as_str().unwrap_or_default();
    let Some(to) = i.detail["references_table"].as_str() else {
        return false;
    };
    let owner = i.detail["references_owner"].as_str().unwrap_or(&i.owner);
    let (Some(from_id), Some(to_id)) = (
        tables.get(&(i.database.clone(), i.owner.clone(), from.to_string())),
        tables.get(&(i.database.clone(), owner.to_string(), to.to_string())),
    ) else {
        return false;
    };
    let desc = format!(
        "{from}.{} → {to}.{}",
        strings(&i.detail["columns"]).join(", "),
        strings(&i.detail["references_columns"]).join(", ")
    );
    let ev = graph.add_evidence(KirEvidence::new(
        SourceLocation::file(&i.target),
        format!("foreign key {} {desc}", i.name),
    ));
    let mut rel = KirRelationship::new(RelationshipKind::ForeignKey, *from_id, *to_id);
    rel.id = live_id("fk", &format!("{}:{}.{}", i.database, i.owner, i.name));
    rel.evidence.push(ev);
    for (k, v) in [
        ("fk_desc", json!(desc)),
        ("constraint", json!(i.name)),
        ("validated", i.detail["validated"].clone()),
        ("on_delete", i.detail["on_delete"].clone()),
        ("origin", json!("live")),
    ] {
        rel.properties.insert(k.into(), v);
    }
    graph.add_relationship(rel);
    true
}

/// A view through RFC 0169's reader: the dictionary holds the query only, so the `CREATE VIEW`
/// head is restored around it, and the Oracle dialect's rewrites run first.
fn add_view(graph: &mut KirGraph, i: &Item) -> bool {
    if i.detail.get("text_truncated").is_some() {
        return false;
    }
    let body = i.detail["text"].as_str().unwrap_or_default();
    let sql = format!("CREATE OR REPLACE VIEW \"{}\" AS {body}\n;\n", i.name);
    let parser = ekos_plugin_sql_dialect_oracle::OracleDialectParser;
    let prepared = parser.preprocess(&sql);
    let (view_graph, _) = recover_views(&i.target, &prepared, parser.sqlparser_dialect().as_ref());
    if view_graph.objects.is_empty() {
        return false;
    }
    merge_stamped(graph, view_graph, i);
    true
}

/// A routine through RFC 0210's PL/SQL reader. The dictionary text starts at `FUNCTION name …`.
fn add_routine(graph: &mut KirGraph, i: &Item) -> Option<PlSqlStats> {
    if !matches!(
        i.kind.as_str(),
        "function" | "procedure" | "package_body" | "trigger"
    ) {
        return None;
    }
    let text = i.detail["text"].as_str().unwrap_or_default();
    let sql = format!("CREATE OR REPLACE {}\n/\n", text.trim_end());
    let (routine_graph, stats) = recover_plsql(&i.target, &sql, None);
    merge_stamped(graph, routine_graph, i);
    Some(stats)
}

fn merge_stamped(graph: &mut KirGraph, mut part: KirGraph, i: &Item) {
    for o in &mut part.objects {
        stamp(o, &i.database, &i.owner);
        if let Some(s) = &i.status {
            o.properties.insert("status".into(), json!(s));
        }
    }
    graph.objects.extend(part.objects);
    graph.relationships.extend(part.relationships);
    graph.events.extend(part.events);
    graph.evidence.extend(part.evidence);
}

/// The pass over every `oracle` observation artifact of the store.
pub struct OracleCatalogPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<OracleCatalogStats>>,
}

impl OracleCatalogPass {
    pub fn new(
        workspace_name: impl Into<String>,
        artifact_ids: Vec<ArtifactId>,
        stats: Arc<Mutex<OracleCatalogStats>>,
    ) -> Self {
        Self {
            pass_id: format!("oracle-catalog:{}", workspace_name.into()),
            artifact_ids,
            stats,
        }
    }
}

#[async_trait]
impl CompilerPass for OracleCatalogPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(ToString::to_string).collect();
        ids.sort();
        ids.push(LOGIC_VERSION.to_string());
        ids.push(crate::sql_predicates::PREDICATES_VERSION.to_string());
        // Routines and views go through those readers: a change to either is a change here.
        ids.push(crate::plpgsql_analyzer::LOGIC_VERSION.to_string());
        ids.push(crate::plsql_analyzer::LOGIC_VERSION.to_string());
        ids.push(crate::view_analyzer::LOGIC_VERSION.to_string());
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut artifacts = Vec::with_capacity(self.artifact_ids.len());
        for id in &self.artifact_ids {
            match ctx.artifact_store.read(id) {
                Ok(Some(json)) => artifacts.push((
                    json["target"].as_str().unwrap_or_default().to_string(),
                    json["data"].clone(),
                )),
                Ok(None) => {}
                Err(e) => ctx.diagnostics.lock().unwrap().warning(
                    "ORACLE001",
                    format!("cannot read oracle artifact {id}: {e}"),
                ),
            }
        }
        let (graph, stats) = recover_catalog(&artifacts);
        if stats.views_unparsed > 0 {
            ctx.diagnostics.lock().unwrap().warning(
                "ORACLE002",
                format!(
                    "{} view(s) from the Oracle catalog could not be read; they are counted in \
                     the catalog census but have no View object",
                    stats.views_unparsed
                ),
            );
        }
        *self.stats.lock().unwrap() = stats;
        // A catalog read is the database's whole state: it replaces earlier reads, so an object
        // dropped from the database leaves the compiled model too.
        let knowledge =
            ekos_artifact::KnowledgeArtifact::new(&self.pass_id, self.artifact_ids.clone(), graph)
                .superseding_earlier_runs();
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: &str, name: &str, detail: Value) -> (String, Value) {
        (
            format!("oracle://erp/ADEMPIERE/{kind}/{name}"),
            json!({
                "kind": "object",
                "database": "erp",
                "object": { "kind": kind, "owner": "ADEMPIERE", "name": name, "status": "VALID", "detail": detail },
            }),
        )
    }

    fn catalog() -> Vec<(String, Value)> {
        vec![
            (
                "oracle://erp/ADEMPIERE".into(),
                json!({ "kind": "catalog", "database": "erp", "owner": "ADEMPIERE", "family": "dba",
                        "census": { "TABLE": { "valid": 2, "invalid": 0 } },
                        "write_check": { "result": "refused_by_server", "message": "ORA-01031" },
                        "log_state": { "log_mode": "NOARCHIVELOG" } }),
            ),
            object(
                "table",
                "C_INVOICE",
                json!({ "columns": [
                    { "name": "C_INVOICE_ID", "data_type": "NUMBER(10,0)", "nullable": false },
                    { "name": "ISACTIVE", "data_type": "CHAR(1)", "nullable": false, "default": "'Y'" },
                    { "name": "C_BPARTNER_ID", "data_type": "NUMBER(10,0)", "nullable": true, "comment": "Business partner",
                      "profile": { "tier": "p2", "nulls": 0, "nulls_exact": true } },
                ], "num_rows": 12, "comment": "Invoice",
                   "profile": { "tier": "p2", "row_count": 12, "row_count_exact": true } }),
            ),
            object(
                "table",
                "C_BPARTNER",
                json!({ "columns": [{ "name": "C_BPARTNER_ID", "data_type": "NUMBER(10,0)", "nullable": false }] }),
            ),
            object(
                "primary_key",
                "C_INVOICE_KEY",
                json!({ "table": "C_INVOICE", "columns": ["C_INVOICE_ID"] }),
            ),
            object(
                "primary_key",
                "C_BPARTNER_KEY",
                json!({ "table": "C_BPARTNER", "columns": ["C_BPARTNER_ID"] }),
            ),
            object(
                "check_constraint",
                "C_INVOICE_ISACTIVE_CHECK",
                json!({ "table": "C_INVOICE", "columns": ["ISACTIVE"], "condition": "IsActive IN ('Y','N')" }),
            ),
            object(
                "foreign_key",
                "CBPARTNER_CINVOICE",
                json!({ "table": "C_INVOICE", "columns": ["C_BPARTNER_ID"], "references_owner": "ADEMPIERE",
                        "references_table": "C_BPARTNER", "references_columns": ["C_BPARTNER_ID"],
                        "validated": false, "on_delete": "NO ACTION" }),
            ),
            object(
                "view",
                "C_INVOICE_V",
                json!({ "text": "SELECT i.C_Invoice_ID, bp.Name FROM C_Invoice i INNER JOIN C_BPartner bp ON (i.C_BPartner_ID=bp.C_BPartner_ID)" }),
            ),
            object(
                "function",
                "INVOICEOPEN",
                json!({ "text": "FUNCTION invoiceOpen (p_C_Invoice_ID IN NUMBER) RETURN NUMBER\nAS\n  v_Total NUMBER := 0;\nBEGIN\n  SELECT GrandTotal INTO v_Total FROM C_Invoice WHERE C_Invoice_ID = p_C_Invoice_ID;\n  RETURN v_Total;\nEND invoiceOpen;" }),
            ),
        ]
    }

    #[test]
    fn tables_columns_and_keys_take_the_sql_analyzer_shape() {
        let (g, stats) = recover_catalog(&catalog());
        assert_eq!(
            (stats.catalogs, stats.tables, stats.foreign_keys),
            (1, 2, 1)
        );
        let t = g.objects.iter().find(|o| o.name == "C_INVOICE").unwrap();
        assert_eq!(t.kind, ObjectKind::Table);
        let cols = t.properties["columns"].as_array().unwrap();
        assert_eq!(cols[0]["primary_key"], true);
        assert_eq!(cols[0]["not_null"], true);
        assert_eq!(cols[1]["default"], "'Y'");
        assert!(cols[2].get("not_null").is_none());
        assert_eq!(cols[2]["description"], "Business partner");
        assert_eq!(cols[2]["profile"]["nulls_exact"], true);
        assert_eq!(t.properties["profile"]["row_count"], 12);
        assert_eq!(t.properties["origin"], "live");
        assert_eq!(t.properties["dialect"], "oracle");
        assert_eq!(t.properties["description"], "Invoice");
        let checks = t.properties["check_constraints"].as_array().unwrap();
        assert_eq!(checks[0]["expression"], "IsActive IN ('Y','N')");
        assert!(
            checks[0]["predicates"]
                .as_array()
                .is_some_and(|p| !p.is_empty())
        );
        let fk = g
            .relationships
            .iter()
            .find(|r| r.kind == RelationshipKind::ForeignKey)
            .unwrap();
        assert_eq!(
            fk.properties["fk_desc"],
            "C_INVOICE.C_BPARTNER_ID → C_BPARTNER.C_BPARTNER_ID"
        );
    }

    #[test]
    fn views_and_routines_are_read_like_their_files() {
        let (g, stats) = recover_catalog(&catalog());
        assert_eq!((stats.views, stats.views_unparsed), (1, 0));
        let v = g
            .objects
            .iter()
            .find(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "View"))
            .unwrap();
        assert_eq!(v.properties["origin"], "live");
        let p = g
            .objects
            .iter()
            .find(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Procedure"))
            .unwrap();
        assert_eq!(p.name.to_lowercase(), "invoiceopen");
        assert_eq!(p.properties["origin"], "live");
        assert_eq!(stats.routines.routines.routines, 1);
        assert!(stats.routines.routines.statements >= 2);
    }

    /// An object an earlier read stored but the newest catalog no longer lists was dropped from
    /// the database: it must not be recovered.
    #[test]
    fn an_object_the_newest_catalog_does_not_list_is_not_recovered() {
        let mut artifacts = catalog();
        let listed: Vec<String> = artifacts
            .iter()
            .filter_map(|(t, _)| t.strip_prefix("oracle://erp/ADEMPIERE/"))
            .filter(|e| *e != "table/C_BPARTNER")
            .map(str::to_string)
            .collect();
        artifacts[0].1["objects"] = json!(listed);
        let (g, stats) = recover_catalog(&artifacts);
        assert_eq!(stats.stale_objects, 1);
        assert!(!g.objects.iter().any(|o| o.name == "C_BPARTNER"));
        assert!(g.objects.iter().any(|o| o.name == "C_INVOICE"));
        // Without a list (a catalog written before it existed) everything is kept.
        let (_, old) = recover_catalog(&catalog());
        assert_eq!(old.stale_objects, 0);
    }

    /// RFC 0212: the dictionary's flags land on the live table of the same name.
    #[test]
    fn dictionary_flags_land_on_the_table() {
        let mut artifacts = catalog();
        artifacts.push((
            "oracle://erp/ADEMPIERE/dictionary".into(),
            json!({ "kind": "dictionary", "database": "erp", "owner": "ADEMPIERE",
                    "tables": [{ "table_name": "C_Invoice", "deleteable": true, "change_log": false }] }),
        ));
        let (g, stats) = recover_catalog(&artifacts);
        assert_eq!(stats.dictionary_tables, 1);
        let t = g.objects.iter().find(|o| o.name == "C_INVOICE").unwrap();
        assert_eq!(t.properties["dictionary"]["deleteable"], true);
        assert_eq!(t.properties["dictionary"]["change_log"], false);
        let other = g.objects.iter().find(|o| o.name == "C_BPARTNER").unwrap();
        assert!(!other.properties.contains_key("dictionary"));
    }

    #[test]
    fn the_catalog_summary_is_one_keyed_object_per_owner() {
        let (g, _) = recover_catalog(&catalog());
        let c: Vec<&KirObject> = g
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == CATALOG_KIND))
            .collect();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].name, "erp/ADEMPIERE");
        assert_eq!(
            c[0].properties["write_check"]["result"],
            "refused_by_server"
        );
        assert_eq!(c[0].properties["log_state"]["log_mode"], "NOARCHIVELOG");
    }

    #[test]
    fn ids_are_stable_and_scoped_by_database_and_owner() {
        let (a, _) = recover_catalog(&catalog());
        let (b, _) = recover_catalog(&catalog());
        let ids = |g: &KirGraph| g.objects.iter().map(|o| o.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        assert_ne!(
            live_id("table", "erp:ADEMPIERE.C_INVOICE"),
            live_id("table", "erp:OTHER.C_INVOICE")
        );
    }
}
