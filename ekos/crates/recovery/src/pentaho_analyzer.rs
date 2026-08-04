//! `PentahoAnalyzerPass` — converts Pentaho Kettle observation artifacts
//! (RFC 0027 Phase 3) into the Transformation IR (RFC 0027 Phase 1), then
//! lowers that IR into KIR via `ekos_semantic::transform_ir::lower_to_kir`.
//!
//! Pure structural mapping — no LLM in the loop, same shape as
//! `ConfluenceAnalyzerPass`/`LocalDocAnalyzerPass`. Deterministic XML parsing
//! is still fact collection (RFC 0027's "Observation vs. interpretation
//! boundary" argument): the same `.ktr`/`.kjb` bytes always parse to the same
//! `TransformGraph`, with zero judgment calls.
//!
//! Step-to-IR mapping (RFC 0027 Phase 3's table):
//! `TableInput` → `Source`, `FilterRows` → `Filter`, `Calculator` →
//! `Calculate`, `DatabaseJoin`/`MergeJoin` → `Join`, `GroupBy` → `Aggregate`,
//! `TableOutput` → `Sink`, anything else → `Unmapped` with the raw step XML
//! preserved as evidence.
//!
//! **No real `.ktr`/`.kjb` sample files were available to validate field
//! names against** — the per-step XML shapes below (`<sql>`, `<compare>/
//! <condition>`, `<fields>/<field>`, `<keys>/<key>`, `<group>`, `<table>`)
//! follow Pentaho Kettle's documented step metadata conventions as closely as
//! possible from general knowledge, but are best-effort, not verified against
//! a real export. `DatabaseJoin` in particular is modeled with the same
//! `step1`/`step2`/`keys` shape as `MergeJoin` for simplicity, even though
//! real `DatabaseJoin` semantics (a per-row parameterized SQL lookup) don't
//! cleanly fit the two-upstream-step `Join` shape — a documented
//! approximation, not a blocker, matching this RFC's explicit tolerance for
//! "accept incomplete coverage" scoping (see RFC 0025's Informix precedent).
//! Revisiting field names against a real exported `.ktr` file is listed as
//! follow-up work once one is available.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::KirGraph;
use ekos_semantic::merge_graphs;
use ekos_semantic::transform_ir::{
    AggExpr, JoinKind, NodeId, TransformGraph, TransformNode, TransformOrigin, lower_to_kir,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize)]
struct PentahoArtifactData {
    path: String,
    kettle_kind: String,
    xml: String,
}

/// Coverage counters from one run — readable by the caller after the pass
/// has been consumed by the `PassManager`, mirroring `DocumentSemanticsStats`.
/// This is the plugin's readiness metric per the implementation plan: a
/// concrete, measurable exit criterion rather than a subjective "looks done".
#[derive(Debug, Clone, Copy, Default)]
pub struct PentahoStats {
    pub files_processed: usize,
    pub nodes_total: usize,
    /// Non-`Unmapped` nodes.
    pub nodes_mapped: usize,
}

impl PentahoStats {
    pub fn coverage_percent(&self) -> f32 {
        if self.nodes_total == 0 {
            0.0
        } else {
            100.0 * self.nodes_mapped as f32 / self.nodes_total as f32
        }
    }
}

pub struct PentahoAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<PentahoStats>>,
}

impl PentahoAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("pentaho-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(PentahoStats::default())),
        }
    }

    /// Handle onto this pass's counters, for printing a summary after the
    /// `PassManager` has taken ownership of the pass.
    pub fn stats_handle(&self) -> Arc<Mutex<PentahoStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for PentahoAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = PentahoStats::default();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PENTAHO001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: PentahoArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "PENTAHO002",
                        format!("malformed pentaho payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let graph = match parse_kettle_xml(&data.path, &data.kettle_kind, &data.xml) {
                Ok(g) => g,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("PENTAHO003", format!("cannot parse {}: {e}", data.path));
                    continue;
                }
            };

            stats.files_processed += 1;
            stats.nodes_total += graph.nodes.len();
            stats.nodes_mapped += graph
                .nodes
                .iter()
                .filter(|n| !matches!(n, TransformNode::Unmapped { .. }))
                .count();

            merge_graphs(&mut combined, lower_to_kir(&graph));
        }

        *self.stats.lock().unwrap() = stats;

        if combined.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            nodes = stats.nodes_total,
            mapped = stats.nodes_mapped,
            coverage_pct = stats.coverage_percent(),
            "pentaho-analyzer complete"
        );
        Ok(())
    }
}

