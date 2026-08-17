//! MCP (Model Context Protocol) server over stdio — RFC 0013.
//!
//! Speaks newline-delimited JSON-RPC 2.0 on stdin/stdout and exposes the
//! read-only Runtime as MCP tools (`ekos_search`, `ekos_ekl`,
//! `ekos_neighborhood`, `ekos_state`, `ekos_dependents`, `ekos_impact`,
//! `ekos_diff`, `ekos_status`, `ekos_transformation_explain`,
//! `ekos_transformation_diff` — RFC 0028). Stdout carries protocol frames
//! only; logging must go to stderr (see `init_logging_stderr`).
//!
//! The ledger is opened per `tools/call`, so the server starts before a first
//! `ekos build` and returns a readable tool error until a ledger exists.

use super::store::open_store;
use anyhow::Result;
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ekl::{EklInterpreter, ekl_parse};
use ekos_kir::{EventKind, KirEvent, KirId, RelationshipKind};
use ekos_runtime::{ImpactDirection, Runtime};
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
        "tools/list" => ok_response(id, json!({ "tools": tool_definitions(config) })),
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

/// RFC 0056: `ekos_clickhouse_query` is the one MCP tool that touches a live external system
/// (every other tool reads only the local ledger). Off by default — only listed when
/// `[clickhouse].enable-mcp-query = true` is set in `ekos.toml`, so a connected AI agent never
/// gets live query access unless a human operator explicitly opts the workspace in.
fn tool_definitions(config: &EkosConfig) -> Vec<Value> {
    let mut tools = base_tool_definitions();
    if config.clickhouse.enable_mcp_query {
        tools.push(clickhouse_query_tool_definition());
    }
    tools
}

fn clickhouse_query_tool_definition() -> Value {
    json!({
        "name": "ekos_clickhouse_query",
        "description": "Live NL-to-SQL query engine over the compiled ClickHouse schema (RFC 0056). Builds a SELECT-only SQL query from the question and the compiled schema, validates it (rejects anything but a single SELECT), runs it live against ClickHouse, and returns the resulting dataset. Unlike every other tool here, this reads a live external system, not just the local ledger — every call is recorded as an Evidence/Event pair in the ledger for audit.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "question": { "type": "string", "description": "Natural-language question to answer with a live ClickHouse query" }
            },
            "required": ["question"]
        }
    })
}

