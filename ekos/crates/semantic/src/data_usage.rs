//! RFC 0252 §3.6 — what the data says about the values code names, folded into RFC 0170's items.
//!
//! `ekos profile run` measures every coded column on the live databases and writes
//! `<state>/profile/current.json` ([`DataSignals`]). At the next semantics step (the same run, or
//! any later `ekos commit`), [`fold`] adds to each `EnumMeaning` (and each unexplained-value
//! `SemanticGap`) a `data_usage` list — rows, share, first/last month, `dead`, `never_seen` per
//! database — and makes one `SemanticGap` with `gap_type: data_only_values` per column whose data
//! holds values no code names.
//!
//! Nothing here changes what an item asserts: `data_usage` is not a signed field
//! (`semantics_review::signature`), and a data-only gap's evidence names the measurement, not its
//! counts, so a re-profile never stales a review. No data value is in the input: the file holds
//! counts per literal recovered from code, and counts for "everything else".

use crate::business_semantics::{
    BlameHit, DESCRIBES, ENUM_MEANING, GAP, HYPOTHESIS, RationaleSource, SemanticsOutput,
    kind_name, object_kind,
};
use ekos_kir::{KirEvidence, KirId, KirObject, KirRelationship, RelationshipKind, SourceLocation};
use ekos_observation_sdk::signals::{Coverage, KeySearch, months_between};
use ekos_observation_sdk::usage::TableUsage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// `gap_type` of a column whose data holds values no code names.
pub const DATA_ONLY: &str = "data_only_values";

/// Months without a row before a code is `dead`, unless the file says otherwise.
pub const DEFAULT_DEAD_AFTER_MONTHS: i64 = 24;

fn default_dead_after() -> i64 {
    DEFAULT_DEAD_AFTER_MONTHS
}

/// `<state>/profile/current.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DataSignals {
    pub rfc: String,
    /// When the run measured (RFC 3339).
    pub measured_at: String,
    #[serde(default = "default_dead_after")]
    pub dead_after_months: i64,
    #[serde(default)]
    pub columns: Vec<ColumnSignal>,
    #[serde(default)]
    pub grain: Vec<GrainSignal>,
    /// §3.2(c): `column = value ⇒ other IS [NOT] NULL`, per table.
    #[serde(default)]
    pub implications: Vec<ImplicationSignal>,
    /// §3.2(e): validity periods and soft-delete flags.
    #[serde(default)]
    pub history: Vec<HistorySignal>,
    /// §3.5: which statements touch which tables, from the database's own workload statistics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usage: Vec<UsageSignal>,
}

/// One table's usage evidence from one source on one live database (counts only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageSignal {
    pub database: String,
    /// What the counts cover (`UsageSource::scope`).
    pub scope: String,
    /// The read counts include EKOS's own queries (a counter cannot tell who read).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub includes_own_reads: bool,
    #[serde(flatten)]
    pub table: TableUsage,
}

/// One implication on one live table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ImplicationSignal {
    pub database: String,
    pub owner: String,
    pub table: String,
    pub column: String,
    pub value: String,
    pub then_column: String,
    pub then: String,
    pub support: i64,
}

/// One history or deletion pattern measured on one live table.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistorySignal {
    pub database: String,
    pub owner: String,
    pub table: String,
    /// `validity` or `flag`.
    pub kind: String,
    pub columns: Vec<String>,
    pub rows: i64,
    /// `validity`: rows with an open end; `flag`: rows whose flag is set.
    pub measured: i64,
}

/// Adds `data_history` to each `WriteBehaviour` (RFC 0212) of a table the signals measured — as
/// evidence beside what code says, never a second write-behaviour model. Returns how many changed.
pub fn attach_history(objects: &mut [KirObject], signals: &DataSignals) -> usize {
    let mut n = 0;
    for o in objects.iter_mut().filter(
        |o| matches!(&o.kind, ekos_kir::ObjectKind::Custom(k) if k == crate::write_behaviour::KIND),
    ) {
        let table = lc(prop(o, "table"));
        let entries: Vec<Value> = signals
            .history
            .iter()
            .filter(|h| lc(&h.table) == table)
            .map(|h| {
                json!({
                    "database": h.database, "owner": h.owner, "kind": h.kind, "columns": h.columns,
                    "rows": h.rows, "measured": h.measured,
                    "meaning": if h.kind == "validity" { "rows whose period is still open" } else { "rows whose flag is set" },
                })
            })
            .collect();
        if entries.is_empty() {
            continue;
        }
        let v = Value::Array(entries);
        if o.properties.get("data_history") != Some(&v) {
            o.properties.insert("data_history".into(), v);
            n += 1;
        }
    }
    n
}

