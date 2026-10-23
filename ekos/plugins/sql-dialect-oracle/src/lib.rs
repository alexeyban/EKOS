//! Oracle `SqlDialectParser` (RFC 0210).
//!
//! `sqlparser` 0.53 has no Oracle dialect. Its `GenericDialect` already parses most of what Oracle
//! schemas and migration scripts hold — `NUMBER(10)`, `VARCHAR2(36 CHAR)`, `DECODE`/`NVL`/`NVL2`,
//! `ROWNUM`, `FROM dual`, `TO_DATE`, `CONNECT BY`, `MERGE`, `MODIFY col type`, `DEFERRABLE
//! INITIALLY DEFERRED` in either order (measured, RFC 0210 §3.1). What it cannot read,
//! [`OracleDialectParser::preprocess`] rewrites or blanks:
//!
//! * the script layer: SQL\*Plus commands, `/` lines and whole PL/SQL units (those are read by the
//!   PL/SQL front end, never by the DDL parser), split by [`ekos_plsql::script`];
//! * Oracle-only quoting: `q'[…]'` and a backslash inside a literal;
//! * a short list of Oracle-only clauses — constraint states (`ENABLE NOVALIDATE`), column lists in
//!   `ALTER TABLE … ADD (…)`, `BYTE` length semantics, `FORCE` views, `CASCADE CONSTRAINTS` /
//!   `PURGE`, sequence options, `ALTER … COMPILE`, physical attributes (`TABLESPACE`, `STORAGE`,
//!   `USING INDEX`, …), the `(+)` outer-join marker and `MINUS`.
//!
//! Blanking replaces every character except line breaks with a space, and no rewrite adds a line,
//! so diagnostics and evidence keep the file's own line numbers (the RFC 0176 rule).

use ekos_plsql::script::{
    Tok, TokKind, Unit, UnitKind, matching_paren, neutralize_quotes, tokens, units,
};
use ekos_sql_dialect_sdk::SqlDialectParser;
use sqlparser::dialect::{Dialect, GenericDialect};

pub struct OracleDialectParser;

impl SqlDialectParser for OracleDialectParser {
    fn name(&self) -> &str {
        "oracle"
    }

    fn sqlparser_dialect(&self) -> Box<dyn Dialect + Send + Sync> {
        Box::new(GenericDialect {})
    }

    fn preprocess(&self, sql: &str) -> String {
        preprocess(sql)
    }
}

/// The whole script, made readable by `GenericDialect`. See the module docs.
pub fn preprocess(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut at = 0;
    for unit in units(sql) {
        out.push_str(&sql[at..unit.start]);
        out.push_str(&rewrite_unit(sql, &unit));
        at = unit.end;
    }
    out.push_str(&sql[at..]);
    out
}

fn rewrite_unit(sql: &str, unit: &Unit) -> String {
    let text = &sql[unit.start..unit.end];
    match unit.kind {
        UnitKind::SqlPlus | UnitKind::PlSql => blanked(text),
        UnitKind::Statement => rewrite_statement(&neutralize_quotes(text)),
    }
}

/// `text` with every character but a line break replaced by a space.
fn blanked(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { c } else { ' ' })
        .collect()
}

/// Replacements over one statement, by byte range. The first edit claiming a range wins.
#[derive(Default)]
struct Edits(Vec<(usize, usize, Option<String>)>);

impl Edits {
    fn blank(&mut self, start: usize, end: usize) {
        self.0.push((start, end, None));
    }

    fn blank_tok(&mut self, t: &Tok) {
        self.blank(t.start, t.end);
    }

    fn replace(&mut self, start: usize, end: usize, with: impl Into<String>) {
        self.0.push((start, end, Some(with.into())));
    }

    fn apply(mut self, text: &str) -> String {
        self.0.sort_by_key(|(s, e, _)| (*s, *e));
        let mut out = String::with_capacity(text.len() + 16);
        let mut at = 0;
        for (s, e, with) in self.0 {
            if s < at {
                continue;
            }
            out.push_str(&text[at..s]);
            match with {
                Some(w) => out.push_str(&w),
                None => out.push_str(&blanked(&text[s..e])),
            }
            at = e;
        }
        out.push_str(&text[at..]);
        out
    }
}

