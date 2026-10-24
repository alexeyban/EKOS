//! RFC 0212 §3.4 — how each table is updated and deleted, from the evidence the code and the
//! database give, at `ekos commit`.
//!
//! Inputs, all already in the ledger:
//!
//! - Java methods' embedded SQL (`JavaMethod.sql`, RFC 0209): raw `UPDATE`s with the columns they
//!   set, raw `DELETE`s with their shape (`plpgsql_footprint`, RFC 0212 §3.1);
//! - `WritesTo {via: "po"}` edges: `PO` deletes on a declared model object (RFC 0212 §3.2);
//! - `ProcedureStatement`s (PL/SQL, PL/pgSQL, trigger bodies), except those in migration scripts —
//!   a migration's data fix runs once, it is not how the application behaves;
//! - live `ForeignKey`s with `on_delete: CASCADE` (RFC 0211);
//! - the application dictionary's flags on live tables (`dictionary`, RFC 0212 §3.3, opt-in).
//!
//! One `Custom("WriteBehaviour")` per table, keyed by the table, with every class and the evidence
//! for it. Nothing is guessed: without the dictionary, user deletes are `unknown`, and a table no
//! code deletes is `never_deleted_in_code` — a statement about code only.

use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

pub const KIND: &str = "WriteBehaviour";

pub const PO_ONLY: &str = "po_only";
pub const RAW_SETS_UPDATED: &str = "raw_sql_sets_updated";
pub const RAW_SKIPS_UPDATED: &str = "raw_sql_skips_updated";
pub const NO_UPDATED_COLUMN: &str = "no_updated_column";

pub const DELETED_BY_APP: &str = "deleted_by_app";
pub const DELETED_BY_USERS: &str = "deleted_by_users";
pub const DELETED_BY_CASCADE: &str = "deleted_by_cascade";
pub const PURGED: &str = "purged";
pub const SOFT_DELETE_ONLY: &str = "soft_delete_only";
pub const NEVER_DELETED_IN_CODE: &str = "never_deleted_in_code";

/// Counts by class, for the commit summary line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteBehaviourStats {
    pub tables: usize,
    pub update: BTreeMap<&'static str, usize>,
    pub delete: BTreeMap<&'static str, usize>,
    pub change_log: BTreeMap<&'static str, usize>,
    /// Whether any table carried dictionary flags.
    pub dictionary: bool,
}

fn custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn prop<'a>(o: &'a KirObject, key: &str) -> Option<&'a str> {
    o.properties.get(key).and_then(Value::as_str)
}

/// `schema.table` → `table`, lower-cased.
fn bare(name: &str) -> String {
    name.rsplit('.')
        .next()
        .unwrap_or(name)
        .trim_matches('"')
        .to_ascii_lowercase()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(bare).collect())
        .unwrap_or_default()
}

/// A migration script: one RFC 0210 read as a change set, or any file under a `migration…`
/// directory (older scripts are not named by date, so they never became change sets).
fn is_migration(path: &str, scripts: &BTreeSet<&str>) -> bool {
    scripts.contains(path)
        || path
            .split('/')
            .any(|c| c.to_ascii_lowercase().starts_with("migration"))
}

/// What one table's evidence says, before classification.
#[derive(Default)]
struct Facts {
    evidence: Vec<Value>,
    raw_updates: usize,
    raw_updates_skipping: usize,
    app_deletes: usize,
    purges: usize,
    cascades: usize,
}

impl Facts {
    fn update(&mut self, sets: &[String], cite: Value) {
        let sets_updated = sets.iter().any(|c| c == "updated");
        self.raw_updates += 1;
        if !sets_updated {
            self.raw_updates_skipping += 1;
        }
        let mut e = cite;
        e["kind"] = "raw_update".into();
        e["sets_updated"] = sets_updated.into();
        self.evidence.push(e);
    }

    fn delete(&mut self, shapes: &[String], cite: Value) {
        let purge = shapes.iter().any(|s| s == "age" || s == "all_rows");
        if purge {
            self.purges += 1;
        } else {
            self.app_deletes += 1;
        }
        let mut e = cite;
        e["kind"] = "raw_delete".into();
        e["shapes"] = json!(shapes);
        self.evidence.push(e);
    }
}

