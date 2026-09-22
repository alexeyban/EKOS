//! Treasury payment ↔ governance approval matching (RFC 0032).
//!
//! The same *shape* as [`crate::cross_system`] (RFC 0029): two independently observed records that
//! plausibly describe one real-world fact, with no direct link between them, scored on several
//! signals and returned as **candidates only**. The caller persists each as an `unconfirmed`
//! `Custom("AuthorizedBy")` relationship (`ekos treasury scan`); nothing here — or anywhere in the
//! compiler — ever decides that a payment *was* authorized. A wrongly auto-confirmed match is a
//! false claim that real money was approved, which is why this is a separate function with its own
//! signal set rather than a parameterisation of `find_cross_system_candidates`.
//!
//! ## Property contract
//!
//! Written by `TreasuryAnalyzerPass` / `GovernanceAnalyzerPass` (`ekos-recovery`):
//!
//! | `Custom("TreasuryPayment")` | |
//! |---|---|
//! | `tx_hash`, `from`, `to` | strings (addresses compared case-insensitively) |
//! | `value` | decimal string in whole units (`"50000"`, `"1.5"`), never a float |
//! | `token` | symbol string, absent/null for the chain's native asset |
//! | `memo` | decoded input-data text, when present |
//! | `timestamp` | RFC 3339 |
//!
//! | `Custom("GovernanceProposal")` | |
//! |---|---|
//! | `proposal_id`, `title` | strings |
//! | `outcome` | `"approved"` / `"rejected"` / `"unknown"` |
//! | `approved_at` | RFC 3339, present only when `outcome == "approved"` |
//! | `approved_recipient`, `approved_amount`, `approved_token` | optional — many real proposals are prose |
//! | `tx_hashes_mentioned` | every 32-byte hex hash the proposal text cites |
//!
//! ## Scoring
//!
//! Four signals, each `Option`: an unavailable signal is **excluded** from the weighted average,
//! never scored as zero (RFC 0029's "degrades gracefully" rule). Weights `{recipient 0.35, amount
//! 0.25, text_reference 0.30, temporal 0.10}` are renormalised over the signals that are present.
//!
//! Two deliberate departures from RFC 0029's purely additive scoring:
//! 1. A payment made **before** its proposal's approval is a strong *negative* signal, not neutral:
//!    the score is multiplied by [`BEFORE_APPROVAL_PENALTY`] and `before_approval` is set. The
//!    floor is applied to the score *before* that penalty, so such a pair is still surfaced —
//!    flagged and low-confidence — rather than dropped.
//! 2. A pair needs at least one *identifying* signal (recipient, amount or text reference).
//!    Temporal proximity alone must never make every payment a candidate for every proposal.
//!
//! Proposals whose recorded outcome is `"rejected"` are never candidates: linking a payment to a
//! rejected vote as if it were an approval would be misleading.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use ekos_kir::{KirId, KirObject, ObjectKind};
use serde::{Deserialize, Serialize};

/// Below this a pair is not written at all. RFC 0029's own floor, reused as a starting point until
/// there is real DAO data to tune against (RFC 0032 Open Question 3).
pub const MIN_APPROVAL_CONFIDENCE: f32 = 0.3;
/// A payment may follow its approval by at most this long before the temporal signal stops counting.
pub const APPROVAL_WINDOW_DAYS: i64 = 90;
/// Relative tolerance for an "exact" amount match (gas deduction, rounding).
pub const AMOUNT_TOLERANCE: f64 = 0.02;
/// Multiplier applied when the payment predates the proposal's approval.
pub const BEFORE_APPROVAL_PENALTY: f32 = 0.25;

const W_RECIPIENT: f32 = 0.35;
const W_AMOUNT: f32 = 0.25;
const W_TEXT: f32 = 0.30;
const W_TEMPORAL: f32 = 0.10;

pub const PAYMENT_KIND: &str = "TreasuryPayment";
pub const PROPOSAL_KIND: &str = "GovernanceProposal";

/// Per-signal breakdown carried onto the persisted relationship so a reviewer sees *why*.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalSignals {
    pub recipient_match: Option<f32>,
    pub amount_match: Option<f32>,
    pub text_reference: Option<f32>,
    pub temporal: Option<f32>,
    /// `true` when the payment's timestamp is earlier than the proposal's approval.
    pub before_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalCandidate {
    pub payment: KirId,
    pub proposal: KirId,
    pub confidence: f32,
    pub signals: ApprovalSignals,
}

fn is_kind(obj: &KirObject, name: &str) -> bool {
    matches!(&obj.kind, ObjectKind::Custom(k) if k == name)
}

pub fn is_payment(obj: &KirObject) -> bool {
    is_kind(obj, PAYMENT_KIND)
}

