//! RFC 0163 — the parser against LedgerSMB, the RFC's primary corpus, read from source files.
//!
//! `corpus.rs` reads five routines back from a live server. This reads every routine LedgerSMB
//! actually installs — the files its `sql/modules/LOADORDER` lists, in that order — straight from a
//! checkout, the way `recover` meets them in a repository: `LANGUAGE` after the body, comments full
//! of apostrophes, pre-8.0 single-quoted bodies, `=` assignment, `ELSEIF`.
//!
//! The first run of this test found 165 routines the parser had never parsed at all (`LANGUAGE
//! plpgsql;` was read as the language `plpgsql;`), and seven more bugs behind those. The floor below
//! is the ratchet: it only ever goes up.
//!
//! ```text
//! EKOS_LEDGERSMB_DIR=/path/to/LedgerSMB cargo test -p ekos-plpgsql --test ledgersmb_corpus -- --nocapture
//! ```
//!
//! Measured against LedgerSMB `544bcd947` (2026-09-13, `1.7.0-beta1-6323`).

use ekos_plpgsql::lex::{Tok, lex};
use ekos_plpgsql::{Fidelity, ProcStmt, ProcedureIr, parse_function};
use std::path::{Path, PathBuf};

fn checkout() -> Option<PathBuf> {
    std::env::var_os("EKOS_LEDGERSMB_DIR").map(PathBuf::from)
}

/// The module files LedgerSMB loads, in load order. A commented-out line is a module it does not
/// load, and its routines are not part of the corpus: `Business_Dates.sql` is excluded upstream and
/// holds routines PostgreSQL itself would reject.
fn loaded_modules(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("sql/modules");
    std::fs::read_to_string(dir.join("LOADORDER"))
        .expect("sql/modules/LOADORDER — is EKOS_LEDGERSMB_DIR a LedgerSMB checkout?")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| dir.join(l))
        .collect()
}

/// Every `CREATE [OR REPLACE] FUNCTION|PROCEDURE` statement in a file, split at top-level
/// semicolons by the crate's own lexer, so a semicolon inside a body never splits it.
fn routines(src: &str) -> Vec<String> {
    let toks = lex(src).expect("module lexes");
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for t in &toks {
        let s = *start.get_or_insert(t.start);
        if t.tok == Tok::Punct(';') {
            start = None;
            let text = &src[s..t.end];
            let head: Vec<String> = text
                .split_whitespace()
                .take(4)
                .map(str::to_ascii_uppercase)
                .collect();
            let head: Vec<&str> = head.iter().map(String::as_str).collect();
            if matches!(
                head.as_slice(),
                ["CREATE", "FUNCTION" | "PROCEDURE", ..]
                    | ["CREATE", "OR", "REPLACE", "FUNCTION" | "PROCEDURE"]
            ) {
                out.push(text.to_string());
            }
        }
    }
    out
}

struct Measured {
    plpgsql: usize,
    complete: Vec<String>,
    partial: Vec<String>,
}

fn measure(root: &Path) -> (Measured, Vec<(String, String, ProcedureIr)>) {
    let mut m = Measured {
        plpgsql: 0,
        complete: Vec::new(),
        partial: Vec::new(),
    };
    let mut all = Vec::new();
    for file in loaded_modules(root) {
        let src = std::fs::read_to_string(&file).expect("module readable");
        let module = file.file_name().unwrap().to_string_lossy().into_owned();
        for def in routines(&src) {
            let ir = parse_function(&def)
                .unwrap_or_else(|e| panic!("{module}: a routine failed to lex: {e}"));
            if ir.signature.language != "plpgsql" {
                continue;
            }
            m.plpgsql += 1;
            let name = format!("{module}::{}", ir.signature.name);
            match ir.fidelity() {
                Fidelity::Statements => m.complete.push(name.clone()),
                Fidelity::Partial {
                    recovered,
                    unrecovered,
                } => m
                    .partial
                    .push(format!("{name} ({recovered} ok, {unrecovered} missed)")),
                Fidelity::Signature => panic!("{name} is plpgsql but its body was never parsed"),
            }
            all.push((name, def, ir));
        }
    }
    (m, all)
}

#[test]
fn ledgersmb_routines_reach_the_statements_floor() {
    let Some(root) = checkout() else {
        eprintln!("skipped: set EKOS_LEDGERSMB_DIR to a LedgerSMB checkout");
        return;
    };
    let (m, _) = measure(&root);

    // The ratchet: 212/212 at the pinned commit. A regression fails here with
    // the routine named. Raise the floor when a newer checkout adds routines — never lower it.
    let floor = 212;
    assert!(
        m.complete.len() >= floor,
        "only {}/{} routines reached Statements (floor {floor}). Partial: {:#?}",
        m.complete.len(),
        m.plpgsql,
        m.partial
    );
    eprintln!(
        "{}/{} LedgerSMB routines fully recovered; partial: {:?}",
        m.complete.len(),
        m.plpgsql,
        m.partial
    );
}

/// The properties `corpus.rs` checks on five live routines, on two hundred real ones: every span
/// lies inside its routine and covers exactly a statement, and parsing is deterministic.
#[test]
fn ledgersmb_spans_are_exact_and_parsing_is_deterministic() {
    let Some(root) = checkout() else {
        return;
    };
    let (_, all) = measure(&root);
    for (name, def, ir) in &all {
        for s in &ir.body {
            s.walk(&mut |x| {
                let sp = x.span();
                assert!(
                    sp.start < sp.end && sp.end <= def.len(),
                    "{name}: span {sp:?} outside a {}-byte definition",
                    def.len()
                );
                let text = &def[sp.start..sp.end];
                assert_eq!(
                    text,
                    text.trim(),
                    "{name}: span carries whitespace: {text:?}"
                );
                if !matches!(x, ProcStmt::Unrecovered { .. }) {
                    assert!(
                        !text.ends_with(';'),
                        "{name}: span runs past its statement: {text:?}"
                    );
                }
            });
        }
        assert_eq!(
            &parse_function(def).unwrap(),
            ir,
            "{name}: a second parse differs"
        );
    }
}
