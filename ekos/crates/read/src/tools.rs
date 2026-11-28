//! The read tools: JSON arguments in, JSON out. These are the bodies of the MCP server's read
//! tools, moved here unchanged (RFC 0269 phase 1) so that the MCP server and the Python package
//! run one implementation. Argument names and result shapes are the tools' public contract.

use crate::args::{
    IMPACT_MAX_HOPS, NEIGHBORHOOD_DEFAULT_OBJECTS, NEIGHBORHOOD_MAX_DEPTH,
    NEIGHBORHOOD_MAX_OBJECTS, bounded_arg, required_id, required_str,
};
use crate::{ReadError, Result};
use chrono::{DateTime, Utc};
use ekos_compiler_core::EkosConfig;
use ekos_ekl::{EklInterpreter, ekl_parse};
use ekos_kir::{KirId, RelationshipKind};
use ekos_ledger::KnowledgeStore;
use ekos_runtime::reason::{execute, plan_question};
use ekos_runtime::retrieval::understand;
use ekos_runtime::{
    ExportLevel, GraphExportOptions, GroupBy, ImpactDirection, RetrievalRequest, Runtime,
    export_graph,
};
use serde_json::{Value, json};
use std::path::Path;
use std::str::FromStr;

macro_rules! arg_err {
    ($($t:tt)*) => { ReadError::Argument(format!($($t)*)) };
}
macro_rules! arg_bail {
    ($($t:tt)*) => { return Err(ReadError::Argument(format!($($t)*))) };
}

/// The search modes that need the query as a vector.
pub fn search_needs_embedding(args: &Value) -> bool {
    matches!(
        args.get("mode").and_then(Value::as_str),
        Some("vector" | "hybrid")
    )
}

/// The `ekos_search` tool. `query_embedding` is the query as a vector, for the modes
/// [`search_needs_embedding`] names: this crate makes no network call, so the caller supplies
/// it — the MCP server from the project's embedding provider, the Python package from its
/// caller.
pub fn search(
    runtime: &Runtime<'_>,
    args: &Value,
    query_embedding: Option<Vec<f32>>,
) -> Result<Value> {
    let query = required_str(args, "query")?;
    // RFC 0119: route through the retrieval seam. RFC 0125: `mode` vector/hybrid.
    let limit = args
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(20);
    let mode = args
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("lexical");
    let mut req = RetrievalRequest::lexical(query);
    req.limit = limit;
    match mode {
        "lexical" => {}
        "vector" | "hybrid" => {
            req.query_embedding = Some(query_embedding.ok_or_else(|| {
                arg_err!("search mode {mode:?} needs the query as a vector (an embedding)")
            })?);
            if mode == "vector" {
                req.arms.bm25 = false;
            }
        }
        other => arg_bail!("unknown mode {other:?} (want lexical/vector/hybrid)"),
    }
    let result = runtime.retrieve(&req)?;
    Ok(json!({
        "arms_run": { "bm25": result.arms_run.bm25, "vector": result.arms_run.vector },
        // RFC 0126: per-arm wall-clock; `log_call` lifts this into the usage log.
        "arm_timings": result.arm_timings.iter().map(|t| json!({
            "source": t.source,
            "elapsed_ms": t.elapsed_ms,
            "candidates": t.candidates,
        })).collect::<Vec<_>>(),
        "matches": result.hits
            .iter()
            .take(limit)
            .map(|hit| json!({ "id": hit.id.to_string(), "name": hit.name }))
            .collect::<Vec<_>>()
    }))
}

/// The `ekos_query` tool.
pub fn query(
    config: &EkosConfig,
    workspace: &Path,
    ledger: &dyn KnowledgeStore,
    runtime: &Runtime<'_>,
    args: &Value,
) -> Result<Value> {
    let question = required_str(args, "question")?;
    let plan = plan_question(question, runtime)?;
    let mut evidence = execute(&plan, runtime)?;
    // RFC 0173 — a claim about an attribute sources disagree on says so.
    if config.conflicts.enabled {
        crate::conflicts::flag_disputed(config, workspace, ledger, &mut evidence)?;
    }
    Ok(serde_json::to_value(&evidence)?)
}

