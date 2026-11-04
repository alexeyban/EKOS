//! Running benchmark SQL against a SQLite database, and the execution-based metrics (RFC 0240
//! §2.5): EX, Soft-F1 and R-VES as `bird-bench/mini_dev`'s evaluator computes them.
//!
//! The official evaluator runs each query through Python's `sqlite3` and compares `fetchall()`
//! results with Python equality, so `1 == 1.0` and `(1,) == (1.0,)`. [`Cell`] keeps that rule:
//! a real with an integral value and an integer are the same cell. Everything else compares by
//! type and value. The connection is read-only and every query runs under a deadline.

use anyhow::{Result, anyhow};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

/// One result cell, normalised so that equal-under-Python means equal here.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Cell {
    Null,
    Int(i64),
    /// A non-integral real, by its bits (`-0.0` is folded into `Int(0)` first).
    Real(u64),
    Text(String),
    Blob(Vec<u8>),
}

impl Cell {
    fn from_ref(v: ValueRef<'_>) -> Cell {
        match v {
            ValueRef::Null => Cell::Null,
            ValueRef::Integer(i) => Cell::Int(i),
            ValueRef::Real(f) => Cell::real(f),
            ValueRef::Text(t) => Cell::Text(String::from_utf8_lossy(t).into_owned()),
            ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
        }
    }

    pub fn real(f: f64) -> Cell {
        if f.fract() == 0.0 && f.is_finite() && f.abs() < 9.0e15 {
            Cell::Int(f as i64)
        } else {
            Cell::Real(f.to_bits())
        }
    }
}

pub type Row = Vec<Cell>;

/// Why a query produced no rows to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecError {
    Timeout,
    TooManyRows,
    Sql(String),
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::Timeout => f.write_str("timeout"),
            ExecError::TooManyRows => f.write_str("result too large"),
            ExecError::Sql(e) => f.write_str(e),
        }
    }
}

/// Limits every query runs under, prediction and gold alike.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub timeout: Duration,
    pub max_rows: usize,
}

impl Default for Limits {
    fn default() -> Self {
        // The official evaluator's `meta_time_out`.
        Self {
            timeout: Duration::from_secs(30),
            max_rows: 2_000_000,
        }
    }
}