/// The statement's shape, from its leading words.
#[derive(Debug, PartialEq, Eq)]
enum Shape {
    CreateTable,
    AlterTable,
    CreateView,
    CreateIndex,
    CreateSequence,
    DropTable,
    /// `ALTER VIEW|TRIGGER|… x COMPILE|ENABLE|DISABLE`: nothing a schema reader needs.
    AlterCompile,
    Other,
}

fn words(toks: &[Tok], n: usize) -> Vec<&str> {
    toks.iter()
        .take(n)
        .map(|t| match &t.kind {
            TokKind::Word(w) => w.as_str(),
            _ => "",
        })
        .collect()
}

/// The index of the first word after `CREATE [OR REPLACE]` and any view/edition modifiers.
fn after_create_modifiers(toks: &[Tok]) -> usize {
    let mut i = 1;
    if toks.get(i).is_some_and(|t| t.is("OR")) {
        i += 2;
    }
    while toks
        .get(i)
        .is_some_and(|t| VIEW_MODIFIERS.iter().any(|m| t.is(m)))
    {
        i += 1;
    }
    i
}

const VIEW_MODIFIERS: &[&str] = &[
    "FORCE",
    "NOFORCE",
    "EDITIONABLE",
    "NONEDITIONABLE",
    "EDITIONING",
    "GLOBAL",
    "PRIVATE",
    "TEMPORARY",
    "UNIQUE",
    "BITMAP",
];

fn shape(toks: &[Tok]) -> Shape {
    let w = words(toks, 2);
    match w.as_slice() {
        ["ALTER", "TABLE"] => Shape::AlterTable,
        ["DROP", "TABLE"] => Shape::DropTable,
        ["ALTER", _] => alter_compile_shape(toks),
        ["CREATE", _] => create_shape(toks),
        _ => Shape::Other,
    }
}

fn alter_compile_shape(toks: &[Tok]) -> Shape {
    let object = [
        "VIEW",
        "TRIGGER",
        "FUNCTION",
        "PROCEDURE",
        "PACKAGE",
        "TYPE",
        "MATERIALIZED",
        "INDEX",
    ];
    let state = ["COMPILE", "ENABLE", "DISABLE", "REBUILD", "UNUSABLE"];
    let is_object = toks.get(1).is_some_and(|t| object.iter().any(|o| t.is(o)));
    let has_state = toks.iter().any(|t| state.iter().any(|s| t.is(s)));
    if is_object && has_state {
        Shape::AlterCompile
    } else {
        Shape::Other
    }
}

fn create_shape(toks: &[Tok]) -> Shape {
    match toks.get(after_create_modifiers(toks)) {
        Some(t) if t.is("TABLE") => Shape::CreateTable,
        Some(t) if t.is("VIEW") => Shape::CreateView,
        Some(t) if t.is("INDEX") => Shape::CreateIndex,
        Some(t) if t.is("SEQUENCE") => Shape::CreateSequence,
        _ => Shape::Other,
    }
}

fn rewrite_statement(text: &str) -> String {
    let toks = tokens(text);
    let mut edits = Edits::default();
    match shape(&toks) {
        Shape::AlterCompile => return blanked(text),
        Shape::CreateTable => {
            constraint_states(&toks, &mut edits);
            using_index(&toks, &mut edits);
            physical_attributes(&toks, &mut edits);
        }
        Shape::AlterTable => {
            if alters_constraint_state(&toks) {
                return blanked(text);
            }
            constraint_states(&toks, &mut edits);
            using_index(&toks, &mut edits);
            column_lists(&toks, &mut edits);
            drop_table_options(&toks, &mut edits);
        }
        Shape::CreateView => {
            create_modifiers(&toks, &mut edits);
            view_restrictions(&toks, &mut edits);
        }
        Shape::CreateIndex => {
            create_modifiers(&toks, &mut edits);
            physical_attributes(&toks, &mut edits);
        }
        Shape::CreateSequence => sequence_options(&toks, &mut edits),
        Shape::DropTable => drop_table_options(&toks, &mut edits),
        Shape::Other => {}
    }
    outer_join_markers(&toks, &mut edits);
    length_semantics(&toks, &mut edits);
    minus(&toks, &mut edits);
    dollar_identifiers(&toks, &mut edits);
    edits.apply(text)
}