/// One coded column on one live database.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnSignal {
    pub database: String,
    pub owner: String,
    pub table: String,
    pub column: String,
    pub tier: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_column: Option<String>,
    pub coverage: Coverage,
    /// A declared foreign key the column follows (`table.column`): its values are the parent's
    /// rows, explained there — no `data_only_values` gap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<String>,
}

/// One table's grain: its declared key, or what a key search measured.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GrainSignal {
    pub database: String,
    pub owner: String,
    pub table: String,
    pub rows: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<KeySearch>,
}

/// What [`fold`] did, for the commit summary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FoldStats {
    pub values_measured: usize,
    pub dead: usize,
    pub never_seen: usize,
    pub data_only_gaps: usize,
}

fn lc(s: &str) -> String {
    s.rsplit('.').next().unwrap_or(s).to_lowercase()
}

fn share(n: i64, of: i64) -> f64 {
    if of <= 0 {
        0.0
    } else {
        ((n as f64 / of as f64) * 10_000.0).round() / 10_000.0
    }
}

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

fn prop<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The signals that apply to an item about `table.column` whose subject is `subject`: only that
/// database's when the subject is a live table, every database's otherwise. Sorted by database.
fn signals_for<'a>(
    signals: &'a DataSignals,
    table: &str,
    column: &str,
    subject: Option<&KirObject>,
) -> Vec<&'a ColumnSignal> {
    let live_db = subject
        .filter(|t| t.properties.get("origin") == Some(&json!("live")))
        .and_then(|t| t.properties.get("database").and_then(Value::as_str));
    let mut v: Vec<&ColumnSignal> = signals
        .columns
        .iter()
        .filter(|c| lc(&c.table) == table && c.column.to_lowercase() == column)
        .filter(|c| live_db.is_none_or(|db| db == c.database))
        .collect();
    v.sort_by(|a, b| (&a.database, &a.owner).cmp(&(&b.database, &b.owner)));
    v
}

/// One `data_usage` entry for `value` on one database.
fn usage_entry(c: &ColumnSignal, value: &str, dead_after: i64) -> Option<Value> {
    let u = c.coverage.literals.get(value)?;
    let dead = u.rows > 0
        && match (&u.last_seen, &c.coverage.latest) {
            (Some(last), Some(latest)) => {
                months_between(last, latest).is_some_and(|m| m > dead_after)
            }
            _ => false,
        };
    let mut e = json!({
        "database": c.database, "owner": c.owner, "table": c.table, "tier": c.tier,
        "rows": u.rows, "share": share(u.rows, c.coverage.non_null),
        "column_rows": c.coverage.non_null,
        "dead": dead, "never_seen": u.rows == 0,
    });
    for (k, v) in [
        ("first_seen", &u.first_seen),
        ("last_seen", &u.last_seen),
        ("latest_in_column", &c.coverage.latest),
        ("time_column", &c.time_column),
    ] {
        if let Some(v) = v {
            e[k] = json!(v);
        }
    }
    Some(e)
}

