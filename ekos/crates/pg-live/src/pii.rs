//! RFC 0157 — PII classification, and what it suppresses.
//!
//! The rule this module exists to enforce: **hashing is not anonymization.** On a low-cardinality
//! column (`status`, `country`, `gender`, a first name) an attacker holding the hash and the domain
//! recovers every value by enumeration in microseconds. Storing "hashed top-k" for such a column is
//! a leak with a reassuring name on it.
//!
//! So a column classified as PII has **no top-k persisted at all** — not values, not hashes — and no
//! textual min/max either, because a min or a max *is* a value from the table.
//!
//! Classification is applied conservatively and immediately: an *unconfirmed* classification
//! suppresses straight away, and downgrading it is a human action. The failure directions are not
//! symmetric. Over-suppressing costs a little insight; under-suppressing writes personal data into
//! an append-only ledger that has no delete.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PiiClass {
    Email,
    Phone,
    NationalId,
    PaymentCard,
    BankAccount,
    PersonName,
    PostalAddress,
    DateOfBirth,
    IpAddress,
    Credential,
    /// Named like personal data but not matched to a specific class. Suppresses exactly the same:
    /// "we are not sure what kind" is not a reason to publish it.
    Unspecified,
}

/// How a classification was reached — recorded on the fact, because a name heuristic and a measured
/// pattern deserve different amounts of trust from a human reviewing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    ColumnName,
    ValuePattern,
    /// Both agreed. The only combination that is more than a guess.
    NameAndPattern,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Classification {
    pub class: PiiClass,
    pub method: Method,
    /// 0.0–1.0. Deliberately *not* used as a suppression threshold — see [`suppresses_values`].
    pub confidence: f64,
}

impl Classification {
    /// Whether values from this column may be persisted in any form.
    ///
    /// Returns `true` for every classification, whatever the confidence. There is deliberately no
    /// threshold: RFC 0060 already established for identity resolution that no confidence cutoff
    /// reliably separates the real cases from the false ones, and here a wrong call in one direction
    /// is unrecoverable because the ledger cannot delete.
    pub fn suppresses_values(&self) -> bool {
        true
    }
}

/// Name fragments that classify a column, longest-match first so `date_of_birth` does not match as
/// a bare `date`.
const NAME_RULES: &[(&str, PiiClass)] = &[
    ("date_of_birth", PiiClass::DateOfBirth),
    ("national_id", PiiClass::NationalId),
    ("social_security", PiiClass::NationalId),
    ("passport", PiiClass::NationalId),
    ("card_number", PiiClass::PaymentCard),
    ("credit_card", PiiClass::PaymentCard),
    ("cardholder", PiiClass::PaymentCard),
    ("account_number", PiiClass::BankAccount),
    ("iban", PiiClass::BankAccount),
    ("sort_code", PiiClass::BankAccount),
    ("first_name", PiiClass::PersonName),
    ("last_name", PiiClass::PersonName),
    ("surname", PiiClass::PersonName),
    ("full_name", PiiClass::PersonName),
    ("maiden", PiiClass::PersonName),
    ("postcode", PiiClass::PostalAddress),
    ("postal_code", PiiClass::PostalAddress),
    ("zip_code", PiiClass::PostalAddress),
    ("street", PiiClass::PostalAddress),
    ("address", PiiClass::PostalAddress),
    ("password", PiiClass::Credential),
    ("passwd", PiiClass::Credential),
    ("secret", PiiClass::Credential),
    ("api_key", PiiClass::Credential),
    ("token", PiiClass::Credential),
    ("email", PiiClass::Email),
    ("phone", PiiClass::Phone),
    ("mobile", PiiClass::Phone),
    ("telephone", PiiClass::Phone),
    ("ssn", PiiClass::NationalId),
    ("dob", PiiClass::DateOfBirth),
    ("birth", PiiClass::DateOfBirth),
    ("ip_addr", PiiClass::IpAddress),
];

/// Classify by column name alone.
pub fn classify_name(column: &str) -> Option<Classification> {
    let lower = column.to_ascii_lowercase();
    NAME_RULES
        .iter()
        .find(|(frag, _)| lower.contains(frag))
        .map(|(_, class)| Classification {
            class: *class,
            method: Method::ColumnName,
            confidence: 0.6,
        })
}

/// Classify by what sampled values look like.
///
/// The samples are read in memory and dropped; only the verdict survives. Nothing here returns a
/// value, on purpose — a "matched example" field would be a leak with a helpful name.
pub fn classify_values(samples: &[String]) -> Option<Classification> {
    let considered: Vec<&String> = samples.iter().filter(|s| !s.is_empty()).collect();
    if considered.len() < 5 {
        // Too few to say anything. Silence beats a guess that later reads as evidence.
        return None;
    }
    let matched = |f: fn(&str) -> bool| {
        considered.iter().filter(|s| f(s)).count() as f64 / considered.len() as f64
    };

    for (class, pred) in [
        (PiiClass::Email, looks_like_email as fn(&str) -> bool),
        (PiiClass::PaymentCard, looks_like_card),
        (PiiClass::Phone, looks_like_phone),
    ] {
        let rate = matched(pred);
        if rate >= 0.8 {
            return Some(Classification {
                class,
                method: Method::ValuePattern,
                confidence: rate,
            });
        }
    }
    None
}