/// The words that state a constraint's checking mode, which `sqlparser` does not know.
const CONSTRAINT_STATES: &[&str] = &[
    "ENABLE",
    "DISABLE",
    "VALIDATE",
    "NOVALIDATE",
    "RELY",
    "NORELY",
];

/// What may precede a constraint state: the end of a constraint, or another state word.
const BEFORE_STATE: &[&str] = &[
    "DEFERRED",
    "IMMEDIATE",
    "DEFERRABLE",
    "NULL",
    "KEY",
    "INDEX",
    "CASCADE",
];

fn constraint_states(toks: &[Tok], edits: &mut Edits) {
    for (i, t) in toks.iter().enumerate().skip(1) {
        if !CONSTRAINT_STATES.iter().any(|s| t.is(s)) {
            continue;
        }
        let prev = &toks[i - 1];
        let follows_constraint = prev.is_sym(b')')
            || BEFORE_STATE.iter().any(|w| prev.is(w))
            || CONSTRAINT_STATES.iter().any(|w| prev.is(w));
        if follows_constraint {
            edits.blank_tok(t);
        }
    }
}

/// `ALTER TABLE t ENABLE|DISABLE …` (a constraint or trigger switched on or off): no schema in it.
fn alters_constraint_state(toks: &[Tok]) -> bool {
    let after_name = name_end(toks, 2);
    toks.get(after_name).is_some_and(|t| {
        ["ENABLE", "DISABLE", "MOVE", "SHRINK"]
            .iter()
            .any(|w| t.is(w))
    })
}

/// The token index after a possibly dotted name starting at `i`.
fn name_end(toks: &[Tok], i: usize) -> usize {
    let mut j = i + 1;
    while toks.get(j).is_some_and(|t| t.is_sym(b'.')) && toks.get(j + 1).is_some() {
        j += 2;
    }
    j
}

/// `USING INDEX [(…) | name | physical attributes]` after a key constraint.
fn using_index(toks: &[Tok], edits: &mut Edits) {
    let mut i = 0;
    while i + 1 < toks.len() {
        if !(toks[i].is("USING") && toks[i + 1].is("INDEX")) {
            i += 1;
            continue;
        }
        let mut end = i + 2;
        if toks.get(end).is_some_and(|t| t.is_sym(b'(')) {
            end = matching_paren(toks, end).unwrap_or(toks.len() - 1) + 1;
        } else {
            end = skip_physical(toks, end);
        }
        let last = &toks[end.max(i + 2) - 1];
        edits.blank(toks[i].start, last.end);
        i = end.max(i + 2);
    }
}

/// Physical storage words. None of them says anything about the data.
const PHYSICAL: &[&str] = &[
    "TABLESPACE",
    "STORAGE",
    "PCTFREE",
    "PCTUSED",
    "INITRANS",
    "MAXTRANS",
    "LOGGING",
    "NOLOGGING",
    "COMPRESS",
    "NOCOMPRESS",
    "CACHE",
    "NOCACHE",
    "PARALLEL",
    "NOPARALLEL",
    "MONITORING",
    "NOMONITORING",
    "ONLINE",
    "COMPUTE",
    "STATISTICS",
    "SEGMENT",
    "CREATION",
    "IMMEDIATE",
    "DEFERRED",
    "LOB",
    "STORE",
    "AS",
    "SECUREFILE",
    "BASICFILE",
    "ROWDEPENDENCIES",
    "NOROWDEPENDENCIES",
    "REVERSE",
    "ORGANIZATION",
    "HEAP",
];

/// From `i`, past a run of physical attributes and their arguments.
fn skip_physical(toks: &[Tok], mut i: usize) -> usize {
    while let Some(t) = toks.get(i) {
        let is_arg = matches!(t.kind, TokKind::Num | TokKind::Quoted(_));
        if t.is_sym(b'(') {
            i = matching_paren(toks, i).map_or(toks.len(), |c| c + 1);
            continue;
        }
        let physical = PHYSICAL.iter().any(|w| t.is(w));
        let tablespace_name = i > 0 && toks[i - 1].is("TABLESPACE") && t.ident().is_some();
        if !(physical || is_arg || tablespace_name) {
            break;
        }
        i += 1;
    }
    i
}

