//! `ViewAnalyzerPass` — RFC 0169: every `CREATE [OR REPLACE] [MATERIALIZED] VIEW` in an observed
//! `.sql` file becomes a `Custom("View")` object.
//!
//! Deterministic, no LLM. The file is split into statements on `ekos-plpgsql`'s lexer tokens, so a
//! semicolon inside a string, comment or dollar quote never splits one, and each statement keeps its
//! byte offset (and so its line). Each view definition is parsed **on its own** with the file's
//! resolved dialect: one view using syntax `sqlparser` lacks costs that view's footprint, never the
//! file, and the view is still emitted — named from its tokens — with `footprint: unparsed`.
//!
//! What a view's query reads and calls comes from `plpgsql_footprint` over the parsed query (the
//! same AST walk RFC 0163 uses for routines), so `procedure_lineage` can link the view to its
//! tables and link routines that read it, by unique name only.
//!
//! Keyed by `(source path, lower-cased name)`, like a routine: a view is a definition in a file. A
//! later definition in the same file replaces the earlier one (`CREATE OR REPLACE`).

use crate::plpgsql_footprint::{Footprint, query_footprint};
use crate::sql_comments::{ObjectCommentKind, extract_object_comments, match_object_comments};
use crate::sql_objects::{clip, file_kir_id};
use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_plpgsql::lex::{Tok, lex};
use ekos_plpgsql::{head_words, line_of, statements};
use serde_json::json;
use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Bumped whenever this pass's output changes for the same input, so a cached run is not reused.
const LOGIC_VERSION: &str = "view-analyzer/2";

/// The most source text a view's evidence carries; the exact span is always recorded.
const MAX_FRAGMENT: usize = 4096;

/// Per-file counters, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewStats {
    /// `View` objects written (after same-file redefinitions collapse).
    pub views: usize,
    /// Of those, whose definition parsed.
    pub parsed: usize,
    /// Definitions replaced by a later one of the same view in the same file.
    pub redefined: usize,
    /// Set when the file could not be lexed at all; nothing was recovered from it.
    pub lex_error: Option<String>,
}

pub struct ViewAnalyzerPass {
    pass_id: String,
    source_path: String,
    sql: String,
    dialect_name: String,
    dialect: Box<dyn Dialect + Send + Sync>,
    /// The owning `File` object's id key, as for routines (`PlPgSqlAnalyzerPass::with_file`).
    file_key: Option<String>,
    stats: Arc<Mutex<ViewStats>>,
}

impl ViewAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        dialect_name: impl Into<String>,
        dialect: Box<dyn Dialect + Send + Sync>,
    ) -> Self {
        let source_path = source_path.into();
        Self {
            pass_id: format!("view-analyzer:{source_path}"),
            sql: sql.into(),
            source_path,
            dialect_name: dialect_name.into(),
            dialect,
            file_key: None,
            stats: Arc::new(Mutex::new(ViewStats::default())),
        }
    }

    /// Attach every view to its `File` object (`Contains`), whose id is the v5 UUID of `file_key`.
    pub fn with_file(mut self, file_key: impl Into<String>) -> Self {
        self.file_key = Some(file_key.into());
        self
    }

    /// Handle onto this pass's counters, readable after the `PassManager` has taken the pass.
    pub fn stats_handle(&self) -> Arc<Mutex<ViewStats>> {
        Arc::clone(&self.stats)
    }

    /// Cheap gate: a file that never says `view` defines none.
    pub fn applies_to(sql: &str) -> bool {
        sql.to_ascii_lowercase().contains("view")
    }
}

#[async_trait]
impl CompilerPass for ViewAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(LOGIC_VERSION.as_bytes());
        hasher.update(self.source_path.as_bytes());
        hasher.update(self.dialect_name.as_bytes());
        hasher.update(self.file_key.as_deref().unwrap_or_default().as_bytes());
        hasher.update(self.sql.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let (graph, stats) = recover_views_in(
            &self.source_path,
            &self.sql,
            self.dialect.as_ref(),
            self.file_key.as_deref(),
        );
        *self.stats.lock().unwrap() = stats.clone();
        if let Some(e) = &stats.lex_error {
            tracing::warn!(pass = %self.pass_id, "view-analyzer: file does not lex: {e}");
        }
        if graph.objects.is_empty() {
            return Ok(());
        }
        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        tracing::info!(
            pass = %self.pass_id,
            views = stats.views,
            parsed = stats.parsed,
            "view-analyzer complete"
        );
        Ok(())
    }
}

