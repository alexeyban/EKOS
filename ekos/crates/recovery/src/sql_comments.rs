//! `COMMENT ON` extraction (RFC 0146 Phase 2).
//!
//! PostgreSQL schemas document themselves with `COMMENT ON TABLE`/`COMMENT ON COLUMN`, and a
//! hand-maintained one does it thoroughly: LedgerSMB's `sql/` tree carries 631 such statements,
//! 277 of them on tables and columns. Until this module existed EKOS threw all of them away —
//! `sqlparser 0.53` rejects dollar-quoted comment bodies outright, `SqlAnalyzerPass` models
//! `COMMENT ON` not at all, and RFC 0146 Phase 1's `preprocess` then strips the statements so the
//! rest of the file can parse. The net effect was that EKOS deleted the schema's authoritative,
//! human-written descriptions and asked an LLM to invent replacements for the same tables.
//!
//! This module reads them back out of the **raw** SQL, before any preprocessing runs, and hands
//! them to `sql_analyzer` as evidence-backed `description` facts. That ordering is the whole
//! design constraint: `SqlDialectParser::preprocess` can only *remove* text, never produce facts,
//! so extraction cannot live in the dialect crate and must happen upstream of it.
//!
//! Deliberately not a `sqlparser` extension: teaching it `COMMENT ON <anything> IS $$...$$` would
//! mean forking a pinned dependency, and the text needed here is a quoted literal that a lexer
//! already delimits unambiguously.

use ekos_sql_dialect_sdk::lex::{is_word_at, skip_non_code, skip_trivia, statement_spans};

/// What a `COMMENT ON` statement documents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommentTarget {
    /// `COMMENT ON TABLE <table> IS ...` — `table` keeps the spelling used in the source,
    /// schema qualification and all.
    Table(String),
    /// `COMMENT ON COLUMN <table>.<column> IS ...`
    Column { table: String, column: String },
}

/// One recovered `COMMENT ON` statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlComment {
    pub target: CommentTarget,
    /// The comment body with its quoting removed and `''` escapes resolved.
    pub text: String,
    /// 1-indexed line of the `COMMENT` keyword in the original file, for `SourceLocation::at`.
    pub line: u32,
}

impl SqlComment {
    /// The unqualified object name — `public.account` → `account`. Table identity in the KIR is
    /// unqualified, so matching has to drop the schema prefix.
    pub fn bare_table(&self) -> &str {
        let qualified = match &self.target {
            CommentTarget::Table(t) => t.as_str(),
            CommentTarget::Column { table, .. } => table.as_str(),
        };
        qualified.rsplit('.').next().unwrap_or(qualified)
    }
}

/// Extracts every `COMMENT ON TABLE`/`COMMENT ON COLUMN` statement from `sql`.
///
/// Must be given the **raw** file, not a preprocessed one — RFC 0146 Phase 1 strips exactly these
/// statements.
///
/// `COMMENT ON <other object type>` (FUNCTION, VIEW, INDEX, …) is skipped: EKOS has no KIR object
/// to attach that text to, and the 354 such statements in LedgerSMB would otherwise be recovered
/// with nowhere to go. `COMMENT ON ... IS NULL` is also skipped — in PostgreSQL that *removes* a
/// comment, so recording the word "NULL" as a description would invert its meaning.
pub fn extract_sql_comments(sql: &str) -> Vec<SqlComment> {
    let chars: Vec<char> = sql.chars().collect();
    let mut line_starts = vec![0usize];
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |pos: usize| -> u32 {
        match line_starts.binary_search(&pos) {
            Ok(i) => (i + 1) as u32,
            Err(i) => i as u32,
        }
    };

    let mut out = Vec::new();
    for (start, end) in statement_spans(&chars) {
        let head = skip_trivia(&chars, start);
        if head >= end || !is_word_at(&chars, head, "COMMENT", false) {
            continue;
        }
        if let Some(comment) = parse_comment_on(&chars, head, end, &line_of) {
            out.push(comment);
        }
    }
    out
}

