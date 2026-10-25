//! RFC 0214 — source-estate documentation: Markdown + Mermaid pages about a database estate,
//! from the ledger only.
//!
//! Pure: [`build`] turns objects, relationships and what the CLI read beside the ledger (review
//! scope, sign-offs, licences, provenance) into pages in the RFC 0208 page format — front matter
//! `title`/`order`/`ekos_generated: true`, `<dir>/index.md` the parent of the other pages in
//! `<dir>` — which the lakehouse-docs publishers read unchanged.
//!
//! Two gates run here, at render time:
//! * **No values.** A column profile contributes counts only (nulls, distinct, rows); `min`,
//!   `max` and seed rows are never read.
//! * **Licences.** Text (descriptions, business names, check expressions, statement conditions)
//!   is shown only when every source it came from has a known licence; otherwise it is dropped
//!   and counted, and the home page says so.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Bump when the output of [`build`] changes for the same input.
pub const ESTATE_VERSION: u32 = 1;

/// Where a table stands in the RFC 0213 scope review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeEntry {
    pub in_scope: bool,
    /// business, transient, import, log, config, translation, …
    pub class: String,
    /// A person decided it (else it is the review's proposal).
    pub decided: bool,
}

/// One review subject's state, for the home page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubjectLine {
    pub subject: String,
    pub items: usize,
    pub blocking: usize,
    /// none, valid, stale, revoked
    pub state: String,
    pub signed_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    pub project: String,
    pub committed_at: Option<String>,
    pub git_head: Option<String>,
}

pub struct EstateInput<'a> {
    /// The latest version of every object.
    pub objects: &'a [KirObject],
    pub relationships: &'a [KirRelationship],
    /// Lower-case table name → scope. A table missing here counts as in scope.
    pub scope: &'a BTreeMap<String, ScopeEntry>,
    pub subjects: &'a [SubjectLine],
    /// Source or database name → SPDX id, `unknown` or `none`.
    pub licences: &'a BTreeMap<String, String>,
    pub provenance: &'a Provenance,
    /// The current conflict ids (`.ekos/conflicts/current.json`); `None` = all.
    pub current_conflicts: Option<&'a BTreeSet<String>>,
    /// Diagram size cap (RFC 0214 §3.3).
    pub max_er_tables: usize,
}

/// One page: a path under the docs root and its full text (front matter included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstatePage {
    pub path: String,
    pub title: String,
    pub body: String,
}

/// Whether a licence lets its source's text be published.
pub fn licence_allows_text(licence: &str) -> bool {
    !matches!(licence, "" | "unknown" | "none")
}

/// The SPDX id a licence file's text states, recognised by the phrases each licence carries.
pub fn detect_licence(text: &str) -> Option<&'static str> {
    // A licence names itself in its title; its body may name others (the GPL-2.0 text mentions the
    // Library GPL), so only the opening is read.
    let head: String = text.chars().take(600).collect();
    let t = head.to_ascii_lowercase();
    let has = |s: &str| t.contains(s);
    if has("gnu affero general public license") {
        Some("AGPL-3.0")
    } else if has("gnu lesser general public license") || has("gnu library general public license")
    {
        Some(if has("version 3") {
            "LGPL-3.0"
        } else {
            "LGPL-2.1"
        })
    } else if has("gnu general public license") {
        Some(if has("version 3") {
            "GPL-3.0"
        } else {
            "GPL-2.0"
        })
    } else if has("apache license") && has("version 2.0") {
        Some("Apache-2.0")
    } else if has("mozilla public license") && has("2.0") {
        Some("MPL-2.0")
    } else if has("eclipse public license") {
        Some(if has("2.0") { "EPL-2.0" } else { "EPL-1.0" })
    } else if has("permission is hereby granted, free of charge") {
        Some("MIT")
    } else if has("redistribution and use in source and binary forms") {
        Some("BSD-3-Clause")
    } else if has("creative commons attribution-sharealike") {
        Some("CC-BY-SA-4.0")
    } else if has("creative commons attribution") {
        Some("CC-BY-4.0")
    } else {
        None
    }
}

// ── small readers ──────────────────────────────────────────────────────────────────────────────

fn custom(o: &KirObject) -> Option<&str> {
    match &o.kind {
        ObjectKind::Custom(k) => Some(k.as_str()),
        _ => None,
    }
}

fn prop<'a>(o: &'a KirObject, k: &str) -> Option<&'a str> {
    o.properties.get(k).and_then(Value::as_str)
}

fn strings(o: &KirObject, k: &str) -> Vec<String> {
    o.properties
        .get(k)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Escape a value for a Markdown table cell.
fn cell(s: &str) -> String {
    s.replace('\r', "")
        .replace('|', "\\|")
        .replace('\n', "<br>")
}

fn code(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        format!("`{}`", cell(&s.replace('`', "'")))
    }
}

fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "_None._\n".into();
    }
    let mut out = format!(
        "| {} |\n|{}|\n",
        headers.join(" | "),
        vec!["---"; headers.len()].join("|")
    );
    for r in rows {
        out.push_str(&format!("| {} |\n", r.join(" | ")));
    }
    out
}

fn yaml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn front(title: &str, order: usize) -> String {
    format!(
        "---\ntitle: {}\norder: {order}\nekos_generated: true\nekos_estate_version: {ESTATE_VERSION}\n---\n\n",
        yaml_str(title)
    )
}

fn manual(id: &str, default: &str) -> String {
    format!("<!-- ekos:manual id=\"{id}\" -->\n{default}\n<!-- /ekos:manual -->\n")
}

/// A Mermaid identifier.
fn m_id(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// A relative link from one page to another, both paths under the docs root.
fn link(from: &str, to: &str, text: &str) -> String {
    let depth = from.matches('/').count();
    format!("[{}]({}{to})", cell(text), "../".repeat(depth))
}

/// A table name as one key: SQL quotes removed, upper case (`"AD_ClientShare"` = `AD_CLIENTSHARE`).
pub fn table_key(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('`')
        .to_ascii_uppercase()
}

/// A table's functional area: its name up to the first `_`.
pub fn area_of(table: &str) -> String {
    match table.split_once('_') {
        Some((p, _)) if !p.is_empty() => p.to_ascii_uppercase(),
        _ => "OTHER".into(),
    }
}

// ── the model ──────────────────────────────────────────────────────────────────────────────────

struct Fk<'a> {
    from: String,
    to: String,
    rel: &'a KirRelationship,
}

struct Model<'a> {
    input: &'a EstateInput<'a>,
    /// Upper-case name → the table object documented for it.
    tables: BTreeMap<String, &'a KirObject>,
    fks: Vec<Fk<'a>>,
    model_tables: HashMap<String, &'a KirObject>,
    write_behaviour: HashMap<String, &'a KirObject>,
    changes: HashMap<String, Vec<&'a KirObject>>,
    conflicts: HashMap<String, Vec<&'a KirObject>>,
    readers: HashMap<String, BTreeSet<(String, String)>>,
    writers: HashMap<String, BTreeSet<(String, String)>>,
    routines: BTreeMap<String, Vec<&'a KirObject>>,
    statements: HashMap<KirId, Vec<&'a KirObject>>,
    callers: HashMap<KirId, BTreeSet<(String, String)>>,
    dropped: std::cell::RefCell<BTreeMap<String, usize>>,
}

/// Which table object documents a name: live first, then a DDL table with a dialect (never a
/// table recovered from a PDF), the `oracle` dialect before others, then the lowest id.
fn better(a: &KirObject, b: &KirObject) -> bool {
    let rank = |o: &KirObject| {
        (
            prop(o, "origin") != Some("live"),
            prop(o, "dialect") != Some("oracle"),
            o.id.to_string(),
        )
    };
    rank(a) < rank(b)
}

/// Keep the better of two objects for one key ([`better`]).
fn keep_better<'a>(map: &mut HashMap<String, &'a KirObject>, key: String, o: &'a KirObject) {
    if map.get(&key).is_none_or(|cur| better(o, cur)) {
        map.insert(key, o);
    }
}

