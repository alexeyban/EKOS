//! RFC 0157 — live-vs-repository drift, down to the column.
//!
//! Comparing a live catalog against the DDL a repository checked in is the cheapest valuable thing
//! a migration does: drift is where the surprises live, and it costs one catalog read.
//!
//! It is also where a naive comparison produces nothing but noise, for two reasons this module
//! exists to handle.
//!
//! **Names.** The live catalog always qualifies a table (`public.orders`). A repository's DDL
//! sometimes does and sometimes does not — `ekos_recovery`'s SQL analyzer keys a `Table` object on
//! whatever `CREATE TABLE` wrote, and its own comment-matching code already notes the same mismatch
//! one layer down. A full-string comparison therefore reports *every* table as both live-only and
//! repo-only on any repository that writes unqualified DDL.
//!
//! **Types.** `format_type()` renders `character varying(50)`; `sqlparser` renders `VARCHAR(50)`.
//! The same column, two spellings. A textual comparison marks every column as drifted.
//!
//! The governing rule for both: **when the comparison cannot be made confidently, report nothing
//! rather than a difference.** A false drift finding costs human review time and teaches people to
//! ignore drift, which is worse than the finding being absent.

use serde::{Deserialize, Serialize};

/// A table name, normalized for comparison.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableRef {
    pub schema: Option<String>,
    pub name: String,
}

impl TableRef {
    /// Parse `schema.table`, `"Schema"."Table"` or a bare `table`. Case is folded and quotes are
    /// stripped, because neither carries meaning across the two sources being compared.
    pub fn parse(raw: &str) -> Self {
        let clean = |s: &str| s.trim().trim_matches('"').to_ascii_lowercase();
        match raw.rsplit_once('.') {
            Some((schema, name)) => Self {
                schema: Some(clean(schema)),
                name: clean(name),
            },
            None => Self {
                schema: None,
                name: clean(raw),
            },
        }
    }

    pub fn display(&self) -> String {
        match &self.schema {
            Some(s) => format!("{s}.{}", self.name),
            None => self.name.clone(),
        }
    }
}

/// One column, as either side describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnRef {
    pub name: String,
    /// The raw rendering, kept for the finding's detail text.
    pub data_type: String,
}

/// A table with its columns, from either side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableShape {
    pub table: TableRef,
    pub columns: Vec<ColumnRef>,
}

/// A type reduced to something comparable across two renderings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedType {
    pub base: &'static str,
    /// `(precision, scale)` for numeric, `(length, _)` for character types. `None` when the
    /// rendering did not state one — which is *not* the same as zero and never compares equal to a
    /// stated one.
    pub params: Option<(u32, Option<u32>)>,
}

/// Reduce a type rendering to a comparable form, or `None` when it is not recognized.
///
/// `None` is a real answer and the caller must treat it as "cannot compare", never as "different".
/// The alias table below covers what PostgreSQL's `format_type` and `sqlparser`'s `Display` produce
/// between them; anything outside it is a gap to fill deliberately rather than guess at.
pub fn normalize_type(raw: &str) -> Option<NormalizedType> {
    let lower = raw.trim().to_ascii_lowercase();
    let (head, params) = match lower.split_once('(') {
        Some((h, rest)) => {
            let inner = rest.trim_end_matches(')');
            let mut it = inner.split(',').map(|p| p.trim().parse::<u32>().ok());
            let first = it.next().flatten();
            let second = it.next().flatten();
            (h.trim().to_string(), first.map(|f| (f, second)))
        }
        None => (lower.clone(), None),
    };
    // `character varying` and friends carry a space; `timestamp with time zone` carries several.
    let head = head.split_whitespace().collect::<Vec<_>>().join(" ");

    let base = match head.as_str() {
        "bigint" | "int8" | "bigserial" | "serial8" => "bigint",
        "integer" | "int" | "int4" | "serial" | "serial4" => "integer",
        "smallint" | "int2" | "smallserial" | "serial2" => "smallint",
        "numeric" | "decimal" => "numeric",
        "real" | "float4" => "real",
        "double precision" | "float8" => "double precision",
        "boolean" | "bool" => "boolean",
        "text" => "text",
        "character varying" | "varchar" => "varchar",
        "character" | "char" | "bpchar" => "char",
        "timestamp with time zone" | "timestamptz" => "timestamptz",
        "timestamp without time zone" | "timestamp" => "timestamp",
        "time with time zone" | "timetz" => "timetz",
        "time without time zone" | "time" => "time",
        "date" => "date",
        "uuid" => "uuid",
        "json" => "json",
        "jsonb" => "jsonb",
        "bytea" => "bytea",
        "inet" => "inet",
        "cidr" => "cidr",
        "interval" => "interval",
        _ => return None,
    };
    Some(NormalizedType { base, params })
}

