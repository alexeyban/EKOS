//! `ekos graph export` — bulk graph extraction from a compiled workspace (RFC 0127 R1).
//!
//! The first "give me the whole graph" read path in EKOS: every other read is per-object
//! (`ekos_neighborhood` / `ekos_impact`) or rank-capped (`ekos_search` at `LIMIT 50`). All the
//! actual work lives in [`ekos_runtime::export_graph`] — this file is argument parsing, a
//! read-only store open (RFC 0005/0097), and serialization. The R3 MCP tool
//! (`ekos_graph_export`) calls the same runtime function, so the two surfaces cannot drift.

use super::store::open_store_read_only;
use anyhow::{Context, Result};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{ObjectKind, RelationshipKind};
use ekos_runtime::{ExportLevel, GraphExportOptions, GroupBy};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// Wire format for `ekos graph export`. `--format json` (default) or `--format ndjson`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One pretty-printed JSON object (the whole [`ekos_runtime::GraphExport`]).
    Json,
    /// Newline-delimited: a `{"record":"header",…}` line, then one `{"record":"node",…}` per node,
    /// then one `{"record":"edge",…}` per edge. Still materialised in full first (RFC 0127 §4.8).
    Ndjson,
}

impl Format {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "json" => Ok(Format::Json),
            "ndjson" => Ok(Format::Ndjson),
            other => Err(anyhow::anyhow!(
                "unknown --format '{other}' — expected 'json' or 'ndjson'"
            )),
        }
    }
}

/// Parses [`ExportLevel`] from `--level`.
pub fn parse_level(s: &str) -> Result<ExportLevel> {
    match s {
        "object" => Ok(ExportLevel::Object),
        "aggregate" => Ok(ExportLevel::Aggregate),
        other => Err(anyhow::anyhow!(
            "unknown --level '{other}' — expected 'object' or 'aggregate'"
        )),
    }
}

/// Parses [`GroupBy`] from `--group-by` + `--path-prefix-depth` (only consulted for
/// `--level aggregate`).
pub fn parse_group_by(s: &str, path_prefix_depth: usize) -> Result<GroupBy> {
    match s {
        "kind" => Ok(GroupBy::Kind),
        "path-prefix" => Ok(GroupBy::PathPrefix {
            depth: path_prefix_depth.max(1),
        }),
        other => Err(anyhow::anyhow!(
            "unknown --group-by '{other}' — expected 'kind' or 'path-prefix'"
        )),
    }
}

/// Parses an [`ObjectKind`] name case-insensitively against the built-in variants, falling back to
/// [`ObjectKind::Custom`] for anything else — the same escape-hatch posture
/// [`RelationshipKind::from_str`] takes. Shared with the R3 MCP tool.
pub fn parse_object_kind(s: &str) -> ObjectKind {
    macro_rules! ci {
        ($($variant:ident),+ $(,)?) => {
            $(if s.eq_ignore_ascii_case(stringify!($variant)) {
                return ObjectKind::$variant;
            })+
        };
    }
    ci!(
        File,
        Directory,
        Table,
        Entity,
        Service,
        Api,
        BusinessRule,
        BusinessConcept,
        Dataset,
        Column,
        Pipeline,
        Dashboard,
        Person,
        Model,
        Prompt,
        Agent,
        Unknown,
    );
    ObjectKind::Custom(s.to_string())
}

/// Fully-resolved arguments for [`export`], one field per CLI flag (`bin/ekos.rs` does the clap
/// parsing, this module does the domain parsing).
#[derive(Debug, Clone)]
pub struct ExportArgs {
    pub workspace: Option<PathBuf>,
    pub level: String,
    pub format: String,
    pub kinds: Vec<String>,
    pub rel_kinds: Vec<String>,
    pub exclude_rel_kinds: Vec<String>,
    pub group_by: String,
    pub path_prefix_depth: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub min_degree: usize,
    pub include_properties: Vec<String>,
    /// RFC 0134 — reconstruct the graph as of this RFC 3339 instant (`None` = current).
    pub as_of: Option<String>,
    /// RFC 0134 — stamp each node/edge with its first-seen time (`fs`).
    pub first_seen: bool,
    pub output: Option<PathBuf>,
}