/// Parses one `COMMENT ON <TABLE|COLUMN> <name> IS <literal>` statement starting at `head`.
/// Returns `None` for any other shape, which is how unsupported object types and `IS NULL` are
/// filtered out.
fn parse_comment_on(
    chars: &[char],
    head: usize,
    end: usize,
    line_of: &dyn Fn(usize) -> u32,
) -> Option<SqlComment> {
    let line = line_of(head);
    let mut i = head + "COMMENT".len();

    i = skip_trivia(chars, i);
    if !is_word_at(chars, i, "ON", false) {
        return None;
    }
    i = skip_trivia(chars, i + 2);

    let is_column = if is_word_at(chars, i, "TABLE", false) {
        i += "TABLE".len();
        false
    } else if is_word_at(chars, i, "COLUMN", false) {
        i += "COLUMN".len();
        true
    } else {
        // FUNCTION / VIEW / INDEX / SEQUENCE / TYPE / TRIGGER / ROLE / CONSTRAINT / AGGREGATE —
        // real statements with no KIR object to carry them.
        return None;
    };

    i = skip_trivia(chars, i);
    let (name, next) = read_qualified_name(chars, i, end)?;
    i = skip_trivia(chars, next);

    if !is_word_at(chars, i, "IS", false) {
        return None;
    }
    i = skip_trivia(chars, i + 2);

    // `IS NULL` removes a comment in PostgreSQL; it is not a description.
    if is_word_at(chars, i, "NULL", false) {
        return None;
    }

    let text = read_quoted_literal(chars, i)?;
    if text.trim().is_empty() {
        return None;
    }

    let mut parts: Vec<&str> = name.split('.').collect();
    let target = if is_column {
        // The last segment is the column; everything before it names the table.
        let column = parts.pop()?.to_string();
        if parts.is_empty() {
            return None;
        }
        CommentTarget::Column {
            table: parts.join("."),
            column,
        }
    } else {
        CommentTarget::Table(name)
    };

    Some(SqlComment {
        target,
        text: text.trim().to_string(),
        line,
    })
}

/// Reads a possibly schema-qualified, possibly double-quoted identifier such as `account`,
/// `public.account` or `public."Odd Name".col`. Returns the name with quotes stripped and the
/// index just past it.
fn read_qualified_name(chars: &[char], from: usize, end: usize) -> Option<(String, usize)> {
    let mut i = from;
    let mut name = String::new();

    loop {
        if i >= end {
            return None;
        }
        if chars[i] == '"' {
            let close = skip_non_code(chars, i)?;
            name.extend(&chars[i + 1..close.saturating_sub(1)]);
            i = close;
        } else {
            let start = i;
            while i < end && (ekos_sql_dialect_sdk::lex::is_ident_char(chars[i])) {
                i += 1;
            }
            if i == start {
                return None;
            }
            name.extend(&chars[start..i]);
        }
        if chars.get(i) == Some(&'.') {
            name.push('.');
            i += 1;
            continue;
        }
        return Some((name, i));
    }
}

/// Reads a single-quoted or dollar-quoted literal at `from`, returning its content with quoting
/// removed and `''` escapes collapsed to `'`.
fn read_quoted_literal(chars: &[char], from: usize) -> Option<String> {
    match chars.get(from)? {
        '\'' => {
            let close = skip_non_code(chars, from)?;
            let raw: String = chars[from + 1..close.saturating_sub(1)].iter().collect();
            Some(raw.replace("''", "'"))
        }
        '$' => {
            let tag_end = ekos_sql_dialect_sdk::lex::dollar_quote_tag_end(chars, from)?;
            let close = skip_non_code(chars, from)?;
            let tag_len = tag_end - from + 1;
            // close is just past the closing tag; content sits between the two tags.
            let content_start = from + tag_len;
            let content_end = close.checked_sub(tag_len)?;
            if content_end < content_start {
                return None;
            }
            Some(chars[content_start..content_end].iter().collect())
        }
        _ => None,
    }
}

/// What kind of non-table object a `COMMENT ON` documents (RFC 0163 routines, RFC 0169 views).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectCommentKind {
    Function,
    Procedure,
    View,
}

