//! RFC 0255 §3.5 — what a source engine provides, and what it cannot: the contract.
//!
//! An engine **declares** its capabilities ([`Capabilities`]); the build **checks what can be
//! checked** ([`checked`]) and records the result as one `Custom("SourceCapabilities")` object per
//! source ([`object`], keyed by the source's name, so the ledger's latest version is the current
//! state). Every consumer reads it to say whether "no relationships" is a fact about the database
//! or a limit of the engine.
//!
//! The list is closed and versioned ([`CAPABILITIES_VERSION`]). A capability added later is an
//! RFC; an engine built before it reads it as `absent` with the reason "predates capability"
//! ([`Capabilities::get`]). Pure: no I/O.

use crate::{KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// The `Custom` kind of the recorded fact (registry row in [`crate::custom_kinds::REGISTRY`]).
pub const KIND: &str = "SourceCapabilities";

/// The version of the capability list. Version 1: the ten capabilities of RFC 0255 §3.5.
pub const CAPABILITIES_VERSION: u32 = 1;

/// One capability of a source engine (RFC 0255 §3.5's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// Tables, columns, types. Required.
    Catalog,
    /// The session's writes are refused (`Source::writes_refused`). Required.
    ReadOnlyProof,
    /// Primary and foreign keys.
    DeclaredKeys,
    /// Table and column comments, business names.
    Descriptions,
    /// Enum labels, `IN (…)` checks.
    ValueLists,
    /// Trigger and procedure source.
    Routines,
    /// A column's identity across renames.
    ColumnIdentity,
    /// Per-table and per-statement statistics.
    UsageStats,
    /// RFC 0252's closed aggregate vocabulary.
    Profiling,
    /// A reconciliation dialect that passes the conformance kit.
    Reconcile,
}

impl Capability {
    /// Every capability of [`CAPABILITIES_VERSION`], in the RFC's order.
    pub const ALL: [Capability; 10] = [
        Capability::Catalog,
        Capability::ReadOnlyProof,
        Capability::DeclaredKeys,
        Capability::Descriptions,
        Capability::ValueLists,
        Capability::Routines,
        Capability::ColumnIdentity,
        Capability::UsageStats,
        Capability::Profiling,
        Capability::Reconcile,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Capability::Catalog => "catalog",
            Capability::ReadOnlyProof => "read_only_proof",
            Capability::DeclaredKeys => "declared_keys",
            Capability::Descriptions => "descriptions",
            Capability::ValueLists => "value_lists",
            Capability::Routines => "routines",
            Capability::ColumnIdentity => "column_identity",
            Capability::UsageStats => "usage_stats",
            Capability::Profiling => "profiling",
            Capability::Reconcile => "reconcile",
        }
    }

    /// An engine without it is not a source.
    pub fn required(self) -> bool {
        matches!(self, Capability::Catalog | Capability::ReadOnlyProof)
    }

    /// What its consumers do when it is absent (RFC 0255 §3.5), in words for the operator.
    pub fn when_absent(self) -> &'static str {
        match self {
            Capability::Catalog | Capability::ReadOnlyProof => "required: the source is refused",
            Capability::DeclaredKeys => {
                "data-seeded discovery is the only seed; each candidate is a review item"
            }
            Capability::Descriptions => "a semantic gap; the passport says the engine has none",
            Capability::ValueLists => "no value test generated; literal coverage is the evidence",
            Capability::Routines => "no watermark is proven from code; the load falls back to full",
            Capability::ColumnIdentity => "a rename is seen as a drop and an add",
            Capability::UsageStats => {
                "observed interaction is empty and labelled \"engine provides none\""
            }
            Capability::Profiling => "no data-derived signals",
            Capability::Reconcile => "tables are never healthy: reported unverified",
        }
    }
}

/// Whether an engine has a capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "support")]
pub enum Support {
    Provided,
    Partial { note: String },
    Absent { reason: String },
}

impl Support {
    pub fn absent(reason: impl Into<String>) -> Self {
        Support::Absent {
            reason: reason.into(),
        }
    }

    pub fn partial(note: impl Into<String>) -> Self {
        Support::Partial { note: note.into() }
    }

    /// `provided`, `partial` or `absent`.
    pub fn word(&self) -> &'static str {
        match self {
            Support::Provided => "provided",
            Support::Partial { .. } => "partial",
            Support::Absent { .. } => "absent",
        }
    }

    /// The note or the reason; empty when provided.
    pub fn detail(&self) -> &str {
        match self {
            Support::Provided => "",
            Support::Partial { note } => note,
            Support::Absent { reason } => reason,
        }
    }

    pub fn is_provided(&self) -> bool {
        matches!(self, Support::Provided)
    }

    pub fn is_absent(&self) -> bool {
        matches!(self, Support::Absent { .. })
    }
}