pub fn export(config: &EkosConfig, cwd: &Path, args: ExportArgs) -> Result<()> {
    let workspace = match &args.workspace {
        Some(p) => p.clone(),
        None => cwd.to_path_buf(),
    };
    let workspace = workspace
        .canonicalize()
        .with_context(|| format!("workspace path does not exist: {}", workspace.display()))?;

    let level = parse_level(&args.level)?;
    let format = Format::parse(&args.format)?;
    let group_by = parse_group_by(&args.group_by, args.path_prefix_depth)?;

    let kinds = if args.kinds.is_empty() {
        None
    } else {
        Some(args.kinds.iter().map(|s| parse_object_kind(s)).collect())
    };
    let rel_kinds = if args.rel_kinds.is_empty() {
        None
    } else {
        Some(
            args.rel_kinds
                .iter()
                // `RelationshipKind::from_str` is infallible (unknown → `Custom`).
                .map(|s| RelationshipKind::from_str(s).unwrap())
                .collect(),
        )
    };
    let exclude_rel_kinds = args
        .exclude_rel_kinds
        .iter()
        .map(|s| RelationshipKind::from_str(s).unwrap())
        .collect();

    let as_of = args
        .as_of
        .as_deref()
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .with_context(|| format!("--as-of is not an RFC 3339 timestamp: {s}"))
        })
        .transpose()?;

    let opts = GraphExportOptions {
        level,
        workspace: workspace.clone(),
        kinds,
        rel_kinds,
        exclude_rel_kinds,
        group_by,
        max_nodes: args.max_nodes,
        max_edges: args.max_edges,
        min_degree: args.min_degree,
        include_properties: args.include_properties,
        as_of,
        include_first_seen: args.first_seen,
    };

    // A read — read-only open (RFC 0005/0097).
    let store = open_store_read_only(config, &workspace)?;
    let graph = ekos_runtime::export_graph(&*store, &opts)?;

    let mut sink: Box<dyn Write> = match &args.output {
        Some(path) => Box::new(std::io::BufWriter::new(
            std::fs::File::create(path)
                .with_context(|| format!("cannot create --output file: {}", path.display()))?,
        )),
        None => Box::new(std::io::stdout().lock()),
    };

    match format {
        Format::Json => {
            serde_json::to_writer_pretty(&mut sink, &graph)?;
            writeln!(sink)?;
        }
        Format::Ndjson => write_ndjson(&mut sink, &graph)?,
    }
    sink.flush()?;
    Ok(())
}

