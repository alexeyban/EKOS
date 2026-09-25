//! RFC 0160 — the statement classifier.
//!
//! Nothing executes that was not parsed and classified. This generalizes
//! `ekos_clickhouse_query::validate::validate_select_only`, which already parses LLM-generated SQL
//! through the dialect SDK and hard-rejects anything but a single `SELECT` — exactly the right
//! shape, and too narrow for a component that legitimately needs to write.
//!
//! Five rules, each of which exists because the obvious alternative fails:
//!
//! 1. **Parse; never match text.** A classifier a comment or a string literal can fool is not a
//!    control.
//! 2. **Unparseable is refused.** This is `validate_select_only`'s behaviour and it is preserved
//!    exactly. A pass-through-with-a-warning is the escape hatch that makes every other control
//!    decorative.
//! 3. **A batch's class is the maximum of its parts.** One `DROP` hidden in forty `INSERT`s makes
//!    the batch destructive.
//! 4. **`Unknown` is refused.** A statement the parser accepted but the classifier cannot place is
//!    treated as dangerous, not benign. Adding a form is a deliberate act.
//! 5. **`DELETE`/`UPDATE` with no `WHERE` is `Destructive`**, not `DmlMutate`.

use serde::{Deserialize, Serialize};
use sqlparser::ast::Statement;

/// What a statement does, ordered by how much it can cost. `Ord` is the batch rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementClass {
    Read,
    DdlCreate,
    DdlAlter,
    DmlInsert,
    DmlMutate,
    Destructive,
    /// Parsed, but not placed. Refused — see rule 4.
    Unknown,
}

impl StatementClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::DdlCreate => "ddl_create",
            Self::DdlAlter => "ddl_alter",
            Self::DmlInsert => "dml_insert",
            Self::DmlMutate => "dml_mutate",
            Self::Destructive => "destructive",
            Self::Unknown => "unknown",
        }
    }

    /// `true` for a class that may never execute.
    pub fn is_refused(self) -> bool {
        self == Self::Unknown
    }

    /// `true` where the statement changes data or schema.
    pub fn writes(self) -> bool {
        !matches!(self, Self::Read)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ClassifyError {
    #[error("refusing to classify unparseable SQL, so it cannot be executed: {message}\n  {sql}")]
    Unparseable { sql: String, message: String },
    #[error(
        "statement {index} parsed but could not be classified ({statement}). An unplaced statement \
         is treated as dangerous rather than benign — add it to StatementClass deliberately."
    )]
    Unclassifiable { index: usize, statement: String },
    #[error("empty statement")]
    Empty,
    #[error(
        "refusing to emit a credential inside a statement: {hint}. Configure a ClickHouse named \
         collection and reference it by name — a generated statement is hashed, pinned to an \
         approval, printed in a dry run and pasted into tickets."
    )]
    EmbeddedCredential { hint: String },
}

/// One classified statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classified {
    pub class: StatementClass,
    pub rendered: String,
}

fn class_of(st: &Statement) -> StatementClass {
    use StatementClass as C;
    match st {
        Statement::Query(_) | Statement::ExplainTable { .. } | Statement::Explain { .. } => C::Read,
        Statement::ShowTables { .. } | Statement::ShowColumns { .. } => C::Read,
        Statement::CreateTable(_)
        | Statement::CreateView { .. }
        | Statement::CreateDatabase { .. }
        | Statement::CreateSchema { .. }
        | Statement::CreateIndex(_) => C::DdlCreate,
        Statement::AlterTable { .. } => C::DdlAlter,
        Statement::Insert(_) => C::DmlInsert,
        // Rule 5: an unfiltered mutation is destructive, whatever it is spelled.
        Statement::Update { selection, .. } => {
            if selection.is_some() {
                C::DmlMutate
            } else {
                C::Destructive
            }
        }
        Statement::Delete(d) => {
            let filtered = d.selection.is_some();
            if filtered {
                C::DmlMutate
            } else {
                C::Destructive
            }
        }
        Statement::Merge { .. } => C::DmlMutate,
        Statement::Drop { .. } | Statement::Truncate { .. } => C::Destructive,
        _ => C::Unknown,
    }
}