fn base_tool_definitions() -> Vec<Value> {
    let tools = json!([
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
            "name": "ekos_impact",
            "description": "Transitive impact analysis: follows dependency edges multiple hops (default 5), directionally and optionally filtered to specific relationship kinds — 'what breaks N levels deep if I change this', not just direct edges. Use ekos_dependents for single-hop; use this for multi-hop.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Object id (UUID) from ekos_search or ekos_ekl" },
                    "direction": { "type": "string", "description": "\"dependents\" (default; what depends on this) or \"dependencies\" (what this depends on)" },
                    "kinds": { "type": "array", "items": { "type": "string" }, "description": "Relationship kind names to follow, e.g. [\"ForeignKey\", \"DependsOn\"] (default: all kinds)" },
                    "max_hops": { "type": "integer", "description": "Hop bound (default 5)" }
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
        },
        {
            "name": "ekos_transformation_explain",
            "description": "Explains a Transformation IR pipeline (Pentaho job or SQL SELECT/VIEW/procedure) by walking the chain of Source/Filter/Join/Aggregate/Calculate/Sink/Unmapped nodes feeding into the given object, with each step's evidence (source file/fragment).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Transformation IR object id (a TransformNode, typically a Sink), from ekos_search or ekos_ekl" },
                    "max_hops": { "type": "integer", "description": "Hop bound walking upstream (default 50)" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "ekos_transformation_diff",
            "description": "Compares two Transformation IR pipelines (e.g. an old Pentaho-derived one and a newly drafted one) and reports added/removed sources, filters, joins, aggregations, and calculations — use to verify a migration preserves intended logic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "old_id": { "type": "string", "description": "Transformation IR object id of the original pipeline's end node" },
                    "new_id": { "type": "string", "description": "Transformation IR object id of the new pipeline's end node" },
                    "max_hops": { "type": "integer", "description": "Hop bound walking upstream on each side (default 50)" }
                },
                "required": ["old_id", "new_id"]
            }
        },
        {
            "name": "ekos_identity_review",
            "description": "Confirm or reject a candidate cross-system identity match (RFC 0029) — e.g. Informix cust_mstr vs. Postgres customers, proposed by `ekos identity scan`. Confirming or rejecting writes a new Event to the ledger. The only write-capable MCP tool; only Custom(\"SameAs\") relationships are reviewable this way.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "relationship_id": { "type": "string", "description": "Id of the unconfirmed SameAs relationship, from ekos_ekl (FIND Relationship WHERE kind CONTAINS 'SameAs')" },
                    "decision": { "type": "string", "description": "\"confirmed\" or \"rejected\"" }
                },
                "required": ["relationship_id", "decision"]
            }
        }
    ]);
    tools.as_array().cloned().unwrap_or_default()
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
    let ledger = open_store(config, workspace)
        .map_err(|e| anyhow::anyhow!("{e}\nRun `ekos build` in the workspace first."))?;
    let runtime = Runtime::over(&*ledger);

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
                // Same rationale as `ekos_impact`/`ekos_neighborhood`: an
                // unreviewed RFC 0029 candidate is a hypothesis, not a fact.
                if rel.is_pending_review() {
                    continue;
                }
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
        "ekos_impact" => {
            let id = required_id(args)?;
            runtime
                .load_object(&id)?
                .ok_or_else(|| anyhow::anyhow!("object not found: {id}"))?;

            let direction = match args.get("direction").and_then(Value::as_str) {
                Some("dependencies") => ImpactDirection::Dependencies,
                Some("dependents") | None => ImpactDirection::Dependents,
                Some(other) => {
                    anyhow::bail!(
                        "invalid `direction`: {other} (want \"dependents\" or \"dependencies\")"
                    )
                }
            };
            let kinds: Vec<RelationshipKind> = args
                .get("kinds")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(|s| RelationshipKind::from_str(s).expect("infallible"))
                        .collect()
                })
                .unwrap_or_default();
            let max_hops = args.get("max_hops").and_then(Value::as_u64).unwrap_or(5) as u32;

            let hops = runtime.trace_impact(&id, direction, &kinds, max_hops)?;
            let by_hop: Vec<Value> = hops
                .iter()
                .map(|h| {
                    json!({
                        "hop": h.hop,
                        "id": h.object.id.to_string(),
                        "name": h.object.name,
                        "kind": h.object.kind.to_string(),
                        "via": h.via.kind.to_string(),
                    })
                })
                .collect();

            Ok(json!({
                "target": { "id": id.to_string() },
                "direction": match direction { ImpactDirection::Dependents => "dependents", ImpactDirection::Dependencies => "dependencies" },
                "max_hops": max_hops,
                "count": by_hop.len(),
                "hops": by_hop,
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

            let diff = ledger.diff(from, to)?;

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
            "ledger_path": super::store::store_display(config, workspace),
        })),
        "ekos_transformation_explain" => {
            let id = required_id(args)?;
            let max_hops = args.get("max_hops").and_then(Value::as_u64).unwrap_or(50) as u32;
            let chain = transformation_chain(&runtime, &id, max_hops)?;

            let steps: Vec<Value> = chain
                .iter()
                .map(|obj| explain_node(&*ledger, obj))
                .collect::<Result<_>>()?;

            Ok(json!({
                "target": { "id": id.to_string() },
                "steps": steps,
                "step_count": steps.len(),
            }))
        }
        "ekos_transformation_diff" => {
            let old_id = KirId::from_str(required_str(args, "old_id")?)
                .map_err(|_| anyhow::anyhow!("invalid `old_id`"))?;
            let new_id = KirId::from_str(required_str(args, "new_id")?)
                .map_err(|_| anyhow::anyhow!("invalid `new_id`"))?;
            let max_hops = args.get("max_hops").and_then(Value::as_u64).unwrap_or(50) as u32;

            let old_chain = transformation_chain(&runtime, &old_id, max_hops)?;
            let new_chain = transformation_chain(&runtime, &new_id, max_hops)?;

            Ok(json!({
                "old": { "id": old_id.to_string(), "step_count": old_chain.len() },
                "new": { "id": new_id.to_string(), "step_count": new_chain.len() },
                "diff": diff_chains(&old_chain, &new_chain),
            }))
        }
        "ekos_identity_review" => {
            let rel_id = KirId::from_str(required_str(args, "relationship_id")?)
                .map_err(|_| anyhow::anyhow!("invalid `relationship_id`"))?;
            let decision = required_str(args, "decision")?;
            if decision != "confirmed" && decision != "rejected" {
                anyhow::bail!(
                    "invalid `decision`: {decision} (want \"confirmed\" or \"rejected\")"
                );
            }

            let mut rel = ledger
                .get_relationship(&rel_id)?
                .ok_or_else(|| anyhow::anyhow!("relationship not found: {rel_id}"))?;
            if !matches!(&rel.kind, RelationshipKind::Custom(k) if k == "SameAs") {
                anyhow::bail!(
                    "not a SameAs candidate, cannot be reviewed through this tool: {rel_id}"
                );
            }

            rel.properties.insert("status".into(), json!(decision));
            rel.properties
                .insert("reviewed_at".into(), json!(Utc::now().to_rfc3339()));
            ledger.append_relationship(&rel)?;

            let event_kind = if decision == "confirmed" {
                EventKind::Merged
            } else {
                EventKind::Modified
            };
            let event = KirEvent {
                id: KirId::new(),
                kind: event_kind,
                subject: rel_id,
                payload: json!({ "decision": decision, "relationship_id": rel_id.to_string() }),
                evidence: Vec::new(),
                occurred_at: Utc::now(),
            };
            ledger.append_event(&event)?;

            Ok(json!({
                "relationship_id": rel_id.to_string(),
                "decision": decision,
                "status": "recorded",
            }))
        }
        "ekos_clickhouse_query" => {
            // Defense-in-depth: re-check the gate even though an ungated server never lists
            // this tool in `tools/list` — a client could still call it by name directly.
            if !config.clickhouse.enable_mcp_query {
                anyhow::bail!(
                    "ekos_clickhouse_query is disabled — set [clickhouse].enable-mcp-query = true in ekos.toml to enable it"
                );
            }
            let question = required_str(args, "question")?;
            run_clickhouse_query_blocking(config, workspace, question)
        }
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    }
}

