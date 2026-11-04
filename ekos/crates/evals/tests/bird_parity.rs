//! RFC 0240 §2.5 / §4: EX and Soft-F1 parity with BIRD's official evaluator.
//!
//! The official scripts are not vendored: this test runs only when pointed at a checkout of
//! `bird-bench/mini_dev` and a recorded run, and is skipped (passing) otherwise.
//!
//! ```text
//! EKOS_BIRD_EVALUATOR=<mini_dev checkout>/evaluation
//! EKOS_BIRD_SUITE_DIR=<repo>/evals/bench/bird-mini-dev   (prepared: data/databases/ exists)
//! EKOS_BIRD_REPORT=<a bench report JSON>                 (the recorded predictions)
//! cargo test -p ekos-evals --test bird_parity -- --nocapture
//! ```
//!
//! A small driver imports the official `evaluation_utils.execute_sql` / `package_sqls`,
//! `evaluation_ex.calculate_ex` and `evaluation_f1.calculate_f1_score` and prints one verdict per
//! question; every verdict must equal the one in the report. `psycopg2`, `pymysql` and
//! `func_timeout` are imported by those modules but not needed for SQLite, so stand-ins are put
//! first on the path when the real ones are absent.

use ekos_evals::bench::bird::{gold_file, load_questions, submission_json};
use ekos_evals::bench::report::BenchReport;
use std::collections::BTreeMap;
use std::path::PathBuf;

const DRIVER: &str = r#"
import json, sys
shims, evaluator, predicted, gold, db_root = sys.argv[1:6]
sys.path.insert(0, evaluator)
sys.path.append(shims)
from evaluation_utils import execute_sql, package_sqls
from evaluation_ex import calculate_ex
from evaluation_f1 import calculate_f1_score

pred, _ = package_sqls(predicted, db_root, mode="pred")
gt, dbs = package_sqls(gold, db_root, mode="gt")
import sqlite3
out = []
for p, g, db in zip(pred, gt, dbs):
    error = None
    try:
        ex = execute_sql(p, g, db, "SQLite", calculate_ex)
    except Exception as e:
        ex, error = 0, str(e)
    try:
        f1 = execute_sql(p, g, db, "SQLite", calculate_f1_score)
    except Exception as e:
        f1, error = 0.0, error or str(e)
    out.append({"ex": ex, "f1": f1, "error": error})
print(json.dumps({"sqlite": sqlite3.sqlite_version, "verdicts": out}))
"#;

const FUNC_TIMEOUT_SHIM: &str = "class FunctionTimedOut(Exception):\n    pass\n\n\
def func_timeout(timeout, func, args=(), kwargs=None):\n    return func(*args, **(kwargs or {}))\n";

#[test]
fn ex_and_soft_f1_verdicts_match_the_official_evaluator() {
    let (Ok(evaluator), Ok(suite_dir), Ok(report)) = (
        std::env::var("EKOS_BIRD_EVALUATOR"),
        std::env::var("EKOS_BIRD_SUITE_DIR"),
        std::env::var("EKOS_BIRD_REPORT"),
    ) else {
        eprintln!("skipped: set EKOS_BIRD_EVALUATOR, EKOS_BIRD_SUITE_DIR and EKOS_BIRD_REPORT");
        return;
    };
    let suite_dir = PathBuf::from(suite_dir);
    let report = BenchReport::load(&PathBuf::from(report)).unwrap();
    let questions = load_questions(&suite_dir.join("data/archives/mini_dev_sqlite.jsonl")).unwrap();
    let by_id: BTreeMap<&str, _> = questions.iter().map(|q| (q.id.as_str(), q)).collect();
    let scenarios: Vec<_> = report
        .questions
        .iter()
        .map(|q| by_id[q.id.as_str()].clone())
        .collect();

    let tmp = tempfile::tempdir().unwrap();
    let shims = tmp.path().join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    std::fs::write(shims.join("func_timeout.py"), FUNC_TIMEOUT_SHIM).unwrap();
    std::fs::write(shims.join("psycopg2.py"), "").unwrap();
    std::fs::write(shims.join("pymysql.py"), "").unwrap();
    let predicted = tmp.path().join("predict.json");
    std::fs::write(&predicted, submission_json(&report)).unwrap();
    let gold = tmp.path().join("gold.sql");
    std::fs::write(&gold, gold_file(&scenarios)).unwrap();
    let driver = tmp.path().join("driver.py");
    std::fs::write(&driver, DRIVER).unwrap();
    let db_root = format!("{}/", suite_dir.join("data/databases").display());

    let out = std::process::Command::new("python3")
        .arg("-I")
        .arg(&driver)
        .arg(&shims)
        .arg(&evaluator)
        .arg(&predicted)
        .arg(&gold)
        .arg(&db_root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let official: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let official_sqlite = official["sqlite"].as_str().unwrap_or("?").to_string();
    let official = official["verdicts"].as_array().unwrap().clone();
    assert_eq!(official.len(), report.questions.len());

    let mut ex_mismatch = Vec::new();
    let mut f1_mismatch = Vec::new();
    // A query the evaluator's SQLite cannot parse but ours can: SQLite before 3.45 has a fixed
    // parser stack ("parser stack overflow" on deep nesting); the bundled 3.46 grows it. That is
    // an engine-version difference, reported, not a scorer mismatch.
    let mut engine_differences = Vec::new();
    for (q, o) in report.questions.iter().zip(&official) {
        if o["error"]
            .as_str()
            .is_some_and(|e| e.contains("parser stack overflow"))
            && q.score.executed
        {
            engine_differences.push(format!(
                "{}: the evaluator's SQLite {official_sqlite} overflowed its parser stack; ours \
                 ran it ({})",
                q.id,
                if q.score.ex {
                    "EX differs"
                } else {
                    "EX false in both"
                }
            ));
            if q.score.ex {
                ex_mismatch.push(format!(
                    "{}: ours true, official false (parser stack)",
                    q.id
                ));
            }
            continue;
        }
        let official_ex = o["ex"].as_i64().unwrap() == 1;
        if official_ex != q.score.ex {
            ex_mismatch.push(format!(
                "{}: ours {} official {official_ex}",
                q.id, q.score.ex
            ));
        }
        let official_f1 = o["f1"].as_f64().unwrap();
        if (official_f1 - q.score.soft_f1).abs() > 1e-9 {
            f1_mismatch.push(format!(
                "{}: ours {:.6} official {official_f1:.6}",
                q.id, q.score.soft_f1
            ));
        }
    }
    let correct = report.questions.iter().filter(|q| q.score.ex).count();
    eprintln!(
        "parity over {} questions ({correct} correct), evaluator SQLite {official_sqlite}: {} EX \
         and {} Soft-F1 mismatches, {} engine-version differences",
        report.questions.len(),
        ex_mismatch.len(),
        f1_mismatch.len(),
        engine_differences.len()
    );
    for d in &engine_differences {
        eprintln!("  engine difference — {d}");
    }
    assert!(
        ex_mismatch.is_empty(),
        "EX verdicts differ: {ex_mismatch:#?}"
    );
    assert!(f1_mismatch.is_empty(), "Soft-F1 differs: {f1_mismatch:#?}");
}
