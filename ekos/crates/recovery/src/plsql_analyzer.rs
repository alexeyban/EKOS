//! `PlSqlAnalyzerPass` — RFC 0210 §3.4: every PL/SQL routine in an Oracle `.sql` file, read by
//! `ekos-plsql` into RFC 0163's procedural IR and written exactly the way `PlPgSqlAnalyzerPass`
//! writes a PL/pgSQL routine: one `Custom("Procedure")` with its computed fidelity, one
//! `Custom("ProcedureStatement")` per statement with its footprint, `Contains` edges carrying the
//! branch. `semantic::procedure_lineage`, docs and MCP read them without knowing the language.
//!
//! An Oracle trigger carries its body inline, so the body becomes a `Procedure` named after the
//! trigger (`returns: trigger`, so RFC 0163's trigger facts are computed), and the binding a
//! `Custom("Trigger")` keyed `(file, table, name)` with `function` naming that body — the shape
//! `TriggerAnalyzerPass` gives a PostgreSQL trigger, so `semantic::triggers` links and classifies
//! both alike.
//!
//! Deterministic and side-effect-free, no LLM. Runs on files resolved to the `oracle` dialect,
//! on the text as written (redacted): spans and lines cite the file.

use crate::plpgsql_analyzer::{ParsedRoutine, PlPgSqlStats, recover_parsed_routines};
use crate::sql_objects::{clip, file_kir_id};
use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_plpgsql::line_of;
use ekos_plsql::{Routine, read_script};
use serde_json::json;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Bumped whenever this pass's output changes for the same input.
const LOGIC_VERSION: &str = "plsql-analyzer/1";
const MAX_FRAGMENT: usize = 4096;

/// Per-file counters, for `ekos recover`'s summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlSqlStats {
    /// The routine counters, as for PL/pgSQL.
    pub routines: PlPgSqlStats,
    /// `Trigger` objects written.
    pub triggers: usize,
    /// Anonymous blocks: counted, not recovered.
    pub anonymous_blocks: usize,
    /// PL/SQL units with no `/` line after them.
    pub unterminated: usize,
    /// Units defining something not read here (`TYPE`, `JAVA SOURCE`).
    pub skipped: usize,
}

pub struct PlSqlAnalyzerPass {
    pass_id: String,
    source_path: String,
    sql: String,
    file_key: Option<String>,
    stats: Arc<Mutex<PlSqlStats>>,
}

impl PlSqlAnalyzerPass {
    pub fn new(source_path: impl Into<String>, sql: impl Into<String>) -> Self {
        let source_path = source_path.into();
        Self {
            pass_id: format!("plsql-analyzer:{source_path}"),
            sql: sql.into(),
            source_path,
            file_key: None,
            stats: Arc::new(Mutex::new(PlSqlStats::default())),
        }
    }

    /// Attach every routine and trigger to its `File` object (`Contains`).
    pub fn with_file(mut self, file_key: impl Into<String>) -> Self {
        self.file_key = Some(file_key.into());
        self
    }

    pub fn stats_handle(&self) -> Arc<Mutex<PlSqlStats>> {
        Arc::clone(&self.stats)
    }

    /// Files resolved to the Oracle dialect. A PL/pgSQL file read as PL/SQL would yield
    /// confident-looking routines that are really another language.
    pub fn applies_to(dialect_name: &str) -> bool {
        matches!(dialect_name, "oracle" | "plsql")
    }
}

#[async_trait]
impl CompilerPass for PlSqlAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(LOGIC_VERSION.as_bytes());
        hasher.update(crate::sql_predicates::PREDICATES_VERSION.as_bytes());
        hasher.update(self.source_path.as_bytes());
        hasher.update(self.file_key.as_deref().unwrap_or_default().as_bytes());
        hasher.update(self.sql.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let (graph, stats) = recover_plsql(&self.source_path, &self.sql, self.file_key.as_deref());
        *self.stats.lock().unwrap() = stats.clone();
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
            routines = stats.routines.routines,
            complete = stats.routines.complete,
            partial = stats.routines.partial,
            triggers = stats.triggers,
            "plsql-analyzer complete"
        );
        Ok(())
    }
}

/// Recover every PL/SQL routine and trigger in one Oracle file. Pure.
pub fn recover_plsql(
    source_path: &str,
    sql: &str,
    file_key: Option<&str>,
) -> (KirGraph, PlSqlStats) {
    let script = read_script(sql);
    let parsed = script
        .routines
        .iter()
        .map(|r| ParsedRoutine {
            ir: r.ir.clone(),
            offset: r.offset,
            text: &sql[r.offset..r.offset + r.len],
            header_len: Some(r.header_len),
        })
        .collect();
    let (mut graph, routines) = recover_parsed_routines(source_path, sql, file_key, parsed);
    let mut stats = PlSqlStats {
        routines,
        anonymous_blocks: script.anonymous_blocks,
        unterminated: script.unterminated,
        skipped: script.skipped.len(),
        ..Default::default()
    };
    let file_id = file_key.map(file_kir_id);
    for r in &script.routines {
        if add_trigger(&mut graph, source_path, sql, r, file_id) {
            stats.triggers += 1;
        }
    }
    (graph, stats)
}

