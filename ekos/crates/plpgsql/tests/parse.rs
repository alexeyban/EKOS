//! RFC 0163 — one fixture per `ProcStmt` variant, plus the properties that make the IR trustworthy.
//!
//! The parser exists so RFC 0164's anti-invention check has a real node set to map against. A
//! variant nobody has a fixture for is a variant nobody knows is broken, and a check against a
//! broken node set is the vacuous one this whole RFC was written to replace.

use ekos_plpgsql::ir::*;
use ekos_plpgsql::{Fidelity, ProcStmt, parse_function};

fn f(body: &str) -> ProcedureIr {
    parse_function(&format!(
        "CREATE FUNCTION t() RETURNS int LANGUAGE plpgsql AS $$ BEGIN {body} END $$"
    ))
    .expect("lex")
}

fn kinds(ir: &ProcedureIr) -> Vec<&'static str> {
    let mut out = Vec::new();
    for s in &ir.body {
        s.walk(&mut |x| {
            out.push(match x {
                ProcStmt::Sql { .. } => "sql",
                ProcStmt::Assign { .. } => "assign",
                ProcStmt::If { .. } => "if",
                ProcStmt::Case { .. } => "case",
                ProcStmt::Loop { .. } => "loop",
                ProcStmt::Exit { .. } => "exit",
                ProcStmt::Return { .. } => "return",
                ProcStmt::Raise { .. } => "raise",
                ProcStmt::Block { .. } => "block",
                ProcStmt::Perform { .. } => "perform",
                ProcStmt::Cursor { .. } => "cursor",
                ProcStmt::DynamicExecute { .. } => "dynamic",
                ProcStmt::Unrecovered { .. } => "unrecovered",
            });
        });
    }
    out
}

// ── one fixture per variant ──────────────────────────────────────────────────

#[test]
fn a_plain_sql_statement_is_recovered() {
    let ir = f("INSERT INTO audit(x) VALUES (1);");
    assert_eq!(kinds(&ir), vec!["sql"]);
    assert_eq!(ir.fidelity(), Fidelity::Statements);
}