/// An engine's capabilities, by capability. A capability the map lacks is absent: an engine
/// declares what it has, and silence is never "provided".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities(pub BTreeMap<Capability, Support>);

impl Capabilities {
    /// Every capability as given; the rest absent with "predates capability".
    pub fn of(entries: impl IntoIterator<Item = (Capability, Support)>) -> Self {
        Self(entries.into_iter().collect())
    }

    pub fn with(mut self, c: Capability, s: Support) -> Self {
        self.0.insert(c, s);
        self
    }

    /// The support for `c`. Not declared: absent, "predates capability".
    pub fn get(&self, c: Capability) -> Support {
        self.0
            .get(&c)
            .cloned()
            .unwrap_or_else(|| Support::absent("predates capability"))
    }

    /// Every capability of this version, in order, with its support.
    pub fn all(&self) -> Vec<(Capability, Support)> {
        Capability::ALL.iter().map(|c| (*c, self.get(*c))).collect()
    }

    /// The required capabilities the engine does not provide.
    pub fn missing_required(&self) -> Vec<Capability> {
        Capability::ALL
            .iter()
            .copied()
            .filter(|c| c.required() && !self.get(*c).is_provided())
            .collect()
    }
}

/// What the build measured, to check a declaration against (RFC 0255 §3.5 rules).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    /// The id of a conformance run of the engine's dialect, and whether it passed
    /// (`ekos_migrate_validate::conformance::Report`).
    pub conformance: Option<(String, bool)>,
    /// Tables read from the source's catalog at plan.
    pub tables_read: usize,
    /// Primary and foreign keys read among them.
    pub keys_read: usize,
}

/// The declaration, downgraded where the evidence contradicts it, with one line per downgrade.
///
/// - `reconcile` provided or partial without a recorded, passing conformance run → absent;
/// - `declared_keys` provided with zero keys read in a non-empty schema → partial, with the count.
pub fn checked(declared: &Capabilities, ev: &Evidence) -> (Capabilities, Vec<String>) {
    let mut out = declared.clone();
    let mut notes = vec![];
    if !declared.get(Capability::Reconcile).is_absent() {
        let reason = match &ev.conformance {
            Some((_, true)) => None,
            Some((id, false)) => Some(format!("conformance run {id} did not pass")),
            None => Some("no conformance run recorded".to_string()),
        };
        if let Some(reason) = reason {
            notes.push(format!(
                "reconcile: declared {}, downgraded to absent: {reason}",
                declared.get(Capability::Reconcile).word()
            ));
            out.0.insert(Capability::Reconcile, Support::absent(reason));
        }
    }
    if declared.get(Capability::DeclaredKeys).is_provided()
        && ev.tables_read > 0
        && ev.keys_read == 0
    {
        let note = format!("0 keys read from {} tables", ev.tables_read);
        notes.push(format!(
            "declared_keys: declared provided, downgraded to partial: {note}"
        ));
        out.0
            .insert(Capability::DeclaredKeys, Support::partial(note));
    }
    (out, notes)
}

/// One source's recorded capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCapabilities {
    /// The `[[databases]]` name, or the Lakehouse source (`lakehouse`).
    pub source: String,
    /// The engine's name (`postgres`, `oracle`, or a registered one).
    pub engine: String,
    pub capabilities_version: u32,
    /// After [`checked`].
    pub capabilities: Capabilities,
    /// The conformance run the `reconcile` capability rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conformance_run: Option<String>,
    /// What [`checked`] changed, and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub downgrades: Vec<String>,
}

/// The fact's id: one per source.
pub fn id_of(source: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("source-capabilities:{source}").as_bytes(),
    ))
}

/// The ledger object for `sc`.
pub fn object(sc: &SourceCapabilities) -> KirObject {
    let mut o = KirObject::new(
        format!("{} source capabilities", sc.source),
        ObjectKind::Custom(KIND.into()),
    );
    o.id = id_of(&sc.source);
    if let serde_json::Value::Object(map) = serde_json::to_value(sc).expect("serializes") {
        o.properties.extend(map);
    }
    o
}

