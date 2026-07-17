//! MCP (Model Context Protocol) server over stdio — RFC 0013.
//!
//! Speaks newline-delimited JSON-RPC 2.0 on stdin/stdout and exposes the
//! read-only Runtime as MCP tools (`ekos_search`, `ekos_ekl`,
//! `ekos_neighborhood`, `ekos_state`, `ekos_status`). Stdout carries protocol
//! frames only; logging must go to stderr (see `init_logging_stderr`).
//!
//! The ledger is opened per `tools/call`, so the server starts before a first
//! `ekos build` and returns a readable tool error until a ledger exists.

use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ekl::{EklInterpreter, ekl_parse};
use ekos_kir::KirId;
use ekos_ledger::Ledger;
use ekos_runtime::Runtime;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;
use std::str::FromStr;

/// Blocking serve loop: one JSON-RPC message per line on stdin, one response
/// per line on stdout. Exits cleanly on EOF (client disconnect).
pub fn run(config: &EkosConfig, workspace: &Path) -> Result<()> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_message(config, workspace, &line) {
            let mut out = stdout.lock();
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Dispatch one raw JSON-RPC line. Returns `None` for notifications (which
/// must never be answered), `Some(response-line)` for requests.
pub fn handle_message(config: &EkosConfig, workspace: &Path, line: &str) -> Option<String> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error_response(
                Value::Null,
                -32700,
                &format!("parse error: {e}"),
            ));
        }
    };

    // Requests carry an `id`; notifications don't and are never answered.
    let id = msg.get("id").cloned()?;

    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let response = match method {
        "initialize" => ok_response(id, initialize_result(&params)),
        "ping" => ok_response(id, json!({})),
        "tools/list" => ok_response(id, json!({ "tools": tool_definitions() })),
        "tools/call" => ok_response(id, tools_call(config, workspace, &params)),
        other => error_response(id, -32601, &format!("method not found: {other}")),
    };
    Some(response)
}

fn ok_response(id: Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: Value, code: i64, message: &str) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }).to_string()
}

