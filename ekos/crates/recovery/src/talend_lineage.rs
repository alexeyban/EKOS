//! RFC 0264 §"Phase 2": the parts of Talend column lineage that need more than the model — a
//! database input's SQL read with sqlparser ([`SqlQueries`]), and lineage linked across the jobs
//! of one project through the tables one writes and another reads ([`link_jobs`]).
//!
//! Pure and deterministic.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::ControlFlow;

use ekos_talend::lineage::{
    ColumnLineage, Origin, QueryReader, QueryShape, SelectColumn, TargetKind,
};
use sqlparser::ast::{
    Expr, Ident, SelectItem, SetExpr, Statement, TableFactor, TableWithJoins, Visit, Visitor,
};
use sqlparser::dialect::{Dialect, GenericDialect, PostgreSqlDialect};
use sqlparser::parser::Parser;

/// Reads a SELECT's projection with sqlparser: PostgreSQL first, then the generic dialect.
pub struct SqlQueries;

impl QueryReader for SqlQueries {
    fn read(&self, sql: &str) -> Option<QueryShape> {
        let sql = sql.trim().trim_end_matches(';').trim();
        let dialects: [&dyn Dialect; 2] = [&PostgreSqlDialect {}, &GenericDialect {}];
        dialects
            .iter()
            .find_map(|d| Parser::parse_sql(*d, sql).ok())
            .and_then(|statements| shape(&statements))
    }
}

fn shape(statements: &[Statement]) -> Option<QueryShape> {
    let [Statement::Query(q)] = statements else {
        return None;
    };
    let SetExpr::Select(select) = q.body.as_ref() else {
        return None;
    };
    let tables = from_tables(&select.from);
    let columns = select
        .projection
        .iter()
        .map(|item| select_column(item, &tables))
        .collect::<Option<Vec<_>>>()?;
    Some(QueryShape {
        columns,
        filter: select.selection.as_ref().map(ToString::to_string),
    })
}

/// Alias (or table name) → table name, for every table the FROM clause reads.
fn from_tables(from: &[TableWithJoins]) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let factors = from
        .iter()
        .flat_map(|t| std::iter::once(&t.relation).chain(t.joins.iter().map(|j| &j.relation)));
    for f in factors {
        if let TableFactor::Table { name, alias, .. } = f
            && let Some(table) = name.0.last()
        {
            out.insert(table.value.clone(), table.value.clone());
            if let Some(a) = alias {
                out.insert(a.name.value.clone(), table.value.clone());
            }
        }
    }
    out
}

/// One select item; `None` for a wildcard, whose columns cannot be matched by position.
fn select_column(item: &SelectItem, tables: &BTreeMap<String, String>) -> Option<SelectColumn> {
    let expr = match item {
        SelectItem::UnnamedExpr(e) | SelectItem::ExprWithAlias { expr: e, .. } => e,
        _ => return None,
    };
    let single: Option<&String> = {
        let distinct: BTreeSet<&String> = tables.values().collect();
        (distinct.len() == 1).then(|| *distinct.iter().next().expect("one table"))
    };
    let mut names = Names::default();
    let _ = expr.visit(&mut names);
    let sources = names
        .0
        .iter()
        .filter_map(|parts| source(parts, tables, single))
        .fold(Vec::new(), |mut v, s| {
            if !v.contains(&s) {
                v.push(s);
            }
            v
        });
    Some(SelectColumn {
        expression: expr.to_string(),
        plain: matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_)),
        sources,
    })
}

/// `(table, column)` for a column reference: `[…, table, column]` or `[column]`.
fn source(
    parts: &[Ident],
    tables: &BTreeMap<String, String>,
    single: Option<&String>,
) -> Option<(Option<String>, String)> {
    let column = parts.last()?.value.clone();
    let table = match parts.len() {
        1 => single.cloned(),
        n => {
            let q = &parts[n - 2].value;
            Some(tables.get(q).cloned().unwrap_or_else(|| q.clone()))
        }
    };
    Some((table, column))
}

