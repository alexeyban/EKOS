//! RFC 0158 — inferred foreign keys, seeded by real join predicates.
//!
//! This is the part no competing migration tool can do, and the reason is not cleverness: EKOS has
//! already compiled the application code, the views and the ETL into a Transformation IR. Every
//! `JOIN … ON a.x = b.y` anywhere in that estate is a *claim by a developer* that two columns
//! reference each other — a claim nobody wrote into the schema.
//!
//! An undeclared relationship is invisible to a schema-only migration, and it matters twice over:
//! it orders the load (a child cannot land before its parent), and the targets enforce nothing, so
//! whatever integrity the source was relying on the application to maintain is now maintained by
//! nothing at all.
//!
//! **A candidate is a hypothesis until it is measured.** Code joins say where to look; an inclusion
//! check says whether the relationship is real. This module produces the candidates and the SQL;
//! the caller runs it, because this crate deliberately has no database.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One `JOIN … ON left = right` observed somewhere in the compiled estate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JoinObservation {
    pub left_table: String,
    pub left_column: String,
    pub right_table: String,
    pub right_column: String,
    /// Where it was seen — a file path, a view name. Carried so a finding can cite it rather than
    /// asserting a relationship from nowhere.
    pub source: String,
}

/// A `(table, column)` pair, lowercased.
type ColumnKey = (String, String);
/// An unordered pair of columns — the identity of a candidate, before direction is decided.
type PairKey = (ColumnKey, ColumnKey);

impl JoinObservation {
    /// The pair, ordered so that `(a, b)` and `(b, a)` collapse into one candidate. Direction is a
    /// separate question, decided by evidence rather than by which side someone typed first.
    fn key(&self) -> PairKey {
        let l = (
            self.left_table.to_lowercase(),
            self.left_column.to_lowercase(),
        );
        let r = (
            self.right_table.to_lowercase(),
            self.right_column.to_lowercase(),
        );
        if l <= r { (l, r) } else { (r, l) }
    }
}

/// Which way a candidate relationship points, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// One side's column is a declared primary key or unique constraint. The strongest signal
    /// available without touching the data.
    ByConstraint,
    /// Neither side is keyed; the direction was chosen by measuring both and taking the one with
    /// fewer orphans. Recorded as such, because a measured direction is a weaker claim.
    ByMeasurement,
    /// Both directions are equally plausible. Reported rather than guessed.
    Ambiguous,
}

/// A hypothesised foreign key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FkCandidate {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
    pub parent_column: String,
    pub direction: Direction,
    /// How many distinct places in the estate join these columns. A pair joined in eleven files is
    /// a stronger claim than one joined once, and neither is proof.
    pub observations: usize,
    /// The distinct sources, for the finding to cite.
    pub sources: Vec<String>,
}

impl FkCandidate {
    /// The inclusion check: how many child rows have a value with no matching parent.
    ///
    /// `IS NOT NULL` because a null foreign key is absence, not an orphan — including nulls would
    /// report every optional relationship as broken.
    pub fn orphan_sql(&self) -> String {
        let q = |s: &str| {
            s.split('.')
                .map(|p| format!("\"{}\"", p.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(".")
        };
        let col = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        format!(
            "SELECT count(*) FROM {child} c WHERE c.{ccol} IS NOT NULL \
             AND NOT EXISTS (SELECT 1 FROM {parent} p WHERE p.{pcol} = c.{ccol})",
            child = q(&self.child_table),
            ccol = col(&self.child_column),
            parent = q(&self.parent_table),
            pcol = col(&self.parent_column),
        )
    }

    /// Total non-null child values, the denominator for an inclusion rate.
    pub fn population_sql(&self) -> String {
        let q = |s: &str| {
            s.split('.')
                .map(|p| format!("\"{}\"", p.replace('"', "\"\"")))
                .collect::<Vec<_>>()
                .join(".")
        };
        format!(
            "SELECT count(*) FROM {child} WHERE \"{ccol}\" IS NOT NULL",
            child = q(&self.child_table),
            ccol = self.child_column.replace('"', "\"\""),
        )
    }

    pub fn reversed(&self) -> Self {
        Self {
            child_table: self.parent_table.clone(),
            child_column: self.parent_column.clone(),
            parent_table: self.child_table.clone(),
            parent_column: self.child_column.clone(),
            ..self.clone()
        }
    }
}

/// What a measured candidate turned out to be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InclusionResult {
    pub orphans: i64,
    pub population: i64,
}

impl InclusionResult {
    /// The fraction of child values that do find a parent.
    pub fn inclusion_rate(&self) -> Option<f64> {
        if self.population == 0 {
            // No non-null values at all: the check says nothing, and a rate of 1.0 here would read
            // as a perfect relationship where there is no evidence of any relationship.
            return None;
        }
        Some(1.0 - (self.orphans as f64 / self.population as f64))
    }

