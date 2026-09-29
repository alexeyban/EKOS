//! RFC 0155 — the golden table.
//!
//! **What these literals are.** Each row is a value, its canonical form and its SHA-256, frozen as a
//! committed literal. They were generated once from the implementation, reviewed, and pinned. From
//! here on they are a ratchet: any change to a canonical-form rule breaks this test loudly, which
//! is the point — the rules are a wire format shared with two other engines and must not drift by
//! accident.
//!
//! **What they are not, yet.** RFC 0155's acceptance criterion is *three-way* agreement:
//! PostgreSQL, ClickHouse and this implementation all matching the literal. Only the third leg is
//! asserted here, because nothing in the workspace can talk to a live PostgreSQL until RFC 0157
//! adds a driver. The engine legs are staged: `sql_snapshot.rs` pins the generated SQL, and the
//! same fixture table drives both, so when the driver lands the remaining assertion is a loop over
//! `cases()`, not a new fixture set.
//!
//! Regenerate with:
//!   `cargo test -p ekos-migrate-validate --test print_golden -- --ignored --nocapture`

use ekos_migrate_validate::{canon, fixtures, row_hash};

/// `(case name, canonical form, SHA-256 of the canonical form)`.
const GOLDEN: &[(&str, &str, &str)] = &[
    (
        "null",
        "\\N",
        "b582d3167c70c098fdfefc2c847a1aae83150febc4602a51ef83a84a175d417c",
    ),
    (
        "empty_string",
        "",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        "literal_backslash_n",
        "\\\\N",
        "157b839cf4bc1b970e9b9aa312bd69dea5d300f92a99fa7edc59f4748cee4aa6",
    ),
    (
        "double_backslash",
        "\\\\\\\\",
        "2798eabd0a6839bf35c3b03c0665df219b4b480ad189625d2c9e3a83b7c16e41",
    ),
    (
        "embedded_unit_separator",
        "a\\x1fb",
        "c8acba88ec8e1b78e53573414748787c67b8b0fe64b4e0f215c3adc00e3a0268",
    ),
    (
        "non_ascii",
        "Ünïcödé",
        "39af95d07d82b5d68b6639fea9557192025b64fcc79d700c4cce10f94c16bfc8",
    ),
    (
        "emoji",
        "🦀",
        "7224c588fa9887541bea6fc37a50363ce1c229547ba65c110095ad23b68c902d",
    ),
    (
        "newline",
        "a\nb",
        "7e18f737311b2dc3b2f269dd78396b0351f14fb66efa879f768cb23181883c78",
    ),
    (
        "char_trailing_spaces",
        "ab   ",
        "dc0df8fe616c60a0d232f4db0466012e2bbcb821a1eeb028a3dd13990942d391",
    ),
    (
        "bool_true",
        "t",
        "e3b98a4da31a127d4bde6e43033f66ba274cab0eb7eb1c70ec41402bf6273dd8",
    ),
    (
        "bool_false",
        "f",
        "252f10c83610ebca1a059c0bae8255eba2f95be4d1d7bcfa89d7248a82d9f111",
    ),
    (
        "int_zero",
        "0",
        "5feceb66ffc86f38d952786c6d696c79c2dbc239dd4e91b46729d73a27fb57e9",
    ),
    (
        "int_negative",
        "-42",
        "fec80006df0542549b4cbaafb8987eee00bb49bca396eefe9ac8be5b5928e8f6",
    ),
    (
        "int_max",
        "9223372036854775807",
        "b34a1c30a715f6bf8b7243afa7fab883ce3612b7231716bdcbbdc1982e1aed29",
    ),
    (
        "decimal_trailing_zeros",
        "1.50",
        "1a60b208ff491c3e2d21cdd5abb003e51e97b072efec59098863da45021de6a9",
    ),
    (
        "decimal_widened",
        "1.00",
        "cf9dcf6da8a82be1335c398a4005def7ee3a53d4698c59dbc6b2b14e72d1263c",
    ),
    (
        "decimal_leading_zero",
        "0.05",
        "0602d7c813c1f6e7351a5832730f29daf739674976caba8d9dd955be838d46ea",
    ),
    (
        "decimal_negative",
        "-1.50",
        "31041bdd9d5a80b01cd6518420ae2ca82ad57eb4ff61faee20e5bf8453adad6f",
    ),
    (
        "decimal_negative_zero",
        "0.00",
        "561b2814d3c09e62a92442c946307918f7f63f833c84876c08bd4c406767e53b",
    ),
    (
        "decimal_large",
        "1234567890123456789012.34",
        "4e5d2fe40aa1b6ce1d2104bebf49ee927beaf56c55822eaa2c5893c3ed920fc0",
    ),
    (
        "timestamp_utc_whole_second",
        "2026-09-24T12:30:00.000000",
        "b5d565f93169782cb5d14c3a3210ee3ebd76e789927174959afe96b92b880745",
    ),
    (
        "timestamp_utc_microseconds",
        "2026-09-24T12:30:00.123456",
        "d5b8d553348523f6f7c54f90f3de57e407c6aaa75f27baa327f2baa7f9cd4a88",
    ),
    (
        "timestamp_naive",
        "1999-12-31T23:59:59.999999",
        "66cf13f18258f163a94ca33f7dcc10cfcc8e0de18a17f886191590c9a3083615",
    ),
    (
        "date_pre_1900",
        "1850-06-15",
        "252997a8a42be3c7092b59942b6bcd92eb83e6efaf96bf16664ca8a24aa4491a",
    ),
    (
        "date_epoch",
        "1970-01-01",
        "85c14296d9598554eeb207f773a614a81cdefaecbf35a0d7051f27cf07f896b3",
    ),
    (
        "time_microseconds",
        "01:02:03.000004",
        "0069950ca80de359a779a19f5b4305f80c30e7fa10190fcde46d73e756c04284",
    ),
    (
        "interval_months_and_days",
        "1:15:3600000000",
        "d2d7e33fbf1e534c623a3c7a158b5a846663c6baa97ec49ad9e154e5d45c6a5b",
    ),
    (
        "uuid_uppercase_input",
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        "0aca198ee7979a67105e1a1f69ddbff10f82c7d8d2f69a2a41a15d75ac2d1146",
    ),
    (
        "bytes",
        "000fff",
        "789de28b88b5d42611d8b7b56bacc754000502f8e4c60e1c09df13844297d608",
    ),
    (
        "bytes_empty",
        "",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        "inet_v4",
        "192.168.0.1/32",
        "16eb25c72dbb510344e212d8e2341d217e042fbf3752f1aab40dca718856d52f",
    ),
    (
        "inet_v6",
        "2001:db8::1/128",
        "986b737b578f0d4f980356e2dff7a9d3fb8ac45cf5f35c0888ecc3af8fd07d78",
    ),
    (
        "array_with_null_element",
        "{1,\\N,3}",
        "196bc2f8b2f46851532726f364089f570fd2a8cf1ecfbc2dcf2db656558427bb",
    ),
    (
        "array_nested",
        "{{a},{}}",
        "13d340899bc98ea36f6810175a00edd4d421dd913a720cb04ad0ace6a3163fac",
    ),
    (
        "array_empty",
        "{}",
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a",
    ),
];

