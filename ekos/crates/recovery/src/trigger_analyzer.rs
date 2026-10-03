//! `TriggerAnalyzerPass` — RFC 0163's *Triggers*: every `CREATE [OR REPLACE] [CONSTRAINT] TRIGGER`
//! in an observed `.sql` file becomes a `Custom("Trigger")` object — the binding of timing, events,
//! level and condition to a table and a function.
//!
//! The trigger's *function* is a `Procedure` (RFC 0163), often in another file, so what the trigger
//! **does** is decided later, over the whole graph: `ekos_semantic::triggers` links the trigger to
//! its table and function and classifies it from the function's IR facts. This pass records only
//! what the `CREATE TRIGGER` statement itself says.
//!
//! Same discipline as the view pass (RFC 0169): statements split on `ekos-plpgsql`'s lexer, each
//! definition parsed on its own with the file's dialect, never dropped — one `sqlparser` cannot
//! read is still emitted from its tokens, with the parser's reason. Keyed by
//! `(source path, table, trigger name)`: trigger names are unique per table, not per schema.

use crate::sql_objects::{clip, file_kir_id};
use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_plpgsql::lex::{Tok, lex};
use ekos_plpgsql::{head_words, line_of, statements};
use serde_json::{Value, json};
use sqlparser::ast::{ObjectName, Statement, TriggerObject};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const LOGIC_VERSION: &str = "trigger-analyzer/1";
const MAX_FRAGMENT: usize = 4096;

/// Per-file counters, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerStats {
    pub triggers: usize,
    pub parsed: usize,
    pub lex_error: Option<String>,
}

pub struct TriggerAnalyzerPass {
    pass_id: String,
    source_path: String,
    sql: String,
    dialect_name: String,
    dialect: Box<dyn Dialect + Send + Sync>,
    file_key: Option<String>,
    stats: Arc<Mutex<TriggerStats>>,
}

impl TriggerAnalyzerPass {
    pub fn new(
        source_path: impl Into<String>,
        sql: impl Into<String>,
        dialect_name: impl Into<String>,
        dialect: Box<dyn Dialect + Send + Sync>,
    ) -> Self {
        let source_path = source_path.into();
        Self {
            pass_id: format!("trigger-analyzer:{source_path}"),
            sql: sql.into(),
            source_path,
            dialect_name: dialect_name.into(),
            dialect,
            file_key: None,
            stats: Arc::new(Mutex::new(TriggerStats::default())),
        }
    }

    /// Attach every trigger to its `File` object (`Contains`), as routines and views are.
    pub fn with_file(mut self, file_key: impl Into<String>) -> Self {
        self.file_key = Some(file_key.into());
        self
    }

    pub fn stats_handle(&self) -> Arc<Mutex<TriggerStats>> {
        Arc::clone(&self.stats)
    }

    /// Cheap gate: a file that never says `trigger` defines none.
    pub fn applies_to(sql: &str) -> bool {
        sql.to_ascii_lowercase().contains("trigger")
    }
}

#[async_trait]
impl CompilerPass for TriggerAnalyzerPass {
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
        let (graph, stats) = recover_triggers_in(
            &self.source_path,
            &self.sql,
            self.dialect.as_ref(),
            self.file_key.as_deref(),
        );
        *self.stats.lock().unwrap() = stats.clone();
        if let Some(e) = &stats.lex_error {
            tracing::warn!(pass = %self.pass_id, "trigger-analyzer: file does not lex: {e}");
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
        Ok(())
    }
}

/// `CREATE [OR REPLACE] [CONSTRAINT] TRIGGER`.
fn is_trigger_definition(text: &str) -> bool {
    let words = head_words(text, 5);
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    matches!(
        w.as_slice(),
        ["CREATE", "TRIGGER", ..]
            | ["CREATE", "CONSTRAINT", "TRIGGER", ..]
            | ["CREATE", "OR", "REPLACE", "TRIGGER", ..]
            | ["CREATE", "OR", "REPLACE", "CONSTRAINT", "TRIGGER"]
    )
}