fn by_better(a: &KirObject, b: &KirObject) -> std::cmp::Ordering {
    if better(a, b) {
        std::cmp::Ordering::Less
    } else if better(b, a) {
        std::cmp::Ordering::Greater
    } else {
        std::cmp::Ordering::Equal
    }
}

impl<'a> Model<'a> {
    fn new(input: &'a EstateInput<'a>) -> Self {
        let mut m = Self {
            input,
            tables: BTreeMap::new(),
            fks: Vec::new(),
            model_tables: HashMap::new(),
            write_behaviour: HashMap::new(),
            changes: HashMap::new(),
            conflicts: HashMap::new(),
            readers: HashMap::new(),
            writers: HashMap::new(),
            routines: BTreeMap::new(),
            statements: HashMap::new(),
            callers: HashMap::new(),
            dropped: Default::default(),
        };
        let mut tables: HashMap<String, &KirObject> = HashMap::new();
        for o in input.objects {
            if o.kind == ObjectKind::Table {
                // Live, or DDL with a dialect; a table read from a PDF is not one here.
                if prop(o, "origin") == Some("live") || prop(o, "dialect").is_some() {
                    keep_better(&mut tables, table_key(&o.name), o);
                }
            } else {
                m.add_object(o);
            }
        }
        m.tables = tables.into_iter().collect();
        let by_id: HashMap<KirId, &KirObject> = input.objects.iter().map(|o| (o.id, o)).collect();
        let chosen: HashMap<KirId, String> =
            m.tables.iter().map(|(k, o)| (o.id, k.clone())).collect();
        let mut seen = BTreeSet::new();
        for r in input.relationships {
            m.add_relationship(r, &by_id, &chosen, &mut seen);
        }
        m.sort();
        m
    }

    fn add_object(&mut self, o: &'a KirObject) {
        let table = |k: &str| prop(o, k).map(table_key);
        match custom(o) {
            Some("ModelTable") => {
                if let Some(t) = table("table_name") {
                    self.model_tables.insert(t, o);
                }
            }
            Some("WriteBehaviour") => {
                if let Some(t) = table("table") {
                    keep_better(&mut self.write_behaviour, t, o);
                }
            }
            Some("SchemaChange") => {
                if let Some(t) = table("table") {
                    self.changes.entry(t).or_default().push(o);
                }
            }
            Some("ConflictingEvidence") => {
                let current = self
                    .input
                    .current_conflicts
                    .is_none_or(|c| c.contains(&o.id.to_string()));
                if let (true, Some("open"), Some(t)) =
                    (current, prop(o, "status"), table("subject_name"))
                {
                    self.conflicts.entry(t).or_default().push(o);
                }
            }
            Some(k @ ("Procedure" | "View" | "Trigger")) => self.add_code(k, o),
            _ => {}
        }
    }

    /// A routine, view or trigger: what it reads and writes, and a routine itself.
    fn add_code(&mut self, kind: &str, o: &'a KirObject) {
        let who = (kind.to_string(), o.name.clone());
        for t in strings(o, "reads") {
            self.readers
                .entry(table_key(&t))
                .or_default()
                .insert(who.clone());
        }
        let mut written: Vec<String> = ["writes", "inserts", "updates", "deletes"]
            .iter()
            .flat_map(|k| strings(o, k))
            .collect();
        if kind == "Trigger" {
            written.extend(prop(o, "table").map(str::to_string));
        }
        for t in written {
            self.writers
                .entry(table_key(&t))
                .or_default()
                .insert(who.clone());
        }
        if kind == "Procedure" {
            self.routines.entry(table_key(&o.name)).or_default().push(o);
        }
    }

