//! `ekos dbt generate` — Pentaho -> dbt model export (RFC 0036).
//!
//! Reads every already-committed `Custom("TransformNode")` object from the ledger (RFC 0027's
//! Transformation IR), resolves each node's upstream model via real `FeedsInto` relationships,
//! and renders one dbt `.sql` model per node plus a `schema.yml` — pure rendering over compiled
//! data, zero LLM calls, zero new extraction, same "post-commit CLI verb" shape as
//! `ekos docs generate` (RFC 0035).
//!
//! Structured node types (`Source`/`Sink`/`Join`/`Aggregate`) render real generated SQL from
//! real compiled table/column/join-key/group-by data. `Filter`/`Calculate` inline their raw
//! source-dialect expression text behind a `-- TODO: verify` comment rather than attempting a
//! transpile RFC 0027 already scoped out. `Unmapped` nodes render a stub carrying the raw source
//! and reason as a comment. Every model still resolves its upstream via a real `ref()`, so the
//! generated project's shape is always correct even where content isn't fully translated.

use super::store::open_store;
use anyhow::{Context, Result};
use ekos_dbt_gen::{DbtModelFile, is_transform_node, upstream_model_names};
use ekos_kir::{KirId, KirObject};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Grouping name for `{{ source(...) }}` calls in generated models. Fixed for now — RFC 0036's
/// Open Questions leave per-workspace naming for a later phase.
const SOURCE_GROUP: &str = "pentaho";

pub async fn generate(
    cwd: &Path,
    output: &Path,
    config: &ekos_compiler_core::EkosConfig,
) -> Result<()> {
    let ledger = open_store(config, cwd).map_err(|e| {
        anyhow::anyhow!(
            "{e}\nRun `ekos build && ekos recover && ekos resolve && ekos compile && ekos commit` first."
        )
    })?;

    let objects = ledger.all_objects()?;
    let nodes: Vec<&KirObject> = objects
        .iter()
        .filter(|o| is_transform_node(&o.kind))
        .collect();

    let model_names: HashMap<KirId, String> = nodes
        .iter()
        .map(|o| (o.id, ekos_dbt_gen::dbt_model_name(o)))
        .collect();

    std::fs::create_dir_all(output)
        .with_context(|| format!("cannot create output dir {}", output.display()))?;

    let mut written_models: Vec<DbtModelFile> = Vec::with_capacity(nodes.len());
    let mut source_tables: Vec<String> = Vec::new();

    for object in &nodes {
        let relationships = ledger.relationships_for(&object.id)?;
        let upstream = upstream_model_names(object.id, &relationships, &model_names);
        let model = ekos_dbt_gen::render_dbt_model(object, &upstream, SOURCE_GROUP);

        if object.properties.get("node_type").and_then(|v| v.as_str()) == Some("Source")
            && let Some(name) = object
                .properties
                .get("object_name")
                .and_then(|v| v.as_str())
        {
            source_tables.push(name.to_string());
        }

        write_model(output, &model)?;
        written_models.push(model);
    }

    let model_name_list: Vec<String> = written_models
        .iter()
        .map(|m| m.model_name.clone())
        .collect();
    let schema_yml =
        ekos_dbt_gen::render_schema_yml(SOURCE_GROUP, &source_tables, &model_name_list);
    std::fs::write(output.join("schema.yml"), &schema_yml)
        .with_context(|| format!("cannot write {}", output.join("schema.yml").display()))?;

    println!("dbt models generated.");
    println!("  Models rendered: {}", written_models.len());
    println!(
        "  Source tables: {}",
        source_tables
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
    );
    println!("  Output: {}", output.display());
    if written_models.is_empty() {
        println!(
            "  (No Custom(\"TransformNode\") objects found in the ledger — nothing to render yet.)"
        );
    }

    Ok(())
}

fn write_model(output: &Path, model: &DbtModelFile) -> Result<()> {
    let path = output.join(&model.file_name);
    std::fs::write(&path, &model.sql).with_context(|| format!("cannot write {}", path.display()))
}