/// Every column reference in an expression, as written.
#[derive(Default)]
struct Names(Vec<Vec<Ident>>);

impl Visitor for Names {
    type Break = ();

    fn pre_visit_expr(&mut self, e: &Expr) -> ControlFlow<()> {
        match e {
            Expr::Identifier(i) => self.0.push(vec![i.clone()]),
            Expr::CompoundIdentifier(parts) => self.0.push(parts.clone()),
            _ => {}
        }
        ControlFlow::Continue(())
    }
}

/// One job version's lineages, for linking.
pub struct JobLineages<'a> {
    pub job: &'a str,
    pub latest: bool,
    pub lineages: &'a [ColumnLineage],
}

/// A link between two lineages: `from` (in job `from.0`, index `from.1`) writes the table column
/// `via`, which `to` reads.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Feed {
    pub from: (usize, usize),
    pub to: (usize, usize),
    pub via: String,
}

/// What [`link_jobs`] finds.
#[derive(Debug, Default)]
pub struct Linked {
    pub feeds: Vec<Feed>,
    /// Per (job index, lineage index): the origins reached across jobs (tables nobody in the
    /// project writes, files, generated values), and the jobs crossed on the way.
    pub upstream: BTreeMap<(usize, usize), (Vec<Origin>, BTreeSet<String>)>,
}

/// `name_key`'s rule (RFC 0075): case and quotes ignored, part by part.
fn key(name: &str) -> String {
    name.split('.')
        .map(|p| p.trim().trim_matches(['"', '`', '[', ']']).to_lowercase())
        .collect::<Vec<_>>()
        .join(".")
}

/// The schema rule: equal, or missing on one side.
fn schemas_agree(a: Option<&String>, b: Option<&String>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => key(a) == key(b),
        _ => true,
    }
}