/// The `ekos_retrieve` tool.
pub fn retrieve(
    config: &EkosConfig,
    workspace: &Path,
    ledger: &dyn KnowledgeStore,
    runtime: &Runtime<'_>,
    args: &Value,
) -> Result<Value> {
    let question = required_str(args, "question")?;
    let understanding = understand(question, runtime)?;
    let plan = plan_question(question, runtime)?;
    let mut evidence = execute(&plan, runtime)?;
    if config.conflicts.enabled {
        crate::conflicts::flag_disputed(config, workspace, ledger, &mut evidence)?;
    }
    // RFC 0126: also run the raw retrieval seam so "show your work" includes which arms
    // fired and how long each took.
    let retrieved = runtime.retrieve(&RetrievalRequest::lexical(question))?;
    Ok(json!({
        "plan": serde_json::to_value(&plan)?,
        "evidence": serde_json::to_value(&evidence)?,
        "arms_run": { "bm25": retrieved.arms_run.bm25, "vector": retrieved.arms_run.vector },
        "arm_timings": retrieved.arm_timings.iter().map(|t| json!({
            "source": t.source,
            "elapsed_ms": t.elapsed_ms,
            "candidates": t.candidates,
        })).collect::<Vec<_>>(),
        "understanding": {
            "query_type": format!("{:?}", understanding.query_type),
            "keywords": understanding.keywords,
            "resolved_entities": understanding.resolved_entities.iter().map(|e| json!({
                "mention": e.mention,
                "id": e.id.to_string(),
                "name": e.name,
                "confidence": e.confidence,
                "via": e.via,
            })).collect::<Vec<_>>(),
        },
    }))
}

/// The `ekos_ekl` tool.
pub fn ekl(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let query = required_str(args, "query")?;
    let ast = ekl_parse(query)
        .map_err(|e| arg_err!("EKL parse error at column {}: {}", e.position, e.message))?;
    let interpreter = EklInterpreter::new(runtime);
    let result = interpreter
        .execute(&ast)
        .map_err(|e| arg_err!("EKL error: {e}"))?;
    Ok(json!({ "count": result.rows.len(), "rows": result.rows }))
}

/// The `ekos_neighborhood` tool.
pub fn neighborhood(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let id = required_id(args)?;
    let depth = bounded_arg(args, "depth", 1, 0, NEIGHBORHOOD_MAX_DEPTH)? as u32;
    let max_objects = bounded_arg(
        args,
        "max_objects",
        NEIGHBORHOOD_DEFAULT_OBJECTS,
        1,
        NEIGHBORHOOD_MAX_OBJECTS,
    )? as usize;
    let (graph, truncated) = runtime.load_neighborhood_bounded(&id, depth, max_objects)?;
    let mut out = serde_json::to_value(&graph)?;
    if let Value::Object(map) = &mut out {
        map.insert("truncated".into(), json!(truncated));
        map.insert("max_objects".into(), json!(max_objects));
    }
    Ok(out)
}

/// The `ekos_audit` tool.
pub fn audit(ledger: &dyn KnowledgeStore, args: &Value) -> Result<Value> {
    let id = required_id(args)?;
    let trail = ledger.audit_trail(&id)?;
    Ok(serde_json::json!({ "id": id.to_string(), "writes": trail }))
}

/// The `ekos_state` tool.
pub fn state(
    config: &EkosConfig,
    workspace: &Path,
    ledger: &dyn KnowledgeStore,
    runtime: &Runtime<'_>,
    args: &Value,
) -> Result<Value> {
    let id = required_id(args)?;
    let state = match args.get("at").and_then(Value::as_str) {
        Some(at) => {
            let at: DateTime<Utc> = at
                .parse()
                .map_err(|e| arg_err!("invalid `at` timestamp (want RFC 3339): {e}"))?;
            runtime.reconstruct_state_at(&id, at)?
        }
        None => runtime.reconstruct_state(&id)?,
    };
    let mut value = state
        .map(|s| serde_json::to_value(&s))
        .transpose()?
        .ok_or_else(|| ReadError::NotFound(format!("object not found: {id}")))?;
    // RFC 0172 — sources that disagree about this object, unresolved.
    if config.conflicts.enabled
        && let Some(obj) = value.as_object_mut()
    {
        let open = crate::conflicts::open_for(config, workspace, ledger, &id)?;
        if !open.is_empty() {
            obj.insert("open_conflicts".into(), json!(open));
        }
    }
    Ok(value)
}

