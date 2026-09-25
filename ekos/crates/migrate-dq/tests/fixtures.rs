//! RFC 0158 — every rule has a fixture, and the catalog test enforces it.
//!
//! "Rules are data" is only an advantage while the data is tested. A rule with no fixture is a rule
//! nobody has seen fire, and it will sit in the catalog looking like coverage.
//!
//! Each fixture states, for one rule: a column (or table) it **must** fire on, and one it must
//! **not**. The negative case is the half that catches an `applies` predicate which matches
//! everything — a rule that always fires is indistinguishable from thorough until someone reads the
//! report.

use ekos_migrate_dq::catalog::{COLUMN_RULES, TABLE_RULES};
use ekos_migrate_dq::{ColumnContext, TableContext, evaluate_column, evaluate_table};

fn col(table: &str, column: &str, ty: &str) -> ColumnContext {
    ColumnContext {
        table: table.into(),
        column: column.into(),
        data_type: ty.into(),
        nullable: true,
        row_count: 1000,
        ..Default::default()
    }
}

/// `(rule id, a context that must fire, a context that must not)`.
fn column_fixtures() -> Vec<(&'static str, ColumnContext, ColumnContext)> {
    vec![
        (
            "DQ.COMPLETE.001",
            col("s.t", "note", "text"),
            ColumnContext {
                nullable: false,
                ..col("s.t", "note", "text")
            },
        ),
        (
            "DQ.COMPLETE.002",
            col("s.t", "closed_at", "date"),
            col("s.t", "note", "text"),
        ),
        (
            "DQ.UNIQ.001",
            ColumnContext {
                distinct_estimate: Some(900.0),
                ..col("s.t", "customer_id", "bigint")
            },
            // Same shape, but never profiled: the rule cannot conclude, so it must not fire.
            col("s.t", "customer_id", "bigint"),
        ),
        (
            "DQ.VALID.001",
            col("s.t", "payload_json", "text"),
            col("s.t", "description", "text"),
        ),
        (
            "COMPAT.CH.NUMERIC_UNCONSTRAINED",
            col("s.t", "amount", "numeric"),
            col("s.t", "amount", "numeric(12,2)"),
        ),
        (
            "COMPAT.CH.NUMERIC_PRECISION",
            col("s.t", "huge", "numeric(80,2)"),
            col("s.t", "ok", "numeric(38,2)"),
        ),
        (
            "COMPAT.CH.DATE_BEFORE_1900",
            col("s.t", "born", "date"),
            col("s.t", "name", "text"),
        ),
        (
            "COMPAT.CH.INFINITE_TIMESTAMP",
            col("s.t", "valid_until", "timestamp with time zone"),
            col("s.t", "name", "text"),
        ),
        (
            "COMPAT.CH.CHAR_PADDING",
            col("s.t", "code", "character(5)"),
            col("s.t", "code", "character varying(5)"),
        ),
        (
            "COMPAT.CH.NULLABLE",
            ColumnContext {
                null_fraction: Some(0.0),
                ..col("s.t", "status", "text")
            },
            ColumnContext {
                null_fraction: Some(0.3),
                ..col("s.t", "status", "text")
            },
        ),
    ]
}

fn table_fixtures() -> Vec<(&'static str, TableContext, TableContext)> {
    vec![
        (
            "COMPAT.CH.NO_CONSTRAINT_ENFORCEMENT",
            TableContext {
                table: "s.orders".into(),
                primary_key_columns: vec!["id".into()],
                ..Default::default()
            },
            TableContext {
                table: "s.log".into(),
                ..Default::default()
            },
        ),
        (
            "DQ.REFINT.001",
            TableContext {
                table: "s.orders".into(),
                unvalidated_constraints: vec!["orders_customer_fk".into()],
                ..Default::default()
            },
            TableContext {
                table: "s.orders".into(),
                ..Default::default()
            },
        ),
        (
            "DQ.TIMELY.001",
            TableContext {
                table: "s.archive".into(),
                row_count: 1000,
                looks_static: true,
                ..Default::default()
            },
            // Static but empty: nothing to archive, so nothing to say.
            TableContext {
                table: "s.empty".into(),
                row_count: 0,
                looks_static: true,
                ..Default::default()
            },
        ),
    ]
}

/// The guard that makes the catalog trustworthy.
#[test]
fn every_rule_in_the_catalog_has_a_fixture() {
    let covered: Vec<&str> = column_fixtures()
        .iter()
        .map(|(id, _, _)| *id)
        .chain(table_fixtures().iter().map(|(id, _, _)| *id))
        .collect();

    for r in COLUMN_RULES {
        assert!(
            covered.contains(&r.id),
            "column rule {} has no fixture — an untested rule looks like coverage",
            r.id
        );
    }
    for r in TABLE_RULES {
        assert!(
            covered.contains(&r.id),
            "table rule {} has no fixture",
            r.id
        );
    }
    assert_eq!(
        covered.len(),
        COLUMN_RULES.len() + TABLE_RULES.len(),
        "a fixture exists for a rule that is not in the catalog"
    );
}

