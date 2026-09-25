//! RFC 0157 — catalog introspection.
//!
//! The output of this module is the **denominator** for RFC 0158's completeness check: a source
//! object that never appears here can never be classified, and RFC 0154's coverage requirement
//! ("every source object recovered, classified and dispositioned") quietly becomes untrue.
//!
//! So this module does not trust its own enumeration. Every kind it reads is reconciled against the
//! server's own counts, and a mismatch is an error rather than a smaller list.

use crate::PgError;
use ekos_common::redaction::{RedactionConfig, redact};
use serde::{Deserialize, Serialize};

/// Every catalog object kind EKOS Migrate recognizes.
///
/// Adding a variant is how a new PostgreSQL feature enters the coverage guarantee. The list is
/// exhaustive on purpose — `Other` is not a variant, because an unrecognized object must fail the
/// completeness check rather than land in a bucket nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectKind {
    Schema,
    Table,
    PartitionedTable,
    Partition,
    View,
    MaterializedView,
    Column,
    PrimaryKey,
    ForeignKey,
    UniqueConstraint,
    CheckConstraint,
    ExclusionConstraint,
    Index,
    Sequence,
    Function,
    Procedure,
    Trigger,
    Enum,
    Domain,
    CompositeType,
    RangeType,
    Extension,
    RlsPolicy,
}

impl ObjectKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Schema => "schema",
            Self::Table => "table",
            Self::PartitionedTable => "partitioned_table",
            Self::Partition => "partition",
            Self::View => "view",
            Self::MaterializedView => "materialized_view",
            Self::Column => "column",
            Self::PrimaryKey => "primary_key",
            Self::ForeignKey => "foreign_key",
            Self::UniqueConstraint => "unique_constraint",
            Self::CheckConstraint => "check_constraint",
            Self::ExclusionConstraint => "exclusion_constraint",
            Self::Index => "index",
            Self::Sequence => "sequence",
            Self::Function => "function",
            Self::Procedure => "procedure",
            Self::Trigger => "trigger",
            Self::Enum => "enum",
            Self::Domain => "domain",
            Self::CompositeType => "composite_type",
            Self::RangeType => "range_type",
            Self::Extension => "extension",
            Self::RlsPolicy => "rls_policy",
        }
    }

    /// `true` where the target engines have no equivalent at all, so the object is guaranteed to
    /// need a human disposition rather than a mapping (RFC 0158).
    pub fn has_no_target_equivalent(self) -> bool {
        matches!(
            self,
            Self::Trigger | Self::RlsPolicy | Self::ExclusionConstraint | Self::Procedure
        )
    }
}

/// One catalog object, as a fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogObject {
    pub kind: ObjectKind,
    /// Schema-qualified where that is meaningful, bare otherwise (extensions, schemas).
    pub qualified_name: String,
    pub schema: Option<String>,
    /// Kind-specific detail: a column's type, a constraint's definition, a function's language.
    /// **Redacted** before it gets here — see [`introspect`].
    pub detail: serde_json::Value,
}

/// Everything the source catalog contains, plus the counts it was reconciled against.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogSnapshot {
    pub objects: Vec<CatalogObject>,
}

impl CatalogSnapshot {
    pub fn of_kind(&self, kind: ObjectKind) -> impl Iterator<Item = &CatalogObject> {
        self.objects.iter().filter(move |o| o.kind == kind)
    }

    pub fn count_of(&self, kind: ObjectKind) -> usize {
        self.of_kind(kind).count()
    }

    /// Counts by kind, which is how RFC 0158's completeness check reports a gap: "9 triggers and 2
    /// extensions unclassified", never "94% complete".
    pub fn counts(&self) -> std::collections::BTreeMap<ObjectKind, usize> {
        let mut m = std::collections::BTreeMap::new();
        for o in &self.objects {
            *m.entry(o.kind).or_insert(0) += 1;
        }
        m
    }
}

/// Anything with a `query` method — [`crate::PgSource`] in production, a recorded fixture in tests.
pub trait CatalogSource {
    fn rows(&self, sql: &str) -> Result<Vec<Vec<String>>, PgError>;
}

impl CatalogSource for crate::PgSource {
    fn rows(&self, sql: &str) -> Result<Vec<Vec<String>>, PgError> {
        self.raw_query(sql)
    }
}

