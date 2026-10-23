//! Measures the `oracle` dialect against a real Oracle script corpus (RFC 0210 §5.1):
//! statements `GenericDialect` rejects as written, and after `preprocess`.
//!
//! `EKOS_ORACLE_CORPUS=<dir> cargo test -p ekos-plugin-sql-dialect-oracle --test corpus -- --ignored --nocapture`

use ekos_plugin_sql_dialect_oracle::preprocess;
use ekos_sql_dialect_sdk::lex::statement_spans;
use sqlparser::dialect::GenericDialect;
use sqlparser::parser::Parser;
use std::collections::BTreeMap;

#[derive(Default)]
struct Tally {
    statements: usize,
    rejected: usize,
    errors: BTreeMap<String, (usize, String)>,
}

impl Tally {
    fn add(&mut self, sql: &str) {
        let chars: Vec<char> = sql.chars().collect();
        for (s, e) in statement_spans(&chars) {
            let text: String = chars[s..e].iter().collect();
            let code: String = text
                .lines()
                .filter(|l| !l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            if code.trim().trim_end_matches(';').trim().is_empty() {
                continue;
            }
            self.statements += 1;
            if let Err(err) = Parser::parse_sql(&GenericDialect {}, &code) {
                self.rejected += 1;
                let msg = err.to_string();
                let key = msg.split(" at Line").next().unwrap_or_default().to_string();
                let entry = self
                    .errors
                    .entry(key)
                    .or_insert((0, code.trim().chars().take(200).collect()));
                entry.0 += 1;
            }
        }
    }
}

#[test]
#[ignore]
fn oracle_corpus() {
    let Ok(root) = std::env::var("EKOS_ORACLE_CORPUS") else {
        return;
    };
    let mut before = Tally::default();
    let mut after = Tally::default();
    let mut files = 0;
    for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
        let path = entry.path();
        let lower = path.to_string_lossy().to_lowercase();
        if !lower.ends_with(".sql") || !lower.contains("oracle") {
            continue;
        }
        let bytes = std::fs::read(path).unwrap();
        let sql = String::from_utf8_lossy(&bytes);
        files += 1;
        before.add(&sql);
        let pre = preprocess(&sql);
        assert_eq!(
            pre.lines().count(),
            sql.lines().count(),
            "{}",
            path.display()
        );
        after.add(&pre);
    }
    println!("files {files}");
    println!(
        "generic : {} statements, {} rejected",
        before.statements, before.rejected
    );
    println!(
        "oracle  : {} statements, {} rejected",
        after.statements, after.rejected
    );
    let mut errs: Vec<_> = after.errors.into_iter().collect();
    errs.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (msg, (n, sample)) in errs.iter().take(25) {
        println!("{n:6} {msg}\n         {sample}");
    }
}