/// Resolve the output directory: the `--output` flag if given, else `<cwd>/dbt-generated`.
pub fn resolve_output_dir(cwd: &Path, output: Option<PathBuf>) -> PathBuf {
    output.unwrap_or_else(|| cwd.join("dbt-generated"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_compiler_core::EkosConfig;
    use ekos_kir::{KirObject, KirRelationship, ObjectKind, RelationshipKind};
    use ekos_ledger::Ledger;
    use tempfile::tempdir;

    fn transform_node(name: &str, node_type: &str, extra: serde_json::Value) -> KirObject {
        let mut obj = KirObject::new(name, ObjectKind::Custom("TransformNode".to_string()));
        obj.properties
            .insert("node_type".to_string(), node_type.into());
        if let serde_json::Value::Object(map) = extra {
            for (k, v) in map {
                obj.properties.insert(k, v);
            }
        }
        obj
    }

    #[tokio::test]
    async fn generate_renders_one_sql_file_per_transform_node() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let source = transform_node(
            "src.ktr:0",
            "Source",
            serde_json::json!({"object_name": "dbo.cust_mstr", "columns": ["id"]}),
        );
        let sink = transform_node(
            "src.ktr:1",
            "Sink",
            serde_json::json!({"object_name": "gold.dim_customer", "columns": ["id"]}),
        );
        ledger.append_object(&source).unwrap();
        ledger.append_object(&sink).unwrap();
        let rel = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            source.id,
            sink.id,
        );
        ledger.append_relationship(&rel).unwrap();

        let output = dir.path().join("out");
        generate(dir.path(), &output, &config).await.unwrap();

        assert!(output.join("src_ktr_0.sql").exists());
        assert!(output.join("src_ktr_1.sql").exists());
        let sink_sql = std::fs::read_to_string(output.join("src_ktr_1.sql")).unwrap();
        assert_eq!(sink_sql, "select * from {{ ref('src_ktr_0') }}\n");
    }

    #[tokio::test]
    async fn generate_writes_a_schema_yml_listing_source_tables_and_models() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();

        let source = transform_node(
            "src.ktr:0",
            "Source",
            serde_json::json!({"object_name": "dbo.cust_mstr", "columns": []}),
        );
        ledger.append_object(&source).unwrap();

        let output = dir.path().join("out");
        generate(dir.path(), &output, &config).await.unwrap();

        let schema = std::fs::read_to_string(output.join("schema.yml")).unwrap();
        assert!(schema.contains("dbo.cust_mstr"));
        assert!(schema.contains("src_ktr_0"));
    }

    #[tokio::test]
    async fn generate_skips_non_transform_node_objects() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        ledger
            .append_object(&KirObject::new("customers", ObjectKind::Table))
            .unwrap();

        let output = dir.path().join("out");
        generate(dir.path(), &output, &config).await.unwrap();

        let entries: Vec<_> = std::fs::read_dir(&output).unwrap().collect();
        // Only schema.yml, no .sql files, since no TransformNode objects exist.
        assert_eq!(entries.len(), 1);
        assert!(output.join("schema.yml").exists());
    }

    #[tokio::test]
    async fn generate_on_unmapped_node_produces_valid_sql_stub_not_a_panic() {
        let dir = tempdir().unwrap();
        let config = EkosConfig::default();
        let ledger = Ledger::open(&config.ledger_path(dir.path())).unwrap();
        let unmapped = transform_node(
            "job.ktr:2",
            "Unmapped",
            serde_json::json!({"raw": "<step/>", "reason": "unrecognized step type: Sequence"}),
        );
        ledger.append_object(&unmapped).unwrap();

        let output = dir.path().join("out");
        generate(dir.path(), &output, &config).await.unwrap();

        let sql = std::fs::read_to_string(output.join("job_ktr_2.sql")).unwrap();
        assert!(sql.contains("-- Unmapped: unrecognized step type: Sequence"));
        assert!(sql.contains("select 1 as placeholder"));
    }

    #[test]
    fn resolve_output_dir_defaults_to_dbt_generated() {
        let cwd = Path::new("/workspace");
        assert_eq!(
            resolve_output_dir(cwd, None),
            Path::new("/workspace/dbt-generated")
        );
        assert_eq!(
            resolve_output_dir(cwd, Some(PathBuf::from("/custom"))),
            Path::new("/custom")
        );
    }
}