/// The `ekos_dependents` tool.
pub fn dependents(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let id = required_id(args)?;
    let target = runtime
        .load_object(&id)?
        .ok_or_else(|| ReadError::NotFound(format!("object not found: {id}")))?;

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
        // Issue #17: a hypothesis or proposal at the other end is not a dependency fact.
        if other
            .as_ref()
            .is_some_and(ekos_kir::KirObject::is_unconfirmed_claim)
        {
            continue;
        }
        bucket.push(json!({
            "id": other_id.to_string(),
            "name": other.as_ref().map(|o| o.name.clone()),
            "kind": other.as_ref().map(|o| o.kind.to_string()),
            "status": other.as_ref().and_then(|o| o.properties.get("status")),
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

/// The `ekos_impact` tool.
pub fn impact(runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let id = required_id(args)?;
    runtime
        .load_object(&id)?
        .ok_or_else(|| ReadError::NotFound(format!("object not found: {id}")))?;

    let direction = match args.get("direction").and_then(Value::as_str) {
        Some("dependencies") => ImpactDirection::Dependencies,
        Some("dependents") | None => ImpactDirection::Dependents,
        Some(other) => {
            arg_bail!("invalid `direction`: {other} (want \"dependents\" or \"dependencies\")")
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
    let max_hops = bounded_arg(args, "max_hops", 5, 1, IMPACT_MAX_HOPS)? as u32;

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

/// The `ekos_graph_export` tool.
pub fn graph_export(workspace: &Path, ledger: &dyn KnowledgeStore, args: &Value) -> Result<Value> {
    let str_list = |key: &str| -> Vec<String> {
        args.get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let usize_arg = |key: &str, default: usize| {
        args.get(key)
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(default)
    };

    let level = match args.get("level").and_then(Value::as_str) {
        Some("aggregate") => ExportLevel::Aggregate,
        Some("object") | None => ExportLevel::Object,
        Some(other) => {
            arg_bail!("invalid `level`: {other} (want \"object\" or \"aggregate\")")
        }
    };
    let group_by = match args.get("group_by").and_then(Value::as_str) {
        Some("path_prefix") => GroupBy::PathPrefix {
            depth: usize_arg("path_prefix_depth", 2).max(1),
        },
        Some("kind") | None => GroupBy::Kind,
        Some(other) => {
            arg_bail!("invalid `group_by`: {other} (want \"kind\" or \"path_prefix\")")
        }
    };

    let kinds = {
        let v = str_list("kinds");
        if v.is_empty() {
            None
        } else {
            Some(
                v.iter()
                    .map(|s| crate::kinds::parse_object_kind(s))
                    .collect(),
            )
        }
    };
    let rel_kinds = {
        let v = str_list("rel_kinds");
        if v.is_empty() {
            None
        } else {
            Some(
                v.iter()
                    .map(|s| RelationshipKind::from_str(s).expect("infallible"))
                    .collect(),
            )
        }
    };
    let exclude_rel_kinds = str_list("exclude_rel_kinds")
        .iter()
        .map(|s| RelationshipKind::from_str(s).expect("infallible"))
        .collect();

    let as_of = match args.get("as_of").and_then(Value::as_str) {
        Some(raw) => Some(
            raw.parse::<DateTime<Utc>>()
                .map_err(|e| arg_err!("invalid `as_of` timestamp (want RFC 3339): {e}"))?,
        ),
        None => None,
    };
    let opts = GraphExportOptions {
        level,
        workspace: workspace.to_path_buf(),
        kinds,
        rel_kinds,
        exclude_rel_kinds,
        group_by,
        max_nodes: usize_arg("max_nodes", 5_000),
        max_edges: usize_arg("max_edges", 20_000),
        min_degree: usize_arg("min_degree", 0),
        include_properties: str_list("include_properties"),
        as_of,
        include_first_seen: args
            .get("include_first_seen")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    };
    let graph = export_graph(ledger, &opts)?;
    Ok(serde_json::to_value(graph)?)
}

/// The `ekos_diff` tool.
pub fn diff(ledger: &dyn KnowledgeStore, runtime: &Runtime<'_>, args: &Value) -> Result<Value> {
    let from: DateTime<Utc> = required_str(args, "from")?
        .parse()
        .map_err(|e| arg_err!("invalid `from` timestamp (want RFC 3339): {e}"))?;
    let to: DateTime<Utc> = match args.get("to").and_then(Value::as_str) {
        Some(raw) => raw
            .parse()
            .map_err(|e| arg_err!("invalid `to` timestamp (want RFC 3339): {e}"))?,
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

/// The `ekos_status` tool's counts. `ledger_path` is the caller's: where the store is.
pub fn status(ledger: &dyn KnowledgeStore, ledger_path: &str) -> Result<Value> {
    Ok(json!({
        "entries": ledger.entry_count()?,
        "objects": ledger.object_count()?,
        "relationships": ledger.relationship_count()?,
        "ledger_path": ledger_path,
    }))
}
