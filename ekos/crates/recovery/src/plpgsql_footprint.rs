//! RFC 0163 — what one PL/pgSQL statement reads, writes and calls, from a real SQL parse.
//!
//! The procedural IR (`ekos-plpgsql`) owns order and condition and carries each embedded SQL
//! statement as text. This module parses that text with `sqlparser`'s PostgreSQL dialect and walks
//! the AST, so the tables a routine touches come from the grammar, not from a regex over the text.
//!
//! - **Writes** are the targets of `INSERT`/`UPDATE`/`DELETE`/`MERGE`/`TRUNCATE`.
//! - **Reads** are every other relation in the statement, minus its own CTE names. A table both
//!   written and read (`INSERT INTO t SELECT … FROM t`) is both.
//! - **Calls** are every function invoked, in an expression or as a table function
//!   (`FROM setting_get(…)` is a call, not a table). Built-ins are included — `coalesce`, `now` —
//!   because only a later whole-graph match against real routines can tell them apart.
//!
//! PL/pgSQL variables need no substitution: where they may appear (expressions), the parser reads
//! them as column references, which name no relation and so add nothing to the footprint.
//!
//! A statement that does not parse is reported as such with the parser's error, never guessed at.

use sqlparser::ast::{
    Expr, FromTable, ObjectName, Query, Statement, TableFactor, TableWithJoins, Visit, Visitor,
};
use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::parser::Parser;
use std::collections::BTreeSet;
use std::ops::ControlFlow;

/// What a fragment of SQL touches. Names are as written, unquoted parts lower-cased the way
/// PostgreSQL folds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Footprint {
    pub reads: BTreeSet<String>,
    pub writes: BTreeSet<String>,
    pub calls: BTreeSet<String>,
    /// Parser errors, one per fragment that did not parse. Empty when everything parsed.
    pub errors: Vec<String>,
    /// Fragments attempted.
    pub fragments: usize,
}

impl Footprint {
    /// `none` (nothing to parse), `parsed`, `partial` (some fragments failed) or `unparsed`.
    pub fn status(&self) -> &'static str {
        match (self.fragments, self.errors.len()) {
            (0, _) => "none",
            (_, 0) => "parsed",
            (n, e) if e < n => "partial",
            _ => "unparsed",
        }
    }

    pub fn merge(&mut self, other: Footprint) {
        self.reads.extend(other.reads);
        self.writes.extend(other.writes);
        self.calls.extend(other.calls);
        self.errors.extend(other.errors);
        self.fragments += other.fragments;
    }
}

/// The footprint of one complete SQL statement (`SELECT …`, `UPDATE …`, …).
pub fn statement_footprint(sql: &str) -> Footprint {
    let mut fp = Footprint {
        fragments: 1,
        ..Default::default()
    };
    match Parser::parse_sql(&PostgreSqlDialect {}, sql) {
        Ok(stmts) => {
            let mut v = Collector::default();
            let _ = stmts.visit(&mut v);
            v.finish(&mut fp);
        }
        Err(e) => fp.errors.push(e.to_string()),
    }
    fp
}

/// The footprint of a PL/pgSQL expression — a condition, an assignment's right-hand side. It is
/// parsed as `SELECT <expr>`, which is exactly how PL/pgSQL itself evaluates one.
pub fn expression_footprint(expr: &str) -> Footprint {
    statement_footprint(&format!("SELECT {expr}"))
}

