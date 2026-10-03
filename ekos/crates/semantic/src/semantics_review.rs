//! RFC 0170 Phase 2 — the review lifecycle of business-semantics items.
//!
//! ```text
//! hypothesis ──confirm/edit──▶ confirmed ──evidence changes──▶ needs_review ──confirm/edit──▶ confirmed
//!      │                           │                                │
//!      └──reject──▶ rejected ──────┴──traces vanish (stale)─────────┘
//! ```
//!
//! **Human-only.** Only the CLI (`ekos semantics confirm|reject|edit`) may call [`apply_review`];
//! nothing in `commands/mcp.rs` may name this module, and a source-scanning test in the CLI fails
//! the build if it does. An agent can read a hypothesis; it can never promote one.
//!
//! **Confirmations do not carry over changed assertions** — the same principle as identity review.
//! Each item carries a `signature` over what it asserts and the evidence it rests on (path +
//! fragment; line numbers are left out so an unrelated edit above a predicate does not reopen every
//! review). At `commit`, [`carry_forward`] keeps a review only while the signature it was given
//! against still matches; otherwise the item becomes `needs_review`, keeping the old decision in
//! `previous_review`. A reviewed item whose traces disappear entirely becomes a stale
//! `needs_review` version ([`stale_version`]): the code no longer supports what was confirmed.

use crate::business_semantics::{
    CONCEPT, CONFLICT, CONSTRAINT, ENUM_MEANING, GAP, HYPOTHESIS, RATIONALE, kind_name,
};
use ekos_kir::{KirEvidence, KirObject};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub const CONFIRMED: &str = "confirmed";
pub const REJECTED: &str = "rejected";
pub const NEEDS_REVIEW: &str = "needs_review";

/// Properties that belong to the review, not to what was recovered. Never part of a signature.
pub const REVIEW_FIELDS: [&str; 10] = [
    "status",
    "review_reason",
    "reviewed_by",
    "reviewed_at",
    "review_note",
    "reviewed_signature",
    "expert_name",
    "expert_description",
    "expert_label",
    "previous_review",
];

/// The properties that state what an item *asserts*, per kind. A change in any of them, or in its
/// evidence, is a new assertion.
fn core_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        k if k == CONCEPT => &["definition", "table"],
        k if k == ENUM_MEANING => &["table", "column", "value", "label", "meanings"],
        k if k == CONSTRAINT => &["table", "expression"],
        k if k == GAP => &["gap_type", "table", "column", "value", "concept"],
        k if k == CONFLICT => &["conflict_type", "definitions"],
        k if k == RATIONALE => &["sha", "path"],
        _ => &[],
    }
}

/// The signature of what `o` asserts and rests on. `evidence` are `o`'s own records.
pub fn signature(o: &KirObject, evidence: &[&KirEvidence]) -> String {
    let kind = kind_name(o).unwrap_or_default();
    let mut core = serde_json::Map::new();
    for f in core_fields(kind) {
        if let Some(v) = o.properties.get(*f) {
            // `meanings` carry lines too; keep only what they say and where.
            let v = if *f == "meanings" {
                json!(
                    v.as_array()
                        .map(|a| a
                            .iter()
                            .map(|m| json!([m["label"], m["source"], m["path"]]))
                            .collect::<Vec<_>>())
                        .unwrap_or_default()
                )
            } else {
                v.clone()
            };
            core.insert((*f).into(), v);
        }
    }
    let ev: BTreeSet<(String, String)> = evidence
        .iter()
        .map(|e| (e.location.path.clone(), e.fragment.clone()))
        .collect();
    let canonical = json!({"kind": kind, "core": core, "evidence": ev});
    ekos_common::ContentHash::of_str(&canonical.to_string())
        .as_str()
        .to_string()
}

fn status(o: &KirObject) -> &str {
    o.properties
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or(HYPOTHESIS)
}

fn review_record(o: &KirObject) -> Value {
    json!({
        "status": status(o),
        "reviewed_by": o.properties.get("reviewed_by"),
        "reviewed_at": o.properties.get("reviewed_at"),
        "review_note": o.properties.get("review_note"),
    })
}

/// Whether `o` carries a human decision (or is waiting for one).
pub fn is_reviewed(o: &KirObject) -> bool {
    matches!(status(o), CONFIRMED | REJECTED | NEEDS_REVIEW)
}

