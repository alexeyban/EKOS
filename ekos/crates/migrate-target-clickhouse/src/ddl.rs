//! RFC 0159 — DDL emission.
//!
//! Identifiers are quoted through a builder, never concatenated (RFC 0160's rule, applied upstream
//! of the statement classifier). The emitter refuses to produce a statement it knows is wrong —
//! a table with nothing to order by — rather than emitting something that will fail at the server
//! and be diagnosed there.

use crate::design::TargetDesign;
use crate::typemap::Mapping;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DdlError {
    #[error(
        "{table} has no ORDER BY. ClickHouse requires one, and choosing it is a design decision \
         (RFC 0159) rather than something the emitter may invent."
    )]
    NoOrderBy { table: String },
    #[error("{table} has no columns")]
    NoColumns { table: String },
    #[error(
        "{table} orders by {column}, which is not one of its columns — the design and the mapping \
         disagree"
    )]
    OrderByUnknownColumn { table: String, column: String },
    #[error(
        "{table}'s design calls for PARTITION BY {expression}, and the RFC 0160 classifier cannot \
         parse a PARTITION BY clause. Emitting it would mean a statement reaching the database \
         without passing the control; dropping it silently would mean a {rows}-row table quietly \
         losing its partitioning. Neither is acceptable, so the DDL is not emitted: apply the \
         partitioning by hand, or extend the ClickHouse dialect parser."
    )]
    UnverifiablePartitioning {
        table: String,
        expression: String,
        rows: String,
    },
}

fn ident(s: &str) -> String {
    format!("`{}`", s.replace('`', "\\`"))
}

