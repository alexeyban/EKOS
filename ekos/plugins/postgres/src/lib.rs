//! RFC 0235 — the live PostgreSQL catalog observer.
//!
//! One `[[databases]]` entry of kind `postgres` becomes one observer. A scan connects through
//! `ekos-pg-live` (read-only session, statement and lock timeouts), reads the listed schemas'
//! catalog, reconciles it against the server's own counts (a short catalog is an error, RFC 0157),
//! probes whether the role can write, and emits RFC 0211's artifact shape
//! (`ekos_observation_sdk::live_catalog`): one `catalog` artifact per schema and one `object`
//! artifact per table, constraint, view, routine and trigger, their text already redacted by
//! `pg-live`. The driver is synchronous, so the read runs on a blocking thread.

use async_trait::async_trait;
use ekos_common::redaction::RedactionConfig;
use ekos_observation_sdk::live_catalog::{self, LiveObject, OwnerCatalog};
use ekos_observation_sdk::{ObservationPackage, ObserveError, Observer, ScanContext};
use ekos_pg_live::{CatalogObject, CatalogSnapshot, ObjectKind, PgError, PgSource, SessionPolicy};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod probe;
pub mod profile;

pub use ekos_observation_sdk::profile::ProfileTier;

/// The observer's name, and the `source_kind` its artifacts carry.
pub const OBSERVER: &str = "postgres";
/// The target scheme: `postgres://<database>/<schema>/…`.
pub const SCHEME: &str = "postgres";

/// One database's connection: the password stays an environment variable's name.
#[derive(Debug, Clone)]
pub struct PgConnection {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password_env: String,
}

impl PgConnection {
    /// `host[:port]/database` (RFC 0235 §2); the port defaults to 5432.
    pub fn parse(connect: &str, user: &str, password_env: &str) -> Result<Self, String> {
        let (address, database) = connect
            .split_once('/')
            .filter(|(a, d)| !a.is_empty() && !d.is_empty() && !d.contains('/'))
            .ok_or_else(|| format!("connect is host[:port]/database, not {connect:?}"))?;
        let (host, port) = match address.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| format!("connect {connect:?}: {p:?} is not a port"))?,
            ),
            None => (address, 5432),
        };
        Ok(Self {
            host: host.to_string(),
            port,
            database: database.to_string(),
            user: user.to_string(),
            password_env: password_env.to_string(),
        })
    }

    pub fn label(&self) -> String {
        format!(
            "postgres:{}@{}:{}/{}",
            self.user, self.host, self.port, self.database
        )
    }

    pub(crate) fn connect(&self, run_id: &str) -> Result<PgSource, PgError> {
        let conn = ekos_migrate::ConnectionRef {
            engine: ekos_migrate::EngineKind::Postgres,
            alias: self.host.clone(),
            database: self.database.clone(),
            secret_env: Some(self.password_env.clone()),
            secret_file: None,
        };
        PgSource::connect(
            &conn,
            &self.host,
            self.port,
            &self.user,
            run_id,
            &SessionPolicy::default(),
        )
    }
}

/// What one scan read.
#[derive(Debug, Clone)]
pub struct DatabaseReading {
    pub catalogs: Vec<OwnerCatalog>,
    pub write_check: Option<Value>,
}

/// Where a reading comes from: the live database, or a fixed reading in tests.
pub trait CatalogReader: Send + Sync {
    fn read(&self) -> Result<DatabaseReading, PgError>;
}

/// Reads a live database through `ekos-pg-live`.
pub struct LiveReader {
    pub connection: PgConnection,
    pub schemas: Vec<String>,
    pub run_id: String,
    pub redaction: RedactionConfig,
    /// RFC 0211 §3.4: how much the read measures beyond the planner's statistics.
    pub profile: ProfileTier,
}

impl CatalogReader for LiveReader {
    fn read(&self) -> Result<DatabaseReading, PgError> {
        read(
            &self.connection,
            &self.schemas,
            &self.run_id,
            &self.redaction,
            self.profile,
        )
    }
}