/// Adds `data_usage` to `EnumMeaning`s and unexplained-value gaps, and appends `data_only_values`
/// gaps. `subjects` are the objects items describe (tables), by id.
pub fn fold(
    out: &mut SemanticsOutput,
    subjects: &HashMap<KirId, &KirObject>,
    signals: &DataSignals,
) -> FoldStats {
    let mut stats = FoldStats::default();
    let describes: HashMap<KirId, KirId> = out
        .relationships
        .iter()
        .filter(|r| r.kind == RelationshipKind::Custom(DESCRIBES.into()))
        .map(|r| (r.from, r.to))
        .collect();
    // (subject id, column) of every coded column met, for the data-only gaps.
    let mut columns: BTreeMap<(String, String), (KirId, String)> = BTreeMap::new();
    for o in &mut out.objects {
        let k = kind_name(o);
        let is_value_gap = k == Some(GAP) && prop(o, "gap_type") == "unexplained_value";
        if k != Some(ENUM_MEANING) && !is_value_gap {
            continue;
        }
        let (table, column, value) = (
            lc(prop(o, "table")),
            prop(o, "column").to_lowercase(),
            prop(o, "value").to_string(),
        );
        let subject_id = describes.get(&o.id).copied();
        let subject = subject_id.and_then(|id| subjects.get(&id).copied());
        let sigs = signals_for(signals, &table, &column, subject);
        if let Some(sid) = subject_id {
            columns
                .entry((sid.to_string(), column.clone()))
                .or_insert((sid, table.clone()));
        }
        let entries: Vec<Value> = sigs
            .iter()
            .filter_map(|c| usage_entry(c, &value, signals.dead_after_months))
            .collect();
        if entries.is_empty() {
            continue;
        }
        let implies: Vec<Value> = signals
            .implications
            .iter()
            .filter(|i| lc(&i.table) == table && i.column.to_lowercase() == column && i.value == value)
            .filter(|i| sigs.iter().any(|c| c.database == i.database))
            .map(|i| json!({"database": i.database, "column": i.then_column, "then": i.then, "support": i.support}))
            .collect();
        if !implies.is_empty() {
            o.properties.insert("implies".into(), Value::Array(implies));
        }
        stats.values_measured += 1;
        stats.dead += usize::from(entries.iter().any(|e| e["dead"] == true));
        stats.never_seen += usize::from(entries.iter().all(|e| e["never_seen"] == true));
        o.properties
            .insert("data_usage".into(), Value::Array(entries));
        o.properties
            .insert("data_measured_at".into(), json!(signals.measured_at));
    }
    for ((_, column), (sid, table)) in columns {
        let subject = subjects.get(&sid).copied();
        for c in signals_for(signals, &table, &column, subject) {
            if c.coverage.unexplained.rows > 0 && c.references.is_none() {
                data_only_gap(out, sid, &table, &column, c);
                stats.data_only_gaps += 1;
            }
        }
    }
    stats
}

// ── New codes: what the data held against when the code knew it (§3.2(b)) ─────────────────────

/// Code lines traced per value (one history walk each).
pub const MAX_TRACED_SITES: usize = 3;
/// Values traced per run: the walk is a `git log -L`, so it is bounded.
pub const MAX_TRACED_VALUES: usize = 400;

/// What [`date_codes`] found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodeDateStats {
    pub traced: usize,
    pub data_before_code: usize,
    pub new_value_columns: usize,
}

/// The code lines an item cites (its evidence with a line in a file), sorted.
fn code_sites(o: &KirObject, evidence: &HashMap<KirId, &KirEvidence>) -> Vec<(String, u32)> {
    let mut v: Vec<(String, u32)> = o
        .evidence
        .iter()
        .filter_map(|id| evidence.get(id))
        // A file in the workspace: not a measurement (`profile://`) or a live catalog's routine
        // (`postgres://…`), which have no history to read.
        .filter(|e| !e.location.path.is_empty() && !e.location.path.contains("://"))
        .filter_map(|e| Some((e.location.path.clone(), e.location.line?)))
        .collect();
    v.sort();
    v.dedup();
    v
}

/// The earliest month a value was seen in any database that holds it.
fn data_first_seen(o: &KirObject) -> Option<String> {
    o.properties
        .get("data_usage")?
        .as_array()?
        .iter()
        .filter(|e| e["rows"].as_i64().is_some_and(|n| n > 0))
        .filter_map(|e| e["first_seen"].as_str().map(str::to_string))
        .min()
}

fn month_of(date: &str) -> &str {
    date.get(..7).unwrap_or(date)
}

/// Orders two months: the data's first row against the code's first mention.
fn order(data_first: &str, code_month: &str) -> &'static str {
    match data_first.cmp(code_month) {
        std::cmp::Ordering::Less => "data_before_code",
        std::cmp::Ordering::Equal => "same_month",
        std::cmp::Ordering::Greater => "code_before_data",
    }
}

fn hit_json(h: &BlameHit, path: &str) -> Value {
    json!({"date": h.date, "sha": h.sha, "summary": h.summary, "path": path, "line": h.line})
}