/// Whether two renderings describe the same type.
///
/// `None` means "cannot tell" — at least one side did not normalize — and the caller reports no
/// drift for it. A `text` column and an unrecognized domain type are not evidence of a change.
///
/// A stated parameter never compares equal to an absent one: `numeric` and `numeric(12,2)` really
/// are different, and that difference is exactly what RFC 0159 needs to know about.
pub fn types_match(a: &str, b: &str) -> Option<bool> {
    match (normalize_type(a), normalize_type(b)) {
        (Some(x), Some(y)) => Some(x == y),
        _ => None,
    }
}

/// What a single drift finding says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftDetail {
    TableLiveOnly,
    TableRepoOnly,
    /// A bare repository name matches more than one live table. Reported rather than resolved:
    /// picking one would silently compare the wrong table.
    TableAmbiguous,
    ColumnLiveOnly,
    ColumnRepoOnly,
    ColumnTypeDiffers,
}

impl DriftDetail {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TableLiveOnly => "table_live_only",
            Self::TableRepoOnly => "table_repo_only",
            Self::TableAmbiguous => "table_ambiguous",
            Self::ColumnLiveOnly => "column_live_only",
            Self::ColumnRepoOnly => "column_repo_only",
            Self::ColumnTypeDiffers => "column_type_differs",
        }
    }

    /// `true` where the difference changes what a migration would produce, rather than only what it
    /// would be named. RFC 0158 blocks on these at R3; the rest inform.
    pub fn is_structural(self) -> bool {
        matches!(
            self,
            Self::ColumnLiveOnly
                | Self::ColumnRepoOnly
                | Self::ColumnTypeDiffers
                | Self::TableLiveOnly
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drift {
    /// The live-side name where there is one, the repository's otherwise.
    pub object: String,
    pub detail: DriftDetail,
    pub message: String,
}

/// Compare the live catalog against the repository's, tables and columns.
pub fn reconcile(live: &[TableShape], repo: &[TableShape]) -> Vec<Drift> {
    let mut out = Vec::new();
    let mut matched_live: std::collections::BTreeSet<TableRef> = Default::default();

    for r in repo {
        match match_table(&r.table, live) {
            Match::One(l) => {
                matched_live.insert(l.table.clone());
                out.extend(compare_columns(l, r));
            }
            Match::None => out.push(Drift {
                object: r.table.display(),
                detail: DriftDetail::TableRepoOnly,
                message: "in the repository's DDL but not deployed".into(),
            }),
            Match::Many(names) => out.push(Drift {
                object: r.table.display(),
                detail: DriftDetail::TableAmbiguous,
                message: format!(
                    "unqualified in the repository and matches {} live tables ({}). Qualify it in \
                     the DDL, or restrict discovery to one schema.",
                    names.len(),
                    names.join(", ")
                ),
            }),
        }
    }

    for l in live {
        if !matched_live.contains(&l.table) {
            out.push(Drift {
                object: l.table.display(),
                detail: DriftDetail::TableLiveOnly,
                message: "deployed but absent from the repository's DDL".into(),
            });
        }
    }

    out.sort_by(|a, b| {
        (a.object.as_str(), a.detail.as_str()).cmp(&(b.object.as_str(), b.detail.as_str()))
    });
    out
}

enum Match<'a> {
    None,
    One(&'a TableShape),
    Many(Vec<String>),
}

/// Match a repository table against the live catalog.
///
/// A qualified name must match schema and table. A bare name matches on table alone — and if that
/// hits more than one live table, the result is ambiguous rather than the first hit.
fn match_table<'a>(want: &TableRef, live: &'a [TableShape]) -> Match<'a> {
    if want.schema.is_some() {
        return match live.iter().find(|l| &l.table == want) {
            Some(l) => Match::One(l),
            None => Match::None,
        };
    }
    let candidates: Vec<&TableShape> = live.iter().filter(|l| l.table.name == want.name).collect();
    match candidates.len() {
        0 => Match::None,
        1 => Match::One(candidates[0]),
        _ => Match::Many(candidates.iter().map(|c| c.table.display()).collect()),
    }
}