type FactsByTable = BTreeMap<String, Facts>;

fn method_name(m: &KirObject) -> String {
    format!("{}.{}", prop(m, "owner").unwrap_or_default(), m.name)
}

/// Raw SQL in Java methods.
fn java_sql(objects: &[KirObject], paths: &HashMap<KirId, String>, facts: &mut FactsByTable) {
    for m in objects.iter().filter(|o| custom(o, "JavaMethod")) {
        let Some(sql) = m.properties.get("sql").and_then(Value::as_array) else {
            continue;
        };
        let at = method_name(m);
        for st in sql {
            let cite = json!({ "source": "java", "method": at, "path": paths.get(&m.id),
                               "line": st["line"] });
            record(facts, st, &cite);
        }
    }
}

/// One statement's updates and deletes, from its `updates` / `update_sets` / `deletes` /
/// `delete_shapes` — the same keys on a Java statement and a `ProcedureStatement`.
fn record(facts: &mut FactsByTable, st: &Value, cite: &Value) {
    for t in strings(st.get("updates")) {
        let sets = strings(st.get("update_sets").and_then(|u| u.get(&t)));
        facts.entry(t).or_default().update(&sets, cite.clone());
    }
    for t in strings(st.get("deletes")) {
        let shapes = strings(st.get("delete_shapes").and_then(|d| d.get(&t)));
        facts.entry(t).or_default().delete(&shapes, cite.clone());
    }
}

/// `PO` deletes: `WritesTo {via: "po"}` edges to a `ModelTable`.
fn po_deletes(
    relationships: &[KirRelationship],
    model_names: &HashMap<KirId, String>,
    by_id: &HashMap<KirId, &KirObject>,
    paths: &HashMap<KirId, String>,
    facts: &mut FactsByTable,
) {
    let po = relationships.iter().filter(|r| {
        matches!(&r.kind, RelationshipKind::Custom(k) if k == "WritesTo")
            && r.properties.get("via") == Some(&json!("po"))
    });
    for r in po {
        let Some(table) = model_names.get(&r.to) else {
            continue;
        };
        let f = facts.entry(table.clone()).or_default();
        f.app_deletes += 1;
        f.evidence.push(json!({
            "kind": "po_delete", "source": "java",
            "method": by_id.get(&r.from).map(|m| method_name(m)),
            "path": paths.get(&r.from), "lines": r.properties.get("lines"),
        }));
    }
}

/// Routine statements outside migration scripts. A live routine repeats a file's routine of the
/// same name, so live statements count only when no file defines that routine.
fn routines(objects: &[KirObject], facts: &mut FactsByTable) {
    let scripts: BTreeSet<&str> = objects
        .iter()
        .filter(|o| custom(o, "SchemaChange") || custom(o, "SchemaChangeSet"))
        .filter_map(|o| prop(o, "source_path"))
        .collect();
    let statements: Vec<&KirObject> = objects
        .iter()
        .filter(|o| custom(o, "ProcedureStatement"))
        .collect();
    let file_routines: BTreeSet<String> = statements
        .iter()
        .filter(|o| prop(o, "origin") != Some("live"))
        .filter_map(|o| prop(o, "procedure").map(bare))
        .collect();
    for st in statements {
        let path = prop(st, "source_path").unwrap_or_default();
        let routine = prop(st, "procedure").unwrap_or_default();
        let repeated = prop(st, "origin") == Some("live") && file_routines.contains(&bare(routine));
        if is_migration(path, &scripts) || repeated {
            continue;
        }
        let cite = json!({ "source": "routine", "routine": routine, "path": path,
                           "line": st.properties.get("line") });
        record(facts, &json!(st.properties), &cite);
    }
}

