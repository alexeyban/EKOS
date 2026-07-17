//! RFC 0016 Phase 1 — the fact model: EAV decomposition and reconstruction.
//!
//! A ledger entry payload (the JSON form of a `KirObject`, `KirRelationship`,
//! `KirEvidence`, or `KirEvent`) decomposes into a set of **facts** —
//! `(entity, attribute, position, value)` tuples — and reconstructs from them
//! into a `serde_json::Value` that is **semantically identical** to the
//! original: value-equal, and therefore byte-identical after the same
//! canonicalization `content_signature` applies. That parity is the
//! invariant-critical property of the whole fact-segment engine: identity
//! (idempotent appends, branch merges, migration verification) is defined
//! over the canonical JSON, so decompose → reconstruct must never change it.
//!
//! Flattening rules (RFC 0016 §1, refined by this phase):
//!
//! - Non-empty JSON objects flatten into dotted attribute paths
//!   (`properties.metrics.rows`). Literal `.` and `\` in a key are escaped
//!   (`\.`, `\\`) so `{"a.b": 1}` and `{"a": {"b": 1}}` stay distinct.
//! - Arrays and **empty** objects/arrays are stored whole as one
//!   [`FactValue::Composite`] — except the top-level `evidence` array of
//!   canonical UUID strings, which decomposes into position-indexed
//!   [`FactValue::Ref`] facts (order is signature-relevant and preserved
//!   through [`Fact::pos`]).
//! - Ref detection is **schema-positional** (`id`, `from`, `to`, `subject`,
//!   `evidence[*]`), never sniffed from arbitrary strings; a value in a ref
//!   position that is not the canonical hyphenated-lowercase UUID form falls
//!   back to a plain string/composite so reconstruction reproduces it
//!   exactly.
//! - Numbers keep [`serde_json::Number`] semantics end to end — the
//!   i64/u64/f64 distinction and lexical form survive, so `1` and `1.0`
//!   remain different values (and different signatures).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

/// Monotone transaction number — the ordering authority of the fact engine
/// (RFC 0016 §2). Wall-clock time is metadata attached at the batch level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TxId(pub u64);

/// Interned attribute path. Ids are handed out append-only by
/// [`AttributeRegistry`] and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AttrId(pub u32);

/// Whether a fact asserts or retracts its `(attribute, position, value)`.
/// Retraction is itself an appended fact — nothing is ever deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FactOp {
    Assert,
    Retract,
}

/// A fact's value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FactValue {
    Null,
    Bool(bool),
    /// Preserves `serde_json::Number` exactly — signature-critical
    /// (RFC 0016 §2, numeric fidelity).
    Number(serde_json::Number),
    String(String),
    /// A reference to another entity (only ever produced for schema ref
    /// positions; reconstructs to the canonical hyphenated UUID string).
    Ref(Uuid),
    /// An array, or an empty object/array, stored whole (v1 flattening rule).
    Composite(serde_json::Value),
}

/// One EAV fact. `tx` and [`FactOp`] are stamped at the commit-batch level in
/// later phases; decomposition produces the timeless `(e, a, pos, v)` core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub entity: Uuid,
    pub attr: AttrId,
    /// Position for ordered multi-valued attributes (the `evidence` ref
    /// array); `None` for single-valued attributes.
    pub pos: Option<u32>,
    pub value: FactValue,
}

#[derive(Debug, Error)]
pub enum FactError {
    #[error("payload must be a JSON object, got {0}")]
    NotAnObject(String),
    #[error("unknown attribute id {0:?}")]
    UnknownAttr(AttrId),
    #[error("conflicting attribute paths at segment '{0}'")]
    PathConflict(String),
    #[error("multi-valued attribute '{0}' has inconsistent positions")]
    BadPositions(String),
}

// ── Attribute registry ──────────────────────────────────────────────────────

/// Append-only interner for attribute paths. Persisted in the manifest by
/// later phases; ids are stable for the lifetime of a ledger and never
/// reused.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AttributeRegistry {
    names: Vec<String>,
    #[serde(skip)]
    index: HashMap<String, AttrId>,
}