    fn add_relationship(
        &mut self,
        r: &'a KirRelationship,
        by_id: &HashMap<KirId, &'a KirObject>,
        chosen: &HashMap<KirId, String>,
        seen: &mut BTreeSet<(String, String, String)>,
    ) {
        match &r.kind {
            RelationshipKind::ForeignKey => {
                let (Some(f), Some(t)) = (chosen.get(&r.from), chosen.get(&r.to)) else {
                    return;
                };
                let label = r
                    .properties
                    .get("fk_desc")
                    .or_else(|| r.properties.get("constraint"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if seen.insert((f.clone(), t.clone(), label)) {
                    self.fks.push(Fk {
                        from: f.clone(),
                        to: t.clone(),
                        rel: r,
                    });
                }
            }
            RelationshipKind::Contains => {
                if let Some(s) = by_id
                    .get(&r.to)
                    .filter(|o| custom(o) == Some("ProcedureStatement"))
                {
                    self.statements.entry(r.from).or_default().push(s);
                }
            }
            RelationshipKind::Calls => {
                if let Some(c) = by_id.get(&r.from) {
                    self.callers
                        .entry(r.to)
                        .or_default()
                        .insert((c.kind.to_string(), c.name.clone()));
                }
            }
            _ => {}
        }
    }

    /// Every list in a fixed order, whatever order the ledger returned.
    fn sort(&mut self) {
        for v in self.routines.values_mut() {
            v.sort_by(|a, b| by_better(a, b));
        }
        for v in self.changes.values_mut() {
            v.sort_by(|a, b| {
                (prop(b, "applied_at"), b.id.to_string())
                    .cmp(&(prop(a, "applied_at"), a.id.to_string()))
            });
        }
        for v in self.conflicts.values_mut() {
            v.sort_by(|a, b| (&a.name, a.id.to_string()).cmp(&(&b.name, b.id.to_string())));
        }
        self.fks.sort_by(|a, b| {
            (&a.from, &a.to, a.rel.id.to_string()).cmp(&(&b.from, &b.to, b.rel.id.to_string()))
        });
        for v in self.statements.values_mut() {
            v.sort_by_key(|s| {
                (
                    s.properties
                        .get("index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    s.id.to_string(),
                )
            });
        }
    }

    /// The sources an object's text came from.
    fn sources_of(o: &KirObject) -> Vec<String> {
        let mut s: Vec<String> = strings(o, "sources");
        for k in ["source", "database"] {
            if let Some(v) = prop(o, k).filter(|v| !s.iter().any(|x| x == v)) {
                s.push(v.to_string());
            }
        }
        s
    }

    /// `text` when every source of `o` has a known licence; otherwise `None`, counted.
    fn text(&self, o: &KirObject, text: Option<&str>) -> Option<String> {
        let text = text.filter(|t| !t.trim().is_empty())?;
        let sources = Self::sources_of(o);
        let ok = !sources.is_empty()
            && sources.iter().all(|s| {
                licence_allows_text(self.input.licences.get(s).map_or("unknown", String::as_str))
            });
        if ok {
            Some(text.to_string())
        } else {
            let key = if sources.is_empty() {
                "(no source)".to_string()
            } else {
                sources.join("+")
            };
            *self.dropped.borrow_mut().entry(key).or_default() += 1;
            None
        }
    }

    fn scope(&self, table: &str) -> ScopeEntry {
        self.input
            .scope
            .get(&table.to_ascii_lowercase())
            .cloned()
            .unwrap_or(ScopeEntry {
                in_scope: true,
                class: "unclassified".into(),
                decided: false,
            })
    }

    fn in_scope(&self, table: &str) -> bool {
        self.scope(table).in_scope
    }

    fn table_path(&self, table: &str) -> String {
        format!("data-model/{}/{}.md", slug(&area_of(table)), slug(table))
    }

    fn area_path(area: &str) -> String {
        format!("data-model/{}/index.md", slug(area))
    }

    /// A link to a table's page when it has one, else its name in code.
    fn table_ref(&self, from: &str, table: &str) -> String {
        if self.tables.contains_key(table) && self.in_scope(table) {
            link(from, &self.table_path(table), table)
        } else {
            code(table)
        }
    }

    /// Every statement under a routine, nested ones included, in source order (a statement's
    /// `index` is its pre-order position in the routine).
    fn descendants(&self, routine: KirId) -> Vec<&'a KirObject> {
        let mut out = Vec::new();
        let mut stack = vec![routine];
        let mut seen = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            for s in self.statements.get(&id).into_iter().flatten() {
                if seen.insert(s.id) {
                    out.push(*s);
                    stack.push(s.id);
                }
            }
        }
        out.sort_by_key(|s| {
            (
                s.properties
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
                s.id.to_string(),
            )
        });
        out
    }

    fn routine_path(name: &str) -> String {
        format!("logic/{}.md", slug(name))
    }
}

// ── ER diagrams ────────────────────────────────────────────────────────────────────────────────

/// An `erDiagram` over `tables` with the FKs among them — only those touching `center` when one
/// is given (a table's neighbourhood is a star, not every key among its neighbours). Over `cap`
/// tables, the most connected ones are drawn; the rest are returned for the page to list.
fn er(
    model: &Model,
    tables: &BTreeSet<String>,
    cap: usize,
    center: Option<&str>,
) -> (String, Vec<String>) {
    let fks: Vec<&Fk> = model
        .fks
        .iter()
        .filter(|f| tables.contains(&f.from) && tables.contains(&f.to))
        .filter(|f| center.is_none_or(|c| f.from == c || f.to == c))
        .collect();
    let mut degree: BTreeMap<&str, usize> = tables.iter().map(|t| (t.as_str(), 0)).collect();
    for f in &fks {
        *degree.entry(&f.from).or_default() += 1;
        *degree.entry(&f.to).or_default() += 1;
    }
    let mut ranked: Vec<(&str, usize)> = degree.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let drawn: BTreeSet<&str> = ranked.iter().take(cap).map(|(t, _)| *t).collect();
    let left: Vec<String> = ranked
        .iter()
        .skip(cap)
        .map(|(t, _)| t.to_string())
        .collect();
    let mut body = String::from("```mermaid\nerDiagram\n");
    let mut edges = BTreeSet::new();
    for f in fks {
        if drawn.contains(f.from.as_str()) && drawn.contains(f.to.as_str()) {
            edges.insert((f.to.clone(), f.from.clone()));
        }
    }
    for (parent, child) in &edges {
        body.push_str(&format!(
            "  {} ||--o{{ {} : \"\"\n",
            m_id(parent),
            m_id(child)
        ));
    }
    let linked: BTreeSet<&str> = edges
        .iter()
        .flat_map(|(a, b)| [a.as_str(), b.as_str()])
        .collect();
    for t in &drawn {
        if !linked.contains(t) {
            body.push_str(&format!("  {} {{\n  }}\n", m_id(t)));
        }
    }
    body.push_str("```\n");
    (body, left)
}

// ── pages ──────────────────────────────────────────────────────────────────────────────────────

fn page(path: String, title: &str, order: usize, body: String) -> EstatePage {
    EstatePage {
        body: format!("{}# {title}\n\n{body}", front(title, order)),
        title: title.to_string(),
        path,
    }
}

fn table_page(m: &Model, name: &str, t: &KirObject, order: usize) -> EstatePage {
    let path = m.table_path(name);
    let mt = m.model_tables.get(name).copied();
    let mut b = String::new();
    let sc = m.scope(name);
    let desc = m
        .text(t, prop(t, "description"))
        .or_else(|| mt.and_then(|mt| m.text(mt, prop(mt, "description"))));
    if let Some(d) = desc {
        b.push_str(&format!("{d}\n\n"));
    }
    let area = area_of(name);
    b.push_str(&format!(
        "Area {} · class **{}**{} · ",
        link(&path, &Model::area_path(&area), &area),
        sc.class,
        if sc.decided {
            " (reviewed)"
        } else {
            " (proposed)"
        }
    ));
    match prop(t, "origin") {
        Some("live") => b.push_str(&format!(
            "read from database `{}` (owner `{}`)",
            prop(t, "database").unwrap_or("?"),
            prop(t, "owner").unwrap_or("?")
        )),
        _ => b.push_str(&format!("from DDL ({})", prop(t, "dialect").unwrap_or("?"))),
    }
    if let Some(rows) = t
        .properties
        .get("profile")
        .and_then(|p| p.get("row_count"))
        .and_then(Value::as_u64)
    {
        b.push_str(&format!(" · {rows} row(s) when profiled"));
    }
    b.push_str("\n\n");
    if let Some(mt) = mt {
        b.push_str(&format!(
            "Application model: `{}`{}.\n\n",
            prop(mt, "model_class").unwrap_or("?"),
            prop(mt, "access_level")
                .map(|a| format!(", access level {a}"))
                .unwrap_or_default()
        ));
    }

    // Columns: counts only, never values.
    let model_cols = mt
        .and_then(|mt| mt.properties.get("columns"))
        .and_then(Value::as_object);
    let pk: BTreeSet<String> = strings(t, "primary_key")
        .into_iter()
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let mut rows = Vec::new();
    for c in t
        .properties
        .get("columns")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let cname = c.get("name").and_then(Value::as_str).unwrap_or("");
        let mc = model_cols.and_then(|mc| {
            mc.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(cname))
                .map(|(_, v)| v)
        });
        let code_ref = mc
            .and_then(|v| v.get("references"))
            .and_then(Value::as_str)
            .map(|t| m.table_ref(&path, &table_key(t)))
            .unwrap_or_default();
        let business = mc
            .and_then(|v| v.get("business_name"))
            .and_then(Value::as_str)
            .and_then(|s| mt.and_then(|mt| m.text(mt, Some(s))))
            .unwrap_or_default();
        let profile = c.get("profile");
        let count = |k: &str| {
            profile
                .and_then(|p| p.get(k))
                .and_then(Value::as_u64)
                .map(|n| n.to_string())
                .unwrap_or_default()
        };
        rows.push(vec![
            code(cname),
            cell(c.get("data_type").and_then(Value::as_str).unwrap_or("")),
            if pk.contains(&cname.to_ascii_uppercase())
                || c.get("primary_key").and_then(Value::as_bool) == Some(true)
            {
                "PK".into()
            } else {
                String::new()
            },
            if c.get("not_null").and_then(Value::as_bool) == Some(true) {
                "yes".into()
            } else {
                String::new()
            },
            cell(&business),
            code_ref,
            count("nulls"),
            count("distinct"),
        ]);
    }
    b.push_str("## Columns\n\n");
    b.push_str(&table(
        &[
            "column",
            "type",
            "key",
            "not null",
            "business name",
            "references (code)",
            "nulls",
            "distinct",
        ],
        &rows,
    ));
    b.push_str("\n*references (code)* is the table the application model's code says a column points to; *Foreign keys* below are the constraints the database declares. Counts come from the profile; no value is ever shown.\n\n");

    let checks: Vec<Vec<String>> = t
        .properties
        .get("check_constraints")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            let expr = m.text(t, c.get("expression").and_then(Value::as_str))?;
            Some(vec![
                code(c.get("name").and_then(Value::as_str).unwrap_or("")),
                code(&expr),
            ])
        })
        .collect();
    if !checks.is_empty() {
        b.push_str("## Check constraints\n\n");
        b.push_str(&table(&["name", "expression"], &checks));
        b.push('\n');
    }

    // Keys and the neighbourhood.
    let out: Vec<&Fk> = m.fks.iter().filter(|f| f.from == name).collect();
    let inc: Vec<&Fk> = m.fks.iter().filter(|f| f.to == name).collect();
    let fk_row = |f: &Fk, other: &str| {
        vec![
            m.table_ref(&path, other),
            cell(
                f.rel
                    .properties
                    .get("fk_desc")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
            cell(
                f.rel
                    .properties
                    .get("on_delete")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
            ),
        ]
    };
    b.push_str("## Foreign keys\n\n**References**\n\n");
    let rows: Vec<Vec<String>> = out.iter().map(|f| fk_row(f, &f.to)).collect();
    b.push_str(&table(&["table", "columns", "on delete"], &rows));
    b.push_str("\n**Referenced by**\n\n");
    let rows: Vec<Vec<String>> = inc.iter().map(|f| fk_row(f, &f.from)).collect();
    b.push_str(&table(&["table", "columns", "on delete"], &rows));
    let mut hood: BTreeSet<String> = out.iter().map(|f| f.to.clone()).collect();
    hood.extend(inc.iter().map(|f| f.from.clone()));
    if !hood.is_empty() {
        hood.insert(name.to_string());
        let (diagram, left) = er(m, &hood, m.input.max_er_tables, Some(name));
        b.push('\n');
        b.push_str(&diagram);
        if !left.is_empty() {
            b.push_str(&format!("\n{} more neighbour(s) not drawn.\n", left.len()));
        }
    }
    b.push('\n');

    // How rows change.
    if let Some(w) = m.write_behaviour.get(name) {
        b.push_str("## Write behaviour\n\n");
        b.push_str(&format!(
            "- updates: **{}**\n- deletes: **{}**\n- change log: **{}**\n- evidence: {} citation(s)\n\n",
            prop(w, "update_class").unwrap_or("unknown"),
            strings(w, "delete_classes").join(", "),
            prop(w, "change_log").unwrap_or("unknown"),
            w.properties
                .get("citation_count")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        ));
    }
    for (title, map) in [("Read by", &m.readers), ("Written by", &m.writers)] {
        if let Some(who) = map.get(name) {
            b.push_str(&format!("## {title}\n\n"));
            let rows: Vec<Vec<String>> = who
                .iter()
                .map(|(k, n)| {
                    let shown = if k == "Procedure" && m.routines.contains_key(&table_key(n)) {
                        link(&path, &Model::routine_path(n), n)
                    } else {
                        code(n)
                    };
                    vec![k.clone(), shown]
                })
                .collect();
            b.push_str(&table(&["kind", "name"], &rows));
            b.push('\n');
        }
    }

    if let Some(ch) = m.changes.get(name) {
        b.push_str(&format!(
            "## Schema history\n\n{} change(s); the latest:\n\n",
            ch.len()
        ));
        let rows: Vec<Vec<String>> = ch
            .iter()
            .take(15)
            .map(|c| {
                vec![
                    cell(prop(c, "applied_at").unwrap_or("")),
                    cell(prop(c, "release").unwrap_or("")),
                    code(prop(c, "operation").unwrap_or("")),
                    cell(
                        c.properties
                            .get("column")
                            .and_then(|v| v.get("name"))
                            .and_then(Value::as_str)
                            .unwrap_or(""),
                    ),
                    cell(&strings(c, "tickets").join(", ")),
                ]
            })
            .collect();
        b.push_str(&table(
            &["applied", "release", "operation", "column", "tickets"],
            &rows,
        ));
        b.push('\n');
    }
    if let Some(cs) = m.conflicts.get(name) {
        b.push_str("## Open conflicts\n\n");
        let rows: Vec<Vec<String>> = cs
            .iter()
            .map(|c| {
                vec![
                    code(prop(c, "conflict_type").unwrap_or("")),
                    code(prop(c, "attribute").unwrap_or("")),
                    cell(prop(c, "explanation").unwrap_or("unexplained")),
                ]
            })
            .collect();
        b.push_str(&table(&["type", "attribute", "explanation"], &rows));
        b.push('\n');
    }
    b.push_str("## Notes\n\n");
    b.push_str(&manual(
        &format!("table.{}.notes", slug(name)),
        "_Business notes about this table._",
    ));
    page(path, name, order, b)
}