/// Whether a statement defines a view: `CREATE [OR REPLACE] [TEMP|TEMPORARY] [MATERIALIZED]
/// [RECURSIVE] VIEW`. Returns how many leading words precede the name.
fn view_head(text: &str) -> Option<usize> {
    let words = head_words(text, 7);
    let mut i = 0;
    if words.get(i).map(String::as_str) != Some("CREATE") {
        return None;
    }
    i += 1;
    if words.get(i).map(String::as_str) == Some("OR")
        && words.get(i + 1).map(String::as_str) == Some("REPLACE")
    {
        i += 2;
    }
    while matches!(
        words.get(i).map(String::as_str),
        Some("TEMP" | "TEMPORARY" | "MATERIALIZED" | "RECURSIVE")
    ) {
        i += 1;
    }
    (words.get(i).map(String::as_str) == Some("VIEW")).then_some(i + 1)
}

/// The view's name from its tokens, for a definition `sqlparser` could not parse: the dotted
/// identifier after `VIEW` (and after `IF NOT EXISTS`).
fn name_from_tokens(text: &str) -> Option<String> {
    let toks = lex(text).ok()?;
    let mut i = toks
        .iter()
        .position(|t| matches!(&t.tok, Tok::Word(w) if w.eq_ignore_ascii_case("VIEW")))?
        + 1;
    let is = |i: usize, w: &str| matches!(toks.get(i).map(|t| &t.tok), Some(Tok::Word(x)) if x.eq_ignore_ascii_case(w));
    if is(i, "IF") && is(i + 1, "NOT") && is(i + 2, "EXISTS") {
        i += 3;
    }
    // `name` or `schema.name`: a word, then `.` + word as long as a dot follows. Taking every word
    // in a row read `weird as select` as one name.
    let mut name = String::new();
    while let Some(Tok::Word(w)) = toks.get(i).map(|t| &t.tok) {
        name.push_str(w);
        if matches!(toks.get(i + 1).map(|t| &t.tok), Some(Tok::Punct('.'))) {
            name.push('.');
            i += 2;
        } else {
            break;
        }
    }
    (!name.is_empty()).then(|| name.to_ascii_lowercase())
}

struct Definition<'a> {
    position: usize,
    name: String,
    text: &'a str,
    offset: usize,
    parsed: Option<Statement>,
    error: Option<String>,
}

/// Recover every view defined in one SQL file. Pure: same input, same graph, ids included.
pub fn recover_views(source_path: &str, sql: &str, dialect: &dyn Dialect) -> (KirGraph, ViewStats) {
    recover_views_in(source_path, sql, dialect, None)
}

