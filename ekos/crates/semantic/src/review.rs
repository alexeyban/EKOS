//! RFC 0213 — review subjects, item decisions and digest-bound sign-off.
//!
//! A **subject** (`scope`, `pii`, `conflicts`, `identity`, `write-behaviour`, `glossary`) is a
//! deterministic list of [`Item`]s computed from the ledger. Each item carries the facts a reviewer
//! is shown, split in two: `facts` are signed (they enter the digest), `info` is shown only —
//! measurements such as row counts change with every profile and would make a sign-off stale for
//! no reason.
//!
//! A **sign-off** records the digest of one subject's items. It is never invalidated by a write:
//! [`signoff_state`] recomputes the digest and compares, so a compile that changed a signed fact
//! makes the sign-off stale without anyone having to remember to say so.
//!
//! Pure: no ledger handle, no clock (callers pass `at`). **Human-only:** [`decision_object`] and
//! [`signoff_object`] are reached from `commands/review.rs` only; `commands/mcp.rs` must never name
//! this module's write functions (`review_guard::HUMAN_ONLY`).

use ekos_common::ContentHash;
use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use uuid::Uuid;

/// Folded into every digest; bump when an item's facts change shape.
pub const REVIEW_VERSION: u32 = 1;
/// One item decision (scope, PII, write behaviour), keyed by (subject, item).
pub const DECISION_KIND: &str = "ReviewDecision";
/// One subject's sign-off, keyed by subject.
pub const SIGNOFF_KIND: &str = "SignOff";

/// The review subjects, in the order the console shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subject {
    Scope,
    Pii,
    Conflicts,
    Identity,
    WriteBehaviour,
    Glossary,
}

impl Subject {
    pub const ALL: [Subject; 6] = [
        Subject::Scope,
        Subject::Pii,
        Subject::Conflicts,
        Subject::Identity,
        Subject::WriteBehaviour,
        Subject::Glossary,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Subject::Scope => "scope",
            Subject::Pii => "pii",
            Subject::Conflicts => "conflicts",
            Subject::Identity => "identity",
            Subject::WriteBehaviour => "write-behaviour",
            Subject::Glossary => "glossary",
        }
    }

    pub fn parse(s: &str) -> Option<Subject> {
        Subject::ALL.into_iter().find(|x| x.as_str() == s)
    }

    /// Whether items of this subject are decided with `ekos review` (the others keep their own
    /// commands: `conflicts resolve`, identity review, `semantics confirm`).
    pub fn decided_here(self) -> bool {
        matches!(
            self,
            Subject::Scope | Subject::Pii | Subject::WriteBehaviour
        )
    }
}

/// `[review.scope]`: ordered name rules, first match wins.
#[derive(Debug, Clone, PartialEq)]
pub struct ScopeRules {
    /// `(pattern, class)`; `*` matches any run of characters, case-insensitive.
    pub classes: Vec<(String, String)>,
    /// Classes proposed out of scope unless decided.
    pub default_out: Vec<String>,
}

impl Default for ScopeRules {
    fn default() -> Self {
        let classes = [
            ("T_*", "transient"),
            ("I_*", "import"),
            ("*_Trl", "translation"),
            ("*Log*", "log"),
            ("AD_*", "config"),
        ];
        Self {
            classes: classes
                .iter()
                .map(|(p, c)| (p.to_string(), c.to_string()))
                .collect(),
            default_out: ["transient", "import", "log"].map(String::from).to_vec(),
        }
    }
}

impl ScopeRules {
    /// `[review.scope]`: its rules first, then the built-in ones unless `replace-defaults`.
    pub fn from_config(c: &ekos_compiler_core::ReviewScopeConfig) -> Self {
        let defaults = Self::default();
        let mut classes: Vec<(String, String)> = c
            .classes
            .iter()
            .map(|r| (r.pattern.clone(), r.class.clone()))
            .collect();
        if !c.replace_defaults {
            classes.extend(defaults.classes);
        }
        Self {
            classes,
            default_out: c.default_out.clone().unwrap_or(defaults.default_out),
        }
    }

    /// The class of `table` and the rule that gave it; `business` when no rule matches.
    pub fn classify(&self, table: &str) -> (String, Option<String>) {
        self.classes
            .iter()
            .find(|(p, _)| glob(p, table))
            .map(|(p, c)| (c.clone(), Some(p.clone())))
            .unwrap_or_else(|| ("business".into(), None))
    }
}

