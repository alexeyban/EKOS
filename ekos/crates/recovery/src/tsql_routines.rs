//! RFC 0252 §3.5 — SQL Server views, routines and triggers from the live catalog
//! (`sys.sql_modules`), for `OracleCatalogPass` under `CatalogDialect::SqlServer`.
//!
//! There is no T-SQL procedural reader (no `ProcedureIr` front end), so a T-SQL routine is
//! recovered at **signature** fidelity: one `Procedure` with its arguments, return type and
//! description, and its footprint (`reads` / `writes` / `calls`) from a real `sqlparser` parse in
//! the MS SQL dialect — the whole definition, else its body after the header, else each
//! `;`-separated fragment, whichever parses; what does not parse is reported in
//! `footprint_errors`, never guessed. No `ProcedureStatement` objects are written. A trigger's
//! body becomes a `Procedure` named after it and a `Trigger` whose `function` names that body, the
//! shape RFC 0210 gives an Oracle trigger, so `semantic::triggers` links both alike.
//!
//! Pure; no connection, no LLM.

use crate::plpgsql_analyzer::{PlPgSqlStats, routine_text_hash, set_footprint};
use crate::plpgsql_footprint::{Footprint, statement_footprint_in};
use crate::plsql_analyzer::PlSqlStats;
use crate::sql_objects::clip;
use ekos_kir::{KirEvidence, KirGraph, KirId, KirObject, ObjectKind, SourceLocation};
use serde_json::{Value, json};
use sqlparser::ast::Statement;
use sqlparser::dialect::MsSqlDialect;
use sqlparser::parser::Parser;
use uuid::Uuid;

const MAX_FRAGMENT: usize = 4096;

/// Bump when this module's output changes for the same catalog; the catalog pass folds it into
/// its cache key.
pub const LOGIC_VERSION: &str = "tsql-routines/2";

fn id(seed: String) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()))
}

