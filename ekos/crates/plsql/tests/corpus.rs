//! Measures the PL/SQL front end on a real Oracle corpus (RFC 0210 §5.2): routines by fidelity,
//! statements recovered, and the reasons for what is not.
//!
//! `EKOS_ORACLE_CORPUS=<dir> cargo test -p ekos-plsql --test corpus -- --ignored --nocapture`

use ekos_plpgsql::{Fidelity, ProcStmt};
use ekos_plsql::read_script;
use std::collections::BTreeMap;

#[test]
#[ignore]
fn plsql_corpus() {
    let Ok(root) = std::env::var("EKOS_ORACLE_CORPUS") else {
        return;
    };
    let (mut files, mut routines, mut triggers, mut anonymous, mut unterminated) = (0, 0, 0, 0, 0);
    let mut fidelity: BTreeMap<&str, usize> = BTreeMap::new();
    let (mut recovered, mut unrecovered) = (0usize, 0usize);
    let mut reasons: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut skipped: BTreeMap<String, usize> = BTreeMap::new();
    for entry in walkdir::WalkDir::new(&root).into_iter().flatten() {
        let path = entry.path();
        let lower = path.to_string_lossy().to_lowercase();
        if !lower.ends_with(".sql") || !lower.contains("oracle") {
            continue;
        }
        let src = String::from_utf8_lossy(&std::fs::read(path).unwrap()).into_owned();
        files += 1;
        let s = read_script(&src);
        anonymous += s.anonymous_blocks;
        unterminated += s.unterminated;
        for k in s.skipped {
            *skipped.entry(k).or_default() += 1;
        }
        for r in s.routines {
            routines += 1;
            triggers += usize::from(r.trigger.is_some());
            let f = match r.ir.fidelity() {
                Fidelity::Signature => "signature",
                Fidelity::Partial { .. } => "partial",
                Fidelity::Statements => "statements",
            };
            *fidelity.entry(f).or_default() += 1;
            for st in &r.ir.body {
                st.walk(&mut |x| {
                    if let ProcStmt::Unrecovered { raw, reason, .. } = x {
                        unrecovered += 1;
                        let e = reasons
                            .entry(reason.clone())
                            .or_insert((0, raw.chars().take(120).collect()));
                        e.0 += 1;
                    } else {
                        recovered += 1;
                    }
                });
            }
        }
    }
    println!(
        "files {files}, routines {routines} (triggers {triggers}), anonymous blocks {anonymous}, unterminated units {unterminated}"
    );
    println!("fidelity {fidelity:?}");
    println!("statements recovered {recovered}, unrecovered {unrecovered}");
    println!("skipped units {skipped:?}");
    let mut r: Vec<_> = reasons.into_iter().collect();
    r.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (why, (n, sample)) in r.iter().take(20) {
        println!("{n:5} {why}\n        {sample}");
    }
}
