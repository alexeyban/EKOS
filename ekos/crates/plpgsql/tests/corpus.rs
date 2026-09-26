//! RFC 0163 — the parser against routines taken from a real database.
//!
//! The unit fixtures prove each construct parses in isolation. This proves the parser survives
//! bodies as PostgreSQL itself stores them — with the whitespace, the nesting and the idioms real
//! routines have — and it establishes the **ratchet**: the proportion reaching `Statements` is
//! asserted, and that floor only ever goes up.
//!
//! ```text
//! docker compose -f docker-compose.migrate.yml up -d
//! EKOS_MIGRATE_LIVE=1 cargo test -p ekos-plpgsql --test corpus
//! ```

use ekos_plpgsql::{Fidelity, parse_function};

fn live() -> bool {
    std::env::var("EKOS_MIGRATE_LIVE").is_ok()
}

/// Read every PL/pgSQL routine in a schema, exactly as `pg_get_functiondef` renders it.
fn routines(schema: &str) -> Vec<(String, String)> {
    let out = std::process::Command::new("psql")
        .args([
            "-h",
            "localhost",
            "-p",
            "55432",
            "-U",
            "ekos",
            "-d",
            "ekos_migrate_fixtures",
            "-tA",
            "-R",
            "\x1e",
            "-F",
            "\x1f",
            "-c",
            &format!(
                "SELECT p.proname, pg_get_functiondef(p.oid) FROM pg_proc p \
                 JOIN pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = '{schema}' ORDER BY p.proname"
            ),
        ])
        .env("PGPASSWORD", "ekos-local-only")
        .output()
        .expect("psql");
    assert!(
        out.status.success(),
        "psql: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .split('\x1e')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|row| {
            let (name, def) = row.split_once('\x1f')?;
            Some((name.trim().to_string(), def.to_string()))
        })
        .collect()
}

#[test]
fn real_routines_reach_the_statements_floor() {
    if !live() {
        eprintln!("skipped: set EKOS_MIGRATE_LIVE=1 with the sandboxes up");
        return;
    }
    let all = routines("ekos_proc");
    assert!(all.len() >= 5, "fixture schema is missing: {}", all.len());

    let mut complete = Vec::new();
    let mut partial = Vec::new();
    for (name, def) in &all {
        let ir = parse_function(def).unwrap_or_else(|e| panic!("{name} failed to lex: {e}"));
        match ir.fidelity() {
            Fidelity::Statements => complete.push(name.clone()),
            Fidelity::Partial {
                recovered,
                unrecovered,
            } => {
                partial.push(format!("{name} ({recovered} ok, {unrecovered} missed)"));
            }
            Fidelity::Signature => panic!("{name} is plpgsql but was not parsed at all"),
        }
    }

    // The ratchet. A regression that drops a routine from `Statements` to `Partial` fails here with
    // the routine named — which is the only way a parser's coverage stays honest over time.
    // Every routine in the fixture schema recovers. The floor only ever goes up.
    let floor = 5;
    assert!(
        complete.len() >= floor,
        "only {}/{} routines reached Statements (floor {floor}). Partial: {partial:?}",
        complete.len(),
        all.len()
    );
    eprintln!(
        "{}/{} routines fully recovered; partial: {partial:?}",
        complete.len(),
        all.len()
    );
}

/// The three properties that matter downstream, checked on real bodies rather than fixtures.
#[test]
fn real_routines_carry_usable_spans_and_honest_labels() {
    if !live() {
        return;
    }
    for (name, def) in routines("ekos_proc") {
        let ir = parse_function(&def).unwrap();

        // 1. Every span points at real text inside the definition.
        for s in &ir.body {
            s.walk(&mut |x| {
                let sp = x.span();
                assert!(
                    sp.end <= def.len() && sp.start < sp.end,
                    "{name}: span {sp:?} outside a {}-byte definition",
                    def.len()
                );
            });
        }

        // 2. A gap is always locatable, so a finding can say what was missed.
        for gap in ir.gaps() {
            assert!(gap.end <= def.len(), "{name}: gap span past the end");
        }

        // 3. Eligibility follows fidelity, never the producer's say-so.
        assert_eq!(
            ir.eligible_for_reconstruction(),
            ir.fidelity() == Fidelity::Statements,
            "{name}"
        );
    }
}

/// Dynamic SQL is a boundary RFC 0164 will not cross, and it must be *found* on a real routine
/// rather than only on a hand-written fixture.
#[test]
fn a_real_dynamic_routine_reports_its_boundary() {
    if !live() {
        return;
    }
    let (_, def) = routines("ekos_proc")
        .into_iter()
        .find(|(n, _)| n == "dynamic")
        .expect("the dynamic fixture");
    let ir = parse_function(&def).unwrap();
    assert_eq!(
        ir.dynamic_sites().len(),
        1,
        "the EXECUTE site must be reported: {:?}",
        ir.body
    );
    assert_eq!(
        ir.fidelity(),
        Fidelity::Statements,
        "constructing SQL is not a recovery failure"
    );
}