/// Connect, read and reconcile the schemas' catalog, profile it at `tier`, and probe a write.
/// Blocking.
pub fn read(
    connection: &PgConnection,
    schemas: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
    tier: ProfileTier,
) -> Result<DatabaseReading, PgError> {
    let source = connection.connect(run_id)?;
    let snapshot = ekos_pg_live::introspect(&source, schemas, redaction)?;
    ekos_pg_live::reconcile(&source, &snapshot, schemas)?;
    let mut catalogs = normalize(&snapshot, schemas);
    for c in &mut catalogs {
        let started = std::time::Instant::now();
        let mut run = |sql: &str| {
            source
                .preview(sql, usize::MAX)
                .map(|p| p.rows)
                .map_err(|e| e.to_string())
        };
        profile::profile_owner(c, tier, redaction, &mut run).map_err(|message| PgError::Query {
            sql: format!("statistics of schema {}", c.owner),
            message,
        })?;
        let mut line = json!({ "owner": c.owner, "tier": tier.as_str() });
        if let Some(p) = &c.profile {
            line["counts"] = serde_json::to_value(p).unwrap_or_default();
        }
        line["ms"] = (started.elapsed().as_millis() as u64).into();
        ekos_common::run_log::emit("postgres.profile", "profiled", &line);
    }
    let write_check = live_catalog::write_check(match source.check_write_refused()? {
        ekos_pg_live::WriteCheck::RefusedByServer { message } => Err(message),
        ekos_pg_live::WriteCheck::RoleCanWrite => Ok(()),
    });
    Ok(DatabaseReading {
        catalogs,
        write_check: Some(write_check),
    })
}

/// The census type a catalog kind is counted under, or `None` for one not counted per schema.
fn census_type(kind: ObjectKind) -> Option<&'static str> {
    Some(match kind {
        ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::Partition => "TABLE",
        ObjectKind::View | ObjectKind::MaterializedView => "VIEW",
        ObjectKind::Sequence => "SEQUENCE",
        ObjectKind::Index => "INDEX",
        ObjectKind::Function => "FUNCTION",
        ObjectKind::Procedure => "PROCEDURE",
        ObjectKind::Trigger => "TRIGGER",
        ObjectKind::PrimaryKey
        | ObjectKind::ForeignKey
        | ObjectKind::UniqueConstraint
        | ObjectKind::CheckConstraint
        | ObjectKind::ExclusionConstraint => "CONSTRAINT",
        ObjectKind::Enum
        | ObjectKind::Domain
        | ObjectKind::CompositeType
        | ObjectKind::RangeType => "TYPE",
        ObjectKind::RlsPolicy => "POLICY",
        ObjectKind::Schema | ObjectKind::Column | ObjectKind::Extension => return None,
    })
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A check constraint's expression from `pg_get_constraintdef`: `CHECK (<expr>)[ NOT VALID]`.
fn check_expression(definition: &str) -> String {
    let d = definition.trim();
    let d = d.strip_suffix("NOT VALID").map_or(d, str::trim_end);
    let d = d.strip_suffix("NO INHERIT").map_or(d, str::trim_end);
    d.strip_prefix("CHECK")
        .map(str::trim)
        .and_then(|d| d.strip_prefix('(').and_then(|d| d.strip_suffix(')')))
        .unwrap_or(d)
        .to_string()
}

/// `ON DELETE <action>` from a foreign key's definition; PostgreSQL omits the default.
fn on_delete(definition: &str) -> &'static str {
    ["CASCADE", "SET NULL", "SET DEFAULT", "RESTRICT"]
        .into_iter()
        .find(|a| definition.contains(&format!("ON DELETE {a}")))
        .unwrap_or("NO ACTION")
}

