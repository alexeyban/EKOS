//! RFC 0254 §3.6 — the source passport: the analyst's sections on an RFC 0214 table page.
//!
//! Pure, like the rest of [`super`]: the ledger's objects plus what the CLI read beside it
//! ([`PassportInput`]: which derived items are current, review decisions in words, and each open
//! question's computed status). Seven sections, each written only when it has content:
//!
//! 1. *What a row is* — the grain (`GrainCandidate`) and the row count.
//! 2. *Keys and references* — declared keys (the page's own table) plus code- and data-seeded
//!    `FkCandidate`s with their verdicts and review status.
//! 3. *Codes* — per coded column, every value code names (`EnumMeaning`) with its meaning, where
//!    the meaning is stated, and what the data says (`data_usage`); values no code names are one
//!    `data_only_values` row with counts only.
//! 4. *How it changes* — RFC 0212 write behaviour with RFC 0252's measured history.
//! 5. *What the code assumes* — `code_vs_data` conflicts and how they were decided.
//! 6. *What people said* — RFC 0253 testimony and RFC 0247 notes.
//! 7. *Open questions* — RFC 0254 questions with status and addressee.
//!
//! The estate's gates hold here too: counts and shares only (a `code_vs_data` finding's measured
//! `min`/`max` are never read), and a meaning's text only from licensed sources — except a
//! meaning a person stated, which is shown like the rest of what people said.

use super::{Model, cell, code, custom, link, prop, strings, table, table_key};
use ekos_kir::annotations::{self, Annotation, Effective};
use ekos_kir::interaction::{Cell, Matrix};
use ekos_kir::{KirObject, ObjectKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// What the CLI read beside the ledger for the passport sections. `None` sets mean "no manifest
/// yet: every item counts as current".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PassportInput {
    /// `GrainCandidate` ids the latest `ekos profile run` wrote (`profile/current.json`).
    pub current_grain: Option<BTreeSet<String>>,
    /// `FkCandidate` ids the latest `ekos relationships discover` wrote.
    pub current_references: Option<BTreeSet<String>>,
    /// RFC 0170 item ids the latest synthesis derived (`semantics/current.json`).
    pub current_semantics: Option<BTreeSet<String>>,
    /// `"<review subject>:<item id>"` → the decision in words ([`decision_words`]); only the
    /// `relationships` and `write-behaviour` subjects are read.
    pub reviewed: BTreeMap<String, String>,
    /// `OpenQuestion` id → its state, computed on read by the CLI. A question missing here, or
    /// `obsolete`, is not shown.
    pub questions: BTreeMap<String, QuestionState>,
    /// RFC 0256: who reads and writes each table, declared and observed. `None`: the old "Read
    /// by" / "Written by" lists from routines are shown instead.
    pub interaction: Option<Matrix>,
    /// RFC 0256: how many `ObservedAccess` facts the matrix holds (0: the source provided none).
    pub observed_facts: usize,
    /// RFC 0256: the current `InteractionFinding` ids (`<state>/interaction/findings.json`).
    pub interaction_findings: BTreeSet<String>,
}

/// An open question's state as `ekos questions` computes it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QuestionState {
    /// `open`, `answered`, `closed` or `obsolete`.
    pub status: String,
    /// Who the question is with now.
    pub addressee: Option<String>,
}

/// An RFC 0213 review decision in words: `confirmed`, `confirmed (with orphans)`, `rejected`,
/// `confirmed (overridden)`; a decision made on facts that changed since is no decision.
pub fn decision_words(decision: Option<&Value>, outdated: bool) -> Option<String> {
    if outdated {
        return Some("hypothesis (decision outdated)".into());
    }
    let d = decision?;
    let verb = ["relationship", "behaviour", "decision"]
        .iter()
        .find_map(|k| d.get(k).and_then(Value::as_str))?;
    Some(
        match verb {
            "confirm" => "confirmed",
            "confirm-with-orphans" => "confirmed (with orphans)",
            "reject" => "rejected",
            "override" => "confirmed (overridden)",
            other => other,
        }
        .to_string(),
    )
}

/// A table name as the passport keys it: the last dotted part, as [`table_key`].
fn tkey(name: &str) -> String {
    table_key(name.rsplit('.').next().unwrap_or(name))
}

fn num(o: &Value, k: &str) -> Option<f64> {
    o.get(k).and_then(Value::as_f64)
}

fn pct(share: f64) -> String {
    format!("{:.1}%", share * 100.0)
}

/// The date part of an RFC 3339 time.
fn day(t: &str) -> &str {
    t.get(..10).unwrap_or(t)
}

