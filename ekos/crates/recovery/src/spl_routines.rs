//! RFC 0252 §3.5 — Informix views, SPL routines and triggers from the live catalog
//! (`sysviews`, `sysprocbody`, `systrigbody`), for `OracleCatalogPass` under
//! `CatalogDialect::Informix`.
//!
//! There is no Informix SPL reader (no `sql-dialect-informix`, no `ProcedureIr` front end), so —
//! as RFC 0252 §3.5 says — an SPL routine is kept as a `Procedure` **without IR**, at signature
//! fidelity: its arguments and return types read from the header, its `DOCUMENT` text as its
//! description, and its footprint (`reads` / `writes` / `calls`) from the SQL statements inside
//! it, each parsed alone in the generic dialect after SPL's own words around it are taken off
//! (`IF … THEN`, `FOREACH [cursor FOR]`, `LET v =`, `RETURN`, `SELECT … INTO vars`); a statement
//! that does not parse is counted in `footprint_errors`, never guessed. Every routine says what is
//! missing (`coverage_gap`), and the pass reports how many there are. A trigger's action becomes a
//! `Procedure` named after it plus a `Trigger` whose `function` names it — RFC 0210's shape for an
//! Oracle trigger.
//!
//! Pure; no connection, no LLM.

use crate::plpgsql_analyzer::{PlPgSqlStats, routine_text_hash, set_footprint};
use crate::plpgsql_footprint::{Footprint, statement_footprint_in};
use crate::plsql_analyzer::PlSqlStats;
use crate::sql_objects::clip;
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use serde_json::{Value, json};
use sqlparser::dialect::GenericDialect;
use uuid::Uuid;

const MAX_FRAGMENT: usize = 4096;

/// Bump when this module's output changes for the same catalog; the catalog pass folds it into
/// its cache key.
pub const LOGIC_VERSION: &str = "spl-routines/1";

/// What every Informix routine says it lacks.
pub const COVERAGE_GAP: &str = "no Informix SPL reader (sql-dialect-informix): the routine's \
     statements are not recovered as IR; its footprint comes from the SQL statements inside it";

fn id(seed: String) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'$')
}

/// The index just past the next `end` at or after `from` (the end of the text when there is none).
fn skip_until(bytes: &[u8], from: usize, end: &[u8]) -> usize {
    let mut i = from;
    while i + end.len() <= bytes.len() {
        if &bytes[i..i + end.len()] == end {
            return i + end.len();
        }
        i += 1;
    }
    bytes.len()
}