    /// The verdict. Thresholds are deliberately wide apart, with a band in the middle that resolves
    /// to "unclear" rather than being forced into one of the two answers.
    pub fn verdict(&self) -> Verdict {
        match self.inclusion_rate() {
            None => Verdict::NoData,
            Some(r) if r >= 1.0 => Verdict::CleanRelationship,
            Some(r) if r >= 0.99 => Verdict::RelationshipWithOrphans,
            Some(r) if r >= 0.5 => Verdict::Unclear,
            Some(_) => Verdict::NotARelationship,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Every child value has a parent. A real, undeclared foreign key.
    CleanRelationship,
    /// Almost every child value has a parent. A real relationship *and* a data-quality finding.
    RelationshipWithOrphans,
    /// Enough matches to be suspicious, not enough to claim. Reported, not concluded.
    Unclear,
    /// The columns join in code but the values do not line up. Probably not a key at all.
    NotARelationship,
    /// Nothing to measure.
    NoData,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CleanRelationship => "clean_relationship",
            Self::RelationshipWithOrphans => "relationship_with_orphans",
            Self::Unclear => "unclear",
            Self::NotARelationship => "not_a_relationship",
            Self::NoData => "no_data",
        }
    }

    /// Whether the migration must account for it: a real relationship orders the load and is lost
    /// on a target that enforces nothing.
    pub fn is_relationship(self) -> bool {
        matches!(
            self,
            Self::CleanRelationship | Self::RelationshipWithOrphans
        )
    }
}

/// Group observations into candidates, dropping pairs the schema already declares.
///
/// `declared` is `(child_table, child_column, parent_table, parent_column)`, lowercased by the
/// caller or not — this normalizes. A declared foreign key needs no inference and would only add
/// noise to the report.
pub fn candidates(
    observations: &[JoinObservation],
    declared: &[(String, String, String, String)],
    keyed_columns: &[(String, String)],
) -> Vec<FkCandidate> {
    let lower = |s: &str| s.to_lowercase();
    let declared_pairs: std::collections::BTreeSet<PairKey> = declared
        .iter()
        .map(|(ct, cc, pt, pc)| {
            let a = (lower(ct), lower(cc));
            let b = (lower(pt), lower(pc));
            if a <= b { (a, b) } else { (b, a) }
        })
        .collect();
    let keyed: std::collections::BTreeSet<ColumnKey> = keyed_columns
        .iter()
        .map(|(t, c)| (lower(t), lower(c)))
        .collect();

    let mut grouped: BTreeMap<PairKey, Vec<String>> = BTreeMap::new();
    for o in observations {
        let key = o.key();
        if declared_pairs.contains(&key) {
            continue;
        }
        // A column joined to itself is not a relationship, it is a tautology in a self-join.
        if key.0 == key.1 {
            continue;
        }
        grouped.entry(key).or_default().push(o.source.clone());
    }

    grouped
        .into_iter()
        .map(|((a, b), mut sources)| {
            sources.sort();
            sources.dedup();
            // The keyed side is the parent: a foreign key points at something unique.
            let (child, parent, direction) = match (keyed.contains(&a), keyed.contains(&b)) {
                (true, false) => (b.clone(), a.clone(), Direction::ByConstraint),
                (false, true) => (a.clone(), b.clone(), Direction::ByConstraint),
                (true, true) => (a.clone(), b.clone(), Direction::Ambiguous),
                (false, false) => (a.clone(), b.clone(), Direction::ByMeasurement),
            };
            FkCandidate {
                child_table: child.0,
                child_column: child.1,
                parent_table: parent.0,
                parent_column: parent.1,
                direction,
                observations: sources.len(),
                sources,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(lt: &str, lc: &str, rt: &str, rc: &str, src: &str) -> JoinObservation {
        JoinObservation {
            left_table: lt.into(),
            left_column: lc.into(),
            right_table: rt.into(),
            right_column: rc.into(),
            source: src.into(),
        }
    }

    #[test]
    fn the_same_join_written_either_way_is_one_candidate() {
        let o = [
            obs("orders", "customer_id", "customers", "id", "a.sql"),
            obs("customers", "id", "orders", "customer_id", "b.sql"),
        ];
        let c = candidates(&o, &[], &[]);
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(
            c[0].observations, 2,
            "both sightings count toward confidence"
        );
    }

    #[test]
    fn a_declared_foreign_key_is_not_re_inferred() {
        let o = [obs("orders", "customer_id", "customers", "id", "a.sql")];
        let declared = [(
            "orders".to_string(),
            "customer_id".to_string(),
            "customers".to_string(),
            "id".to_string(),
        )];
        assert!(candidates(&o, &declared, &[]).is_empty());
        // And the reverse spelling of the declaration also suppresses it.
        let declared_rev = [(
            "customers".to_string(),
            "id".to_string(),
            "orders".to_string(),
            "customer_id".to_string(),
        )];
        assert!(candidates(&o, &declared_rev, &[]).is_empty());
    }

    /// A foreign key points at something unique, so the keyed side is the parent — whichever way
    /// the developer happened to write the join.
    #[test]
    fn the_keyed_side_becomes_the_parent() {
        let o = [obs("orders", "customer_id", "customers", "id", "a.sql")];
        let keyed = [("customers".to_string(), "id".to_string())];
        let c = candidates(&o, &[], &keyed);
        assert_eq!(c[0].child_table, "orders");
        assert_eq!(c[0].parent_table, "customers");
        assert_eq!(c[0].direction, Direction::ByConstraint);
    }

    #[test]
    fn two_keyed_sides_are_ambiguous_rather_than_guessed() {
        let o = [obs("a", "id", "b", "id", "x.sql")];
        let keyed = [
            ("a".to_string(), "id".to_string()),
            ("b".to_string(), "id".to_string()),
        ];
        assert_eq!(
            candidates(&o, &[], &keyed)[0].direction,
            Direction::Ambiguous
        );
    }

    #[test]
    fn a_self_join_on_the_same_column_is_not_a_relationship() {
        let o = [obs("orders", "id", "orders", "id", "a.sql")];
        assert!(candidates(&o, &[], &[]).is_empty());
    }

    /// A parent-child relationship inside one table is real — an employee's manager — and must not
    /// be dropped along with the tautological self-join.
    #[test]
    fn a_self_referencing_relationship_on_different_columns_survives() {
        let o = [obs("employees", "manager_id", "employees", "id", "a.sql")];
        let c = candidates(&o, &[], &[("employees".into(), "id".into())]);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].child_column, "manager_id");
        assert_eq!(c[0].parent_column, "id");
    }

    #[test]
    fn sources_are_deduplicated_so_confidence_counts_places_not_sightings() {
        let o = [
            obs("orders", "customer_id", "customers", "id", "a.sql"),
            obs("orders", "customer_id", "customers", "id", "a.sql"),
            obs("orders", "customer_id", "customers", "id", "b.sql"),
        ];
        let c = candidates(&o, &[], &[]);
        assert_eq!(
            c[0].observations, 2,
            "three sightings in two files is two places"
        );
        assert_eq!(c[0].sources, vec!["a.sql", "b.sql"]);
    }

    // ── verdicts ─────────────────────────────────────────────────────────────

    #[test]
    fn a_perfect_inclusion_is_a_clean_relationship() {
        let r = InclusionResult {
            orphans: 0,
            population: 1000,
        };
        assert_eq!(r.verdict(), Verdict::CleanRelationship);
        assert!(r.verdict().is_relationship());
    }

    #[test]
    fn a_handful_of_orphans_is_still_a_relationship_and_also_a_finding() {
        let r = InclusionResult {
            orphans: 3,
            population: 10_000,
        };
        assert_eq!(r.verdict(), Verdict::RelationshipWithOrphans);
        assert!(r.verdict().is_relationship());
    }

    #[test]
    fn values_that_do_not_line_up_are_not_claimed_as_a_key() {
        let r = InclusionResult {
            orphans: 900,
            population: 1000,
        };
        assert_eq!(r.verdict(), Verdict::NotARelationship);
        assert!(!r.verdict().is_relationship());
    }

    /// The band in the middle exists so a marginal result is reported as marginal instead of being
    /// forced into one of the two confident answers.
    #[test]
    fn a_marginal_inclusion_rate_is_unclear_not_a_verdict() {
        let r = InclusionResult {
            orphans: 200,
            population: 1000,
        };
        assert_eq!(r.verdict(), Verdict::Unclear);
        assert!(!r.verdict().is_relationship());
    }

    /// An all-null column matches nothing and misses nothing; reporting 100% inclusion there would
    /// manufacture a relationship out of an empty set.
    #[test]
    fn an_empty_population_says_nothing_rather_than_everything() {
        let r = InclusionResult {
            orphans: 0,
            population: 0,
        };
        assert_eq!(r.inclusion_rate(), None);
        assert_eq!(r.verdict(), Verdict::NoData);
        assert!(!r.verdict().is_relationship());
    }

    // ── generated SQL ────────────────────────────────────────────────────────

    #[test]
    fn the_inclusion_check_ignores_nulls_and_quotes_identifiers() {
        let c = FkCandidate {
            child_table: "public.orders".into(),
            child_column: "customer_id".into(),
            parent_table: "public.customers".into(),
            parent_column: "id".into(),
            direction: Direction::ByConstraint,
            observations: 1,
            sources: vec!["a.sql".into()],
        };
        let sql = c.orphan_sql();
        assert!(sql.contains("\"public\".\"orders\""), "{sql}");
        assert!(sql.contains("\"public\".\"customers\""), "{sql}");
        assert!(
            sql.contains("IS NOT NULL"),
            "a null foreign key is absence, not an orphan: {sql}"
        );
        assert!(sql.contains("NOT EXISTS"), "{sql}");
        assert!(c.population_sql().contains("IS NOT NULL"));
    }

    #[test]
    fn reversing_a_candidate_swaps_both_sides() {
        let c = FkCandidate {
            child_table: "a".into(),
            child_column: "x".into(),
            parent_table: "b".into(),
            parent_column: "y".into(),
            direction: Direction::ByMeasurement,
            observations: 1,
            sources: vec![],
        };
        let r = c.reversed();
        assert_eq!(
            (r.child_table, r.child_column),
            ("b".to_string(), "y".to_string())
        );
        assert_eq!(
            (r.parent_table, r.parent_column),
            ("a".to_string(), "x".to_string())
        );
    }
}