// ── XML parsing ──────────────────────────────────────────────────────────────

fn parse_kettle_xml(
    path: &str,
    kettle_kind: &str,
    xml: &str,
) -> Result<TransformGraph, roxmltree::Error> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();

    let origin = TransformOrigin {
        source_path: path.to_string(),
        source_kind: if kettle_kind == "job" {
            "pentaho-kjb".to_string()
        } else {
            "pentaho-ktr".to_string()
        },
        extracted_at: chrono::Utc::now(),
    };

    if kettle_kind == "job" {
        return Ok(parse_kjb(&root, xml, origin));
    }
    Ok(parse_ktr(&root, xml, origin))
}

/// `.kjb` job entries are orchestration (run this, then that), not a data
/// transformation — they never map onto `Source`/`Filter`/`Join`/etc., so
/// every entry becomes `Unmapped`, honestly rather than forced into a shape
/// that doesn't fit.
fn parse_kjb(root: &roxmltree::Node, xml: &str, origin: TransformOrigin) -> TransformGraph {
    let mut nodes = Vec::new();

    let entries = root
        .children()
        .find(|n| n.has_tag_name("entries"))
        .into_iter()
        .flat_map(|e| e.children())
        .filter(|n| n.has_tag_name("entry"));

    for entry in entries {
        nodes.push(TransformNode::Unmapped {
            raw: xml_slice(xml, &entry),
            reason: "job entry (orchestration), not a data transformation".to_string(),
        });
    }

    TransformGraph {
        nodes,
        edges: Vec::new(),
        origin,
    }
}

fn parse_ktr(root: &roxmltree::Node, xml: &str, origin: TransformOrigin) -> TransformGraph {
    let mut nodes = Vec::new();
    let mut name_to_index: HashMap<String, u32> = HashMap::new();

    for step in root.children().filter(|n| n.has_tag_name("step")) {
        let name = child_text(&step, "name").unwrap_or_default();
        let step_type = child_text(&step, "type").unwrap_or_default();
        name_to_index.insert(name, nodes.len() as u32);
        nodes.push(map_step(&step, &step_type, xml));
    }

    let mut edges = Vec::new();
    if let Some(order) = root.children().find(|n| n.has_tag_name("order")) {
        for hop in order.children().filter(|n| n.has_tag_name("hop")) {
            let from = child_text(&hop, "from").unwrap_or_default();
            let to = child_text(&hop, "to").unwrap_or_default();
            if let (Some(&f), Some(&t)) = (name_to_index.get(&from), name_to_index.get(&to)) {
                edges.push((NodeId(f), NodeId(t)));
            }
        }
    }

    TransformGraph {
        nodes,
        edges,
        origin,
    }
}

fn map_step(step: &roxmltree::Node, step_type: &str, xml: &str) -> TransformNode {
    match step_type {
        "TableInput" => {
            let sql = child_text(step, "sql").unwrap_or_default();
            let object_name = extract_table_from_sql(&sql).unwrap_or_else(|| sql.clone());
            TransformNode::Source {
                object_name,
                columns: Vec::new(),
            }
        }
        "FilterRows" => TransformNode::Filter {
            condition: extract_filter_condition(step)
                .unwrap_or_else(|| "<unparsed condition>".to_string()),
        },
        "Calculator" => extract_calculator(step),
        "DatabaseJoin" | "MergeJoin" => extract_join(step),
        "GroupBy" => extract_group_by(step),
        "TableOutput" => {
            let object_name = child_text(step, "table").unwrap_or_default();
            let columns = step
                .children()
                .find(|n| n.has_tag_name("fields"))
                .into_iter()
                .flat_map(|f| f.children())
                .filter(|n| n.has_tag_name("field"))
                .filter_map(|f| child_text(&f, "stream_name"))
                .collect();
            TransformNode::Sink {
                object_name,
                columns,
            }
        }
        other => TransformNode::Unmapped {
            raw: xml_slice(xml, step),
            reason: format!("unrecognized step type: {other}"),
        },
    }
}