fn area_pages(m: &Model, pages: &mut Vec<EstatePage>) -> Vec<(String, usize, usize)> {
    let mut areas: BTreeMap<String, Vec<&String>> = BTreeMap::new();
    for name in m.tables.keys() {
        areas.entry(area_of(name)).or_default().push(name);
    }
    let mut summary = Vec::new();
    for (i, (area, names)) in areas.iter().enumerate() {
        let path = Model::area_path(area);
        let inside: BTreeSet<String> = names
            .iter()
            .filter(|n| m.in_scope(n))
            .map(|n| n.to_string())
            .collect();
        summary.push((area.clone(), names.len(), inside.len()));
        let mut b = format!("{} table(s), {} in scope.\n\n", names.len(), inside.len());
        if !inside.is_empty() {
            let (diagram, left) = er(m, &inside, m.input.max_er_tables, None);
            b.push_str(&diagram);
            if !left.is_empty() {
                b.push_str(&format!(
                    "\nThe diagram shows the {} most connected tables; {} more are listed below.\n",
                    m.input.max_er_tables,
                    left.len()
                ));
            }
            b.push('\n');
        }
        let rows: Vec<Vec<String>> = names
            .iter()
            .map(|n| {
                let t = m.tables[*n];
                let sc = m.scope(n);
                vec![
                    m.table_ref(&path, n),
                    sc.class.clone(),
                    if sc.in_scope {
                        "in".into()
                    } else {
                        "out".into()
                    },
                    t.properties
                        .get("profile")
                        .and_then(|p| p.get("row_count"))
                        .and_then(Value::as_u64)
                        .map(|r| r.to_string())
                        .unwrap_or_default(),
                    m.text(t, prop(t, "description"))
                        .or_else(|| {
                            m.model_tables
                                .get(n.as_str())
                                .and_then(|mt| m.text(mt, prop(mt, "description")))
                        })
                        .map(|d| cell(&d))
                        .unwrap_or_default(),
                ]
            })
            .collect();
        b.push_str(&table(
            &["table", "class", "scope", "rows", "description"],
            &rows,
        ));
        pages.push(page(path.clone(), &format!("Area {area}"), i + 1, b));
        for (j, n) in names.iter().enumerate() {
            if m.in_scope(n) {
                pages.push(table_page(m, n, m.tables[*n], j + 1));
            }
        }
    }
    summary
}

fn data_model_index(m: &Model, summary: &[(String, usize, usize)]) -> EstatePage {
    let path = "data-model/index.md".to_string();
    let in_scope: usize = summary.iter().map(|s| s.2).sum();
    let mut b = format!(
        "{} table(s) in {} functional area(s) (by name prefix); {} in scope, each with a page. The scope comes from the source-model review: decided where a person decided, proposed otherwise.\n\n",
        m.tables.len(),
        summary.len(),
        in_scope
    );
    let rows: Vec<Vec<String>> = summary
        .iter()
        .map(|(a, n, i)| {
            vec![
                link(&path, &Model::area_path(a), a),
                n.to_string(),
                i.to_string(),
            ]
        })
        .collect();
    b.push_str(&table(&["area", "tables", "in scope"], &rows));
    let mut classes: BTreeMap<String, usize> = BTreeMap::new();
    for n in m.tables.keys() {
        let sc = m.scope(n);
        if !sc.in_scope {
            *classes.entry(sc.class).or_default() += 1;
        }
    }
    if !classes.is_empty() {
        b.push_str("\n**Out of scope, by class:** ");
        b.push_str(
            &classes
                .iter()
                .map(|(c, n)| format!("{c} {n}"))
                .collect::<Vec<_>>()
                .join(" · "),
        );
        b.push('\n');
    }
    page(path, "Data model", 2, b)
}

/// A glossary term's use: `TABLE.COLUMN`, description, where it is stated.
type Use = (String, String, String);

/// The columns a glossary row links; a term like "Tenant" names a column in nearly every table.
const GLOSSARY_COLUMNS: usize = 10;