/// Emit `CREATE TABLE`.
///
/// **Codecs are not emitted**, and that is a deliberate trade rather than an oversight. RFC 0160's
/// classifier is the control every statement passes through, and `sqlparser`'s ClickHouse dialect
/// does not parse a `CODEC(...)` column annotation — in a `CREATE TABLE` or in an
/// `ALTER TABLE ... MODIFY COLUMN` (both probed against the parser). The options were to weaken the
/// control so the optimization fits, or to drop the optimization.
///
/// A codec is a compression choice. The classifier is a safety one. Preprocessing the text before
/// classification would mean the classified statement is not the executed statement, which is the
/// hole the whole control exists to close, so the codecs are carried into
/// [`rationale_comment`] instead — visible to whoever reviews the DDL, and applied by hand or by a
/// later `ALTER` if they are worth it.
pub fn create_table(
    design: &TargetDesign,
    mappings: &[Mapping],
    target_database: &str,
) -> Result<String, DdlError> {
    if mappings.is_empty() {
        return Err(DdlError::NoColumns {
            table: design.table.clone(),
        });
    }
    if design.order_by.is_empty() {
        return Err(DdlError::NoOrderBy {
            table: design.table.clone(),
        });
    }
    let known: std::collections::BTreeSet<&str> =
        mappings.iter().map(|m| m.column.as_str()).collect();
    for c in &design.order_by {
        if !known.contains(c.as_str()) {
            return Err(DdlError::OrderByUnknownColumn {
                table: design.table.clone(),
                column: c.clone(),
            });
        }
    }

    // The bare table name inside the target database: a migration's sandbox is a separate database,
    // never a prefix inside a real one (RFC 0160).
    let bare = design.table.rsplit('.').next().unwrap_or(&design.table);
    let mut sql = format!(
        "CREATE TABLE {}.{} (\n",
        ident(target_database),
        ident(bare)
    );
    let body: Vec<String> = mappings
        .iter()
        .map(|m| format!("    {} {}", ident(&m.column), m.target_type))
        .collect();
    sql.push_str(&body.join(",\n"));
    sql.push_str("\n)\n");
    sql.push_str(&format!("ENGINE = {}\n", design.engine.render()));
    // See `UnverifiablePartitioning`: the classifier cannot read a PARTITION BY clause, and neither
    // bypassing the control nor silently dropping a design decision is an acceptable way to ship
    // one.
    if let Some(p) = &design.partition_by {
        return Err(DdlError::UnverifiablePartitioning {
            table: design.table.clone(),
            expression: p.clone(),
            rows: "large".into(),
        });
    }
    sql.push_str(&format!(
        "ORDER BY ({})",
        design
            .order_by
            .iter()
            .map(|c| ident(c))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    Ok(sql)
}

/// Render the design's reasoning as SQL comments, so the DDL a human reviews carries the evidence
/// with it rather than pointing at a report they have to go and find.
pub fn rationale_comment(design: &TargetDesign) -> String {
    let mut out = vec![
        format!("-- source: {}", design.table),
        format!("-- engine: {}", design.engine_rationale),
        format!("-- order by: {}", design.order_by_rationale),
        format!("-- partitioning: {}", design.partition_rationale),
    ];
    if let Some(p) = &design.partition_by {
        out.push(format!(
            "-- PARTITION BY {p} — chosen, and NOT emitted: the RFC 0160 classifier cannot parse a"
        ));
        out.push(
            "-- PARTITION BY clause, so this must be applied deliberately by an operator.".into(),
        );
    }
    if !design.codecs.is_empty() {
        out.push(
            "-- codecs chosen from the profile, NOT emitted: the RFC 0160 classifier cannot parse a"
                .into(),
        );
        out.push("-- CODEC clause, and the control is worth more than the compression.".into());
        for (column, codec) in &design.codecs {
            out.push(format!("--   {column}: {codec}"));
        }
    }
    for f in &design.findings {
        out.push(format!("-- NEEDS A DECISION: {f}"));
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{DesignColumn, Engine, TableEvidence, design};
    use crate::typemap::{ColumnEvidence, map_column};

    fn mapping(name: &str, ty: &str) -> Mapping {
        map_column(name, ty, false, &ColumnEvidence::default())
    }

    fn simple_design() -> TargetDesign {
        design(
            "public.orders",
            &TableEvidence {
                primary_key: vec!["id".into()],
                ..Default::default()
            },
            &[DesignColumn {
                name: "id".into(),
                target_type: "Int64".into(),
                distinct: None,
                monotonic: Some(true),
            }],
        )
    }

    #[test]
    fn a_minimal_table_renders() {
        let d = simple_design();
        let sql = create_table(&d, &[mapping("id", "bigint")], "sandbox").unwrap();
        assert!(sql.contains("CREATE TABLE `sandbox`.`orders`"), "{sql}");
        assert!(sql.contains("`id` Int64"), "{sql}");
        assert!(sql.contains("ENGINE = MergeTree"), "{sql}");
        assert!(sql.contains("ORDER BY (`id`)"), "{sql}");
        // Codecs are chosen, and deliberately not emitted — see the note on `create_table`.
        assert!(!sql.contains("CODEC"), "{sql}");
        assert!(
            rationale_comment(&d).contains("CODEC(Delta, ZSTD(1))"),
            "the choice must survive into the comment even though it is not emitted"
        );
    }

    /// The invariant that would have caught the codec problem at build time rather than at the
    /// first real load: **everything this emitter produces must pass the RFC 0160 classifier.**
    /// The classifier is what stands between a generated statement and a production database, so
    /// emitting SQL it cannot read is not a limitation to work around — it is a bug here.
    #[test]
    fn every_generated_statement_passes_the_classifier() {
        use crate::classify::{StatementClass, batch_class};
        use crate::design::{TableEvidence, design};

        let cases: Vec<(TargetDesign, Vec<Mapping>)> =
            vec![(simple_design(), vec![mapping("id", "bigint")]), {
                let ev = TableEvidence {
                    row_count: 500_000_000,
                    has_updates: true,
                    update_time_column: Some("updated_at".into()),
                    primary_key: vec!["id".into()],
                    time_column: Some(("created_at".into(), 36)),
                    ..Default::default()
                };
                let cols = ["id", "updated_at", "created_at"]
                    .iter()
                    .map(|n| DesignColumn {
                        name: (*n).into(),
                        target_type: "Int64".into(),
                        distinct: None,
                        monotonic: Some(true),
                    })
                    .collect::<Vec<_>>();
                let mut d = design("public.orders", &ev, &cols);
                // Partitioned designs are refused by `create_table` (see UnverifiablePartitioning),
                // so the invariant is asserted over what it will actually emit.
                d.partition_by = None;
                let m = vec![
                    mapping("id", "bigint"),
                    mapping("updated_at", "timestamp without time zone"),
                    mapping("created_at", "timestamp without time zone"),
                ];
                (d, m)
            }];

        for (d, m) in cases {
            let sql = create_table(&d, &m, "sandbox").unwrap();
            let class = batch_class(&sql).unwrap_or_else(|e| {
                panic!("generated DDL does not pass the classifier: {e}\n{sql}")
            });
            assert_eq!(class, StatementClass::DdlCreate, "{sql}");
        }
    }

    /// The emitter refuses rather than producing a statement it knows the server will reject.
    #[test]
    fn a_table_with_no_order_by_is_refused() {
        let d = design("s.t", &TableEvidence::default(), &[]);
        assert_eq!(
            create_table(&d, &[mapping("x", "text")], "sandbox"),
            Err(DdlError::NoOrderBy {
                table: "s.t".into()
            })
        );
    }

    /// A design and a mapping that disagree is a bug worth failing on, not one to discover when the
    /// server complains about an unknown column.
    #[test]
    fn ordering_by_a_column_that_was_not_mapped_is_refused() {
        let mut d = simple_design();
        d.order_by = vec!["ghost".into()];
        assert!(matches!(
            create_table(&d, &[mapping("id", "bigint")], "sandbox"),
            Err(DdlError::OrderByUnknownColumn { .. })
        ));
    }

    #[test]
    fn identifiers_are_quoted_not_concatenated() {
        let mut d = simple_design();
        d.table = "public.weird`name".into();
        d.order_by = vec!["odd`col".into()];
        let sql = create_table(&d, &[mapping("odd`col", "bigint")], "sand`box").unwrap();
        assert!(sql.contains("`sand\\`box`"), "{sql}");
        assert!(sql.contains("`odd\\`col`"), "{sql}");
    }

    #[test]
    fn partitioning_and_a_version_column_render() {
        let ev = TableEvidence {
            row_count: 500_000_000,
            has_updates: true,
            update_time_column: Some("updated_at".into()),
            primary_key: vec!["id".into()],
            time_column: Some(("created_at".into(), 36)),
            ..Default::default()
        };
        let cols = [
            DesignColumn {
                name: "id".into(),
                target_type: "Int64".into(),
                distinct: None,
                monotonic: None,
            },
            DesignColumn {
                name: "updated_at".into(),
                target_type: "DateTime64(6)".into(),
                distinct: None,
                monotonic: None,
            },
            DesignColumn {
                name: "created_at".into(),
                target_type: "DateTime64(6)".into(),
                distinct: None,
                monotonic: None,
            },
        ];
        let d = design("public.orders", &ev, &cols);
        assert_eq!(
            d.engine,
            Engine::ReplacingMergeTree {
                version_column: Some("updated_at".into())
            }
        );
        let m = [
            mapping("id", "bigint"),
            mapping("updated_at", "timestamp without time zone"),
            mapping("created_at", "timestamp without time zone"),
        ];
        // The design partitions, so emission is refused rather than bypassing the classifier or
        // silently dropping the decision.
        assert!(matches!(
            create_table(&d, &m, "sandbox"),
            Err(DdlError::UnverifiablePartitioning { .. })
        ));
        assert!(
            rationale_comment(&d).contains("PARTITION BY toYYYYMM(created_at)"),
            "the decision must survive into the comment"
        );

        // Without partitioning the same design emits, and the engine renders.
        let mut unpartitioned = d.clone();
        unpartitioned.partition_by = None;
        let sql = create_table(&unpartitioned, &m, "sandbox").unwrap();
        assert!(
            sql.contains("ENGINE = ReplacingMergeTree(updated_at)"),
            "{sql}"
        );
    }

    /// The DDL a human reviews carries the reasoning, so approving it does not mean going to find a
    /// report somewhere else.
    #[test]
    fn the_rationale_comment_carries_findings_prominently() {
        let ev = TableEvidence {
            has_updates: true,
            update_time_column: None,
            primary_key: vec!["id".into()],
            ..Default::default()
        };
        let d = design(
            "public.orders",
            &ev,
            &[DesignColumn {
                name: "id".into(),
                target_type: "Int64".into(),
                distinct: None,
                monotonic: None,
            }],
        );
        let c = rationale_comment(&d);
        assert!(c.contains("-- engine:"), "{c}");
        assert!(c.contains("NEEDS A DECISION"), "{c}");
        assert!(c.contains("now()"), "{c}");
    }
}