/// Links lineages across the latest job versions of one project: a lineage whose target is the
/// table column `T.c` feeds every lineage of another job with an origin `T.c`.
pub fn link_jobs(jobs: &[JobLineages]) -> Linked {
    let mut writers: BTreeMap<(String, String), Vec<(usize, usize)>> = BTreeMap::new();
    for (j, job) in jobs.iter().enumerate().filter(|(_, j)| j.latest) {
        for (i, l) in job.lineages.iter().enumerate() {
            if l.target.kind == TargetKind::Table && !l.update_key {
                writers
                    .entry((key(&l.target.name), key(&l.target.column)))
                    .or_default()
                    .push((j, i));
            }
        }
    }
    let writes = |o: &Origin, reader_job: usize| -> Vec<(usize, usize)> {
        let Origin::Table {
            table,
            schema,
            column,
            ..
        } = o
        else {
            return Vec::new();
        };
        writers
            .get(&(key(table), key(column)))
            .map(|ws| {
                ws.iter()
                    .copied()
                    .filter(|&(j, i)| {
                        j != reader_job
                            && schemas_agree(
                                jobs[j].lineages[i].target.schema.as_ref(),
                                schema.as_ref(),
                            )
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut linked = Linked::default();
    for (j, job) in jobs.iter().enumerate().filter(|(_, j)| j.latest) {
        for (i, l) in job.lineages.iter().enumerate() {
            for o in &l.origins {
                for w in writes(o, j) {
                    linked.feeds.push(Feed {
                        from: w,
                        to: (j, i),
                        via: o.label(),
                    });
                }
            }
        }
    }
    linked.feeds.sort();
    linked.feeds.dedup();
    for (j, job) in jobs.iter().enumerate().filter(|(_, j)| j.latest) {
        for i in 0..job.lineages.len() {
            let mut origins = BTreeSet::new();
            let mut crossed = BTreeSet::new();
            let mut seen = BTreeSet::from([(j, i)]);
            upstream_of(jobs, &writes, (j, i), &mut seen, &mut origins, &mut crossed);
            linked
                .upstream
                .insert((j, i), (origins.into_iter().collect(), crossed));
        }
    }
    linked
}

/// The lineages (job index, lineage index) that write an origin read by a job.
type Writers<'w> = dyn Fn(&Origin, usize) -> Vec<(usize, usize)> + 'w;

fn upstream_of(
    jobs: &[JobLineages],
    writes: &Writers,
    at: (usize, usize),
    seen: &mut BTreeSet<(usize, usize)>,
    origins: &mut BTreeSet<Origin>,
    crossed: &mut BTreeSet<String>,
) {
    for o in &jobs[at.0].lineages[at.1].origins {
        let ws = writes(o, at.0);
        if ws.is_empty() {
            origins.insert(o.clone());
            continue;
        }
        for w in ws {
            if seen.insert(w) {
                crossed.insert(jobs[w.0].job.to_string());
                upstream_of(jobs, writes, w, seen, origins, crossed);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_talend::lineage::Target;

    #[test]
    fn a_select_list_maps_item_by_item_with_its_where() {
        let s = SqlQueries
            .read(
                r#"SELECT "ODS"."ODS_VENTE"."QTE_VENTE", v.PRIX * 2 AS P FROM "ODS"."ODS_VENTE" v WHERE "BL" = False;"#,
            )
            .unwrap();
        assert_eq!(s.columns.len(), 2);
        assert!(s.columns[0].plain);
        assert_eq!(
            s.columns[0].sources,
            [(Some("ODS_VENTE".into()), "QTE_VENTE".into())]
        );
        assert!(!s.columns[1].plain);
        assert_eq!(
            s.columns[1].sources,
            [(Some("ODS_VENTE".into()), "PRIX".into())]
        );
        assert_eq!(s.filter.as_deref(), Some(r#""BL" = false"#));
    }

    #[test]
    fn a_wildcard_or_two_statements_give_nothing() {
        assert!(SqlQueries.read("SELECT * FROM t").is_none());
        assert!(
            SqlQueries
                .read("SELECT a FROM t; SELECT b FROM u")
                .is_none()
        );
        assert!(SqlQueries.read("not sql at all").is_none());
    }

    fn lineage(table: &str, column: &str, from: &[(&str, &str)]) -> ColumnLineage {
        ColumnLineage {
            target: Target {
                kind: TargetKind::Table,
                name: table.into(),
                schema: None,
                column: column.into(),
            },
            component: "out".into(),
            data_action: Some("INSERT".into()),
            update_key: false,
            formula: String::new(),
            formula_truncated: false,
            steps: Vec::new(),
            origins: from
                .iter()
                .map(|(t, c)| Origin::Table {
                    table: (*t).into(),
                    schema: None,
                    column: (*c).into(),
                    component: "in".into(),
                    by_name: false,
                })
                .collect(),
            gaps: Vec::new(),
        }
    }

    #[test]
    fn jobs_link_through_the_tables_they_write_and_read_back_to_the_first_source() {
        let ods = [lineage("ODS", "QTY", &[("RAW", "qty")])];
        let fact = [lineage("FACT", "TOTAL", &[("\"ods\"", "QTY")])];
        let old = [lineage("FACT", "TOTAL", &[("ODS", "QTY")])];
        let jobs = [
            JobLineages {
                job: "jOds",
                latest: true,
                lineages: &ods,
            },
            JobLineages {
                job: "jFact",
                latest: true,
                lineages: &fact,
            },
            JobLineages {
                job: "jFact",
                latest: false,
                lineages: &old,
            },
        ];
        let linked = link_jobs(&jobs);
        assert_eq!(
            linked.feeds,
            [Feed {
                from: (0, 0),
                to: (1, 0),
                via: "\"ods\".QTY".into()
            }]
        );
        let (origins, crossed) = &linked.upstream[&(1, 0)];
        assert_eq!(origins.len(), 1);
        assert_eq!(origins[0].label(), "RAW.qty");
        assert_eq!(crossed.iter().collect::<Vec<_>>(), ["jOds"]);
        assert!(
            !linked.upstream.contains_key(&(2, 0)),
            "old versions are not linked"
        );
    }
}