/// RFC 0252 §3.2(b) "new codes", against the code's own history (`rationale`):
///
/// - each value the data holds with a first month (`data_usage`) gets `code_history`: the commit
///   that introduced the literal at the code lines citing it (the oldest across up to
///   [`MAX_TRACED_SITES`] lines), the data's first month, and `order` — `data_before_code` when
///   the data held the value before the code named it (the code is behind the data);
/// - each `data_only_values` gap gets `new`: true when every value no code names first appeared
///   after the code mentioning the column last changed (`code_last_changed`).
///
/// Neither property is signed (`semantics_review::signature`), so a code change or a re-profile
/// never stales a review.
pub fn date_codes(out: &mut SemanticsOutput, rationale: &dyn RationaleSource) -> CodeDateStats {
    let mut stats = CodeDateStats::default();
    let evidence: HashMap<KirId, &KirEvidence> = out.evidence.iter().map(|e| (e.id, e)).collect();
    // (table, column) → every code line any value of the column cites.
    let mut column_sites: BTreeMap<(String, String), BTreeSet<(String, u32)>> = BTreeMap::new();
    let mut history: Vec<(usize, Value)> = Vec::new();
    for (i, o) in out.objects.iter().enumerate() {
        let k = kind_name(o);
        if k != Some(ENUM_MEANING)
            && !(k == Some(GAP) && prop(o, "gap_type") == "unexplained_value")
        {
            continue;
        }
        let sites = code_sites(o, &evidence);
        column_sites
            .entry((lc(prop(o, "table")), prop(o, "column").to_lowercase()))
            .or_default()
            .extend(sites.iter().cloned());
        let Some(first) = data_first_seen(o) else {
            continue;
        };
        if stats.traced >= MAX_TRACED_VALUES || sites.is_empty() {
            continue;
        }
        stats.traced += 1;
        let value = prop(o, "value");
        let intro = sites
            .iter()
            .take(MAX_TRACED_SITES)
            .filter_map(|(p, l)| rationale.introduced(p, *l, value).map(|h| (h, p.clone())))
            .min_by(|a, b| (&a.0.date, &a.0.sha).cmp(&(&b.0.date, &b.0.sha)));
        let Some((hit, path)) = intro else { continue };
        let ord = order(&first, month_of(&hit.date));
        stats.data_before_code += usize::from(ord == "data_before_code");
        let mut h = json!({
            "introduced": hit.date, "sha": hit.sha, "path": path, "line": hit.line,
            "data_first_seen": first, "order": ord,
        });
        if !hit.summary.is_empty() {
            h["summary"] = json!(hit.summary);
        }
        history.push((i, h));
    }
    for (i, h) in history {
        out.objects[i].properties.insert("code_history".into(), h);
    }
    // New values: a column's unexplained values all first seen after its code last changed.
    for o in out.objects.iter_mut() {
        if kind_name(o) != Some(GAP) || prop(o, "gap_type") != DATA_ONLY {
            continue;
        }
        let Some(first) = o
            .properties
            .get("first_seen")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let key = (lc(prop(o, "table")), prop(o, "column").to_lowercase());
        let Some(last) = last_change(rationale, column_sites.get(&key)) else {
            continue;
        };
        let new = first.as_str() > month_of(&last.0.date);
        stats.new_value_columns += usize::from(new);
        o.properties.insert("new".into(), json!(new));
        o.properties
            .insert("code_last_changed".into(), hit_json(&last.0, &last.1));
    }
    stats
}

/// The latest commit touching any of `sites` (`git blame`, grouped per file).
fn last_change(
    rationale: &dyn RationaleSource,
    sites: Option<&BTreeSet<(String, u32)>>,
) -> Option<(BlameHit, String)> {
    let mut by_path: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
    for (p, l) in sites? {
        by_path.entry(p.as_str()).or_default().push(*l);
    }
    by_path
        .into_iter()
        .flat_map(|(p, lines)| {
            rationale
                .blame(p, &lines)
                .into_iter()
                .map(move |h| (h, p.to_string()))
        })
        .max_by(|a, b| (&a.0.date, &a.0.sha).cmp(&(&b.0.date, &b.0.sha)))
}