/// One `COMMENT ON FUNCTION|PROCEDURE|[MATERIALIZED] VIEW` statement.
///
/// Kept apart from [`SqlComment`] on purpose: `sql_analyzer` matches every `SqlComment` against
/// `Table` names, and a `COMMENT ON VIEW v` must never land on a table that happens to be called
/// `v`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectComment {
    pub kind: ObjectCommentKind,
    /// The name as written, quotes stripped, schema qualification kept.
    pub name: String,
    /// For a routine, its argument list as written (`["int", "text"]`), so an overloaded name can
    /// be matched by arity. `None` for a view, or a routine commented without a list.
    pub args: Option<Vec<String>>,
    pub text: String,
    /// 1-indexed line of the `COMMENT` keyword.
    pub line: u32,
}

/// Every `COMMENT ON FUNCTION|PROCEDURE|[MATERIALIZED] VIEW … IS <literal>` in the **raw** `sql`.
/// `IS NULL` (which removes a comment) and empty comments are skipped, as for tables.
pub fn extract_object_comments(sql: &str) -> Vec<ObjectComment> {
    let chars: Vec<char> = sql.chars().collect();
    let mut line_starts = vec![0usize];
    for (i, c) in chars.iter().enumerate() {
        if *c == '\n' {
            line_starts.push(i + 1);
        }
    }
    let line_of = |pos: usize| -> u32 {
        match line_starts.binary_search(&pos) {
            Ok(i) => (i + 1) as u32,
            Err(i) => i as u32,
        }
    };
    let mut out = Vec::new();
    for (start, end) in statement_spans(&chars) {
        let head = skip_trivia(&chars, start);
        if head >= end || !is_word_at(&chars, head, "COMMENT", false) {
            continue;
        }
        if let Some(c) = parse_object_comment(&chars, head, end, line_of(head)) {
            out.push(c);
        }
    }
    out
}

fn parse_object_comment(
    chars: &[char],
    head: usize,
    end: usize,
    line: u32,
) -> Option<ObjectComment> {
    let mut i = skip_trivia(chars, head + "COMMENT".len());
    if !is_word_at(chars, i, "ON", false) {
        return None;
    }
    i = skip_trivia(chars, i + 2);
    let kind = if is_word_at(chars, i, "FUNCTION", false) {
        i += "FUNCTION".len();
        ObjectCommentKind::Function
    } else if is_word_at(chars, i, "PROCEDURE", false) {
        i += "PROCEDURE".len();
        ObjectCommentKind::Procedure
    } else if is_word_at(chars, i, "VIEW", false) {
        i += "VIEW".len();
        ObjectCommentKind::View
    } else if is_word_at(chars, i, "MATERIALIZED", false) {
        i = skip_trivia(chars, i + "MATERIALIZED".len());
        if !is_word_at(chars, i, "VIEW", false) {
            return None;
        }
        i += "VIEW".len();
        ObjectCommentKind::View
    } else {
        return None;
    };
    i = skip_trivia(chars, i);
    let (name, next) = read_qualified_name(chars, i, end)?;
    i = skip_trivia(chars, next);

    // A routine's argument list, `(int, text)`, split at top-level commas.
    let mut args = None;
    if chars.get(i) == Some(&'(') && kind != ObjectCommentKind::View {
        let mut depth = 0i32;
        let mut current = String::new();
        let mut list = Vec::new();
        let mut j = i;
        while j < end {
            let c = chars[j];
            match c {
                '(' => {
                    depth += 1;
                    if depth > 1 {
                        current.push(c);
                    }
                }
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    current.push(c);
                }
                ',' if depth == 1 => list.push(std::mem::take(&mut current)),
                _ => current.push(c),
            }
            j += 1;
        }
        if depth != 0 {
            return None;
        }
        list.push(current);
        let list: Vec<String> = list
            .into_iter()
            .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|a| !a.is_empty())
            .collect();
        args = Some(list);
        i = skip_trivia(chars, j + 1);
    }

    if !is_word_at(chars, i, "IS", false) {
        return None;
    }
    i = skip_trivia(chars, i + 2);
    if is_word_at(chars, i, "NULL", false) {
        return None;
    }
    let text = read_quoted_literal(chars, i)?;
    if text.trim().is_empty() {
        return None;
    }
    Some(ObjectComment {
        kind,
        name,
        args,
        text: text.trim().to_string(),
        line,
    })
}