/// `*`-only glob, case-insensitive.
fn glob(pattern: &str, text: &str) -> bool {
    let p = pattern.to_ascii_lowercase();
    let t = text.to_ascii_lowercase();
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == t;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !t.starts_with(first) || t.len() < first.len() + last.len() || !t.ends_with(last) {
        return false;
    }
    let mut rest = &t[first.len()..t.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

/// One reviewable item.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Item {
    pub id: String,
    pub label: String,
    /// Signed: these enter the digest.
    pub facts: BTreeMap<String, Value>,
    /// Shown only.
    pub info: BTreeMap<String, Value>,
    /// What the review proposes without a decision (scope: `in`/`out`).
    pub proposed: Option<String>,
    /// The current decision, when one exists and its facts still hold.
    pub decision: Option<Value>,
    /// A decision exists but was made on facts that have since changed.
    pub outdated: bool,
    /// Sign-off is refused while this item is open.
    pub blocking: bool,
}

impl Item {
    fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            facts: BTreeMap::new(),
            info: BTreeMap::new(),
            proposed: None,
            decision: None,
            outdated: false,
            blocking: false,
        }
    }

    fn fact(mut self, k: &str, v: Value) -> Self {
        self.facts.insert(k.into(), v);
        self
    }

    fn info(mut self, k: &str, v: Value) -> Self {
        if !v.is_null() {
            self.info.insert(k.into(), v);
        }
        self
    }
}

/// Everything a subject is computed from.
pub struct Inputs<'a> {
    /// The latest version of every object.
    pub objects: &'a [KirObject],
    pub relationships: &'a [KirRelationship],
    /// The ids of the current conflicts (`.ekos/conflicts/current.json`); `None` = all.
    pub conflicts_current: Option<&'a BTreeSet<String>>,
    /// The ids of the current semantics items (`.ekos/semantics/current.json`); `None` = all.
    pub semantics_current: Option<&'a BTreeSet<String>>,
    pub rules: &'a ScopeRules,
}

fn custom_kind(o: &KirObject) -> Option<&str> {
    match &o.kind {
        ObjectKind::Custom(k) => Some(k.as_str()),
        _ => None,
    }
}