impl AttributeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild the lookup index after deserialization.
    pub fn reindex(&mut self) {
        self.index = self
            .names
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), AttrId(i as u32)))
            .collect();
    }

    /// Intern a path, allocating the next id if unseen.
    pub fn intern(&mut self, path: &str) -> AttrId {
        if let Some(&id) = self.index.get(path) {
            return id;
        }
        let id = AttrId(self.names.len() as u32);
        self.names.push(path.to_string());
        self.index.insert(path.to_string(), id);
        id
    }

    /// Look up an already-interned path without allocating an id.
    pub fn get(&self, path: &str) -> Option<AttrId> {
        self.index.get(path).copied()
    }

    pub fn name(&self, id: AttrId) -> Option<&str> {
        self.names.get(id.0 as usize).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

// ── Path escaping ───────────────────────────────────────────────────────────

/// Escape one key segment for use inside a dotted attribute path.
fn escape_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for c in segment.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '.' => out.push_str("\\."),
            other => out.push(other),
        }
    }
    out
}

/// Split a dotted attribute path back into unescaped key segments.
fn split_path(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(escaped) = chars.next() {
                    current.push(escaped);
                }
            }
            '.' => segments.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    segments.push(current);
    segments
}

// ── Decomposition ───────────────────────────────────────────────────────────

/// Top-level fields holding a single entity reference.
const REF_SCALAR_ATTRS: &[&str] = &["id", "from", "to", "subject"];
/// Top-level fields holding an ordered array of entity references.
const REF_ARRAY_ATTRS: &[&str] = &["evidence"];

/// A string is treated as a reference only in its canonical serialized form,
/// so reconstruction (`Uuid::to_string`) reproduces the input byte-for-byte.
fn canonical_uuid(s: &str) -> Option<Uuid> {
    let u = Uuid::parse_str(s).ok()?;
    (u.to_string() == s).then_some(u)
}

/// Decompose an entry payload into facts. The payload must be a JSON object
/// (every KIR entry is). Inverse of [`reconstruct`].
pub fn decompose(
    entity: Uuid,
    payload: &serde_json::Value,
    registry: &mut AttributeRegistry,
) -> Result<Vec<Fact>, FactError> {
    let serde_json::Value::Object(map) = payload else {
        return Err(FactError::NotAnObject(type_name(payload).to_string()));
    };

    let mut facts = Vec::new();
    for (key, value) in map {
        let path = escape_segment(key);

        // Schema ref positions first; anything not in canonical form falls
        // through to the generic rules so it round-trips verbatim.
        if REF_SCALAR_ATTRS.contains(&key.as_str())
            && let Some(u) = value.as_str().and_then(canonical_uuid)
        {
            facts.push(Fact {
                entity,
                attr: registry.intern(&path),
                pos: None,
                value: FactValue::Ref(u),
            });
            continue;
        }
        if REF_ARRAY_ATTRS.contains(&key.as_str())
            && let serde_json::Value::Array(items) = value
            && !items.is_empty()
            && let Some(refs) = items
                .iter()
                .map(|v| v.as_str().and_then(canonical_uuid))
                .collect::<Option<Vec<_>>>()
        {
            let attr = registry.intern(&path);
            for (i, u) in refs.into_iter().enumerate() {
                facts.push(Fact {
                    entity,
                    attr,
                    pos: Some(i as u32),
                    value: FactValue::Ref(u),
                });
            }
            continue;
        }

        flatten(entity, &path, value, registry, &mut facts);
    }
    Ok(facts)
}