/// A person's text on one line, inert as Markdown and HTML.
fn plain(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('`', "'")
        .replace('|', "\\|")
}

/// `profile://…` evidence, with the day it was measured.
fn profile_evidence(path: &str, at: Option<&str>) -> String {
    match at.filter(|a| !a.is_empty()) {
        Some(a) => format!("{} ({})", code(path), day(a)),
        None => code(path),
    }
}

/// A code location `path:line` (or the path alone).
fn location(path: &str, line: Option<u64>) -> String {
    match line {
        Some(l) if l > 0 => code(&format!("{path}:{l}")),
        _ => code(path),
    }
}

/// The passport's view of the ledger, by table key.
#[derive(Default)]
pub(super) struct Index<'a> {
    grain: HashMap<String, Vec<&'a KirObject>>,
    references: HashMap<String, Vec<&'a KirObject>>,
    codes: HashMap<String, Vec<&'a KirObject>>,
    assumptions: HashMap<String, Vec<&'a KirObject>>,
    testimony: HashMap<String, Vec<&'a KirObject>>,
    questions: HashMap<String, Vec<&'a KirObject>>,
    notes: HashMap<String, Vec<(Annotation, &'static str)>>,
}

fn current(set: &Option<BTreeSet<String>>, o: &KirObject) -> bool {
    set.as_ref().is_none_or(|c| c.contains(&o.id.to_string()))
}

impl<'a> Index<'a> {
    pub(super) fn new(
        objects: &'a [KirObject],
        current_conflicts: Option<&BTreeSet<String>>,
        p: &PassportInput,
    ) -> Self {
        let mut ix = Self::default();
        let by_id: HashMap<String, &KirObject> =
            objects.iter().map(|o| (o.id.to_string(), o)).collect();
        let mut testimony: BTreeMap<String, &KirObject> = BTreeMap::new();
        for o in objects {
            let push = |map: &mut HashMap<String, Vec<&'a KirObject>>, t: &str| {
                if !t.is_empty() {
                    map.entry(tkey(t)).or_default().push(o);
                }
            };
            match custom(o) {
                Some("GrainCandidate") if current(&p.current_grain, o) => {
                    push(&mut ix.grain, prop(o, "table").unwrap_or(""));
                }
                Some("FkCandidate") if current(&p.current_references, o) => {
                    let (c, pa) = (
                        prop(o, "child_table").unwrap_or(""),
                        prop(o, "parent_table").unwrap_or(""),
                    );
                    push(&mut ix.references, c);
                    if tkey(c) != tkey(pa) {
                        push(&mut ix.references, pa);
                    }
                }
                Some("EnumMeaning") if current(&p.current_semantics, o) => {
                    push(&mut ix.codes, prop(o, "table").unwrap_or(""));
                }
                Some("SemanticGap")
                    if current(&p.current_semantics, o)
                        && prop(o, "gap_type") == Some("data_only_values") =>
                {
                    push(&mut ix.codes, prop(o, "table").unwrap_or(""));
                }
                Some("ConflictingEvidence")
                    if prop(o, "conflict_type") == Some("code_vs_data")
                        && current_conflicts.is_none_or(|c| c.contains(&o.id.to_string())) =>
                {
                    push(&mut ix.assumptions, prop(o, "subject_name").unwrap_or(""));
                }
                Some("OpenQuestion")
                    if p.questions
                        .get(&o.id.to_string())
                        .is_some_and(|s| s.status != "obsolete") =>
                {
                    push(&mut ix.questions, prop(o, "table").unwrap_or(""));
                }
                Some("Testimony") => {
                    let k = o.id.to_string();
                    let newer = testimony.get(&k).is_none_or(|old| {
                        prop(o, "updated_at").unwrap_or("") >= prop(old, "updated_at").unwrap_or("")
                    });
                    if newer {
                        testimony.insert(k, o);
                    }
                }
                _ => {}
            }
        }
        for t in testimony.into_values() {
            if prop(t, "status") != Some("active") {
                continue;
            }
            for key in testimony_tables(t, &by_id) {
                ix.testimony.entry(key).or_default().push(t);
            }
        }
        for a in annotations::current(objects) {
            let Some(target) = by_id.get(&a.target).filter(|o| o.kind == ObjectKind::Table) else {
                continue;
            };
            let state =
                match annotations::effective(&a, Some(&annotations::digest_of_object(target))) {
                    Effective::Active => "active",
                    Effective::Stale => "stale",
                    Effective::Withdrawn => continue,
                };
            ix.notes
                .entry(tkey(&target.name))
                .or_default()
                .push((a, state));
        }
        ix.sort();
        ix
    }

    /// Every list in a fixed order, whatever order the ledger returned.
    fn sort(&mut self) {
        let by_name = |a: &&KirObject, b: &&KirObject| {
            (&a.name, a.id.to_string()).cmp(&(&b.name, b.id.to_string()))
        };
        for map in [&mut self.grain, &mut self.references, &mut self.assumptions] {
            for v in map.values_mut() {
                v.sort_by(by_name);
                v.dedup_by(|a, b| a.id == b.id);
            }
        }
        for v in self.codes.values_mut() {
            v.sort_by_key(|o| {
                (
                    prop(o, "column").unwrap_or("").to_lowercase(),
                    custom(o) == Some("SemanticGap"),
                    prop(o, "value").unwrap_or("").to_string(),
                    o.id.to_string(),
                )
            });
            v.dedup_by(|a, b| a.id == b.id);
        }
        for v in self.testimony.values_mut() {
            v.sort_by_key(|o| {
                (
                    prop(o, "sent_at").unwrap_or("").to_string(),
                    o.id.to_string(),
                )
            });
        }
        for v in self.questions.values_mut() {
            v.sort_by_key(|q| {
                let p = q
                    .properties
                    .get("priority")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                (-(p * 100.0) as i64, q.id.to_string())
            });
            v.dedup_by(|a, b| a.id == b.id);
        }
        for v in self.notes.values_mut() {
            v.sort_by(|a, b| (&a.0.updated_at, a.0.id.0).cmp(&(&b.0.updated_at, b.0.id.0)));
        }
    }
}