/// Physical attributes after the column list of `CREATE TABLE` / `CREATE INDEX`, up to `AS` (for
/// `CREATE TABLE … AS SELECT`) or the end of the statement.
fn physical_attributes(toks: &[Tok], edits: &mut Edits) {
    let Some(open) = toks.iter().position(|t| t.is_sym(b'(')) else {
        return;
    };
    let Some(close) = matching_paren(toks, open) else {
        return;
    };
    let start = close + 1;
    if !toks
        .get(start)
        .is_some_and(|t| PHYSICAL.iter().any(|w| t.is(w)) && !t.is("AS"))
    {
        return;
    }
    let mut end = start;
    while let Some(t) = toks.get(end) {
        let stops = t.is("AS")
            && toks
                .get(end + 1)
                .is_some_and(|n| n.is("SELECT") || n.is_sym(b'('));
        if stops || t.is_sym(b';') {
            break;
        }
        end += 1;
    }
    if end > start {
        edits.blank(toks[start].start, toks[end - 1].end);
    }
}

/// `ALTER TABLE t ADD (c1 T1, c2 T2)` → `ADD c1 T1, ADD c2 T2`; the same for `MODIFY`, and
/// `DROP (c1, c2)` → `DROP COLUMN c1, DROP COLUMN c2`. A `MODIFY`
/// item that changes only an attribute — `c DEFAULT x`, `c NOT NULL`, `c NULL` — becomes the ANSI
/// `ALTER COLUMN c SET DEFAULT x | SET NOT NULL | DROP NOT NULL`, which `sqlparser` reads exactly;
/// as `MODIFY` it fails on `DEFAULT` and misreads `NOT`/`NULL` as a type name.
fn column_lists(toks: &[Tok], edits: &mut Edits) {
    for (i, t) in toks.iter().enumerate() {
        let verb = match &t.kind {
            TokKind::Word(w) if w == "ADD" || w == "MODIFY" => w.as_str(),
            TokKind::Word(w) if w == "DROP" => "DROP COLUMN",
            _ => continue,
        };
        let listed = toks.get(i + 1).is_some_and(|n| n.is_sym(b'('));
        let items = if listed {
            let Some(close) = matching_paren(toks, i + 1) else {
                continue;
            };
            edits.blank_tok(&toks[i + 1]);
            edits.blank_tok(&toks[close]);
            item_starts(toks, i + 1, close)
        } else if verb == "MODIFY" {
            vec![i + 1]
        } else {
            continue;
        };
        for (n, &at) in items.iter().enumerate() {
            let alter = verb == "MODIFY" && attribute_only(toks, at, edits);
            let head = if alter { "ALTER COLUMN" } else { verb };
            if n == 0 {
                if alter || verb == "DROP COLUMN" {
                    edits.replace(t.start, t.end, head);
                }
            } else {
                let comma = &toks[at - 1];
                edits.replace(comma.start, comma.end, format!(", {head} "));
            }
        }
    }
}

/// The first token of each item in the list `open..close`.
fn item_starts(toks: &[Tok], open: usize, close: usize) -> Vec<usize> {
    let mut out = vec![open + 1];
    out.extend(
        top_level_commas(toks, open, close)
            .into_iter()
            .map(|c| c + 1),
    );
    out.retain(|&s| s < close);
    out
}

/// Whether the `MODIFY` item whose column name is at `at` changes only an attribute; when it does,
/// the attribute's words are rewritten into their `ALTER COLUMN` form.
fn attribute_only(toks: &[Tok], at: usize, edits: &mut Edits) -> bool {
    let Some(next) = toks.get(at + 1) else {
        return false;
    };
    if next.is("DEFAULT") {
        edits.replace(next.start, next.end, "SET DEFAULT");
        // `c DEFAULT x NOT NULL` is two changes: the second becomes its own `ALTER COLUMN`.
        if let (Some(not), Some(col)) = (trailing_not_null(toks, at + 2), toks[at].ident()) {
            let t = &toks[not];
            edits.replace(t.start, t.end, format!(", ALTER COLUMN {col} SET NOT"));
        }
        return true;
    }
    let not_null = next.is("NOT") && toks.get(at + 2).is_some_and(|t| t.is("NULL"));
    if not_null {
        edits.replace(next.start, next.end, "SET NOT");
        return true;
    }
    if next.is("NULL") {
        edits.replace(next.start, next.end, "DROP NOT NULL");
        return true;
    }
    false
}