/// Byte offsets of `;` (or `,` when `commas`) at depth 0, outside strings, quoted names and
/// comments (`--`, `{ … }`, `/* … */`).
fn top_level(text: &str, sep: u8) -> Vec<usize> {
    let bytes = text.as_bytes();
    let (mut out, mut depth, mut i) = (Vec::new(), 0i32, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_until(bytes, i + 1, b"'"),
            b'"' => i = skip_until(bytes, i + 1, b"\""),
            b'{' => i = skip_until(bytes, i + 1, b"}"),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_until(bytes, i + 2, b"\n"),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_until(bytes, i + 2, b"*/"),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                i += 1;
            }
            b if b == sep && depth == 0 => {
                out.push(i);
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn split_at(text: &str, sep: u8) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for at in top_level(text, sep) {
        parts.push(&text[start..at]);
        start = at + 1;
    }
    parts.push(&text[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect()
}

/// The first `(`…`)` group at or after `from`: (open, close) byte offsets.
fn group(text: &str, from: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let open = from + text[from..].find('(')?;
    let mut depth = 0i32;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => {
                i = skip_until(bytes, i + 1, b"'");
                continue;
            }
            b'"' => {
                i = skip_until(bytes, i + 1, b"\"");
                continue;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, i));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The position of `word` as a whole word at depth 0, case-insensitive.
fn find_word(text: &str, word: &str) -> Option<usize> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let w = word.to_ascii_lowercase();
    let mut from = 0;
    while let Some(rel) = lower[from..].find(&w) {
        let at = from + rel;
        let before_ok = at == 0 || !is_word(bytes[at - 1]);
        let after_ok = bytes.get(at + w.len()).is_none_or(|b| !is_word(*b));
        if before_ok && after_ok {
            return Some(at);
        }
        from = at + w.len();
    }
    None
}

/// The first word of `s`, lower-cased, and the rest.
fn first_word(s: &str) -> (String, &str) {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    (s[..end].to_ascii_lowercase(), s[end..].trim_start())
}

/// An SPL routine's header: (arguments, return types, body).
pub fn split_routine(text: &str) -> (Vec<String>, Option<String>, &str) {
    let Some((open, close)) = group(text, 0) else {
        return (Vec::new(), None, "");
    };
    let arguments = split_at(&text[open + 1..close], b',')
        .into_iter()
        .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect();
    let mut rest = &text[close + 1..];
    let mut returns = None;
    let (w, after) = first_word(rest);
    if w == "returning" || w == "returns" {
        let end = top_level(after, b';')
            .first()
            .copied()
            .unwrap_or(after.len());
        returns = Some(
            after[..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        );
        rest = after.get(end + 1..).unwrap_or_default();
    }
    // The body ends at `END PROCEDURE` / `END FUNCTION`; a `DOCUMENT` clause follows it.
    let lower = rest.to_ascii_lowercase();
    let end = ["end procedure", "end function"]
        .iter()
        .filter_map(|e| lower.rfind(e))
        .max()
        .unwrap_or(rest.len());
    (arguments, returns, &rest[..end])
}

/// SPL's own words before a statement, removed: what is left is SQL or nothing.
fn strip_spl(fragment: &str) -> Option<String> {
    let mut f = fragment.trim();
    loop {
        let (w, rest) = first_word(f);
        match w.as_str() {
            // Block ends and else-branches: the statement follows them.
            "end" => {
                let (next, after) = first_word(rest);
                f = if matches!(
                    next.as_str(),
                    "if" | "for" | "foreach" | "while" | "loop" | "exception" | "case"
                ) {
                    after
                } else {
                    rest
                };
            }
            "else" | "begin" => f = rest,
            "if" | "elif" | "when" => {
                f = &rest[find_word(rest, "then")? + 4..];
            }
            "foreach" => {
                // `FOREACH [cursor [WITH HOLD] FOR] SELECT …` or `FOREACH EXECUTE …`.
                f = match find_word(rest, "for") {
                    Some(at)
                        if first_word(rest).0 != "select" && first_word(rest).0 != "execute" =>
                    {
                        &rest[at + 3..]
                    }
                    _ => rest,
                };
            }
            "define" | "on" | "raise" | "system" | "trace" | "continue" | "exit" | "loop" => {
                return None;
            }
            _ => break,
        }
        if f.trim().is_empty() {
            return None;
        }
    }
    let (w, rest) = first_word(f);
    Some(match w.as_str() {
        "" => return None,
        "let" => format!("SELECT {}", rest.split_once('=')?.1.trim()),
        "return" if rest.is_empty() => return None,
        "return" => {
            let rest = rest.trim_end();
            let rest = rest.strip_suffix("with resume").unwrap_or(rest);
            format!("SELECT {rest}")
        }
        "select" => match (find_word(f, "into"), find_word(f, "from")) {
            // `SELECT a INTO v FROM t`: the variables are not a table.
            (Some(into), Some(from)) if into < from => format!("{}{}", &f[..into], &f[from..]),
            _ => f.to_string(),
        },
        _ => f.to_string(),
    })
}

/// `EXECUTE PROCEDURE p(…)`, `EXECUTE FUNCTION f(…)`, `CALL p(…)`: the routine called.
fn called(statement: &str) -> Option<String> {
    let (w, rest) = first_word(statement);
    let rest = match w.as_str() {
        "execute" => {
            let (kind, rest) = first_word(rest);
            if kind != "procedure" && kind != "function" {
                return None;
            }
            rest
        }
        "call" => rest,
        _ => return None,
    };
    let name = rest.split('(').next()?.trim().replace('"', "");
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Words that start a statement and never occur inside a `SELECT`.
const STATEMENT_WORDS: &[&str] = &[
    "execute", "insert", "delete", "let", "call", "return", "if", "foreach", "merge",
];

/// A `FOREACH` cursor's `SELECT` runs straight into the loop's first statement with no `;`
/// between them: (the select, the rest), cut at the first word that starts a statement — `UPDATE`
/// too, unless it is the cursor's `FOR UPDATE`.
fn split_cursor_select(sql: &str) -> (&str, Option<&str>) {
    let from = find_word(sql, "from").unwrap_or(0);
    let tail = &sql[from..];
    let mut cut = STATEMENT_WORDS
        .iter()
        .filter_map(|w| find_word(tail, w))
        .min();
    if let Some(u) = find_word(tail, "update") {
        let for_update = tail[..u].trim_end().to_ascii_lowercase().ends_with("for");
        if !for_update {
            cut = Some(cut.map_or(u, |c| c.min(u)));
        }
    }
    match cut {
        Some(c) => (sql[..from + c].trim(), Some(&sql[from + c..])),
        None => (sql, None),
    }
}

fn add_statement(fp: &mut Footprint, d: &GenericDialect, fragment: &str, depth: usize) {
    let Some(sql) = strip_spl(fragment) else {
        return;
    };
    if let Some(name) = called(&sql) {
        fp.calls.insert(name);
        fp.fragments += 1;
        return;
    }
    let (statement, rest) = if first_word(&sql).0 == "select" {
        split_cursor_select(&sql)
    } else {
        (sql.as_str(), None)
    };
    fp.merge(statement_footprint_in(d, statement));
    if let Some(rest) = rest.filter(|_| depth < 8) {
        add_statement(fp, d, rest, depth + 1);
    }
}

/// What an SPL body touches: each `;`-separated statement, SPL's words taken off.
pub fn footprint(body: &str) -> Footprint {
    let d = GenericDialect {};
    let mut fp = Footprint::default();
    for fragment in split_at(body, b';') {
        add_statement(&mut fp, &d, fragment, 0);
    }
    fp
}

/// A trigger's action list: `[FOR EACH ROW | BEFORE | AFTER] [WHEN (cond)] (stmt, stmt …)`; each
/// statement of each list.
pub fn trigger_statements(action: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some((open, close)) = group(action, at) {
        let inner = &action[open + 1..close];
        let head = first_word(inner).0;
        let starts = |w: &str| {
            matches!(
                w,
                "insert" | "update" | "delete" | "select" | "execute" | "call" | "merge"
            )
        };
        if starts(&head) {
            // Statements are separated by commas, and an `UPDATE … SET a = 1, b = 2` has its own:
            // a piece that does not start a statement belongs to the one before it.
            let mut list: Vec<String> = Vec::new();
            for piece in split_at(inner, b',') {
                match list.last_mut() {
                    Some(prev) if !starts(&first_word(piece).0) => {
                        prev.push_str(", ");
                        prev.push_str(piece);
                    }
                    _ => list.push(piece.to_string()),
                }
            }
            out.extend(list);
        }
        at = close + 1;
    }
    out
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// One routine or trigger from the catalog: `kind` is `procedure`, `function` or `trigger`;
/// `detail` is the observer's (`text`, `comment`, and for a trigger `table`, `events`, `timing`).
pub fn recover_routine(
    target: &str,
    kind: &str,
    name: &str,
    detail: &Value,
) -> (KirGraph, PlSqlStats) {
    let mut graph = KirGraph::new();
    let mut stats = PlSqlStats {
        routines: PlPgSqlStats {
            routines: 1,
            ..Default::default()
        },
        ..Default::default()
    };
    let text = detail["text"].as_str().unwrap_or_default();
    let lines = text.lines().count().max(1);
    let (arguments, returns, body, fp) = if kind == "trigger" {
        let action = detail["action"].as_str().unwrap_or(text);
        let mut fp = Footprint::default();
        for s in trigger_statements(action) {
            match called(&s) {
                Some(n) => {
                    fp.calls.insert(n);
                }
                None => fp.merge(statement_footprint_in(&GenericDialect {}, &s)),
            }
        }
        (Vec::new(), Some("trigger".to_string()), action, fp)
    } else {
        let (arguments, returns, body) = split_routine(text);
        (arguments, returns, body, footprint(body))
    };
    let header = text
        .get(..text.len() - body.len().min(text.len()))
        .unwrap_or(text);
    let header = if header.trim().is_empty() {
        text
    } else {
        header
    };
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: target.to_string(),
            line: Some(1),
            column: None,
        },
        clip(header.trim_end(), MAX_FRAGMENT),
    );
    ev.id = id(format!("spl:procedure-evidence:{target}"));
    let ev = graph.add_evidence(ev);
    let language = if kind == "trigger" { "informix" } else { "spl" };
    let mut obj = KirObject::new(name, ObjectKind::Custom("Procedure".into()))
        .with_property("language", json!(language))
        .with_property("arguments", json!(arguments))
        .with_property("returns", returns.map_or(Value::Null, |r| json!(r)))
        .with_property("fidelity", json!("signature"))
        .with_property("ir", json!(false))
        .with_property("coverage_gap", json!(COVERAGE_GAP))
        .with_property("statements_recovered", json!(0))
        .with_property("statements_unrecovered", json!(0))
        .with_property("source_path", json!(target))
        .with_property("text_hash", json!(routine_text_hash(text)))
        .with_property("line", json!(1))
        .with_property("span_start", json!(0))
        .with_property("span_end", json!(text.len()))
        .with_property("source_span", json!({"start_line": 1, "end_line": lines}));
    obj.id = id(format!("spl:procedure:{target}"));
    obj.evidence.push(ev);
    if let Some(c) = detail["comment"].as_str() {
        obj.properties.insert("description".into(), json!(c));
    }
    if text.trim().is_empty() {
        obj.properties.insert("text_missing".into(), json!(true));
    }
    set_footprint(&mut obj.properties, &fp);
    obj.properties
        .insert("footprint_fragments".into(), json!(fp.fragments));
    obj.properties
        .insert("footprint_errors".into(), json!(fp.errors.len()));
    graph.add_object(obj);
    if kind == "trigger" {
        let level = if find_word(text, "each").is_some() {
            "ROW"
        } else {
            "STATEMENT"
        };
        let mut t = KirObject::new(name, ObjectKind::Custom("Trigger".into()));
        t.id = id(format!("spl:trigger:{target}"));
        for (k, v) in [
            ("table", detail["table"].clone()),
            ("function", json!(name)),
            ("timing", detail["timing"].clone()),
            ("events", json!(strings(&detail["events"]))),
            ("level", json!(level)),
            ("condition", Value::Null),
            ("constraint", json!(false)),
            ("parsed", json!(true)),
            ("language", json!("informix")),
            ("source_path", json!(target)),
            ("line", json!(1)),
            ("source_span", json!({"start_line": 1, "end_line": lines})),
        ] {
            t.properties.insert(k.into(), v);
        }
        t.evidence.push(ev);
        graph.add_object(t);
        stats.triggers += 1;
    }
    (graph, stats)
}

/// A view's definition as RFC 0169's reader takes it: `CREATE VIEW "<name>" AS <query>`, the
/// query being what follows the stored statement's top-level `AS` (after the column list).
pub fn view_sql(name: &str, definition: &str) -> Option<String> {
    let at = crate::tsql_routines::body_start(definition)?;
    let query = definition[at..].trim().trim_end_matches(';').trim();
    if query.is_empty() {
        return None;
    }
    Some(format!(
        "CREATE VIEW \"{}\" AS {query}\n;\n",
        name.replace('"', "\"\"")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop<'a>(g: &'a KirGraph, kind: &str, key: &str) -> &'a Value {
        &g.objects
            .iter()
            .find(|o| o.kind == ObjectKind::Custom(kind.into()))
            .unwrap()
            .properties[key]
    }

    const PROC: &str = "CREATE PROCEDURE \"sales\".close_order(p_id INT8, p_note LVARCHAR(200))\n    UPDATE \"sales\".orders SET status = 9 WHERE id = p_id;\n    INSERT INTO \"sales\".order_log (order_id) VALUES (p_id);\nEND PROCEDURE\nDOCUMENT 'Closes one order and logs it';";

    const FUNC: &str = "CREATE FUNCTION \"sales\".order_total(p_id INT8) RETURNING DECIMAL(12, 2);\n    DEFINE t DECIMAL(12, 2);\n    SELECT SUM(l.quantity * p.price) INTO t\n      FROM \"sales\".order_lines l, \"sales\".products p\n     WHERE p.sku = l.sku AND l.order_id = p_id;\n    IF t IS NULL THEN LET t = (SELECT MAX(price) FROM \"sales\".products); END IF;\n    FOREACH c1 FOR SELECT id INTO p_id FROM \"sales\".order_log\n        EXECUTE PROCEDURE \"sales\".close_order(p_id);\n    END FOREACH;\n    RETURN t;\nEND FUNCTION;";

    #[test]
    fn the_header_gives_arguments_and_return_types() {
        let (args, ret, body) = split_routine(PROC);
        assert_eq!(args, ["p_id INT8", "p_note LVARCHAR(200)"]);
        assert_eq!(ret, None);
        assert!(body.trim_start().starts_with("UPDATE"), "{body}");
        assert!(!body.to_lowercase().contains("document"));
        let (args, ret, body) = split_routine(FUNC);
        assert_eq!(args, ["p_id INT8"]);
        assert_eq!(ret.as_deref(), Some("DECIMAL(12, 2)"));
        assert!(body.trim_start().starts_with("DEFINE"), "{body}");
    }

    #[test]
    fn a_procedure_reads_and_writes_what_its_statements_name() {
        let (g, stats) = recover_routine(
            "informix://shop/sales/procedure/close_order",
            "procedure",
            "close_order",
            &json!({ "text": PROC, "comment": "Closes one order and logs it", "language": "spl" }),
        );
        assert_eq!(stats.routines.routines, 1);
        let writes = prop(&g, "Procedure", "writes").to_string();
        assert!(
            writes.contains("orders") && writes.contains("order_log"),
            "{writes}"
        );
        assert_eq!(prop(&g, "Procedure", "ir"), false);
        assert_eq!(prop(&g, "Procedure", "fidelity"), "signature");
        assert_eq!(prop(&g, "Procedure", "coverage_gap"), COVERAGE_GAP);
        assert_eq!(
            prop(&g, "Procedure", "description"),
            "Closes one order and logs it"
        );
        assert_eq!(prop(&g, "Procedure", "footprint_errors"), 0);
    }

    #[test]
    fn spl_words_are_taken_off_and_select_into_is_a_read() {
        let (_, _, body) = split_routine(FUNC);
        let fp = footprint(body);
        assert!(fp.errors.is_empty(), "{:?}", fp.errors);
        assert!(
            fp.reads.iter().any(|r| r.ends_with("order_lines")),
            "{:?}",
            fp.reads
        );
        assert!(
            fp.reads.iter().any(|r| r.ends_with("products")),
            "{:?}",
            fp.reads
        );
        assert!(
            fp.reads.iter().any(|r| r.ends_with("order_log")),
            "{:?}",
            fp.reads
        );
        assert!(
            fp.writes.is_empty(),
            "a variable is not a table: {:?}",
            fp.writes
        );
        assert!(
            fp.calls.iter().any(|c| c.ends_with("close_order")),
            "{:?}",
            fp.calls
        );
    }

    #[test]
    fn a_trigger_is_an_action_and_a_binding() {
        let text = "create trigger \"sales\".orders_audit insert on \"sales\".orders referencing new as n\nfor each row\n        (\n        insert into \"sales\".order_log (order_id)  values (n.id ), execute procedure \"sales\".close_order(n.id));";
        let (g, stats) = recover_routine(
            "informix://shop/sales/trigger/orders_audit",
            "trigger",
            "orders_audit",
            &json!({ "text": text, "table": "orders", "events": ["INSERT"], "timing": "AFTER" }),
        );
        assert_eq!(stats.triggers, 1);
        assert_eq!(prop(&g, "Trigger", "function"), "orders_audit");
        assert_eq!(prop(&g, "Trigger", "level"), "ROW");
        assert_eq!(prop(&g, "Procedure", "returns"), "trigger");
        assert!(
            prop(&g, "Procedure", "writes")
                .to_string()
                .contains("order_log")
        );
        assert!(
            prop(&g, "Procedure", "calls")
                .to_string()
                .contains("close_order")
        );
        assert_eq!(
            trigger_statements("when (n.status > 1) (update t set a = 1, b = 2)"),
            ["update t set a = 1, b = 2"],
            "a comma inside a statement does not split it"
        );
    }

    #[test]
    fn a_view_keeps_its_query_and_takes_the_catalog_name() {
        let v = view_sql(
            "big_orders",
            "create view \"sales\".big_orders (id,customer_id,total) as select x0.id ,x0.customer_id from \"sales\".orders x0 ;",
        )
        .unwrap();
        assert_eq!(
            v,
            "CREATE VIEW \"big_orders\" AS select x0.id ,x0.customer_id from \"sales\".orders x0\n;\n"
        );
    }
}