/// The recorded capabilities in `o`, or `None` when `o` is not such an object.
pub fn from_object(o: &KirObject) -> Option<SourceCapabilities> {
    if o.kind != ObjectKind::Custom(KIND.into()) {
        return None;
    }
    let map: serde_json::Map<String, serde_json::Value> = o
        .properties
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    serde_json::from_value(serde_json::Value::Object(map)).ok()
}

/// The capability table as Markdown: one row per capability, with what is lost when it is not
/// provided.
pub fn markdown_table(sc: &SourceCapabilities) -> String {
    let mut out =
        String::from("| Capability | Support | Detail | When not provided |\n|---|---|---|---|\n");
    for (c, s) in sc.capabilities.all() {
        let cell = |t: &str| t.replace('|', "\\|").replace('\n', " ");
        out.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            c.name(),
            s.word(),
            cell(s.detail()),
            if s.is_provided() { "" } else { c.when_absent() }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_provided() -> Capabilities {
        Capabilities::of(Capability::ALL.iter().map(|c| (*c, Support::Provided)))
    }

    #[test]
    fn an_undeclared_capability_is_absent_and_says_why() {
        let c = Capabilities::of([(Capability::Catalog, Support::Provided)]);
        assert_eq!(
            c.get(Capability::UsageStats),
            Support::absent("predates capability")
        );
        assert_eq!(c.missing_required(), [Capability::ReadOnlyProof]);
        assert_eq!(c.all().len(), 10);
    }

    /// RFC 0255 §4 phase 4 exit: a planted engine declaring `reconcile: provided` without a run is
    /// downgraded; with a failed run it is downgraded naming the run; with a passing run it stays.
    #[test]
    fn reconcile_without_a_passing_run_is_downgraded() {
        let (c, notes) = checked(&all_provided(), &Evidence::default());
        assert_eq!(
            c.get(Capability::Reconcile),
            Support::absent("no conformance run recorded")
        );
        assert_eq!(
            notes,
            ["reconcile: declared provided, downgraded to absent: no conformance run recorded"]
        );
        let failed = Evidence {
            conformance: Some(("abc".into(), false)),
            ..Default::default()
        };
        let (c, _) = checked(&all_provided(), &failed);
        assert_eq!(
            c.get(Capability::Reconcile).detail(),
            "conformance run abc did not pass"
        );
        let passed = Evidence {
            conformance: Some(("abc".into(), true)),
            ..Default::default()
        };
        let (c, notes) = checked(&all_provided(), &passed);
        assert!(c.get(Capability::Reconcile).is_provided());
        assert!(notes.is_empty());
    }

    #[test]
    fn declared_keys_with_none_read_become_partial() {
        let ev = Evidence {
            conformance: Some(("x".into(), true)),
            tables_read: 12,
            keys_read: 0,
        };
        let (c, notes) = checked(&all_provided(), &ev);
        assert_eq!(
            c.get(Capability::DeclaredKeys),
            Support::partial("0 keys read from 12 tables")
        );
        assert_eq!(notes.len(), 1);
        // An empty schema proves nothing either way.
        let empty = Evidence {
            conformance: Some(("x".into(), true)),
            ..Default::default()
        };
        assert!(
            checked(&all_provided(), &empty)
                .0
                .get(Capability::DeclaredKeys)
                .is_provided()
        );
    }

    #[test]
    fn the_fact_round_trips_and_is_keyed_by_source() {
        let sc = SourceCapabilities {
            source: "erp".into(),
            engine: "oracle".into(),
            capabilities_version: CAPABILITIES_VERSION,
            capabilities: all_provided().with(Capability::UsageStats, Support::absent("not read")),
            conformance_run: Some("r1".into()),
            downgrades: vec![],
        };
        let o = object(&sc);
        assert_eq!(o.id, id_of("erp"));
        assert_eq!(o.kind, ObjectKind::Custom(KIND.into()));
        assert_eq!(from_object(&o), Some(sc.clone()));
        let t = markdown_table(&sc);
        assert!(
            t.contains("| `usage_stats` | absent | not read | observed interaction is empty"),
            "{t}"
        );
        assert!(t.contains("| `catalog` | provided |  |  |"), "{t}");
        assert_eq!(t.lines().count(), 12);
    }

    #[test]
    fn support_serializes_with_its_word() {
        let s = serde_json::to_string(&Support::partial("enum types only")).unwrap();
        assert_eq!(s, r#"{"support":"partial","note":"enum types only"}"#);
        let c = serde_json::to_string(&Capabilities::of([(
            Capability::Catalog,
            Support::Provided,
        )]))
        .unwrap();
        assert_eq!(c, r#"{"catalog":{"support":"provided"}}"#);
    }
}