/// Bring the ledger's `current` review state onto a freshly synthesized item. `fresh` carries its
/// own `signature` and `status: hypothesis`.
pub fn carry_forward(fresh: &mut KirObject, current: Option<&KirObject>) {
    let Some(current) = current else { return };
    if !is_reviewed(current) {
        return;
    }
    let fresh_sig = fresh
        .properties
        .get("signature")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let reviewed_sig = current
        .properties
        .get("reviewed_signature")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let unchanged = status(current) != NEEDS_REVIEW && reviewed_sig == fresh_sig;
    // Expert edits and the decision trail survive either way; the status only while unchanged.
    for f in REVIEW_FIELDS {
        if let Some(v) = current.properties.get(f) {
            fresh.properties.insert(f.into(), v.clone());
        }
    }
    fresh.properties.remove("stale");
    if unchanged {
        return;
    }
    if status(current) != NEEDS_REVIEW {
        fresh
            .properties
            .insert("previous_review".into(), review_record(current));
        fresh.properties.insert(
            "review_reason".into(),
            json!("what this item asserts, or its evidence, changed since it was reviewed"),
        );
    }
    fresh
        .properties
        .insert("status".into(), json!(NEEDS_REVIEW));
}

/// The version to write for a reviewed item the latest run no longer derives: its traces are gone.
/// `None` when nothing needs writing (not reviewed, rejected, or already marked stale).
pub fn stale_version(current: &KirObject) -> Option<KirObject> {
    if !is_reviewed(current)
        || status(current) == REJECTED
        || current.properties.get("stale") == Some(&json!(true))
    {
        return None;
    }
    let mut o = current.clone();
    if status(current) != NEEDS_REVIEW {
        o.properties
            .insert("previous_review".into(), review_record(current));
    }
    o.properties.insert("status".into(), json!(NEEDS_REVIEW));
    o.properties.insert("stale".into(), json!(true));
    o.properties.insert(
        "review_reason".into(),
        json!("the code no longer contains the traces this item was recovered from"),
    );
    Some(o)
}

/// A human decision.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Confirm,
    Reject,
    /// Confirm with corrections; `None` leaves a field as recovered.
    Edit {
        name: Option<String>,
        description: Option<String>,
        label: Option<String>,
    },
}

