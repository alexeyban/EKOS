//! RFC 0245: Apache Ossie models ingested from a project's sources, linked to what EKOS compiled.
//!
//! `recovery`'s `OssieAnalyzerPass` turns each Ossie file into `OssieModel` / `OssieDataset` /
//! `OssieMetric` objects. At `ekos commit`:
//!
//! - [`link`] resolves each dataset's `source` to the one `Table` or `View` of that name — the
//!   full name first, then its last dotted part, by [`NameIndex`]'s unique-name rule; a miss or
//!   an ambiguous name links nothing — with a deterministic `DependsOn` edge (`link:
//!   "ossie_source"`).
//! - [`conflicts`] reports where the Ossie file and the schema disagree, as RFC 0172
//!   `ConflictingEvidence` about the table: a field whose expression is a plain column the table
//!   does not have (only for tables whose column list is complete — DDL or a live catalog, never
//!   a dbt model's partial documentation), and a declared `datatype` from another family than the
//!   column's SQL type (`Integer`/`Decimal`/`Float` are one family; so are `DateTime`/`DateTimeTz`).

use crate::conflicts::{Claim, conflict};
use crate::procedure_lineage::{NameIndex, is_live};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use ekos_ossie::DataType;
use ekos_ossie::build::plain_column;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub use ekos_ossie::kinds::{LINK, OSSIE_DATASET, OSSIE_METRIC, OSSIE_MODEL};

/// Whether `o` belongs to the Ossie files the latest `recover` found. The ledger is append-only,
/// so a deleted file's objects stay in it; `files` (the `ossie/current.json` manifest, `None`
/// before one exists) is how every reader leaves them out. Non-Ossie objects always pass.
pub fn is_current(o: &KirObject, files: Option<&BTreeSet<String>>) -> bool {
    let ossie = [OSSIE_MODEL, OSSIE_DATASET, OSSIE_METRIC]
        .iter()
        .any(|k| is_custom(o, k));
    !ossie || files.is_none_or(|f| f.contains(text(o, "path")))
}

/// The conflict type of an Ossie-vs-schema disagreement.
pub const OSSIE_VS_SCHEMA: &str = "ossie_vs_schema";

/// What [`link`] resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OssieLinkStats {
    pub datasets: usize,
    pub linked: usize,
    pub unresolved: usize,
    pub ambiguous: usize,
}

fn is_custom(o: &KirObject, kind: &str) -> bool {
    matches!(&o.kind, ObjectKind::Custom(k) if k == kind)
}

fn text<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

enum Resolved {
    Found(KirId, &'static str),
    Ambiguous,
    Missing,
}

/// The table a dataset's `source` names: the whole name, then its last part alone.
fn resolve(index: &NameIndex, source: &str) -> Resolved {
    let source = source.replace(['"', '`', '[', ']'], "");
    match index.resolve(&source) {
        Ok((id, how)) => return Resolved::Found(id, how),
        Err(true) => return Resolved::Ambiguous,
        Err(false) => {}
    }
    let tail = source.rsplit('.').next().unwrap_or(&source);
    if tail == source {
        return Resolved::Missing;
    }
    match index.resolve(tail) {
        Ok((id, _)) => Resolved::Found(id, "unqualified"),
        Err(true) => Resolved::Ambiguous,
        Err(false) => Resolved::Missing,
    }
}

fn table_index(objects: &[KirObject]) -> NameIndex {
    let mut index = NameIndex::default();
    for o in objects {
        if o.kind == ObjectKind::Table || is_custom(o, "View") {
            index.add_object(o);
        }
    }
    index
}

/// Every ingested dataset with the table it resolves to.
fn resolved_pairs<'a>(
    objects: &'a [KirObject],
    files: Option<&BTreeSet<String>>,
) -> (Vec<(&'a KirObject, KirId, &'static str)>, OssieLinkStats) {
    let index = table_index(objects);
    let mut stats = OssieLinkStats::default();
    let mut pairs = Vec::new();
    for d in objects
        .iter()
        .filter(|o| is_custom(o, OSSIE_DATASET) && is_current(o, files))
    {
        stats.datasets += 1;
        match resolve(&index, text(d, "source")) {
            Resolved::Found(id, how) => {
                stats.linked += 1;
                pairs.push((d, id, how));
            }
            Resolved::Ambiguous => stats.ambiguous += 1,
            Resolved::Missing => stats.unresolved += 1,
        }
    }
    pairs.sort_by_key(|(d, t, _)| (d.id.to_string(), t.to_string()));
    (pairs, stats)
}