fn str_prop<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Canonical JSON: object keys sorted at every level, no whitespace. Independent of whether
/// `serde_json` was built with `preserve_order`.
pub fn canonical(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Value::String((*k).clone()), canonical(&m[*k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(a) => format!(
            "[{}]",
            a.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => other.to_string(),
    }
}

/// The digest of one item's facts alone — what a decision was made on.
pub fn facts_digest(item: &Item) -> String {
    let facts: Map<String, Value> = item.facts.clone().into_iter().collect();
    ContentHash::of_str(&canonical(&json!({ "id": item.id, "facts": facts }))).0
}

/// The digest of one item as signed: its facts plus its decision (who and when excluded).
pub fn item_digest(item: &Item) -> String {
    let facts: Map<String, Value> = item.facts.clone().into_iter().collect();
    ContentHash::of_str(&canonical(&json!({
        "id": item.id,
        "facts": facts,
        "proposed": item.proposed,
        "decision": item.decision,
        "blocking": item.blocking,
    })))
    .0
}

/// The digest of a whole subject: the subject, [`REVIEW_VERSION`] and every `(id, item digest)`,
/// sorted by id — input order never matters.
pub fn subject_digest(subject: Subject, items: &[Item]) -> String {
    let mut pairs: Vec<(String, String)> = items
        .iter()
        .map(|i| (i.id.clone(), item_digest(i)))
        .collect();
    pairs.sort();
    let mut text = format!("{}|{}", subject.as_str(), REVIEW_VERSION);
    for (id, d) in pairs {
        text.push('|');
        text.push_str(&id);
        text.push('=');
        text.push_str(&d);
    }
    ContentHash::of_str(&text).0
}

/// The latest decision per `(subject, item)`.
pub fn decisions(objects: &[KirObject]) -> HashMap<(String, String), &KirObject> {
    let mut out: HashMap<(String, String), &KirObject> = HashMap::new();
    for o in objects
        .iter()
        .filter(|o| custom_kind(o) == Some(DECISION_KIND))
    {
        let key = (
            str_prop(o, "subject").to_string(),
            str_prop(o, "item").to_string(),
        );
        match out.get(&key) {
            Some(prev) if str_prop(prev, "reviewed_at") > str_prop(o, "reviewed_at") => {}
            _ => {
                out.insert(key, o);
            }
        }
    }
    out
}

/// The latest sign-off per subject.
pub fn signoffs(objects: &[KirObject]) -> HashMap<String, &KirObject> {
    let mut out: HashMap<String, &KirObject> = HashMap::new();
    for o in objects
        .iter()
        .filter(|o| custom_kind(o) == Some(SIGNOFF_KIND))
    {
        let key = str_prop(o, "subject").to_string();
        match out.get(&key) {
            Some(prev) if str_prop(prev, "signed_at") > str_prop(o, "signed_at") => {}
            _ => {
                out.insert(key, o);
            }
        }
    }
    out
}

/// Attach the recorded decision to `item` when it was made on the item's current facts.
fn apply_decision(item: &mut Item, subject: Subject, d: &HashMap<(String, String), &KirObject>) {
    let Some(o) = d.get(&(subject.as_str().to_string(), item.id.clone())) else {
        return;
    };
    if str_prop(o, "facts_digest") != facts_digest(item) {
        item.outdated = true;
        return;
    }
    item.decision = o.properties.get("decision").cloned();
}

/// One table per name: live objects first, else the DDL `Table`s (those with a `dialect` — a
/// `Table` from a PDF has none). Keyed by the lower-case name.
fn tables(objects: &[KirObject]) -> BTreeMap<String, &KirObject> {
    let is_table = |o: &&KirObject| matches!(o.kind, ObjectKind::Table);
    let live: Vec<&KirObject> = objects
        .iter()
        .filter(is_table)
        .filter(|o| str_prop(o, "origin") == "live")
        .collect();
    let pool: Vec<&KirObject> = if live.is_empty() {
        objects
            .iter()
            .filter(is_table)
            .filter(|o| o.properties.contains_key("dialect"))
            .collect()
    } else {
        live
    };
    let mut out = BTreeMap::new();
    for t in pool {
        out.entry(t.name.to_ascii_lowercase()).or_insert(t);
    }
    out
}

fn columns(t: &KirObject) -> Vec<&Value> {
    match t.properties.get("columns") {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(m)) => m.values().collect(),
        _ => Vec::new(),
    }
}

fn column_name(c: &Value) -> &str {
    c.get("name").and_then(Value::as_str).unwrap_or("")
}

fn scope_items(inputs: &Inputs<'_>, d: &HashMap<(String, String), &KirObject>) -> Vec<Item> {
    let models: BTreeSet<String> = inputs
        .objects
        .iter()
        .filter(|o| custom_kind(o) == Some("ModelTable"))
        .map(|o| str_prop(o, "table_name").to_ascii_lowercase())
        .collect();
    tables(inputs.objects)
        .into_iter()
        .map(|(key, t)| {
            let (class, rule) = inputs.rules.classify(&t.name);
            let proposed = if inputs.rules.default_out.contains(&class) {
                "out"
            } else {
                "in"
            };
            let rows = t
                .properties
                .get("profile")
                .and_then(|p| p.get("row_count"))
                .or_else(|| t.properties.get("num_rows"))
                .cloned()
                .unwrap_or(Value::Null);
            let mut item = Item::new(format!("table:{key}"), t.name.clone())
                .fact("table", json!(key))
                .fact("class", json!(class))
                .fact("rule", json!(rule))
                .fact("model_table", json!(models.contains(&key)))
                .info("columns", json!(columns(t).len()))
                .info("rows", rows);
            item.proposed = Some(proposed.into());
            apply_decision(&mut item, Subject::Scope, d);
            item
        })
        .collect()
}

/// The tables in scope: a current `in`/`out` decision wins, else the proposal.
pub fn in_scope(scope: &[Item]) -> BTreeSet<String> {
    scope
        .iter()
        .filter(|i| {
            let decided = i
                .decision
                .as_ref()
                .and_then(|d| d.get("scope"))
                .and_then(Value::as_str);
            decided.or(i.proposed.as_deref()) == Some("in")
        })
        .filter_map(|i| {
            i.facts
                .get("table")
                .and_then(Value::as_str)
                .map(String::from)
        })
        .collect()
}

fn pii_items(
    inputs: &Inputs<'_>,
    scope: &BTreeSet<String>,
    d: &HashMap<(String, String), &KirObject>,
) -> Vec<Item> {
    let mut out = Vec::new();
    for (key, t) in tables(inputs.objects) {
        if !scope.contains(&key) {
            continue;
        }
        for c in columns(t) {
            let name = column_name(c);
            let profile = c.get("profile");
            let (class, basis) = match profile.and_then(|p| p.get("pii")) {
                Some(p) => (p.get("class").cloned(), p.get("method").cloned()),
                None => match ekos_common::pii::classify_name(name) {
                    Some(cl) => (
                        serde_json::to_value(cl.class).ok(),
                        serde_json::to_value(cl.method).ok(),
                    ),
                    None => continue,
                },
            };
            let column = name.to_ascii_lowercase();
            let mut item = Item::new(
                format!("column:{key}.{column}"),
                format!("{}.{name}", t.name),
            )
            .fact("table", json!(key))
            .fact("column", json!(column))
            .fact("class", class.unwrap_or(Value::Null))
            .fact("basis", basis.unwrap_or(Value::Null))
            .info(
                "nulls",
                profile
                    .and_then(|p| p.get("nulls"))
                    .cloned()
                    .unwrap_or(Value::Null),
            )
            .info(
                "distinct",
                profile
                    .and_then(|p| p.get("distinct"))
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            apply_decision(&mut item, Subject::Pii, d);
            out.push(item);
        }
    }
    out
}

fn conflict_items(inputs: &Inputs<'_>, scope: &BTreeSet<String>) -> Vec<Item> {
    let known: BTreeSet<String> = tables(inputs.objects).into_keys().collect();
    inputs
        .objects
        .iter()
        .filter(|o| custom_kind(o) == Some("ConflictingEvidence"))
        .filter(|o| {
            inputs
                .conflicts_current
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        // A conflict about a table that is out of scope is not this review's business; one about
        // anything else (a routine, a column of an unknown table) stays.
        .filter(|o| {
            let subject = str_prop(o, "subject_name").to_ascii_lowercase();
            let table = subject.split('.').next().unwrap_or("").to_string();
            scope.contains(&table) || !known.contains(&table)
        })
        .map(|o| {
            let status = str_prop(o, "status");
            let explained = o
                .properties
                .get("explanation")
                .is_some_and(|e| !e.is_null());
            let mut item = Item::new(o.id.to_string(), o.name.clone())
                .fact("type", json!(str_prop(o, "conflict_type")))
                .fact("subject", json!(str_prop(o, "subject_name")))
                .fact("attribute", json!(str_prop(o, "attribute")))
                .fact("signature", json!(str_prop(o, "signature")))
                .fact("status", json!(status))
                .fact(
                    "resolution",
                    o.properties
                        .get("resolution")
                        .cloned()
                        .unwrap_or(Value::Null),
                )
                .fact("explained", json!(explained));
            item.blocking = status == "open" && !explained;
            item
        })
        .collect()
}

fn identity_items(inputs: &Inputs<'_>) -> Vec<Item> {
    let names: HashMap<&KirId, &str> = inputs
        .objects
        .iter()
        .map(|o| (&o.id, o.name.as_str()))
        .collect();
    inputs
        .relationships
        .iter()
        .filter(|r| matches!(&r.kind, RelationshipKind::Custom(k) if k == "SameAs"))
        .map(|r| {
            let status = r
                .properties
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unconfirmed");
            let from = names.get(&r.from).copied().unwrap_or("?");
            let to = names.get(&r.to).copied().unwrap_or("?");
            let mut item = Item::new(r.id.to_string(), format!("{from} = {to}"))
                .fact("from", json!(r.from.to_string()))
                .fact("to", json!(r.to.to_string()))
                .fact("status", json!(status))
                .info(
                    "score",
                    r.properties.get("score").cloned().unwrap_or(Value::Null),
                );
            item.blocking = status == "unconfirmed";
            item
        })
        .collect()
}

fn write_behaviour_items(
    inputs: &Inputs<'_>,
    scope: &BTreeSet<String>,
    d: &HashMap<(String, String), &KirObject>,
) -> Vec<Item> {
    let mut by_table: BTreeMap<String, &KirObject> = BTreeMap::new();
    for o in inputs
        .objects
        .iter()
        .filter(|o| custom_kind(o) == Some(crate::write_behaviour::KIND))
    {
        let key = str_prop(o, "table").to_ascii_lowercase();
        if scope.contains(&key) {
            // Live first, as for the tables themselves.
            let live = str_prop(o, "origin") == "live";
            match by_table.get(&key) {
                Some(prev) if str_prop(prev, "origin") == "live" || !live => {}
                _ => {
                    by_table.insert(key, o);
                }
            }
        }
    }
    by_table
        .into_iter()
        .map(|(key, o)| {
            let get = |k: &str| o.properties.get(k).cloned().unwrap_or(Value::Null);
            let mut item = Item::new(format!("table:{key}"), o.name.clone())
                .fact("table", json!(key))
                .fact("update_class", get("update_class"))
                .fact("delete_classes", get("delete_classes"))
                .fact("change_log", get("change_log"))
                .info("citations", get("citation_count"));
            apply_decision(&mut item, Subject::WriteBehaviour, d);
            item
        })
        .collect()
}

fn glossary_items(inputs: &Inputs<'_>) -> Vec<Item> {
    inputs
        .objects
        .iter()
        .filter(|o| matches!(o.kind, ObjectKind::BusinessConcept))
        .filter(|o| {
            inputs
                .semantics_current
                .is_none_or(|c| c.contains(&o.id.to_string()))
        })
        .map(|o| {
            Item::new(o.id.to_string(), o.name.clone())
                .fact("term", json!(o.name))
                .fact(
                    "status",
                    o.properties.get("status").cloned().unwrap_or(Value::Null),
                )
                .fact(
                    "definition",
                    o.properties
                        .get("definition")
                        .cloned()
                        .unwrap_or(Value::Null),
                )
        })
        .collect()
}

/// The items of `subject`, sorted by id.
pub fn items(subject: Subject, inputs: &Inputs<'_>) -> Vec<Item> {
    let d = decisions(inputs.objects);
    let scope = || in_scope(&scope_items(inputs, &d));
    let mut out = match subject {
        Subject::Scope => scope_items(inputs, &d),
        Subject::Pii => pii_items(inputs, &scope(), &d),
        Subject::Conflicts => conflict_items(inputs, &scope()),
        Subject::Identity => identity_items(inputs),
        Subject::WriteBehaviour => write_behaviour_items(inputs, &scope(), &d),
        Subject::Glossary => glossary_items(inputs),
    };
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Check a decision's value for `subject`; returns the canonical decision object.
pub fn check_decision(subject: Subject, decision: &Value) -> Result<Value, String> {
    let get = |k: &str| decision.get(k).and_then(Value::as_str);
    match subject {
        Subject::Scope => match get("scope") {
            Some("in" | "out") => Ok(json!({ "scope": get("scope") })),
            _ => Err("a scope decision is `in` or `out`".into()),
        },
        Subject::Pii => {
            let verdict = get("pii");
            let mask = get("mask");
            match (verdict, mask) {
                (Some("confirm"), Some("hash" | "redact" | "none")) => {
                    Ok(json!({ "pii": "confirm", "mask": mask }))
                }
                (Some("confirm"), None) => Ok(json!({ "pii": "confirm", "mask": "redact" })),
                (Some("reject"), None) => Ok(json!({ "pii": "reject" })),
                (Some("reject"), Some(_)) => Err("a rejected PII candidate has no mask".into()),
                _ => Err("a PII decision is `confirm` (with a mask) or `reject`".into()),
            }
        }
        Subject::WriteBehaviour => match get("behaviour") {
            Some("confirm") => Ok(json!({ "behaviour": "confirm" })),
            Some("override") => {
                let fields = decision
                    .get("fields")
                    .and_then(Value::as_object)
                    .filter(|f| !f.is_empty())
                    .ok_or("an override names the fields it changes")?;
                for k in fields.keys() {
                    if !["update_class", "delete_classes", "change_log"].contains(&k.as_str()) {
                        return Err(format!(
                            "`{k}` is not a write-behaviour field (update_class, delete_classes, change_log)"
                        ));
                    }
                }
                Ok(json!({ "behaviour": "override", "fields": fields }))
            }
            _ => Err("a write-behaviour decision is `confirm` or `override`".into()),
        },
        other => Err(format!(
            "{} items are decided with their own command",
            other.as_str()
        )),
    }
}

/// A new version of the decision object for `(subject, item)`. The caller checks the decision
/// with [`check_decision`] first.
pub fn decision_object(
    subject: Subject,
    item: &Item,
    decision: Value,
    reason: Option<&str>,
    by: &str,
    at: &str,
) -> Result<KirObject, String> {
    if by.trim().is_empty() {
        return Err("a decision needs a reviewer".into());
    }
    if subject == Subject::WriteBehaviour
        && decision.get("behaviour").and_then(Value::as_str) == Some("override")
        && reason.is_none_or(|r| r.trim().is_empty())
    {
        return Err("say why: an override needs --reason".into());
    }
    let mut o = KirObject::new(
        format!("{}:{}", subject.as_str(), item.label),
        ObjectKind::Custom(DECISION_KIND.into()),
    );
    o.id = KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("review-decision:{}:{}", subject.as_str(), item.id).as_bytes(),
    ));
    for (k, v) in [
        ("subject", json!(subject.as_str())),
        ("item", json!(item.id)),
        ("decision", decision),
        ("reason", json!(reason)),
        ("facts_digest", json!(facts_digest(item))),
        ("reviewed_by", json!(by)),
        ("reviewed_at", json!(at)),
    ] {
        o.properties.insert(k.into(), v);
    }
    Ok(o)
}

/// Why a sign-off cannot be recorded now, if it cannot.
pub fn approve_check(
    subject: Subject,
    items: &[Item],
    shown_digest: &str,
) -> Result<String, String> {
    let current = subject_digest(subject, items);
    if shown_digest != current {
        return Err(format!(
            "the {} review changed since it was shown (digest {} now {}): show it again and re-check",
            subject.as_str(),
            short(shown_digest),
            short(&current)
        ));
    }
    let open: Vec<&str> = items
        .iter()
        .filter(|i| i.blocking)
        .map(|i| i.label.as_str())
        .collect();
    if !open.is_empty() {
        return Err(format!(
            "{} {} item(s) still open, e.g. {}",
            open.len(),
            subject.as_str(),
            open.iter().take(3).copied().collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(current)
}

fn short(d: &str) -> &str {
    &d[..d.len().min(12)]
}

/// A new version of `subject`'s sign-off: approved on `digest`, or revoked when `revoked` gives
/// the reason.
pub fn signoff_object(
    subject: Subject,
    digest: &str,
    items: &[Item],
    by: &str,
    at: &str,
    note: Option<&str>,
    revoked: Option<&str>,
) -> Result<KirObject, String> {
    if by.trim().is_empty() {
        return Err("a sign-off needs a person".into());
    }
    let per_item: Map<String, Value> = items
        .iter()
        .map(|i| (i.id.clone(), json!(item_digest(i))))
        .collect();
    let mut o = KirObject::new(
        format!("sign-off:{}", subject.as_str()),
        ObjectKind::Custom(SIGNOFF_KIND.into()),
    );
    o.id = KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("sign-off:{}", subject.as_str()).as_bytes(),
    ));
    for (k, v) in [
        ("subject", json!(subject.as_str())),
        (
            "state",
            json!(if revoked.is_some() {
                "revoked"
            } else {
                "approved"
            }),
        ),
        ("digest", json!(digest)),
        ("item_count", json!(items.len())),
        ("item_digests", Value::Object(per_item)),
        ("review_version", json!(REVIEW_VERSION)),
        ("signed_by", json!(by)),
        ("signed_at", json!(at)),
        ("note", json!(note)),
        ("revoked_reason", json!(revoked)),
    ] {
        o.properties.insert(k.into(), v);
    }
    Ok(o)
}

/// Where a subject's sign-off stands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SignState {
    /// Never signed.
    None,
    Valid {
        signed_by: String,
        signed_at: String,
    },
    /// Signed, but the facts changed since.
    Stale {
        signed_by: String,
        signed_at: String,
        added: Vec<String>,
        removed: Vec<String>,
        changed: Vec<String>,
    },
    Revoked {
        signed_by: String,
        signed_at: String,
        reason: String,
    },
}

impl SignState {
    pub fn name(&self) -> &'static str {
        match self {
            SignState::None => "none",
            SignState::Valid { .. } => "valid",
            SignState::Stale { .. } => "stale",
            SignState::Revoked { .. } => "revoked",
        }
    }
}

/// Recompute `subject`'s digest and compare it with the latest sign-off.
pub fn signoff_state(subject: Subject, signoff: Option<&KirObject>, items: &[Item]) -> SignState {
    let Some(s) = signoff else {
        return SignState::None;
    };
    let signed_by = str_prop(s, "signed_by").to_string();
    let signed_at = str_prop(s, "signed_at").to_string();
    if str_prop(s, "state") == "revoked" {
        return SignState::Revoked {
            signed_by,
            signed_at,
            reason: str_prop(s, "revoked_reason").to_string(),
        };
    }
    let current = subject_digest(subject, items);
    let signed_version = s.properties.get("review_version").and_then(Value::as_u64);
    if str_prop(s, "digest") == current && signed_version == Some(u64::from(REVIEW_VERSION)) {
        return SignState::Valid {
            signed_by,
            signed_at,
        };
    }
    let before: BTreeMap<String, String> = s
        .properties
        .get("item_digests")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();
    let now: BTreeMap<String, String> = items
        .iter()
        .map(|i| (i.id.clone(), item_digest(i)))
        .collect();
    let label: HashMap<&str, &str> = items
        .iter()
        .map(|i| (i.id.as_str(), i.label.as_str()))
        .collect();
    let name = |id: &String| label.get(id.as_str()).map_or(id.clone(), |l| l.to_string());
    SignState::Stale {
        signed_by,
        signed_at,
        added: now
            .keys()
            .filter(|k| !before.contains_key(*k))
            .map(name)
            .collect(),
        removed: before
            .keys()
            .filter(|k| !now.contains_key(*k))
            .cloned()
            .collect(),
        changed: now
            .iter()
            .filter(|(k, v)| before.get(*k).is_some_and(|b| b != *v))
            .map(|(k, _)| name(k))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_table(name: &str, cols: &[(&str, Option<Value>)], rows: u64) -> KirObject {
        let columns: Vec<Value> = cols
            .iter()
            .map(|(c, pii)| {
                let mut profile = json!({ "nulls": 0, "distinct": 3 });
                if let Some(p) = pii {
                    profile["pii"] = p.clone();
                }
                json!({ "name": c, "profile": profile })
            })
            .collect();
        KirObject::new(name, ObjectKind::Table)
            .with_property("origin", json!("live"))
            .with_property("columns", json!(columns))
            .with_property("profile", json!({ "row_count": rows }))
    }

    fn behaviour(table: &str, update: &str) -> KirObject {
        KirObject::new(
            table,
            ObjectKind::Custom(crate::write_behaviour::KIND.into()),
        )
        .with_property("table", json!(table))
        .with_property("origin", json!("live"))
        .with_property("update_class", json!(update))
        .with_property("delete_classes", json!(["users"]))
        .with_property("change_log", json!(false))
    }

    fn conflict(subject: &str, status: &str) -> KirObject {
        KirObject::new(subject, ObjectKind::Custom("ConflictingEvidence".into()))
            .with_property("subject_name", json!(subject))
            .with_property("conflict_type", json!("model_column_missing"))
            .with_property("status", json!(status))
            .with_property("signature", json!("sig"))
    }

    fn estate() -> Vec<KirObject> {
        vec![
            live_table(
                "C_BPARTNER",
                &[
                    ("NAME", None),
                    (
                        "EMAIL",
                        Some(json!({ "class": "email", "method": "column_name" })),
                    ),
                ],
                120,
            ),
            live_table("T_REPORT", &[("EMAIL", None)], 5),
            live_table("C_ORDER_TRL", &[], 1),
            behaviour("C_BPARTNER", "stamps_updated"),
            behaviour("T_REPORT", "plain"),
            conflict("c_bpartner.email", "open"),
            conflict("t_report.x", "open"),
        ]
    }

    fn run(objects: &[KirObject], subject: Subject) -> Vec<Item> {
        let rules = ScopeRules::default();
        items(
            subject,
            &Inputs {
                objects,
                relationships: &[],
                conflicts_current: None,
                semantics_current: None,
                rules: &rules,
            },
        )
    }

    #[test]
    fn globs_match_prefixes_suffixes_and_middles_case_insensitively() {
        assert!(glob("T_*", "t_report"));
        assert!(glob("*_Trl", "C_ORDER_TRL"));
        assert!(glob("*Log*", "AD_ChangeLog"));
        assert!(glob("*Log*", "AD_ChangeLog_X"));
        assert!(!glob("T_*", "AT_X"));
        assert!(!glob("a*b*c", "acb"));
        assert!(glob("exact", "EXACT"));
    }

    #[test]
    fn scope_classes_by_first_matching_rule_and_proposes_default_out() {
        let scope = run(&estate(), Subject::Scope);
        let by: HashMap<_, _> = scope.iter().map(|i| (i.label.as_str(), i)).collect();
        assert_eq!(by["T_REPORT"].facts["class"], "transient");
        assert_eq!(by["T_REPORT"].proposed.as_deref(), Some("out"));
        assert_eq!(by["C_ORDER_TRL"].facts["class"], "translation");
        assert_eq!(by["C_ORDER_TRL"].proposed.as_deref(), Some("in"));
        assert_eq!(by["C_BPARTNER"].facts["class"], "business");
        assert_eq!(by["C_BPARTNER"].info["rows"], 120);
    }

    #[test]
    fn row_counts_are_shown_but_not_signed() {
        let mut a = estate();
        let before = subject_digest(Subject::Scope, &run(&a, Subject::Scope));
        a[0] = live_table("C_BPARTNER", &[("NAME", None)], 99_999);
        assert_eq!(
            before,
            subject_digest(Subject::Scope, &run(&a, Subject::Scope))
        );
    }

    #[test]
    fn digest_ignores_input_order() {
        let a = estate();
        let mut b = a.clone();
        b.reverse();
        for s in Subject::ALL {
            assert_eq!(
                subject_digest(s, &run(&a, s)),
                subject_digest(s, &run(&b, s)),
                "{}",
                s.as_str()
            );
        }
    }

    #[test]
    fn pii_items_carry_no_values_and_follow_scope() {
        let pii = run(&estate(), Subject::Pii);
        // T_REPORT.EMAIL is out of scope (transient); C_BPARTNER.EMAIL from the profile.
        assert_eq!(pii.len(), 1);
        assert_eq!(pii[0].label, "C_BPARTNER.EMAIL");
        assert_eq!(pii[0].facts["class"], "email");
        let text = serde_json::to_string(&pii[0]).unwrap();
        assert!(!text.contains("sample"), "{text}");
    }

    #[test]
    fn pii_falls_back_to_column_names_without_a_profile() {
        let t = KirObject::new("customers", ObjectKind::Table)
            .with_property("dialect", json!("postgres"))
            .with_property("columns", json!([{ "name": "email" }, { "name": "id" }]));
        let pii = run(&[t], Subject::Pii);
        assert_eq!(pii.len(), 1);
        assert_eq!(pii[0].facts["basis"], "column_name");
    }

    #[test]
    fn scoping_a_table_out_drops_its_items_from_other_subjects() {
        let mut objects = estate();
        let wb = run(&objects, Subject::WriteBehaviour);
        assert_eq!(wb.len(), 1, "T_REPORT is proposed out");
        let scope = run(&objects, Subject::Scope);
        let bp = scope.iter().find(|i| i.label == "C_BPARTNER").unwrap();
        objects.push(
            decision_object(
                Subject::Scope,
                bp,
                json!({ "scope": "out" }),
                None,
                "ann",
                "t1",
            )
            .unwrap(),
        );
        assert!(run(&objects, Subject::WriteBehaviour).is_empty());
        assert!(run(&objects, Subject::Pii).is_empty());
        assert!(run(&objects, Subject::Conflicts).is_empty());
    }

    #[test]
    fn open_unexplained_conflicts_block_explained_ones_do_not() {
        let mut objects = estate();
        let c = run(&objects, Subject::Conflicts);
        assert_eq!(c.len(), 1, "the T_REPORT conflict is out of scope");
        assert!(c[0].blocking);
        objects[5]
            .properties
            .insert("explanation".into(), json!({ "kind": "pending_migration" }));
        assert!(!run(&objects, Subject::Conflicts)[0].blocking);
    }

    #[test]
    fn a_decision_on_changed_facts_is_outdated() {
        let mut objects = estate();
        let wb = run(&objects, Subject::WriteBehaviour);
        objects.push(
            decision_object(
                Subject::WriteBehaviour,
                &wb[0],
                json!({ "behaviour": "confirm" }),
                None,
                "ann",
                "t1",
            )
            .unwrap(),
        );
        assert!(run(&objects, Subject::WriteBehaviour)[0].decision.is_some());
        objects[3] = behaviour("C_BPARTNER", "plain");
        let after = run(&objects, Subject::WriteBehaviour);
        assert!(after[0].outdated);
        assert!(after[0].decision.is_none());
    }

    #[test]
    fn the_latest_decision_wins() {
        let mut objects = estate();
        let scope = run(&objects, Subject::Scope);
        let t = scope.iter().find(|i| i.label == "T_REPORT").unwrap();
        for (v, at) in [("in", "t1"), ("out", "t2"), ("in", "t3")] {
            objects.push(
                decision_object(Subject::Scope, t, json!({ "scope": v }), None, "ann", at).unwrap(),
            );
        }
        let again = run(&objects, Subject::Scope);
        let t = again.iter().find(|i| i.label == "T_REPORT").unwrap();
        assert_eq!(t.decision.as_ref().unwrap()["scope"], "in");
    }

    #[test]
    fn decisions_are_checked() {
        assert!(check_decision(Subject::Scope, &json!({ "scope": "maybe" })).is_err());
        assert_eq!(
            check_decision(Subject::Pii, &json!({ "pii": "confirm" })).unwrap()["mask"],
            "redact"
        );
        assert!(check_decision(Subject::Pii, &json!({ "pii": "reject", "mask": "hash" })).is_err());
        assert!(
            check_decision(
                Subject::WriteBehaviour,
                &json!({ "behaviour": "override", "fields": { "colour": "red" } })
            )
            .is_err()
        );
        assert!(check_decision(Subject::Conflicts, &json!({})).is_err());
        let wb = run(&estate(), Subject::WriteBehaviour);
        let over = json!({ "behaviour": "override", "fields": { "change_log": true } });
        assert!(
            decision_object(
                Subject::WriteBehaviour,
                &wb[0],
                over.clone(),
                None,
                "a",
                "t"
            )
            .is_err()
        );
        assert!(
            decision_object(Subject::WriteBehaviour, &wb[0], over, Some("why"), "a", "t").is_ok()
        );
    }

    #[test]
    fn sign_off_needs_the_shown_digest_and_no_open_items() {
        let mut objects = estate();
        let c = run(&objects, Subject::Conflicts);
        let d = subject_digest(Subject::Conflicts, &c);
        assert!(
            approve_check(Subject::Conflicts, &c, &d)
                .unwrap_err()
                .contains("still open")
        );
        objects[5]
            .properties
            .insert("status".into(), json!("resolved"));
        let c = run(&objects, Subject::Conflicts);
        assert!(
            approve_check(Subject::Conflicts, &c, &d)
                .unwrap_err()
                .contains("changed")
        );
        let d = subject_digest(Subject::Conflicts, &c);
        assert_eq!(approve_check(Subject::Conflicts, &c, &d).unwrap(), d);
    }

    #[test]
    fn a_sign_off_is_valid_until_a_signed_fact_changes() {
        let mut objects = estate();
        let wb = run(&objects, Subject::WriteBehaviour);
        let d = approve_check(
            Subject::WriteBehaviour,
            &wb,
            &subject_digest(Subject::WriteBehaviour, &wb),
        )
        .unwrap();
        let s = signoff_object(Subject::WriteBehaviour, &d, &wb, "ann", "t1", None, None).unwrap();
        assert_eq!(
            signoff_state(Subject::WriteBehaviour, Some(&s), &wb).name(),
            "valid"
        );
        objects[3] = behaviour("C_BPARTNER", "plain");
        let wb = run(&objects, Subject::WriteBehaviour);
        match signoff_state(Subject::WriteBehaviour, Some(&s), &wb) {
            SignState::Stale {
                changed,
                added,
                removed,
                ..
            } => {
                assert_eq!(changed, vec!["C_BPARTNER".to_string()]);
                assert!(added.is_empty() && removed.is_empty());
            }
            other => panic!("{other:?}"),
        }
        let r = signoff_object(
            Subject::WriteBehaviour,
            &d,
            &wb,
            "ann",
            "t2",
            None,
            Some("redo"),
        )
        .unwrap();
        assert_eq!(
            signoff_state(Subject::WriteBehaviour, Some(&r), &wb).name(),
            "revoked"
        );
        assert_eq!(
            signoff_state(Subject::WriteBehaviour, None, &wb).name(),
            "none"
        );
    }

    #[test]
    fn the_latest_sign_off_wins() {
        let wb = run(&estate(), Subject::WriteBehaviour);
        let d = subject_digest(Subject::WriteBehaviour, &wb);
        let a = signoff_object(
            Subject::WriteBehaviour,
            &d,
            &wb,
            "ann",
            "t2",
            None,
            Some("x"),
        )
        .unwrap();
        let b = signoff_object(Subject::WriteBehaviour, &d, &wb, "ann", "t1", None, None).unwrap();
        let objects = [b, a];
        let latest = signoffs(&objects);
        assert_eq!(str_prop(latest["write-behaviour"], "state"), "revoked");
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_level() {
        let v = json!({ "b": 1, "a": { "d": [ { "z": 1, "y": 2 } ], "c": null } });
        assert_eq!(
            canonical(&v),
            r#"{"a":{"c":null,"d":[{"y":2,"z":1}]},"b":1}"#
        );
    }
}