/// Within the item from `from` (up to a comma or closing parenthesis at its own depth), the index
/// of a `NOT` that starts `NOT NULL`.
fn trailing_not_null(toks: &[Tok], from: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (j, t) in toks.iter().enumerate().skip(from) {
        if t.is_sym(b'(') {
            depth += 1;
        } else if t.is_sym(b')') {
            depth -= 1;
        }
        if depth < 0 || (depth == 0 && t.is_sym(b',')) {
            return None;
        }
        if depth == 0 && t.is("NOT") && toks.get(j + 1).is_some_and(|n| n.is("NULL")) {
            return Some(j);
        }
    }
    None
}

/// Token indices of the commas directly inside the parentheses `open..close`.
fn top_level_commas(toks: &[Tok], open: usize, close: usize) -> Vec<usize> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for (j, t) in toks.iter().enumerate().take(close).skip(open + 1) {
        if t.is_sym(b'(') {
            depth += 1;
        } else if t.is_sym(b')') {
            depth -= 1;
        } else if depth == 0 && t.is_sym(b',') {
            out.push(j);
        }
    }
    out
}

/// `CREATE [OR REPLACE] FORCE|EDITIONABLE|BITMAP|… VIEW|INDEX`: the modifiers.
fn create_modifiers(toks: &[Tok], edits: &mut Edits) {
    let from = if toks.get(1).is_some_and(|t| t.is("OR")) {
        3
    } else {
        1
    };
    for t in toks.iter().take(after_create_modifiers(toks)).skip(from) {
        // `UNIQUE` is meaningful on an index and `sqlparser` reads it; keep it.
        if !t.is("UNIQUE") {
            edits.blank_tok(t);
        }
    }
}

/// `WITH READ ONLY` / `WITH CHECK OPTION [CONSTRAINT c]` closing a view.
fn view_restrictions(toks: &[Tok], edits: &mut Edits) {
    let n = toks.len();
    let tail = toks.iter().rposition(|t| t.is("WITH")).filter(|&i| {
        let next = toks.get(i + 1);
        next.is_some_and(|t| t.is("READ") || t.is("CHECK"))
    });
    if let Some(i) = tail {
        let last = if toks[n - 1].is_sym(b';') {
            n - 2
        } else {
            n - 1
        };
        edits.blank(toks[i].start, toks[last].end);
    }
}

/// `CREATE SEQUENCE s <options>` → `CREATE SEQUENCE s`. The options are numbers about allocation.
fn sequence_options(toks: &[Tok], edits: &mut Edits) {
    let name_at = after_create_modifiers(toks) + 1;
    let from = name_end(toks, name_at);
    let Some(last) = toks.iter().rposition(|t| !t.is_sym(b';')) else {
        return;
    };
    if from <= last {
        edits.blank(toks[from].start, toks[last].end);
    }
}

/// `DROP TABLE t CASCADE CONSTRAINTS [PURGE]`, and `… DROP (c) CASCADE CONSTRAINTS` in `ALTER TABLE`.
fn drop_table_options(toks: &[Tok], edits: &mut Edits) {
    for (i, t) in toks.iter().enumerate() {
        let cascade = t.is("CASCADE")
            && toks
                .get(i + 1)
                .is_some_and(|n| n.is("CONSTRAINTS") || n.is("CONSTRAINT"));
        if cascade {
            edits.blank(t.start, toks[i + 1].end);
        } else if t.is("PURGE") {
            edits.blank_tok(t);
        }
    }
}

/// `a.id = b.id(+)`: the outer-join marker. The predicate stays, so both tables are still read.
fn outer_join_markers(toks: &[Tok], edits: &mut Edits) {
    for w in toks.windows(3) {
        if w[0].is_sym(b'(') && w[1].is_sym(b'+') && w[2].is_sym(b')') {
            edits.blank(w[0].start, w[2].end);
        }
    }
}

/// `VARCHAR2(10 BYTE)`, `CHAR(1 CHAR)`: length semantics. `sqlparser` reads them after a custom
/// type (`VARCHAR2`) but not after a built-in one (`CHAR`, `VARCHAR`).
fn length_semantics(toks: &[Tok], edits: &mut Edits) {
    for w in toks.windows(4) {
        if w[0].is_sym(b'(')
            && w[1].kind == TokKind::Num
            && (w[2].is("BYTE") || w[2].is("CHAR"))
            && w[3].is_sym(b')')
        {
            edits.blank_tok(&w[2]);
        }
    }
}