#[test]
fn every_fixture_matches_its_frozen_canonical_form_and_hash() {
    let cases = fixtures::cases();
    assert_eq!(
        cases.len(),
        GOLDEN.len(),
        "the fixture table and the golden table disagree on how many cases exist — regenerate \
         the literals deliberately, do not delete a row to make this pass"
    );
    for ((name, value, spec), (gname, gcanon, ghash)) in cases.iter().zip(GOLDEN) {
        assert_eq!(name, gname, "fixture order changed");
        let c = canon(value, spec).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(&c, gcanon, "{name}: canonical form drifted");
        assert_eq!(&row_hash(&c), ghash, "{name}: hash drifted");
    }
}

/// The sentinel is only sound while nothing else can render to it. This asserts the property
/// directly rather than trusting the escaping code to be right.
#[test]
fn only_null_renders_as_the_null_sentinel() {
    for (name, value, spec) in fixtures::cases() {
        let c = canon(&value, &spec).unwrap();
        if matches!(value, ekos_migrate_validate::Value::Null) {
            assert_eq!(c, ekos_migrate_validate::NULL_SENTINEL);
        } else {
            assert_ne!(
                c,
                ekos_migrate_validate::NULL_SENTINEL,
                "{name} forged NULL"
            );
        }
    }
}

/// No canonical form may contain a bare unit separator, or a column boundary could be forged from
/// inside a value.
#[test]
fn no_canonical_form_contains_a_bare_separator() {
    for (name, value, spec) in fixtures::cases() {
        let c = canon(&value, &spec).unwrap();
        assert!(
            !c.contains(ekos_migrate_validate::UNIT_SEPARATOR),
            "{name} contains a bare unit separator: {c:?}"
        );
    }
}

/// Distinct *values* must produce distinct canonical forms — within a column, which is the only
/// place it matters. The two collisions below are across types (an empty `text` and an empty
/// `bytea` both render as the empty string), and a column has exactly one type, so they can never
/// occupy the same position in a row. Asserted explicitly so the exception stays deliberate.
#[test]
fn canonical_forms_collide_only_across_types() {
    use std::collections::HashMap;
    let mut seen: HashMap<String, Vec<&str>> = HashMap::new();
    for (name, value, spec) in fixtures::cases() {
        seen.entry(canon(&value, &spec).unwrap())
            .or_default()
            .push(name);
    }
    let collisions: Vec<_> = seen
        .iter()
        .filter(|(_, names)| names.len() > 1)
        .map(|(c, names)| (c.clone(), names.clone()))
        .collect();
    assert_eq!(
        collisions.len(),
        1,
        "unexpected canonical-form collisions: {collisions:?}"
    );
    let (form, mut names) = collisions.into_iter().next().unwrap();
    names.sort();
    assert_eq!(form, "");
    assert_eq!(names, vec!["bytes_empty", "empty_string"]);
}