/// Live foreign keys that delete a child with its parent.
fn cascades(
    relationships: &[KirRelationship],
    by_id: &HashMap<KirId, &KirObject>,
    facts: &mut FactsByTable,
) {
    for r in relationships.iter().filter(|r| {
        matches!(&r.kind, RelationshipKind::ForeignKey)
            && r.properties.get("on_delete") == Some(&json!("CASCADE"))
    }) {
        let (Some(child), Some(parent)) = (by_id.get(&r.from), by_id.get(&r.to)) else {
            continue;
        };
        if prop(child, "origin") != Some("live") {
            continue;
        }
        let f = facts.entry(bare(&child.name)).or_default();
        f.cascades += 1;
        f.evidence.push(json!({
            "kind": "cascade", "parent": parent.name,
            "constraint": r.properties.get("constraint"),
        }));
    }
}

/// A table's column names, lower-cased: a live table lists them, a `ModelTable` keys them.
fn column_names(table: &KirObject) -> BTreeSet<String> {
    match table.properties.get("columns") {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|c| c.get("name").and_then(Value::as_str))
            .map(str::to_ascii_lowercase)
            .collect(),
        Some(Value::Object(m)) => m.keys().map(|k| k.to_ascii_lowercase()).collect(),
        _ => BTreeSet::new(),
    }
}

/// One table's classes.
struct Classes {
    update: &'static str,
    delete: Vec<&'static str>,
    change_log: &'static str,
}

fn classify(f: &Facts, columns: &BTreeSet<String>, dictionary: Option<&Value>) -> Classes {
    let flag = |k: &str| dictionary.and_then(|d| d.get(k)).and_then(Value::as_bool);
    let update = if !columns.contains("updated") {
        NO_UPDATED_COLUMN
    } else if f.raw_updates == 0 {
        PO_ONLY
    } else if f.raw_updates_skipping > 0 {
        RAW_SKIPS_UPDATED
    } else {
        RAW_SETS_UPDATED
    };
    let mut delete: Vec<&'static str> = [
        (f.app_deletes > 0, DELETED_BY_APP),
        (flag("deleteable") == Some(true), DELETED_BY_USERS),
        (f.cascades > 0, DELETED_BY_CASCADE),
        (f.purges > 0, PURGED),
    ]
    .into_iter()
    .filter_map(|(on, class)| on.then_some(class))
    .collect();
    if delete.is_empty() {
        let soft = flag("deleteable") == Some(false) && columns.contains("isactive");
        delete.push(if soft {
            SOFT_DELETE_ONLY
        } else {
            NEVER_DELETED_IN_CODE
        });
    }
    let change_log = match flag("change_log") {
        Some(true) => "recorded",
        Some(false) => "not_recorded",
        None => "unknown",
    };
    Classes {
        update,
        delete,
        change_log,
    }
}

/// The `WriteBehaviour` object for `table`, and its `Describes` link.
fn behaviour_object(
    table: &KirObject,
    c: &Classes,
    mut citations: Vec<Value>,
) -> (KirObject, KirRelationship) {
    if let Some(d) = table.properties.get("dictionary") {
        citations.push(json!({ "kind": "dictionary", "flags": d }));
    }
    let mut o = KirObject::new(table.name.clone(), ObjectKind::Custom(KIND.into()));
    o.id = KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("write-behaviour:{}", table.id).as_bytes(),
    ));
    for (k, v) in [
        ("table", json!(table.name)),
        ("update_class", json!(c.update)),
        ("delete_classes", json!(c.delete)),
        ("change_log", json!(c.change_log)),
        (
            "change_log_note",
            json!(
                "iDempiere's PO.delete logs a delete per column only when the table's \
                 IsChangeLog flag is on and a session exists; raw SQL is never logged"
            ),
        ),
        ("citation_count", json!(citations.len())),
        ("citations", json!(citations)),
    ] {
        o.properties.insert(k.into(), v);
    }
    for k in ["origin", "database", "owner", "dialect"] {
        if let Some(v) = table.properties.get(k) {
            o.properties.insert(k.into(), v.clone());
        }
    }
    let link = KirRelationship::deterministic(
        RelationshipKind::Custom("Describes".into()),
        o.id,
        table.id,
        "write-behaviour",
    );
    (o, link)
}