/// A read-only connection to one benchmark database.
pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| anyhow!("opening {}: {e}", path.display()))?;
        // No other database file can be reached from a model's SQL.
        conn.set_limit(rusqlite::limits::Limit::SQLITE_LIMIT_ATTACHED, 0);
        register_math_functions(&conn)?;
        Ok(Db { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Run one statement and fetch every row, as `cursor.execute(sql); cursor.fetchall()` does.
    pub fn query(&self, sql: &str, limits: Limits) -> Result<Vec<Row>, ExecError> {
        let deadline = Instant::now() + limits.timeout;
        self.conn
            .progress_handler(10_000, Some(move || Instant::now() > deadline));
        let result = self.query_inner(sql, limits.max_rows, deadline);
        self.conn.progress_handler(0, None::<fn() -> bool>);
        result
    }

    fn query_inner(
        &self,
        sql: &str,
        max_rows: usize,
        deadline: Instant,
    ) -> Result<Vec<Row>, ExecError> {
        let map = |e: rusqlite::Error| {
            if Instant::now() > deadline {
                ExecError::Timeout
            } else {
                ExecError::Sql(e.to_string())
            }
        };
        let mut stmt = self.conn.prepare(sql).map_err(map)?;
        let width = stmt.column_count();
        let mut rows = stmt.query([]).map_err(map)?;
        let mut out = Vec::new();
        while let Some(row) = rows.next().map_err(map)? {
            if out.len() >= max_rows {
                return Err(ExecError::TooManyRows);
            }
            let mut cells = Vec::with_capacity(width);
            for i in 0..width {
                cells.push(Cell::from_ref(row.get_ref(i).map_err(map)?));
            }
            out.push(cells);
        }
        Ok(out)
    }

    /// Wall-clock time of one full execution, for R-VES.
    pub fn time(&self, sql: &str, limits: Limits) -> Result<Duration, ExecError> {
        let start = Instant::now();
        self.query(sql, limits)?;
        Ok(start.elapsed())
    }
}

/// SQLite's built-in math functions (`SQLITE_ENABLE_MATH_FUNCTIONS`, SQLite 3.35+). The official
/// evaluator runs Python's `sqlite3`, whose system library has them; the bundled library here is
/// built without, so a query using `FLOOR` or `POWER` failed here and ran there (BIRD Mini-Dev
/// question 245's gold SQL). Same definitions as SQLite's `func.c`: integer in, integer out for
/// `floor`/`ceil`/`trunc`; a non-numeric argument or a domain error gives NULL; `log(x)` is
/// base 10, computed as `ln(x) × (1 / ln 10)` like SQLite 3.37 (Ubuntu 22.04's Python), so
/// `log10(100)` is 1.9999999999999998 there and here.
type Rounding = fn(f64) -> f64;
type Unary = fn(f64) -> Option<f64>;
type Binary = fn(f64, f64) -> Option<f64>;

fn register_math_functions(conn: &Connection) -> Result<()> {
    use rusqlite::functions::{Context, FunctionFlags};
    use rusqlite::types::Value;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;

    fn num(ctx: &Context<'_>, i: usize) -> Option<f64> {
        match ctx.get_raw(i) {
            ValueRef::Integer(n) => Some(n as f64),
            ValueRef::Real(f) => Some(f),
            ValueRef::Text(t) => std::str::from_utf8(t).ok()?.trim().parse().ok(),
            _ => None,
        }
    }
    fn real(v: Option<f64>) -> Value {
        match v {
            Some(f) if f.is_finite() => Value::Real(f),
            _ => Value::Null,
        }
    }

    // floor / ceil / ceiling / trunc keep an integer argument an integer.
    let rounding: [(&str, Rounding); 4] = [
        ("floor", f64::floor),
        ("ceil", f64::ceil),
        ("ceiling", f64::ceil),
        ("trunc", f64::trunc),
    ];
    for (name, f) in rounding {
        conn.create_scalar_function(name, 1, flags, move |ctx| {
            Ok(match ctx.get_raw(0) {
                ValueRef::Integer(n) => Value::Integer(n),
                _ => real(num(ctx, 0).map(f)),
            })
        })?;
    }
    let unary: [(&str, Unary); 20] = [
        ("sqrt", |x| (x >= 0.0).then(|| x.sqrt())),
        ("exp", |x| Some(x.exp())),
        ("ln", |x| (x > 0.0).then(|| x.ln())),
        ("log", |x| {
            (x > 0.0).then(|| x.ln() * (1.0 / std::f64::consts::LN_10))
        }),
        ("log10", |x| {
            (x > 0.0).then(|| x.ln() * (1.0 / std::f64::consts::LN_10))
        }),
        ("log2", |x| {
            (x > 0.0).then(|| x.ln() * (1.0 / std::f64::consts::LN_2))
        }),
        ("sin", |x| Some(x.sin())),
        ("cos", |x| Some(x.cos())),
        ("tan", |x| Some(x.tan())),
        ("asin", |x| (-1.0..=1.0).contains(&x).then(|| x.asin())),
        ("acos", |x| (-1.0..=1.0).contains(&x).then(|| x.acos())),
        ("atan", |x| Some(x.atan())),
        ("sinh", |x| Some(x.sinh())),
        ("cosh", |x| Some(x.cosh())),
        ("tanh", |x| Some(x.tanh())),
        ("asinh", |x| Some(x.asinh())),
        ("acosh", |x| (x >= 1.0).then(|| x.acosh())),
        ("atanh", |x| (x > -1.0 && x < 1.0).then(|| x.atanh())),
        ("degrees", |x| Some(x.to_degrees())),
        ("radians", |x| Some(x.to_radians())),
    ];
    for (name, f) in unary {
        conn.create_scalar_function(name, 1, flags, move |ctx| Ok(real(num(ctx, 0).and_then(f))))?;
    }
    let binary: [(&str, Binary); 5] = [
        ("pow", |x, y| Some(x.powf(y))),
        ("power", |x, y| Some(x.powf(y))),
        ("mod", |x, y| (y != 0.0).then(|| x % y)),
        ("atan2", |y, x| Some(y.atan2(x))),
        // log(B, X): base B.
        ("log", |b, x| {
            (b > 0.0 && b != 1.0 && x > 0.0).then(|| x.ln() / b.ln())
        }),
    ];
    for (name, f) in binary {
        conn.create_scalar_function(name, 2, flags, move |ctx| {
            Ok(real(match (num(ctx, 0), num(ctx, 1)) {
                (Some(a), Some(b)) => f(a, b),
                _ => None,
            }))
        })?;
    }
    conn.create_scalar_function("pi", 0, flags, |_| Ok(Value::Real(std::f64::consts::PI)))?;
    Ok(())
}

/// EX: the two result *sets* are equal (order-insensitive, duplicates collapsed).
pub fn ex(predicted: &[Row], gold: &[Row]) -> bool {
    let p: HashSet<&Row> = predicted.iter().collect();
    let g: HashSet<&Row> = gold.iter().collect();
    p == g
}

/// Soft-F1 exactly as `evaluation_f1.py::calculate_f1_score`: rows deduplicated in order, then
/// compared position by position, cell membership within the row.
pub fn soft_f1(predicted: &[Row], gold: &[Row]) -> f64 {
    if predicted.is_empty() && gold.is_empty() {
        return 1.0;
    }
    let predicted = dedup(predicted);
    let gold = dedup(gold);
    let (mut tp, mut fp, mut fn_) = (0.0, 0.0, 0.0);
    for (i, gt_row) in gold.iter().enumerate() {
        let Some(pred_row) = predicted.get(i) else {
            fn_ += 1.0;
            continue;
        };
        let width = gt_row.len() as f64;
        if width == 0.0 {
            continue;
        }
        let matches = pred_row.iter().filter(|v| gt_row.contains(v)).count() as f64;
        let pred_only = pred_row.len() as f64 - matches;
        let truth_only = gt_row.iter().filter(|v| !pred_row.contains(v)).count() as f64;
        tp += matches / width;
        fp += pred_only / width;
        fn_ += truth_only / width;
    }
    if predicted.len() > gold.len() {
        fp += (predicted.len() - gold.len()) as f64;
    }
    let precision = if tp + fp > 0.0 { tp / (tp + fp) } else { 0.0 };
    let recall = if tp + fn_ > 0.0 { tp / (tp + fn_) } else { 0.0 };
    if precision + recall > 0.0 {
        2.0 * precision * recall / (precision + recall)
    } else {
        0.0
    }
}

fn dedup(rows: &[Row]) -> Vec<&Row> {
    let mut seen = HashSet::new();
    rows.iter().filter(|r| seen.insert(*r)).collect()
}

/// R-VES reward for one question (`evaluation_ves.py::iterated_execute_sql`): only a correct
/// prediction earns one; the time ratio is gold time over predicted time, averaged after
/// dropping samples beyond three standard deviations. Like the reference, samples with no spread
/// all fail the strict test and earn 0 (there it is a caught division by zero).
pub fn rves_reward(ratios: &[f64]) -> f64 {
    let kept = clean_abnormal(ratios);
    if kept.is_empty() {
        return 0.0;
    }
    let ratio = kept.iter().sum::<f64>() / kept.len() as f64;
    if ratio == 0.0 {
        0.0
    } else if ratio >= 2.0 {
        1.25
    } else if ratio >= 1.0 {
        1.0
    } else if ratio >= 0.5 {
        0.75
    } else if ratio >= 0.25 {
        0.5
    } else {
        0.25
    }
}

fn clean_abnormal(xs: &[f64]) -> Vec<f64> {
    if xs.is_empty() {
        return Vec::new();
    }
    let n = xs.len() as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let std = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    xs.iter()
        .copied()
        .filter(|x| *x < mean + 3.0 * std && *x > mean - 3.0 * std)
        .collect()
}

/// R-VES over a run: `mean(sqrt(reward)) × 100` (`compute_ves`).
pub fn rves(rewards: &[f64]) -> f64 {
    if rewards.is_empty() {
        return 0.0;
    }
    rewards.iter().map(|r| r.sqrt() * 100.0).sum::<f64>() / rewards.len() as f64
}

/// The time ratios for one correct prediction: `iterations` paired runs, gold / predicted.
pub fn time_ratios(
    db: &Db,
    predicted: &str,
    gold: &str,
    iterations: usize,
    limits: Limits,
) -> Vec<f64> {
    let mut out = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let (Ok(p), Ok(g)) = (db.time(predicted, limits), db.time(gold, limits)) else {
            return Vec::new();
        };
        let p = p.as_secs_f64().max(1e-9);
        out.push(g.as_secs_f64() / p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE t (a INTEGER, b REAL, c TEXT);
             INSERT INTO t VALUES (1, 1.0, 'x'), (2, 2.5, 'y'), (2, 2.5, 'y'), (3, NULL, 'z');",
        )
        .unwrap();
        drop(c);
        let db = Db::open(&path).unwrap();
        (dir, db)
    }

    #[test]
    fn integers_and_integral_reals_are_the_same_cell_as_in_python() {
        let (_d, db) = db();
        let a = db
            .query("SELECT a FROM t WHERE a = 1", Limits::default())
            .unwrap();
        let b = db
            .query("SELECT b FROM t WHERE a = 1", Limits::default())
            .unwrap();
        assert!(ex(&a, &b));
        let c = db
            .query("SELECT c FROM t WHERE a = 1", Limits::default())
            .unwrap();
        assert!(!ex(&a, &c), "'x' is not 1");
    }

    #[test]
    fn ex_ignores_order_and_duplicates() {
        let (_d, db) = db();
        let l = Limits::default();
        let p = db.query("SELECT a FROM t ORDER BY a DESC", l).unwrap();
        let g = db.query("SELECT DISTINCT a FROM t", l).unwrap();
        assert!(ex(&p, &g));
    }

    #[test]
    fn the_database_is_read_only_and_errors_are_reported() {
        let (_d, db) = db();
        let err = db.query("DELETE FROM t", Limits::default()).unwrap_err();
        assert!(matches!(err, ExecError::Sql(_)), "{err:?}");
        let err = db
            .query("SELECT nope FROM t", Limits::default())
            .unwrap_err();
        assert!(err.to_string().contains("nope"));
    }

    #[test]
    fn a_slow_query_times_out() {
        let (_d, db) = db();
        let sql = "WITH RECURSIVE r(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM r) \
                   SELECT count(*) FROM r";
        let err = db
            .query(
                sql,
                Limits {
                    timeout: Duration::from_millis(200),
                    max_rows: 10,
                },
            )
            .unwrap_err();
        assert_eq!(err, ExecError::Timeout);
    }

    #[test]
    fn math_functions_answer_as_python_sqlite3_does() {
        let (_d, db) = db();
        let rows = db
            .query(
                "SELECT floor(2.7), ceil(2.1), power(2,3), sqrt(16), ln(1), log10(100), \
                 mod(7,3), floor(5), sqrt(-1), log(2, 8)",
                Limits::default(),
            )
            .unwrap();
        // Python 3.10 / SQLite 3.37: (2.0, 3.0, 8.0, 4.0, 0.0, 1.9999999999999998, 1.0, 5, None, 3.0)
        assert_eq!(
            rows[0],
            vec![
                Cell::real(2.0),
                Cell::real(3.0),
                Cell::real(8.0),
                Cell::real(4.0),
                Cell::real(0.0),
                Cell::real(1.9999999999999998),
                Cell::real(1.0),
                Cell::Int(5),
                Cell::Null,
                Cell::real(3.0),
            ]
        );
    }

    #[test]
    fn soft_f1_matches_the_reference_formula() {
        let r = |v: &[i64]| v.iter().map(|i| Cell::Int(*i)).collect::<Row>();
        // Identical → 1.
        assert_eq!(soft_f1(&[r(&[1, 2])], &[r(&[1, 2])]), 1.0);
        // Both empty → 1; one empty → 0.
        assert_eq!(soft_f1(&[], &[]), 1.0);
        assert_eq!(soft_f1(&[], &[r(&[1])]), 0.0);
        // One of two cells right: tp=.5 fp=.5 fn=.5 → p=r=.5 → f1=.5.
        assert!((soft_f1(&[r(&[1, 9])], &[r(&[1, 2])]) - 0.5).abs() < 1e-12);
        // An extra predicted row: tp=1 fp=1 fn=0 → p=.5 r=1 → 2/3.
        let f = soft_f1(&[r(&[1]), r(&[5])], &[r(&[1])]);
        assert!((f - 2.0 / 3.0).abs() < 1e-12, "{f}");
    }

    #[test]
    fn rves_rewards_follow_the_reference_table() {
        let around = |r: f64| [r * 0.99, r, r * 1.01];
        assert_eq!(rves_reward(&around(2.5)), 1.25);
        assert_eq!(rves_reward(&around(1.2)), 1.0);
        assert_eq!(rves_reward(&around(0.6)), 0.75);
        assert_eq!(rves_reward(&around(0.3)), 0.5);
        assert_eq!(rves_reward(&around(0.1)), 0.25);
        assert_eq!(rves_reward(&[]), 0.0);
        // As in the reference: with no spread every sample fails the strict 3σ test, the list is
        // empty and the reward is 0 — so one timed run never earns R-VES.
        assert_eq!(rves_reward(&[1.2]), 0.0);
        assert_eq!(rves_reward(&[2.5, 2.5]), 0.0);
        assert!((rves(&[1.0, 0.0, 0.25]) - 50.0).abs() < 1e-9);
    }
}
