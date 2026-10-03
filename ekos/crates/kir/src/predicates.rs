//! RFC 0170 — the shape of one business-meaning trace: a normalized column-vs-literal predicate
//! found in a `WHERE`, `HAVING`, `JOIN … ON`, `CASE` branch or `CHECK`.
//!
//! Produced by `ekos-recovery`'s `sql_predicates` (as the `predicates` property of `View`,
//! `ProcedureStatement` and `LANGUAGE sql` `Procedure` objects, and inside a `Table`'s
//! `check_constraints`), consumed by `ekos-semantic`'s `business_semantics`. It lives here because
//! both sides need the one definition and `ekos-semantic` cannot depend on `ekos-recovery`.

use serde::{Deserialize, Serialize};

/// Where in a statement a predicate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Clause {
    /// A `WHERE` filter (of a `SELECT`, `UPDATE` or `DELETE`).
    Where,
    Having,
    JoinOn,
    /// A `CASE` branch condition. `label` carries the branch's literal result, if any.
    Case,
    /// A `CHECK` constraint on a table.
    Check,
    /// A PL/pgSQL control-flow condition (`IF`/`ELSIF`/`WHILE`/`EXIT WHEN`). Its `NEW.`/`OLD.`
    /// columns carry the placeholder relations [`NEW_ROW`]/[`OLD_ROW`] until synthesis maps them to
    /// the table of the trigger that runs the routine.
    Condition,
}

/// Placeholder relation of `NEW.col` in a routine's condition (resolved via its trigger).
pub const NEW_ROW: &str = "$new";
/// Placeholder relation of `OLD.col`.
pub const OLD_ROW: &str = "$old";

/// One normalized column-vs-literal predicate, with where it was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PredicateSite {
    /// The relation the column belongs to, lower-cased; `None` when it could not be resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation: Option<String>,
    /// When `relation` is `None` for an unqualified column: the relations that were in scope, one
    /// of which owns it. Resolved later against the tables' real columns, never guessed here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
    /// The column, lower-cased.
    pub column: String,
    /// `in`, `not_in`, `<`, `<=`, `>`, `>=`, `between`, `not_between`, `is_null`, `is_not_null`,
    /// `is_true`, `is_false`, `is_not_true`, `is_not_false`, `like`, `not_like`.
    pub op: String,
    /// Normalized literals: numbers as written, strings single-quoted, `true`/`false`/`null`.
    /// Sorted and de-duplicated for `in`/`not_in`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    pub clause: Clause,
    /// A top-level conjunct of its `WHERE`/`HAVING`/`CHECK` — part of the clause's own definition,
    /// not a branch of an `OR` or a `NOT`.
    #[serde(default)]
    pub top_level: bool,
    /// Inside a subquery of its statement — a filter on the subquery's rows, not on the
    /// statement's (a view's own definition is its outermost `WHERE`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub subquery: bool,
    /// For a `CASE` branch: its literal result (`'suspended'`), the code's candidate meaning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 1-based line within the parsed text (0 when the parser recorded none).
    pub line: u64,
}

impl PredicateSite {
    /// `relation.column op values` — equal for equal predicates, whatever their spelling.
    pub fn canonical(&self) -> String {
        let target = match &self.relation {
            Some(r) => format!("{r}.{}", self.column),
            None => self.column.clone(),
        };
        canonical_text(&target, &self.op, &self.values)
    }
}

/// The canonical text of `target op values`.
pub fn canonical_text(target: &str, op: &str, values: &[String]) -> String {
    match op {
        "in" => format!("{target} IN ({})", values.join(", ")),
        "not_in" => format!("{target} NOT IN ({})", values.join(", ")),
        "between" | "not_between" => format!(
            "{target} {}BETWEEN {} AND {}",
            if op == "not_between" { "NOT " } else { "" },
            values.first().map(String::as_str).unwrap_or("?"),
            values.get(1).map(String::as_str).unwrap_or("?")
        ),
        "is_null" => format!("{target} IS NULL"),
        "is_not_null" => format!("{target} IS NOT NULL"),
        "is_true" => format!("{target} IS TRUE"),
        "is_false" => format!("{target} IS FALSE"),
        "is_not_true" => format!("{target} IS NOT TRUE"),
        "is_not_false" => format!("{target} IS NOT FALSE"),
        "like" => format!("{target} LIKE {}", values.join(", ")),
        "not_like" => format!("{target} NOT LIKE {}", values.join(", ")),
        cmp => format!("{target} {cmp} {}", values.join(", ")),
    }
}