fn flatten(
    entity: Uuid,
    path: &str,
    value: &serde_json::Value,
    registry: &mut AttributeRegistry,
    out: &mut Vec<Fact>,
) {
    use serde_json::Value as J;
    let leaf = |value: FactValue, registry: &mut AttributeRegistry| Fact {
        entity,
        attr: registry.intern(path),
        pos: None,
        value,
    };
    match value {
        J::Object(map) if !map.is_empty() => {
            for (key, child) in map {
                let child_path = format!("{path}.{}", escape_segment(key));
                flatten(entity, &child_path, child, registry, out);
            }
        }
        J::Object(_) | J::Array(_) => {
            out.push(leaf(FactValue::Composite(value.clone()), registry));
        }
        J::Null => out.push(leaf(FactValue::Null, registry)),
        J::Bool(b) => out.push(leaf(FactValue::Bool(*b), registry)),
        J::Number(n) => out.push(leaf(FactValue::Number(n.clone()), registry)),
        J::String(s) => out.push(leaf(FactValue::String(s.clone()), registry)),
    }
}

fn type_name(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ── Reconstruction ──────────────────────────────────────────────────────────

fn value_to_json(value: &FactValue) -> serde_json::Value {
    match value {
        FactValue::Null => serde_json::Value::Null,
        FactValue::Bool(b) => serde_json::Value::Bool(*b),
        FactValue::Number(n) => serde_json::Value::Number(n.clone()),
        FactValue::String(s) => serde_json::Value::String(s.clone()),
        FactValue::Ref(u) => serde_json::Value::String(u.to_string()),
        FactValue::Composite(v) => v.clone(),
    }
}

/// Reconstruct the entry payload from one entity's facts. Fact order does not
/// matter. Inverse of [`decompose`]: the result is value-equal to the
/// original payload, and signature-equal after canonicalization.
pub fn reconstruct(
    facts: &[Fact],
    registry: &AttributeRegistry,
) -> Result<serde_json::Value, FactError> {
    // Group by attribute; positioned groups become ordered arrays.
    let mut groups: HashMap<AttrId, Vec<&Fact>> = HashMap::new();
    for fact in facts {
        groups.entry(fact.attr).or_default().push(fact);
    }

    // Deterministic assembly order (serde_json's BTreeMap sorts keys anyway,
    // but path insertion conflicts should not depend on input order).
    let mut entries: Vec<(&str, Vec<&Fact>)> = Vec::with_capacity(groups.len());
    for (attr, group) in groups {
        let name = registry.name(attr).ok_or(FactError::UnknownAttr(attr))?;
        entries.push((name, group));
    }
    entries.sort_by(|a, b| a.0.cmp(b.0));

    let mut root = serde_json::Map::new();
    for (name, mut group) in entries {
        let json = if group.len() == 1 && group[0].pos.is_none() {
            value_to_json(&group[0].value)
        } else {
            // Ordered multi-valued attribute: positions must be 0..n.
            group.sort_by_key(|f| f.pos);
            for (i, f) in group.iter().enumerate() {
                if f.pos != Some(i as u32) {
                    return Err(FactError::BadPositions(name.to_string()));
                }
            }
            serde_json::Value::Array(group.iter().map(|f| value_to_json(&f.value)).collect())
        };
        insert_path(&mut root, &split_path(name), json)?;
    }
    Ok(serde_json::Value::Object(root))
}

fn insert_path(
    root: &mut serde_json::Map<String, serde_json::Value>,
    segments: &[String],
    value: serde_json::Value,
) -> Result<(), FactError> {
    let (first, rest) = segments
        .split_first()
        .expect("split_path yields ≥1 segment");
    if rest.is_empty() {
        if root.insert(first.clone(), value).is_some() {
            return Err(FactError::PathConflict(first.clone()));
        }
        return Ok(());
    }
    let child = root
        .entry(first.clone())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    match child {
        serde_json::Value::Object(map) => insert_path(map, rest, value),
        _ => Err(FactError::PathConflict(first.clone())),
    }
}

// ── Diff ────────────────────────────────────────────────────────────────────

/// Semantic delta between two versions of one entity: the assert/retract
/// facts a commit batch writes. This is the compactness win of RFC 0016 —
/// a changed property is one retract + one assert, not a payload copy.
pub fn diff(old: &[Fact], new: &[Fact]) -> Vec<(FactOp, Fact)> {
    let key = |f: &Fact| (f.attr, f.pos);
    let old_map: HashMap<_, &Fact> = old.iter().map(|f| (key(f), f)).collect();
    let new_map: HashMap<_, &Fact> = new.iter().map(|f| (key(f), f)).collect();

    let mut out = Vec::new();
    for (k, f) in &old_map {
        match new_map.get(k) {
            Some(n) if n.value == f.value => {}
            _ => out.push((FactOp::Retract, (*f).clone())),
        }
    }
    for (k, f) in &new_map {
        match old_map.get(k) {
            Some(o) if o.value == f.value => {}
            _ => out.push((FactOp::Assert, (*f).clone())),
        }
    }
    // Deterministic output order: retracts before asserts, then by (attr, pos).
    out.sort_by_key(|(op, f)| (matches!(op, FactOp::Assert), f.attr, f.pos));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_signature;
    use ekos_kir::{
        KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
        SourceLocation,
    };

    fn round_trip(payload: &serde_json::Value) -> serde_json::Value {
        let mut reg = AttributeRegistry::new();
        let facts = decompose(Uuid::new_v4(), payload, &mut reg).unwrap();
        reconstruct(&facts, &reg).unwrap()
    }

    /// Value equality plus signature equality — the Phase 1 gate.
    fn assert_parity(payload: serde_json::Value) {
        let back = round_trip(&payload);
        assert_eq!(back, payload, "reconstruction must be value-equal");
        assert_eq!(
            content_signature(&back),
            content_signature(&payload),
            "signature parity is mandatory"
        );
    }

    #[test]
    fn object_round_trips_with_signature_parity() {
        let obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("path", serde_json::json!("db/orders.sql"))
            .with_property("size_bytes", serde_json::json!(4096))
            .with_property("excerpt", serde_json::json!("CREATE TABLE orders (…)"))
            .with_evidence(KirId::new())
            .with_evidence(KirId::new());
        assert_parity(serde_json::to_value(&obj).unwrap());
    }

    #[test]
    fn relationship_and_evidence_round_trip() {
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, KirId::new(), KirId::new());
        assert_parity(serde_json::to_value(&rel).unwrap());

        // SourceLocation carries Option fields serialized as nulls.
        let ev = KirEvidence::new(SourceLocation::at("schema.sql", 42), "CREATE TABLE …")
            .with_confidence(0.87);
        assert_parity(serde_json::to_value(&ev).unwrap());
    }

    #[test]
    fn typed_reconstruction_is_lossless() {
        let obj = KirObject::new("customers", ObjectKind::Table)
            .with_property("rows", serde_json::json!(120));
        let payload = serde_json::to_value(&obj).unwrap();
        let back: KirObject = serde_json::from_value(round_trip(&payload)).unwrap();
        assert_eq!(back.id, obj.id);
        assert_eq!(back.name, obj.name);
        assert_eq!(back.properties, obj.properties);
        assert_eq!(back.evidence, obj.evidence);
    }

    /// RFC 0016 §2: numbers must survive exactly — `1` and `1.0` are
    /// different values with different signatures.
    #[test]
    fn numeric_fidelity_edge_cases() {
        for n in [
            serde_json::json!(1),
            serde_json::json!(1.0),
            serde_json::json!(u64::MAX),
            serde_json::json!(i64::MIN),
            serde_json::json!(-0.0),
            serde_json::json!(1e300),
            serde_json::json!(0.1),
        ] {
            assert_parity(serde_json::json!({ "v": n }));
        }

        let int = serde_json::json!({ "v": 1 });
        let float = serde_json::json!({ "v": 1.0 });
        assert_ne!(round_trip(&int), round_trip(&float));
        assert_ne!(content_signature(&int), content_signature(&float));
    }

    /// Dotted keys must not collide with genuinely nested objects.
    #[test]
    fn dotted_keys_stay_distinct_from_nesting() {
        let dotted = serde_json::json!({ "properties": { "a.b": 1 } });
        let nested = serde_json::json!({ "properties": { "a": { "b": 1 } } });
        assert_parity(dotted.clone());
        assert_parity(nested.clone());
        assert_ne!(round_trip(&dotted), round_trip(&nested));

        // Backslashes in keys survive the escaping too.
        assert_parity(serde_json::json!({ "properties": { "win\\path.ext": true } }));
    }

    #[test]
    fn empty_containers_and_arrays_round_trip() {
        assert_parity(serde_json::json!({
            "properties": {
                "empty_obj": {},
                "empty_arr": [],
                "tags": ["a", "b", "a"],
                "mixed": [1, "two", null, {"three": 3}],
                "nothing": null
            },
            "evidence": []
        }));
    }

    /// Evidence order is signature-relevant and must survive via positions.
    #[test]
    fn evidence_order_is_preserved() {
        let a = KirId::new();
        let b = KirId::new();
        let fwd = serde_json::json!({ "evidence": [a.to_string(), b.to_string()] });
        let rev = serde_json::json!({ "evidence": [b.to_string(), a.to_string()] });
        assert_parity(fwd.clone());
        assert_parity(rev.clone());
        assert_ne!(round_trip(&fwd), round_trip(&rev));
    }

    /// A ref-position value that is not canonical UUID text must round-trip
    /// verbatim (fallback to string/composite, never a lossy re-parse).
    #[test]
    fn non_canonical_ref_values_fall_back_verbatim() {
        let payload = serde_json::json!({
            "id": "NOT-A-UUID",
            "from": "6E185858-1FCA-4B29-B0E9-B0F63B4A0B41", // uppercase: non-canonical
            "evidence": ["also not a uuid"]
        });
        assert_parity(payload);
    }

    /// The compactness claim: a one-property change is exactly one retract
    /// plus one assert — not a payload copy.
    #[test]
    fn diff_of_property_change_is_two_facts() {
        let mut obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("size_bytes", serde_json::json!(100))
            .with_property("path", serde_json::json!("db/orders.sql"))
            .with_evidence(KirId::new());
        let mut reg = AttributeRegistry::new();
        let e = obj.id.0;
        let old = decompose(e, &serde_json::to_value(&obj).unwrap(), &mut reg).unwrap();

        obj.properties
            .insert("size_bytes".into(), serde_json::json!(200));
        let new = decompose(e, &serde_json::to_value(&obj).unwrap(), &mut reg).unwrap();

        let delta = diff(&old, &new);
        assert_eq!(delta.len(), 2, "one changed property = retract + assert");
        assert!(matches!(delta[0], (FactOp::Retract, _)));
        assert!(matches!(delta[1], (FactOp::Assert, _)));

        // Identical versions produce an empty delta; a new evidence entry
        // appends exactly one fact.
        assert!(diff(&new, &new).is_empty());
        obj.evidence.push(KirId::new());
        let appended = decompose(e, &serde_json::to_value(&obj).unwrap(), &mut reg).unwrap();
        let delta = diff(&new, &appended);
        assert_eq!(delta.len(), 1);
        assert!(matches!(delta[0], (FactOp::Assert, _)));
    }

    #[test]
    fn registry_ids_are_stable_and_reindexable() {
        let mut reg = AttributeRegistry::new();
        let a = reg.intern("name");
        let b = reg.intern("properties.path");
        assert_eq!(reg.intern("name"), a, "re-interning returns the same id");
        assert_ne!(a, b);

        // Serialize → deserialize → reindex reproduces lookups.
        let json = serde_json::to_string(&reg).unwrap();
        let mut back: AttributeRegistry = serde_json::from_str(&json).unwrap();
        back.reindex();
        assert_eq!(back.intern("properties.path"), b);
        assert_eq!(back.name(a), Some("name"));
    }

    #[test]
    fn reconstruction_is_order_independent() {
        let obj = KirObject::new("orders", ObjectKind::Table)
            .with_property("nested", serde_json::json!({"a": {"b": 1}, "c": 2}))
            .with_evidence(KirId::new())
            .with_evidence(KirId::new());
        let payload = serde_json::to_value(&obj).unwrap();

        let mut reg = AttributeRegistry::new();
        let mut facts = decompose(obj.id.0, &payload, &mut reg).unwrap();
        facts.reverse();
        let back = reconstruct(&facts, &reg).unwrap();
        assert_eq!(back, payload);
    }
}
