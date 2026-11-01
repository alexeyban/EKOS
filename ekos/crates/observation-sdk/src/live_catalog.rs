//! RFC 0211 / 0235 — the artifact shape of a live database catalog read.
//!
//! Every `[[databases]]` observer (Oracle, PostgreSQL, ClickHouse) emits the same artifacts, so one
//! analyzer (`recovery::oracle_catalog_analyzer`, parameterised by dialect) reads them all:
//!
//! - one `catalog` artifact per owner (schema or database): the census, what was listed, the write
//!   probe — the facts a reviewer needs to trust the rest — and the list of objects read, so an
//!   object dropped from the database leaves the compiled model;
//! - one `object` artifact per catalog object: `{kind, owner, name, status, detail}`.
//!
//! Targets are `<scheme>://<database>/<owner>` and `…/<kind>/<name>`: stable, and never a file
//! path, so RFC 0043's excluded-file globs cannot match one by accident.

use ekos_artifact::ObservationArtifact;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Counts of one catalog object type, by status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Census {
    pub valid: usize,
    pub invalid: usize,
}

/// One catalog object, as the analyzer reads it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveObject {
    /// `table`, `view`, `primary_key`, `unique_constraint`, `check_constraint`, `foreign_key`,
    /// `function`, `procedure`, `trigger`.
    pub kind: String,
    pub owner: String,
    /// Unique within (owner, kind): an overloaded routine carries its argument list.
    pub name: String,
    pub status: Option<String>,
    pub detail: Value,
}

/// Everything one owner's catalog read found.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OwnerCatalog {
    pub owner: String,
    /// Which catalog views were read (`pg_catalog`, `system`, …).
    pub family: String,
    /// Every catalog object type the server counts, upper-case (`TABLE`, `VIEW`, `FUNCTION`, …).
    pub census: BTreeMap<String, Census>,
    pub columns: usize,
    pub objects: Vec<LiveObject>,
}

impl OwnerCatalog {
    /// Emitted objects by kind.
    pub fn listed(&self) -> BTreeMap<String, usize> {
        let mut m = BTreeMap::new();
        for o in &self.objects {
            *m.entry(o.kind.clone()).or_insert(0) += 1;
        }
        m
    }

    /// `<kind>/<name>` of every object, sorted: the tail of each object's target.
    pub fn entries(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .objects
            .iter()
            .map(|o| format!("{}/{}", o.kind, o.name))
            .collect();
        v.sort();
        v
    }
}

/// The write probe's result, in RFC 0211's shape (`{"result": "refused_by_server", "message"}` or
/// `{"result": "user_can_write"}`).
pub fn write_check(refused: Result<(), String>) -> Value {
    match refused {
        Err(message) => json!({ "result": "refused_by_server", "message": message }),
        Ok(()) => json!({ "result": "user_can_write" }),
    }
}

pub fn catalog_target(scheme: &str, database: &str, owner: &str) -> String {
    format!("{scheme}://{database}/{owner}")
}

pub fn object_target(scheme: &str, database: &str, o: &LiveObject) -> String {
    format!("{scheme}://{database}/{}/{}/{}", o.owner, o.kind, o.name)
}

/// The artifacts of one read. Pure.
pub fn artifacts(
    observer: &str,
    producer: &str,
    scheme: &str,
    database: &str,
    catalogs: &[OwnerCatalog],
    write_check: Option<&Value>,
) -> Vec<ObservationArtifact> {
    let mut out = Vec::new();
    for c in catalogs {
        let data = json!({
            "kind": "catalog",
            "database": database,
            "owner": c.owner,
            "family": c.family,
            "census": c.census,
            "listed": c.listed(),
            "columns": c.columns,
            "write_check": write_check,
            "objects": c.entries(),
        });
        out.push(
            ObservationArtifact::new(observer, catalog_target(scheme, database, &c.owner), data)
                .with_producer(producer),
        );
        for o in &c.objects {
            let data = json!({ "kind": "object", "database": database, "object": o });
            out.push(
                ObservationArtifact::new(observer, object_target(scheme, database, o), data)
                    .with_producer(producer),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_catalog_artifact_per_owner_then_one_per_object() {
        let c = OwnerCatalog {
            owner: "public".into(),
            family: "pg_catalog".into(),
            census: [(
                "TABLE".to_string(),
                Census {
                    valid: 1,
                    invalid: 0,
                },
            )]
            .into(),
            columns: 2,
            objects: vec![LiveObject {
                kind: "table".into(),
                owner: "public".into(),
                name: "orders".into(),
                status: None,
                detail: json!({ "columns": [] }),
            }],
        };
        let wc = write_check(Err("permission denied".into()));
        let a = artifacts("postgres", "p", "postgres", "shop", &[c], Some(&wc));
        let targets: Vec<&str> = a.iter().map(|a| a.content.target.as_str()).collect();
        assert_eq!(
            targets,
            [
                "postgres://shop/public",
                "postgres://shop/public/table/orders"
            ]
        );
        let d = &a[0].content.data;
        assert_eq!(d["write_check"]["result"], "refused_by_server");
        assert_eq!(d["listed"]["table"], 1);
        assert_eq!(d["objects"][0], "table/orders");
        assert_eq!(a[1].content.data["object"]["name"], "orders");
        assert_eq!(write_check(Ok(()))["result"], "user_can_write");
    }
}