pub fn is_proposal(obj: &KirObject) -> bool {
    is_kind(obj, PROPOSAL_KIND)
}

fn str_prop<'a>(obj: &'a KirObject, key: &str) -> Option<&'a str> {
    obj.properties
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

fn time_prop(obj: &KirObject, key: &str) -> Option<DateTime<Utc>> {
    str_prop(obj, key)
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn amount_prop(obj: &KirObject, key: &str) -> Option<f64> {
    str_prop(obj, key).and_then(|s| s.replace(',', "").parse::<f64>().ok())
}

fn recipient_signal(payment: &KirObject, proposal: &KirObject) -> Option<f32> {
    let approved = str_prop(proposal, "approved_recipient")?;
    let to = str_prop(payment, "to")?;
    Some(if approved.eq_ignore_ascii_case(to) {
        1.0
    } else {
        0.0
    })
}

fn amount_signal(payment: &KirObject, proposal: &KirObject) -> Option<f32> {
    let approved = amount_prop(proposal, "approved_amount")?;
    let paid = amount_prop(payment, "value")?;
    // A different asset can never be the approved amount, whatever the numbers say.
    if let (Some(pt), Some(at)) = (
        str_prop(payment, "token"),
        str_prop(proposal, "approved_token"),
    ) && !pt.eq_ignore_ascii_case(at)
    {
        return Some(0.0);
    }
    if approved <= 0.0 {
        return None;
    }
    let ratio = paid / approved;
    Some(if (ratio - 1.0).abs() <= AMOUNT_TOLERANCE {
        1.0
    } else if ratio < 1.0 {
        0.25 // consistent with a partial tranche of a larger approved total, but weak
    } else {
        0.0 // paid more than was approved
    })
}

fn text_signal(payment: &KirObject, proposal: &KirObject) -> Option<f32> {
    // Either direction counts: the proposal cites the transaction hash, or the payment's memo
    // cites the proposal.
    let tx_hash = str_prop(payment, "tx_hash");
    let cited_by_proposal = tx_hash.is_some_and(|h| {
        proposal
            .properties
            .get("tx_hashes_mentioned")
            .and_then(|v| v.as_array())
            .is_some_and(|hs| {
                hs.iter()
                    .filter_map(|v| v.as_str())
                    .any(|m| m.eq_ignore_ascii_case(h))
            })
    });
    let memo = str_prop(payment, "memo");
    let cites_proposal = memo
        .zip(str_prop(proposal, "proposal_id"))
        .is_some_and(|(m, id)| m.to_ascii_lowercase().contains(&id.to_ascii_lowercase()));
    if cited_by_proposal || cites_proposal {
        return Some(1.0);
    }
    // Checked and found nothing → 0.0; nothing to check with → unavailable.
    memo.map(|_| 0.0)
}

/// `(signal, before_approval)`.
fn temporal_signal(payment: &KirObject, proposal: &KirObject) -> (Option<f32>, bool) {
    let (Some(paid_at), Some(approved_at)) = (
        time_prop(payment, "timestamp"),
        time_prop(proposal, "approved_at"),
    ) else {
        return (None, false);
    };
    if paid_at < approved_at {
        return (Some(0.0), true);
    }
    let within = paid_at - approved_at <= Duration::days(APPROVAL_WINDOW_DAYS);
    (Some(if within { 1.0 } else { 0.0 }), false)
}

/// Score one pair; `None` when it is not a candidate (no identifying signal, rejected proposal, or
/// below [`MIN_APPROVAL_CONFIDENCE`]).
pub fn score_pair(payment: &KirObject, proposal: &KirObject) -> Option<ApprovalCandidate> {
    if str_prop(proposal, "outcome") == Some("rejected") {
        return None;
    }
    let recipient = recipient_signal(payment, proposal);
    let amount = amount_signal(payment, proposal);
    let text = text_signal(payment, proposal);
    let (temporal, before_approval) = temporal_signal(payment, proposal);

    if recipient.is_none() && amount.is_none() && text.is_none() {
        return None; // temporal proximity alone identifies nothing
    }

    let parts = [
        (recipient, W_RECIPIENT),
        (amount, W_AMOUNT),
        (text, W_TEXT),
        (temporal, W_TEMPORAL),
    ];
    let (num, den) = parts
        .iter()
        .fold((0.0f32, 0.0f32), |(n, d), (s, w)| match s {
            Some(v) => (n + v * w, d + w),
            None => (n, d),
        });
    let raw = num / den;
    // The floor applies to the *evidence of a match*; the before-approval penalty is applied after
    // it, so a full structured match that was paid early still surfaces — flagged, with a low
    // confidence — instead of silently vanishing below the floor.
    if raw < MIN_APPROVAL_CONFIDENCE {
        return None;
    }
    let confidence = if before_approval {
        raw * BEFORE_APPROVAL_PENALTY
    } else {
        raw
    };
    Some(ApprovalCandidate {
        payment: payment.id,
        proposal: proposal.id,
        confidence,
        signals: ApprovalSignals {
            recipient_match: recipient,
            amount_match: amount,
            text_reference: text,
            temporal,
            before_approval,
        },
    })
}

/// Every payment × proposal pair scoring at or above the floor, best first. Low-confidence
/// candidates are kept on purpose: a reviewer should see "0.35, weak evidence" rather than the
/// system silently deciding.
pub fn find_treasury_approval_candidates(objects: &[KirObject]) -> Vec<ApprovalCandidate> {
    let payments: Vec<&KirObject> = objects.iter().filter(|o| is_payment(o)).collect();
    let proposals: Vec<&KirObject> = objects.iter().filter(|o| is_proposal(o)).collect();
    let mut out: Vec<ApprovalCandidate> = payments
        .iter()
        .flat_map(|pay| proposals.iter().filter_map(|prop| score_pair(pay, prop)))
        .collect();
    out.sort_by(|a, b| {
        b.confidence
            .total_cmp(&a.confidence)
            .then_with(|| a.payment.0.cmp(&b.payment.0))
            .then_with(|| a.proposal.0.cmp(&b.proposal.0))
    });
    out
}

/// Payments with no candidate at or above the floor — the "unapproved spend" watchlist. An
/// observation for a human to judge, never a verdict.
pub fn payments_without_candidate<'a>(
    objects: &'a [KirObject],
    candidates: &[ApprovalCandidate],
) -> Vec<&'a KirObject> {
    let covered: HashSet<KirId> = candidates.iter().map(|c| c.payment).collect();
    objects
        .iter()
        .filter(|o| is_payment(o) && !covered.contains(&o.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const TREASURY_TX_A: &str =
        "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RECIPIENT: &str = "0x1111111111111111111111111111111111111111";

    fn payment(tx: &str, to: &str, value: &str, token: Option<&str>, ts: &str) -> KirObject {
        let mut o = KirObject::new(
            format!("payment {tx}"),
            ObjectKind::Custom(PAYMENT_KIND.into()),
        )
        .with_property("tx_hash", json!(tx))
        .with_property("to", json!(to))
        .with_property("value", json!(value))
        .with_property("timestamp", json!(ts));
        if let Some(t) = token {
            o = o.with_property("token", json!(t));
        }
        o
    }

    fn proposal(id: &str, outcome: &str) -> KirObject {
        KirObject::new(
            format!("proposal {id}"),
            ObjectKind::Custom(PROPOSAL_KIND.into()),
        )
        .with_property("proposal_id", json!(id))
        .with_property("outcome", json!(outcome))
    }

    #[test]
    fn a_full_structured_match_scores_at_the_top() {
        let pay = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-03-10T12:00:00Z",
        );
        let prop = proposal("0xprop1", "approved")
            .with_property(
                "approved_recipient",
                json!(RECIPIENT.to_uppercase().replace("0X", "0x")),
            )
            .with_property("approved_amount", json!("50000"))
            .with_property("approved_token", json!("usdc"))
            .with_property("approved_at", json!("2026-03-01T00:00:00Z"));
        let c = score_pair(&pay, &prop).expect("must be a candidate");
        assert!(c.confidence > 0.95, "got {}", c.confidence);
        assert_eq!(c.signals.recipient_match, Some(1.0));
        assert_eq!(c.signals.amount_match, Some(1.0));
        assert_eq!(c.signals.temporal, Some(1.0));
        assert!(!c.signals.before_approval);
    }

    #[test]
    fn prose_only_proposal_matches_through_the_text_reference_alone() {
        // No structured amount/recipient at all — the honest, common case.
        let pay = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "12345",
            None,
            "2026-03-10T00:00:00Z",
        )
        .with_property("memo", json!("payout per proposal 0xprop2 (marketing)"));
        let prop = proposal("0xprop2", "approved");
        let c = score_pair(&pay, &prop).expect("text reference must suffice");
        assert_eq!(c.signals.text_reference, Some(1.0));
        assert_eq!(c.signals.recipient_match, None, "unavailable, not scored 0");
        assert_eq!(c.signals.amount_match, None);
        assert!(c.confidence >= 0.99);
    }

    #[test]
    fn proposal_citing_the_transaction_hash_counts_too() {
        let pay = payment(TREASURY_TX_A, RECIPIENT, "10", None, "2026-03-10T00:00:00Z");
        let prop = proposal("0xprop3", "approved").with_property(
            "tx_hashes_mentioned",
            json!([TREASURY_TX_A.to_uppercase().replace("0X", "0x")]),
        );
        assert_eq!(
            score_pair(&pay, &prop).unwrap().signals.text_reference,
            Some(1.0)
        );
    }

    #[test]
    fn a_deliberate_non_match_is_below_the_floor() {
        let pay = payment(
            TREASURY_TX_A,
            "0x2222222222222222222222222222222222222222",
            "9000",
            Some("USDC"),
            "2026-03-10T00:00:00Z",
        );
        let prop = proposal("0xprop4", "approved")
            .with_property("approved_recipient", json!(RECIPIENT))
            .with_property("approved_amount", json!("50000"))
            .with_property("approved_token", json!("USDC"))
            .with_property("approved_at", json!("2026-03-01T00:00:00Z"));
        assert!(
            score_pair(&pay, &prop).is_none(),
            "wrong recipient + partial amount must not surface"
        );
    }

    #[test]
    fn a_payment_before_its_approval_is_penalised_not_neutral() {
        let prop = proposal("0xprop5", "approved")
            .with_property("approved_recipient", json!(RECIPIENT))
            .with_property("approved_amount", json!("50000"))
            .with_property("approved_token", json!("USDC"))
            .with_property("approved_at", json!("2026-03-01T00:00:00Z"));
        let after = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-03-02T00:00:00Z",
        );
        let before = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-02-20T00:00:00Z",
        );
        let a = score_pair(&after, &prop).unwrap();
        let b = score_pair(&before, &prop).expect("a full match still surfaces, flagged");
        assert!(b.signals.before_approval);
        assert!(
            b.confidence < a.confidence * 0.5,
            "before={} after={}",
            b.confidence,
            a.confidence
        );
    }

    #[test]
    fn temporal_proximity_alone_never_creates_a_candidate() {
        let pay = payment(TREASURY_TX_A, RECIPIENT, "1", None, "2026-03-02T00:00:00Z");
        let prop = proposal("0xprop6", "approved")
            .with_property("approved_at", json!("2026-03-01T00:00:00Z"));
        assert!(score_pair(&pay, &prop).is_none());
    }

    #[test]
    fn a_rejected_proposal_is_never_offered_as_an_approval() {
        let pay = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-03-02T00:00:00Z",
        );
        let prop = proposal("0xprop7", "rejected")
            .with_property("approved_recipient", json!(RECIPIENT))
            .with_property("approved_amount", json!("50000"));
        assert!(score_pair(&pay, &prop).is_none());
    }

    #[test]
    fn a_different_token_is_never_the_approved_amount() {
        let pay = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("DAI"),
            "2026-03-02T00:00:00Z",
        );
        let prop = proposal("0xprop8", "approved")
            .with_property("approved_amount", json!("50000"))
            .with_property("approved_token", json!("USDC"));
        // amount = 0.0 is the only identifying signal → average 0 → below floor.
        assert!(score_pair(&pay, &prop).is_none());
    }

    #[test]
    fn a_partial_tranche_scores_between_a_full_match_and_a_miss() {
        let prop = proposal("0xprop9", "approved")
            .with_property("approved_amount", json!("100000"))
            .with_property("approved_recipient", json!(RECIPIENT));
        let tranche = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "25000",
            None,
            "2026-03-02T00:00:00Z",
        );
        let c = score_pair(&tranche, &prop).unwrap();
        assert_eq!(c.signals.amount_match, Some(0.25));
    }

    #[test]
    fn candidates_are_ranked_and_the_watchlist_lists_only_unmatched_payments() {
        let matched = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-03-10T00:00:00Z",
        );
        let orphan = payment(
            "0xbbbb",
            "0x3333333333333333333333333333333333333333",
            "777",
            Some("USDC"),
            "2026-03-11T00:00:00Z",
        );
        let prop = proposal("0xp", "approved")
            .with_property("approved_recipient", json!(RECIPIENT))
            .with_property("approved_amount", json!("50000"))
            .with_property("approved_token", json!("USDC"))
            .with_property("approved_at", json!("2026-03-01T00:00:00Z"));
        let all = vec![matched.clone(), orphan.clone(), prop];
        let cands = find_treasury_approval_candidates(&all);
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].payment, matched.id);
        let watch = payments_without_candidate(&all, &cands);
        assert_eq!(watch.len(), 1);
        assert_eq!(watch[0].id, orphan.id);
    }

    #[test]
    fn scoring_is_deterministic() {
        let pay = payment(
            TREASURY_TX_A,
            RECIPIENT,
            "50000",
            Some("USDC"),
            "2026-03-10T00:00:00Z",
        );
        let prop =
            proposal("0xp", "approved").with_property("approved_recipient", json!(RECIPIENT));
        let objs = vec![pay, prop];
        assert_eq!(
            find_treasury_approval_candidates(&objs),
            find_treasury_approval_candidates(&objs)
        );
    }
}