/// [`recover_views`], with each view attached to the `File` whose id key is `file_key`.
pub fn recover_views_in(
    source_path: &str,
    sql: &str,
    dialect: &dyn Dialect,
    file_key: Option<&str>,
) -> (KirGraph, ViewStats) {
    let mut stats = ViewStats::default();
    let mut graph = KirGraph::new();
    let found = match statements(sql) {
        Ok(s) => s,
        Err(e) => {
            stats.lex_error = Some(e.to_string());
            return (graph, stats);
        }
    };

    let mut by_key: BTreeMap<String, Definition> = BTreeMap::new();
    for (position, st) in found.iter().enumerate() {
        if view_head(st.text).is_none() {
            continue;
        }
        let (parsed, error) = match Parser::parse_sql(dialect, st.text) {
            Ok(mut v) if v.len() == 1 && matches!(v[0], Statement::CreateView { .. }) => {
                (v.pop(), None)
            }
            Ok(_) => (None, Some("not a single CREATE VIEW statement".to_string())),
            Err(e) => (None, Some(e.to_string())),
        };
        let name = match &parsed {
            Some(Statement::CreateView { name, .. }) => name
                .0
                .iter()
                .map(|i| {
                    if i.quote_style.is_some() {
                        i.value.clone()
                    } else {
                        i.value.to_ascii_lowercase()
                    }
                })
                .collect::<Vec<_>>()
                .join("."),
            _ => match name_from_tokens(st.text) {
                Some(n) => n,
                None => continue,
            },
        };
        let def = Definition {
            position,
            name: name.clone(),
            text: st.text,
            offset: st.offset,
            parsed,
            error,
        };
        if by_key
            .insert(format!("{source_path}:{}", name.to_lowercase()), def)
            .is_some()
        {
            stats.redefined += 1;
        }
    }
    let mut ordered: Vec<(String, Definition)> = by_key.into_iter().collect();
    ordered.sort_by_key(|(_, d)| d.position);

    // `COMMENT ON [MATERIALIZED] VIEW` in the same file: the author's own description.
    let comments = extract_object_comments(sql);
    let candidates: Vec<(String, usize)> =
        ordered.iter().map(|(_, d)| (d.name.clone(), 0)).collect();
    let docs = match_object_comments(&comments, &[ObjectCommentKind::View], &candidates);
    let file_id = file_key.map(file_kir_id);

    for (index, (key, def)) in ordered.iter().enumerate() {
        let line = line_of(sql, def.offset);
        let mut evidence = KirEvidence::new(
            SourceLocation {
                path: source_path.to_string(),
                line: Some(line),
                column: None,
            },
            clip(def.text, MAX_FRAGMENT),
        );
        evidence.id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("view-evidence:{key}").as_bytes(),
        ));
        let evidence_id = graph.add_evidence(evidence);

        let mut obj = KirObject::new(def.name.clone(), ObjectKind::Custom("View".into()));
        obj.id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("view:{key}").as_bytes(),
        ));
        let (fp, materialized, temporary, or_replace, columns) = match &def.parsed {
            Some(Statement::CreateView {
                query,
                materialized,
                temporary,
                or_replace,
                columns,
                ..
            }) => (
                query_footprint(query),
                *materialized,
                *temporary,
                *or_replace,
                columns.iter().map(|c| c.name.value.clone()).collect(),
            ),
            _ => {
                // What the statement says about itself is still known from its words.
                let words = head_words(def.text, 6);
                let has = |w: &str| words.iter().any(|x| x == w);
                let fp = Footprint {
                    errors: def.error.iter().cloned().collect(),
                    fragments: 1,
                    ..Default::default()
                };
                (
                    fp,
                    has("MATERIALIZED"),
                    has("TEMP") || has("TEMPORARY"),
                    has("REPLACE"),
                    Vec::<String>::new(),
                )
            }
        };
        stats.views += 1;
        if fp.errors.is_empty() {
            stats.parsed += 1;
        }
        for (k, v) in [
            ("materialized", json!(materialized)),
            ("temporary", json!(temporary)),
            ("or_replace", json!(or_replace)),
            ("columns", json!(columns)),
            ("reads", json!(fp.reads)),
            ("calls", json!(fp.calls)),
            ("footprint", json!(fp.status())),
            ("source_path", json!(source_path)),
            ("line", json!(line)),
            ("span_start", json!(def.offset)),
            ("span_end", json!(def.offset + def.text.len())),
        ] {
            obj.properties.insert(k.into(), v);
        }
        if !fp.errors.is_empty() {
            obj.properties
                .insert("footprint_errors".into(), json!(fp.errors));
        }
        obj.properties.insert(
            "source_span".into(),
            json!({"start_line": line, "end_line": line_of(sql, def.offset + def.text.len())}),
        );
        obj.evidence.push(evidence_id);
        if let Some(c) = docs.get(&index) {
            let mut ev = KirEvidence::new(
                SourceLocation {
                    path: source_path.to_string(),
                    line: Some(c.line),
                    column: None,
                },
                clip(
                    &format!("COMMENT ON VIEW {} IS {}", c.name, c.text),
                    MAX_FRAGMENT,
                ),
            );
            ev.id = KirId(Uuid::new_v5(
                &Uuid::NAMESPACE_URL,
                format!("view-comment:{key}").as_bytes(),
            ));
            obj.evidence.push(graph.add_evidence(ev));
            obj.properties.insert("description".into(), json!(c.text));
        }
        if let Some(file_id) = file_id {
            graph.add_relationship(KirRelationship::deterministic(
                RelationshipKind::Contains,
                file_id,
                obj.id,
                "",
            ));
        }
        graph.add_object(obj);
    }
    (graph, stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sqlparser::dialect::PostgreSqlDialect;

    const FILE: &str = "CREATE TABLE acc_trans (id int);\n\
-- a view; with a semicolon in a comment\n\
CREATE OR REPLACE VIEW account_heading_tree (id, path) AS\n\
  WITH RECURSIVE t AS (SELECT id FROM account_heading UNION ALL SELECT id FROM t)\n\
  SELECT id, in_tree(id) FROM t JOIN account a USING (id);\n\
COMMENT ON VIEW account_heading_tree IS $$ CREATE VIEW not_a_view AS SELECT 1; $$;\n\
DROP VIEW IF EXISTS old_view;\n\
CREATE MATERIALIZED VIEW periods AS SELECT * FROM generate_series(1, 12) g;\n\
create temporary view weird as select * from t where a ~~~ b @@@ ((;\n";

    fn views(g: &KirGraph) -> Vec<&KirObject> {
        g.objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "View"))
            .collect()
    }

    fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Value {
        o.properties.get(k).unwrap_or(&Value::Null)
    }

    #[test]
    fn every_view_form_is_found_and_nothing_else_is() {
        let (g, stats) = recover_views("schema.sql", FILE, &PostgreSqlDialect {});
        let names: Vec<&str> = views(&g).iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, ["account_heading_tree", "periods", "weird"]);
        assert_eq!(stats.views, 3);
        assert_eq!(stats.parsed, 2);
    }

    #[test]
    fn a_parsed_view_records_its_shape_and_what_its_query_touches() {
        let (g, _) = recover_views("schema.sql", FILE, &PostgreSqlDialect {});
        let v = views(&g)[0];
        assert_eq!(prop(v, "columns"), &json!(["id", "path"]));
        assert_eq!(prop(v, "or_replace"), &json!(true));
        assert_eq!(prop(v, "materialized"), &json!(false));
        // The recursive CTE `t` is not a table; the view's own name is not one of its reads.
        assert_eq!(prop(v, "reads"), &json!(["account", "account_heading"]));
        assert_eq!(prop(v, "calls"), &json!(["in_tree"]));
        assert_eq!(prop(v, "footprint"), &json!("parsed"));
        assert_eq!(prop(v, "line"), &json!(3));

        let periods = views(&g)[1];
        assert_eq!(prop(periods, "materialized"), &json!(true));
        // A table function is a call, not a table.
        assert_eq!(prop(periods, "reads"), &json!([]));
        assert_eq!(prop(periods, "calls"), &json!(["generate_series"]));

        let ev = g.evidence.iter().find(|e| e.id == v.evidence[0]).unwrap();
        assert!(
            ev.fragment
                .starts_with("CREATE OR REPLACE VIEW account_heading_tree")
        );
        assert_eq!(ev.location.line, Some(3));
    }

    /// A definition `sqlparser` cannot parse is still a view: named from its tokens, its flags
    /// read from its words, and the parser's reason kept.
    #[test]
    fn an_unparseable_view_is_still_emitted_and_says_why() {
        let (g, _) = recover_views("schema.sql", FILE, &PostgreSqlDialect {});
        let v = views(&g)[2];
        assert_eq!(v.name, "weird");
        assert_eq!(prop(v, "temporary"), &json!(true));
        assert_eq!(prop(v, "footprint"), &json!("unparsed"));
        assert_eq!(prop(v, "footprint_errors").as_array().unwrap().len(), 1);
        assert_eq!(prop(v, "reads"), &json!([]));

        let (g, _) = recover_views(
            "f.sql",
            "CREATE VIEW IF NOT EXISTS app.v2 AS SELECT ((;",
            &PostgreSqlDialect {},
        );
        assert_eq!(views(&g)[0].name, "app.v2");
    }

    #[test]
    fn keys_are_file_and_name_and_a_later_definition_in_a_file_replaces_the_earlier() {
        let sql = "CREATE VIEW v AS SELECT 1 FROM a;\nCREATE OR REPLACE VIEW V AS SELECT 1 FROM b;";
        let (g, stats) = recover_views("f.sql", sql, &PostgreSqlDialect {});
        assert_eq!(views(&g).len(), 1);
        assert_eq!(stats.redefined, 1);
        assert_eq!(prop(views(&g)[0], "reads"), &json!(["b"]));

        let (other, _) = recover_views("g.sql", sql, &PostgreSqlDialect {});
        assert_ne!(views(&g)[0].id, views(&other)[0].id);
        let (again, _) = recover_views("f.sql", sql, &PostgreSqlDialect {});
        assert_eq!(views(&g)[0].id, views(&again)[0].id);
        assert_eq!(g.evidence[0].id, again.evidence[0].id);
    }

    #[test]
    fn a_view_belongs_to_its_file_and_carries_its_documented_description() {
        let sql = "CREATE VIEW v AS\n  SELECT 1 FROM t;\nCOMMENT ON VIEW v IS 'the v view';";
        let (g, _) = recover_views_in("db/v.sql", sql, &PostgreSqlDialect {}, Some("db/v.sql"));
        let v = views(&g)[0];
        let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, b"db/v.sql"));
        assert!(
            g.relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains && r.from == file_id && r.to == v.id)
        );
        assert_eq!(
            prop(v, "source_span"),
            &json!({"start_line": 1, "end_line": 2})
        );
        assert_eq!(prop(v, "description"), &json!("the v view"));
        assert_eq!(v.evidence.len(), 2);
    }

    #[test]
    fn a_file_that_does_not_lex_recovers_nothing_and_says_why() {
        let (g, stats) =
            recover_views("f.sql", "CREATE VIEW v AS SELECT 'x", &PostgreSqlDialect {});
        assert!(g.objects.is_empty());
        assert!(stats.lex_error.is_some());
    }
}