/// The byte offset just past the first top-level `AS` keyword after the object's name: where a
/// view's query or a routine's body starts. Strings, bracketed names and comments are skipped.
pub fn body_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut depth = 0i32;
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_until(bytes, i + 1, b"'"),
            b'[' => i = skip_until(bytes, i + 1, b"]"),
            b'"' => i = skip_until(bytes, i + 1, b"\""),
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
            b'a' | b'A'
                if depth == 0
                    && bytes
                        .get(i + 1)
                        .is_some_and(|b| b.eq_ignore_ascii_case(&b's'))
                    && (i == 0 || !is_word(bytes[i - 1]))
                    && bytes.get(i + 2).is_none_or(|b| !is_word(*b)) =>
            {
                // `WITH EXECUTE AS OWNER` is a routine option, not the body's start.
                let before = text[..i].trim_end().to_ascii_lowercase();
                if !(before.ends_with("execute") || before.ends_with("exec")) {
                    return Some(i + 2);
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    None
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'@' | b'#' | b'$')
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

/// A view's definition as RFC 0169's reader takes it: `CREATE VIEW "<name>" AS <query>`, the
/// query taken from the parsed statement, else from the text after the header's `AS`
/// (`WITH SCHEMABINDING` and other view options are dropped with the header).
pub fn view_sql(name: &str, definition: &str) -> Option<String> {
    let query = match Parser::parse_sql(&MsSqlDialect {}, definition)
        .ok()
        .as_deref()
    {
        Some([Statement::CreateView { query, .. }]) => query.to_string(),
        _ => {
            let at = body_start(definition)?;
            definition[at..].trim().trim_end_matches(';').to_string()
        }
    };
    if query.is_empty() {
        return None;
    }
    Some(format!(
        "CREATE VIEW \"{}\" AS {query}\n;\n",
        name.replace('"', "\"\"")
    ))
}

/// What a T-SQL routine touches: the whole `CREATE` statement parsed, else the body after the
/// header, else each `;`-separated fragment of the body.
pub fn footprint(text: &str) -> Footprint {
    let d = MsSqlDialect {};
    let whole = statement_footprint_in(&d, text);
    if whole.errors.is_empty() {
        return whole;
    }
    let Some(at) = body_start(text) else {
        return whole;
    };
    let body = text[at..].trim();
    let body = strip_block(body);
    let as_one = statement_footprint_in(&d, body);
    if as_one.errors.is_empty() {
        return as_one;
    }
    let mut fp = Footprint::default();
    for fragment in split_statements(body) {
        if let Some(f) = fragment_footprint(&d, fragment) {
            fp.merge(f);
        }
    }
    fp
}

/// The body split at `;` outside strings, bracketed names and comments.
fn split_statements(body: &str) -> Vec<&str> {
    let bytes = body.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut i) = (0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' => i = skip_until(bytes, i + 1, b"'"),
            b'[' => i = skip_until(bytes, i + 1, b"]"),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_until(bytes, i + 2, b"\n"),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_until(bytes, i + 2, b"*/"),
            b';' => {
                out.push(&body[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push(&body[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect()
}

/// The first word of `s`, upper-cased, and the rest.
fn first_word(s: &str) -> (String, &str) {
    let s = s.trim_start();
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '@'))
        .unwrap_or(s.len());
    (s[..end].to_ascii_uppercase(), s[end..].trim_start())
}

/// One fragment's footprint, `None` for a fragment that touches nothing by construction (a
/// session option, a bare `RETURN`, a declaration without a value). Block keywords around it are
/// dropped; a value — `RETURN <expr>`, `SET @v = <expr>`, `DECLARE @v <type> = <expr>` — is read
/// as `SELECT <expr>`, the way T-SQL evaluates it.
fn fragment_footprint(d: &MsSqlDialect, fragment: &str) -> Option<Footprint> {
    let mut f = fragment.trim();
    loop {
        let (word, rest) = first_word(f);
        match word.as_str() {
            "BEGIN" if !rest.to_ascii_uppercase().starts_with("TRAN") => f = rest,
            "END" => f = rest,
            _ => break,
        }
    }
    let lower = f.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix("end") {
        // A trailing block end, not part of the statement (`… WHERE id = @id END`).
        if stripped.ends_with(char::is_whitespace) {
            f = f[..stripped.len()].trim_end();
        }
    }
    let (word, rest) = first_word(f);
    let value = |expr: &str| statement_footprint_in(d, &format!("SELECT {expr}"));
    match word.as_str() {
        "" => None,
        "RETURN" if rest.is_empty() => None,
        "RETURN" => Some(value(rest)),
        "SET" => {
            let (target, tail) = first_word(rest);
            if !target.starts_with('@') {
                // `SET NOCOUNT ON`, `SET XACT_ABORT ON`: a session option.
                return None;
            }
            tail.strip_prefix('=').map(|expr| value(expr.trim()))
        }
        "DECLARE" => rest.split_once('=').map(|(_, expr)| value(expr.trim())),
        _ => Some(statement_footprint_in(d, f)),
    }
}

/// `BEGIN … END` around a whole body, removed.
fn strip_block(body: &str) -> &str {
    let lower = body.to_ascii_lowercase();
    if lower.starts_with("begin")
        && lower.trim_end_matches(';').trim_end().ends_with("end")
        && !lower.starts_with("begin tran")
    {
        let inner = &body[5..];
        let end = inner.trim_end().trim_end_matches(';').trim_end();
        return end[..end.len() - 3].trim();
    }
    body
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
/// `detail` is the observer's (`text`, `arguments`, `returns`, `comment`, `encrypted`, and for a
/// trigger `table`, `timing`, `events`).
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
    let header = body_start(text).map_or(text, |at| &text[..at]);
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: target.to_string(),
            line: Some(1),
            column: None,
        },
        clip(header.trim_end(), MAX_FRAGMENT),
    );
    ev.id = id(format!("tsql:procedure-evidence:{target}"));
    let ev = graph.add_evidence(ev);
    let mut obj = KirObject::new(name, ObjectKind::Custom("Procedure".into()))
        .with_property("language", json!("tsql"))
        .with_property("arguments", json!(strings(&detail["arguments"])))
        .with_property(
            "returns",
            if kind == "trigger" {
                json!("trigger")
            } else {
                detail.get("returns").cloned().unwrap_or(Value::Null)
            },
        )
        // No T-SQL procedural reader: the routine's statements are not recovered.
        .with_property("fidelity", json!("signature"))
        .with_property("statements_recovered", json!(0))
        .with_property("statements_unrecovered", json!(0))
        .with_property("source_path", json!(target))
        .with_property("text_hash", json!(routine_text_hash(text)))
        .with_property("line", json!(1))
        .with_property("span_start", json!(0))
        .with_property("span_end", json!(text.len()))
        .with_property("source_span", json!({"start_line": 1, "end_line": lines}));
    obj.id = id(format!("tsql:procedure:{target}"));
    obj.evidence.push(ev);
    if let Some(c) = detail["comment"].as_str() {
        obj.properties.insert("description".into(), json!(c));
    }
    let fp = if detail["encrypted"] == true || text.trim().is_empty() {
        obj.properties.insert("encrypted".into(), json!(true));
        Footprint::default()
    } else {
        let mut fp = footprint(text);
        if kind == "trigger" {
            // `inserted` / `deleted` are a T-SQL trigger's pseudo-tables, not relations.
            let pseudo =
                |t: &String| matches!(t.to_ascii_lowercase().as_str(), "inserted" | "deleted");
            fp.reads.retain(|t| !pseudo(t));
            fp.writes.retain(|t| !pseudo(t));
        }
        fp
    };
    set_footprint(&mut obj.properties, &fp);
    obj.properties
        .insert("footprint_fragments".into(), json!(fp.fragments));
    graph.add_object(obj);
    if kind == "trigger" {
        let mut t = KirObject::new(name, ObjectKind::Custom("Trigger".into()));
        t.id = id(format!("tsql:trigger:{target}"));
        for (k, v) in [
            ("table", detail["table"].clone()),
            ("function", json!(name)),
            ("timing", detail["timing"].clone()),
            ("events", json!(strings(&detail["events"]))),
            // A T-SQL DML trigger fires once per statement, over `inserted` / `deleted`.
            ("level", json!("STATEMENT")),
            ("condition", Value::Null),
            ("constraint", json!(false)),
            ("parsed", json!(true)),
            ("language", json!("tsql")),
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

    #[test]
    fn the_body_starts_after_the_header_as() {
        let t = "CREATE PROCEDURE [dbo].[p as q] @a int = 1 /* as */ AS\nBEGIN SELECT 1 END";
        assert_eq!(&t[body_start(t).unwrap()..], "\nBEGIN SELECT 1 END");
        let f = "CREATE FUNCTION f(@x int) RETURNS TABLE AS RETURN (SELECT @x AS y)";
        assert_eq!(&f[body_start(f).unwrap()..], " RETURN (SELECT @x AS y)");
        let e = "CREATE PROCEDURE p WITH EXECUTE AS OWNER AS SELECT 1";
        assert_eq!(&e[body_start(e).unwrap()..], " SELECT 1");
    }

    #[test]
    fn a_view_keeps_its_query_and_takes_the_catalog_name() {
        let v = view_sql(
            "big_orders",
            "CREATE VIEW sales.big_orders AS SELECT id FROM sales.orders WHERE total > 100",
        )
        .unwrap();
        assert!(
            v.starts_with("CREATE VIEW \"big_orders\" AS SELECT id FROM sales.orders"),
            "{v}"
        );
        let s = view_sql(
            "v",
            "CREATE VIEW dbo.v WITH SCHEMABINDING AS SELECT a FROM dbo.t",
        )
        .unwrap();
        assert!(s.contains("AS SELECT a FROM dbo.t"), "{s}");
    }

    #[test]
    fn a_procedure_reads_and_writes_what_its_statements_name() {
        let detail = json!({
            "text": "CREATE PROCEDURE sales.close_order @id bigint AS\nBEGIN\n  SET NOCOUNT ON;\n  UPDATE sales.orders SET note = 'closed' WHERE id = @id;\n  INSERT INTO sales.order_log (order_id) SELECT id FROM sales.orders WHERE id = @id;\nEND",
            "arguments": ["@id bigint"],
            "comment": "Closes one order",
        });
        let (g, stats) = recover_routine(
            "sqlserver://erp/sales/procedure/close_order",
            "procedure",
            "close_order",
            &detail,
        );
        assert_eq!(stats.routines.routines, 1);
        let reads = prop(&g, "Procedure", "reads").to_string();
        let writes = prop(&g, "Procedure", "writes").to_string();
        assert!(
            writes.contains("orders") && writes.contains("order_log"),
            "{writes}"
        );
        assert!(reads.contains("orders"), "{reads}");
        assert_eq!(prop(&g, "Procedure", "fidelity"), "signature");
        assert_eq!(prop(&g, "Procedure", "description"), "Closes one order");
        assert_eq!(prop(&g, "Procedure", "language"), "tsql");
    }

    #[test]
    fn session_options_and_block_keywords_are_not_parse_failures() {
        let p = footprint(
            "CREATE PROCEDURE sales.cancel_order @order_id bigint\nAS\nBEGIN\n    SET NOCOUNT ON;\n    UPDATE sales.orders SET status = 9 WHERE id = @order_id;\n    DELETE FROM sales.order_lines WHERE order_id = @order_id;\nEND;",
        );
        assert_eq!(p.status(), "parsed", "{:?}", p.errors);
        assert_eq!(p.writes.len(), 2, "{:?}", p.writes);
        let f = footprint(
            "CREATE FUNCTION sales.order_total (@order_id bigint)\nRETURNS decimal(12, 2)\nAS\nBEGIN\n    RETURN (\n        SELECT SUM(l.quantity * p.price)\n        FROM sales.order_lines l JOIN sales.products p ON p.sku = l.sku\n        WHERE l.order_id = @order_id\n    );\nEND;",
        );
        assert_eq!(f.status(), "parsed", "{:?}", f.errors);
        assert!(
            f.reads.iter().any(|r| r.ends_with("order_lines")),
            "{:?}",
            f.reads
        );
        assert!(
            f.reads.iter().any(|r| r.ends_with("products")),
            "{:?}",
            f.reads
        );
        let v = footprint(
            "CREATE PROCEDURE p AS BEGIN DECLARE @n int = (SELECT COUNT(*) FROM dbo.t); SET @n = (SELECT MAX(a) FROM dbo.u); SELECT 'a;b' AS s END",
        );
        assert_eq!(v.status(), "parsed", "{:?}", v.errors);
        assert!(
            v.reads.contains("dbo.t") && v.reads.contains("dbo.u"),
            "{:?}",
            v.reads
        );
    }

    #[test]
    fn a_trigger_is_a_body_and_a_binding() {
        let detail = json!({
            "text": "CREATE TRIGGER sales.orders_audit ON sales.orders AFTER INSERT AS\nINSERT INTO sales.order_log (order_id) SELECT id FROM inserted",
            "table": "orders", "timing": "AFTER", "events": ["INSERT"],
        });
        let (g, stats) = recover_routine(
            "sqlserver://erp/sales/trigger/orders_audit",
            "trigger",
            "orders_audit",
            &detail,
        );
        assert_eq!(stats.triggers, 1);
        assert_eq!(prop(&g, "Trigger", "function"), "orders_audit");
        assert_eq!(prop(&g, "Trigger", "table"), "orders");
        assert_eq!(prop(&g, "Procedure", "returns"), "trigger");
        assert!(
            prop(&g, "Procedure", "writes")
                .to_string()
                .contains("order_log")
        );
        assert!(
            !prop(&g, "Procedure", "reads")
                .to_string()
                .contains("inserted"),
            "a pseudo-table is not a relation"
        );
    }

    #[test]
    fn an_encrypted_routine_has_a_signature_and_no_footprint() {
        let (g, _) = recover_routine(
            "t",
            "procedure",
            "p",
            &json!({ "text": "", "encrypted": true }),
        );
        assert_eq!(prop(&g, "Procedure", "encrypted"), true);
        assert_eq!(prop(&g, "Procedure", "footprint"), "none");
    }
}