/// Bridges `call_tool`'s synchronous context (the stdio serve loop is a blocking `for line in
/// stdin.lock().lines()`, invoked directly inside `main`'s `#[tokio::main]` runtime, never
/// spawned onto its own task) into `ekos_clickhouse_query::ask_clickhouse`'s async pipeline.
/// Same `Handle::try_current()` branch RFC 0055's `ingest_sources` needed for the identical
/// class of problem — calling `Runtime::block_on` from *inside* an already-running multi-thread
/// runtime panics ("Cannot start a runtime from within a runtime"), so this bridges via
/// `block_in_place` instead of assuming no runtime is active.
fn run_clickhouse_query_blocking(
    config: &EkosConfig,
    workspace: &Path,
    question: &str,
) -> Result<Value> {
    let answer = match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| {
            handle.block_on(super::clickhouse::run_query(config, workspace, question))
        }),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(super::clickhouse::run_query(config, workspace, question))
        }
    }?;

    Ok(json!({
        "sql": answer.sql,
        "columns": answer.columns,
        "rows": answer.rows,
        "row_count": answer.row_count,
        "summary": answer.summary,
        "audit_event_id": answer.audit_event_id.to_string(),
    }))
}

/// Walks a Transformation IR chain upstream from `id` (RFC 0027/0028): the
/// target object itself, followed by everything that `FeedsInto` it,
/// ordered root-first-then-by-hop. Reuses `Runtime::trace_impact` exactly as
/// `ekos_impact` already does — no bespoke graph-walking mechanism.
fn transformation_chain(
    runtime: &Runtime,
    id: &KirId,
    max_hops: u32,
) -> Result<Vec<ekos_kir::KirObject>> {
    let root = runtime
        .load_object(id)?
        .ok_or_else(|| anyhow::anyhow!("object not found: {id}"))?;

    // FeedsInto edges point downstream (Source -> Filter -> Sink), so
    // walking *upstream* from `id` means following edges where `rel.to ==
    // current` back to `rel.from` — that's `ImpactDirection::Dependents`
    // ("what points at this"), not `Dependencies`, despite "what feeds into
    // this" sounding like a dependency relationship at first glance.
    let hops = runtime.trace_impact(
        id,
        ImpactDirection::Dependents,
        &[RelationshipKind::Custom("FeedsInto".to_string())],
        max_hops,
    )?;

    let mut chain = vec![root];
    chain.extend(hops.into_iter().map(|h| h.object));
    Ok(chain)
}