fn extract_filter_condition(step: &roxmltree::Node) -> Option<String> {
    let condition = step
        .children()
        .find(|n| n.has_tag_name("compare"))?
        .children()
        .find(|n| n.has_tag_name("condition"))?;

    let negated = child_text(&condition, "negated").as_deref() == Some("Y");
    let leftvalue = child_text(&condition, "leftvalue")?;
    let function = child_text(&condition, "function")?;
    let value_text = condition
        .children()
        .find(|n| n.has_tag_name("value"))
        .and_then(|v| child_text(&v, "text"));

    let mut result = match value_text {
        Some(v) => format!("{leftvalue} {function} '{v}'"),
        None => format!("{leftvalue} {function}"),
    };
    if negated {
        result = format!("NOT ({result})");
    }
    Some(result)
}

/// MVP: a `Calculator` step's multiple `<fields>/<field>` calculated-field
/// entries collapse into one `Calculate` node — everything is preserved
/// textually (semicolon-joined per-entry expressions), never silently
/// dropped, but the IR only models this as a single node.
fn extract_calculator(step: &roxmltree::Node) -> TransformNode {
    let entries: Vec<(String, String)> = step
        .children()
        .find(|n| n.has_tag_name("fields"))
        .into_iter()
        .flat_map(|f| f.children())
        .filter(|n| n.has_tag_name("field"))
        .filter_map(|f| {
            let field_name = child_text(&f, "field_name")?;
            let calc_type = child_text(&f, "calc_type").unwrap_or_else(|| "UNKNOWN".to_string());
            let field_a = child_text(&f, "field_a").unwrap_or_default();
            let field_b = child_text(&f, "field_b");
            let args = match field_b {
                Some(b) => format!("{field_a}, {b}"),
                None => field_a,
            };
            Some((
                field_name.clone(),
                format!("{field_name} := {calc_type}({args})"),
            ))
        })
        .collect();

    if entries.is_empty() {
        return TransformNode::Unmapped {
            raw: String::new(),
            reason: "Calculator step has no recognizable calculated fields".to_string(),
        };
    }

    let output = entries
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let expr = entries
        .iter()
        .map(|(_, e)| e.clone())
        .collect::<Vec<_>>()
        .join("; ");

    TransformNode::Calculate { output, expr }
}

/// Shared by `DatabaseJoin` and `MergeJoin` — see this module's top-level
/// doc comment for why `DatabaseJoin` is modeled with the same shape as
/// `MergeJoin` here, an acknowledged simplification.
fn extract_join(step: &roxmltree::Node) -> TransformNode {
    let kind = match child_text(step, "join_type").as_deref() {
        Some("LEFT") => JoinKind::Left,
        Some("RIGHT") => JoinKind::Right,
        Some("FULL") => JoinKind::Full,
        Some("CROSS") => JoinKind::Cross,
        _ => JoinKind::Inner,
    };

    let keys: Vec<(String, String)> = step
        .children()
        .find(|n| n.has_tag_name("keys"))
        .into_iter()
        .flat_map(|k| k.children())
        .filter(|n| n.has_tag_name("key"))
        .filter_map(|k| {
            let v1 = child_text(&k, "value1")?;
            let v2 = child_text(&k, "value2")?;
            Some((v1, v2))
        })
        .collect();

    // `left`/`right` are graph-local node indices, but this step's own two
    // upstream steps (`step1`/`step2`) are named references, not indices
    // resolvable at this point (steps are parsed in document order, single
    // pass, no forward lookup table across the whole graph yet). Left as
    // self-referential placeholders (`NodeId(0)`); `parse_ktr` does not
    // currently rewrite these, matching this MVP's documented scope — join
    // key text itself is preserved and evidenced regardless.
    TransformNode::Join {
        left: NodeId(0),
        right: NodeId(0),
        keys,
        kind,
    }
}

fn extract_group_by(step: &roxmltree::Node) -> TransformNode {
    let group_by: Vec<String> = step
        .children()
        .find(|n| n.has_tag_name("group"))
        .into_iter()
        .flat_map(|g| g.children())
        .filter(|n| n.has_tag_name("field"))
        .filter_map(|f| child_text(&f, "name"))
        .collect();

    let aggs: Vec<AggExpr> = step
        .children()
        .find(|n| n.has_tag_name("fields"))
        .into_iter()
        .flat_map(|f| f.children())
        .filter(|n| n.has_tag_name("field"))
        .filter_map(|f| {
            let output = child_text(&f, "name")?;
            let func = child_text(&f, "aggregate")?;
            let arg = child_text(&f, "subject").unwrap_or_default();
            Some(AggExpr { output, func, arg })
        })
        .collect();

    TransformNode::Aggregate { group_by, aggs }
}