/// The snapshot as RFC 0211's per-owner objects. Pure. A schema listed but empty still gets a
/// catalog, so the census says "nothing there" rather than nothing at all.
pub fn normalize(snapshot: &CatalogSnapshot, schemas: &[String]) -> Vec<OwnerCatalog> {
    let mut owners: BTreeMap<String, OwnerCatalog> = BTreeMap::new();
    for s in schemas {
        owners.entry(s.clone()).or_insert_with(|| OwnerCatalog {
            owner: s.clone(),
            family: "pg_catalog".into(),
            ..Default::default()
        });
    }
    // Columns by (schema, relation), in attribute order.
    let mut columns: BTreeMap<(String, String), Vec<&CatalogObject>> = BTreeMap::new();
    for o in snapshot.of_kind(ObjectKind::Column) {
        if let Some(schema) = &o.schema {
            columns
                .entry((schema.clone(), text(&o.detail, "relation")))
                .or_default()
                .push(o);
        }
    }
    for o in &snapshot.objects {
        let Some(schema) = o.schema.clone() else {
            continue;
        };
        let c = owners
            .entry(schema.clone())
            .or_insert_with(|| OwnerCatalog {
                owner: schema.clone(),
                family: "pg_catalog".into(),
                ..Default::default()
            });
        if o.kind == ObjectKind::Column {
            c.columns += 1;
            continue;
        }
        if let Some(t) = census_type(o.kind) {
            c.census.entry(t.to_string()).or_default().valid += 1;
        }
        if let Some(object) = live_object(o, &schema, &columns) {
            c.objects.push(object);
        }
    }
    owners.into_values().collect()
}

fn live_object(
    o: &CatalogObject,
    schema: &str,
    columns: &BTreeMap<(String, String), Vec<&CatalogObject>>,
) -> Option<LiveObject> {
    let d = &o.detail;
    let make = |kind: &str, name: String, detail: Value| LiveObject {
        kind: kind.to_string(),
        owner: schema.to_string(),
        name,
        status: None,
        detail,
    };
    Some(match o.kind {
        ObjectKind::Table | ObjectKind::PartitionedTable | ObjectKind::Partition => {
            let name = text(d, "name");
            let cols: Vec<Value> = columns
                .get(&(schema.to_string(), name.clone()))
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .map(|c| {
                    let mut col = json!({
                        "name": text(&c.detail, "name"),
                        "data_type": text(&c.detail, "type"),
                        "nullable": c.detail["not_null"] != true,
                    });
                    for k in ["default", "comment", "identity", "generated"] {
                        if let Some(v) = c.detail.get(k) {
                            col[k] = v.clone();
                        }
                    }
                    col
                })
                .collect();
            let mut detail = json!({ "columns": cols });
            if let Some(v) = d.get("comment") {
                detail["comment"] = v.clone();
            }
            match o.kind {
                ObjectKind::PartitionedTable => detail["partitioned"] = true.into(),
                ObjectKind::Partition => detail["partition"] = true.into(),
                _ => {}
            }
            make("table", name, detail)
        }
        ObjectKind::View | ObjectKind::MaterializedView => {
            let mut detail = json!({ "text": text(d, "definition") });
            if o.kind == ObjectKind::MaterializedView {
                detail["materialized"] = true.into();
            }
            if let Some(v) = d.get("comment") {
                detail["comment"] = v.clone();
            }
            make("view", text(d, "name"), detail)
        }
        ObjectKind::PrimaryKey | ObjectKind::UniqueConstraint | ObjectKind::CheckConstraint => {
            let table = text(d, "table");
            let kind = match o.kind {
                ObjectKind::PrimaryKey => "primary_key",
                ObjectKind::UniqueConstraint => "unique_constraint",
                _ => "check_constraint",
            };
            let mut detail = json!({
                "table": table,
                "columns": d.get("columns").cloned().unwrap_or(json!([])),
                "validated": d["validated"],
            });
            if o.kind == ObjectKind::CheckConstraint {
                detail["condition"] = check_expression(&text(d, "definition")).into();
            }
            make(kind, format!("{table}.{}", text(d, "name")), detail)
        }
        ObjectKind::ForeignKey => {
            let table = text(d, "table");
            let definition = text(d, "definition");
            make(
                "foreign_key",
                format!("{table}.{}", text(d, "name")),
                json!({
                    "table": table,
                    "columns": d.get("columns").cloned().unwrap_or(json!([])),
                    "references_owner": d.get("references_schema").cloned().unwrap_or(json!(schema)),
                    "references_table": d.get("references_table").cloned().unwrap_or(Value::Null),
                    "references_columns": d.get("references_columns").cloned().unwrap_or(json!([])),
                    "validated": d["validated"],
                    "on_delete": on_delete(&definition),
                }),
            )
        }
        ObjectKind::Function | ObjectKind::Procedure => make(
            if o.kind == ObjectKind::Function {
                "function"
            } else {
                "procedure"
            },
            format!("{}({})", text(d, "name"), text(d, "arguments")),
            json!({ "text": text(d, "definition"), "language": d["language"] }),
        ),
        ObjectKind::Trigger => {
            let table = text(d, "table");
            make(
                "trigger",
                format!("{table}.{}", text(d, "name")),
                json!({ "table": table, "text": text(d, "definition") }),
            )
        }
        _ => return None,
    })
}