/// Which of `candidates` — `(name, argument count)` of the routines or views one file defines —
/// each comment of `kinds` documents. Returns, per candidate index, the comment that applies.
///
/// Matching is by unqualified, lower-cased name within the file. When several candidates share
/// the name (overloads), the comment's own argument list narrows them by arity; a comment still
/// matching more than one candidate is left unattached rather than guessed. A later comment on
/// the same object replaces an earlier one, as it does in the database.
pub fn match_object_comments<'c>(
    comments: &'c [ObjectComment],
    kinds: &[ObjectCommentKind],
    candidates: &[(String, usize)],
) -> std::collections::BTreeMap<usize, &'c ObjectComment> {
    let tail = |n: &str| n.rsplit('.').next().unwrap_or(n).to_lowercase();
    let mut out = std::collections::BTreeMap::new();
    for c in comments.iter().filter(|c| kinds.contains(&c.kind)) {
        let name = tail(&c.name);
        let mut hits: Vec<usize> = candidates
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| tail(n) == name)
            .map(|(i, _)| i)
            .collect();
        if hits.len() > 1
            && let Some(args) = &c.args
        {
            hits.retain(|&i| candidates[i].1 == args.len());
        }
        if let [only] = hits.as_slice() {
            out.insert(*only, c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_a_dollar_quoted_table_comment() {
        let sql = "COMMENT ON TABLE lsmb_module IS $$ This stores categories. $$;";
        let got = extract_sql_comments(sql);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].target, CommentTarget::Table("lsmb_module".into()));
        assert_eq!(got[0].text, "This stores categories.");
    }

    #[test]
    fn extracts_a_single_quoted_table_comment_and_unescapes() {
        let sql = "COMMENT ON TABLE account IS 'the company''s chart of accounts';";
        let got = extract_sql_comments(sql);
        assert_eq!(got[0].text, "the company's chart of accounts");
    }

    #[test]
    fn extracts_a_column_comment_and_splits_table_from_column() {
        let sql = "COMMENT ON COLUMN language.code IS $$ ISO 639 code. $$;";
        let got = extract_sql_comments(sql);
        assert_eq!(
            got[0].target,
            CommentTarget::Column {
                table: "language".into(),
                column: "code".into()
            }
        );
    }

    #[test]
    fn strips_schema_qualification_for_matching() {
        let sql = "COMMENT ON COLUMN public.account.accno IS 'x';";
        let got = extract_sql_comments(sql);
        assert_eq!(
            got[0].target,
            CommentTarget::Column {
                table: "public.account".into(),
                column: "accno".into()
            }
        );
        assert_eq!(got[0].bare_table(), "account");
    }

    #[test]
    fn multiline_body_keeps_its_content_and_reports_the_opening_line() {
        let sql =
            "CREATE TABLE a (id INT);\n\nCOMMENT ON TABLE a IS\n$$ first line\nsecond line $$;";
        let got = extract_sql_comments(sql);
        assert_eq!(got.len(), 1);
        assert!(got[0].text.contains("second line"), "got {:?}", got[0].text);
        assert_eq!(got[0].line, 3, "line should point at the COMMENT keyword");
    }

    #[test]
    fn a_semicolon_inside_the_body_does_not_truncate_it() {
        let sql = "COMMENT ON TABLE a IS $$ one; two; three $$;\nCOMMENT ON TABLE b IS 'second';";
        let got = extract_sql_comments(sql);
        assert_eq!(got.len(), 2, "got {got:?}");
        assert_eq!(got[0].text, "one; two; three");
        assert_eq!(got[1].text, "second");
    }

    #[test]
    fn unsupported_object_types_are_skipped() {
        let sql = "\
COMMENT ON FUNCTION menu_generate() IS $$ returns the tree $$;
COMMENT ON VIEW v IS 'a view';
COMMENT ON INDEX i IS 'an index';
COMMENT ON TABLE t IS 'a table';";
        let got = extract_sql_comments(sql);
        assert_eq!(
            got.len(),
            1,
            "only the TABLE comment is usable, got {got:?}"
        );
        assert_eq!(got[0].target, CommentTarget::Table("t".into()));
    }

    #[test]
    fn is_null_removes_a_comment_and_is_not_a_description() {
        let sql = "COMMENT ON TABLE t IS NULL;";
        assert!(extract_sql_comments(sql).is_empty());
    }

    #[test]
    fn empty_body_is_not_a_description() {
        assert!(extract_sql_comments("COMMENT ON TABLE t IS '   ';").is_empty());
    }

    #[test]
    fn comment_behind_a_banner_is_still_found() {
        let sql = "\
CREATE TABLE a (id INT);
-- Moving this comment to SQL comments because it is about this code
-- rather than the database structure as API. --CT
COMMENT ON TABLE a IS $$ Hardwired classifications. $$;";
        let got = extract_sql_comments(sql);
        assert_eq!(got.len(), 1, "got {got:?}");
    }

    #[test]
    fn the_word_comment_inside_a_function_body_is_not_a_statement() {
        let sql = "CREATE FUNCTION f() RETURNS INT AS $$ -- COMMENT ON TABLE fake IS 'x'\n SELECT 1; $$ LANGUAGE sql;";
        assert!(extract_sql_comments(sql).is_empty());
    }

    #[test]
    fn quoted_identifiers_are_unquoted() {
        let sql = "COMMENT ON TABLE \"Odd Name\" IS 'x';";
        let got = extract_sql_comments(sql);
        assert_eq!(got[0].target, CommentTarget::Table("Odd Name".into()));
    }

    #[test]
    fn case_insensitive_keywords() {
        let sql = "comment on table a is $$ lower $$;";
        let got = extract_sql_comments(sql);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "lower");
    }

    #[test]
    fn a_file_with_no_comments_yields_nothing() {
        assert!(extract_sql_comments("CREATE TABLE a (id INT); SELECT 1;").is_empty());
    }

    #[test]
    fn routine_and_view_comments_are_extracted_separately() {
        let sql = "\
COMMENT ON FUNCTION menu_generate() IS $$ returns the tree $$;
COMMENT ON FUNCTION payment_post(int, numeric(10,2)[], text) IS 'posts a payment';
COMMENT ON PROCEDURE p IS 'no list';
COMMENT ON VIEW account_heading_tree IS $$ the heading tree $$;
COMMENT ON MATERIALIZED VIEW public.mv IS 'materialized';
COMMENT ON FUNCTION gone() IS NULL;
COMMENT ON INDEX i IS 'an index';
COMMENT ON TABLE t IS 'a table';";
        let got = extract_object_comments(sql);
        assert_eq!(got.len(), 5, "{got:?}");
        assert_eq!(got[0].kind, ObjectCommentKind::Function);
        assert_eq!(got[0].args, Some(vec![]));
        assert_eq!(got[0].text, "returns the tree");
        assert_eq!(got[1].name, "payment_post");
        assert_eq!(
            got[1].args,
            Some(vec!["int".into(), "numeric(10,2)[]".into(), "text".into()])
        );
        assert_eq!(got[1].line, 2);
        assert_eq!(got[2].args, None);
        assert_eq!(got[3].kind, ObjectCommentKind::View);
        assert_eq!(got[4].name, "public.mv");
        // The table extractor is unchanged: it still sees only the table comment.
        assert_eq!(extract_sql_comments(sql).len(), 1);
    }

    #[test]
    fn object_comments_match_by_name_then_arity_and_never_guess() {
        let sql = "\
COMMENT ON FUNCTION f(int) IS 'one arg';
COMMENT ON FUNCTION f(int, text) IS 'two args';
COMMENT ON FUNCTION g IS 'overloaded, no list';
COMMENT ON FUNCTION public.h() IS 'first';
COMMENT ON FUNCTION h() IS 'second wins';
COMMENT ON VIEW f IS 'a view, not the function';";
        let comments = extract_object_comments(sql);
        let routines = vec![
            ("f".to_string(), 1),
            ("f".to_string(), 2),
            ("g".to_string(), 0),
            ("g".to_string(), 1),
            ("h".to_string(), 0),
        ];
        let kinds = [ObjectCommentKind::Function, ObjectCommentKind::Procedure];
        let m = match_object_comments(&comments, &kinds, &routines);
        assert_eq!(m[&0].text, "one arg");
        assert_eq!(m[&1].text, "two args");
        assert!(
            !m.contains_key(&2) && !m.contains_key(&3),
            "ambiguous g is not guessed"
        );
        assert_eq!(m[&4].text, "second wins");
        let views =
            match_object_comments(&comments, &[ObjectCommentKind::View], &[("f".into(), 0)]);
        assert_eq!(views[&0].text, "a view, not the function");
    }
}