fn glossary_pages(m: &Model, pages: &mut Vec<EstatePage>) -> usize {
    // business name → (table.column, description, source)
    let mut terms: BTreeMap<String, BTreeSet<(String, String, String)>> = BTreeMap::new();
    for (table_name, mt) in &m.model_tables {
        if !m.tables.contains_key(table_name) || !m.in_scope(table_name) {
            continue;
        }
        let Some(cols) = mt.properties.get("columns").and_then(Value::as_object) else {
            continue;
        };
        for (col, v) in cols {
            let Some(term) = m.text(mt, v.get("business_name").and_then(Value::as_str)) else {
                continue;
            };
            let desc = m
                .text(mt, v.get("description").and_then(Value::as_str))
                .unwrap_or_default();
            let src = v
                .get("source")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            terms.entry(term).or_default().insert((
                format!("{table_name}.{}", col.to_ascii_uppercase()),
                desc,
                src,
            ));
        }
    }
    let mut letters: BTreeMap<char, Vec<(&String, &BTreeSet<Use>)>> = BTreeMap::new();
    for (t, uses) in &terms {
        let c = t
            .chars()
            .next()
            .map(|c| c.to_ascii_uppercase())
            .filter(char::is_ascii_alphabetic)
            .unwrap_or('#');
        letters.entry(c).or_default().push((t, uses));
    }
    let index = "glossary/index.md".to_string();
    let mut b = format!(
        "{} business term(s) from the application model's column names, each with the columns it names and the code that states it.\n\n",
        terms.len()
    );
    let mut links = Vec::new();
    for (i, (c, entries)) in letters.iter().enumerate() {
        let path = format!(
            "glossary/{}.md",
            if *c == '#' {
                "other".into()
            } else {
                c.to_ascii_lowercase().to_string()
            }
        );
        links.push(format!(
            "{} ({})",
            link(&index, &path, &c.to_string()),
            entries.len()
        ));
        let mut rows = Vec::new();
        for (term, uses) in entries {
            // One row per term: its first description, the columns it names (the first ten
            // linked), and where the first is stated.
            let desc = uses
                .iter()
                .map(|u| u.1.as_str())
                .find(|d| !d.is_empty())
                .unwrap_or("");
            let mut cols: Vec<String> = uses
                .iter()
                .take(GLOSSARY_COLUMNS)
                .map(|(tc, _, _)| {
                    let table_name = tc.split('.').next().unwrap_or("");
                    if m.tables.contains_key(table_name) && m.in_scope(table_name) {
                        link(&path, &m.table_path(table_name), tc)
                    } else {
                        code(tc)
                    }
                })
                .collect();
            if uses.len() > GLOSSARY_COLUMNS {
                cols.push(format!("and {} more", uses.len() - GLOSSARY_COLUMNS));
            }
            rows.push(vec![
                format!("**{}**", cell(term)),
                cell(desc),
                cols.join(", "),
                code(&uses.iter().next().map(|u| u.2.clone()).unwrap_or_default()),
            ]);
        }
        let pb = table(&["term", "description", "columns", "stated in"], &rows);
        pages.push(page(path, &format!("Glossary {c}"), i + 1, pb));
    }
    b.push_str(&links.join(" · "));
    b.push('\n');
    pages.push(page(index, "Glossary", 3, b));
    terms.len()
}

/// RFC 0214: the `DocAction` methods a document class implements. Statuses follow iDempiere's
/// `DocAction` interface; per-class transitions are not recovered, so none are drawn.
const DOC_ACTIONS: &[(&str, &str)] = &[
    ("prepareIt", "Prepare"),
    ("approveIt", "Approve"),
    ("rejectIt", "Reject"),
    ("completeIt", "Complete"),
    ("voidIt", "Void"),
    ("closeIt", "Close"),
    ("reverseCorrectIt", "Reverse – correct"),
    ("reverseAccrualIt", "Reverse – accrual"),
    ("reActivateIt", "Re-activate"),
    ("invalidateIt", "Invalidate"),
    ("unlockIt", "Unlock"),
];

fn lifecycle_page(m: &Model) -> (EstatePage, usize) {
    let path = "lifecycles/index.md".to_string();
    let mut docs: Vec<(&String, &KirObject)> = m
        .model_tables
        .iter()
        .filter(|(t, mt)| {
            mt.properties.contains_key("document_lifecycle")
                && (!m.tables.contains_key(*t) || m.in_scope(t))
        })
        .map(|(t, mt)| (t, *mt))
        .collect();
    docs.sort_by(|a, b| a.0.cmp(b.0));
    let mut b = format!(
        "{} document type(s) whose business class implements iDempiere's `DocAction` interface, with the line where each action's method starts — the place to read what that action does for this document. Which statuses an action sets is not recovered yet.\n\n",
        docs.len()
    );
    let mut headers = vec!["table", "class"];
    headers.extend(DOC_ACTIONS.iter().map(|(_, n)| *n));
    let rows: Vec<Vec<String>> = docs
        .iter()
        .flat_map(|(t, mt)| {
            mt.properties
                .get("document_lifecycle")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .map(|l| {
                    let class = l.get("class").and_then(Value::as_str).unwrap_or("");
                    let methods: BTreeMap<&str, u64> = l
                        .get("methods")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|x| {
                            Some((
                                x.get("method")?.as_str()?,
                                x.get("start_line").and_then(Value::as_u64).unwrap_or(0),
                            ))
                        })
                        .collect();
                    let mut row = vec![m.table_ref(&path, t), code(class)];
                    row.extend(DOC_ACTIONS.iter().map(|(meth, _)| {
                        methods
                            .get(meth)
                            .map(|line| format!("✓ {line}"))
                            .unwrap_or_default()
                    }));
                    row
                })
                .collect::<Vec<_>>()
        })
        .collect();
    b.push_str(&table(&headers, &rows));
    b.push_str("\nThe number is the method's line in the class's source file.\n");
    (page(path, "Document lifecycles", 4, b), docs.len())
}

