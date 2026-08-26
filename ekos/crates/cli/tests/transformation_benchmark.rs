//! Phase 7 end-to-end benchmark (`ekos-transformation-semantics-plan.md`):
//! the target scenario, run for real.
//!
//! Scenario: a legacy Pentaho job (two source tables, a filter, a join, a
//! calculated field, a sink) needs to be reproduced in a new pipeline with
//! one rule changed. Success criterion, verbatim from the plan: *"the
//! developer... can correctly describe the original logic and produce a
//! correct new pipeline draft, using only what EKOS surfaces — no manual
//! XML reading required."*
//!
//! This test plays the role of that developer mechanically: after running
//! the real pipeline once, every assertion below reads **only** MCP tool
//! JSON responses (`ekos_search`, `ekos_transformation_explain`,
//! `ekos_transformation_diff`) — never the fixture files' raw text — the
//! same discipline `mcp_session.rs` already holds itself to for the
//! non-Transformation-IR tools. See `devlog_29.md`'s Phase 7 section for the
//! narrative write-up of what this run found (including the one honest gap
//! it surfaces: SQL table aliases render differently from Pentaho's bare
//! column names in filter-condition text, a known v1 text-level-diff
//! limitation, not something this fixture was tuned to hide).

use std::path::Path;
use tempfile::TempDir;

const LEGACY_KTR: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<transformation>
  <info><name>Legacy Order Summary</name></info>
  <order>
    <hop><from>Read Customers</from><to>Filter Active</to></hop>
    <hop><from>Filter Active</from><to>Join Customer Orders</to></hop>
    <hop><from>Read Orders</from><to>Join Customer Orders</to></hop>
    <hop><from>Join Customer Orders</from><to>Calc Total With Tax</to></hop>
    <hop><from>Calc Total With Tax</from><to>Write Order Summary</to></hop>
  </order>
  <step>
    <name>Read Customers</name>
    <type>TableInput</type>
    <sql>SELECT id, name, status, region FROM dbo.cust_mstr</sql>
  </step>
  <step>
    <name>Read Orders</name>
    <type>TableInput</type>
    <sql>SELECT order_id, customer_id, amount FROM dbo.order_mstr</sql>
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
    <name>Join Customer Orders</name>
    <type>MergeJoin</type>
    <join_type>INNER</join_type>
    <step1>Filter Active</step1>
    <step2>Read Orders</step2>
    <keys>
      <key><value1>id</value1><value2>customer_id</value2></key>
    </keys>
  </step>
  <step>
    <name>Calc Total With Tax</name>
    <type>Calculator</type>
    <fields>
      <field>
        <field_name>total_with_tax</field_name>
        <calc_type>MULTIPLY</calc_type>
        <field_a>amount</field_a>
        <field_b>tax_rate</field_b>
      </field>
    </fields>
  </step>
  <step>
    <name>Write Order Summary</name>
    <type>TableOutput</type>
    <table>gold.order_summary</table>
    <fields>
      <field><stream_name>id</stream_name></field>
      <field><stream_name>total_with_tax</stream_name></field>
    </fields>
  </step>
</transformation>
"#;

/// The "one modified rule": adds `region = 'EU'` to the active-customer
/// filter. Everything else (sources, join, calculation, sink) is the same
/// underlying logic, drafted as SQL instead of Pentaho.
const NEW_PIPELINE_SQL: &str = "CREATE VIEW gold.order_summary AS \
     SELECT id, amount * tax_rate AS total_with_tax \
     FROM dbo.cust_mstr \
     INNER JOIN dbo.order_mstr ON customer_id = id \
     WHERE status = 'active' AND region = 'EU';\n";

fn setup_workspace(dir: &Path) {
    std::fs::write(dir.join("legacy_order_summary.ktr"), LEGACY_KTR).unwrap();
    std::fs::write(dir.join("new_order_summary.sql"), NEW_PIPELINE_SQL).unwrap();

    // api-key-env points at a variable that is never set, so recover always
    // selects the mock LLM provider — no network in tests (mirrors
    // mcp_session.rs). The Transformation IR passes never call an LLM at
    // all (RFC 0027/0028: pure structural), so this only matters for the
    // unrelated DDL SqlAnalyzerPass, which finds nothing here anyway.
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n\
          [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
    )
    .unwrap();
}

fn load_config(dir: &Path) -> ekos_compiler_core::EkosConfig {
    ekos_compiler_core::EkosConfig::from_file(&dir.join("ekos.toml")).unwrap()
}