/// RFC 0223 / 0235: what `ekos database test` reports — the same JSON for every kind.
pub fn test_connection(
    connection: &PgConnection,
    schemas: &[String],
    run_id: &str,
    redaction: &RedactionConfig,
) -> Result<Value, PgError> {
    let reading = read(connection, schemas, run_id, redaction, ProfileTier::P0)?;
    Ok(test_report(&connection.label(), &reading))
}

/// The test report of one reading. Pure.
pub fn test_report(label: &str, reading: &DatabaseReading) -> Value {
    json!({
        "label": label,
        "family": "pg_catalog",
        "owners": reading.catalogs.iter().map(|c| json!({
            "owner": c.owner,
            "census": c.census,
            "listed": c.listed(),
            "columns": c.columns,
        })).collect::<Vec<_>>(),
        "write_check": reading.write_check,
        "any_privileges": [],
    })
}

/// One `[[databases]]` entry of kind `postgres`.
pub struct PostgresObserver {
    database: String,
    reader: Arc<dyn CatalogReader>,
}

impl PostgresObserver {
    pub fn new(database: impl Into<String>, reader: Arc<dyn CatalogReader>) -> Self {
        Self {
            database: database.into(),
            reader,
        }
    }
}

#[async_trait]
impl Observer for PostgresObserver {
    fn name(&self) -> &str {
        OBSERVER
    }