#[test]
fn every_column_rule_fires_on_its_positive_fixture() {
    for (id, positive, _) in column_fixtures() {
        let fired: Vec<String> = evaluate_column(&positive)
            .into_iter()
            .map(|f| f.rule_id)
            .collect();
        assert!(
            fired.iter().any(|f| f == id),
            "{id} did not fire on its own positive fixture (fired: {fired:?})"
        );
    }
}

/// The half that catches an `applies` predicate matching everything.
#[test]
fn no_column_rule_fires_on_its_negative_fixture() {
    for (id, _, negative) in column_fixtures() {
        let fired: Vec<String> = evaluate_column(&negative)
            .into_iter()
            .map(|f| f.rule_id)
            .collect();
        assert!(
            !fired.iter().any(|f| f == id),
            "{id} fired on a column it should ignore — a rule that always fires is \
             indistinguishable from thorough until someone reads the report"
        );
    }
}

#[test]
fn every_table_rule_fires_on_its_positive_and_not_its_negative_fixture() {
    for (id, positive, negative) in table_fixtures() {
        let pos: Vec<String> = evaluate_table(&positive)
            .into_iter()
            .map(|f| f.rule_id)
            .collect();
        assert!(
            pos.iter().any(|f| f == id),
            "{id} did not fire (fired: {pos:?})"
        );
        let neg: Vec<String> = evaluate_table(&negative)
            .into_iter()
            .map(|f| f.rule_id)
            .collect();
        assert!(
            !neg.iter().any(|f| f == id),
            "{id} fired when it should not"
        );
    }
}

/// Every rule that measures must produce SQL, and every rule that does not must explain why it can
/// conclude without one.
#[test]
fn a_measuring_rule_produces_runnable_sql() {
    for (id, positive, _) in column_fixtures() {
        let f = evaluate_column(&positive)
            .into_iter()
            .find(|f| f.rule_id == id)
            .unwrap();
        if let Some(sql) = &f.evidence_sql {
            assert!(
                sql.starts_with("SELECT count(*)") || sql.starts_with("SELECT COALESCE"),
                "{id}: {sql}"
            );
            assert!(sql.contains("FROM"), "{id}: {sql}");
            // Identifiers are quoted, never concatenated raw (RFC 0160's rule).
            assert!(
                sql.contains("\"s\".\"t\""),
                "{id} did not quote its table: {sql}"
            );
        }
    }
}

/// A finding must never carry a value from the data — not in its message, not anywhere. The message
/// is rendered from the column's *name* and *type*, both of which are schema, not data.
#[test]
fn no_finding_message_can_carry_a_value() {
    for (_, positive, _) in column_fixtures() {
        for f in evaluate_column(&positive) {
            assert!(
                f.affected_rows.is_none(),
                "the crate has no database; a count here would be invented"
            );
            assert!(!f.message.is_empty());
        }
    }
}

/// Every behavioural finding needs a human, whatever its severity. These are the ones that break
/// nothing on load day.
#[test]
fn behavioural_findings_always_block() {
    let ctx = col("s.t", "code", "character(5)");
    let f = evaluate_column(&ctx)
        .into_iter()
        .find(|f| f.rule_id == "COMPAT.CH.CHAR_PADDING")
        .unwrap();
    assert_eq!(f.severity, ekos_migrate_dq::Severity::Warn);
    assert!(
        f.blocks(),
        "a behavioural difference must not be dismissible as a warning"
    );
}

/// The unprofiled case, which is where a rule is most tempted to guess.
#[test]
fn an_unprofiled_unconstrained_numeric_says_so_rather_than_choosing() {
    let unprofiled = col("s.t", "amount", "numeric");
    let f = evaluate_column(&unprofiled)
        .into_iter()
        .find(|f| f.rule_id == "COMPAT.CH.NUMERIC_UNCONSTRAINED")
        .unwrap();
    assert!(
        f.message.contains("has not been profiled"),
        "an unprofiled column must say so: {}",
        f.message
    );

    let profiled = ColumnContext {
        numeric_precision_used: Some(14),
        numeric_scale_used: Some(2),
        ..unprofiled
    };
    let f = evaluate_column(&profiled)
        .into_iter()
        .find(|f| f.rule_id == "COMPAT.CH.NUMERIC_UNCONSTRAINED")
        .unwrap();
    assert!(f.message.contains("narrowing-safe"), "{}", f.message);
    assert!(f.message.contains("precision 14"), "{}", f.message);
}