/// Renders one Transformation IR `KirObject` (RFC 0027's
/// `Custom("TransformNode")` shape) into a human-readable explanation step
/// with resolved evidence — mirrors `Runtime::reconstruct_state`'s evidence
/// resolution (`ekos_state`'s pattern) applied to a single object instead of
/// a whole `ObjectState`.
fn explain_node(
    ledger: &dyn ekos_ledger::KnowledgeStore,
    obj: &ekos_kir::KirObject,
) -> Result<Value> {
    let node_type = obj
        .properties
        .get("node_type")
        .and_then(Value::as_str)
        .unwrap_or("Unknown");

    let evidence: Vec<Value> = obj
        .evidence
        .iter()
        .filter_map(|ev_id| ledger.get_evidence(ev_id).ok().flatten())
        .map(|ev| {
            json!({
                "source": ev.location.path,
                "fragment": ev.fragment,
                "confidence": ev.confidence,
            })
        })
        .collect();

    Ok(json!({
        "id": obj.id.to_string(),
        "node_type": node_type,
        "summary": node_summary(obj, node_type),
        "evidence": evidence,
    }))
}

/// One human-readable sentence per Transformation IR node type, per RFC
/// 0028's mapping table.
fn node_summary(obj: &ekos_kir::KirObject, node_type: &str) -> String {
    let prop = |key: &str| {
        obj.properties
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    match node_type {
        "Source" => format!("reads from {}", prop("object_name")),
        "Sink" => format!("writes to {}", prop("object_name")),
        "Filter" => format!("filters rows where {}", prop("excerpt")),
        "Calculate" => format!("calculates {} = {}", prop("output"), prop("excerpt")),
        "Join" => format!(
            "{} joins on {}",
            prop("join_kind"),
            obj.properties
                .get("keys")
                .map(|v| v.to_string())
                .unwrap_or_default()
        ),
        "Aggregate" => format!(
            "groups by {}, aggregates {}",
            obj.properties
                .get("group_by")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            obj.properties
                .get("aggs")
                .map(|v| v.to_string())
                .unwrap_or_default()
        ),
        "Unmapped" => format!(
            "⚠ not understood: {} — raw: {}",
            prop("reason"),
            prop("raw")
        ),
        other => format!("unrecognized node_type: {other}"),
    }
}

/// A node's canonical comparable text, used only for `ekos_transformation_diff`'s
/// set-based comparison — deliberately not the same as `node_summary`'s
/// English-sentence rendering, since diffing should ignore wording, only the
/// underlying value (RFC 0028's "structural diffing over node text").
fn node_comparable(obj: &ekos_kir::KirObject, node_type: &str) -> String {
    let prop = |key: &str| {
        obj.properties
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    match node_type {
        "Source" | "Sink" => prop("object_name").to_string(),
        "Filter" => prop("excerpt").to_string(),
        "Calculate" => format!("{}={}", prop("output"), prop("excerpt")),
        "Join" => format!(
            "{}|{}",
            prop("join_kind"),
            obj.properties
                .get("keys")
                .map(|v| v.to_string())
                .unwrap_or_default()
        ),
        "Aggregate" => format!(
            "{}|{}",
            obj.properties
                .get("group_by")
                .map(|v| v.to_string())
                .unwrap_or_default(),
            obj.properties
                .get("aggs")
                .map(|v| v.to_string())
                .unwrap_or_default()
        ),
        _ => String::new(),
    }
}

/// Buckets two Transformation IR chains by node type and reports set
/// differences per bucket, plus `Unmapped` counts (RFC 0028's "Structural
/// diffing over node text, not a typed expression diff" design decision).
fn diff_chains(old: &[ekos_kir::KirObject], new: &[ekos_kir::KirObject]) -> Value {
    use std::collections::BTreeSet;

    fn bucket(chain: &[ekos_kir::KirObject], node_type: &str) -> BTreeSet<String> {
        chain
            .iter()
            .filter(|o| o.properties.get("node_type").and_then(Value::as_str) == Some(node_type))
            .map(|o| node_comparable(o, node_type))
            .collect()
    }

    fn set_diff(old_set: &BTreeSet<String>, new_set: &BTreeSet<String>) -> Value {
        json!({
            "added": new_set.difference(old_set).cloned().collect::<Vec<_>>(),
            "removed": old_set.difference(new_set).cloned().collect::<Vec<_>>(),
        })
    }

    let count_unmapped = |chain: &[ekos_kir::KirObject]| {
        chain
            .iter()
            .filter(|o| o.properties.get("node_type").and_then(Value::as_str) == Some("Unmapped"))
            .count()
    };

    json!({
        "sources": set_diff(&bucket(old, "Source"), &bucket(new, "Source")),
        "sinks": set_diff(&bucket(old, "Sink"), &bucket(new, "Sink")),
        "filters": set_diff(&bucket(old, "Filter"), &bucket(new, "Filter")),
        "joins": set_diff(&bucket(old, "Join"), &bucket(new, "Join")),
        "aggregates": set_diff(&bucket(old, "Aggregate"), &bucket(new, "Aggregate")),
        "calculates": set_diff(&bucket(old, "Calculate"), &bucket(new, "Calculate")),
        "unmapped": { "old_count": count_unmapped(old), "new_count": count_unmapped(new) },
    })
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
                "ekos_impact",
                "ekos_diff",
                "ekos_status",
                "ekos_transformation_explain",
                "ekos_transformation_diff",
                "ekos_identity_review"
            ]
        );
        for tool in tools {
            assert!(
                tool["inputSchema"]["type"] == "object",
                "every tool declares an object schema"
            );
        }
    }

    /// RFC 0056: `ekos_clickhouse_query` is the one MCP tool that touches a live external
    /// system — it must be absent from `tools/list` unless a workspace explicitly opts in.
    #[test]
    fn clickhouse_query_tool_absent_without_opt_in() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let resp =
            parse(&handle_message(&config, tmp.path(), &req(3, "tools/list", json!({}))).unwrap());
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(
            tools.iter().all(|t| t["name"] != "ekos_clickhouse_query"),
            "ekos_clickhouse_query must not be listed with the flag unset"
        );
    }

    #[test]
    fn clickhouse_query_tool_present_with_opt_in() {
        let mut config = EkosConfig::default();
        config.clickhouse.enable_mcp_query = true;
        let tmp = tempfile::tempdir().unwrap();
        let resp =
            parse(&handle_message(&config, tmp.path(), &req(3, "tools/list", json!({}))).unwrap());
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(
            tools.iter().any(|t| t["name"] == "ekos_clickhouse_query"),
            "ekos_clickhouse_query must be listed once [clickhouse].enable-mcp-query = true"
        );
    }

    /// Defense-in-depth: calling the tool by name directly (bypassing `tools/list`) must still
    /// be rejected when the gate is off, not merely hidden from discovery.
    #[test]
    fn clickhouse_query_call_rejected_when_gate_is_off() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let params = json!({ "name": "ekos_clickhouse_query", "arguments": { "question": "how many orders?" } });
        let resp =
            parse(&handle_message(&config, tmp.path(), &req(4, "tools/call", params)).unwrap());
        assert_eq!(resp["result"]["isError"], true);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("disabled"), "unexpected message: {text}");
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
    fn impact_of_unknown_object_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            13,
            "tools/call",
            json!({ "name": "ekos_impact",
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

    fn seeded_ledger(config: &EkosConfig, tmp: &Path) -> (ekos_kir::KirId, ekos_kir::KirId) {
        use ekos_kir::{KirObject, KirRelationship, ObjectKind};
        use ekos_ledger::Ledger;
        let ledger = Ledger::open(&config.ledger_path(tmp)).unwrap();
        let orders = KirObject::new("orders", ObjectKind::Table);
        let items = KirObject::new("order_items", ObjectKind::Table);
        ledger.append_object(&orders).unwrap();
        ledger.append_object(&items).unwrap();
        // order_items → orders: order_items depends on orders.
        ledger
            .append_relationship(&KirRelationship::new(
                RelationshipKind::ForeignKey,
                items.id,
                orders.id,
            ))
            .unwrap();
        (orders.id, items.id)
    }

    #[test]
    fn impact_traces_multi_hop_dependents() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let (orders_id, items_id) = seeded_ledger(&config, tmp.path());

        let line = req(
            15,
            "tools/call",
            json!({ "name": "ekos_impact",
                    "arguments": { "id": orders_id.to_string(), "direction": "dependents" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["count"], 1);
        assert_eq!(body["hops"][0]["id"], items_id.to_string());
        assert_eq!(body["hops"][0]["hop"], 1);
    }

    #[test]
    fn impact_with_invalid_direction_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let (orders_id, _items_id) = seeded_ledger(&config, tmp.path());

        let line = req(
            14,
            "tools/call",
            json!({ "name": "ekos_impact",
                    "arguments": { "id": orders_id.to_string(), "direction": "sideways" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
        assert!(
            resp["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("invalid `direction`")
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

    /// Builds a real `Source → Filter → Sink` Transformation IR graph (RFC
    /// 0027/0028) — `condition` is a parameter so two independently-built
    /// chains sharing everything but one Filter can be seeded for diff tests.
    fn seeded_transformation_ledger(
        config: &EkosConfig,
        tmp: &Path,
        source_path: &str,
        condition: &str,
    ) -> ekos_kir::KirId {
        use ekos_ledger::Ledger;
        use ekos_semantic::transform_ir::{
            TransformGraph, TransformNode, TransformOrigin, lower_to_kir,
        };

        let graph = TransformGraph {
            nodes: vec![
                TransformNode::Source {
                    object_name: "dbo.cust_mstr".to_string(),
                    columns: vec!["id".to_string(), "status".to_string()],
                },
                TransformNode::Filter {
                    condition: condition.to_string(),
                },
                TransformNode::Sink {
                    object_name: "gold.dim_customer".to_string(),
                    columns: vec!["id".to_string()],
                },
            ],
            edges: vec![
                (
                    ekos_semantic::transform_ir::NodeId(0),
                    ekos_semantic::transform_ir::NodeId(1),
                ),
                (
                    ekos_semantic::transform_ir::NodeId(1),
                    ekos_semantic::transform_ir::NodeId(2),
                ),
            ],
            origin: TransformOrigin {
                source_path: source_path.to_string(),
                source_kind: "pentaho-ktr".to_string(),
                extracted_at: Utc::now(),
            },
        };

        let kir = lower_to_kir(&graph);
        let ledger = Ledger::open(&config.ledger_path(tmp)).unwrap();
        for ev in &kir.evidence {
            ledger.append_evidence(ev).unwrap();
        }
        for obj in &kir.objects {
            ledger.append_object(obj).unwrap();
        }
        for rel in &kir.relationships {
            ledger.append_relationship(rel).unwrap();
        }

        // Sink is always the last node.
        kir.objects.last().unwrap().id
    }

    #[test]
    fn explain_walks_the_full_chain_with_evidence() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let sink_id = seeded_transformation_ledger(
            &config,
            tmp.path(),
            "jobs/load_customers.ktr",
            "status = 'active'",
        );

        let line = req(
            20,
            "tools/call",
            json!({ "name": "ekos_transformation_explain",
                    "arguments": { "id": sink_id.to_string() } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

        let steps = body["steps"].as_array().unwrap();
        assert_eq!(steps.len(), 3, "Sink, then Filter, then Source, root-first");
        assert_eq!(steps[0]["node_type"], "Sink");

        let filter_step = steps
            .iter()
            .find(|s| s["node_type"] == "Filter")
            .expect("Filter step present");
        assert!(
            filter_step["summary"]
                .as_str()
                .unwrap()
                .contains("status = 'active'")
        );
        assert!(
            !filter_step["evidence"].as_array().unwrap().is_empty(),
            "every step must carry resolved evidence"
        );
        assert_eq!(
            filter_step["evidence"][0]["source"],
            "jobs/load_customers.ktr"
        );

        assert!(steps.iter().any(|s| s["node_type"] == "Source"));
    }

    #[test]
    fn explain_of_unknown_object_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            21,
            "tools/call",
            json!({ "name": "ekos_transformation_explain",
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
    fn diff_detects_added_and_removed_filter() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let old_sink =
            seeded_transformation_ledger(&config, tmp.path(), "jobs/old.ktr", "status = 'active'");
        let new_sink =
            seeded_transformation_ledger(&config, tmp.path(), "jobs/new.ktr", "region = 'EU'");

        let line = req(
            22,
            "tools/call",
            json!({ "name": "ekos_transformation_diff",
                    "arguments": { "old_id": old_sink.to_string(), "new_id": new_sink.to_string() } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

        let filters = &body["diff"]["filters"];
        assert_eq!(filters["added"], json!(["region = 'EU'"]));
        assert_eq!(filters["removed"], json!(["status = 'active'"]));

        // Same Source/Sink object_name text on both sides — no diff there.
        assert_eq!(body["diff"]["sources"]["added"], json!([]));
        assert_eq!(body["diff"]["sources"]["removed"], json!([]));
        assert_eq!(body["diff"]["sinks"]["added"], json!([]));
        assert_eq!(body["diff"]["sinks"]["removed"], json!([]));
    }

    #[test]
    fn diff_of_identical_chains_reports_no_differences() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let sink_a =
            seeded_transformation_ledger(&config, tmp.path(), "jobs/a.ktr", "status = 'active'");
        let sink_b =
            seeded_transformation_ledger(&config, tmp.path(), "jobs/b.ktr", "status = 'active'");

        let line = req(
            23,
            "tools/call",
            json!({ "name": "ekos_transformation_diff",
                    "arguments": { "old_id": sink_a.to_string(), "new_id": sink_b.to_string() } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();

        for bucket in [
            "sources",
            "sinks",
            "filters",
            "joins",
            "aggregates",
            "calculates",
        ] {
            assert_eq!(body["diff"][bucket]["added"], json!([]), "{bucket} added");
            assert_eq!(
                body["diff"][bucket]["removed"],
                json!([]),
                "{bucket} removed"
            );
        }
        assert_eq!(body["diff"]["unmapped"]["old_count"], 0);
        assert_eq!(body["diff"]["unmapped"]["new_count"], 0);
    }

    /// Seeds one unconfirmed `Custom("SameAs")` relationship between two
    /// plain `Table` objects (RFC 0029), returning its id.
    fn seeded_same_as_relationship(config: &EkosConfig, tmp: &Path) -> ekos_kir::KirId {
        use ekos_kir::{KirObject, KirRelationship, ObjectKind};
        use ekos_ledger::Ledger;

        let ledger = Ledger::open(&config.ledger_path(tmp)).unwrap();
        let a = KirObject::new("cust_mstr", ObjectKind::Table);
        let b = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();

        let mut rel =
            KirRelationship::new(RelationshipKind::Custom("SameAs".to_string()), a.id, b.id);
        rel.properties.insert("status".into(), json!("unconfirmed"));
        rel.properties.insert("confidence".into(), json!(0.72));
        let rel_id = rel.id;
        ledger.append_relationship(&rel).unwrap();
        rel_id
    }

    #[test]
    fn identity_review_confirms_a_candidate_and_writes_an_event() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let rel_id = seeded_same_as_relationship(&config, tmp.path());

        let line = req(
            30,
            "tools/call",
            json!({ "name": "ekos_identity_review",
                    "arguments": { "relationship_id": rel_id.to_string(), "decision": "confirmed" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);
        let body: Value =
            serde_json::from_str(resp["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["decision"], "confirmed");

        // Status actually persisted — re-open the ledger and check directly.
        let ledger = ekos_ledger::Ledger::open(&config.ledger_path(tmp.path())).unwrap();
        let rel = ledger.get_relationship(&rel_id).unwrap().unwrap();
        assert_eq!(rel.properties["status"], "confirmed");
        assert!(rel.properties.contains_key("reviewed_at"));
    }

    #[test]
    fn identity_review_rejects_a_candidate() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let rel_id = seeded_same_as_relationship(&config, tmp.path());

        let line = req(
            31,
            "tools/call",
            json!({ "name": "ekos_identity_review",
                    "arguments": { "relationship_id": rel_id.to_string(), "decision": "rejected" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], false);

        let ledger = ekos_ledger::Ledger::open(&config.ledger_path(tmp.path())).unwrap();
        let rel = ledger.get_relationship(&rel_id).unwrap().unwrap();
        assert_eq!(rel.properties["status"], "rejected");
    }

    #[test]
    fn identity_review_with_invalid_decision_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let rel_id = seeded_same_as_relationship(&config, tmp.path());

        let line = req(
            32,
            "tools/call",
            json!({ "name": "ekos_identity_review",
                    "arguments": { "relationship_id": rel_id.to_string(), "decision": "maybe" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn identity_review_of_non_same_as_relationship_is_a_tool_error() {
        use ekos_kir::{KirObject, KirRelationship, ObjectKind};
        use ekos_ledger::Ledger;

        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(&config.ledger_path(tmp.path())).unwrap();
        let a = KirObject::new("orders", ObjectKind::Table);
        let b = KirObject::new("customers", ObjectKind::Table);
        ledger.append_object(&a).unwrap();
        ledger.append_object(&b).unwrap();
        let rel = KirRelationship::new(RelationshipKind::ForeignKey, a.id, b.id);
        let rel_id = rel.id;
        ledger.append_relationship(&rel).unwrap();

        let line = req(
            33,
            "tools/call",
            json!({ "name": "ekos_identity_review",
                    "arguments": { "relationship_id": rel_id.to_string(), "decision": "confirmed" } }),
        );
        let resp = parse(&handle_message(&config, tmp.path(), &line).unwrap());
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn identity_review_of_unknown_relationship_is_a_tool_error() {
        let config = EkosConfig::default();
        let tmp = tempfile::tempdir().unwrap();
        let line = req(
            34,
            "tools/call",
            json!({ "name": "ekos_identity_review",
                    "arguments": { "relationship_id": "00000000-0000-0000-0000-000000000000", "decision": "confirmed" } }),
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
}