/// The tables a testimony is about: the entities it names (a table, or an item about one) and
/// the table of the question it answers.
fn testimony_tables(t: &KirObject, by_id: &HashMap<String, &KirObject>) -> BTreeSet<String> {
    let table_of = |o: &KirObject| -> Option<String> {
        if o.kind == ObjectKind::Table {
            return Some(tkey(&o.name));
        }
        ["table", "child_table", "subject_name"]
            .iter()
            .find_map(|k| prop(o, k).filter(|v| !v.is_empty()))
            .map(tkey)
    };
    let mut ids = strings(t, "about");
    ids.extend(
        prop(t, "answers")
            .filter(|a| !a.is_empty())
            .map(str::to_string),
    );
    ids.iter()
        .filter_map(|id| by_id.get(id))
        .filter_map(|o| table_of(o))
        .collect()
}

impl Model<'_> {
    fn reviewed(&self, subject: &str, item: &str) -> Option<&str> {
        self.input
            .passport
            .reviewed
            .get(&format!("{subject}:{item}"))
            .map(String::as_str)
    }
}

/// 1. What a row is.
pub(super) fn row_section(m: &Model, name: &str, t: &KirObject) -> String {
    let grains = m
        .passport
        .grain
        .get(&tkey(name))
        .cloned()
        .unwrap_or_default();
    let rows = t
        .properties
        .get("profile")
        .and_then(|p| p.get("row_count"))
        .and_then(Value::as_u64)
        .or_else(|| {
            grains
                .iter()
                .filter_map(|g| g.properties.get("rows").and_then(Value::as_u64))
                .max()
        });
    if grains.is_empty() && rows.is_none() {
        return String::new();
    }
    let mut b = String::from("## What a row is\n\n");
    if let Some(r) = rows {
        b.push_str(&format!("{r} row(s) when profiled.\n\n"));
    }
    let lines: Vec<Vec<String>> = grains.iter().map(|g| grain_row(g)).collect();
    if !lines.is_empty() {
        b.push_str("One row is identified by:\n\n");
        b.push_str(&table(
            &["key", "how it was found", "database", "status", "evidence"],
            &lines,
        ));
    }
    b.push('\n');
    b
}

fn grain_row(g: &KirObject) -> Vec<String> {
    let key: Vec<String> = g
        .properties
        .get("key")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|c| c.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let declared = prop(g, "source") == Some("declared");
    let how = match prop(g, "source") {
        Some("declared") => "primary key declared in the catalog".to_string(),
        Some("measured") => format!(
            "measured: unique over {} row(s)",
            g.properties
                .get("rows")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ),
        _ => format!(
            "no unique key among {} column set(s) tried{}",
            g.properties
                .get("tested")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            if g.properties.get("exhausted").and_then(Value::as_bool) == Some(true) {
                " (search exhausted)"
            } else {
                ""
            }
        ),
    };
    let (db, owner, tab) = (
        prop(g, "database").unwrap_or(""),
        prop(g, "owner").unwrap_or(""),
        prop(g, "table").unwrap_or(""),
    );
    vec![
        if key.is_empty() {
            "_none found_".into()
        } else {
            code(&key.join(", "))
        },
        cell(&how),
        code(db),
        if declared {
            "confirmed (declared)".into()
        } else {
            "hypothesis".into()
        },
        profile_evidence(
            &format!("profile://{db}/{owner}.{tab}"),
            prop(g, "measured_at"),
        ),
    ]
}