/// Table functions that take a credential positionally. Every one of them is a way to put a
/// production password into an artifact that gets hashed, pinned and shared.
const CREDENTIAL_FUNCTIONS: &[&str] = &["postgresql", "mysql", "mongodb", "s3", "url"];

/// Refuse a statement carrying an inline credential.
///
/// **Checked on the AST, not the text.** The first version of this scanned for `postgresql(` and
/// friends in the SQL string — and refused
/// `SELECT count(*) FROM orders WHERE note = 'url(a,b,c)'`, because a string literal contains the
/// pattern. That is rule 1 of this module broken by its own credential check: a control that text
/// matching can fool is not a control, whichever direction it fails in. A false refusal is milder
/// than a false pass, and it is still a bug.
///
/// A named-collection call — `postgresql(ekos_migrate_source, table = 'orders')` — has exactly one
/// positional argument. A credential-bearing call has several.
fn reject_embedded_credentials_ast(statements: &[Statement]) -> Result<(), ClassifyError> {
    use sqlparser::ast::{BinaryOperator, Expr, FunctionArg, FunctionArgExpr, TableFactor};

    for st in statements {
        let mut offender = None;
        visit_table_factors(st, &mut |f: &TableFactor| {
            if offender.is_some() {
                return;
            }
            let TableFactor::Table {
                name,
                args: Some(args),
                ..
            } = f
            else {
                return;
            };
            let fname = name
                .0
                .last()
                .map(|i| i.value.to_ascii_lowercase())
                .unwrap_or_default();
            if !CREDENTIAL_FUNCTIONS.contains(&fname.as_str()) {
                return;
            }
            // ClickHouse spells a keyword argument `table = 'orders'`, which sqlparser parses as
            // an *unnamed* argument holding an `Eq` expression — not as `FunctionArg::Named`. So
            // counting unnamed arguments alone reads a perfectly safe named-collection call as
            // five positional ones. A genuine positional argument is an unnamed, non-assignment
            // expression.
            let positional = args
                .args
                .iter()
                .filter(|a| match a {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(Expr::BinaryOp {
                        op: BinaryOperator::Eq,
                        ..
                    })) => false,
                    FunctionArg::Unnamed(_) => true,
                    _ => false,
                })
                .count();
            if positional > 1 {
                offender = Some(fname);
            }
        });
        if let Some(f) = offender {
            return Err(ClassifyError::EmbeddedCredential {
                hint: format!("{f}() was called with positional arguments"),
            });
        }
    }
    Ok(())
}