/// `MINUS` is Oracle's `EXCEPT`.
fn minus(toks: &[Tok], edits: &mut Edits) {
    for t in toks.iter().filter(|t| t.is("MINUS")) {
        edits.replace(t.start, t.end, "EXCEPT");
    }
}

/// `CREATE$JAVA$LOB$TABLE`: Oracle allows `$` and `#` inside an unquoted identifier, `sqlparser`
/// does not. Quoting it keeps the name and lets it parse.
fn dollar_identifiers(toks: &[Tok], edits: &mut Edits) {
    for t in toks {
        if let TokKind::Word(w) = &t.kind
            && (w.contains('$') || w.contains('#'))
        {
            edits.replace(t.start, t.end, format!("\"{w}\""));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlparser::parser::Parser;

    fn parses(sql: &str) -> bool {
        Parser::parse_sql(&GenericDialect {}, &preprocess(sql)).is_ok()
    }

    fn assert_parses(sql: &str) {
        let pre = preprocess(sql);
        if let Err(e) = Parser::parse_sql(&GenericDialect {}, &pre) {
            panic!("{sql}\n→ {pre}\n{e}");
        }
    }

    #[test]
    fn line_count_is_preserved() {
        let src = "SET DEFINE OFF\nCREATE OR REPLACE FUNCTION f RETURN NUMBER AS\nBEGIN\n RETURN 1;\nEND;\n/\n\
                   ALTER TABLE t ADD (a NUMBER(10),\n b CHAR(1));\n";
        assert_eq!(preprocess(src).lines().count(), src.lines().count());
    }

    #[test]
    fn sqlplus_and_plsql_are_blanked_and_statements_survive() {
        let src = "SET SQLBLANKLINES ON\nSET DEFINE OFF\nCREATE OR REPLACE FUNCTION f RETURN NUMBER AS\n\
                   BEGIN\n  IF 1 = 1 THEN RETURN 1; END IF;\nEND;\n/\nINSERT INTO t (a) VALUES (1)\n;\n";
        let pre = preprocess(src);
        assert!(
            !pre.contains("FUNCTION") && !pre.contains("SQLBLANKLINES"),
            "{pre}"
        );
        let stmts = Parser::parse_sql(&GenericDialect {}, &pre).unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn constraint_states_are_removed() {
        assert_parses(
            "ALTER TABLE X ADD CONSTRAINT C FOREIGN KEY (A) REFERENCES Y(A) INITIALLY DEFERRED DEFERRABLE ENABLE NOVALIDATE",
        );
        assert_parses("ALTER TABLE X ADD CONSTRAINT P PRIMARY KEY (A) ENABLE");
        assert_parses(
            "CREATE TABLE T (A NUMBER(10) NOT NULL ENABLE, CONSTRAINT K PRIMARY KEY (A) USING INDEX ENABLE)",
        );
    }

    #[test]
    fn a_column_named_like_a_state_word_is_kept() {
        let pre = preprocess("ALTER TABLE X ADD Enable CHAR(1)");
        assert!(pre.contains("Enable"), "{pre}");
    }

    #[test]
    fn switching_a_constraint_on_or_off_is_not_schema() {
        assert_eq!(preprocess("ALTER TABLE X DISABLE CONSTRAINT C;").trim(), "");
        assert_eq!(preprocess("ALTER VIEW V COMPILE;").trim(), "");
        assert_eq!(preprocess("ALTER TRIGGER T ENABLE;").trim(), "");
    }

    #[test]
    fn column_lists_become_one_operation_each() {
        let pre = preprocess("ALTER TABLE X ADD (A NUMBER(10), B CHAR(1) DEFAULT 'N')");
        assert_eq!(
            pre.split_whitespace().collect::<Vec<_>>().join(" "),
            "ALTER TABLE X ADD A NUMBER(10), ADD B CHAR(1) DEFAULT 'N'"
        );
        assert_parses("ALTER TABLE X ADD (A NUMBER(10), B CHAR(1) DEFAULT 'N')");
        assert_parses(
            "ALTER TABLE M_ATTRIBUTESET MODIFY (LOTCHAREOVERWRITE CHAR(1 CHAR), SERNOCHARSOVERWRITE CHAR(1 CHAR))",
        );
    }

    #[test]
    fn oracle_types_views_and_drops() {
        assert_parses(
            "CREATE TABLE T (A VARCHAR2(36 CHAR) DEFAULT NULL, B CHAR(1 BYTE), C NUMBER, D DATE DEFAULT SYSDATE)",
        );
        assert_parses("CREATE OR REPLACE FORCE VIEW V AS SELECT 1 FROM dual");
        assert_parses("CREATE OR REPLACE VIEW V AS SELECT 1 FROM dual WITH READ ONLY");
        assert_parses("DROP TABLE T CASCADE CONSTRAINTS PURGE");
        assert_parses(
            "CREATE SEQUENCE S MINVALUE 1 MAXVALUE 2147483647 INCREMENT BY 1 START WITH 1000000 CACHE 20 NOCYCLE NOORDER",
        );
        assert_parses(
            "CREATE TABLE T (A NUMBER(10)) TABLESPACE USERS STORAGE (INITIAL 64K) NOLOGGING",
        );
        assert_parses("CREATE BITMAP INDEX I ON T (A) TABLESPACE X ONLINE");
        assert_parses(
            "ALTER TABLE X ADD CONSTRAINT C PRIMARY KEY (A) USING INDEX TABLESPACE USERS",
        );
    }

    #[test]
    fn modify_attribute_only_becomes_alter_column() {
        let squash = |s: &str| {
            preprocess(s)
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert_eq!(
            squash("ALTER TABLE a MODIFY created DEFAULT SysDate"),
            "ALTER TABLE a ALTER COLUMN created SET DEFAULT SysDate"
        );
        assert_eq!(
            squash("ALTER TABLE a MODIFY b NOT NULL"),
            "ALTER TABLE a ALTER COLUMN b SET NOT NULL"
        );
        assert_eq!(
            squash("ALTER TABLE a MODIFY b NULL"),
            "ALTER TABLE a ALTER COLUMN b DROP NOT NULL"
        );
        assert_parses("ALTER TABLE a MODIFY (b DEFAULT 0, c VARCHAR2(10), d NOT NULL)");
        assert_eq!(
            squash("ALTER TABLE a MODIFY updated DEFAULT SysDate NOT NULL"),
            "ALTER TABLE a ALTER COLUMN updated SET DEFAULT SysDate , ALTER COLUMN UPDATED SET NOT NULL"
        );
        assert_parses("ALTER TABLE a MODIFY updated DEFAULT SysDate NOT NULL");
        assert_parses(
            "ALTER TABLE X ADD CONSTRAINT C FOREIGN KEY (A) REFERENCES Y(A) ON DELETE CASCADE ENABLE",
        );
    }

    #[test]
    fn dropped_column_lists() {
        assert_parses("ALTER TABLE AD_ACCESSLOG DROP (tmp_col_1_tmp,tmp_col_2_tmp)");
        assert_parses("ALTER TABLE I_FIXEDASSET DROP ( I_ISIMPORTEDDROP ) CASCADE CONSTRAINT");
        assert_parses("ALTER TABLE T DROP CONSTRAINT C");
    }

    #[test]
    fn index_maintenance_and_dollar_names() {
        assert_eq!(preprocess("ALTER INDEX K REBUILD;").trim(), "");
        assert_parses("ALTER INDEX K RENAME TO L");
        assert_parses("ALTER TABLE CREATE$JAVA$LOB$TABLE MODIFY NAME VARCHAR2(700)");
    }

    #[test]
    fn queries() {
        assert_parses("SELECT a.x FROM a, b WHERE a.id = b.id(+)");
        assert_parses("SELECT x FROM t MINUS SELECT x FROM u");
        assert_parses("UPDATE t SET a = q'[it's]' WHERE b = 'C:\\'");
        assert!(parses(
            "SELECT DECODE(a,1,'x','y'), NVL(b,0) FROM t WHERE ROWNUM <= 10"
        ));
    }

    #[test]
    fn create_table_as_select_keeps_its_query() {
        let pre = preprocess("CREATE TABLE T NOLOGGING AS SELECT * FROM U");
        assert!(pre.contains("SELECT * FROM U"), "{pre}");
    }

    #[test]
    fn strings_are_never_rewritten() {
        let pre = preprocess("INSERT INTO t (a) VALUES ('x(+) MINUS ENABLE')");
        assert!(pre.contains("'x(+) MINUS ENABLE'"), "{pre}");
    }
}