/// 2. The inferred references under the declared keys.
pub(super) fn inferred_references(m: &Model, name: &str, path: &str) -> String {
    let Some(cands) = m.passport.references.get(&tkey(name)) else {
        return String::new();
    };
    let side = |o: &KirObject, s: &str| {
        let t = prop(o, &format!("{s}_table")).unwrap_or("");
        let c = prop(o, &format!("{s}_column")).unwrap_or("");
        if tkey(t) == tkey(name) {
            code(&format!("{t}.{c}"))
        } else {
            format!("{}.{}", m.table_ref(path, &table_key(t)), code(c))
        }
    };
    let rows: Vec<Vec<String>> = cands
        .iter()
        .map(|o| {
            vec![
                side(o, "child"),
                side(o, "parent"),
                code(prop(o, "seed").unwrap_or("")),
                code(prop(o, "verdict").unwrap_or("not_measured")),
                found_in_target(o),
                m.reviewed("relationships", &o.id.to_string())
                    .unwrap_or("hypothesis")
                    .to_string(),
                reference_evidence(o),
            ]
        })
        .collect();
    let mut b = String::from(
        "\n**Inferred** — references that code or data suggest and the schema does not declare (RFC 0252):\n\n",
    );
    b.push_str(&table(
        &[
            "from",
            "to",
            "seed",
            "verdict",
            "values found in target",
            "status",
            "evidence",
        ],
        &rows,
    ));
    b
}

fn found_in_target(o: &KirObject) -> String {
    let p = &o.properties;
    let rate = p.get("inclusion_rate").and_then(Value::as_f64);
    let orphans = p.get("orphans").and_then(Value::as_u64);
    let population = p.get("population").and_then(Value::as_u64);
    match (rate, orphans, population) {
        (Some(r), Some(o), Some(n)) => format!("{} ({o} orphan(s) of {n})", pct(r)),
        (Some(r), _, _) => pct(r),
        _ => match p.get("containment_estimate").and_then(Value::as_f64) {
            Some(e) => format!("~{} (estimate)", pct(e)),
            None => "not measured".into(),
        },
    }
}

fn reference_evidence(o: &KirObject) -> String {
    let owner = prop(o, "child_owner")
        .filter(|w| !w.is_empty())
        .map(|w| format!("{w}."))
        .unwrap_or_default();
    let mut ev = vec![profile_evidence(
        &format!(
            "profile://{}/{owner}{}.{}",
            prop(o, "database").unwrap_or(""),
            prop(o, "child_table").unwrap_or(""),
            prop(o, "child_column").unwrap_or("")
        ),
        prop(o, "measured_at"),
    )];
    ev.extend(strings(o, "code_sources").iter().map(|s| code(s)));
    ev.join("<br>")
}

/// 3. Codes.
pub(super) fn codes_section(m: &Model, name: &str, t: &KirObject) -> String {
    let Some(items) = m.passport.codes.get(&tkey(name)) else {
        return String::new();
    };
    let rows: Vec<Vec<String>> = items
        .iter()
        .map(|o| {
            if custom(o) == Some("SemanticGap") {
                data_only_row(o)
            } else {
                code_row(m, o, t)
            }
        })
        .collect();
    let mut b = String::from(
        "## Codes\n\nEvery value code names for a coded column, what it means and where that is stated, and how the data uses it. Values no code names are counted, never shown.\n\n",
    );
    b.push_str(&table(
        &[
            "column",
            "value",
            "meaning",
            "stated by",
            "share of rows",
            "first seen",
            "last seen",
            "in data",
            "status",
            "evidence",
        ],
        &rows,
    ));
    b.push('\n');
    b
}

fn status_words(o: &KirObject) -> String {
    match prop(o, "status").unwrap_or("hypothesis") {
        "edited" => "confirmed (edited)".into(),
        "" => "hypothesis".into(),
        s => s.to_string(),
    }
}