/// One `data_only_values` gap: counts only, never the values.
fn data_only_gap(
    out: &mut SemanticsOutput,
    subject: KirId,
    table: &str,
    column: &str,
    c: &ColumnSignal,
) {
    let key = format!("semantic-gap:data-only:{subject}:{column}:{}", c.database);
    let u = &c.coverage.unexplained;
    let pct = share(u.rows, c.coverage.non_null) * 100.0;
    let distinct = c
        .coverage
        .unexplained_distinct
        .map(|d| format!("{d} value(s)"))
        .unwrap_or_else(|| "values".into());
    let mut o = KirObject::new(
        format!("{table}.{column}: data holds values no code names"),
        object_kind(GAP),
    );
    o.id = kid(&key);
    let props = [
        ("status", json!(HYPOTHESIS)),
        ("gap_type", json!(DATA_ONLY)),
        ("table", json!(table)),
        ("column", json!(column)),
        ("database", json!(c.database)),
        ("owner", json!(c.owner)),
        ("rows", json!(u.rows)),
        ("share", json!(share(u.rows, c.coverage.non_null))),
        ("column_rows", json!(c.coverage.non_null)),
        ("distinct", json!(c.coverage.unexplained_distinct)),
        ("first_seen", json!(u.first_seen)),
        ("last_seen", json!(u.last_seen)),
        ("code_values", json!(c.coverage.literals.len())),
        ("tier", json!(c.tier)),
        (
            "question",
            json!(format!(
                "{pct:.1}% of rows in {table}.{column} ({} of {} on {}) hold {distinct} that no \
                 code mentions. What are they?",
                u.rows, c.coverage.non_null, c.database
            )),
        ),
    ];
    for (k, v) in props {
        o.properties.insert(k.into(), v);
    }
    // The evidence names the measurement, never its counts: a re-profile must not stale a review.
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: format!("profile://{}/{}.{}.{column}", c.database, c.owner, c.table),
            line: None,
            column: None,
        },
        format!(
            "{}.{}.{column} on {} measured against the values code names (RFC 0252 literal coverage)",
            c.owner, c.table, c.database
        ),
    );
    ev.id = kid(&format!("semantics-evidence:{key}:0"));
    o.evidence = vec![ev.id];
    out.evidence.push(ev);
    out.relationships.push(KirRelationship::deterministic(
        RelationshipKind::Custom(DESCRIBES.into()),
        o.id,
        subject,
        "",
    ));
    out.objects.push(o);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::ObjectKind;
    use ekos_observation_sdk::signals::Usage;

    fn usage(rows: i64, first: &str, last: &str) -> Usage {
        Usage {
            rows,
            first_seen: Some(first.into()),
            last_seen: Some(last.into()),
        }
    }

    fn signals() -> DataSignals {
        let mut cov = Coverage {
            non_null: 100,
            latest: Some("2026-10".into()),
            unexplained: usage(4, "2025-01", "2026-10"),
            unexplained_distinct: Some(2),
            ..Default::default()
        };
        cov.literals
            .insert("1".into(), usage(60, "2019-01", "2026-10"));
        cov.literals
            .insert("2".into(), usage(36, "2019-01", "2024-04"));
        cov.literals.insert("3".into(), Usage::default());
        DataSignals {
            rfc: "0252".into(),
            measured_at: "2026-11-10T00:00:00Z".into(),
            dead_after_months: 24,
            columns: vec![ColumnSignal {
                database: "erp".into(),
                owner: "public".into(),
                table: "orders".into(),
                column: "status".into(),
                tier: "p1".into(),
                time_column: Some("created_at".into()),
                coverage: cov,
                references: None,
            }],
            grain: vec![],
            ..Default::default()
        }
    }

    fn table() -> KirObject {
        let mut t = KirObject::new("orders", ObjectKind::Table);
        t.properties.insert("origin".into(), json!("live"));
        t.properties.insert("database".into(), json!("erp"));
        t
    }

    fn meaning(value: &str, table: KirId) -> (KirObject, KirRelationship) {
        let mut o = KirObject::new(
            format!("orders.status = {value}"),
            object_kind(ENUM_MEANING),
        );
        for (k, v) in [("table", "orders"), ("column", "status"), ("value", value)] {
            o.properties.insert(k.into(), json!(v));
        }
        let r = KirRelationship::deterministic(
            RelationshipKind::Custom(DESCRIBES.into()),
            o.id,
            table,
            "",
        );
        (o, r)
    }

    fn output(t: &KirObject) -> SemanticsOutput {
        let mut out = SemanticsOutput::default();
        for v in ["1", "2", "3"] {
            let (o, r) = meaning(v, t.id);
            out.objects.push(o);
            out.relationships.push(r);
        }
        out
    }

    #[test]
    fn usage_share_dead_and_never_seen() {
        let t = table();
        let mut out = output(&t);
        let subjects = HashMap::from([(t.id, &t)]);
        let s = fold(&mut out, &subjects, &signals());
        let u = |i: usize| out.objects[i].properties["data_usage"][0].clone();
        assert_eq!(u(0)["rows"], 60);
        assert_eq!(u(0)["share"], 0.6);
        assert_eq!(u(0)["dead"], false);
        // Last seen 2024-04, latest 2026-10: 30 months > 24.
        assert_eq!(u(1)["dead"], true);
        assert_eq!(u(2)["never_seen"], true);
        assert_eq!(
            (s.values_measured, s.dead, s.never_seen, s.data_only_gaps),
            (3, 1, 1, 1)
        );
    }

    #[test]
    fn data_only_gap_has_counts_and_never_a_value() {
        let t = table();
        let mut out = output(&t);
        let subjects = HashMap::from([(t.id, &t)]);
        fold(&mut out, &subjects, &signals());
        let g = out
            .objects
            .iter()
            .find(|o| prop(o, "gap_type") == DATA_ONLY)
            .unwrap();
        assert_eq!(g.properties["rows"], 4);
        assert_eq!(g.properties["distinct"], 2);
        assert!(prop(g, "question").contains("4.0% of rows"));
        assert!(!g.properties.contains_key("value"));
    }

    #[test]
    fn a_reprofile_with_other_counts_keeps_the_signature() {
        let t = table();
        let subjects = HashMap::from([(t.id, &t)]);
        let sig = |s: &DataSignals| {
            let mut out = output(&t);
            fold(&mut out, &subjects, s);
            let ev: HashMap<KirId, &KirEvidence> = out.evidence.iter().map(|e| (e.id, e)).collect();
            out.objects
                .iter()
                .map(|o| {
                    let evs: Vec<&KirEvidence> = o
                        .evidence
                        .iter()
                        .filter_map(|i| ev.get(i).copied())
                        .collect();
                    crate::semantics_review::signature(o, &evs)
                })
                .collect::<Vec<_>>()
        };
        let a = signals();
        let mut b = signals();
        b.columns[0].coverage.unexplained.rows = 40;
        b.columns[0].coverage.literals.get_mut("1").unwrap().rows = 7;
        assert_eq!(sig(&a), sig(&b));
    }

    #[test]
    fn another_databases_measurement_does_not_land_on_a_live_table() {
        let mut t = table();
        t.properties.insert("database".into(), json!("warehouse"));
        let mut out = output(&t);
        let subjects = HashMap::from([(t.id, &t)]);
        let s = fold(&mut out, &subjects, &signals());
        assert_eq!(s.values_measured, 0);
        assert!(!out.objects[0].properties.contains_key("data_usage"));
    }

    /// Code history from a fixture: value → introduction date; any blame → `last`.
    struct History {
        introduced: HashMap<&'static str, &'static str>,
        last: &'static str,
    }

    impl RationaleSource for History {
        fn blame(&self, _path: &str, lines: &[u32]) -> Vec<BlameHit> {
            lines
                .iter()
                .map(|l| BlameHit {
                    line: *l,
                    sha: "b".repeat(40),
                    summary: "touch".into(),
                    author: "a".into(),
                    date: self.last.into(),
                })
                .collect()
        }
        fn introduced(&self, _path: &str, line: u32, needle: &str) -> Option<BlameHit> {
            self.introduced.get(needle).map(|d| BlameHit {
                line,
                sha: "a".repeat(40),
                summary: format!("add {needle}"),
                author: "a".into(),
                date: (*d).into(),
            })
        }
    }

    /// `output` with each meaning citing one code line.
    fn cited(t: &KirObject) -> SemanticsOutput {
        let mut out = output(t);
        for (n, o) in out.objects.iter_mut().enumerate() {
            let mut ev = KirEvidence::new(
                SourceLocation {
                    path: "sql/orders.sql".into(),
                    line: Some(10 + n as u32),
                    column: None,
                },
                "WHERE status = …",
            );
            ev.id = kid(&format!("ev{n}"));
            o.evidence.push(ev.id);
            out.evidence.push(ev);
        }
        out
    }

    #[test]
    fn a_value_the_data_held_before_the_code_named_it_is_reported() {
        let t = table();
        let mut out = cited(&t);
        let subjects = HashMap::from([(t.id, &t)]);
        fold(&mut out, &subjects, &signals());
        let h = History {
            // "1": data from 2019-01, code from 2020-03. "2": code from 2018-06, data 2019-01.
            introduced: HashMap::from([
                ("1", "2020-03-02T10:00:00+00:00"),
                ("2", "2018-06-01T00:00:00+00:00"),
            ]),
            last: "2024-06-01T00:00:00+00:00",
        };
        let s = date_codes(&mut out, &h);
        let ch = |i: usize| out.objects[i].properties["code_history"].clone();
        assert_eq!(ch(0)["order"], "data_before_code");
        assert_eq!(ch(0)["data_first_seen"], "2019-01");
        assert_eq!(ch(0)["path"], "sql/orders.sql");
        assert_eq!(ch(1)["order"], "code_before_data");
        // "3" is never seen: nothing to compare.
        assert!(!out.objects[2].properties.contains_key("code_history"));
        assert_eq!((s.traced, s.data_before_code), (2, 1));
        // Unexplained values first seen 2025-01, code last changed 2024-06: all new.
        let gap = out
            .objects
            .iter()
            .find(|o| prop(o, "gap_type") == DATA_ONLY)
            .unwrap();
        assert_eq!(gap.properties["new"], true);
        assert_eq!(s.new_value_columns, 1);
    }

    #[test]
    fn values_older_than_the_last_code_change_are_not_new_and_nothing_stales() {
        let t = table();
        let subjects = HashMap::from([(t.id, &t)]);
        let run = |last: &'static str| {
            let mut out = cited(&t);
            fold(&mut out, &subjects, &signals());
            let before: Vec<String> = signatures(&out);
            date_codes(
                &mut out,
                &History {
                    introduced: HashMap::from([("1", "2010-01-01")]),
                    last,
                },
            );
            (out, before)
        };
        let (out, before) = run("2025-06-01T00:00:00+00:00");
        let gap = out
            .objects
            .iter()
            .find(|o| prop(o, "gap_type") == DATA_ONLY)
            .unwrap();
        assert_eq!(gap.properties["new"], false);
        assert_eq!(
            signatures(&out),
            before,
            "code history is not a signed field"
        );
    }

    fn signatures(out: &SemanticsOutput) -> Vec<String> {
        let ev: HashMap<KirId, &KirEvidence> = out.evidence.iter().map(|e| (e.id, e)).collect();
        out.objects
            .iter()
            .map(|o| {
                let evs: Vec<&KirEvidence> = o
                    .evidence
                    .iter()
                    .filter_map(|i| ev.get(i).copied())
                    .collect();
                crate::semantics_review::signature(o, &evs)
            })
            .collect()
    }

    /// Asked paths, to see which lines a run traces.
    struct Asked(std::cell::RefCell<Vec<String>>);

    impl RationaleSource for Asked {
        fn blame(&self, _path: &str, _lines: &[u32]) -> Vec<BlameHit> {
            Vec::new()
        }
        fn introduced(&self, path: &str, _line: u32, _needle: &str) -> Option<BlameHit> {
            self.0.borrow_mut().push(path.to_string());
            None
        }
    }

    #[test]
    fn a_live_catalogs_routine_is_not_traced_and_does_not_use_the_budget() {
        let t = table();
        let mut out = cited(&t);
        // Sorted before `sql/…`: a routine read from the live catalog cites a URL, not a file.
        for n in 0..MAX_TRACED_SITES {
            let mut ev = KirEvidence::new(
                SourceLocation {
                    path: format!("postgres://erp/public/function/f{n}()"),
                    line: Some(3),
                    column: None,
                },
                "status IN (1)",
            );
            ev.id = kid(&format!("live{n}"));
            out.objects[0].evidence.push(ev.id);
            out.evidence.push(ev);
        }
        let subjects = HashMap::from([(t.id, &t)]);
        fold(&mut out, &subjects, &signals());
        let asked = Asked(Default::default());
        date_codes(&mut out, &asked);
        let paths = asked.0.into_inner();
        assert!(paths.iter().all(|p| p == "sql/orders.sql"), "{paths:?}");
        assert!(!paths.is_empty());
    }
}