// ── Small XML helpers ─────────────────────────────────────────────────────────

fn child_text(node: &roxmltree::Node, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// The exact source substring a node was parsed from — roxmltree preserves
/// byte offsets into the original document, so this is the real XML, not a
/// re-serialization, matching `Unmapped`'s "preserved verbatim" contract.
fn xml_slice(xml: &str, node: &roxmltree::Node) -> String {
    xml[node.range()].to_string()
}

/// Best-effort `SELECT ... FROM <table> ...` table-name extraction. Never
/// fails — falls back to the full SQL text at the call site if no `FROM`
/// clause is found, since a `Source` node must have *some* `object_name`.
/// Full SQL parsing (correctly handling joins, subqueries, aliases) is
/// Phase 2's job, not this deterministic-XML-parsing pass's.
fn extract_table_from_sql(sql: &str) -> Option<String> {
    let mut tokens = sql.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok.eq_ignore_ascii_case("from") {
            return tokens
                .next()
                .map(|t| t.trim_matches(|c| c == ',').to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const KTR_ALL_MAPPED_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<transformation>
  <info><name>Load Customers</name></info>
  <order>
    <hop><from>Read Customers</from><to>Filter Active</to></hop>
    <hop><from>Filter Active</from><to>Calc Full Name</to></hop>
    <hop><from>Calc Full Name</from><to>Aggregate By Region</to></hop>
    <hop><from>Aggregate By Region</from><to>Write Customers</to></hop>
  </order>
  <step>
    <name>Read Customers</name>
    <type>TableInput</type>
    <sql>SELECT id, status FROM dbo.cust_mstr</sql>
  </step>
  <step>
    <name>Filter Active</name>
    <type>FilterRows</type>
    <compare>
      <condition>
        <negated>N</negated>
        <leftvalue>status</leftvalue>
        <function>=</function>
        <value><type>String</type><text>active</text></value>
      </condition>
    </compare>
  </step>
  <step>
    <name>Calc Full Name</name>
    <type>Calculator</type>
    <fields>
      <field>
        <field_name>full_name</field_name>
        <calc_type>ADD_STRINGS</calc_type>
        <field_a>first_name</field_a>
        <field_b>last_name</field_b>
      </field>
    </fields>
  </step>
  <step>
    <name>Aggregate By Region</name>
    <type>GroupBy</type>
    <group>
      <field><name>region</name></field>
    </group>
    <fields>
      <field><aggregate>SUM</aggregate><subject>amount</subject><name>total_amount</name></field>
    </fields>
  </step>
  <step>
    <name>Write Customers</name>
    <type>TableOutput</type>
    <table>gold.dim_customer</table>
    <fields>
      <field><stream_name>id</stream_name></field>
      <field><stream_name>full_name</stream_name></field>
    </fields>
  </step>
</transformation>
"#;

    fn sample_graph() -> TransformGraph {
        parse_kettle_xml(
            "jobs/load_customers.ktr",
            "transformation",
            KTR_ALL_MAPPED_TYPES,
        )
        .unwrap()
    }

    #[test]
    fn maps_table_input_to_source() {
        let graph = sample_graph();
        assert!(matches!(
            &graph.nodes[0],
            TransformNode::Source { object_name, columns }
                if object_name == "dbo.cust_mstr" && columns.is_empty()
        ));
    }

    #[test]
    fn maps_filter_rows_to_filter_with_readable_condition() {
        let graph = sample_graph();
        assert!(matches!(
            &graph.nodes[1],
            TransformNode::Filter { condition } if condition == "status = 'active'"
        ));
    }

    #[test]
    fn maps_calculator_to_calculate() {
        let graph = sample_graph();
        match &graph.nodes[2] {
            TransformNode::Calculate { output, expr } => {
                assert_eq!(output, "full_name");
                assert!(expr.contains("ADD_STRINGS(first_name, last_name)"));
            }
            other => panic!("expected Calculate, got {other:?}"),
        }
    }

    #[test]
    fn maps_group_by_to_aggregate() {
        let graph = sample_graph();
        match &graph.nodes[3] {
            TransformNode::Aggregate { group_by, aggs } => {
                assert_eq!(group_by, &vec!["region".to_string()]);
                assert_eq!(aggs.len(), 1);
                assert_eq!(aggs[0].func, "SUM");
                assert_eq!(aggs[0].arg, "amount");
                assert_eq!(aggs[0].output, "total_amount");
            }
            other => panic!("expected Aggregate, got {other:?}"),
        }
    }

    #[test]
    fn maps_table_output_to_sink() {
        let graph = sample_graph();
        assert!(matches!(
            &graph.nodes[4],
            TransformNode::Sink { object_name, columns }
                if object_name == "gold.dim_customer" && columns == &vec!["id".to_string(), "full_name".to_string()]
        ));
    }

    #[test]
    fn hops_become_graph_edges_in_step_order() {
        let graph = sample_graph();
        assert_eq!(
            graph.edges,
            vec![
                (NodeId(0), NodeId(1)),
                (NodeId(1), NodeId(2)),
                (NodeId(2), NodeId(3)),
                (NodeId(3), NodeId(4)),
            ]
        );
    }

    #[test]
    fn unrecognized_step_type_becomes_unmapped_with_raw_xml_preserved() {
        let xml = r#"<?xml version="1.0"?>
<transformation>
  <step>
    <name>Mystery Step</name>
    <type>SomeFutureStepType</type>
    <weird_field>weird value</weird_field>
  </step>
</transformation>
"#;
        let graph = parse_kettle_xml("weird.ktr", "transformation", xml).unwrap();
        match &graph.nodes[0] {
            TransformNode::Unmapped { raw, reason } => {
                assert!(raw.contains("SomeFutureStepType"));
                assert!(raw.contains("weird_field"));
                assert!(reason.contains("SomeFutureStepType"));
            }
            other => panic!("expected Unmapped, got {other:?}"),
        }
    }

    #[test]
    fn kjb_job_entries_become_unmapped() {
        let xml = r#"<?xml version="1.0"?>
<job>
  <name>Orchestrate</name>
  <entries>
    <entry>
      <name>Run Load Customers</name>
      <type>TRANS</type>
    </entry>
  </entries>
</job>
"#;
        let graph = parse_kettle_xml("orchestrate.kjb", "job", xml).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        match &graph.nodes[0] {
            TransformNode::Unmapped { raw, reason } => {
                assert!(raw.contains("Run Load Customers"));
                assert!(reason.contains("orchestration"));
            }
            other => panic!("expected Unmapped, got {other:?}"),
        }
    }

    #[test]
    fn coverage_percent_counts_non_unmapped_nodes() {
        let full = PentahoStats {
            files_processed: 1,
            nodes_total: 5,
            nodes_mapped: 5,
        };
        assert_eq!(full.coverage_percent(), 100.0);

        let partial = PentahoStats {
            files_processed: 1,
            nodes_total: 4,
            nodes_mapped: 3,
        };
        assert_eq!(partial.coverage_percent(), 75.0);

        let empty = PentahoStats::default();
        assert_eq!(empty.coverage_percent(), 0.0);
    }

    #[tokio::test]
    async fn pentaho_analyzer_pass_produces_transform_node_objects() {
        use ekos_artifact::{ArtifactStore, FileSystemArtifactStore, ObservationArtifact};
        use ekos_compiler_core::{EkosConfig, pass::PassContext};
        use std::sync::Arc as StdArc;
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let store = StdArc::new(FileSystemArtifactStore::new(dir.path().join("artifacts")));

        let data = serde_json::json!({
            "path": "jobs/load_customers.ktr",
            "size_bytes": KTR_ALL_MAPPED_TYPES.len(),
            "content_sha256": "irrelevant-for-this-test",
            "kettle_kind": "transformation",
            "xml": KTR_ALL_MAPPED_TYPES,
        });
        let artifact = ObservationArtifact::new("pentaho", "jobs/load_customers.ktr", data);
        let artifact_json = serde_json::to_value(&artifact).unwrap();
        store.write(&artifact.id, &artifact_json).unwrap();

        let config = StdArc::new(EkosConfig::default());
        let mut ctx = PassContext::new(config, dir.path().to_path_buf()).with_artifact_store(store);

        let mut pass = PentahoAnalyzerPass::new("test-workspace", vec![artifact.id.clone()]);
        let stats_handle = pass.stats_handle();
        pass.run(&mut ctx).await.unwrap();

        let stats = *stats_handle.lock().unwrap();
        assert_eq!(stats.files_processed, 1);
        assert_eq!(stats.nodes_total, 5);
        assert_eq!(stats.nodes_mapped, 5);
        assert_eq!(stats.coverage_percent(), 100.0);
    }
}