/// One `EnumMeaning`: a value code names (a code literal, never a data value).
fn code_row(m: &Model, o: &KirObject, t: &KirObject) -> Vec<String> {
    let best = o
        .properties
        .get("meanings")
        .and_then(Value::as_array)
        .and_then(|a| a.first());
    let stated_by = best
        .and_then(|b| b.get("source"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let label = prop(o, "label").filter(|l| !l.trim().is_empty());
    // A meaning a person stated is shown like what people said; any other is source text.
    let meaning = if stated_by == "testimony" {
        label.map(plain)
    } else if Model::sources_of(o).is_empty() {
        m.text(t, label).map(|l| cell(&l))
    } else {
        m.text(o, label).map(|l| cell(&l))
    };
    let usage: Vec<&Value> = o
        .properties
        .get("data_usage")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let [share, first, last, mut in_data] = usage_cells(&usage);
    for i in o
        .properties
        .get("implies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let col = i.get("column").and_then(Value::as_str).unwrap_or("");
        let then = i
            .get("then")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('_', " ");
        in_data.push_str(&format!("<br>implies {} {}", code(col), cell(&then)));
    }
    // RFC 0252 §3.2(b): the data held the value before the code named it — the code is behind.
    if let Some(h) = o.properties.get("code_history")
        && h.get("order").and_then(Value::as_str) == Some("data_before_code")
    {
        let since = h.get("introduced").and_then(Value::as_str).unwrap_or("");
        in_data.push_str(&format!(
            "<br>new: in the data before the code named it (code since {})",
            cell(since.get(..7).unwrap_or(since))
        ));
    }
    let mut ev = Vec::new();
    if let Some(b) = best
        && let Some(p) = b
            .get("path")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
    {
        ev.push(location(p, b.get("line").and_then(Value::as_u64)));
    }
    if !usage.is_empty() {
        ev.push(match prop(o, "data_measured_at") {
            Some(at) => format!("profile ({})", day(at)),
            None => "profile".into(),
        });
    }
    vec![
        code(prop(o, "column").unwrap_or("")),
        code(prop(o, "value").unwrap_or("")),
        meaning.unwrap_or_else(|| match label {
            // QA: a meaning exists but its source's licence does not let its text be shown.
            Some(_) => "_withheld: source licence unknown_".into(),
            None => "_unknown_".into(),
        }),
        cell(stated_by),
        share,
        first,
        last,
        in_data,
        status_words(o),
        ev.join("<br>"),
    ]
}

/// Share, first seen, last seen and state over a value's per-database usage; a database is named
/// when there are several.
fn usage_cells(usage: &[&Value]) -> [String; 4] {
    let many = usage.len() > 1;
    let mut out: [Vec<String>; 4] = Default::default();
    for u in usage {
        let db = u.get("database").and_then(Value::as_str).unwrap_or("?");
        let tag = |s: String| if many { format!("{db}: {s}") } else { s };
        let s = |k: &str| u.get(k).and_then(Value::as_str).unwrap_or("—").to_string();
        out[0].push(tag(num(u, "share").map_or_else(|| "—".into(), pct)));
        out[1].push(tag(s("first_seen")));
        out[2].push(tag(s("last_seen")));
        let state = if u.get("never_seen").and_then(Value::as_bool) == Some(true) {
            "never seen"
        } else if u.get("dead").and_then(Value::as_bool) == Some(true) {
            "dead"
        } else {
            "in use"
        };
        out[3].push(tag(state.into()));
    }
    out.map(|v| {
        if v.is_empty() {
            "—".to_string()
        } else {
            cell(&v.join("\n"))
        }
    })
}

/// "unexplained", and "new" when every value no code names first appeared after the code that
/// mentions the column last changed (RFC 0252 §3.2(b)).
fn new_values_state(o: &KirObject) -> String {
    if o.properties.get("new").and_then(Value::as_bool) != Some(true) {
        return "unexplained".into();
    }
    let changed = o
        .properties
        .get("code_last_changed")
        .and_then(|c| c.get("date"))
        .and_then(Value::as_str)
        .unwrap_or("");
    format!(
        "unexplained<br>new: all first seen after the code last changed ({})",
        cell(changed.get(..7).unwrap_or(changed))
    )
}

/// One `data_only_values` gap: counts only.
fn data_only_row(o: &KirObject) -> Vec<String> {
    let p = &o.properties;
    let distinct = p
        .get("distinct")
        .and_then(Value::as_u64)
        .map(|d| format!("{d} distinct"))
        .unwrap_or_else(|| "values".into());
    vec![
        code(prop(o, "column").unwrap_or("")),
        "_other values_".into(),
        format!("_named by no code ({distinct})_"),
        "data".into(),
        p.get("share")
            .and_then(Value::as_f64)
            .map_or_else(|| "—".into(), pct),
        cell(prop(o, "first_seen").unwrap_or("—")),
        cell(prop(o, "last_seen").unwrap_or("—")),
        new_values_state(o),
        status_words(o),
        profile_evidence(
            &format!(
                "profile://{}/{}.{}.{}",
                prop(o, "database").unwrap_or(""),
                prop(o, "owner").unwrap_or(""),
                prop(o, "table").unwrap_or(""),
                prop(o, "column").unwrap_or("")
            ),
            None,
        ),
    ]
}

/// 4. The review status and the measured history under the write behaviour.
pub(super) fn change_lines(m: &Model, name: &str, w: &KirObject) -> String {
    let mut b = format!(
        "- status: **{}**\n",
        m.reviewed(
            "write-behaviour",
            &format!("table:{}", name.to_ascii_lowercase())
        )
        .unwrap_or("hypothesis")
    );
    for h in w
        .properties
        .get("data_history")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let s = |k: &str| h.get(k).and_then(Value::as_str).unwrap_or("");
        let n = |k: &str| h.get(k).and_then(Value::as_i64).unwrap_or(0);
        let cols: Vec<&str> = h
            .get("columns")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        b.push_str(&format!(
            "- data ({}): {} on {} — {}: {} of {} row(s)\n",
            code(s("database")),
            match s("kind") {
                "validity" => "validity period",
                "flag" => "soft-delete flag",
                other => other,
            },
            code(&cols.join(", ")),
            cell(s("meaning")),
            n("measured"),
            n("rows"),
        ));
    }
    b
}

/// 5. What the code assumes.
pub(super) fn assumptions_section(m: &Model, name: &str) -> String {
    let Some(cs) = m.passport.assumptions.get(&tkey(name)) else {
        return String::new();
    };
    let rows: Vec<Vec<String>> = cs.iter().map(|c| assumption_row(c)).collect();
    let mut b = String::from(
        "## What the code assumes\n\nWhere recovered logic assumes something the measured data contradicts (RFC 0254 code-vs-data checks).\n\n",
    );
    b.push_str(&table(
        &[
            "check",
            "code says",
            "data says",
            "measured",
            "status",
            "evidence",
        ],
        &rows,
    ));
    b.push('\n');
    b
}

fn assumption_row(c: &KirObject) -> Vec<String> {
    let claims: Vec<&Value> = c
        .properties
        .get("claims")
        .and_then(Value::as_array)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    let is_data = |x: &&&Value| x.get("source").and_then(Value::as_str) == Some("profile");
    let text = |x: &Value| cell(x.get("value").and_then(Value::as_str).unwrap_or(""));
    let says = |data: bool| {
        claims
            .iter()
            .filter(|x| is_data(x) == data)
            .map(|x| text(x))
            .collect::<Vec<_>>()
            .join("<br>")
    };
    let evidence: Vec<String> = claims
        .iter()
        .map(|x| {
            location(
                x.get("path").and_then(Value::as_str).unwrap_or(""),
                x.get("line").and_then(Value::as_u64),
            )
        })
        .collect();
    vec![
        code(prop(c, "check").unwrap_or("")),
        says(false),
        says(true),
        measured(c.properties.get("measured")),
        conflict_status(c),
        evidence.join("<br>"),
    ]
}

/// A finding's measurement: counts, shares and verdicts only — never `min`/`max`.
fn measured(m: Option<&Value>) -> String {
    let Some(m) = m else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some(n) = m.get("rows_unhandled").and_then(Value::as_i64) {
        parts.push(format!("{n} row(s) unhandled"));
    }
    if let Some(n) = m.get("rows").and_then(Value::as_i64) {
        parts.push(format!("{n} row(s)"));
    }
    if let Some(s) = num(m, "share") {
        parts.push(pct(s));
    }
    if let Some(v) = m.get("verdict").and_then(Value::as_str) {
        parts.push(code(v));
    }
    parts.join(", ")
}

fn conflict_status(c: &KirObject) -> String {
    let by = prop(c, "reviewed_by")
        .map(|b| format!(" by {}", cell(b)))
        .unwrap_or_default();
    match prop(c, "status").unwrap_or("open") {
        "resolved" => {
            let side = c
                .properties
                .get("picked_claim")
                .and_then(|p| p.get("source"))
                .and_then(Value::as_str);
            let which = if side == Some("profile") {
                "the data is right"
            } else {
                "the code is right"
            };
            format!("confirmed: {which}{by}")
        }
        "dismissed" => format!("dismissed: both valid{by}"),
        other => cell(other),
    }
}

/// 6. What people said.
pub(super) fn people_section(m: &Model, name: &str) -> String {
    let key = tkey(name);
    let said = m.passport.testimony.get(&key);
    let notes = m.passport.notes.get(&key);
    if said.is_none() && notes.is_none() {
        return String::new();
    }
    let mut b = String::from(
        "## What people said\n\n_Recorded as evidence with who, where and when; none of it is a review decision._\n\n",
    );
    for t in said.into_iter().flatten() {
        let s = |k: &str| prop(t, k).unwrap_or("");
        let who = match prop(t, "on_behalf_of").filter(|w| !w.is_empty()) {
            Some(w) => format!("{} (recorded by {})", plain(w), plain(s("author"))),
            None => plain(s("author")),
        };
        let link = prop(t, "permalink")
            .filter(|p| p.starts_with("https://") || p.starts_with("http://"))
            .map(|p| {
                format!(
                    " — [message]({})",
                    p.replace(')', "%29").replace(' ', "%20")
                )
            })
            .unwrap_or_default();
        b.push_str(&format!(
            "- According to {who} ({}, {}): {}{}{link}\n",
            plain(s("channel")),
            day(s("sent_at")),
            plain(s("text")),
            if s("answers").is_empty() {
                ""
            } else {
                " _(answers an open question)_"
            },
        ));
    }
    for (a, state) in notes.into_iter().flatten() {
        let mut label = format!("{} by {}", a.kind.as_str(), plain(&a.author));
        match &a.approved_by {
            Some(by) => label.push_str(&format!(", approved by {}", plain(by))),
            None => label.push_str(", not approved"),
        }
        if *state == "stale" {
            label.push_str(" — STALE: the table changed after this was written");
        }
        b.push_str(&format!("- **{label}** — {}\n", plain(&a.text)));
    }
    b.push('\n');
    b
}

/// 7. Open questions.
pub(super) fn questions_section(m: &Model, name: &str) -> String {
    let Some(qs) = m.passport.questions.get(&tkey(name)) else {
        return String::new();
    };
    let rows: Vec<Vec<String>> = qs
        .iter()
        .map(|q| {
            let st = m.input.passport.questions.get(&q.id.to_string());
            let addressee = st
                .and_then(|s| s.addressee.clone())
                .or_else(|| strings(q, "addressees").into_iter().next())
                .unwrap_or_else(|| "—".into());
            vec![
                cell(prop(q, "text").unwrap_or(&q.name)),
                code(prop(q, "question_type").unwrap_or("")),
                st.map_or_else(|| "open".into(), |s| cell(&s.status)),
                cell(&addressee),
                if q.properties.get("blocking").and_then(Value::as_bool) == Some(true) {
                    "yes".into()
                } else {
                    String::new()
                },
            ]
        })
        .collect();
    let mut b = String::from(
        "## Open questions\n\nWhat EKOS could not settle from code and data, highest priority first (`ekos questions show <id>`).\n\n",
    );
    b.push_str(&table(
        &["question", "type", "status", "addressee", "blocking"],
        &rows,
    ));
    b.push('\n');
    b
}

/// A routine with its own page is a link to it, as the "Read by" / "Written by" lists were.
fn actor_name(m: &Model, page: &str, a: &ekos_kir::interaction::Actor) -> String {
    if a.kind == ekos_kir::interaction::ActorKind::Routine
        && m.routines.contains_key(&table_key(&a.name))
    {
        link(page, &Model::routine_path(&a.name), &a.name)
    } else {
        code(&a.name)
    }
}

fn declared_words(c: &Cell) -> String {
    let ops: Vec<&str> = c.declared.iter().map(|o| o.as_str()).collect();
    ops.join(", ")
}

fn observed_words(c: &Cell) -> String {
    let ops: Vec<String> = c
        .observed
        .iter()
        .map(|(op, o)| format!("{} ×{}", op.as_str(), o.calls))
        .collect();
    ops.join(", ")
}

fn window_words(c: &Cell) -> String {
    let Some(o) = c.observed.values().next() else {
        return String::new();
    };
    let from = o.window_start.as_deref().map(day).unwrap_or("unknown");
    let cut = if o.truncated { " (truncated)" } else { "" };
    format!("{from} – {}{cut}", day(&o.window_end))
}

/// The table's open interaction findings, under its writers and readers.
fn interaction_findings(m: &Model, name: &str) -> String {
    let current = &m.input.passport.interaction_findings;
    let rows: Vec<Vec<String>> = m
        .input
        .objects
        .iter()
        .filter(|o| custom(o) == Some("InteractionFinding") && current.contains(&o.id.to_string()))
        .filter(|o| prop(o, "table").is_some_and(|t| tkey(t) == tkey(name)))
        .map(|o| {
            vec![
                cell(&prop(o, "finding").unwrap_or("").replace('_', " ")),
                code(prop(o, "actor").unwrap_or("-")),
                strings(o, "operations").join(", "),
            ]
        })
        .collect();
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "Open findings (`ekos interaction findings`):\n\n{}\n",
        table(&["finding", "actor", "operations"], &rows)
    )
}

