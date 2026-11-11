//! `ekos transformation graph|list` — RFC 0257: a recovered transformation as a chain of steps.
//!
//! The chain itself is `ekos_semantic::transform_chain` (pure). This file loads the graph's
//! objects, edges, evidence and annotations from a store, reads the owning definition's text from
//! its file (redacted, RFC 0043 — the same text recovery read), and prints. The MCP tool
//! `ekos_transformation_graph` calls [`chain_json`] with the call's (possibly scoped) store, so a
//! restricted reader is shown only steps that store returns.
//!
//! Read-only: nothing here writes. Annotations on a step are written with `ekos annotate add
//! <node id> …` (human-only, RFC 0247).

use super::store::open_store_read_only;
use anyhow::{Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{KirId, KirObject, RelationshipKind};
use ekos_ledger::KnowledgeReader;
use ekos_semantic::annotations;
use ekos_semantic::transform_chain::{
    self, ChainGraph, Context, EvidenceView, Expand, Selection, SourceText,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::str::FromStr;

/// `--expand`: `all`, or 1-based statement numbers separated by commas.
pub fn parse_expand(raw: Option<&str>) -> Result<Expand> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(Expand::Auto);
    };
    if raw.eq_ignore_ascii_case("all") {
        return Ok(Expand::All);
    }
    let mut out = Vec::new();
    for part in raw.split(',') {
        let n: u32 = part
            .trim()
            .parse()
            .map_err(|_| anyhow!("--expand takes `all` or statement numbers, not `{part}`"))?;
        if n == 0 {
            bail!("statement numbers start at 1");
        }
        out.push(n - 1);
    }
    Ok(Expand::Statements(out))
}

fn feeds_into() -> RelationshipKind {
    RelationshipKind::Custom("FeedsInto".into())
}

fn edges_of(
    reader: &dyn KnowledgeReader,
    selection: &Selection<'_>,
) -> Result<Vec<(KirId, KirId)>> {
    let mut out = Vec::new();
    for node in &selection.nodes {
        for rel in reader.relationships_for(&node.id)? {
            // Each edge is listed by both its ends; keep it once, from its source.
            if rel.kind == feeds_into() && rel.from == node.id {
                out.push((rel.from, rel.to));
            }
        }
    }
    Ok(out)
}

fn evidence_of(
    reader: &dyn KnowledgeReader,
    selection: &Selection<'_>,
) -> Result<HashMap<KirId, Vec<EvidenceView>>> {
    let mut out = HashMap::new();
    for node in &selection.nodes {
        let mut views = Vec::new();
        for id in &node.evidence {
            if let Some(ev) = reader.get_evidence(id)? {
                views.push(EvidenceView {
                    path: ev.location.path.clone(),
                    line: ev.location.line,
                    fragment: ev.fragment.clone(),
                    confidence: ev.confidence,
                });
            }
        }
        out.insert(node.id, views);
    }
    Ok(out)
}

fn annotations_of(objects: &[KirObject], selection: &Selection<'_>) -> HashMap<String, Vec<Value>> {
    let ids: Vec<String> = selection.nodes.iter().map(|o| o.id.0.to_string()).collect();
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    for a in annotations::current(objects) {
        if ids.contains(&a.target) {
            out.entry(a.target.clone())
                .or_default()
                .push(super::annotate::view(&a, objects));
        }
    }
    out
}

fn span(o: &KirObject, key: &str) -> Option<usize> {
    o.properties
        .get(key)
        .and_then(Value::as_u64)
        .map(|n| n as usize)
}

/// The owning definition's text by its recorded span, redacted. `None` when no span is recorded,
/// the file is excluded or gone, or the span no longer holds a definition of this name.
fn definition_text(config: &EkosConfig, cwd: &Path, owner: &KirObject) -> Option<SourceText> {
    let path = owner.properties.get("source_path")?.as_str()?;
    let (start, end) = (span(owner, "span_start")?, span(owner, "span_end")?);
    let text = ekos_common::redaction::read_redacted(cwd, path, &config.redaction_config()).ok()?;
    let slice = text.get(start..end)?;
    let bare = owner.name.rsplit('.').next().unwrap_or(&owner.name);
    if !slice
        .to_ascii_lowercase()
        .contains(&bare.to_ascii_lowercase())
    {
        return None;
    }
    let start_line = owner
        .properties
        .get("line")
        .and_then(Value::as_u64)
        .map_or_else(
            || 1 + text[..start].matches('\n').count() as u32,
            |l| l as u32,
        );
    Some(SourceText {
        path: path.to_string(),
        start_line,
        text: slice.to_string(),
    })
}