fn compare_columns(live: &TableShape, repo: &TableShape) -> Vec<Drift> {
    let key = |c: &ColumnRef| c.name.to_ascii_lowercase();
    let mut out = Vec::new();

    for r in &repo.columns {
        match live.columns.iter().find(|l| key(l) == key(r)) {
            None => out.push(Drift {
                object: format!("{}.{}", live.table.display(), r.name),
                detail: DriftDetail::ColumnRepoOnly,
                message: "in the repository's DDL but not deployed".into(),
            }),
            Some(l) => {
                // `None` from `types_match` means at least one rendering is unrecognized, and an
                // unrecognized rendering is not evidence of a change.
                if types_match(&l.data_type, &r.data_type) == Some(false) {
                    out.push(Drift {
                        object: format!("{}.{}", live.table.display(), r.name),
                        detail: DriftDetail::ColumnTypeDiffers,
                        message: format!(
                            "live is {}, the repository's DDL says {}",
                            l.data_type, r.data_type
                        ),
                    });
                }
            }
        }
    }

    for l in &live.columns {
        if !repo.columns.iter().any(|r| key(r) == key(l)) {
            out.push(Drift {
                object: format!("{}.{}", live.table.display(), l.name),
                detail: DriftDetail::ColumnLiveOnly,
                message: "deployed but absent from the repository's DDL".into(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(name: &str, cols: &[(&str, &str)]) -> TableShape {
        TableShape {
            table: TableRef::parse(name),
            columns: cols
                .iter()
                .map(|(n, d)| ColumnRef {
                    name: n.to_string(),
                    data_type: d.to_string(),
                })
                .collect(),
        }
    }

    // ── names ────────────────────────────────────────────────────────────────

    /// The bug a full-string comparison has: a repository writing `CREATE TABLE orders` against a
    /// live catalog that always qualifies would report every table twice, as both live-only and
    /// repo-only.
    #[test]
    fn a_bare_repository_name_matches_a_qualified_live_one() {
        let live = [t("public.orders", &[("id", "bigint")])];
        let repo = [t("orders", &[("id", "BIGINT")])];
        assert!(
            reconcile(&live, &repo).is_empty(),
            "a bare repo name must match the qualified live table"
        );
    }

    #[test]
    fn quoting_and_case_are_not_drift() {
        let live = [t("public.orders", &[("id", "bigint")])];
        let repo = [t("\"Public\".\"ORDERS\"", &[("ID", "bigint")])];
        assert!(reconcile(&live, &repo).is_empty());
    }

    /// A bare name that could be either table is reported, not resolved. Picking the first would
    /// silently compare the wrong table's columns.
    #[test]
    fn an_ambiguous_bare_name_is_a_finding() {
        let live = [
            t("public.orders", &[("id", "bigint")]),
            t("archive.orders", &[("id", "bigint")]),
        ];
        let repo = [t("orders", &[("id", "bigint")])];
        let d = reconcile(&live, &repo);
        let amb: Vec<_> = d
            .iter()
            .filter(|x| x.detail == DriftDetail::TableAmbiguous)
            .collect();
        assert_eq!(amb.len(), 1, "{d:?}");
        assert!(
            amb[0].message.contains("archive.orders"),
            "{}",
            amb[0].message
        );
        assert!(amb[0].message.contains("public.orders"));
    }

    #[test]
    fn a_qualified_repository_name_does_not_match_another_schema() {
        let live = [t("public.orders", &[])];
        let repo = [t("archive.orders", &[])];
        let d = reconcile(&live, &repo);
        assert_eq!(d.len(), 2);
        assert!(d.iter().any(|x| x.detail == DriftDetail::TableRepoOnly));
        assert!(d.iter().any(|x| x.detail == DriftDetail::TableLiveOnly));
    }

    // ── types ────────────────────────────────────────────────────────────────

    /// The other half of the noise problem: `format_type` and `sqlparser` spell the same type
    /// differently, so a textual comparison marks every column as drifted.
    #[test]
    fn the_two_renderings_of_the_same_type_agree() {
        for (live, repo) in [
            ("character varying(50)", "VARCHAR(50)"),
            ("bigint", "BIGINT"),
            ("integer", "INT"),
            ("timestamp with time zone", "TIMESTAMPTZ"),
            ("timestamp without time zone", "TIMESTAMP"),
            ("numeric(12,2)", "DECIMAL(12,2)"),
            ("boolean", "BOOL"),
            ("double precision", "FLOAT8"),
            ("bytea", "BYTEA"),
        ] {
            assert_eq!(
                types_match(live, repo),
                Some(true),
                "{live:?} should match {repo:?}"
            );
        }
    }

    /// A `serial` column is an `integer` once deployed. Reporting that as drift would flag a column
    /// in every table that has one.
    #[test]
    fn serial_normalizes_to_its_deployed_type() {
        assert_eq!(types_match("integer", "SERIAL"), Some(true));
        assert_eq!(types_match("bigint", "BIGSERIAL"), Some(true));
    }

    #[test]
    fn real_type_changes_are_reported() {
        assert_eq!(types_match("integer", "BIGINT"), Some(false));
        assert_eq!(types_match("numeric(12,2)", "NUMERIC(12,4)"), Some(false));
        assert_eq!(
            types_match("character varying(50)", "VARCHAR(100)"),
            Some(false)
        );
    }

    /// An unconstrained `numeric` and a `numeric(12,2)` are genuinely different, and it is exactly
    /// the difference RFC 0159 needs, so an absent parameter never compares equal to a stated one.
    #[test]
    fn an_absent_parameter_is_not_a_wildcard() {
        assert_eq!(types_match("numeric", "NUMERIC(12,2)"), Some(false));
    }

    /// The conservative rule. An unrecognized rendering — a domain, an enum, a PostGIS type — is
    /// not evidence of a change, and reporting one would train people to ignore drift.
    #[test]
    fn an_unrecognized_type_reports_no_drift_rather_than_a_difference() {
        assert_eq!(normalize_type("ekos_cat.mood"), None);
        assert_eq!(types_match("ekos_cat.mood", "MOOD"), None);
        assert_eq!(types_match("geometry(Point,4326)", "GEOMETRY"), None);

        let live = [t("public.t", &[("status", "ekos_cat.mood")])];
        let repo = [t("public.t", &[("status", "MOOD")])];
        assert!(
            reconcile(&live, &repo).is_empty(),
            "an unrecognized type pair must not produce a finding"
        );
    }

    // ── columns ──────────────────────────────────────────────────────────────

    #[test]
    fn column_differences_are_reported_in_both_directions() {
        let live = [t(
            "public.orders",
            &[
                ("id", "bigint"),
                ("total", "numeric(12,2)"),
                ("added_live", "text"),
            ],
        )];
        let repo = [t(
            "orders",
            &[
                ("id", "BIGINT"),
                ("total", "NUMERIC(12,4)"),
                ("only_in_repo", "TEXT"),
            ],
        )];
        let d = reconcile(&live, &repo);
        let by = |k: DriftDetail| d.iter().filter(|x| x.detail == k).collect::<Vec<_>>();

        assert_eq!(by(DriftDetail::ColumnTypeDiffers).len(), 1);
        assert!(
            by(DriftDetail::ColumnTypeDiffers)[0]
                .object
                .ends_with(".total")
        );
        assert!(
            by(DriftDetail::ColumnTypeDiffers)[0]
                .message
                .contains("12,2")
                && by(DriftDetail::ColumnTypeDiffers)[0]
                    .message
                    .contains("12,4"),
            "the finding must name both renderings: {}",
            by(DriftDetail::ColumnTypeDiffers)[0].message
        );

        assert_eq!(by(DriftDetail::ColumnRepoOnly).len(), 1);
        assert!(
            by(DriftDetail::ColumnRepoOnly)[0]
                .object
                .ends_with(".only_in_repo")
        );
        assert_eq!(by(DriftDetail::ColumnLiveOnly).len(), 1);
        assert!(
            by(DriftDetail::ColumnLiveOnly)[0]
                .object
                .ends_with(".added_live")
        );

        // A column finding is named on the *live* table, so it is actionable against the database
        // that exists.
        assert!(d.iter().all(|x| x.object.starts_with("public.orders")));
    }

    #[test]
    fn a_column_only_added_live_is_structural() {
        assert!(DriftDetail::ColumnLiveOnly.is_structural());
        assert!(DriftDetail::ColumnTypeDiffers.is_structural());
        assert!(DriftDetail::TableLiveOnly.is_structural());
        assert!(!DriftDetail::TableRepoOnly.is_structural());
        assert!(!DriftDetail::TableAmbiguous.is_structural());
    }

    #[test]
    fn identical_catalogs_produce_nothing() {
        let a = [
            t(
                "public.orders",
                &[("id", "bigint"), ("total", "numeric(12,2)")],
            ),
            t("public.customers", &[("id", "bigint")]),
        ];
        let repo = [
            t(
                "public.orders",
                &[("id", "BIGINT"), ("total", "NUMERIC(12,2)")],
            ),
            t("public.customers", &[("id", "BIGINT")]),
        ];
        assert!(reconcile(&a, &repo).is_empty());
    }

    #[test]
    fn an_empty_repository_reports_every_live_table_once() {
        let live = [t("public.a", &[]), t("public.b", &[])];
        let d = reconcile(&live, &[]);
        assert_eq!(d.len(), 2);
        assert!(d.iter().all(|x| x.detail == DriftDetail::TableLiveOnly));
    }
}