    async fn scan(&self, _ctx: &ScanContext) -> Result<ObservationPackage, ObserveError> {
        let reader = Arc::clone(&self.reader);
        let reading = tokio::task::spawn_blocking(move || reader.read())
            .await
            .map_err(|e| ObserveError::connector(format!("postgres reader stopped: {e}")))?
            .map_err(|e| {
                ObserveError::connector(format!("postgres database {}: {e}", self.database))
            })?;
        let mut pkg = ObservationPackage::new(OBSERVER, "database");
        for a in live_catalog::artifacts(
            OBSERVER,
            "ekos-plugin-postgres",
            SCHEME,
            &self.database,
            &reading.catalogs,
            reading.write_check.as_ref(),
        ) {
            pkg.push(a);
        }
        let objects: usize = reading.catalogs.iter().map(|c| c.objects.len()).sum();
        tracing::info!(database = %self.database, schemas = reading.catalogs.len(), objects, "postgres catalog read");
        Ok(pkg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(kind: ObjectKind, qualified: &str, detail: Value) -> CatalogObject {
        CatalogObject {
            kind,
            qualified_name: qualified.into(),
            schema: Some("shop".into()),
            detail,
        }
    }

    /// What `ekos_pg_live::introspect` returns for a small schema.
    fn snapshot() -> CatalogSnapshot {
        CatalogSnapshot {
            objects: vec![
                CatalogObject {
                    kind: ObjectKind::Schema,
                    qualified_name: "shop".into(),
                    schema: None,
                    detail: json!({}),
                },
                obj(
                    ObjectKind::Table,
                    "shop.customers",
                    json!({ "name": "customers" }),
                ),
                obj(
                    ObjectKind::Table,
                    "shop.orders",
                    json!({ "name": "orders", "comment": "One order" }),
                ),
                obj(
                    ObjectKind::View,
                    "shop.big_orders",
                    json!({ "name": "big_orders", "definition": " SELECT id FROM shop.orders WHERE total > 100;" }),
                ),
                obj(
                    ObjectKind::Column,
                    "shop.customers.id",
                    json!({ "relation": "customers", "name": "id", "type": "integer", "not_null": true }),
                ),
                obj(
                    ObjectKind::Column,
                    "shop.orders.id",
                    json!({ "relation": "orders", "name": "id", "type": "integer", "not_null": true }),
                ),
                obj(
                    ObjectKind::Column,
                    "shop.orders.customer_id",
                    json!({ "relation": "orders", "name": "customer_id", "type": "integer", "not_null": false, "comment": "Who ordered" }),
                ),
                obj(
                    ObjectKind::Column,
                    "shop.orders.total",
                    json!({ "relation": "orders", "name": "total", "type": "numeric(10,2)", "not_null": true, "default": "0" }),
                ),
                obj(
                    ObjectKind::PrimaryKey,
                    "shop.orders.orders_pkey",
                    json!({ "table": "orders", "name": "orders_pkey", "validated": true, "definition": "PRIMARY KEY (id)", "columns": ["id"] }),
                ),
                obj(
                    ObjectKind::CheckConstraint,
                    "shop.orders.orders_total_check",
                    json!({ "table": "orders", "name": "orders_total_check", "validated": true, "definition": "CHECK (total >= 0::numeric)", "columns": ["total"] }),
                ),
                obj(
                    ObjectKind::ForeignKey,
                    "shop.orders.orders_customer_fk",
                    json!({ "table": "orders", "name": "orders_customer_fk", "validated": false, "definition": "FOREIGN KEY (customer_id) REFERENCES shop.customers(id) ON DELETE CASCADE NOT VALID", "columns": ["customer_id"], "references_schema": "shop", "references_table": "customers", "references_columns": ["id"] }),
                ),
                obj(
                    ObjectKind::Index,
                    "shop.orders_total_idx",
                    json!({ "on": "shop.orders" }),
                ),
                obj(
                    ObjectKind::Function,
                    "shop.order_total(integer)",
                    json!({ "name": "order_total", "arguments": "integer", "language": "plpgsql", "body": "...", "definition": "CREATE OR REPLACE FUNCTION shop.order_total(p integer)\n RETURNS numeric\n LANGUAGE plpgsql\nAS $function$\nBEGIN RETURN (SELECT total FROM shop.orders WHERE id = p); END;\n$function$\n" }),
                ),
                obj(
                    ObjectKind::Trigger,
                    "shop.orders.orders_audit",
                    json!({ "table": "orders", "name": "orders_audit", "definition": "CREATE TRIGGER orders_audit AFTER INSERT ON shop.orders FOR EACH ROW EXECUTE FUNCTION shop.audit()" }),
                ),
            ],
        }
    }

    #[test]
    fn columns_fold_into_their_table_and_constraints_keep_their_table() {
        let c = normalize(&snapshot(), &["shop".into()]);
        assert_eq!(c.len(), 1);
        let c = &c[0];
        assert_eq!((c.owner.as_str(), c.columns), ("shop", 4));
        assert_eq!(c.census["TABLE"].valid, 2);
        assert_eq!(c.census["INDEX"].valid, 1);
        assert_eq!(c.census["CONSTRAINT"].valid, 3);
        let orders = c.objects.iter().find(|o| o.name == "orders").unwrap();
        let cols = orders.detail["columns"].as_array().unwrap();
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[1]["nullable"], true);
        assert_eq!(cols[1]["comment"], "Who ordered");
        assert_eq!(cols[2]["default"], "0");
        assert_eq!(orders.detail["comment"], "One order");
        let check = c
            .objects
            .iter()
            .find(|o| o.kind == "check_constraint")
            .unwrap();
        assert_eq!(check.name, "orders.orders_total_check");
        assert_eq!(check.detail["condition"], "total >= 0::numeric");
        let fk = c.objects.iter().find(|o| o.kind == "foreign_key").unwrap();
        assert_eq!(fk.detail["references_table"], "customers");
        assert_eq!(fk.detail["on_delete"], "CASCADE");
        assert_eq!(fk.detail["validated"], false);
        let f = c.objects.iter().find(|o| o.kind == "function").unwrap();
        assert_eq!(f.name, "order_total(integer)");
        assert!(
            f.detail["text"]
                .as_str()
                .unwrap()
                .starts_with("CREATE OR REPLACE FUNCTION")
        );
        assert!(
            !c.objects.iter().any(|o| o.kind == "index"),
            "counted, not emitted"
        );
    }

    #[test]
    fn a_listed_schema_with_nothing_in_it_still_has_a_catalog() {
        let c = normalize(&CatalogSnapshot::default(), &["empty".into()]);
        assert_eq!(c.len(), 1);
        assert!(c[0].census.is_empty() && c[0].objects.is_empty());
    }

    #[test]
    fn check_expressions_lose_their_wrapper() {
        assert_eq!(check_expression("CHECK ((a > 0))"), "(a > 0)");
        assert_eq!(check_expression("CHECK (a > 0) NOT VALID"), "a > 0");
        assert_eq!(on_delete("FOREIGN KEY (a) REFERENCES b(c)"), "NO ACTION");
        assert_eq!(
            on_delete("FOREIGN KEY (a) REFERENCES b(c) ON DELETE SET NULL"),
            "SET NULL"
        );
    }

    #[test]
    fn connect_is_host_port_database() {
        let c = PgConnection::parse("db.local:6543/shop", "ro", "PW").unwrap();
        assert_eq!(
            (c.host.as_str(), c.port, c.database.as_str()),
            ("db.local", 6543, "shop")
        );
        assert_eq!(
            PgConnection::parse("db.local/shop", "ro", "PW")
                .unwrap()
                .port,
            5432
        );
        for bad in ["db.local", "db.local:x/shop", "/shop", "h/a/b"] {
            assert!(PgConnection::parse(bad, "ro", "PW").is_err(), "{bad}");
        }
    }

    struct Fixed(DatabaseReading);
    impl CatalogReader for Fixed {
        fn read(&self) -> Result<DatabaseReading, PgError> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn a_scan_emits_the_live_catalog_shape() {
        let reading = DatabaseReading {
            catalogs: normalize(&snapshot(), &["shop".into()]),
            write_check: Some(live_catalog::write_check(Err("permission denied".into()))),
        };
        let pkg = PostgresObserver::new("shop", Arc::new(Fixed(reading.clone())))
            .scan(&ScanContext::new("."))
            .await
            .unwrap();
        assert_eq!(pkg.artifacts[0].content.target, "postgres://shop/shop");
        assert!(
            pkg.artifacts
                .iter()
                .any(|a| a.content.target == "postgres://shop/shop/table/orders")
        );
        let report = test_report("postgres:ro@h:5432/shop", &reading);
        assert_eq!(report["owners"][0]["census"]["TABLE"]["valid"], 2);
        assert_eq!(report["write_check"]["result"], "refused_by_server");
    }
}
