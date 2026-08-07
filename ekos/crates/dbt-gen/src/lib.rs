//! RFC 0036 — Pentaho -> dbt Model Export.
//!
//! Pure rendering over already-compiled `Custom("TransformNode")` KIR objects (RFC 0027's
//! Transformation IR, lowered by `ekos_semantic::transform_ir::lower_to_kir`): no LLM, no new
//! extraction, no expression transpilation. A `TransformNode` object plus its resolved upstream
//! model names, in — a dbt `.sql` model, out.
//!
//! `Source`/`Sink`/`Join`/`Aggregate` carry real, structured data (table names, columns, join
//! keys/kind, group-by/agg-func) and render real generated SQL. `Filter.condition`/
//! `Calculate.expr` are raw, un-parsed source-dialect text (RFC 0027 explicitly rejected a
//! cross-format expression AST as out of scope) — these render as flagged raw text, never
//! silently transpiled. `Unmapped` nodes (49% of a real Pentaho repo's steps in RFC 0035's real
//! test) render as a stub carrying the raw XML and reason as a comment. Every node still resolves
//! its upstream via a real `ref()`, so the pipeline shape stays connected end to end even where
//! content isn't fully translated — the same "Unmapped is a citizen, not a failure" contract
//! `ekos-docs-gen` already established (RFC 0035), applied to a second output format.

use ekos_kir::{KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use std::collections::HashMap;

/// The `ObjectKind::Custom(_)` label `ekos_semantic::transform_ir::lower_to_kir` gives every
/// Transformation IR node.
pub const TRANSFORM_NODE_KIND: &str = "TransformNode";

/// The `RelationshipKind::Custom(_)` label `lower_to_kir` gives every data-flow edge.
pub const FEEDS_INTO: &str = "FeedsInto";

/// Whether `kind` is a Transformation IR node this crate can render — the gate callers filter
/// `KnowledgeStore::all_objects()` with before calling [`render_dbt_model`], the same role
/// `ekos_docs_gen::is_significant` plays for the documentation renderer.
pub fn is_transform_node(kind: &ObjectKind) -> bool {
    matches!(kind, ObjectKind::Custom(s) if s.as_str() == TRANSFORM_NODE_KIND)
}

/// Whether `kind` is a Transformation IR data-flow edge.
pub fn is_feeds_into(kind: &RelationshipKind) -> bool {
    matches!(kind, RelationshipKind::Custom(s) if s.as_str() == FEEDS_INTO)
}

/// One generated dbt model file.
#[derive(Debug, Clone, PartialEq)]
pub struct DbtModelFile {
    /// Filesystem-safe file name (without directory), e.g. `fact_sales_ktr_10.sql`.
    pub file_name: String,
    /// The model name `{{ ref('...') }}` calls use — same string minus the `.sql` extension.
    pub model_name: String,
    pub sql: String,
}

/// dbt-safe snake_case slug used as both a node's model name (for `ref()`) and its `.sql` file
/// name stem — e.g. `fact_sales.ktr:10` -> `fact_sales_ktr_10`. Every `TransformNode` object's
/// name is already unique (`{source_path}:{node_index}`, see `transform_ir.rs::lower_to_kir`), so
/// no additional kind-prefixing is needed the way `ekos-docs-gen`'s multi-kind page names need.
pub fn dbt_model_name(object: &KirObject) -> String {
    slugify_snake(&object.name)
}

fn slugify_snake(name: &str) -> String {
    let mut out = String::new();
    let mut last_was_us = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_us = false;
        } else if !last_was_us {
            out.push('_');
            last_was_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Resolve `object_id`'s upstream models via `FeedsInto` inbound edges (`rel.to == object_id`),
/// in relationship order — deliberately never via a `Join` node's own `left`/`right` fields,
/// which `pentaho_analyzer.rs` documents as unreliable self-referential placeholders for the
/// Pentaho source path (see RFC 0036's "What already exists and is reused"). An upstream whose
/// `KirId` isn't in `model_names` (not a `TransformNode`, or not passed in) is silently skipped
/// rather than rendering a broken `ref()` to nothing.
pub fn upstream_model_names(
    object_id: KirId,
    relationships: &[KirRelationship],
    model_names: &HashMap<KirId, String>,
) -> Vec<String> {
    relationships
        .iter()
        .filter(|r| r.to == object_id && is_feeds_into(&r.kind))
        .filter_map(|r| model_names.get(&r.from).cloned())
        .collect()
}

type Props = HashMap<String, serde_json::Value>;

fn get_str<'a>(props: &'a Props, key: &str) -> Option<&'a str> {
    props.get(key).and_then(|v| v.as_str())
}