/// NDJSON: a `header` record carrying everything except `nodes`/`edges`, then one record per node
/// and per edge. The node/edge objects are emitted exactly as they serialize in the JSON form
/// (short keys and all), with a `"record"` discriminator added.
fn write_ndjson(sink: &mut dyn Write, graph: &ekos_runtime::GraphExport) -> Result<()> {
    let mut header = serde_json::to_value(graph)?;
    let obj = header
        .as_object_mut()
        .expect("GraphExport serializes to an object");
    obj.remove("nodes");
    obj.remove("edges");
    obj.insert("record".into(), "header".into());
    writeln!(sink, "{}", serde_json::to_string(&header)?)?;

    for node in &graph.nodes {
        let mut v = serde_json::to_value(node)?;
        v.as_object_mut()
            .unwrap()
            .insert("record".into(), "node".into());
        writeln!(sink, "{}", serde_json::to_string(&v)?)?;
    }
    for edge in &graph.edges {
        let mut v = serde_json::to_value(edge)?;
        v.as_object_mut()
            .unwrap()
            .insert("record".into(), "edge".into());
        writeln!(sink, "{}", serde_json::to_string(&v)?)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_object_kind_builtin_vs_custom() {
        assert_eq!(parse_object_kind("Table"), ObjectKind::Table);
        assert_eq!(parse_object_kind("table"), ObjectKind::Table);
        assert_eq!(parse_object_kind("BusinessRule"), ObjectKind::BusinessRule);
        assert_eq!(
            parse_object_kind("RustSymbol"),
            ObjectKind::Custom("RustSymbol".to_string())
        );
    }

    #[test]
    fn parse_level_rejects_unknown() {
        assert!(parse_level("object").is_ok());
        assert!(parse_level("aggregate").is_ok());
        assert!(parse_level("Object").is_err());
        assert!(parse_level("").is_err());
    }

    #[test]
    fn parse_format_rejects_unknown() {
        assert_eq!(Format::parse("json").unwrap(), Format::Json);
        assert_eq!(Format::parse("ndjson").unwrap(), Format::Ndjson);
        assert!(Format::parse("yaml").is_err());
    }

    #[test]
    fn parse_group_by_carries_depth() {
        assert_eq!(parse_group_by("kind", 2).unwrap(), GroupBy::Kind);
        assert_eq!(
            parse_group_by("path-prefix", 3).unwrap(),
            GroupBy::PathPrefix { depth: 3 }
        );
        // depth is clamped to at least 1
        assert_eq!(
            parse_group_by("path-prefix", 0).unwrap(),
            GroupBy::PathPrefix { depth: 1 }
        );
        assert!(parse_group_by("prefix", 2).is_err());
    }

    fn args() -> ExportArgs {
        ExportArgs {
            workspace: None,
            level: "object".into(),
            format: "json".into(),
            kinds: vec![],
            rel_kinds: vec![],
            exclude_rel_kinds: vec![],
            group_by: "kind".into(),
            path_prefix_depth: 2,
            max_nodes: 5000,
            max_edges: 20000,
            min_degree: 0,
            include_properties: vec![],
            as_of: None,
            first_seen: false,
            output: None,
        }
    }

    #[test]
    fn export_writes_reparseable_json_to_output_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        // A read-only open on a fresh workspace bootstraps an empty fact ledger.
        let out = dir.path().join("graph.json");
        let mut a = args();
        a.workspace = Some(dir.path().to_path_buf());
        a.output = Some(out.clone());
        export(&config, dir.path(), a).unwrap();

        let text = std::fs::read_to_string(&out).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["level"], "object");
        assert!(v["nodes"].is_array());
    }

    #[test]
    fn export_ndjson_first_line_is_the_header_record() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let out = dir.path().join("graph.ndjson");
        let mut a = args();
        a.workspace = Some(dir.path().to_path_buf());
        a.format = "ndjson".into();
        a.output = Some(out.clone());
        export(&config, dir.path(), a).unwrap();

        let text = std::fs::read_to_string(&out).unwrap();
        let first = text.lines().next().unwrap();
        let v: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(v["record"], "header");
        assert_eq!(v["schema_version"], 1);
        assert!(v.get("nodes").is_none());
    }

    #[test]
    fn export_rejects_a_non_rfc3339_as_of() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let mut a = args();
        a.workspace = Some(dir.path().to_path_buf());
        a.as_of = Some("last tuesday".into());
        let err = export(&config, dir.path(), a).unwrap_err().to_string();
        assert!(err.contains("RFC 3339"), "{err}");
    }

    #[test]
    fn export_with_as_of_and_first_seen_echoes_as_of_in_the_output() {
        let dir = tempfile::tempdir().unwrap();
        let config = EkosConfig::default();
        let out = dir.path().join("g.json");
        let mut a = args();
        a.workspace = Some(dir.path().to_path_buf());
        a.as_of = Some("2026-09-01T00:00:00Z".into());
        a.first_seen = true;
        a.output = Some(out.clone());
        export(&config, dir.path(), a).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(v["as_of"], "2026-09-01T00:00:00Z");
    }
}