/// RFC 0256 §3.5: "Writers and readers" — every actor's operations on the table, declared by code
/// and observed in the database, writers first. `None` when the matrix was not given.
pub(super) fn interaction_section(m: &Model, name: &str, page: &str) -> Option<String> {
    let matrix = m.input.passport.interaction.as_ref()?;
    let Some(row) = matrix.row(name) else {
        return Some(String::new());
    };
    let mut cells: Vec<_> = row.iter().collect();
    cells.sort_by_key(|(a, c)| (!c.writes(), a.kind, a.name.clone()));
    let rows: Vec<Vec<String>> = cells
        .iter()
        .map(|(a, c)| {
            vec![
                a.kind.as_str().to_string(),
                actor_name(m, page, a),
                declared_words(c),
                observed_words(c),
                window_words(c),
            ]
        })
        .collect();
    let note = if m.input.passport.observed_facts == 0 {
        "_Declared by code only: no usage statistics were read for this source, so a writer outside the code would not show here._"
    } else {
        "_Observed counts come from the engine's statement statistics over the window shown; \"not observed\" is not \"never happens\"._"
    };
    let mut b = format!(
        "## Writers and readers\n\n{note}\n\n{}",
        table(&["actor", "name", "declared", "observed", "window"], &rows)
    );
    b.push('\n');
    b.push_str(&interaction_findings(m, name));
    Some(b)
}