/// The chain for `object` (a TransformNode, View or Procedure id), read through `reader`.
pub fn chain(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn KnowledgeReader,
    object: &str,
    expand: Expand,
) -> Result<ChainGraph> {
    let id = KirId::from_str(object.trim()).map_err(|_| anyhow!("`{object}` is not an id"))?;
    let objects = reader.all_objects()?;
    let target = objects
        .iter()
        .find(|o| o.id == id)
        .ok_or_else(|| anyhow!("object not found: {id}"))?;
    let selection = transform_chain::select(&objects, target).map_err(|e| anyhow!(e))?;
    let ctx = Context {
        edges: edges_of(reader, &selection)?,
        evidence: evidence_of(reader, &selection)?,
        annotations: annotations_of(&objects, &selection),
        source: selection
            .owner
            .and_then(|o| definition_text(config, cwd, o)),
        expand,
    };
    Ok(transform_chain::build(&selection, &ctx))
}

/// [`chain`] as JSON — what the MCP tool returns.
pub fn chain_json(
    config: &EkosConfig,
    cwd: &Path,
    reader: &dyn KnowledgeReader,
    args: &Value,
) -> Result<Value> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing `id`"))?;
    let expand = parse_expand(args.get("expand").and_then(Value::as_str))?;
    Ok(serde_json::to_value(chain(
        config, cwd, reader, id, expand,
    )?)?)
}

/// `ekos transformation graph --object <id> [--expand …] [--json]`.
pub fn graph(
    config: &EkosConfig,
    cwd: &Path,
    object: &str,
    expand: Option<&str>,
    json_out: bool,
) -> Result<()> {
    let store = open_store_read_only(config, cwd)?;
    let g = chain(config, cwd, store.as_ref(), object, parse_expand(expand)?)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&g)?);
        return Ok(());
    }
    let t = &g.transformation;
    println!("{} `{}` — {}", t.kind, t.name, t.key);
    println!(
        "{} step(s), {} not understood{}",
        g.counts.nodes,
        g.counts.unmapped,
        if g.collapsed {
            " (collapsed by statement; --expand all or --expand <n> to see the steps)"
        } else {
            ""
        }
    );
    for n in &g.nodes {
        let index = n.index.map_or_else(|| "-".to_string(), |i| i.to_string());
        println!("  [{index:>3}] {:<9} {}", n.node_type, n.summary);
        if !n.annotations.is_empty() {
            println!("        {} annotation(s)", n.annotations.len());
        }
    }
    Ok(())
}

/// One row of `ekos transformation list`.
fn list_rows(objects: &[KirObject]) -> Vec<Value> {
    let mut by_key: BTreeMap<&str, Vec<&KirObject>> = BTreeMap::new();
    for o in objects
        .iter()
        .filter(|o| transform_chain::is_transform_node(o))
    {
        if let Some(key) = transform_chain::graph_key(&o.name) {
            by_key.entry(key).or_default().push(o);
        }
    }
    let prop = |o: &KirObject, k: &str| {
        o.properties
            .get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    };
    let mut rows: Vec<Value> = by_key
        .into_iter()
        .map(|(key, nodes)| {
            let first = nodes
                .iter()
                .min_by_key(|o| {
                    o.name
                        .rsplit(':')
                        .next()
                        .and_then(|i| i.parse::<u32>().ok())
                })
                .copied()
                .unwrap_or(nodes[0]);
            let owner = prop(first, "owner");
            let kind = match prop(first, "owner_kind").as_str() {
                "" => "query".to_string(),
                k => k.to_string(),
            };
            let unmapped = nodes
                .iter()
                .filter(|o| prop(o, "node_type") == "Unmapped")
                .count();
            json!({
                "key": key,
                "name": if owner.is_empty() { key.to_string() } else { owner },
                "kind": kind,
                "object": first.id.0.to_string(),
                "steps": nodes.len(),
                "unmapped": unmapped,
            })
        })
        .collect();
    // Views first, then routines, then standalone statements; by name within each.
    let rank = |v: &Value| match v["kind"].as_str() {
        Some("view") => 0,
        Some("routine") => 1,
        _ => 2,
    };
    rows.sort_by_cached_key(|v| {
        let name = v["name"].as_str().unwrap_or_default().to_lowercase();
        (rank(v), name)
    });
    rows
}

/// `ekos transformation list` options.
pub struct ListArgs<'a> {
    pub filter: Option<&'a str>,
    pub limit: usize,
    /// Keep standalone statements of which nothing was understood (by default they are counted
    /// as `hidden`: a script whose whole-file parse failed yields one per statement).
    pub all: bool,
    pub json: bool,
}

fn keep_row(row: &Value, needle: Option<&str>, all: bool) -> bool {
    let nothing_understood = row["kind"] == "query" && row["steps"] == row["unmapped"];
    let named = needle.is_none_or(|n| {
        row["name"]
            .as_str()
            .is_some_and(|s| s.to_ascii_lowercase().contains(n))
    });
    named && (all || !nothing_understood)
}

