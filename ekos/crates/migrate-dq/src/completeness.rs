//! RFC 0158 — the completeness check.
//!
//! RFC 0154 promises that every source object is recovered, classified and dispositioned, and that
//! a migration reaching `signed_off` with an unclassified object is a defect in Migrate rather than
//! a limitation of it. That sentence is worth nothing without a mechanical check, and this is it.
//!
//! The asymmetry is the point: **"no rule matched this object" is a failure of the check, not a
//! pass.** A silent gap has to cost something, or the coverage guarantee decays into "we handled
//! what we thought of".

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How one source object came to be accounted for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "how")]
pub enum Accounted {
    /// A mapping exists, with the evidence behind it.
    Translated { evidence: String },
    /// A human decided what happens to it.
    Dispositioned { disposition: String, actor: String },
}

/// The denominator: every object the source catalog reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceObject {
    pub qualified_name: String,
    pub kind: String,
    /// `true` where no target has an equivalent at all — a trigger, an RLS policy, an exclusion
    /// constraint. These can only ever be *dispositioned*, never translated, and saying so up front
    /// is more honest than letting them fail a mapping search later.
    pub no_target_equivalent: bool,
}

/// What the check found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletenessReport {
    pub total: usize,
    pub accounted: usize,
    /// Unaccounted objects, by kind, so a gap is always specific: "9 triggers and 2 extensions",
    /// never "94% complete".
    pub gaps: BTreeMap<String, Vec<String>>,
}

impl CompletenessReport {
    /// The only question sign-off asks.
    pub fn passes(&self) -> bool {
        self.gaps.is_empty()
    }

    /// A human-readable gap summary. Deliberately not a percentage: a percentage invites someone to
    /// call 97% good enough, and the missing 3% is where the triggers are.
    pub fn summary(&self) -> String {
        if self.passes() {
            return format!(
                "{}/{} source objects accounted for.",
                self.accounted, self.total
            );
        }
        let mut parts: Vec<String> = self
            .gaps
            .iter()
            .map(|(kind, names)| format!("{} {kind}", names.len()))
            .collect();
        parts.sort();
        format!(
            "{}/{} accounted for. Unclassified: {}.",
            self.accounted,
            self.total,
            parts.join(", ")
        )
    }
}

/// Check every source object against what is known about it.
pub fn check(
    objects: &[SourceObject],
    accounted: &BTreeMap<String, Accounted>,
) -> CompletenessReport {
    let mut gaps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut n = 0;
    for o in objects {
        if accounted.contains_key(&o.qualified_name) {
            n += 1;
        } else {
            gaps.entry(o.kind.clone())
                .or_default()
                .push(o.qualified_name.clone());
        }
    }
    for names in gaps.values_mut() {
        names.sort();
    }
    CompletenessReport {
        total: objects.len(),
        accounted: n,
        gaps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(name: &str, kind: &str, no_equiv: bool) -> SourceObject {
        SourceObject {
            qualified_name: name.into(),
            kind: kind.into(),
            no_target_equivalent: no_equiv,
        }
    }

    fn dispositioned() -> Accounted {
        Accounted::Dispositioned {
            disposition: "DISP.1".into(),
            actor: "human:alex".into(),
        }
    }

    #[test]
    fn an_unmatched_object_fails_the_check() {
        let objects = [obj("public.orders", "table", false)];
        let r = check(&objects, &BTreeMap::new());
        assert!(!r.passes(), "no rule matched must not read as a pass");
        assert_eq!(r.gaps["table"], vec!["public.orders"]);
    }

    #[test]
    fn everything_accounted_for_passes() {
        let objects = [obj("public.orders", "table", false)];
        let mut acc = BTreeMap::new();
        acc.insert(
            "public.orders".to_string(),
            Accounted::Translated {
                evidence: "MAP.1".into(),
            },
        );
        let r = check(&objects, &acc);
        assert!(r.passes());
        assert_eq!(r.summary(), "1/1 source objects accounted for.");
    }

    /// A trigger has no ClickHouse equivalent. That does not excuse it from the check — it just
    /// means the only route to "accounted" is a human decision.
    #[test]
    fn an_object_with_no_target_equivalent_still_has_to_be_dispositioned() {
        let objects = [obj("public.orders_audit", "trigger", true)];
        assert!(!check(&objects, &BTreeMap::new()).passes());

        let mut acc = BTreeMap::new();
        acc.insert("public.orders_audit".to_string(), dispositioned());
        assert!(check(&objects, &acc).passes());
    }

    /// The gap is named per kind, because "9 triggers and 2 extensions are unclassified" is
    /// actionable and "94% complete" is not.
    #[test]
    fn gaps_are_reported_per_kind_never_as_a_percentage() {
        let objects = [
            obj("a", "trigger", true),
            obj("b", "trigger", true),
            obj("c", "extension", true),
            obj("d", "table", false),
        ];
        let mut acc = BTreeMap::new();
        acc.insert("d".to_string(), dispositioned());
        let r = check(&objects, &acc);
        assert_eq!(r.gaps["trigger"].len(), 2);
        assert_eq!(r.gaps["extension"].len(), 1);
        let s = r.summary();
        assert!(s.contains("2 trigger"), "{s}");
        assert!(s.contains("1 extension"), "{s}");
        assert!(
            !s.contains('%'),
            "a percentage invites calling 97% good enough: {s}"
        );
    }

    #[test]
    fn an_empty_source_passes_vacuously_and_says_so() {
        let r = check(&[], &BTreeMap::new());
        assert!(r.passes());
        assert_eq!(r.total, 0);
        assert_eq!(r.summary(), "0/0 source objects accounted for.");
    }
}