/// The new version of `current` after a human `decision`. The CLI writes it; nothing else may.
pub fn apply_review(
    current: &KirObject,
    decision: &Decision,
    by: &str,
    at: &str,
    note: Option<&str>,
) -> Result<KirObject, String> {
    let kind = kind_name(current).ok_or("not a business-semantics item")?;
    if kind == RATIONALE {
        return Err("a rationale link is a git fact, not a hypothesis to review".into());
    }
    if by.trim().is_empty() {
        return Err("a review needs a reviewer".into());
    }
    if *decision == Decision::Reject && note.is_none_or(|n| n.trim().is_empty()) {
        return Err("say why: a rejection needs --note".into());
    }
    // Confirming what is already confirmed, unchanged and without a new note, says nothing new:
    // the same version comes back, so the ledger writes nothing.
    if *decision == Decision::Confirm
        && status(current) == CONFIRMED
        && note.is_none_or(|n| n.trim().is_empty())
        && current.properties.get("reviewed_signature") == current.properties.get("signature")
    {
        return Ok(current.clone());
    }
    let mut o = current.clone();
    let (new_status, _) = match decision {
        Decision::Confirm => (CONFIRMED, ()),
        Decision::Reject => (REJECTED, ()),
        Decision::Edit {
            name,
            description,
            label,
        } => {
            if name.is_none() && description.is_none() && label.is_none() {
                return Err("an edit needs --name, --description or --label".into());
            }
            if label.is_some() && kind != ENUM_MEANING {
                return Err("--label applies to a coded value (EnumMeaning) only".into());
            }
            for (k, v) in [
                ("expert_name", name),
                ("expert_description", description),
                ("expert_label", label),
            ] {
                if let Some(v) = v {
                    o.properties.insert(k.into(), json!(v));
                }
            }
            (CONFIRMED, ())
        }
    };
    if is_reviewed(current) && status(current) != NEEDS_REVIEW {
        o.properties
            .insert("previous_review".into(), review_record(current));
    }
    let sig = current
        .properties
        .get("signature")
        .cloned()
        .unwrap_or(Value::Null);
    o.properties.insert("status".into(), json!(new_status));
    o.properties.insert("reviewed_by".into(), json!(by));
    o.properties.insert("reviewed_at".into(), json!(at));
    o.properties.insert("reviewed_signature".into(), sig);
    match note {
        Some(n) => o.properties.insert("review_note".into(), json!(n)),
        None => o.properties.remove("review_note"),
    };
    o.properties.remove("review_reason");
    o.properties.remove("stale");
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{ObjectKind, SourceLocation};

    fn concept(def: &str) -> KirObject {
        let mut o = KirObject::new("PartsNotObsolete", ObjectKind::BusinessConcept);
        o.properties.insert("definition".into(), json!(def));
        o.properties.insert("table".into(), json!("parts"));
        o.properties.insert("status".into(), json!(HYPOTHESIS));
        let ev = KirEvidence::new(
            SourceLocation::at("sql/a.sql", 10),
            "parts.obsolete IS FALSE",
        );
        o.properties
            .insert("signature".into(), json!(signature(&o, &[&ev])));
        o
    }

    #[test]
    fn a_signature_ignores_line_moves_but_not_content() {
        let o = concept("parts.obsolete IS FALSE");
        let a = KirEvidence::new(SourceLocation::at("sql/a.sql", 10), "x");
        let b = KirEvidence::new(SourceLocation::at("sql/a.sql", 99), "x");
        let c = KirEvidence::new(SourceLocation::at("sql/a.sql", 10), "y");
        assert_eq!(signature(&o, &[&a]), signature(&o, &[&b]));
        assert_ne!(signature(&o, &[&a]), signature(&o, &[&c]));
        assert_ne!(
            signature(&o, &[&a]),
            signature(&concept("parts.obsolete IS TRUE"), &[&a])
        );
    }

    #[test]
    fn a_confirmation_survives_only_an_unchanged_assertion() {
        let reviewed = apply_review(
            &concept("parts.obsolete IS FALSE"),
            &Decision::Confirm,
            "ann",
            "2026-10-03T00:00:00Z",
            None,
        )
        .unwrap();
        assert_eq!(status(&reviewed), CONFIRMED);

        let mut same = concept("parts.obsolete IS FALSE");
        carry_forward(&mut same, Some(&reviewed));
        assert_eq!(status(&same), CONFIRMED);
        assert_eq!(
            same.properties, reviewed.properties,
            "a re-run writes nothing new"
        );

        let mut changed = concept("parts.obsolete IS TRUE");
        carry_forward(&mut changed, Some(&reviewed));
        assert_eq!(status(&changed), NEEDS_REVIEW);
        assert_eq!(
            changed.properties["previous_review"]["status"],
            json!(CONFIRMED)
        );

        // Once flagged, it stays flagged until a human looks again — and an unchanged re-run
        // reproduces the flagged version exactly (the reason included), so nothing is rewritten.
        let mut again = concept("parts.obsolete IS TRUE");
        carry_forward(&mut again, Some(&changed));
        assert_eq!(status(&again), NEEDS_REVIEW);
        assert_eq!(again.properties, changed.properties);
        let re = apply_review(&again, &Decision::Confirm, "ann", "t2", None).unwrap();
        let mut after = concept("parts.obsolete IS TRUE");
        carry_forward(&mut after, Some(&re));
        assert_eq!(status(&after), CONFIRMED);
    }

    #[test]
    fn an_unreviewed_item_is_untouched_and_vanished_reviews_go_stale() {
        let mut fresh = concept("parts.obsolete IS FALSE");
        let before = fresh.clone();
        carry_forward(&mut fresh, Some(&concept("parts.obsolete IS FALSE")));
        assert_eq!(fresh.properties, before.properties);
        assert!(stale_version(&before).is_none());

        let confirmed = apply_review(&before, &Decision::Confirm, "ann", "t", None).unwrap();
        let stale = stale_version(&confirmed).unwrap();
        assert_eq!(status(&stale), NEEDS_REVIEW);
        assert!(stale_version(&stale).is_none(), "marked once");
        let rejected =
            apply_review(&before, &Decision::Reject, "ann", "t", Some("plumbing")).unwrap();
        assert!(stale_version(&rejected).is_none());
    }

    #[test]
    fn reconfirming_an_unchanged_confirmation_is_a_no_op() {
        let c = apply_review(
            &concept("parts.obsolete IS FALSE"),
            &Decision::Confirm,
            "ann",
            "t1",
            None,
        )
        .unwrap();
        let again = apply_review(&c, &Decision::Confirm, "bob", "t2", None).unwrap();
        assert_eq!(again.properties, c.properties);
        let noted =
            apply_review(&c, &Decision::Confirm, "bob", "t2", Some("checked again")).unwrap();
        assert_eq!(noted.properties["reviewed_by"], json!("bob"));
    }

    #[test]
    fn reviews_are_checked() {
        let c = concept("parts.obsolete IS FALSE");
        assert!(apply_review(&c, &Decision::Reject, "ann", "t", None).is_err());
        assert!(apply_review(&c, &Decision::Confirm, " ", "t", None).is_err());
        let label = Decision::Edit {
            name: None,
            description: None,
            label: Some("x".into()),
        };
        assert!(apply_review(&c, &label, "ann", "t", None).is_err());
        let edit = Decision::Edit {
            name: Some("ActivePart".into()),
            description: Some("A part still sold.".into()),
            label: None,
        };
        let e = apply_review(&c, &edit, "ann", "t", Some("renamed")).unwrap();
        assert_eq!(status(&e), CONFIRMED);
        assert_eq!(e.properties["expert_name"], json!("ActivePart"));
        assert_eq!(
            e.properties["reviewed_signature"],
            c.properties["signature"]
        );
    }
}