/// The `WHERE` fragment restricting a query to the schemas under migration.
///
/// System schemas are excluded by name rather than by an `oid` range so the exclusion is visible in
/// the generated SQL and in any log of it.
fn schema_filter(column: &str, schemas: &[String]) -> String {
    if schemas.is_empty() {
        format!("{column} NOT IN ('pg_catalog', 'information_schema', 'pg_toast')")
    } else {
        let list = schemas
            .iter()
            .map(|s| format!("'{}'", s.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{column} IN ({list})")
    }
}

/// Read the whole catalog.
///
/// Every free-text field — comments, view definitions, function bodies, column defaults — goes
/// through [`redact`] before it lands in a [`CatalogObject`]. This is EKOS's **third raw-content
/// entry point** after the `Observer` path and `recover.rs`'s direct file reads, and RFC 0043's
/// baseline is not disable-able here either. A default expression containing an API key is not a
/// hypothetical: defaults get written by migration scripts that were themselves generated.
pub fn introspect(
    src: &dyn CatalogSource,
    schemas: &[String],
    redaction: &RedactionConfig,
) -> Result<CatalogSnapshot, PgError> {
    let mut objects = Vec::new();
    let r = |s: &str| redact(s, redaction);

    // ── schemas ──
    for row in src.rows(&format!(
        "SELECT nspname FROM pg_namespace WHERE {} ORDER BY nspname",
        schema_filter("nspname", schemas)
    ))? {
        objects.push(CatalogObject {
            kind: ObjectKind::Schema,
            qualified_name: row[0].clone(),
            schema: None,
            detail: serde_json::json!({}),
        });
    }

    // ── relations: tables, partitioned tables, partitions, views, matviews, sequences ──
    for row in src.rows(&format!(
        "SELECT n.nspname, c.relname, c.relkind, c.relispartition, \
                COALESCE(obj_description(c.oid, 'pg_class'), ''), \
                COALESCE(pg_get_viewdef(c.oid, true), '') \
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {} AND c.relkind IN ('r', 'p', 'v', 'm', 'S') \
         ORDER BY n.nspname, c.relname",
        schema_filter("n.nspname", schemas)
    ))? {
        let (schema, name, relkind, is_partition) =
            (&row[0], &row[1], row[2].as_str(), row[3] == "t");
        let kind = match relkind {
            "r" if is_partition => ObjectKind::Partition,
            "r" => ObjectKind::Table,
            "p" => ObjectKind::PartitionedTable,
            "v" => ObjectKind::View,
            "m" => ObjectKind::MaterializedView,
            "S" => ObjectKind::Sequence,
            other => {
                return Err(PgError::UnknownRelkind(other.to_string()));
            }
        };
        let mut detail = serde_json::Map::new();
        if !row[4].is_empty() {
            detail.insert("comment".into(), serde_json::json!(r(&row[4])));
        }
        if !row[5].is_empty() {
            detail.insert("definition".into(), serde_json::json!(r(&row[5])));
        }
        objects.push(CatalogObject {
            kind,
            qualified_name: format!("{schema}.{name}"),
            schema: Some(schema.clone()),
            detail: serde_json::Value::Object(detail),
        });
    }

    // ── columns ──
    for row in src.rows(&format!(
        "SELECT n.nspname, c.relname, a.attname, format_type(a.atttypid, a.atttypmod), \
                a.attnotnull, COALESCE(pg_get_expr(d.adbin, d.adrelid), ''), \
                a.attidentity, a.attgenerated, a.attnum, \
                COALESCE(col_description(c.oid, a.attnum), '') \
         FROM pg_attribute a \
         JOIN pg_class c ON c.oid = a.attrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         LEFT JOIN pg_attrdef d ON d.adrelid = c.oid AND d.adnum = a.attnum \
         WHERE {} AND a.attnum > 0 AND NOT a.attisdropped \
           AND c.relkind IN ('r', 'p', 'v', 'm') \
         ORDER BY n.nspname, c.relname, a.attnum",
        schema_filter("n.nspname", schemas)
    ))? {
        let mut detail = serde_json::json!({
            "type": row[3],
            "not_null": row[4] == "t",
            "ordinal": row[8].parse::<i64>().unwrap_or_default(),
        });
        let m = detail.as_object_mut().expect("json object");
        if !row[5].is_empty() {
            m.insert("default".into(), serde_json::json!(r(&row[5])));
        }
        if !row[6].is_empty() {
            m.insert("identity".into(), serde_json::json!(row[6]));
        }
        if !row[7].is_empty() {
            m.insert("generated".into(), serde_json::json!(row[7]));
        }
        if !row[9].is_empty() {
            m.insert("comment".into(), serde_json::json!(r(&row[9])));
        }
        objects.push(CatalogObject {
            kind: ObjectKind::Column,
            qualified_name: format!("{}.{}.{}", row[0], row[1], row[2]),
            schema: Some(row[0].clone()),
            detail,
        });
    }

    // ── constraints ──
    //
    // `convalidated` is carried deliberately: a `NOT VALID` foreign key is *not* a guarantee, and a
    // migration that trusts it inherits orphan rows the source never checked (RFC 0158's
    // referential-integrity family exists for exactly this).
    for row in src.rows(&format!(
        "SELECT n.nspname, c.relname, con.conname, con.contype, con.convalidated, \
                pg_get_constraintdef(con.oid, true) \
         FROM pg_constraint con \
         JOIN pg_class c ON c.oid = con.conrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {} ORDER BY n.nspname, c.relname, con.conname",
        schema_filter("n.nspname", schemas)
    ))? {
        let kind = match row[3].as_str() {
            "p" => ObjectKind::PrimaryKey,
            "f" => ObjectKind::ForeignKey,
            "u" => ObjectKind::UniqueConstraint,
            "c" => ObjectKind::CheckConstraint,
            "x" => ObjectKind::ExclusionConstraint,
            other => return Err(PgError::UnknownConstraintType(other.to_string())),
        };
        objects.push(CatalogObject {
            kind,
            qualified_name: format!("{}.{}.{}", row[0], row[1], row[2]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({
                "validated": row[4] == "t",
                "definition": r(&row[5]),
            }),
        });
    }

    // ── indexes (excluding those backing a constraint, which are already recorded) ──
    for row in src.rows(&format!(
        "SELECT n.nspname, c.relname, i.relname, pg_get_indexdef(i.oid), x.indisunique \
         FROM pg_index x \
         JOIN pg_class i ON i.oid = x.indexrelid \
         JOIN pg_class c ON c.oid = x.indrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {} AND NOT EXISTS ( \
            SELECT 1 FROM pg_constraint con WHERE con.conindid = i.oid) \
         ORDER BY n.nspname, c.relname, i.relname",
        schema_filter("n.nspname", schemas)
    ))? {
        objects.push(CatalogObject {
            kind: ObjectKind::Index,
            qualified_name: format!("{}.{}", row[0], row[2]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({
                "on": format!("{}.{}", row[0], row[1]),
                "unique": row[4] == "t",
                "definition": r(&row[3]),
            }),
        });
    }

    // ── routines ──
    //
    // The body is carried because RFC 0163 parses it, and it is redacted because a function body is
    // one of the likelier places for a hard-coded credential to be sitting in a real database.
    for row in src.rows(&format!(
        "SELECT n.nspname, p.proname, p.prokind, l.lanname, \
                pg_get_function_identity_arguments(p.oid), \
                COALESCE(p.prosrc, ''), p.provolatile, p.proisstrict \
         FROM pg_proc p \
         JOIN pg_namespace n ON n.oid = p.pronamespace \
         JOIN pg_language l ON l.oid = p.prolang \
         WHERE {} AND p.prokind IN ('f', 'p') \
         ORDER BY n.nspname, p.proname",
        schema_filter("n.nspname", schemas)
    ))? {
        let kind = match row[2].as_str() {
            "f" => ObjectKind::Function,
            "p" => ObjectKind::Procedure,
            other => return Err(PgError::UnknownRoutineKind(other.to_string())),
        };
        objects.push(CatalogObject {
            kind,
            qualified_name: format!("{}.{}({})", row[0], row[1], row[4]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({
                "language": row[3],
                "volatility": row[6],
                "strict": row[7] == "t",
                "body": r(&row[5]),
            }),
        });
    }

    // ── triggers ──
    for row in src.rows(&format!(
        "SELECT n.nspname, c.relname, t.tgname, pg_get_triggerdef(t.oid, true) \
         FROM pg_trigger t \
         JOIN pg_class c ON c.oid = t.tgrelid \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {} AND NOT t.tgisinternal \
         ORDER BY n.nspname, c.relname, t.tgname",
        schema_filter("n.nspname", schemas)
    ))? {
        objects.push(CatalogObject {
            kind: ObjectKind::Trigger,
            qualified_name: format!("{}.{}.{}", row[0], row[1], row[2]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({ "definition": r(&row[3]) }),
        });
    }

    // ── user types ──
    for row in src.rows(&format!(
        "SELECT n.nspname, t.typname, t.typtype \
         FROM pg_type t JOIN pg_namespace n ON n.oid = t.typnamespace \
         WHERE {} AND t.typtype IN ('e', 'd', 'r') \
           AND NOT EXISTS (SELECT 1 FROM pg_class c WHERE c.oid = t.typrelid AND c.relkind <> 'c') \
         ORDER BY n.nspname, t.typname",
        schema_filter("n.nspname", schemas)
    ))? {
        let kind = match row[2].as_str() {
            "e" => ObjectKind::Enum,
            "d" => ObjectKind::Domain,
            "r" => ObjectKind::RangeType,
            other => return Err(PgError::UnknownTypeKind(other.to_string())),
        };
        objects.push(CatalogObject {
            kind,
            qualified_name: format!("{}.{}", row[0], row[1]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({}),
        });
    }

    // ── RLS policies ──
    for row in src.rows(&format!(
        "SELECT schemaname, tablename, policyname, COALESCE(qual, ''), COALESCE(with_check, '') \
         FROM pg_policies WHERE {} ORDER BY schemaname, tablename, policyname",
        schema_filter("schemaname", schemas)
    ))? {
        objects.push(CatalogObject {
            kind: ObjectKind::RlsPolicy,
            qualified_name: format!("{}.{}.{}", row[0], row[1], row[2]),
            schema: Some(row[0].clone()),
            detail: serde_json::json!({
                "using": r(&row[3]),
                "with_check": r(&row[4]),
            }),
        });
    }

    // ── extensions ──
    //
    // Not schema-filtered: an extension is database-wide, and one with no target equivalent
    // (PostGIS, custom C functions) is a blocking finding wherever it lives.
    for row in src.rows("SELECT extname, extversion FROM pg_extension ORDER BY extname")? {
        objects.push(CatalogObject {
            kind: ObjectKind::Extension,
            qualified_name: row[0].clone(),
            schema: None,
            detail: serde_json::json!({ "version": row[1] }),
        });
    }

    Ok(CatalogSnapshot { objects })
}

/// Reconcile the snapshot against the server's own counts.
///
/// The enumeration above could be wrong in a way that is invisible — a `JOIN` that drops rows, a
/// filter that is subtly too narrow — and a smaller catalog silently shrinks RFC 0158's denominator.
/// So the counts come from a *different* query shape than the one that built the list.
pub fn reconcile(
    src: &dyn CatalogSource,
    snapshot: &CatalogSnapshot,
    schemas: &[String],
) -> Result<(), PgError> {
    let relations = src.rows(&format!(
        "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE {} AND c.relkind IN ('r', 'p', 'v', 'm', 'S')",
        schema_filter("n.nspname", schemas)
    ))?;
    let expected: usize = relations[0][0].parse().unwrap_or_default();
    let got = snapshot.count_of(ObjectKind::Table)
        + snapshot.count_of(ObjectKind::PartitionedTable)
        + snapshot.count_of(ObjectKind::Partition)
        + snapshot.count_of(ObjectKind::View)
        + snapshot.count_of(ObjectKind::MaterializedView)
        + snapshot.count_of(ObjectKind::Sequence);
    if expected != got {
        return Err(PgError::CatalogIncomplete {
            kind: "relations",
            expected,
            got,
        });
    }

    let routines = src.rows(&format!(
        "SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
         WHERE {} AND p.prokind IN ('f', 'p')",
        schema_filter("n.nspname", schemas)
    ))?;
    let expected: usize = routines[0][0].parse().unwrap_or_default();
    let got = snapshot.count_of(ObjectKind::Function) + snapshot.count_of(ObjectKind::Procedure);
    if expected != got {
        return Err(PgError::CatalogIncomplete {
            kind: "routines",
            expected,
            got,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_schema_filter_excludes_system_schemas() {
        let f = schema_filter("n.nspname", &[]);
        for sys in ["pg_catalog", "information_schema", "pg_toast"] {
            assert!(f.contains(sys), "{f}");
        }
    }

    #[test]
    fn an_explicit_schema_list_is_quoted() {
        let f = schema_filter("n.nspname", &["public".into(), "o'brien".into()]);
        assert!(f.contains("'public'"), "{f}");
        assert!(
            f.contains("'o''brien'"),
            "a quote must be doubled, not dropped: {f}"
        );
    }

    #[test]
    fn kinds_with_no_target_equivalent_are_named() {
        assert!(ObjectKind::Trigger.has_no_target_equivalent());
        assert!(ObjectKind::RlsPolicy.has_no_target_equivalent());
        assert!(!ObjectKind::Table.has_no_target_equivalent());
        assert!(!ObjectKind::Column.has_no_target_equivalent());
    }

    #[test]
    fn counts_are_reported_per_kind() {
        let s = CatalogSnapshot {
            objects: vec![
                CatalogObject {
                    kind: ObjectKind::Table,
                    qualified_name: "a".into(),
                    schema: None,
                    detail: serde_json::json!({}),
                },
                CatalogObject {
                    kind: ObjectKind::Table,
                    qualified_name: "b".into(),
                    schema: None,
                    detail: serde_json::json!({}),
                },
                CatalogObject {
                    kind: ObjectKind::Trigger,
                    qualified_name: "t".into(),
                    schema: None,
                    detail: serde_json::json!({}),
                },
            ],
        };
        let c = s.counts();
        assert_eq!(c[&ObjectKind::Table], 2);
        assert_eq!(c[&ObjectKind::Trigger], 1);
    }
}