/// Combine both signals. Agreement raises confidence; either alone still suppresses.
pub fn classify(column: &str, samples: &[String]) -> Option<Classification> {
    match (classify_name(column), classify_values(samples)) {
        (Some(n), Some(v)) if n.class == v.class => Some(Classification {
            class: n.class,
            method: Method::NameAndPattern,
            confidence: (n.confidence + v.confidence).min(2.0) / 2.0 + 0.3,
        }),
        // Disagreement is not a tie-break: take the measured one, since a name is a convention and a
        // pattern is evidence, and record it as such.
        (Some(_), Some(v)) => Some(v),
        (Some(n), None) => Some(n),
        (None, Some(v)) => Some(v),
        (None, None) => None,
    }
}

fn looks_like_email(s: &str) -> bool {
    match s.split_once('@') {
        Some((user, domain)) => {
            !user.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        }
        None => false,
    }
}

fn looks_like_card(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    if s.chars()
        .any(|c| !c.is_ascii_digit() && !matches!(c, ' ' | '-'))
    {
        return false;
    }
    // Luhn. Length alone matches too many order ids to be useful on its own.
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn looks_like_phone(s: &str) -> bool {
    let digits = s.chars().filter(char::is_ascii_digit).count();
    let allowed = s
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')' | '.'));
    allowed && (8..=15).contains(&digits) && !looks_like_card(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longest_name_match_wins() {
        assert_eq!(
            classify_name("date_of_birth").unwrap().class,
            PiiClass::DateOfBirth
        );
        assert_eq!(
            classify_name("customer_email").unwrap().class,
            PiiClass::Email
        );
        assert_eq!(
            classify_name("api_key").unwrap().class,
            PiiClass::Credential
        );
        assert!(classify_name("order_total").is_none());
        assert!(classify_name("id").is_none());
    }

    #[test]
    fn name_matching_is_case_insensitive() {
        assert!(classify_name("EMAIL").is_some());
        assert!(classify_name("CustomerEmail").is_some());
    }

    #[test]
    fn every_classification_suppresses_regardless_of_confidence() {
        for confidence in [0.01, 0.5, 0.99] {
            let c = Classification {
                class: PiiClass::Unspecified,
                method: Method::ColumnName,
                confidence,
            };
            assert!(
                c.suppresses_values(),
                "a confidence threshold would put personal data in a ledger that cannot delete"
            );
        }
    }

    #[test]
    fn too_few_samples_says_nothing() {
        assert!(classify_values(&["a@b.com".into(), "c@d.com".into()]).is_none());
    }

    #[test]
    fn emails_are_recognised_and_near_misses_are_not() {
        let emails: Vec<String> = (0..10).map(|i| format!("user{i}@example.com")).collect();
        assert_eq!(classify_values(&emails).unwrap().class, PiiClass::Email);

        let not: Vec<String> = (0..10).map(|i| format!("order-{i}")).collect();
        assert!(classify_values(&not).is_none());
        // A bare '@' is not an address.
        let partial: Vec<String> = (0..10).map(|i| format!("user{i}@localhost")).collect();
        assert!(classify_values(&partial).is_none(), "no dot in the domain");
    }

    #[test]
    fn card_detection_uses_luhn_not_just_length() {
        let valid: Vec<String> =
            std::iter::repeat_n("4242 4242 4242 4242".to_string(), 10).collect();
        assert_eq!(
            classify_values(&valid).unwrap().class,
            PiiClass::PaymentCard
        );
        // A 16-digit order id that fails Luhn must not be classified as a card.
        let ids: Vec<String> = (0..10).map(|i| format!("123456789012345{i}")).collect();
        let c = classify_values(&ids);
        assert!(
            c.is_none() || c.unwrap().class != PiiClass::PaymentCard,
            "length alone matches too many order ids"
        );
    }

    #[test]
    fn a_pattern_outranks_a_disagreeing_name() {
        // A column called `phone` that actually holds emails is classified by what is in it.
        let emails: Vec<String> = (0..10).map(|i| format!("user{i}@example.com")).collect();
        let c = classify("phone", &emails).unwrap();
        assert_eq!(c.class, PiiClass::Email);
        assert_eq!(c.method, Method::ValuePattern);
    }

    #[test]
    fn agreement_raises_confidence_above_either_signal() {
        let emails: Vec<String> = (0..10).map(|i| format!("user{i}@example.com")).collect();
        let both = classify("customer_email", &emails).unwrap();
        assert_eq!(both.method, Method::NameAndPattern);
        assert!(both.confidence > classify_name("customer_email").unwrap().confidence);
    }
}