fn name(n: &ObjectName) -> String {
    n.0.iter()
        .map(|i| {
            if i.quote_style.is_some() {
                i.value.clone()
            } else {
                i.value.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

#[derive(Default)]
struct Collector {
    /// Every relation occurrence, in visiting order — a multiset, so a written table that is also
    /// read keeps its read.
    relations: Vec<String>,
    writes: Vec<String>,
    calls: BTreeSet<String>,
    ctes: BTreeSet<String>,
    /// The next relation the visitor reports is a table function's name, already counted as a call.
    skip_relation: Option<String>,
}

impl Collector {
    fn finish(mut self, fp: &mut Footprint) {
        for w in &self.writes {
            if let Some(at) = self.relations.iter().position(|r| r == w) {
                self.relations.remove(at);
            }
        }
        fp.writes.extend(self.writes);
        fp.reads.extend(
            self.relations
                .into_iter()
                .filter(|r| !self.ctes.contains(r)),
        );
        fp.calls.extend(self.calls);
    }

    fn write_target(&mut self, t: &TableWithJoins) {
        if let TableFactor::Table { name: n, .. } = &t.relation {
            self.writes.push(name(n));
        }
    }
}

impl Visitor for Collector {
    type Break = ();

    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<()> {
        if let Some(with) = &q.with {
            for cte in &with.cte_tables {
                self.ctes.insert(cte.alias.name.value.to_ascii_lowercase());
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, t: &TableFactor) -> ControlFlow<()> {
        if let TableFactor::Table {
            name: n,
            args: Some(_),
            ..
        } = t
        {
            let n = name(n);
            self.calls.insert(n.clone());
            self.skip_relation = Some(n);
        }
        if let TableFactor::Function { name: n, .. } = t {
            self.calls.insert(name(n));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, r: &ObjectName) -> ControlFlow<()> {
        let n = name(r);
        if self.skip_relation.as_deref() == Some(n.as_str()) {
            self.skip_relation = None;
        } else {
            self.relations.push(n);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        if let Expr::Function(f) = e {
            self.calls.insert(name(&f.name));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, s: &Statement) -> ControlFlow<()> {
        match s {
            Statement::Insert(i) => self.writes.push(name(&i.table_name)),
            Statement::Update { table, .. } => self.write_target(table),
            Statement::Delete(d) => {
                if d.tables.is_empty() {
                    let (FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from)) =
                        &d.from;
                    for t in from {
                        self.write_target(t);
                    }
                } else {
                    self.writes.extend(d.tables.iter().map(name));
                }
            }
            Statement::Truncate { table_names, .. } => {
                self.writes
                    .extend(table_names.iter().map(|t| name(&t.name)));
            }
            Statement::Merge {
                table: TableFactor::Table { name: n, .. },
                ..
            } => self.writes.push(name(n)),
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_select_reads_its_tables_and_calls_its_functions() {
        let fp = statement_footprint(
            "SELECT a.id, coalesce(sum(t.amount), 0) FROM acc_trans t \
             JOIN account a ON a.id = t.chart_id WHERE t.trans_id = in_id \
             AND EXISTS (SELECT 1 FROM Public.Transactions x WHERE x.id = t.trans_id)",
        );
        assert_eq!(fp.status(), "parsed");
        assert_eq!(
            fp.reads,
            set(&["acc_trans", "account", "public.transactions"])
        );
        assert!(fp.writes.is_empty());
        assert_eq!(fp.calls, set(&["coalesce", "sum"]));
    }

    #[test]
    fn dml_targets_are_writes_and_their_sources_are_reads() {
        let fp = statement_footprint(
            "INSERT INTO cr_report_line (report_id) SELECT id FROM cr_report WHERE id = in_id",
        );
        assert_eq!(fp.writes, set(&["cr_report_line"]));
        assert_eq!(fp.reads, set(&["cr_report"]));

        let fp = statement_footprint(
            "UPDATE invoice SET allocated = allocated + 1 FROM parts p WHERE p.id = invoice.parts_id",
        );
        assert_eq!(fp.writes, set(&["invoice"]));
        assert_eq!(fp.reads, set(&["parts"]));

        let fp = statement_footprint("DELETE FROM currency WHERE curr = in_curr");
        assert_eq!(fp.writes, set(&["currency"]));
        assert!(fp.reads.is_empty());

        let fp = statement_footprint("TRUNCATE lines_to_be_added");
        assert_eq!(fp.writes, set(&["lines_to_be_added"]));
    }

    /// `INSERT INTO t SELECT … FROM t` writes `t` and reads it; neither occurrence hides the other.
    #[test]
    fn a_table_written_and_read_is_both() {
        let fp = statement_footprint("INSERT INTO t SELECT * FROM t WHERE x > 0");
        assert_eq!(fp.writes, set(&["t"]));
        assert_eq!(fp.reads, set(&["t"]));
    }

    #[test]
    fn cte_names_are_not_tables() {
        let fp = statement_footprint(
            "WITH matched AS (UPDATE lines la SET x = 1 RETURNING id) \
             UPDATE cr_report_line SET y = 2 WHERE id IN (SELECT id FROM matched)",
        );
        assert_eq!(fp.writes, set(&["lines", "cr_report_line"]));
        assert!(fp.reads.is_empty(), "{:?}", fp.reads);
    }

    /// `FROM setting_get('x')` is a call to a set-returning function, not a table named
    /// `setting_get`.
    #[test]
    fn a_table_function_is_a_call_not_a_table() {
        let fp = statement_footprint("SELECT value FROM setting_get('decimal_places') s");
        assert!(fp.reads.is_empty(), "{:?}", fp.reads);
        assert_eq!(fp.calls, set(&["setting_get"]));
    }

    #[test]
    fn an_expression_is_parsed_as_plpgsql_evaluates_it() {
        let fp = expression_footprint("EXISTS (SELECT 1 FROM account WHERE id = new.chart_id)");
        assert_eq!(fp.status(), "parsed");
        assert_eq!(fp.reads, set(&["account"]));
        let fp = expression_footprint("person__get_my_entity_id()");
        assert_eq!(fp.calls, set(&["person__get_my_entity_id"]));
        assert!(fp.reads.is_empty());
    }

    /// What does not parse is reported with the parser's reason, and contributes nothing.
    #[test]
    fn an_unparseable_fragment_is_reported_not_guessed() {
        let fp = statement_footprint("SELECT FROM WHERE nonsense ((");
        assert_eq!(fp.status(), "unparsed");
        assert_eq!(fp.errors.len(), 1);
        assert!(fp.reads.is_empty() && fp.calls.is_empty());

        let mut both = statement_footprint("SELECT 1 FROM t");
        both.merge(statement_footprint("SELECT (("));
        assert_eq!(both.status(), "partial");
        assert_eq!(both.reads, set(&["t"]));
    }
}
