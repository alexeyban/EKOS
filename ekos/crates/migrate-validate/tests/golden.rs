//! RFC 0155 — the golden table.
//!
//! **What these literals are.** Each row is a value, its canonical form and its `md5`, frozen as a
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

/// `(case name, canonical form, md5 of the canonical form)`.
const GOLDEN: &[(&str, &str, &str)] = &[
    ("null", "\\N", "44d0dc437936b13f7cea2f77053806bd"),
    ("empty_string", "", "d41d8cd98f00b204e9800998ecf8427e"),
    (
        "literal_backslash_n",
        "\\\\N",
        "3766e5bb8e3dc00c16eb6e390c648637",
    ),
    (
        "double_backslash",
        "\\\\\\\\",
        "c55afac70b5c0b7c79a14ffbb790bc92",
    ),
    (
        "embedded_unit_separator",
        "a\\x1fb",
        "6993fab6dc625cad4c845fc373f3a2df",
    ),
    ("non_ascii", "Ünïcödé", "102ea64e403ab307d9bc065e12acd34e"),
    ("emoji", "🦀", "cce906c42b6c90f0cd516a0b3bae0e3e"),
    ("newline", "a\nb", "8cdeb44417f3c26826595d5820cf5700"),
    (
        "char_trailing_spaces",
        "ab   ",
        "10b156a4e4c9529cbde8c9a58044da30",
    ),
    ("bool_true", "t", "e358efa489f58062f10dd7316b65649e"),
    ("bool_false", "f", "8fa14cdd754f91cc6554c9e71929cce7"),
    ("int_zero", "0", "cfcd208495d565ef66e7dff9f98764da"),
    ("int_negative", "-42", "8dfcb89fd8620e3e7fb6a03a53f307dc"),
    (
        "int_max",
        "9223372036854775807",
        "15767b252275cf5107bba9267b88e787",
    ),
    (
        "decimal_trailing_zeros",
        "1.50",
        "a6acbd7fe3dcc4f4328712278f6da218",
    ),
    (
        "decimal_widened",
        "1.00",
        "41cf2677cc4ec9356dad8e76dfb87448",
    ),
    (
        "decimal_leading_zero",
        "0.05",
        "b14399cbaac6da4b5b733b483106383f",
    ),
    (
        "decimal_negative",
        "-1.50",
        "7947ab338938f244592c66b3ca8601f9",
    ),
    (
        "decimal_negative_zero",
        "0.00",
        "f7ddd489ab0a82567b241b05971cbdb3",
    ),
    (
        "decimal_large",
        "1234567890123456789012.34",
        "01487724ec98ffb7b7ca3cac665f96a4",
    ),
    (
        "timestamp_utc_whole_second",
        "2026-09-24T12:30:00.000000",
        "8fdd6d0367de8c12d24af98af1cca3bf",
    ),
    (
        "timestamp_utc_microseconds",
        "2026-09-24T12:30:00.123456",
        "839a2ed49a7c5ec14ed19e508e0d414a",
    ),
    (
        "timestamp_naive",
        "1999-12-31T23:59:59.999999",
        "28ecb3174c2e87f79190baa5e6b2d3c0",
    ),
    (
        "date_pre_1900",
        "1850-06-15",
        "9d8c37eff1e7e930099501554f720df6",
    ),
    (
        "date_epoch",
        "1970-01-01",
        "0c0fb6b2ee7dbb235035f7f6fdcfe8fb",
    ),
    (
        "time_microseconds",
        "01:02:03.000004",
        "d7a4a68a6c329706001a025bdafbd6ab",
    ),
    (
        "interval_months_and_days",
        "1:15:3600000000",
        "aea3fe881092a3721ed5da86c96a5b3e",
    ),
    (
        "uuid_uppercase_input",
        "a0eebc99-9c0b-4ef8-bb6d-6bb9bd380a11",
        "40c7d28cdd8ddd783e4465008dd0b227",
    ),
    ("bytes", "000fff", "3f91db15a143c1cd6de108b1ed42bac7"),
    ("bytes_empty", "", "d41d8cd98f00b204e9800998ecf8427e"),
    (
        "inet_v4",
        "192.168.0.1/32",
        "207bc994c99409222d645f0dce5bf9fe",
    ),
    (
        "inet_v6",
        "2001:db8::1/128",
        "f946435677c14b6e1d0ad3e6abb028f6",
    ),
    (
        "array_with_null_element",
        "{1,\\N,3}",
        "48a2fdf432a6643120072829392a5215",
    ),
    (
        "array_nested",
        "{{a},{}}",
        "0791bd538c529182f34e7ff86d70ec4a",
    ),
    ("array_empty", "{}", "99914b932bd37a50b983c5e7c90ae93b"),
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