/// Visit every table factor in a statement's `FROM` clauses, including inside a subquery.
fn visit_table_factors(st: &Statement, f: &mut impl FnMut(&sqlparser::ast::TableFactor)) {
    use sqlparser::ast::{Query, SetExpr, TableFactor};

    fn walk_query(q: &Query, f: &mut impl FnMut(&TableFactor)) {
        match q.body.as_ref() {
            SetExpr::Select(s) => {
                for t in &s.from {
                    walk_factor(&t.relation, f);
                    for j in &t.joins {
                        walk_factor(&j.relation, f);
                    }
                }
            }
            SetExpr::Query(inner) => walk_query(inner, f),
            SetExpr::SetOperation { left, right, .. } => {
                for side in [left, right] {
                    if let SetExpr::Select(s) = side.as_ref() {
                        for t in &s.from {
                            walk_factor(&t.relation, f);
                            for j in &t.joins {
                                walk_factor(&j.relation, f);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn walk_factor(factor: &TableFactor, f: &mut impl FnMut(&TableFactor)) {
        f(factor);
        if let TableFactor::Derived { subquery, .. } = factor {
            walk_query(subquery, f);
        }
    }

    match st {
        Statement::Query(q) => walk_query(q, f),
        Statement::Insert(i) => {
            if let Some(src) = &i.source {
                walk_query(src, f);
            }
        }
        _ => {}
    }
}

/// Parse and classify every statement in `sql`.
pub fn classify(sql: &str) -> Result<Vec<Classified>, ClassifyError> {
    let dialect = sqlparser::dialect::ClickHouseDialect {};
    let statements = sqlparser::parser::Parser::parse_sql(&dialect, sql).map_err(|e| {
        ClassifyError::Unparseable {
            sql: sql.trim().to_string(),
            message: e.to_string(),
        }
    })?;
    if statements.is_empty() {
        return Err(ClassifyError::Empty);
    }
    reject_embedded_credentials_ast(&statements)?;

    let mut out = Vec::with_capacity(statements.len());
    for (index, st) in statements.into_iter().enumerate() {
        let class = class_of(&st);
        if class.is_refused() {
            return Err(ClassifyError::Unclassifiable {
                index,
                statement: st.to_string(),
            });
        }
        out.push(Classified {
            class,
            rendered: st.to_string(),
        });
    }
    Ok(out)
}

/// The class of a whole batch: the maximum of its parts (rule 3).
pub fn batch_class(sql: &str) -> Result<StatementClass, ClassifyError> {
    Ok(classify(sql)?
        .into_iter()
        .map(|c| c.class)
        .max()
        .unwrap_or(StatementClass::Read))
}

#[cfg(test)]
mod tests {
    use super::*;
    use StatementClass as C;

    /// Probe: which ClickHouse forms does the parser actually accept?
    #[test]
    #[ignore]
    fn probe_dialect_coverage() {
        for sql in [
            "CREATE TABLE t (a Int64 CODEC(Delta, ZSTD(1))) ENGINE = MergeTree ORDER BY a",
            "ALTER TABLE t MODIFY COLUMN a Int64 CODEC(Delta, ZSTD(1))",
            "CREATE TABLE t (a Int64) ENGINE = MergeTree ORDER BY a",
            "CREATE TABLE t (a Int64, b Date) ENGINE = MergeTree PARTITION BY toYYYYMM(b) ORDER BY a",
            "CREATE TABLE t (a Int64, b Date) ENGINE = MergeTree ORDER BY a PARTITION BY toYYYYMM(b)",
            "CREATE TABLE t (a Int64) ENGINE = ReplacingMergeTree(a) ORDER BY a",
            "CREATE TABLE t (a Int64) ENGINE = MergeTree ORDER BY (a)",
            "CREATE TABLE t (a Int64, b Int64) ENGINE = MergeTree ORDER BY (a, b)",
        ] {
            let r = classify(sql);
            eprintln!("{:<8} {sql}", if r.is_ok() { "OK" } else { "REFUSED" });
        }
    }

    #[test]
    fn each_form_classifies() {
        for (sql, want) in [
            ("SELECT 1", C::Read),
            (
                "CREATE TABLE t (a Int64) ENGINE = MergeTree ORDER BY a",
                C::DdlCreate,
            ),
            ("ALTER TABLE t ADD COLUMN b Int64", C::DdlAlter),
            ("INSERT INTO t SELECT 1", C::DmlInsert),
            ("DELETE FROM t WHERE a = 1", C::DmlMutate),
            ("DROP TABLE t", C::Destructive),
            ("TRUNCATE TABLE t", C::Destructive),
        ] {
            assert_eq!(batch_class(sql).unwrap(), want, "{sql}");
        }
    }

    /// Rule 5. An unfiltered mutation is not an ordinary one.
    #[test]
    fn an_unfiltered_delete_is_destructive() {
        assert_eq!(
            batch_class("DELETE FROM t WHERE a = 1").unwrap(),
            C::DmlMutate
        );
        assert_eq!(batch_class("DELETE FROM t").unwrap(), C::Destructive);
    }

    /// Rule 3. The adversarial case: one destructive statement buried in a long batch.
    #[test]
    fn one_drop_in_forty_inserts_makes_the_batch_destructive() {
        let mut sql = (0..40)
            .map(|i| format!("INSERT INTO t SELECT {i};"))
            .collect::<Vec<_>>()
            .join(" ");
        sql.push_str(" DROP TABLE t;");
        assert_eq!(batch_class(&sql).unwrap(), C::Destructive);
    }

    /// Rule 1. A classifier text-matching would be fooled by both of these.
    #[test]
    fn a_drop_inside_a_comment_or_a_string_is_not_a_drop() {
        assert_eq!(
            batch_class("SELECT 1 -- DROP TABLE t").unwrap(),
            C::Read,
            "a comment is not a statement"
        );
        assert_eq!(
            batch_class("SELECT 'DROP TABLE t' AS s").unwrap(),
            C::Read,
            "a string literal is not a statement"
        );
    }

    /// Rule 2. Preserved exactly from `validate_select_only`.
    #[test]
    fn unparseable_sql_is_refused_not_passed_through() {
        assert!(matches!(
            classify("this is not sql"),
            Err(ClassifyError::Unparseable { .. })
        ));
        assert!(matches!(
            classify(""),
            Err(ClassifyError::Unparseable { .. }) | Err(ClassifyError::Empty)
        ));
    }

    /// Rule 4. A statement the parser accepted but the classifier cannot place is dangerous.
    #[test]
    fn an_unplaced_statement_is_refused() {
        // `GRANT` parses on the ClickHouse dialect and is deliberately not in `class_of`.
        let r = classify("GRANT SELECT ON db.t TO someone");
        assert!(
            matches!(r, Err(ClassifyError::Unclassifiable { .. })),
            "got {r:?}"
        );
    }

    // ── credentials ──────────────────────────────────────────────────────────

    /// The rule this codebase needs most: a generated statement is hashed, pinned to an approval,
    /// printed in a dry run and pasted into tickets. A password must not travel that road.
    #[test]
    fn a_positional_postgresql_call_is_refused() {
        let sql = "INSERT INTO sandbox.orders SELECT * FROM \
                   postgresql('host:5432', 'db', 'orders', 'user', 'hunter2')";
        assert!(
            matches!(classify(sql), Err(ClassifyError::EmbeddedCredential { .. })),
            "a positional call carries the password inline"
        );
    }

    #[test]
    fn a_named_collection_call_is_allowed() {
        let sql = "INSERT INTO sandbox.orders SELECT * FROM \
                   postgresql(ekos_migrate_source, table = 'orders', schema = 'public')";
        assert_eq!(batch_class(sql).unwrap(), C::DmlInsert);
    }

    #[test]
    fn other_credential_taking_functions_are_covered_too() {
        for f in ["mysql", "s3", "url", "mongodb"] {
            let sql = format!("SELECT * FROM {f}('a', 'b', 'c', 'd')");
            assert!(
                matches!(
                    classify(&sql),
                    Err(ClassifyError::EmbeddedCredential { .. })
                ),
                "{f} takes credentials positionally and must be refused"
            );
        }
    }

    /// The regression this module's own rule 1 demanded: a string literal containing
    /// `url(a,b,c)` is not a function call, and a text-matching check refuses it.
    #[test]
    fn the_credential_check_does_not_fire_on_a_string_literal() {
        assert_eq!(
            batch_class("SELECT count(*) FROM orders WHERE note = 'url(a,b,c)'").unwrap(),
            C::Read,
            "a string literal is not a function call"
        );
        assert_eq!(
            batch_class("SELECT 1 -- postgresql('h','d','t','u','p')").unwrap(),
            C::Read,
            "nor is a comment"
        );
    }

    /// A credential hidden one level down still counts.
    #[test]
    fn a_credential_inside_a_subquery_is_found() {
        let sql = "INSERT INTO t SELECT * FROM (SELECT * FROM postgresql('h', 'd', 't', 'u', 'p'))";
        assert!(matches!(
            classify(sql),
            Err(ClassifyError::EmbeddedCredential { .. })
        ));
    }

    #[test]
    fn ordering_is_what_makes_the_batch_rule_work() {
        assert!(C::Destructive > C::DmlMutate);
        assert!(C::DmlMutate > C::DmlInsert);
        assert!(C::DmlInsert > C::DdlAlter);
        assert!(C::Read < C::DdlCreate);
        assert!(!C::Read.writes());
        assert!(C::DmlInsert.writes());
    }
}