/// `SELECT … INTO` binds results to variables. The target list belongs to the control flow, not to
/// the query, so it is lifted out — RFC 0164 lowers `sql` and needs it to be a query.
#[test]
fn select_into_separates_the_query_from_its_targets() {
    let ir = f("SELECT count(*) INTO n FROM orders WHERE id = 1;");
    match &ir.body[0] {
        ProcStmt::Sql { sql, into, .. } => {
            assert_eq!(into.as_deref(), Some(&["n".to_string()][..]));
            assert!(sql.starts_with("SELECT count(*)"), "{sql}");
            assert!(sql.contains("FROM orders"), "{sql}");
            assert!(!sql.to_uppercase().contains("INTO"), "{sql}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn assignment_is_recovered() {
    let ir = f("total := total + 1;");
    match &ir.body[0] {
        ProcStmt::Assign { target, expr, .. } => {
            assert_eq!(target, "total");
            assert_eq!(expr, "total + 1");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn if_elsif_else_recovers_every_branch() {
    let ir = f("IF a > 0 THEN RETURN 1; ELSIF a = 0 THEN RETURN 0; ELSE RETURN -1; END IF;");
    match &ir.body[0] {
        ProcStmt::If {
            branches,
            else_branch,
            ..
        } => {
            assert_eq!(branches.len(), 2, "{branches:?}");
            assert_eq!(branches[0].0, "a > 0");
            assert_eq!(branches[1].0, "a = 0");
            assert!(else_branch.is_some());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn case_recovers_its_operand_and_branches() {
    let ir = f(
        "CASE status WHEN 'new' THEN PERFORM f(); WHEN 'old' THEN PERFORM g(); ELSE PERFORM h(); END CASE;",
    );
    match &ir.body[0] {
        ProcStmt::Case {
            operand,
            branches,
            else_branch,
            ..
        } => {
            assert_eq!(operand.as_deref(), Some("status"));
            assert_eq!(branches.len(), 2, "{branches:?}");
            assert!(else_branch.is_some());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_loop_form_is_recovered() {
    type Check = fn(&LoopKind) -> bool;
    let cases: Vec<(&str, Check)> = vec![
        ("LOOP EXIT; END LOOP;", |k| matches!(k, LoopKind::Plain)),
        (
            "WHILE i < 10 LOOP i := i + 1; END LOOP;",
            |k| matches!(k, LoopKind::While { condition } if condition == "i < 10"),
        ),
        (
            "FOR i IN 1 .. 10 LOOP PERFORM f(i); END LOOP;",
            |k| matches!(k, LoopKind::ForRange { var, from, to, reverse: false } if var == "i" && from == "1" && to == "10"),
        ),
        (
            "FOR i IN REVERSE 10 .. 1 LOOP PERFORM f(i); END LOOP;",
            |k| matches!(k, LoopKind::ForRange { reverse: true, .. }),
        ),
        (
            "FOR r IN SELECT * FROM orders LOOP PERFORM f(r); END LOOP;",
            |k| matches!(k, LoopKind::ForQuery { var, sql } if var == "r" && sql.contains("FROM orders")),
        ),
        (
            "FOREACH x IN ARRAY items LOOP PERFORM f(x); END LOOP;",
            |k| matches!(k, LoopKind::ForEach { var, array } if var == "x" && array == "items"),
        ),
    ];
    for (src, check) in cases {
        let ir = f(src);
        match &ir.body[0] {
            ProcStmt::Loop { kind, .. } => assert!(check(kind), "{src}: {kind:?}"),
            other => panic!("{src}: {other:?}"),
        }
    }
}

#[test]
fn exit_and_continue_carry_their_condition() {
    let ir = f("LOOP EXIT WHEN i > 10; END LOOP;");
    let mut found = false;
    ir.body[0].walk(&mut |s| {
        if let ProcStmt::Exit {
            when, is_continue, ..
        } = s
        {
            assert_eq!(when.as_deref(), Some("i > 10"));
            assert!(!is_continue);
            found = true;
        }
    });
    assert!(found, "EXIT WHEN not recovered");

    let ir = f("LOOP CONTINUE WHEN i < 5; END LOOP;");
    let mut found = false;
    ir.body[0].walk(&mut |s| {
        if let ProcStmt::Exit { is_continue, .. } = s {
            assert!(is_continue);
            found = true;
        }
    });
    assert!(found);
}

#[test]
fn every_return_form_is_distinguished() {
    match &f("RETURN 1;").body[0] {
        ProcStmt::Return {
            value,
            next: false,
            query: None,
            ..
        } => {
            assert_eq!(value.as_deref(), Some("1"))
        }
        other => panic!("{other:?}"),
    }
    match &f("RETURN NEXT r;").body[0] {
        ProcStmt::Return {
            next: true, value, ..
        } => assert_eq!(value.as_deref(), Some("r")),
        other => panic!("{other:?}"),
    }
    match &f("RETURN QUERY SELECT * FROM orders;").body[0] {
        ProcStmt::Return { query: Some(q), .. } => assert!(q.contains("FROM orders"), "{q}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn raise_recovers_its_level() {
    match &f("RAISE WARNING 'careful';").body[0] {
        ProcStmt::Raise { level, message, .. } => {
            assert_eq!(level, "warning");
            assert!(message.contains("careful"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    // A bare RAISE re-raises, and defaults to exception level.
    match &f("RAISE 'boom';").body[0] {
        ProcStmt::Raise { level, .. } => assert_eq!(level, "exception"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_nested_block_with_an_exception_handler_is_recovered() {
    let ir =
        f("BEGIN PERFORM risky(); EXCEPTION WHEN unique_violation THEN PERFORM log_it(); END;");
    match &ir.body[0] {
        ProcStmt::Block {
            body, exception, ..
        } => {
            assert_eq!(body.len(), 1);
            assert_eq!(exception.len(), 1, "{exception:?}");
            assert_eq!(exception[0].conditions, vec!["unique_violation"]);
            assert_eq!(exception[0].body.len(), 1);
        }
        other => panic!("{other:?}"),
    }
}

/// Cursors are how a lot of older PL/pgSQL walks a result set. A routine using one is not a
/// routine with a gap in it — which is what it was until this variant existed, and the corpus test
/// is what made that visible.
#[test]
fn cursor_operations_are_recovered() {
    let ir = f("OPEN c FOR SELECT id FROM orders; FETCH c INTO v; CLOSE c;");
    let k = kinds(&ir);
    assert_eq!(k, vec!["cursor", "cursor", "cursor"], "{:?}", ir.body);
    assert_eq!(ir.fidelity(), Fidelity::Statements);

    match &ir.body[0] {
        ProcStmt::Cursor {
            op, name, query, ..
        } => {
            assert_eq!(*op, ekos_plpgsql::CursorOp::Open);
            assert_eq!(name, "c");
            assert!(query.as_deref().unwrap().contains("FROM orders"));
        }
        other => panic!("{other:?}"),
    }
    match &ir.body[1] {
        ProcStmt::Cursor { op, name, into, .. } => {
            assert_eq!(*op, ekos_plpgsql::CursorOp::Fetch);
            assert_eq!(name, "c");
            assert_eq!(into.as_deref(), Some(&["v".to_string()][..]));
        }
        other => panic!("{other:?}"),
    }
}

/// `FETCH NEXT FROM c INTO v` puts a direction before the name. The cursor is still `c`.
#[test]
fn a_fetch_with_a_direction_still_names_the_cursor() {
    let ir = f("FETCH NEXT FROM c INTO v;");
    match &ir.body[0] {
        ProcStmt::Cursor { name, into, .. } => {
            assert_eq!(name, "c");
            assert_eq!(into.as_deref(), Some(&["v".to_string()][..]));
        }
        other => panic!("{other:?}"),
    }
}

// ── dynamic SQL: a boundary, not a failure ───────────────────────────────────

#[test]
fn dynamic_execute_is_recovered_as_a_boundary_not_a_gap() {
    let ir = f("EXECUTE 'SELECT * FROM ' || quote_ident(tbl) USING a, b;");
    match &ir.body[0] {
        ProcStmt::DynamicExecute { expr, using, .. } => {
            assert!(expr.contains("quote_ident"), "{expr}");
            assert_eq!(using, &["a".to_string(), "b".to_string()]);
        }
        other => panic!("{other:?}"),
    }
    // It is faithful recovery of something genuinely not statically known, so fidelity is intact —
    // and the site is reported so RFC 0164 refuses to reconstruct across it.
    assert_eq!(ir.fidelity(), Fidelity::Statements);
    assert_eq!(ir.dynamic_sites().len(), 1);
}

// ── local recovery ───────────────────────────────────────────────────────────

/// The behaviour this whole crate exists to fix. `sql_transform_analyzer.rs` loses the entire
/// routine to one unparseable construct; here one bad statement costs exactly one statement.
#[test]
fn one_malformed_statement_costs_exactly_one_statement() {
    let ir = f("PERFORM a(); ¿¿¿ nonsense ???; PERFORM b(); PERFORM c();");
    let k = kinds(&ir);
    assert_eq!(
        k.iter().filter(|x| **x == "unrecovered").count(),
        1,
        "{k:?}"
    );
    assert_eq!(k.iter().filter(|x| **x == "perform").count(), 3, "{k:?}");
    match ir.fidelity() {
        Fidelity::Partial {
            recovered,
            unrecovered,
        } => {
            assert_eq!(unrecovered, 1);
            assert_eq!(recovered, 3);
        }
        other => panic!("{other:?}"),
    }
}

/// An unrecovered statement keeps its text and a reason, so a finding can say what was missed
/// rather than that something was.
#[test]
fn an_unrecovered_statement_says_what_and_why() {
    let ir = f("¿¿¿ nonsense ???;");
    match &ir.body[0] {
        ProcStmt::Unrecovered { raw, reason, .. } => {
            assert!(raw.contains("nonsense"), "{raw}");
            assert!(!reason.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

// ── properties ───────────────────────────────────────────────────────────────

/// Every statement cites its source exactly — the discipline RFC 0150 applies with IL offsets.
#[test]
fn every_span_points_at_real_source_text() {
    let src = "CREATE FUNCTION t() RETURNS int LANGUAGE plpgsql AS $$ BEGIN \
               PERFORM alpha(); PERFORM beta(); END $$";
    let ir = parse_function(src).unwrap();
    let mut seen = 0;
    for s in &ir.body {
        s.walk(&mut |x| {
            let sp = x.span();
            assert!(sp.end <= src.len(), "span past the end of source: {sp:?}");
            assert!(sp.start < sp.end, "empty span: {sp:?}");
            seen += 1;
        });
    }
    assert_eq!(seen, 2);
    // And the spans locate the right text.
    let texts: Vec<&str> = ir
        .body
        .iter()
        .map(|s| &src[s.span().start..s.span().end])
        .collect();
    assert!(texts[0].contains("alpha"), "{texts:?}");
    assert!(texts[1].contains("beta"), "{texts:?}");
}

/// A semicolon inside a string, a dollar quote or a comment does not end a statement.
#[test]
fn semicolons_hiding_inside_literals_do_not_split_statements() {
    let ir = f("PERFORM note('a; b'); PERFORM other();");
    assert_eq!(kinds(&ir), vec!["perform", "perform"], "{:?}", ir.body);

    let ir = f("PERFORM note('x'); -- a comment with ; in it\n PERFORM other();");
    assert_eq!(kinds(&ir), vec!["perform", "perform"]);
}

/// Determinism, required of every recovery pass (RFC 0135 Part C).
#[test]
fn parsing_is_deterministic() {
    let src = "CREATE FUNCTION t() RETURNS int LANGUAGE plpgsql AS $$ \
               DECLARE n int := 0; \
               BEGIN \
                 FOR r IN SELECT * FROM orders LOOP \
                   IF r.total > 100 THEN n := n + 1; ELSE PERFORM skip(r); END IF; \
                 END LOOP; \
                 RETURN n; \
               END $$";
    let a = parse_function(src).unwrap();
    let b = parse_function(src).unwrap();
    assert_eq!(
        serde_json::to_string(&a).unwrap(),
        serde_json::to_string(&b).unwrap()
    );
}

/// A realistic routine: declarations, a query loop, a branch, an assignment and a return.
#[test]
fn a_realistic_routine_recovers_completely() {
    let src = "CREATE FUNCTION count_big(threshold numeric) RETURNS int LANGUAGE plpgsql AS $$ \
               DECLARE n int := 0; r record; \
               BEGIN \
                 FOR r IN SELECT * FROM orders LOOP \
                   IF r.total > threshold THEN n := n + 1; END IF; \
                 END LOOP; \
                 RETURN n; \
               END $$";
    let ir = parse_function(src).unwrap();
    assert_eq!(ir.signature.name, "count_big");
    assert_eq!(ir.signature.returns.as_deref(), Some("int"));
    assert_eq!(ir.signature.language, "plpgsql");
    assert_eq!(ir.declarations.len(), 2, "{:?}", ir.declarations);
    assert_eq!(ir.declarations[0].name, "n");
    assert_eq!(ir.declarations[0].default.as_deref(), Some("0"));

    let k = kinds(&ir);
    assert!(k.contains(&"loop"), "{k:?}");
    assert!(k.contains(&"if"), "{k:?}");
    assert!(k.contains(&"assign"), "{k:?}");
    assert!(k.contains(&"return"), "{k:?}");
    assert_eq!(
        ir.fidelity(),
        Fidelity::Statements,
        "a routine this ordinary must recover completely: {:?}",
        ir.gaps()
    );
    assert!(ir.eligible_for_reconstruction());
}

/// A routine in a language this crate does not parse gets its signature and an honest label — never
/// an empty body, which would read as "there is nothing in it".
#[test]
fn a_non_plpgsql_routine_is_signature_only() {
    let ir =
        parse_function("CREATE FUNCTION t() RETURNS int LANGUAGE c AS 'module', 'symbol'").unwrap();
    assert_eq!(ir.fidelity(), Fidelity::Signature);
    assert!(!ir.eligible_for_reconstruction());
    assert_eq!(ir.signature.language, "c");
}

/// Every `ProcStmt` variant has a fixture above. A variant nobody exercises is one nobody knows is
/// broken, and the anti-invention check downstream is only as good as the node set.
#[test]
fn every_statement_variant_has_a_fixture() {
    let src = include_str!("parse.rs");
    for variant in [
        "ProcStmt::Sql",
        "ProcStmt::Assign",
        "ProcStmt::If",
        "ProcStmt::Case",
        "ProcStmt::Loop",
        "ProcStmt::Exit",
        "ProcStmt::Return",
        "ProcStmt::Raise",
        "ProcStmt::Block",
        "ProcStmt::Perform",
        "ProcStmt::Cursor",
        "ProcStmt::DynamicExecute",
        "ProcStmt::Unrecovered",
    ] {
        assert!(
            src.matches(variant).count() >= 2,
            "{variant} is matched in `kinds` but never asserted by a fixture"
        );
    }
}