/// RFC 0255 §3.5: every source's recorded engine capabilities (`SourceCapabilities`), by source.
fn source_capabilities(
    objects: &[KirObject],
) -> Vec<ekos_kir::source_capabilities::SourceCapabilities> {
    let mut v: Vec<_> = objects
        .iter()
        .filter_map(ekos_kir::source_capabilities::from_object)
        .collect();
    v.sort_by(|a, b| a.source.cmp(&b.source));
    v
}

/// RFC 0255 §3.5: what each source's engine provides and what it cannot, at the top of the
/// source: an analyst reading "no relationships" sees whether that is a fact about the database
/// or a limit of the engine. Empty when no source recorded its capabilities.
pub fn source_engines_section(objects: &[KirObject]) -> String {
    let all = source_capabilities(objects);
    if all.is_empty() {
        return String::new();
    }
    let mut b = String::from(
        "\n## Source engines\n\nWhat each source's engine provides (RFC 0255). A capability that is \
         not provided is a limit of the engine, not a fact about the database.\n",
    );
    for sc in &all {
        b.push_str(&format!(
            "\n**{}** — engine {}, capability list v{}{}\n\n",
            plain(&sc.source),
            code(&sc.engine),
            sc.capabilities_version,
            sc.conformance_run
                .as_deref()
                .map(|r| format!(", conformance run {}", code(r)))
                .unwrap_or_default()
        ));
        b.push_str(&ekos_kir::source_capabilities::markdown_table(sc));
        for d in &sc.downgrades {
            b.push_str(&format!("\n- {}", plain(d)));
        }
        if !sc.downgrades.is_empty() {
            b.push('\n');
        }
    }
    b
}

/// RFC 0255 §3.5: a line saying that `capability` is a limit of the engine, when the project has
/// exactly one recorded source and its engine does not provide it.
pub fn engine_limit(
    objects: &[KirObject],
    capability: ekos_kir::source_capabilities::Capability,
) -> String {
    let all = source_capabilities(objects);
    let [sc] = all.as_slice() else {
        return String::new();
    };
    let s = sc.capabilities.get(capability);
    if s.is_provided() {
        return String::new();
    }
    format!(
        "\n_The source engine {} has `{}` {}{}: what is missing here may be a limit of the engine, \
         not a fact about the database._\n",
        code(&sc.engine),
        capability.name(),
        s.word(),
        if s.detail().is_empty() {
            String::new()
        } else {
            format!(" ({})", plain(s.detail()))
        }
    )
}