/// A trigger's binding, in `TriggerAnalyzerPass`'s shape. `false` when `r` is not a trigger.
fn add_trigger(
    graph: &mut KirGraph,
    source_path: &str,
    sql: &str,
    r: &Routine,
    file_id: Option<KirId>,
) -> bool {
    let Some(t) = &r.trigger else {
        return false;
    };
    let name = &r.ir.signature.name;
    let key = format!("{source_path}:{}:{name}", t.table);
    let id = |seed: String| KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()));
    let line = line_of(sql, r.offset);
    let header = &sql[r.offset..r.offset + r.header_len.min(r.len)];
    let mut evidence = KirEvidence::new(
        SourceLocation {
            path: source_path.to_string(),
            line: Some(line),
            column: None,
        },
        clip(header.trim_end(), MAX_FRAGMENT),
    );
    evidence.id = id(format!("trigger-evidence:{key}"));
    let evidence_id = graph.add_evidence(evidence);

    let mut obj = KirObject::new(name.clone(), ObjectKind::Custom("Trigger".into()));
    obj.id = id(format!("trigger:{key}"));
    let end = r.offset + r.len;
    for (k, v) in [
        ("table", json!(t.table)),
        ("function", json!(name)),
        ("timing", json!(t.timing)),
        ("events", json!(t.events)),
        ("level", json!(t.level)),
        ("condition", json!(t.condition)),
        ("constraint", json!(false)),
        ("parsed", json!(true)),
        ("language", json!("plsql")),
        ("source_path", json!(source_path)),
        ("line", json!(line)),
        ("span_start", json!(r.offset)),
        ("span_end", json!(end)),
        (
            "source_span",
            json!({"start_line": line, "end_line": line_of(sql, end)}),
        ),
    ] {
        obj.properties.insert(k.into(), v);
    }
    obj.evidence.push(evidence_id);
    if let Some(file_id) = file_id {
        graph.add_relationship(KirRelationship::deterministic(
            RelationshipKind::Contains,
            file_id,
            obj.id,
            "",
        ));
    }
    graph.add_object(obj);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const SRC: &str = "SET DEFINE OFF
CREATE OR REPLACE FUNCTION invoiceOpen (p_C_Invoice_ID IN NUMBER)
RETURN NUMBER
AS
    v_Total NUMBER := 0;
BEGIN
    SELECT SUM(GrandTotal) INTO v_Total FROM C_Invoice WHERE C_Invoice_ID = p_C_Invoice_ID;
    UPDATE C_Invoice SET IsPaid = 'Y' WHERE C_Invoice_ID = p_C_Invoice_ID;
    RETURN v_Total;
END invoiceOpen;
/
CREATE OR REPLACE TRIGGER C_BPartner_Trg
BEFORE INSERT OR UPDATE ON C_BPartner
FOR EACH ROW
BEGIN
    :new.Value := UPPER(:new.Name);
END;
/
";

    fn custom<'a>(g: &'a KirGraph, kind: &str) -> Vec<&'a KirObject> {
        g.objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == kind))
            .collect()
    }

    #[test]
    fn routines_and_triggers_are_written_like_plpgsql_ones() {
        let (g, stats) = recover_plsql("db/oracle/x.sql", SRC, Some("x.sql"));
        let procs = custom(&g, "Procedure");
        let names: Vec<&str> = procs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["invoiceOpen", "C_BPartner_Trg"]);
        assert_eq!(procs[0].properties["language"], "plsql");
        assert_eq!(procs[0].properties["fidelity"], "statements");
        assert_eq!(procs[0].properties["line"], 2);
        let reads: Vec<String> =
            serde_json::from_value(procs[0].properties["reads"].clone()).unwrap();
        assert!(
            reads.iter().any(|r| r.eq_ignore_ascii_case("c_invoice")),
            "{reads:?}"
        );
        assert_eq!(procs[1].properties["assigns_new"], json!(["value"]));
        assert_eq!(stats.routines.routines, 2);
        assert_eq!(stats.routines.complete, 2);
        assert_eq!(stats.triggers, 1);

        let triggers = custom(&g, "Trigger");
        assert_eq!(triggers.len(), 1);
        let t = triggers[0];
        assert_eq!(t.properties["table"], "C_BPartner");
        assert_eq!(t.properties["function"], "C_BPartner_Trg");
        assert_eq!(t.properties["events"], json!(["INSERT", "UPDATE"]));
        assert_eq!(t.properties["level"], "ROW");
        assert_eq!(t.properties["line"], 12);
        assert!(!custom(&g, "ProcedureStatement").is_empty());
        let contained = g
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Contains && r.from == file_kir_id("x.sql"))
            .count();
        assert_eq!(contained, 3, "two routines and a trigger hang off the file");
    }

    #[test]
    fn output_is_deterministic() {
        let (a, _) = recover_plsql("p.sql", SRC, Some("p.sql"));
        let (b, _) = recover_plsql("p.sql", SRC, Some("p.sql"));
        let ids = |g: &KirGraph| g.objects.iter().map(|o| o.id).collect::<Vec<_>>();
        assert_eq!(ids(&a), ids(&b));
        let props = |g: &KirGraph| {
            g.objects
                .iter()
                .map(|o| Value::Object(o.properties.clone().into_iter().collect()))
                .collect::<Vec<_>>()
        };
        assert_eq!(props(&a), props(&b));
    }

    #[test]
    fn only_oracle_files() {
        assert!(PlSqlAnalyzerPass::applies_to("oracle"));
        assert!(PlSqlAnalyzerPass::applies_to("plsql"));
        assert!(!PlSqlAnalyzerPass::applies_to("postgres"));
        assert!(!PlSqlAnalyzerPass::applies_to("generic"));
    }
}