fn get_str_vec(props: &Props, key: &str) -> Vec<String> {
    props
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn get_pairs(props: &Props, key: &str) -> Vec<(String, String)> {
    props
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    let a = pair.first()?.as_str()?.to_string();
                    let b = pair.get(1)?.as_str()?.to_string();
                    Some((a, b))
                })
                .collect()
        })
        .unwrap_or_default()
}

struct AggExprRow {
    output: String,
    func: String,
    arg: String,
}

fn get_aggs(props: &Props) -> Vec<AggExprRow> {
    props
        .get("aggs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| {
                    let obj = v.as_object()?;
                    Some(AggExprRow {
                        output: obj.get("output")?.as_str()?.to_string(),
                        func: obj.get("func")?.as_str()?.to_string(),
                        arg: obj.get("arg")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Comments out `text` as SQL line comments, one `-- ` per source line — used for `Unmapped`'s
/// raw XML so the whole model file stays valid SQL even though most of it is a comment.
fn comment_block(text: &str) -> String {
    text.lines()
        .map(|l| format!("-- {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn no_upstream_placeholder() -> String {
    "select 1 as placeholder -- no upstream FeedsInto edge found\n".to_string()
}

/// Render one dbt `.sql` model for `object` (must be a `Custom("TransformNode")` object — callers
/// filter with [`is_transform_node`] first).
///
/// `upstream_models` are this node's upstream model names, already resolved via
/// [`upstream_model_names`] — passed in rather than recomputed here so this function stays pure
/// rendering with no relationship-walking duplicated between this crate and its caller.
///
/// `source_group` names the dbt `source:` group `Source` nodes reference (e.g. `"pentaho"`).
pub fn render_dbt_model(
    object: &KirObject,
    upstream_models: &[String],
    source_group: &str,
) -> DbtModelFile {
    let model_name = dbt_model_name(object);
    let node_type = get_str(&object.properties, "node_type").unwrap_or("Unmapped");

    let sql = match node_type {
        "Source" => render_source(&object.properties, source_group),
        "Sink" => render_sink(upstream_models),
        "Join" => render_join(&object.properties, upstream_models),
        "Aggregate" => render_aggregate(&object.properties, upstream_models),
        "Filter" => render_filter(&object.properties, upstream_models),
        "Calculate" => render_calculate(&object.properties, upstream_models),
        _ => render_unmapped(&object.properties, upstream_models),
    };

    DbtModelFile {
        file_name: format!("{model_name}.sql"),
        model_name,
        sql,
    }
}

fn render_source(props: &Props, source_group: &str) -> String {
    let object_name = get_str(props, "object_name").unwrap_or("unknown_source");
    let columns = get_str_vec(props, "columns");
    let select_list = if columns.is_empty() {
        "*".to_string()
    } else {
        columns.join(",\n    ")
    };
    format!("select\n    {select_list}\nfrom {{{{ source('{source_group}', '{object_name}') }}}}\n")
}

fn render_sink(upstream_models: &[String]) -> String {
    match upstream_models.first() {
        Some(upstream) => format!("select * from {{{{ ref('{upstream}') }}}}\n"),
        None => no_upstream_placeholder(),
    }
}

fn render_join(props: &Props, upstream_models: &[String]) -> String {
    let kind = get_str(props, "join_kind")
        .unwrap_or("Inner")
        .to_lowercase();
    let keys = get_pairs(props, "keys");
    let (a, b) = match (upstream_models.first(), upstream_models.get(1)) {
        (Some(a), Some(b)) => (a.clone(), b.clone()),
        (Some(a), None) => (a.clone(), a.clone()),
        _ => return no_upstream_placeholder(),
    };
    // Key text is rendered exactly as compiled, never prefixed with an invented `l.`/`r.` alias:
    // Pentaho's DatabaseJoin/MergeJoin/StreamLookup keys are bare column names, but SQL-sourced
    // joins (sql_transform_analyzer) already carry the original query's own table alias (e.g.
    // `o.customer_id`) — found by running this against a real compiled SQL view, not assumed.
    // Prefixing either would either leave Pentaho's bare names ambiguous or double-qualify SQL's
    // already-qualified ones, so the honest choice is to pass the compiled text through unchanged
    // and flag it for verification, matching Filter/Calculate's TODO-comment contract.
    let on_clause = if keys.is_empty() {
        "true -- no join keys compiled".to_string()
    } else {
        keys.iter()
            .map(|(l, r)| format!("{l} = {r}"))
            .collect::<Vec<_>>()
            .join("\n    and ")
    };
    let side_note = if upstream_models.len() == 2 {
        "-- NOTE: l/r assignment is positional (first two FeedsInto edges), not guaranteed to \
         match the original Kettle step's left/right\n"
    } else {
        ""
    };
    format!(
        "{side_note}select *\nfrom {{{{ ref('{a}') }}}} as l\n{kind} join {{{{ ref('{b}') }}}} as r\n    on {on_clause} -- TODO: verify column qualification, source dialect: Pentaho\n"
    )
}

fn render_aggregate(props: &Props, upstream_models: &[String]) -> String {
    let group_by = get_str_vec(props, "group_by");
    let aggs = get_aggs(props);
    let upstream = match upstream_models.first() {
        Some(u) => u.clone(),
        None => return no_upstream_placeholder(),
    };
    let mut select_items: Vec<String> = group_by.clone();
    for agg in &aggs {
        select_items.push(format!("{}({}) as {}", agg.func, agg.arg, agg.output));
    }
    let select_list = if select_items.is_empty() {
        "*".to_string()
    } else {
        select_items.join(",\n    ")
    };
    let group_by_clause = if group_by.is_empty() {
        String::new()
    } else {
        format!("\ngroup by {}", group_by.join(", "))
    };
    format!("select\n    {select_list}\nfrom {{{{ ref('{upstream}') }}}}{group_by_clause}\n")
}

fn render_filter(props: &Props, upstream_models: &[String]) -> String {
    let condition = get_str(props, "excerpt").unwrap_or("true");
    let upstream = match upstream_models.first() {
        Some(u) => u.clone(),
        None => return no_upstream_placeholder(),
    };
    format!(
        "select *\nfrom {{{{ ref('{upstream}') }}}}\nwhere {condition} -- TODO: verify, source dialect: Pentaho\n"
    )
}

fn render_calculate(props: &Props, upstream_models: &[String]) -> String {
    let output = get_str(props, "output").unwrap_or("calculated_field");
    let expr = get_str(props, "excerpt").unwrap_or("null");
    let upstream = match upstream_models.first() {
        Some(u) => u.clone(),
        None => return no_upstream_placeholder(),
    };
    format!(
        "select\n    *,\n    {expr} as {output} -- TODO: verify, source dialect: Pentaho\nfrom {{{{ ref('{upstream}') }}}}\n"
    )
}

fn render_unmapped(props: &Props, upstream_models: &[String]) -> String {
    let reason = get_str(props, "reason").unwrap_or("unrecognized");
    let raw = get_str(props, "raw").unwrap_or("");
    let mut out = format!("-- Unmapped: {reason}\n");
    if !raw.is_empty() {
        out.push_str("-- Raw source:\n");
        out.push_str(&comment_block(raw));
        out.push('\n');
    }
    match upstream_models.first() {
        Some(upstream) => {
            out.push_str(&format!(
                "select * from {{{{ ref('{upstream}') }}}} -- passthrough stub, not translated\n"
            ));
        }
        None => {
            out.push_str(
                "select 1 as placeholder -- passthrough stub, no upstream FeedsInto edge found\n",
            );
        }
    }
    out
}

/// Render a `schema.yml` describing the generated dbt project: one `sources:` entry per distinct
/// `Source` node's `object_name` (grouped under `source_group`), and one `models:` entry per
/// generated model name. Hand-rolled string building, matching `ekos-docs-gen`'s own Markdown/
/// HTML hand-rolling — no new YAML-serializer dependency for a structure this small and fixed.
pub fn render_schema_yml(
    source_group: &str,
    source_tables: &[String],
    model_names: &[String],
) -> String {
    let mut out = String::from("version: 2\n\n");

    out.push_str("sources:\n");
    if source_tables.is_empty() {
        out.push_str(&format!(
            "  - name: {source_group}\n    tables: [] # no Source nodes compiled\n"
        ));
    } else {
        out.push_str(&format!("  - name: {source_group}\n    tables:\n"));
        let mut sorted = source_tables.to_vec();
        sorted.sort();
        sorted.dedup();
        for table in &sorted {
            out.push_str(&format!("      - name: \"{table}\"\n"));
        }
    }

    out.push_str("\nmodels:\n");
    if model_names.is_empty() {
        out.push_str("  [] # no models generated\n");
    } else {
        let mut sorted = model_names.to_vec();
        sorted.sort();
        for name in &sorted {
            out.push_str(&format!("  - name: {name}\n"));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_kir::{KirObject, ObjectKind};

    fn node(name: &str, node_type: &str, extra: serde_json::Value) -> KirObject {
        let mut obj = KirObject::new(name, ObjectKind::Custom(TRANSFORM_NODE_KIND.to_string()));
        obj.properties
            .insert("node_type".to_string(), node_type.into());
        if let serde_json::Value::Object(map) = extra {
            for (k, v) in map {
                obj.properties.insert(k, v);
            }
        }
        obj
    }

    #[test]
    fn dbt_model_name_slugifies_source_path_and_index() {
        let obj = node("fact_sales.ktr:10", "Join", serde_json::json!({}));
        assert_eq!(dbt_model_name(&obj), "fact_sales_ktr_10");
    }

    #[test]
    fn is_transform_node_matches_only_the_custom_kind() {
        assert!(is_transform_node(&ObjectKind::Custom(
            "TransformNode".to_string()
        )));
        assert!(!is_transform_node(&ObjectKind::Table));
        assert!(!is_transform_node(&ObjectKind::Custom("Other".to_string())));
    }

    #[test]
    fn is_feeds_into_matches_only_the_custom_kind() {
        assert!(is_feeds_into(&RelationshipKind::Custom(
            "FeedsInto".to_string()
        )));
        assert!(!is_feeds_into(&RelationshipKind::ForeignKey));
    }

    #[test]
    fn source_node_renders_select_from_source_macro() {
        let obj = node(
            "dbo.cust_mstr",
            "Source",
            serde_json::json!({"object_name": "dbo.cust_mstr", "columns": ["id", "status"]}),
        );
        let model = render_dbt_model(&obj, &[], "pentaho");
        assert_eq!(model.model_name, "dbo_cust_mstr");
        assert_eq!(model.file_name, "dbo_cust_mstr.sql");
        assert!(
            model
                .sql
                .contains("from {{ source('pentaho', 'dbo.cust_mstr') }}")
        );
        assert!(model.sql.contains("id,\n    status"));
    }

    #[test]
    fn source_node_with_no_columns_selects_star() {
        let obj = node(
            "dbo.cust_mstr",
            "Source",
            serde_json::json!({"object_name": "dbo.cust_mstr", "columns": []}),
        );
        let model = render_dbt_model(&obj, &[], "pentaho");
        assert!(model.sql.contains("select\n    *\n"));
    }

    #[test]
    fn sink_node_selects_star_from_its_upstream_ref() {
        let obj = node(
            "gold.dim_customer",
            "Sink",
            serde_json::json!({"object_name": "gold.dim_customer", "columns": ["id"]}),
        );
        let model = render_dbt_model(&obj, &["dim_customer_ktr_4".to_string()], "pentaho");
        assert_eq!(model.sql, "select * from {{ ref('dim_customer_ktr_4') }}\n");
    }

    #[test]
    fn sink_node_with_no_upstream_renders_an_honest_placeholder_not_a_panic() {
        let obj = node(
            "gold.dim_customer",
            "Sink",
            serde_json::json!({"object_name": "gold.dim_customer", "columns": []}),
        );
        let model = render_dbt_model(&obj, &[], "pentaho");
        assert!(model.sql.contains("no upstream FeedsInto edge found"));
    }

    #[test]
    fn join_node_renders_real_keys_and_kind() {
        let obj = node(
            "fact_sales.ktr:10",
            "Join",
            serde_json::json!({
                "join_kind": "Left",
                "keys": [["customer_id", "id"]],
                "left": 0,
                "right": 0
            }),
        );
        let model = render_dbt_model(
            &obj,
            &[
                "fact_sales_ktr_0".to_string(),
                "fact_sales_ktr_14".to_string(),
            ],
            "pentaho",
        );
        assert!(model.sql.contains("left join"));
        assert!(
            model
                .sql
                .contains("from {{ ref('fact_sales_ktr_0') }} as l")
        );
        assert!(
            model
                .sql
                .contains("join {{ ref('fact_sales_ktr_14') }} as r")
        );
        assert!(model.sql.contains("customer_id = id"));
        assert!(
            model.sql.contains("NOTE: l/r assignment is positional"),
            "two-upstream join must flag positional side ambiguity honestly"
        );
    }

    /// Regression test for a real bug found by running this against a real compiled SQL view
    /// (not a hand-built fixture): `sql_transform_analyzer` produces join keys that already carry
    /// the original query's own table alias (e.g. `o.customer_id`), unlike Pentaho's bare column
    /// names. Prefixing with an invented `l.`/`r.` alias double-qualified them into invalid SQL
    /// (`l.o.customer_id`) — the key text must pass through exactly as compiled.
    #[test]
    fn join_node_never_double_qualifies_already_aliased_sql_source_keys() {
        let obj = node(
            "schema.sql#2:2",
            "Join",
            serde_json::json!({
                "join_kind": "Inner",
                "keys": [["o.customer_id", "c.id"]],
                "left": 0,
                "right": 0
            }),
        );
        let model = render_dbt_model(
            &obj,
            &["schema_sql_2_0".to_string(), "schema_sql_2_1".to_string()],
            "pentaho",
        );
        assert!(model.sql.contains("on o.customer_id = c.id"));
        assert!(
            !model.sql.contains("l.o.customer_id"),
            "must never double-qualify a key that already carries its own table alias"
        );
    }

    #[test]
    fn join_node_with_no_keys_renders_honest_true_condition() {
        let obj = node(
            "fact_sales.ktr:10",
            "Join",
            serde_json::json!({"join_kind": "Left", "keys": [], "left": 0, "right": 0}),
        );
        let model = render_dbt_model(&obj, &["a".to_string(), "b".to_string()], "pentaho");
        assert!(model.sql.contains("true -- no join keys compiled"));
    }

    #[test]
    fn aggregate_node_renders_real_group_by_and_agg_funcs() {
        let obj = node(
            "northwind.sql#13:2",
            "Aggregate",
            serde_json::json!({
                "group_by": ["region"],
                "aggs": [{"output": "total", "func": "sum", "arg": "amount"}]
            }),
        );
        let model = render_dbt_model(&obj, &["upstream_node".to_string()], "pentaho");
        assert!(model.sql.contains("region"));
        assert!(model.sql.contains("sum(amount) as total"));
        assert!(model.sql.contains("group by region"));
        assert!(model.sql.contains("from {{ ref('upstream_node') }}"));
    }

    #[test]
    fn filter_node_inlines_raw_condition_flagged_as_unverified() {
        let obj = node(
            "northwind.sql#13:1",
            "Filter",
            serde_json::json!({"excerpt": "status = 'active'"}),
        );
        let model = render_dbt_model(&obj, &["upstream_node".to_string()], "pentaho");
        assert!(
            model
                .sql
                .contains("where status = 'active' -- TODO: verify, source dialect: Pentaho")
        );
    }

    #[test]
    fn calculate_node_inlines_raw_expr_flagged_as_unverified() {
        let obj = node(
            "northwind.sql#13:3",
            "Calculate",
            serde_json::json!({"output": "full_name", "excerpt": "first_name || ' ' || last_name"}),
        );
        let model = render_dbt_model(&obj, &["upstream_node".to_string()], "pentaho");
        assert!(model.sql.contains(
            "first_name || ' ' || last_name as full_name -- TODO: verify, source dialect: Pentaho"
        ));
    }

    #[test]
    fn unmapped_node_preserves_raw_and_reason_as_comments_and_still_chains_ref() {
        let obj = node(
            "dim_customer.ktr:1",
            "Unmapped",
            serde_json::json!({
                "raw": "<step><type>Sequence</type></step>",
                "reason": "unrecognized step type: Sequence"
            }),
        );
        let model = render_dbt_model(&obj, &["dim_customer_ktr_5".to_string()], "pentaho");
        assert!(
            model
                .sql
                .contains("-- Unmapped: unrecognized step type: Sequence")
        );
        assert!(model.sql.contains("-- <step><type>Sequence</type></step>"));
        assert!(
            model
                .sql
                .contains("select * from {{ ref('dim_customer_ktr_5') }}")
        );
        assert!(model.sql.contains("passthrough stub, not translated"));
    }

    #[test]
    fn unmapped_node_with_no_upstream_still_renders_valid_sql_not_a_panic() {
        let obj = node(
            "dim_customer.ktr:1",
            "Unmapped",
            serde_json::json!({"raw": "<step/>", "reason": "unrecognized step type: X"}),
        );
        let model = render_dbt_model(&obj, &[], "pentaho");
        assert!(model.sql.contains("select 1 as placeholder"));
    }

    #[test]
    fn upstream_model_names_resolves_via_feeds_into_not_join_fields() {
        let a = node("a", "Source", serde_json::json!({}));
        let b = node("b", "Source", serde_json::json!({}));
        let join = node("j", "Join", serde_json::json!({}));

        let rel_a = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            a.id,
            join.id,
        );
        let rel_b = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            b.id,
            join.id,
        );
        // A non-FeedsInto relationship touching the same node must never leak in as an upstream.
        let unrelated = KirRelationship::new(RelationshipKind::ForeignKey, a.id, join.id);

        let mut model_names = HashMap::new();
        model_names.insert(a.id, dbt_model_name(&a));
        model_names.insert(b.id, dbt_model_name(&b));

        let upstream = upstream_model_names(join.id, &[rel_a, rel_b, unrelated], &model_names);
        assert_eq!(upstream, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn upstream_model_names_skips_edges_to_unresolved_nodes() {
        let join = node("j", "Join", serde_json::json!({}));
        let unknown_id = KirId::new();
        let rel = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            unknown_id,
            join.id,
        );

        let upstream = upstream_model_names(join.id, &[rel], &HashMap::new());
        assert!(upstream.is_empty());
    }

    #[test]
    fn feeds_into_chain_resolves_correctly_through_an_unmapped_node() {
        // source -> unmapped -> sink, mirroring the real dim_customer.ktr shape from RFC 0035's
        // real-data test: an Unmapped node sitting between two mapped nodes must not break the
        // ref() chain on either side.
        let source = node(
            "src",
            "Source",
            serde_json::json!({"object_name": "src", "columns": []}),
        );
        let unmapped = node(
            "mid",
            "Unmapped",
            serde_json::json!({"raw": "<step/>", "reason": "unrecognized"}),
        );
        let sink = node(
            "snk",
            "Sink",
            serde_json::json!({"object_name": "snk", "columns": []}),
        );

        let rel1 = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            source.id,
            unmapped.id,
        );
        let rel2 = KirRelationship::new(
            RelationshipKind::Custom("FeedsInto".to_string()),
            unmapped.id,
            sink.id,
        );
        let relationships = vec![rel1, rel2];

        let mut model_names = HashMap::new();
        model_names.insert(source.id, dbt_model_name(&source));
        model_names.insert(unmapped.id, dbt_model_name(&unmapped));
        model_names.insert(sink.id, dbt_model_name(&sink));

        let unmapped_upstream = upstream_model_names(unmapped.id, &relationships, &model_names);
        let unmapped_model = render_dbt_model(&unmapped, &unmapped_upstream, "pentaho");
        assert!(unmapped_model.sql.contains("from {{ ref('src') }}"));

        let sink_upstream = upstream_model_names(sink.id, &relationships, &model_names);
        let sink_model = render_dbt_model(&sink, &sink_upstream, "pentaho");
        assert_eq!(sink_model.sql, "select * from {{ ref('mid') }}\n");
    }

    #[test]
    fn schema_yml_lists_deduplicated_sources_and_sorted_models() {
        let yml = render_schema_yml(
            "pentaho",
            &["dbo.cust_mstr".to_string(), "dbo.cust_mstr".to_string()],
            &["b_model".to_string(), "a_model".to_string()],
        );
        assert!(yml.contains("version: 2"));
        assert!(yml.contains("- name: pentaho"));
        assert_eq!(
            yml.matches("dbo.cust_mstr").count(),
            1,
            "duplicate source tables collapse"
        );
        let a_pos = yml.find("a_model").unwrap();
        let b_pos = yml.find("b_model").unwrap();
        assert!(a_pos < b_pos, "models are sorted");
    }

    #[test]
    fn schema_yml_on_empty_input_is_honest_not_invalid_yaml() {
        let yml = render_schema_yml("pentaho", &[], &[]);
        assert!(yml.contains("no Source nodes compiled"));
        assert!(yml.contains("no models generated"));
    }
}