/// Derive the write behaviour of every table. `method_paths` gives the source file of a
/// `JavaMethod` by id (from its evidence), when known.
pub fn derive(
    objects: &[KirObject],
    relationships: &[KirRelationship],
    method_paths: &HashMap<KirId, String>,
) -> (KirGraph, WriteBehaviourStats) {
    let live: Vec<&KirObject> = objects
        .iter()
        .filter(|o| o.kind == ObjectKind::Table && prop(o, "origin") == Some("live"))
        .collect();
    let models: Vec<&KirObject> = objects.iter().filter(|o| custom(o, "ModelTable")).collect();
    // The subjects: the live tables, or the model when no database was read. Unique names only.
    let subjects = if live.is_empty() { &models } else { &live };
    let mut by_name: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for s in subjects {
        by_name.entry(bare(&s.name)).or_default().push(s);
    }
    let model_names: HashMap<KirId, String> =
        models.iter().map(|m| (m.id, bare(&m.name))).collect();
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();

    let mut facts = FactsByTable::new();
    java_sql(objects, method_paths, &mut facts);
    po_deletes(
        relationships,
        &model_names,
        &by_id,
        method_paths,
        &mut facts,
    );
    routines(objects, &mut facts);
    cascades(relationships, &by_id, &mut facts);

    let mut graph = KirGraph::new();
    let mut stats = WriteBehaviourStats::default();
    for (name, tables) in &by_name {
        let [table] = tables.as_slice() else {
            continue;
        };
        let f = facts.remove(name).unwrap_or_default();
        let dictionary = table.properties.get("dictionary");
        stats.dictionary |= dictionary.is_some();
        let c = classify(&f, &column_names(table), dictionary);
        stats.tables += 1;
        *stats.update.entry(c.update).or_default() += 1;
        for d in &c.delete {
            *stats.delete.entry(d).or_default() += 1;
        }
        *stats.change_log.entry(c.change_log).or_default() += 1;
        let (o, link) = behaviour_object(table, &c, f.evidence);
        graph.objects.push(o);
        graph.relationships.push(link);
    }
    (graph, stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_table(name: &str, cols: &[&str]) -> KirObject {
        let columns: Vec<Value> = cols.iter().map(|c| json!({ "name": c })).collect();
        KirObject::new(name, ObjectKind::Table)
            .with_property("origin", json!("live"))
            .with_property("columns", json!(columns))
    }

    fn statement(path: &str, props: Value) -> KirObject {
        let mut o = KirObject::new("s", ObjectKind::Custom("ProcedureStatement".into()))
            .with_property("source_path", json!(path))
            .with_property("procedure", json!("p"))
            .with_property("line", json!(3));
        for (k, v) in props.as_object().unwrap() {
            o.properties.insert(k.clone(), v.clone());
        }
        o
    }

    fn classes(g: &KirGraph, table: &str) -> (String, Vec<String>, String) {
        let o = g.objects.iter().find(|o| o.name == table).unwrap();
        (
            o.properties["update_class"].as_str().unwrap().to_string(),
            o.properties["delete_classes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect(),
            o.properties["change_log"].as_str().unwrap().to_string(),
        )
    }

    #[test]
    fn every_class_follows_from_its_evidence() {
        let std_cols = ["C_ID", "UPDATED", "ISACTIVE"];
        let mut flagged = live_table("AD_ORG", &std_cols);
        flagged.properties.insert(
            "dictionary".into(),
            json!({ "deleteable": false, "change_log": true }),
        );
        let mut users = live_table("C_ORDER", &std_cols);
        users.properties.insert(
            "dictionary".into(),
            json!({ "deleteable": true, "change_log": false }),
        );
        let parent = live_table("C_INVOICE", &std_cols);
        let child = live_table("C_INVOICETAX", &std_cols);
        let mut fk =
            KirRelationship::deterministic(RelationshipKind::ForeignKey, child.id, parent.id, "fk");
        fk.properties.insert("on_delete".into(), json!("CASCADE"));
        let objects = vec![
            flagged,
            users,
            parent,
            child,
            live_table("AD_SESSION", &std_cols),
            live_table("C_PAYMENT", &std_cols),
            live_table("T_REPORT", &["AD_PINSTANCE_ID"]),
            statement(
                "db/oracle/procedures/DBA_Cleanup.sql",
                json!({ "deletes": ["ad_session"], "delete_shapes": { "ad_session": ["age"] } }),
            ),
            statement(
                "db/oracle/procedures/Pay.sql",
                json!({ "updates": ["c_payment"], "update_sets": { "c_payment": ["isallocated"] },
                        "deletes": ["c_payment"], "delete_shapes": { "c_payment": ["selective"] } }),
            ),
            // A migration's data fix: not how the application behaves.
            // An undated historic script redefining a routine: a migration all the same.
            statement(
                "migration-historic/353a-354a/oracle/456_FR2783860.sql",
                json!({ "deletes": ["c_invoice"], "delete_shapes": { "c_invoice": ["all_rows"] } }),
            ),
            statement(
                "migration/iD12/oracle/x.sql",
                json!({ "deletes": ["c_invoice"], "delete_shapes": { "c_invoice": ["selective"] } }),
            )
            .with_property("source_path", json!("migration/iD12/oracle/x.sql")),
            KirObject::new("x", ObjectKind::Custom("SchemaChangeSet".into()))
                .with_property("source_path", json!("migration/iD12/oracle/x.sql")),
        ];
        let (g, stats) = derive(&objects, &[fk], &HashMap::new());
        let c = |t: &str| classes(&g, t);
        assert_eq!(c("AD_ORG").1, [SOFT_DELETE_ONLY]);
        assert_eq!(c("AD_ORG").2, "recorded");
        assert_eq!(c("C_ORDER").1, [DELETED_BY_USERS]);
        assert_eq!(c("C_ORDER").2, "not_recorded");
        assert_eq!(c("C_INVOICETAX").1, [DELETED_BY_CASCADE]);
        assert_eq!(
            c("C_INVOICE").1,
            [NEVER_DELETED_IN_CODE],
            "migration ignored"
        );
        assert_eq!(c("C_INVOICE").2, "unknown");
        assert_eq!(c("AD_SESSION").1, [PURGED]);
        assert_eq!(c("C_PAYMENT").0, RAW_SKIPS_UPDATED);
        assert_eq!(c("C_PAYMENT").1, [DELETED_BY_APP]);
        assert_eq!(c("AD_ORG").0, PO_ONLY);
        assert_eq!(c("T_REPORT").0, NO_UPDATED_COLUMN);
        assert_eq!(stats.tables, 7);
        assert!(stats.dictionary);
        assert_eq!(g.relationships.len(), 7, "one Describes per table");
    }

    #[test]
    fn java_raw_sql_and_po_deletes_are_cited_with_their_method() {
        let model = KirObject::new("C_OrderLine", ObjectKind::Custom("ModelTable".into()))
            .with_property("columns", json!({ "Updated": {}, "IsActive": {} }));
        let mut method = KirObject::new("drop", ObjectKind::Custom("JavaMethod".into()))
            .with_property("owner", json!("org.compiere.model.MOrderLine"))
            .with_property(
                "sql",
                json!([{ "line": 9, "updates": ["c_orderline"],
                         "update_sets": { "c_orderline": ["updated", "qty"] } }]),
            );
        method.id = KirId(Uuid::new_v4());
        let mut po = KirRelationship::deterministic(
            RelationshipKind::Custom("WritesTo".into()),
            method.id,
            model.id,
            "po",
        );
        po.properties.insert("via".into(), json!("po"));
        po.properties.insert("lines".into(), json!([12]));
        let paths = HashMap::from([(method.id, "base/src/MOrderLine.java".to_string())]);
        let (g, _) = derive(&[model, method], &[po], &paths);
        let (update, delete, _) = classes(&g, "C_OrderLine");
        assert_eq!(update, RAW_SETS_UPDATED);
        assert_eq!(delete, [DELETED_BY_APP]);
        let ev = g.objects[0].properties["citations"].as_array().unwrap();
        assert!(ev.iter().any(|e| e["kind"] == "po_delete"
            && e["path"] == "base/src/MOrderLine.java"
            && e["method"] == "org.compiere.model.MOrderLine.drop"));
    }
}