/// `ekos transformation list [--filter] [--limit] [--all] [--json]`.
pub fn list(config: &EkosConfig, cwd: &Path, args: ListArgs<'_>) -> Result<()> {
    let store = open_store_read_only(config, cwd)?;
    let objects = store.all_objects()?;
    let needle = args.filter.map(str::to_ascii_lowercase);
    let rows = list_rows(&objects);
    let hidden = rows
        .iter()
        .filter(|r| {
            keep_row(r, needle.as_deref(), true) && !keep_row(r, needle.as_deref(), args.all)
        })
        .count();
    let all: Vec<Value> = rows
        .into_iter()
        .filter(|r| keep_row(r, needle.as_deref(), args.all))
        .collect();
    let total = all.len();
    let rows: Vec<Value> = all.into_iter().take(args.limit).collect();
    if args.json {
        let out = json!({ "total": total, "hidden": hidden, "transformations": rows });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    for r in &rows {
        println!(
            "{:<8} {:<40} {:>4} steps {:>4} not understood  {}",
            r["kind"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
            r["steps"],
            r["unmapped"],
            r["object"].as_str().unwrap_or("")
        );
    }
    println!(
        "{} of {total} transformation(s); {hidden} statement(s) of which nothing was understood \
         not listed (--all)",
        rows.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_takes_all_or_one_based_numbers() {
        assert_eq!(parse_expand(None).unwrap(), Expand::Auto);
        assert_eq!(parse_expand(Some("ALL")).unwrap(), Expand::All);
        assert_eq!(
            parse_expand(Some("1, 3")).unwrap(),
            Expand::Statements(vec![0, 2])
        );
        assert!(parse_expand(Some("0")).is_err());
        assert!(parse_expand(Some("x")).is_err());
    }

    use ekos_kir::{KirEvidence, KirRelationship, ObjectKind, SourceLocation};
    use ekos_ledger::KnowledgeStore;
    use ekos_ledger::scope::{Effect, ReadScope, Rule, ScopedStore};

    /// A graph `key` whose node `i` cites evidence in `paths[i]`, chained 0 → 1 → …
    fn put_graph(store: &dyn KnowledgeStore, key: &str, paths: &[&str]) -> Vec<KirId> {
        let mut ids = Vec::new();
        for (i, path) in paths.iter().enumerate() {
            let mut ev = KirEvidence::new(SourceLocation::file(*path), format!("t{i}"));
            ev.id = KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("{key}:{i}:ev").as_bytes(),
            ));
            store.append_evidence(&ev).unwrap();
            let mut o = KirObject::new(
                format!("{key}:{i}"),
                ObjectKind::Custom("TransformNode".into()),
            );
            o.id = KirId(uuid::Uuid::new_v5(
                &uuid::Uuid::NAMESPACE_URL,
                format!("{key}:{i}").as_bytes(),
            ));
            o.properties.insert("node_type".into(), json!("Source"));
            o.properties
                .insert("object_name".into(), json!(format!("t{i}")));
            o.evidence.push(ev.id);
            store.append_object(&o).unwrap();
            if let Some(prev) = ids.last() {
                let rel = KirRelationship::deterministic(feeds_into(), *prev, o.id, "");
                store.append_relationship(&rel).unwrap();
            }
            ids.push(o.id);
        }
        ids
    }

    /// RFC 0257 §5: a restricted reader sees no step whose source is out of scope.
    #[test]
    fn a_restricted_reader_sees_no_step_whose_source_is_out_of_scope() {
        let dir = tempfile::tempdir().unwrap();
        let ledger = ekos_ledger::Ledger::open(&dir.path().join("ledger.db")).unwrap();
        let mixed = put_graph(
            &ledger,
            "a/v.sql#0",
            &["a/v.sql", "b/secret.sql", "a/v.sql"],
        );
        let other = put_graph(&ledger, "b/w.sql#0", &["b/w.sql", "b/w.sql"]);
        let scope = ReadScope {
            default: Effect::Deny,
            rules: vec![Rule {
                effect: Effect::Allow,
                source: None,
                path: Some("a/**".into()),
                object: None,
            }],
        };
        let scoped = ScopedStore::with_scopes(Box::new(ledger), &[scope]).unwrap();
        let config = EkosConfig::default();

        let g = chain(
            &config,
            dir.path(),
            &scoped,
            &mixed[0].0.to_string(),
            Expand::All,
        )
        .unwrap();
        let shown: Vec<String> = g.nodes.iter().map(|n| n.id.clone()).collect();
        assert_eq!(
            shown,
            vec![mixed[0].0.to_string(), mixed[2].0.to_string()],
            "the step citing b/ is not shown"
        );
        assert!(g.edges.is_empty(), "both edges touch the hidden step");
        assert!(
            g.nodes
                .iter()
                .flat_map(|n| &n.evidence)
                .all(|e| e.path.starts_with("a/"))
        );
        let hidden = chain(
            &config,
            dir.path(),
            &scoped,
            &other[0].0.to_string(),
            Expand::All,
        );
        assert!(hidden.is_err(), "a graph wholly out of scope is not found");
        let args = json!({"id": mixed[1].0.to_string()});
        assert!(chain_json(&config, dir.path(), &scoped, &args).is_err());
    }
}