fn object_name(n: &ObjectName) -> String {
    n.0.iter()
        .map(|i| {
            if i.quote_style.is_some() {
                i.value.clone()
            } else {
                i.value.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

/// What a `CREATE TRIGGER` says, from the AST or — when `sqlparser` cannot parse it — from its
/// tokens.
#[derive(Default)]
struct Binding {
    name: String,
    table: String,
    function: Option<String>,
    timing: Option<String>,
    events: Vec<String>,
    level: Option<String>,
    condition: Option<String>,
    constraint: bool,
    error: Option<String>,
}

fn from_ast(stmt: &Statement) -> Option<Binding> {
    let Statement::CreateTrigger {
        is_constraint,
        name,
        period,
        events,
        table_name,
        trigger_object,
        include_each,
        condition,
        exec_body,
        ..
    } = stmt
    else {
        return None;
    };
    Some(Binding {
        name: object_name(name),
        table: object_name(table_name),
        function: Some(object_name(&exec_body.func_desc.name)),
        timing: Some(period.to_string()),
        events: events.iter().map(|e| e.to_string()).collect(),
        // PostgreSQL defaults to FOR EACH STATEMENT when no FOR EACH clause is given.
        level: Some(
            match (include_each, trigger_object) {
                (_, TriggerObject::Row) => "ROW",
                _ => "STATEMENT",
            }
            .to_string(),
        ),
        condition: condition.as_ref().map(|c| c.to_string()),
        constraint: *is_constraint,
        error: None,
    })
}

/// The word after `TRIGGER` is the name, the dotted word after `ON` the table, the dotted word
/// after `EXECUTE PROCEDURE|FUNCTION` the function.
fn from_tokens(text: &str, error: String) -> Option<Binding> {
    let toks = lex(text).ok()?;
    let word = |i: usize| match toks.get(i).map(|t| &t.tok) {
        Some(Tok::Word(w)) => Some(w.clone()),
        _ => None,
    };
    let is = |i: usize, w: &str| word(i).is_some_and(|x| x.eq_ignore_ascii_case(w));
    let dotted = |mut i: usize| {
        let mut name = String::new();
        while let Some(w) = word(i) {
            name.push_str(&w.to_ascii_lowercase());
            if matches!(toks.get(i + 1).map(|t| &t.tok), Some(Tok::Punct('.'))) {
                name.push('.');
                i += 2;
            } else {
                break;
            }
        }
        (!name.is_empty()).then_some(name)
    };
    let at = (0..toks.len()).find(|&i| is(i, "TRIGGER"))?;
    let name = word(at + 1)?.to_ascii_lowercase();
    let on = (at..toks.len()).find(|&i| is(i, "ON"))?;
    let table = dotted(on + 1)?;
    let function = (on..toks.len())
        .find(|&i| is(i, "EXECUTE") && (is(i + 1, "PROCEDURE") || is(i + 1, "FUNCTION")))
        .and_then(|i| dotted(i + 2));
    // Timing and events sit between the name and `ON`: `BEFORE INSERT OR UPDATE OF a, b`.
    let (mut timing, mut events) = (None, Vec::new());
    let mut i = at + 2;
    if is(i, "BEFORE") || is(i, "AFTER") {
        timing = word(i).map(|w| w.to_ascii_uppercase());
        i += 1;
    } else if is(i, "INSTEAD") && is(i + 1, "OF") {
        timing = Some("INSTEAD OF".to_string());
        i += 2;
    }
    let mut current: Vec<String> = Vec::new();
    while i < on {
        match &toks[i].tok {
            Tok::Word(w) if w.eq_ignore_ascii_case("OR") => {
                events.push(current.join(" "));
                current.clear();
            }
            Tok::Word(w) => {
                let w = if current.is_empty() || w.eq_ignore_ascii_case("OF") {
                    w.to_ascii_uppercase()
                } else {
                    w.clone()
                };
                current.push(w);
            }
            Tok::Punct(',') => {
                if let Some(last) = current.last_mut() {
                    last.push(',');
                }
            }
            _ => {}
        }
        i += 1;
    }
    if !current.is_empty() {
        events.push(current.join(" "));
    }
    // `FOR [EACH] ROW`, else PostgreSQL's default.
    let row = (on..toks.len())
        .any(|i| is(i, "FOR") && (is(i + 1, "ROW") || (is(i + 1, "EACH") && is(i + 2, "ROW"))));
    let words = head_words(text, 12);
    Some(Binding {
        name,
        table,
        function,
        timing,
        events,
        level: Some(if row { "ROW" } else { "STATEMENT" }.to_string()),
        constraint: words.iter().take(5).any(|w| w == "CONSTRAINT"),
        error: Some(error),
        ..Default::default()
    })
}

/// Recover every trigger defined in one SQL file. Pure: same input, same graph, ids included.
pub fn recover_triggers_in(
    source_path: &str,
    sql: &str,
    dialect: &dyn Dialect,
    file_key: Option<&str>,
) -> (KirGraph, TriggerStats) {
    let mut stats = TriggerStats::default();
    let mut graph = KirGraph::new();
    let found = match statements(sql) {
        Ok(s) => s,
        Err(e) => {
            stats.lex_error = Some(e.to_string());
            return (graph, stats);
        }
    };

    // Keyed (path, table, name); a later definition in the same file replaces the earlier one.
    let mut by_key: BTreeMap<String, (usize, Binding, &str, usize)> = BTreeMap::new();
    for (position, st) in found.iter().enumerate() {
        if !is_trigger_definition(st.text) {
            continue;
        }
        let binding = match Parser::parse_sql(dialect, st.text) {
            Ok(v) if v.len() == 1 => from_ast(&v[0]),
            Ok(_) => from_tokens(st.text, "not a single CREATE TRIGGER statement".into()),
            Err(e) => from_tokens(st.text, e.to_string()),
        };
        let Some(b) = binding else {
            continue;
        };
        let key = format!("{source_path}:{}:{}", b.table, b.name);
        by_key.insert(key, (position, b, st.text, st.offset));
    }
    let mut ordered: Vec<_> = by_key.into_iter().collect();
    ordered.sort_by_key(|(_, (position, ..))| *position);

    let file_id = file_key.map(file_kir_id);
    for (key, (_, b, text, offset)) in &ordered {
        let line = line_of(sql, *offset);
        let mut evidence = KirEvidence::new(
            SourceLocation {
                path: source_path.to_string(),
                line: Some(line),
                column: None,
            },
            clip(text, MAX_FRAGMENT),
        );
        evidence.id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("trigger-evidence:{key}").as_bytes(),
        ));
        let evidence_id = graph.add_evidence(evidence);

        let mut obj = KirObject::new(b.name.clone(), ObjectKind::Custom("Trigger".into()));
        obj.id = KirId(Uuid::new_v5(
            &Uuid::NAMESPACE_URL,
            format!("trigger:{key}").as_bytes(),
        ));
        let opt = |v: &Option<String>| v.as_ref().map_or(Value::Null, |s| json!(s));
        for (k, v) in [
            ("table", json!(b.table)),
            ("function", opt(&b.function)),
            ("timing", opt(&b.timing)),
            ("events", json!(b.events)),
            ("level", opt(&b.level)),
            ("condition", opt(&b.condition)),
            ("constraint", json!(b.constraint)),
            ("parsed", json!(b.error.is_none())),
            ("source_path", json!(source_path)),
            ("line", json!(line)),
            ("span_start", json!(offset)),
            ("span_end", json!(offset + text.len())),
            (
                "source_span",
                json!({"start_line": line, "end_line": line_of(sql, offset + text.len())}),
            ),
        ] {
            obj.properties.insert(k.into(), v);
        }
        if let Some(e) = &b.error {
            obj.properties.insert("parse_error".into(), json!(e));
        }
        obj.evidence.push(evidence_id);
        stats.triggers += 1;
        if b.error.is_none() {
            stats.parsed += 1;
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
    use sqlparser::dialect::PostgreSqlDialect;

    const FILE: &str = "CREATE TRIGGER eca_maintain_b_units AFTER INSERT OR UPDATE\n\
       ON entity_credit_account\n\
       FOR EACH ROW EXECUTE PROCEDURE eca_bu_trigger();\n\
CREATE TRIGGER cr_report_links_update AFTER UPDATE OF submitted ON cr_report\n\
    FOR EACH ROW WHEN (NEW.submitted) EXECUTE PROCEDURE cr_report_submitted_update();\n\
CREATE CONSTRAINT TRIGGER check_balance AFTER INSERT ON journal_line\n\
    DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.check_balance();\n\
CREATE TRIGGER stmt_level BEFORE TRUNCATE ON t EXECUTE PROCEDURE no_truncate();\n\
COMMENT ON TRIGGER stmt_level ON t IS 'not a trigger definition';\n\
DROP TRIGGER IF EXISTS gone ON t;\n";

    fn triggers(g: &KirGraph) -> Vec<&KirObject> {
        g.objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == "Trigger"))
            .collect()
    }

    fn prop<'a>(o: &'a KirObject, k: &str) -> &'a Value {
        o.properties.get(k).unwrap_or(&Value::Null)
    }

    #[test]
    fn every_trigger_binding_is_recovered_and_nothing_else_is() {
        let (g, stats) = recover_triggers_in("t.sql", FILE, &PostgreSqlDialect {}, Some("t.sql"));
        let t = triggers(&g);
        let names: Vec<&str> = t.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "eca_maintain_b_units",
                "cr_report_links_update",
                "check_balance",
                "stmt_level"
            ]
        );
        assert_eq!(stats.triggers, 4);

        let eca = t[0];
        assert_eq!(prop(eca, "table"), &json!("entity_credit_account"));
        assert_eq!(prop(eca, "function"), &json!("eca_bu_trigger"));
        assert_eq!(prop(eca, "timing"), &json!("AFTER"));
        assert_eq!(prop(eca, "events"), &json!(["INSERT", "UPDATE"]));
        assert_eq!(prop(eca, "level"), &json!("ROW"));
        assert_eq!(prop(eca, "line"), &json!(1));

        let cr = t[1];
        assert_eq!(prop(cr, "events"), &json!(["UPDATE OF submitted"]));
        assert_eq!(
            prop(cr, "condition"),
            &json!("(NEW.submitted)"),
            "as written"
        );

        assert_eq!(prop(t[2], "constraint"), &json!(true));
        // sqlparser 0.53 requires `FOR EACH`, which PostgreSQL makes optional — read from tokens,
        // with PostgreSQL's default level.
        assert_eq!(prop(t[3], "level"), &json!("STATEMENT"));
        assert_eq!(prop(t[3], "timing"), &json!("BEFORE"));
        assert_eq!(prop(t[3], "events"), &json!(["TRUNCATE"]));
        assert_eq!(prop(t[3], "function"), &json!("no_truncate"));

        let file = file_kir_id("t.sql");
        assert!(t.iter().all(|o| {
            g.relationships
                .iter()
                .any(|r| r.kind == RelationshipKind::Contains && r.from == file && r.to == o.id)
        }));
    }

    /// A definition `sqlparser` cannot parse is still a trigger, its binding read from its tokens.
    #[test]
    fn an_unparseable_trigger_is_still_recovered_from_its_tokens() {
        let sql = "CREATE TRIGGER odd INSTEAD OF INSERT ON v_view REFERENCING NEW TABLE AS x \
                   FOR EACH ROW ((( EXECUTE FUNCTION app.v_insert();";
        let (g, _) = recover_triggers_in("t.sql", sql, &PostgreSqlDialect {}, None);
        let t = triggers(&g);
        assert_eq!(t.len(), 1);
        assert_eq!(prop(t[0], "table"), &json!("v_view"));
        assert_eq!(prop(t[0], "function"), &json!("app.v_insert"));
        assert_eq!(prop(t[0], "timing"), &json!("INSTEAD OF"));
        assert_eq!(prop(t[0], "events"), &json!(["INSERT"]));
        assert_eq!(prop(t[0], "level"), &json!("ROW"));
        assert_eq!(prop(t[0], "parsed"), &json!(false));
        assert!(prop(t[0], "parse_error").is_string());
    }

    #[test]
    fn token_fallback_reads_multi_event_lists_with_update_columns() {
        let sql = "CREATE TRIGGER t2 AFTER INSERT OR UPDATE OF a, b OR DELETE ON t \
                   EXECUTE PROCEDURE f() ((( ;";
        let (g, _) = recover_triggers_in("t.sql", sql, &PostgreSqlDialect {}, None);
        let t = triggers(&g);
        assert_eq!(
            prop(t[0], "events"),
            &json!(["INSERT", "UPDATE OF a, b", "DELETE"])
        );
        assert_eq!(prop(t[0], "level"), &json!("STATEMENT"));
    }

    #[test]
    fn keys_are_file_table_and_name() {
        let sql = "CREATE TRIGGER tr AFTER INSERT ON a FOR EACH ROW EXECUTE PROCEDURE f();\n\
                   CREATE TRIGGER tr AFTER INSERT ON b FOR EACH ROW EXECUTE PROCEDURE f();";
        let (g, _) = recover_triggers_in("t.sql", sql, &PostgreSqlDialect {}, None);
        let t = triggers(&g);
        assert_eq!(
            t.len(),
            2,
            "same trigger name on two tables is two triggers"
        );
        assert_ne!(t[0].id, t[1].id);
        let (again, _) = recover_triggers_in("t.sql", sql, &PostgreSqlDialect {}, None);
        assert_eq!(t[0].id, triggers(&again)[0].id);
    }
}
