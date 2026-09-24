//! RFC 0156's planted-defect controls, for the defects that live in *this* layer.
//!
//! The rule the whole validator rests on: a tier that cannot catch a planted defect does not get
//! to report green. These are the serialization-layer defects — the ones a wrong canonical form
//! would hide — and each must change the bucket checksum.
//!
//! The complementary assertion matters just as much and is at the bottom: a clean pair of tables
//! produces **zero** differences. A validator that cries wolf is abandoned, and an abandoned
//! validator proves nothing.

use ekos_migrate_validate::{BucketChecksum, Spec, Value, canon, checksum_buckets, row_canonical};
use std::collections::BTreeMap;

const BUCKETS: u32 = 16;

/// A small, realistic table: `(id, name, amount, updated_at, note)`.
fn source_rows() -> Vec<Row> {
    (1..=200)
        .map(|i| Row {
            id: i,
            name: format!("customer-{i}"),
            amount_digits: format!("{}", 1000 + i * 7),
            amount_scale: 2,
            ts_micros: 123_456,
            note: if i % 5 == 0 {
                None
            } else {
                Some(format!("n{i}"))
            },
        })
        .collect()
}

#[derive(Clone)]
struct Row {
    id: i64,
    name: String,
    amount_digits: String,
    amount_scale: u32,
    ts_micros: u32,
    note: Option<String>,
}

impl Row {
    fn canonical(&self, spec: Spec) -> (String, String) {
        let ts = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_micro_opt(0, 0, 0, self.ts_micros)
            .unwrap();
        let cols = vec![
            canon(&Value::Int(self.id), &Spec::default()).unwrap(),
            canon(&Value::Text(self.name.clone()), &Spec::default()).unwrap(),
            canon(
                &Value::Decimal {
                    digits: self.amount_digits.clone(),
                    scale: self.amount_scale,
                },
                &spec,
            )
            .unwrap(),
            canon(&Value::TimestampUtc(ts), &Spec::default()).unwrap(),
            canon(
                &match &self.note {
                    Some(n) => Value::Text(n.clone()),
                    None => Value::Null,
                },
                &Spec::default(),
            )
            .unwrap(),
        ];
        let pk = canon(&Value::Int(self.id), &Spec::default()).unwrap();
        (pk, row_canonical(&cols))
    }
}

fn checksums(rows: &[Row], spec: Spec) -> BTreeMap<u32, BucketChecksum> {
    let canonical: Vec<(String, String)> = rows.iter().map(|r| r.canonical(spec)).collect();
    checksum_buckets(
        canonical.iter().map(|(p, r)| (p.as_str(), r.as_str())),
        BUCKETS,
    )
}

fn spec() -> Spec {
    Spec {
        decimal_scale: Some(2),
    }
}