/// One agent turn — identical helper to `mcp_session.rs`'s, duplicated
/// rather than shared across two independent test binaries (Rust's
/// per-binary `tests/` compilation model gives each file no access to a
/// shared, non-`pub` support module without a `tests/common/mod.rs`
/// restructuring neither file currently needs for anything else).
fn call_tool(
    config: &ekos_compiler_core::EkosConfig,
    dir: &Path,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
    cache: &mut ekos::commands::mcp::StoreCache,
) -> serde_json::Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
    .to_string();

    let response = ekos::commands::mcp::handle_message(config, dir, &request, cache)
        .expect("tools/call is a request and must be answered");
    let response: serde_json::Value = serde_json::from_str(&response).unwrap();

    assert_eq!(
        response["result"]["isError"], false,
        "tool {name} failed: {}",
        response["result"]["content"][0]["text"]
    );
    serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn phase7_benchmark_recover_explain_diff_over_mcp_only() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);
    let config = load_config(dir);

    // ── The pipeline an operator runs before pointing an agent at EKOS ──────
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();
    ekos::commands::recover::run(&config, dir, false)
        .await
        .unwrap();
    ekos::commands::resolve::run(&config, dir, false).unwrap();
    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run(&config, dir, true)
        .await
        .unwrap();

    let mut cache = ekos::commands::mcp::StoreCache::new();

    // ── Step 1: locate each pipeline's Sink purely via ekos_ekl + ekos_state ──
    // ("developer" doesn't know ids or node ordering yet, only that the job
    // writes gold.order_summary) — found by node_type, not by guessing an
    // index, since Pentaho's node order (document order: 2 sources, filter,
    // join, calculate, sink) and SQL's (sql_transform_analyzer.rs's fixed
    // FROM/JOIN → WHERE → aggregate → calculate → Sink order) differ.
    let nodes = call_tool(
        &config,
        dir,
        1,
        "ekos_ekl",
        serde_json::json!({ "query": "FIND Object WHERE kind CONTAINS 'TransformNode'" }),
        &mut cache,
    );
    let rows = nodes["rows"].as_array().unwrap();
    assert_eq!(
        rows.len(),
        12,
        "6 legacy Pentaho nodes (2 sources, filter, join, calculate, sink) + \
         6 new SQL nodes (same shape) expected, got {rows:?}"
    );

    let find_sink_id = |source_path_prefix: &str,
                        cache: &mut ekos::commands::mcp::StoreCache|
     -> String {
        rows.iter()
            .filter(|r| r["name"].as_str().unwrap().starts_with(source_path_prefix))
            .map(|r| r["id"].as_str().unwrap().to_string())
            .find(|id| {
                let state = call_tool(
                    &config,
                    dir,
                    100,
                    "ekos_state",
                    serde_json::json!({ "id": id }),
                    cache,
                );
                state["object"]["properties"]["node_type"] == "Sink"
            })
            .unwrap_or_else(|| panic!("no Sink node found with name prefix {source_path_prefix}"))
    };

    let legacy_sink_id = find_sink_id("legacy_order_summary.ktr:", &mut cache);
    let new_sink_id = find_sink_id("new_order_summary.sql", &mut cache);

    // ── Step 2: explain the legacy pipeline — no XML read, MCP tool only ────
    let explain = call_tool(
        &config,
        dir,
        2,
        "ekos_transformation_explain",
        serde_json::json!({ "id": legacy_sink_id }),
        &mut cache,
    );
    let steps = explain["steps"].as_array().unwrap();
    assert_eq!(
        steps.len(),
        6,
        "all 6 legacy nodes (2 Source, Filter, Join, Calculate, Sink) are upstream \
         of the Sink in this fully-connected pipeline — got {steps:?}"
    );

    let node_types: Vec<&str> = steps
        .iter()
        .map(|s| s["node_type"].as_str().unwrap())
        .collect();
    for expected in ["Source", "Filter", "Join", "Calculate", "Sink"] {
        assert!(
            node_types.contains(&expected),
            "explanation must cover every real node type, missing {expected}, got {node_types:?}"
        );
    }

    // Every step must carry evidence — the plan's explicit requirement
    // ("each claim linked to its Evidence").
    for step in steps {
        assert!(
            !step["evidence"].as_array().unwrap().is_empty(),
            "step {step} has no evidence — every claim must be traceable"
        );
    }

    // The filter's evidence must cite the real condition text, not a paraphrase.
    let filter_step = steps
        .iter()
        .find(|s| s["node_type"] == "Filter")
        .expect("Filter step present");
    assert!(
        filter_step["summary"]
            .as_str()
            .unwrap()
            .contains("status = 'active'"),
        "filter summary must quote the real evidenced condition, got {filter_step}"
    );

    // Zero Unmapped nodes — this benchmark's coverage should be 100%,
    // proving Phase 3's step-mapping table handles every step type used
    // here (TableInput, FilterRows, MergeJoin, Calculator, TableOutput).
    assert!(
        !node_types.contains(&"Unmapped"),
        "this fixture uses only mapped step types; any Unmapped here is a real coverage gap"
    );

    // ── Step 3: diff old vs. new — verify the migration preserved logic ─────
    let diff = call_tool(
        &config,
        dir,
        3,
        "ekos_transformation_diff",
        serde_json::json!({ "old_id": legacy_sink_id, "new_id": new_sink_id }),
        &mut cache,
    );

    // Sources and sink are unchanged — same table names on both sides.
    assert_eq!(diff["diff"]["sources"]["added"], serde_json::json!([]));
    assert_eq!(diff["diff"]["sources"]["removed"], serde_json::json!([]));
    assert_eq!(diff["diff"]["sinks"]["added"], serde_json::json!([]));
    assert_eq!(diff["diff"]["sinks"]["removed"], serde_json::json!([]));

    // The filter changed — this is the one modified rule the plan's
    // scenario asks for, and it must be visible in the diff.
    let filters = &diff["diff"]["filters"];
    assert!(
        !filters["removed"].as_array().unwrap().is_empty(),
        "the old, narrower filter condition must show as removed"
    );
    assert!(
        !filters["added"].as_array().unwrap().is_empty(),
        "the new, EU-restricted filter condition must show as added"
    );
    let added_filter = filters["added"][0].as_str().unwrap();
    assert!(
        added_filter.contains("region") && added_filter.contains("EU"),
        "the added filter text must show the actual new rule (region = 'EU'), got {added_filter}"
    );

    // No spurious unmapped-coverage regression from old to new.
    assert_eq!(diff["diff"]["unmapped"]["old_count"], 0);
    assert_eq!(diff["diff"]["unmapped"]["new_count"], 0);
}