/// `DependsOn` from every current ingested dataset ([`is_current`]) to the one table its `source`
/// names. Deterministic ids: re-linking unchanged input appends nothing.
pub fn link(graph: &mut KirGraph, files: Option<&BTreeSet<String>>) -> OssieLinkStats {
    let (pairs, stats) = resolved_pairs(&graph.objects, files);
    let edges: Vec<KirRelationship> = pairs
        .into_iter()
        .map(|(d, table, how)| {
            let mut rel =
                KirRelationship::deterministic(RelationshipKind::DependsOn, d.id, table, LINK);
            rel.properties.insert("link".into(), json!(LINK));
            rel.properties.insert("match".into(), json!(how));
            rel.evidence = d.evidence.clone();
            rel
        })
        .collect();
    graph.relationships.extend(edges);
    stats
}

/// A table's columns: name (case-folded) → SQL type.
fn columns(t: &KirObject) -> BTreeMap<String, String> {
    t.properties
        .get("columns")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| match c {
                    Value::String(n) => Some((n.to_lowercase(), String::new())),
                    Value::Object(m) => Some((
                        m.get("name")?.as_str()?.to_lowercase(),
                        m.get("data_type")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether a table's `columns` list every column it has: read from DDL (a stated dialect) or a
/// live catalog. A dbt model documents only some.
fn columns_complete(t: &KirObject) -> bool {
    is_live(t) || (!text(t, "dialect").is_empty() && !t.properties.contains_key("dbt_project"))
}

fn family(t: DataType) -> Option<&'static str> {
    match t {
        DataType::Integer | DataType::Decimal | DataType::Float => Some("number"),
        DataType::DateTime | DataType::DateTimeTz => Some("datetime"),
        DataType::Date => Some("date"),
        DataType::Time => Some("time"),
        DataType::String => Some("string"),
        DataType::Boolean => Some("boolean"),
        DataType::Opaque => None,
    }
}

/// Whether two data types disagree. `Boolean` beside a number is not a dispute: MySQL's
/// `tinyint(1)` and Oracle's `NUMBER(1)` are how those engines spell a flag.
fn types_disagree(ossie: DataType, column: DataType) -> bool {
    let (Some(a), Some(b)) = (family(ossie), family(column)) else {
        return false;
    };
    let flag = |x, y| x == "boolean" && y == "number";
    a != b && !flag(a, b) && !flag(b, a)
}

/// The (path, line) an evidence id records, if the caller can look it up.
pub type EvidenceLookup<'a> = &'a dyn Fn(&KirId) -> Option<(String, Option<u32>)>;

/// Where a table is stated: its first evidence's file.
fn table_path(t: &KirObject, evidence: EvidenceLookup) -> (String, Option<u32>) {
    t.evidence
        .iter()
        .find_map(|e| evidence(e).filter(|(p, _)| !p.is_empty()))
        .unwrap_or_else(|| (text(t, "source_path").to_string(), None))
}

/// The disagreements between ingested Ossie datasets and the tables they resolve to, as
/// `ConflictingEvidence` graphs. `evidence` looks up an evidence id's (path, line), for the
/// table's side of a claim; it is called only for linked tables.
pub fn conflicts(
    objects: &[KirObject],
    files: Option<&BTreeSet<String>>,
    evidence: EvidenceLookup,
) -> KirGraph {
    let by_id: HashMap<KirId, &KirObject> = objects.iter().map(|o| (o.id, o)).collect();
    let (pairs, _) = resolved_pairs(objects, files);
    let mut out = KirGraph::new();
    for (d, table_id, _) in pairs {
        let Some(table) = by_id.get(&table_id) else {
            continue;
        };
        let cols = columns(table);
        if cols.is_empty() {
            continue;
        }
        let complete = columns_complete(table);
        let (tpath, tline) = table_path(table, evidence);
        let dpath = text(d, "path").to_string();
        let dline = d
            .properties
            .get("line")
            .and_then(Value::as_u64)
            .map(|l| l as u32);
        let fields = d.properties.get("fields").and_then(Value::as_array);
        for f in fields.into_iter().flatten() {
            let Some(col) = f.get("sql").and_then(Value::as_str).and_then(plain_column) else {
                continue;
            };
            let ossie_side = |value: Value| Claim {
                value,
                path: dpath.clone(),
                line: dline,
                source: OSSIE_DATASET.into(),
            };
            let table_side = |value: Value| Claim {
                value,
                path: tpath.clone(),
                line: tline,
                source: "Table".into(),
            };
            match cols.get(&col.to_lowercase()) {
                None if complete => {
                    let g = conflict(
                        table,
                        &format!("columns.{col}"),
                        OSSIE_VS_SCHEMA,
                        vec![ossie_side(json!("present")), table_side(json!("absent"))],
                        None,
                    );
                    merge(&mut out, g);
                }
                None => {}
                Some(sql_type) => {
                    let declared = f
                        .get("datatype")
                        .and_then(|v| serde_json::from_value::<DataType>(v.clone()).ok());
                    let actual = ekos_ossie::datatype::from_sql(sql_type);
                    if let (Some(declared), Some(actual)) = (declared, actual)
                        && types_disagree(declared, actual)
                    {
                        let g = conflict(
                            table,
                            &format!("columns.{col}.datatype"),
                            OSSIE_VS_SCHEMA,
                            vec![
                                ossie_side(serde_json::to_value(declared).unwrap_or_default()),
                                table_side(json!(sql_type)),
                            ],
                            None,
                        );
                        merge(&mut out, g);
                    }
                }
            }
        }
    }
    out
}

fn merge(out: &mut KirGraph, g: KirGraph) {
    for o in g.objects {
        if !out.objects.iter().any(|x| x.id == o.id) {
            out.objects.push(o);
        }
    }
    for r in g.relationships {
        if !out.relationships.iter().any(|x| x.id == r.id) {
            out.relationships.push(r);
        }
    }
    for e in g.evidence {
        if !out.evidence.iter().any(|x| x.id == e.id) {
            out.evidence.push(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(name: &str, kind: ObjectKind, props: Value) -> KirObject {
        let mut o = KirObject::new(name, kind);
        o.id = KirId(uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            format!("{:?}:{name}", o.kind).as_bytes(),
        ));
        o.properties = serde_json::from_value(props).unwrap();
        o
    }

    fn ds(name: &str, source: &str, fields: Value) -> KirObject {
        obj(
            name,
            ObjectKind::Custom(OSSIE_DATASET.into()),
            json!({"source": source, "path": "m.ossie.yaml", "line": 4, "fields": fields}),
        )
    }

    fn orders() -> KirObject {
        obj(
            "public.orders",
            ObjectKind::Table,
            json!({"dialect": "postgres", "columns": [
                {"name": "id", "data_type": "integer"},
                {"name": "status", "data_type": "char(1)"},
                {"name": "flag", "data_type": "tinyint(1)"},
                {"name": "total", "data_type": "numeric(12,2)"}]}),
        )
    }

    #[test]
    fn a_dataset_links_to_its_one_table_by_full_or_last_name() {
        let mut g = KirGraph::new();
        g.objects = vec![
            orders(),
            ds("orders", "warehouse.public.orders", json!([])),
            ds("o2", "\"public\".\"orders\"", json!([])),
            ds("nowhere", "db.s.nowhere", json!([])),
        ];
        let stats = link(&mut g, None);
        assert_eq!(
            stats,
            OssieLinkStats {
                datasets: 3,
                linked: 2,
                unresolved: 1,
                ambiguous: 0
            }
        );
        assert!(g.relationships.iter().all(|r| r.properties["link"] == LINK));
        // Deterministic: the same input links to the same ids.
        let mut again = KirGraph::new();
        again.objects = g.objects.clone();
        link(&mut again, None);
        let ids = |g: &KirGraph| g.relationships.iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(&g), ids(&again));
    }

    #[test]
    fn two_tables_of_one_name_link_nothing() {
        let mut g = KirGraph::new();
        let mut other = orders();
        other.name = "sales.orders".into();
        other.id = KirId(uuid::Uuid::new_v4());
        g.objects = vec![orders(), other, ds("orders", "orders", json!([]))];
        let stats = link(&mut g, None);
        assert_eq!((stats.linked, stats.ambiguous), (0, 1));
        assert!(g.relationships.is_empty());
    }

    #[test]
    fn disagreements_are_conflicts_and_compatible_types_are_not() {
        let objects = vec![
            orders(),
            ds(
                "orders",
                "orders",
                json!([
                    {"name": "id", "sql": "id", "datatype": "Decimal"},
                    {"name": "status", "sql": "status", "datatype": "Integer"},
                    {"name": "flag", "sql": "flag", "datatype": "Boolean"},
                    {"name": "gone", "sql": "\"gone\""},
                    {"name": "net", "sql": "total - discount", "datatype": "Decimal"},
                ]),
            ),
        ];
        let g = conflicts(&objects, None, &|_| None);
        let mut attrs: Vec<String> = g
            .objects
            .iter()
            .map(|o| o.properties["attribute"].as_str().unwrap().to_string())
            .collect();
        attrs.sort();
        assert_eq!(attrs, ["columns.gone", "columns.status.datatype"]);
        let c = g
            .objects
            .iter()
            .find(|o| o.properties["attribute"] == "columns.status.datatype")
            .unwrap();
        assert_eq!(c.properties["conflict_type"], OSSIE_VS_SCHEMA);
        assert_eq!(c.properties["claims"].as_array().unwrap().len(), 2);
        assert_eq!(g.relationships.len(), 2, "one Disputes link each");
    }

    #[test]
    fn a_dbt_model_s_partial_columns_never_report_a_missing_one() {
        let mut t = orders();
        t.properties.insert("dbt_project".into(), json!("shop"));
        let objects = vec![
            t,
            ds("orders", "orders", json!([{"name": "x", "sql": "x"}])),
        ];
        assert!(conflicts(&objects, None, &|_| None).objects.is_empty());
    }

    #[test]
    fn a_deleted_file_s_datasets_are_not_linked() {
        let mut g = KirGraph::new();
        g.objects = vec![orders(), ds("orders", "orders", json!([]))];
        let other: BTreeSet<String> = ["other.ossie.yaml".to_string()].into();
        assert_eq!(link(&mut g, Some(&other)).datasets, 0);
        let this: BTreeSet<String> = ["m.ossie.yaml".to_string()].into();
        assert_eq!(link(&mut g, Some(&this)).linked, 1);
        assert!(
            is_current(&orders(), Some(&other)),
            "a table is never filtered"
        );
    }

    #[test]
    fn the_kind_names_match_rfc_0170() {
        use crate::business_semantics::{CONCEPT, ENUM_MEANING};
        assert_eq!(ekos_ossie::kinds::CONCEPT, CONCEPT);
        assert_eq!(ekos_ossie::kinds::ENUM_MEANING, ENUM_MEANING);
    }
}