/// How many buckets differ between two sides. Zero means the tables agree.
fn differing_buckets(
    a: &BTreeMap<u32, BucketChecksum>,
    b: &BTreeMap<u32, BucketChecksum>,
) -> usize {
    let mut keys: Vec<u32> = a.keys().chain(b.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    keys.iter().filter(|k| a.get(k) != b.get(k)).count()
}

fn assert_caught(name: &str, mutated: &[Row]) {
    let source = checksums(&source_rows(), spec());
    let target = checksums(mutated, spec());
    assert!(
        differing_buckets(&source, &target) > 0,
        "planted defect '{name}' was NOT caught — the tier must report failed, not green"
    );
}

// ── the controls ─────────────────────────────────────────────────────────────

#[test]
fn control_dropped_row() {
    let mut rows = source_rows();
    rows.remove(100);
    assert_caught("dropped row", &rows);
}

#[test]
fn control_duplicated_row() {
    let mut rows = source_rows();
    rows.push(rows[7].clone());
    assert_caught("duplicated row", &rows);
}

#[test]
fn control_truncated_decimal_scale() {
    // The target stored 10.07 as 10.0 — one fractional digit silently gone.
    let mut rows = source_rows();
    rows[3].amount_digits = format!(
        "{}0",
        &rows[3].amount_digits[..rows[3].amount_digits.len() - 2]
    );
    assert_caught("truncated decimal", &rows);
}

#[test]
fn control_microsecond_precision_loss() {
    let mut rows = source_rows();
    for r in &mut rows {
        r.ts_micros = 0; // µs truncated to whole seconds
    }
    assert_caught("microsecond precision loss", &rows);
}

#[test]
fn control_null_became_empty_string() {
    let mut rows = source_rows();
    for r in &mut rows {
        if r.note.is_none() {
            r.note = Some(String::new());
        }
    }
    assert_caught("NULL -> empty string", &rows);
}

#[test]
fn control_trailing_space_trimmed() {
    let mut rows = source_rows();
    for r in &mut rows {
        r.name.push_str("  ");
    }
    let padded = checksums(&rows, spec());
    let trimmed = checksums(&source_rows(), spec());
    assert!(
        differing_buckets(&padded, &trimmed) > 0,
        "trailing-space trimming was not caught"
    );
}

/// The control V1 and V2 cannot see: two same-typed columns swapped. Row count is identical, and
/// every per-column aggregate is identical because the multiset of values per column is unchanged.
/// Only a row-level hash catches it — which is why V3 exists.
#[test]
fn control_two_same_typed_columns_swapped() {
    let mut rows = source_rows();
    for r in &mut rows {
        if let Some(note) = r.note.take() {
            let name = std::mem::replace(&mut r.name, note);
            r.note = Some(name);
        }
    }
    assert_caught("swapped same-typed columns", &rows);
}

/// One wrong byte in one row out of two hundred. The defect a sample would miss.
#[test]
fn control_single_row_single_byte_corruption() {
    let mut rows = source_rows();
    rows[137].name = "customer-137 ".into();
    let source = checksums(&source_rows(), spec());
    let target = checksums(&rows, spec());
    assert_eq!(
        differing_buckets(&source, &target),
        1,
        "exactly one bucket should differ, so bisect has somewhere specific to go"
    );
}

#[test]
fn control_off_by_one_chunk_boundary() {
    // A chunk loaded (lo, hi] instead of [lo, hi): the first row of the range is missing.
    let rows: Vec<Row> = source_rows().into_iter().skip(1).collect();
    assert_caught("off-by-one chunk boundary", &rows);
}

// ── the complement: clean runs must be silent ────────────────────────────────

#[test]
fn a_clean_migration_produces_zero_differences() {
    let source = checksums(&source_rows(), spec());
    let target = checksums(&source_rows(), spec());
    assert_eq!(
        differing_buckets(&source, &target),
        0,
        "a clean run must produce no divergences at all"
    );
    assert_eq!(source.values().map(|c| c.count).sum::<u64>(), 200);
}

/// Row order differs between engines constantly — a target scan order is never a source scan
/// order. It must never register as a divergence.
#[test]
fn row_order_is_not_a_divergence() {
    let forward = checksums(&source_rows(), spec());
    let mut reversed = source_rows();
    reversed.reverse();
    assert_eq!(
        differing_buckets(&forward, &checksums(&reversed, spec())),
        0
    );
}

/// Every control above is listed here. A control added to the suite without an entry, or an entry
/// without a control, fails — so "we added a control nothing catches" stays visible.
#[test]
fn the_control_catalogue_is_complete() {
    const CONTROLS: &[&str] = &[
        "control_dropped_row",
        "control_duplicated_row",
        "control_truncated_decimal_scale",
        "control_microsecond_precision_loss",
        "control_null_became_empty_string",
        "control_trailing_space_trimmed",
        "control_two_same_typed_columns_swapped",
        "control_single_row_single_byte_corruption",
        "control_off_by_one_chunk_boundary",
    ];
    let src = include_str!("controls.rs");
    for name in CONTROLS {
        assert!(
            src.contains(&format!("fn {name}(")),
            "catalogued control '{name}' has no test"
        );
    }
    // Count only definitions at column zero. `src.matches("fn control_")` would also match this
    // very line — a filter comparing itself against itself, which is a mistake this project has
    // shipped before.
    let defined = src.lines().filter(|l| l.starts_with("fn control_")).count();
    assert_eq!(
        defined,
        CONTROLS.len(),
        "a control test exists that is not in the catalogue"
    );
}
