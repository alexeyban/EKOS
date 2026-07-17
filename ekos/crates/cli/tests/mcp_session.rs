//! Agent-session integration test: how Claude Code actually uses EKOS.
//!
//! Drives the full pipeline (init → build → recover → compile → commit) over a
//! small fixture project, then scripts the MCP session a coding agent would
//! run — each tool call consuming the previous call's output:
//!
//!   initialize → ekos_status → ekos_ekl (find tables) → take an id →
//!   ekos_neighborhood (what's connected?) → ekos_state (show me evidence) →
//!   ekos_search (free-text entry point)
//!
//! This is the contract the Claude Code registration relies on; if any link in
//! the chain breaks (tool shape, id round-tripping, evidence attachment), this
//! test fails before a user does.

use std::path::Path;
use tempfile::TempDir;

fn setup_workspace(dir: &Path) {
    std::fs::create_dir_all(dir.join("shop/src")).unwrap();
    std::fs::write(dir.join("shop/src/main.rs"), b"fn main() {}").unwrap();
    std::fs::write(dir.join("shop/README.md"), b"# Shop Service").unwrap();
    std::fs::write(
        dir.join("shop/schema.sql"),
        b"CREATE TABLE customers (id INT PRIMARY KEY, name TEXT);\n\
          CREATE TABLE orders (\n\
              id INT PRIMARY KEY,\n\
              customer_id INT REFERENCES customers(id),\n\
              total DECIMAL\n\
          );\n",
    )
    .unwrap();

    // api-key-env points at a variable that is never set, so recover always
    // selects the mock LLM provider — no network in tests.
    std::fs::write(
        dir.join("ekos.toml"),
        b"[workspace]\nroot = \".\"\n\n\
          [observe]\npaths = [\"shop\"]\nignore-patterns = [\".ekos\"]\n\n\
          [llm]\napi-key-env = \"EKOS_TEST_KEY_THAT_DOES_NOT_EXIST\"\n",
    )
    .unwrap();
}

fn load_config(dir: &Path) -> ekos_compiler_core::EkosConfig {
    ekos_compiler_core::EkosConfig::from_file(&dir.join("ekos.toml")).unwrap()
}

/// One agent turn: send a tools/call, decode the JSON body out of the MCP
/// content envelope the way an MCP client does.
fn call_tool(
    config: &ekos_compiler_core::EkosConfig,
    dir: &Path,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    })
    .to_string();

    let response = ekos::commands::mcp::handle_message(config, dir, &request)
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
async fn claude_code_session_over_mcp() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    setup_workspace(dir);
    let config = load_config(dir);

    // ── The pipeline an operator runs before pointing an agent at EKOS ──────
    ekos::commands::init::run(&config, dir).unwrap();
    ekos::commands::build::run(&config, dir).await.unwrap();
    ekos::commands::recover::run(&config, dir, false).await.unwrap();
    ekos::commands::compile::run(&config, dir).await.unwrap();
    ekos::commands::commit::run(&config, dir).unwrap();

    // ── Turn 0: MCP handshake (what `claude mcp list` verifies) ─────────────
    let init = ekos::commands::mcp::handle_message(
        &config,
        dir,
        &serde_json::json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18" }
        })
        .to_string(),
    )
    .unwrap();
    let init: serde_json::Value = serde_json::from_str(&init).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "ekos");

    // ── Turn 1: "is there anything here?" ───────────────────────────────────
    let status = call_tool(&config, dir, 1, "ekos_status", serde_json::json!({}));
    assert!(status["objects"].as_u64().unwrap() >= 5, "files + tables expected");

    // ── Turn 2: "what tables does this workspace have?" ─────────────────────
    let tables = call_tool(
        &config,
        dir,
        2,
        "ekos_ekl",
        serde_json::json!({ "query": "FIND Object WHERE kind = 'Table' ORDER BY name" }),
    );
    let names: Vec<&str> = tables["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["customers", "orders"], "SQL recovery must find both tables");

    // ── Turn 3: agent picks `orders` from turn 2 and asks what's connected ──
    let orders_id = tables["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "orders")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let neighborhood = call_tool(
        &config,
        dir,
        3,
        "ekos_neighborhood",
        serde_json::json!({ "id": orders_id, "depth": 1 }),
    );
    let neighbor_names: Vec<&str> = neighborhood["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["name"].as_str().unwrap())
        .collect();
    assert!(
        neighbor_names.contains(&"customers"),
        "orders → customers foreign key must surface in the neighborhood, got {neighbor_names:?}"
    );

    // ── Turn 4: "prove it" — state reconstruction carries evidence ──────────
    let state = call_tool(
        &config,
        dir,
        4,
        "ekos_state",
        serde_json::json!({ "id": orders_id }),
    );
    assert_eq!(state["object"]["name"], "orders");
    let evidence = state["evidence"].as_array().unwrap();
    assert!(!evidence.is_empty(), "every conclusion must be traceable to evidence");
    let fragments = evidence
        .iter()
        .map(|e| e["fragment"].as_str().unwrap_or_default().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        fragments.contains("orders"),
        "evidence must point back at the SQL that defined the table"
    );

    // ── Turn 5: free-text entry point works for the same concept ────────────
    let search = call_tool(
        &config,
        dir,
        5,
        "ekos_search",
        serde_json::json!({ "query": "orders" }),
    );
    assert!(
        !search["matches"].as_array().unwrap().is_empty(),
        "full-text search must find the orders concept"
    );

    // ── Turn 6: impact analysis — "what breaks if customers changes?" ────────
    let customers_id = tables["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "customers")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let impact = call_tool(
        &config,
        dir,
        6,
        "ekos_dependents",
        serde_json::json!({ "id": customers_id }),
    );
    assert_eq!(impact["target"]["name"], "customers");
    let dependent_names: Vec<&str> = impact["dependents"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["name"].as_str())
        .collect();
    assert!(
        dependent_names.contains(&"orders"),
        "orders holds an FK into customers, so it must appear as a dependent, got {dependent_names:?}"
    );

    // ── Turn 7: "what changed?" — the whole build lands inside the window ────
    let diff = call_tool(
        &config,
        dir,
        7,
        "ekos_diff",
        serde_json::json!({ "from": "2020-01-01T00:00:00Z" }),
    );
    assert!(
        diff["changed_total"].as_u64().unwrap() >= 2,
        "the freshly committed tables must appear as changes"
    );
    let changed_names: Vec<&str> = diff["changed"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    assert!(
        changed_names.contains(&"orders") && changed_names.contains(&"customers"),
        "diff must resolve changed ids to readable names, got {changed_names:?}"
    );

    // ── Turn 8: content search (RFC 0014) — the README body, not its name ────
    let content_hit = call_tool(
        &config,
        dir,
        8,
        "ekos_search",
        serde_json::json!({ "query": "Shop Service" }),
    );
    let hit_names: Vec<&str> = content_hit["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    assert!(
        hit_names.contains(&"README.md"),
        "the phrase lives in README.md's body, not its filename — content \
         indexing must surface it, got {hit_names:?}"
    );
}