fn routine_pages(m: &Model, pages: &mut Vec<EstatePage>) -> usize {
    let index = "logic/index.md".to_string();
    let mut rows = Vec::new();
    for (i, (key, defs)) in m.routines.iter().enumerate() {
        let r = defs[0];
        let path = Model::routine_path(key);
        rows.push(vec![
            link(&index, &path, &r.name),
            cell(prop(r, "language").or(prop(r, "dialect")).unwrap_or("")),
            cell(prop(r, "origin").unwrap_or("file")),
            cell(prop(r, "fidelity").unwrap_or("")),
            cell(&strings(r, "reads").join(", ")),
            cell(
                &["writes", "inserts", "updates", "deletes"]
                    .iter()
                    .flat_map(|k| strings(r, k))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        ]);
        let mut b = String::new();
        if let Some(d) = m.text(r, prop(r, "description")) {
            b.push_str(&format!("{d}\n\n"));
        }
        b.push_str(&format!(
            "- defined in: {}{}\n- language: {} · fidelity: {} · statements recovered: {} · unrecovered: {}\n",
            code(prop(r, "source_path").unwrap_or("")),
            if prop(r, "status").is_some_and(|s| s != "VALID") {
                format!(" · status **{}**", prop(r, "status").unwrap_or(""))
            } else {
                String::new()
            },
            prop(r, "language").unwrap_or("?"),
            prop(r, "fidelity").unwrap_or("?"),
            r.properties.get("statements_recovered").and_then(Value::as_u64).unwrap_or(0),
            r.properties.get("statements_unrecovered").and_then(Value::as_u64).unwrap_or(0),
        ));
        if let Some(args) = m.text(r, Some(&strings(r, "arguments").join(", "))) {
            b.push_str(&format!("- arguments: {}\n", code(&args)));
        }
        if let Some(ret) = prop(r, "returns") {
            b.push_str(&format!("- returns: {}\n", code(ret)));
        }
        for other in &defs[1..] {
            b.push_str(&format!(
                "- also defined in {} ({})\n",
                code(prop(other, "source_path").unwrap_or("")),
                prop(other, "dialect")
                    .or(prop(other, "language"))
                    .unwrap_or("?")
            ));
        }
        b.push('\n');
        for (title, keys) in [
            ("Reads", vec!["reads"]),
            ("Writes", vec!["writes", "inserts", "updates", "deletes"]),
        ] {
            let ts: BTreeSet<String> = keys
                .iter()
                .flat_map(|k| strings(r, k))
                .map(|t| table_key(&t))
                .collect();
            if !ts.is_empty() {
                b.push_str(&format!(
                    "**{title}:** {}\n\n",
                    ts.iter()
                        .map(|t| m.table_ref(&path, t))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        if let Some(callers) = m.callers.get(&r.id) {
            b.push_str("## Called by\n\n");
            let rows: Vec<Vec<String>> = callers
                .iter()
                .map(|(k, n)| vec![k.clone(), code(n)])
                .collect();
            b.push_str(&table(&["kind", "name"], &rows));
            b.push('\n');
        }
        let stmts = m.descendants(r.id);
        if !stmts.is_empty() {
            b.push_str("## Statements\n\n");
            let rows: Vec<Vec<String>> = stmts
                .iter()
                .map(|s| {
                    let depth = s
                        .properties
                        .get("depth")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
                    let conditions = s
                        .properties
                        .get("conditions")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .filter_map(|c| c.as_str())
                                .collect::<Vec<_>>()
                                .join(" / ")
                        })
                        .unwrap_or_default();
                    vec![
                        s.properties
                            .get("line")
                            .and_then(Value::as_u64)
                            .map(|l| l.to_string())
                            .unwrap_or_default(),
                        format!(
                            "{}{}",
                            "· ".repeat(depth),
                            code(prop(s, "stmt").unwrap_or("?"))
                        ),
                        m.text(r, Some(&conditions))
                            .map(|c| code(&c))
                            .unwrap_or_default(),
                        cell(&strings(s, "reads").join(", ")),
                        cell(
                            &["writes", "inserts", "updates", "deletes"]
                                .iter()
                                .flat_map(|k| strings(s, k))
                                .collect::<BTreeSet<_>>()
                                .into_iter()
                                .collect::<Vec<_>>()
                                .join(", "),
                        ),
                    ]
                })
                .collect();
            b.push_str(&table(
                &["line", "statement", "condition", "reads", "writes"],
                &rows,
            ));
            b.push('\n');
        }
        pages.push(page(path, &format!("{}()", r.name), i + 1, b));
    }
    let mut b = format!(
        "{} routine(s); a name defined in the database and in files is shown once, the database's first.\n\n",
        m.routines.len()
    );
    b.push_str(&table(
        &[
            "routine", "language", "origin", "fidelity", "reads", "writes",
        ],
        &rows,
    ));
    pages.push(page(index, "Logic", 5, b));
    m.routines.len()
}

fn history_pages(m: &Model, pages: &mut Vec<EstatePage>) -> usize {
    let sets: Vec<&KirObject> = m
        .input
        .objects
        .iter()
        .filter(|o| custom(o) == Some("SchemaChangeSet"))
        .collect();
    let mut by_release: BTreeMap<String, Vec<&KirObject>> = BTreeMap::new();
    for s in &sets {
        by_release
            .entry(prop(s, "release").unwrap_or("(none)").to_string())
            .or_default()
            .push(s);
    }
    let index = "history/index.md".to_string();
    let mut rows = Vec::new();
    for (i, (rel, mut list)) in by_release.into_iter().enumerate() {
        list.sort_by(|a, b| {
            (prop(a, "applied_at"), prop(a, "engine"), a.id.to_string()).cmp(&(
                prop(b, "applied_at"),
                prop(b, "engine"),
                b.id.to_string(),
            ))
        });
        let path = format!("history/{}.md", slug(&rel));
        let changes: u64 = list
            .iter()
            .map(|s| {
                s.properties
                    .get("changes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
            })
            .sum();
        let first = list
            .first()
            .and_then(|s| prop(s, "applied_at"))
            .unwrap_or("");
        let last = list
            .last()
            .and_then(|s| prop(s, "applied_at"))
            .unwrap_or("");
        rows.push(vec![
            link(&index, &path, &rel),
            list.len().to_string(),
            changes.to_string(),
            cell(first),
            cell(last),
        ]);
        let set_rows: Vec<Vec<String>> = list
            .iter()
            .map(|s| {
                vec![
                    cell(prop(s, "applied_at").unwrap_or("")),
                    cell(prop(s, "engine").unwrap_or("")),
                    cell(&strings(s, "tickets").join(", ")),
                    s.properties
                        .get("changes")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
                        .to_string(),
                    code(prop(s, "source_path").unwrap_or("")),
                ]
            })
            .collect();
        let b = format!(
            "{} migration script(s), {} schema change(s), {first} – {last}.\n\n{}",
            list.len(),
            changes,
            table(
                &["applied", "engine", "tickets", "changes", "script"],
                &set_rows
            )
        );
        pages.push(page(path, &format!("Release {rel}"), i + 1, b));
    }
    let b = format!(
        "{} migration script(s) by release. Each table's page lists its own changes.\n\n{}",
        sets.len(),
        table(&["release", "scripts", "changes", "first", "last"], &rows)
    );
    pages.push(page(index, "Schema history", 6, b));
    sets.len()
}

fn findings_page(m: &Model) -> EstatePage {
    let path = "findings/index.md".to_string();
    let mut b = String::new();
    let mut by_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut rows = Vec::new();
    for (t, cs) in &m.conflicts {
        if !m.in_scope(t) && m.tables.contains_key(t) {
            continue;
        }
        for c in cs {
            let e = by_type
                .entry(prop(c, "conflict_type").unwrap_or("?").to_string())
                .or_default();
            e.0 += 1;
            if prop(c, "explanation").is_some() {
                e.1 += 1;
            }
            rows.push(vec![
                m.table_ref(&path, t),
                code(prop(c, "conflict_type").unwrap_or("")),
                code(prop(c, "attribute").unwrap_or("")),
                cell(prop(c, "explanation").unwrap_or("unexplained")),
            ]);
        }
    }
    rows.sort();
    b.push_str(
        "## Code ↔ database conflicts\n\nOpen conflicts about in-scope tables, by type:\n\n",
    );
    let type_rows: Vec<Vec<String>> = by_type
        .iter()
        .map(|(k, (n, e))| vec![code(k), n.to_string(), e.to_string()])
        .collect();
    b.push_str(&table(&["type", "open", "explained"], &type_rows));
    b.push('\n');
    b.push_str(&table(
        &["table", "type", "attribute", "explanation"],
        &rows,
    ));

    b.push_str("\n## Invalid database objects\n\n");
    let mut invalid: Vec<Vec<String>> = m
        .input
        .objects
        .iter()
        .filter(|o| prop(o, "origin") == Some("live"))
        .filter(|o| prop(o, "status").is_some_and(|s| s != "VALID"))
        .map(|o| {
            vec![
                o.kind.to_string(),
                code(&o.name),
                cell(prop(o, "status").unwrap_or("")),
            ]
        })
        .collect();
    invalid.sort();
    b.push_str(&table(&["kind", "name", "status"], &invalid));

    b.push_str("\n## Write behaviour classes\n\n");
    let mut classes: BTreeMap<(String, String), usize> = BTreeMap::new();
    for (t, w) in &m.write_behaviour {
        if m.tables.contains_key(t) && !m.in_scope(t) {
            continue;
        }
        *classes
            .entry((
                prop(w, "update_class").unwrap_or("unknown").to_string(),
                strings(w, "delete_classes").join(", "),
            ))
            .or_default() += 1;
    }
    let rows: Vec<Vec<String>> = classes
        .iter()
        .map(|((u, d), n)| vec![code(u), cell(d), n.to_string()])
        .collect();
    b.push_str(&table(&["updates", "deletes", "tables"], &rows));
    page(path, "Findings", 7, b)
}

fn home(m: &Model, counts: &[(&str, String)]) -> EstatePage {
    let p = m.input.provenance;
    let mut b = format!(
        "Documentation of **{}**, generated by EKOS from its knowledge ledger: nothing here was written by hand except the sections marked for notes, and nothing comes from a live connection at publishing time.\n\n",
        p.project
    );
    b.push_str(&manual(
        "home.purpose",
        "_What this system is for, in a sentence or two._",
    ));
    b.push_str("\n## At a glance\n\n");
    let rows: Vec<Vec<String>> = counts
        .iter()
        .map(|(k, v)| vec![k.to_string(), v.clone()])
        .collect();
    b.push_str(&table(&["", "count"], &rows));
    b.push_str("\n## Contents\n\n");
    for (path, title) in [
        ("data-model/index.md", "Data model"),
        ("glossary/index.md", "Glossary"),
        ("lifecycles/index.md", "Document lifecycles"),
        ("logic/index.md", "Logic"),
        ("history/index.md", "Schema history"),
        ("findings/index.md", "Findings"),
    ] {
        b.push_str(&format!("- {}\n", link("index.md", path, title)));
    }
    b.push_str("\n## Review\n\nThe source model's review (RFC 0213) as of this generation:\n\n");
    let rows: Vec<Vec<String>> = m
        .input
        .subjects
        .iter()
        .map(|s| {
            vec![
                s.subject.clone(),
                s.items.to_string(),
                s.blocking.to_string(),
                format!(
                    "{}{}",
                    s.state,
                    s.signed_by
                        .as_deref()
                        .map(|w| format!(" ({w})"))
                        .unwrap_or_default()
                ),
            ]
        })
        .collect();
    b.push_str(&table(&["subject", "items", "blocking", "sign-off"], &rows));
    b.push_str("\n## Sources and licences\n\nText (descriptions, business names, expressions) is shown only from sources with a known licence.\n\n");
    let dropped = m.dropped.borrow();
    let rows: Vec<Vec<String>> = m
        .input
        .licences
        .iter()
        .map(|(s, l)| {
            let n: usize = dropped
                .iter()
                .filter(|(k, _)| k.split('+').any(|x| x == s))
                .map(|(_, n)| n)
                .sum();
            vec![
                code(s),
                cell(l),
                if licence_allows_text(l) {
                    "used".into()
                } else {
                    format!("left out ({n} text(s))")
                },
            ]
        })
        .collect();
    b.push_str(&table(&["source", "licence", "text"], &rows));
    let unattributed = dropped.get("(no source)").copied().unwrap_or(0);
    if unattributed > 0 {
        b.push_str(&format!(
            "\n{unattributed} text(s) with no recorded source were left out.\n"
        ));
    }
    b.push_str(
        "\nNo data value appears in this documentation: column profiles contribute counts only.\n",
    );
    b.push_str("\n## Provenance\n\n");
    b.push_str(&format!(
        "- ledger committed: {}\n- source revision: {}\n- generator: EKOS {} (estate v{ESTATE_VERSION})\n",
        p.committed_at.as_deref().unwrap_or("unknown"),
        p.git_head.as_deref().unwrap_or("unknown"),
        env!("CARGO_PKG_VERSION"),
    ));
    page("index.md".into(), &p.project, 0, b)
}

/// Every page of the estate documentation, sorted by path.
pub fn build(input: &EstateInput<'_>) -> Vec<EstatePage> {
    let m = Model::new(input);
    let mut pages = Vec::new();
    let summary = area_pages(&m, &mut pages);
    pages.push(data_model_index(&m, &summary));
    let terms = glossary_pages(&m, &mut pages);
    let (lc, docs) = lifecycle_page(&m);
    pages.push(lc);
    let routines = routine_pages(&m, &mut pages);
    let scripts = history_pages(&m, &mut pages);
    pages.push(findings_page(&m));
    let in_scope = m.tables.keys().filter(|t| m.in_scope(t)).count();
    let open: usize = m.conflicts.values().map(Vec::len).sum();
    let counts = [
        ("tables", m.tables.len().to_string()),
        ("tables in scope", in_scope.to_string()),
        ("functional areas", summary.len().to_string()),
        ("foreign keys", m.fks.len().to_string()),
        ("application-model tables", m.model_tables.len().to_string()),
        ("document types with a lifecycle", docs.to_string()),
        ("routines", routines.to_string()),
        ("glossary terms", terms.to_string()),
        ("migration scripts", scripts.to_string()),
        ("open conflicts", open.to_string()),
    ];
    // Home last: it reports what the other pages dropped.
    pages.push(home(&m, &counts));
    pages.sort_by(|a, b| a.path.cmp(&b.path));
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(kind: ObjectKind, name: &str, props: Value) -> KirObject {
        let mut o = KirObject::new(name, kind);
        o.id = KirId(uuid_for(name, &format!("{:?}", o.kind)));
        for (k, v) in props.as_object().unwrap() {
            o.properties.insert(k.clone(), v.clone());
        }
        o
    }

    fn uuid_for(a: &str, b: &str) -> uuid::Uuid {
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, format!("{a}:{b}").as_bytes())
    }

    fn fixture() -> (Vec<KirObject>, Vec<KirRelationship>) {
        let order = obj(
            ObjectKind::Table,
            "C_ORDER",
            json!({
                "origin": "live", "database": "db", "owner": "ADEMPIERE", "dialect": "oracle",
                "description": "Sales order (database comment)",
                "primary_key": ["C_ORDER_ID"],
                "profile": {"row_count": 8},
                "check_constraints": [{"name": "C1", "expression": "IsActive in ('Y','N')"}],
                "columns": [
                    {"name": "C_ORDER_ID", "data_type": "NUMBER(10,0)", "not_null": true,
                     "profile": {"nulls": 0, "distinct": 8, "min": "100", "max": "SECRET-MAX-VALUE"}},
                    {"name": "C_BPARTNER_ID", "data_type": "NUMBER(10,0)",
                     "profile": {"nulls": 1, "distinct": 3, "min": "SECRET-MIN-VALUE", "max": "9"}}
                ]
            }),
        );
        let bp = obj(
            ObjectKind::Table,
            "C_BPARTNER",
            json!({"origin": "live", "database": "db", "dialect": "oracle", "columns": []}),
        );
        let tmp = obj(
            ObjectKind::Table,
            "T_REPORT",
            json!({"origin": "live", "database": "db", "dialect": "oracle", "columns": []}),
        );
        let pdf = obj(ObjectKind::Table, "FROM_A_PDF", json!({}));
        let model = obj(
            ObjectKind::Custom("ModelTable".into()),
            "C_Order",
            json!({
                "table_name": "C_Order", "source": "code", "sources": ["code"],
                "model_class": "org.compiere.model.X_C_Order", "description": "Sales order (model)",
                "columns": {"C_BPartner_ID": {"business_name": "Business Partner",
                    "description": "Identifies a business partner", "source": "I_C_Order.java:10"}},
                "document_lifecycle": [{"class": "MOrder", "methods": [
                    {"method": "prepareIt", "start_line": 5}, {"method": "completeIt", "start_line": 9}]}]
            }),
        );
        let wb = obj(
            ObjectKind::Custom("WriteBehaviour".into()),
            "C_ORDER",
            json!({"table": "C_ORDER", "origin": "live", "update_class": "po_only",
                   "delete_classes": ["deleted_by_users"], "change_log": "recorded", "citation_count": 2}),
        );
        let routine = obj(
            ObjectKind::Custom("Procedure".into()),
            "documentNo",
            json!({"origin": "live", "database": "db", "language": "plsql", "status": "VALID",
                   "reads": ["c_order"], "writes": [], "source_path": "oracle://db/x"}),
        );
        let doc = obj(
            ObjectKind::Custom("Document".into()),
            "docs/x.md",
            json!({"source": "docs", "description": "NOT LICENSED TEXT"}),
        );
        let fk = {
            let mut r = KirRelationship::new(RelationshipKind::ForeignKey, order.id, bp.id);
            r.id = KirId(uuid_for("fk", "1"));
            r.properties.insert(
                "fk_desc".into(),
                json!("C_ORDER.C_BPARTNER_ID → C_BPARTNER.C_BPARTNER_ID"),
            );
            r
        };
        (vec![order, bp, tmp, pdf, model, wb, routine, doc], vec![fk])
    }

    fn render(objects: &[KirObject], rels: &[KirRelationship]) -> Vec<EstatePage> {
        let mut scope = BTreeMap::new();
        scope.insert(
            "t_report".into(),
            ScopeEntry {
                in_scope: false,
                class: "transient".into(),
                decided: false,
            },
        );
        let licences: BTreeMap<String, String> = [
            ("code".to_string(), "GPL-2.0".to_string()),
            ("db".to_string(), "unknown".to_string()),
            ("docs".to_string(), "unknown".to_string()),
        ]
        .into();
        let prov = Provenance {
            project: "shop".into(),
            ..Default::default()
        };
        build(&EstateInput {
            objects,
            relationships: rels,
            scope: &scope,
            subjects: &[],
            licences: &licences,
            provenance: &prov,
            current_conflicts: None,
            max_er_tables: 30,
        })
    }

    fn body<'a>(pages: &'a [EstatePage], path: &str) -> &'a str {
        &pages
            .iter()
            .find(|p| p.path == path)
            .unwrap_or_else(|| panic!("no {path}"))
            .body
    }

    #[test]
    fn no_value_ever_reaches_a_page() {
        let (o, r) = fixture();
        let pages = render(&o, &r);
        for p in &pages {
            assert!(!p.body.contains("SECRET-"), "{} shows a value", p.path);
        }
        let order = body(&pages, "data-model/c/c_order.md");
        assert!(
            order
                .contains("| `C_BPARTNER_ID` | NUMBER(10,0) |  |  | Business Partner |  | 1 | 3 |"),
            "{order}"
        );
    }

    #[test]
    fn text_from_a_source_without_a_licence_is_left_out_and_counted() {
        let (o, r) = fixture();
        let pages = render(&o, &r);
        let order = body(&pages, "data-model/c/c_order.md");
        // The database comment is dropped (db: unknown); the model's description (GPL) is used.
        assert!(!order.contains("database comment"), "{order}");
        assert!(order.contains("Sales order (model)"), "{order}");
        // A check expression is database text too.
        assert!(!order.contains("IsActive in"), "{order}");
        let home = body(&pages, "index.md");
        assert!(home.contains("| `db` | unknown | left out ("), "{home}");
        assert!(home.contains("| `code` | GPL-2.0 | used |"), "{home}");
        assert!(pages.iter().all(|p| !p.body.contains("NOT LICENSED TEXT")));
    }

    #[test]
    fn only_in_scope_ddl_and_live_tables_get_pages() {
        let (o, r) = fixture();
        let pages = render(&o, &r);
        let paths: Vec<&str> = pages.iter().map(|p| p.path.as_str()).collect();
        assert!(paths.contains(&"data-model/c/c_order.md"));
        assert!(!paths.contains(&"data-model/t/t_report.md"), "out of scope");
        assert!(
            !paths.iter().any(|p| p.contains("from_a_pdf")),
            "a PDF table is not a table here"
        );
        let area = body(&pages, "data-model/t/index.md");
        assert!(area.contains("| `T_REPORT` | transient | out |"), "{area}");
    }

    #[test]
    fn keys_lifecycles_routines_and_diagrams() {
        let (o, r) = fixture();
        let pages = render(&o, &r);
        let order = body(&pages, "data-model/c/c_order.md");
        assert!(order.contains("C_BPARTNER ||--o{ C_ORDER"), "{order}");
        assert!(
            order.contains("[C_BPARTNER](../../data-model/c/c_bpartner.md)"),
            "{order}"
        );
        assert!(order.contains("updates: **po_only**"), "{order}");
        assert!(
            order.contains("[documentNo](../../logic/documentno.md)"),
            "{order}"
        );
        let lc = body(&pages, "lifecycles/index.md");
        assert!(lc.contains("| `MOrder` | ✓ 5 |  |  | ✓ 9 |"), "{lc}");
        let g = body(&pages, "glossary/b.md");
        assert!(
            g.contains("| **Business Partner** | Identifies a business partner |"),
            "{g}"
        );
        assert!(body(&pages, "logic/documentno.md").contains("**Reads:**"));
    }

    #[test]
    fn nested_statements_are_listed_in_source_order() {
        let (mut o, mut r) = fixture();
        let routine = o.iter().find(|x| x.name == "documentNo").unwrap().id;
        let block = obj(
            ObjectKind::Custom("ProcedureStatement".into()),
            "documentNo#0",
            json!({"stmt": "block", "index": 0, "depth": 0, "line": 3}),
        );
        let assign = obj(
            ObjectKind::Custom("ProcedureStatement".into()),
            "documentNo#1",
            json!({"stmt": "if", "index": 1, "depth": 1, "line": 4, "conditions": ["v > 0"], "reads": ["c_order"]}),
        );
        for (from, to, n) in [(routine, block.id, "a"), (block.id, assign.id, "b")] {
            let mut c = KirRelationship::new(RelationshipKind::Contains, from, to);
            c.id = KirId(uuid_for("contains", n));
            r.push(c);
        }
        o.push(assign);
        o.push(block);
        let pages = render(&o, &r);
        let p = body(&pages, "logic/documentno.md");
        let a = p.find("`block`").expect(p);
        let b = p.find("· `if`").expect(p);
        assert!(a < b, "{p}");
        // The routine's text is database text (db: unknown here): its condition is left out.
        assert!(!p.contains("v > 0"), "{p}");
    }

    #[test]
    fn the_er_diagram_is_capped_by_connections() {
        let (mut o, mut r) = fixture();
        for i in 0..5 {
            let t = obj(
                ObjectKind::Table,
                &format!("C_X{i}"),
                json!({"origin": "live", "database": "db"}),
            );
            let mut fk = KirRelationship::new(RelationshipKind::ForeignKey, t.id, o[0].id);
            fk.id = KirId(uuid_for("fkx", &i.to_string()));
            r.push(fk);
            o.push(t);
        }
        let m_input_pages = {
            let mut scope = BTreeMap::new();
            scope.insert(
                "t_report".to_string(),
                ScopeEntry {
                    in_scope: false,
                    class: "transient".into(),
                    decided: false,
                },
            );
            let lic = BTreeMap::new();
            let prov = Provenance::default();
            build(&EstateInput {
                objects: &o,
                relationships: &r,
                scope: &scope,
                subjects: &[],
                licences: &lic,
                provenance: &prov,
                current_conflicts: None,
                max_er_tables: 3,
            })
        };
        let area = body(&m_input_pages, "data-model/c/index.md");
        assert!(
            area.contains("the 3 most connected tables; 4 more"),
            "{area}"
        );
        assert!(area.contains("C_ORDER"), "the hub is drawn");
    }

    #[test]
    fn output_does_not_depend_on_input_order() {
        let (mut o, mut r) = fixture();
        let a = render(&o, &r);
        o.reverse();
        r.reverse();
        assert_eq!(a, render(&o, &r));
    }

    #[test]
    fn licence_files_are_recognised() {
        assert_eq!(
            detect_licence("GNU General Public License\nVersion 2, June 1991"),
            Some("GPL-2.0")
        );
        assert_eq!(
            detect_licence("Apache License\nVersion 2.0, January 2004"),
            Some("Apache-2.0")
        );
        assert_eq!(
            detect_licence("Permission is hereby granted, free of charge, to any person"),
            Some("MIT")
        );
        assert_eq!(detect_licence("All rights reserved."), None);
        let gpl2 = format!(
            "# GNU General Public License\nVersion 2, June 1991\n{}use the GNU Library General Public License instead",
            "x".repeat(700)
        );
        assert_eq!(detect_licence(&gpl2), Some("GPL-2.0"));
        assert!(!licence_allows_text("unknown") && !licence_allows_text("none"));
    }
}