fn initialize_result(params: &Value) -> Value {
    // Echo the client's protocol version; the stdio message shapes we rely on
    // are stable across published revisions.
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2025-06-18");
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "ekos", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "ekos_search",
            "description": "Full-text search over compiled knowledge objects — names, kinds, and content excerpts — ranked by relevance (name matches first). Use 2-3 keywords, not natural-language questions. Returns matching object ids and names; feed an id to ekos_state or ekos_neighborhood for detail.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search text; a trailing * enables prefix search (e.g. 'order*')" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "ekos_ekl",
            "description": "Run an Enterprise Knowledge Language query against the ledger, e.g. FIND Object WHERE kind = 'Table' AND name CONTAINS 'order' ORDER BY name LIMIT 10. Entities: Object, Relationship. Results carry evidence.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "EKL query text" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "ekos_neighborhood",
            "description": "BFS graph traversal from an object: everything connected within `depth` hops, as objects + relationships.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Object id (UUID) from ekos_search or ekos_ekl" },
                    "depth": { "type": "integer", "description": "Hops to traverse (default 1)" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "ekos_state",
            "description": "Reconstruct the full state of one object: the object, its relationships, and the evidence behind each conclusion. Pass `at` (RFC 3339 timestamp) to reconstruct historical state.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Object id (UUID)" },
                    "at": { "type": "string", "description": "Optional RFC 3339 timestamp for historical state" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "ekos_dependents",
            "description": "Impact analysis: objects with a relationship pointing AT the given object (incoming edges) — 'what depends on this / what breaks if it changes'. Outgoing edges (what the object itself depends on) are listed separately.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Object id (UUID) from ekos_search or ekos_ekl" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "ekos_diff",
            "description": "What knowledge changed in the ledger in a time window: objects/relationships written in (from, to], resolved to names and kinds. Use to answer 'what changed since yesterday?'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Window start, RFC 3339 (exclusive)" },
                    "to": { "type": "string", "description": "Window end, RFC 3339 (inclusive; default: now)" }
                },
                "required": ["from"]
            }
        },
        {
            "name": "ekos_status",
            "description": "Ledger health: total entries, object count, relationship count, and the ledger path being served.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

/// Execute a tools/call request. Tool failures (bad query, unknown id,
/// missing ledger) are reported as `isError: true` results — readable by the
/// agent — never as protocol errors.
fn tools_call(config: &EkosConfig, workspace: &Path, params: &Value) -> Value {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match call_tool(config, workspace, name, &arguments) {
        Ok(result) => {
            let text = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|e| format!("serialization error: {e}"));
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        Err(e) => {
            json!({ "content": [{ "type": "text", "text": e.to_string() }], "isError": true })
        }
    }
}

fn call_tool(config: &EkosConfig, workspace: &Path, name: &str, args: &Value) -> Result<Value> {
    let ledger_path = config.ledger_path(workspace);
    let ledger = Ledger::open(&ledger_path).map_err(|e| {
        anyhow::anyhow!(
            "cannot open ledger at {}: {e}\nRun `ekos build` in the workspace first.",
            ledger_path.display()
        )
    })?;
    let runtime = Runtime::new(&ledger);

    match name {
        "ekos_search" => {
            let query = required_str(args, "query")?;
            let matches = runtime.find_objects(query)?;
            Ok(json!({
                "matches": matches
                    .iter()
                    .map(|(id, name)| json!({ "id": id.to_string(), "name": name }))
                    .collect::<Vec<_>>()
            }))
        }
        "ekos_ekl" => {
            let query = required_str(args, "query")?;
            let ast = ekl_parse(query).map_err(|e| {
                anyhow::anyhow!("EKL parse error at column {}: {}", e.position, e.message)
            })?;
            let interpreter = EklInterpreter::new(&runtime);
            let result = interpreter
                .execute(&ast)
                .map_err(|e| anyhow::anyhow!("EKL error: {e}"))?;
            Ok(json!({ "count": result.rows.len(), "rows": result.rows }))
        }
        "ekos_neighborhood" => {
            let id = required_id(args)?;
            let depth = args.get("depth").and_then(Value::as_u64).unwrap_or(1) as u32;
            let graph = runtime.load_neighborhood(&id, depth)?;
            Ok(serde_json::to_value(&graph)?)
        }
        "ekos_state" => {
            let id = required_id(args)?;
            let state = match args.get("at").and_then(Value::as_str) {
                Some(at) => {
                    let at: DateTime<Utc> = at.parse().map_err(|e| {
                        anyhow::anyhow!("invalid `at` timestamp (want RFC 3339): {e}")
                    })?;
                    runtime.reconstruct_state_at(&id, at)?
                }
                None => runtime.reconstruct_state(&id)?,
            };
            state
                .map(|s| serde_json::to_value(&s))
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("object not found: {}", id))
        }
        "ekos_dependents" => {
            let id = required_id(args)?;
            let target = runtime
                .load_object(&id)?
                .ok_or_else(|| anyhow::anyhow!("object not found: {id}"))?;

            let mut dependents = Vec::new();
            let mut dependencies = Vec::new();
            for rel in runtime.relationships_for(&id)? {
                let (other_id, bucket) = if rel.to == id {
                    (rel.from, &mut dependents)
                } else {
                    (rel.to, &mut dependencies)
                };
                let other = runtime.load_object(&other_id)?;
                bucket.push(json!({
                    "id": other_id.to_string(),
                    "name": other.as_ref().map(|o| o.name.clone()),
                    "kind": other.as_ref().map(|o| o.kind.to_string()),
                    "relationship": rel.kind.to_string(),
                    "properties": rel.properties,
                }));
            }

            Ok(json!({
                "target": { "id": id.to_string(), "name": target.name, "kind": target.kind.to_string() },
                "dependents": dependents,
                "dependents_count": dependents.len(),
                "dependencies": dependencies,
                "dependencies_count": dependencies.len(),
            }))
        }
        "ekos_diff" => {
            let from: DateTime<Utc> = required_str(args, "from")?
                .parse()
                .map_err(|e| anyhow::anyhow!("invalid `from` timestamp (want RFC 3339): {e}"))?;
            let to: DateTime<Utc> = match args.get("to").and_then(Value::as_str) {
                Some(raw) => raw
                    .parse()
                    .map_err(|e| anyhow::anyhow!("invalid `to` timestamp (want RFC 3339): {e}"))?,
                None => Utc::now(),
            };

            let diff = ekos_ledger::diff_ledger(&ledger, from, to)?;

            // Resolve touched logical ids to something an agent can read;
            // cap the listing so a full-rebuild window stays consumable.
            const MAX_LISTED: usize = 200;
            let mut changed = Vec::new();
            for raw_id in diff.touched.iter().take(MAX_LISTED) {
                let Ok(id) = KirId::from_str(raw_id) else {
                    continue;
                };
                if let Some(obj) = runtime.load_object(&id)? {
                    changed.push(json!({
                        "entity": "Object", "id": raw_id, "name": obj.name, "kind": obj.kind.to_string()
                    }));
                } else if let Some(rel) = ledger.get_relationship(&id)? {
                    changed.push(json!({
                        "entity": "Relationship", "id": raw_id, "kind": rel.kind.to_string(),
                        "from": rel.from.to_string(), "to": rel.to.to_string()
                    }));
                } else {
                    changed.push(json!({ "entity": "Unknown", "id": raw_id }));
                }
            }

            Ok(json!({
                "from": from.to_rfc3339(),
                "to": to.to_rfc3339(),
                "changed_total": diff.touched.len(),
                "changed": changed,
                "changed_listed": changed.len(),
                "unchanged": diff.unchanged,
            }))
        }
        "ekos_status" => Ok(json!({
            "entries": ledger.entry_count()?,
            "objects": ledger.object_count()?,
            "relationships": ledger.relationship_count()?,
            "ledger_path": ledger_path.display().to_string(),
        })),
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing required string argument `{key}`"))
}

fn required_id(args: &Value) -> Result<KirId> {
    let raw = required_str(args, "id")?;
    KirId::from_str(raw).map_err(|_| anyhow::anyhow!("invalid object id: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(id: u64, method: &str, params: Value) -> String {
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
    }

    fn parse(response: &str) -> Value {
        serde_json::from_str(response).expect("response is valid JSON")
    }

    #[test]
    fn initialize_echoes_protocol_version_and_names_server() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(1, "initialize", json!({ "protocolVersion": "2025-03-26" }));

        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(resp["result"]["serverInfo"]["name"], "ekos");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn notifications_are_never_answered() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string();
        assert!(handle_message(&config, tmp.path(), &line).is_none());
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let resp = parse(
            &handle_message(&config, tmp.path(), &req(2, "resources/list", json!({}))).unwrap(),
        );
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn tools_list_exposes_the_runtime_tools() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let resp =
            parse(&handle_message(&config, tmp.path(), &req(3, "tools/list", json!({}))).unwrap());
        let tools = resp["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "ekos_search",
                "ekos_ekl",
                "ekos_neighborhood",
                "ekos_state",
                "ekos_dependents",
                "ekos_diff",
                "ekos_status"
            ]
        );
        for tool in tools {
            assert!(
                tool["inputSchema"]["type"] == "object",
                "every tool declares an object schema"
            );
        }
    }

    #[test]
    fn dependents_of_unknown_object_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            10,
            "tools/call",
            json!({ "name": "ekos_dependents",
                    "arguments": { "id": "00000000-0000-0000-0000-000000000000" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
        assert!(
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("not found")
        );
    }

    #[test]
    fn diff_on_fresh_workspace_reports_nothing_changed() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            11,
            "tools/call",
            json!({ "name": "ekos_diff", "arguments": { "from": "2020-01-01T00:00:00Z" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["changed_total"], 0);
        assert_eq!(body["unchanged"], 0);
    }

    #[test]
    fn diff_with_bad_timestamp_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            12,
            "tools/call",
            json!({ "name": "ekos_diff", "arguments": { "from": "yesterday-ish" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
        assert!(
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("RFC 3339")
        );
    }

    #[test]
    fn status_works_on_a_fresh_workspace() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            4,
            "tools/call",
            json!({ "name": "ekos_status", "arguments": {} }),
        );

        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["objects"], 0);
        assert_eq!(body["entries"], 0);
    }

    #[test]
    fn search_returns_empty_matches_on_a_fresh_workspace() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            5,
            "tools/call",
            json!({ "name": "ekos_search", "arguments": { "query": "anything" } }),
        );

        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["matches"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn ekl_syntax_error_is_a_tool_error_not_a_protocol_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            6,
            "tools/call",
            json!({ "name": "ekos_ekl", "arguments": { "query": "FIND Widget" } }),
        );

        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert!(
            resp.get("error").is_none(),
            "tool failures must not be JSON-RPC errors"
        );
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn unknown_tool_is_reported_as_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            7,
            "tools/call",
            json!({ "name": "ekos_write", "arguments": {} }),
        );

        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn malformed_json_returns_parse_error_with_null_id() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let resp = parse(&handle_message(&config, tmp.path(), "{not json").unwrap());
        assert_eq!(resp["error"]["code"], -32700);
        assert!(resp["id"].is_null());
    }
}
