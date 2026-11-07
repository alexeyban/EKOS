//! RFC 0245: Apache Ossie semantic model files in a project's sources → knowledge.
//!
//! One pass per file (`recover` finds `*.ossie.{yaml,yml,json}` and any YAML/JSON file that
//! sniffs as Ossie, and redacts it before it reaches this pass). Both spec versions are read:
//! a `0.1.1` file's every model, never only the first.
//!
//! | Ossie | KIR |
//! |---|---|
//! | model | `Custom("OssieModel")`, keyed (file, model) |
//! | dataset | `Custom("OssieDataset")`, keyed (file, model, dataset): `source`, `fields`, keys, outgoing `relationships`, synonyms |
//! | metric | `Custom("OssieMetric")`, keyed (file, model, scope, name): `expression`, `datatype`, synonyms |
//!
//! `Contains` edges run `File` → model → dataset → dataset metric, and model → model metric.
//! Nothing here touches tables: `ekos_semantic::ossie_links` links a dataset to its table at
//! `ekos commit`, by unique name only.

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_ossie::kinds::{OSSIE_DATASET, OSSIE_METRIC, OSSIE_MODEL};
use ekos_ossie::{AiContext, Dataset, Metric, SemanticModel};
use serde_json::{Value, json};
use uuid::Uuid;

/// Bump when the objects this pass writes change.
pub const LOGIC_VERSION: &str = "ossie/1";

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

/// The id of a model's object.
pub fn model_id(file_key: &str, model: &str) -> KirId {
    kid(&format!("ossie-model:{file_key}:{model}"))
}

/// The id of a dataset's object.
pub fn dataset_id(file_key: &str, model: &str, dataset: &str) -> KirId {
    kid(&format!("ossie-dataset:{file_key}:{model}:{dataset}"))
}

/// The id of a metric's object; `scope` is the dataset's name, or empty for a model metric.
pub fn metric_id(file_key: &str, model: &str, scope: &str, metric: &str) -> KirId {
    kid(&format!("ossie-metric:{file_key}:{model}:{scope}:{metric}"))
}

/// The 1-based line of the first `name: <name>` (YAML) or `"name": "<name>"` (JSON) at or after
/// line `from`, if any: where an element is declared.
fn line_of(text: &str, name: &str, from: u32) -> Option<u32> {
    let yaml = [
        format!("name: {name}"),
        format!("name: '{name}'"),
        format!("name: \"{name}\""),
    ];
    let json = format!("\"name\": \"{name}\"");
    text.lines()
        .enumerate()
        .skip(from.saturating_sub(1) as usize)
        .find(|(_, l)| {
            let t = l.trim_start().trim_start_matches("- ").trim_end();
            yaml.iter().any(|y| t == y) || l.contains(&json)
        })
        .map(|(i, _)| i as u32 + 1)
}

fn ctx_parts(c: &Option<AiContext>) -> (Option<String>, Vec<String>) {
    match c {
        None => (None, Vec::new()),
        Some(c) => (c.instructions().map(str::to_string), c.synonyms().to_vec()),
    }
}

/// Builds one file's graph.
struct Builder<'a> {
    file_key: &'a str,
    text: &'a str,
    graph: KirGraph,
}

impl Builder<'_> {
    fn evidence(&mut self, line: Option<u32>, fragment: String) -> KirId {
        let mut loc = SourceLocation::file(self.file_key);
        loc.line = line;
        self.graph.add_evidence(KirEvidence::new(loc, fragment))
    }

    fn contains(&mut self, from: KirId, to: KirId) {
        let rel = KirRelationship::deterministic(RelationshipKind::Contains, from, to, "ossie");
        self.graph.add_relationship(rel);
    }

    #[allow(clippy::too_many_arguments)]
    fn object(
        &mut self,
        id: KirId,
        name: &str,
        kind: &str,
        line: Option<u32>,
        fragment: String,
        props: Vec<(&str, Value)>,
    ) {
        let ev = self.evidence(line, fragment);
        let mut o = KirObject::new(name, ObjectKind::Custom(kind.into())).with_evidence(ev);
        o.id = id;
        o.properties.insert("path".into(), json!(self.file_key));
        if let Some(l) = line {
            o.properties.insert("line".into(), json!(l));
        }
        for (k, v) in props {
            let empty = v.is_null()
                || v.as_array().is_some_and(Vec::is_empty)
                || v.as_str().is_some_and(|s| s.trim().is_empty());
            if !empty {
                o.properties.insert(k.into(), v);
            }
        }
        self.graph.add_object(o);
    }

    fn metric(&mut self, model: &str, scope: &str, m: &Metric, parent: KirId, from: u32) {
        let id = metric_id(self.file_key, model, scope, &m.name);
        let line = line_of(self.text, &m.name, from);
        let (instructions, synonyms) = ctx_parts(&m.ai_context);
        let expression = serde_json::to_value(&m.expression).unwrap_or(Value::Null);
        self.object(
            id,
            &m.name,
            OSSIE_METRIC,
            line,
            format!(
                "Ossie metric {} = {}",
                m.name,
                m.expression.text().unwrap_or_default()
            ),
            vec![
                ("model", json!(model)),
                (
                    "scope",
                    json!(if scope.is_empty() { "model" } else { "dataset" }),
                ),
                (
                    "dataset",
                    if scope.is_empty() {
                        Value::Null
                    } else {
                        json!(scope)
                    },
                ),
                ("expression", expression),
                (
                    "datatype",
                    serde_json::to_value(m.datatype).unwrap_or(Value::Null),
                ),
                ("description", json!(m.description)),
                ("instructions", json!(instructions)),
                ("synonyms", json!(synonyms)),
            ],
        );
        self.contains(parent, id);
    }

    fn dataset(&mut self, model: &SemanticModel, d: &Dataset, parent: KirId, from: u32) -> u32 {
        let id = dataset_id(self.file_key, &model.name, &d.name);
        let line = line_of(self.text, &d.name, from);
        let (instructions, synonyms) = ctx_parts(&d.ai_context);
        let fields: Vec<Value> = d
            .fields
            .iter()
            .map(|f| {
                let (_, syn) = ctx_parts(&f.ai_context);
                let mut v = json!({"name": f.name, "is_time": f.is_time()});
                for (k, x) in [
                    ("sql", json!(f.expression.text())),
                    (
                        "datatype",
                        serde_json::to_value(f.datatype).unwrap_or(Value::Null),
                    ),
                    ("description", json!(f.description)),
                    ("label", json!(f.label)),
                ] {
                    if !x.is_null() {
                        v[k] = x;
                    }
                }
                if !syn.is_empty() {
                    v["synonyms"] = json!(syn);
                }
                v
            })
            .collect();
        let relationships: Vec<Value> = model
            .relationships
            .iter()
            .filter(|r| r.from == d.name)
            .map(|r| {
                json!({"name": r.name, "to": r.to, "from_columns": r.from_columns,
                       "to_columns": r.to_columns})
            })
            .collect();
        self.object(
            id,
            &d.name,
            OSSIE_DATASET,
            line,
            format!("Ossie dataset {} over {}", d.name, d.source),
            vec![
                ("model", json!(model.name)),
                ("source", json!(d.source)),
                ("description", json!(d.description)),
                ("instructions", json!(instructions)),
                ("synonyms", json!(synonyms)),
                ("fields", json!(fields)),
                ("primary_key", json!(d.primary_key)),
                ("unique_keys", json!(d.unique_keys)),
                ("relationships", json!(relationships)),
            ],
        );
        self.contains(parent, id);
        let at = line.unwrap_or(from);
        for m in &d.metrics {
            self.metric(&model.name, &d.name, m, id, at);
        }
        at
    }

    fn model(&mut self, m: &SemanticModel, file_id: Option<KirId>) {
        let id = model_id(self.file_key, &m.name);
        let line = line_of(self.text, &m.name, 1);
        let (instructions, synonyms) = ctx_parts(&m.ai_context);
        let metrics = m.metrics.len() + m.datasets.iter().map(|d| d.metrics.len()).sum::<usize>();
        self.object(
            id,
            &m.name,
            OSSIE_MODEL,
            line,
            format!("Ossie semantic model {} (spec {})", m.name, m.version),
            vec![
                ("version", json!(m.version)),
                ("description", json!(m.description)),
                ("instructions", json!(instructions)),
                ("synonyms", json!(synonyms)),
                ("datasets", json!(m.datasets.len())),
                ("metrics", json!(metrics)),
            ],
        );
        if let Some(f) = file_id {
            self.contains(f, id);
        }
        let mut at = line.unwrap_or(1);
        for d in &m.datasets {
            at = self.dataset(m, d, id, at);
        }
        for mm in &m.metrics {
            self.metric(&m.name, "", mm, id, at);
        }
    }
}

/// Whether a model is one `ekos export ossie` wrote: its model-level `EKOS` extension names the
/// export's status filter. EKOS's own output is never read back as a source — it would feed its
/// derived meanings and carried metrics into the next export as if someone had written them.
pub fn is_ekos_export(m: &SemanticModel) -> bool {
    ekos_ossie::ekos_data(&m.custom_extensions).is_some_and(|d| d.get("export_status").is_some())
}

/// The graph of one Ossie file, or the reason it is not one. `file_key` is the owning `File`
/// object's id key (project-qualified path). Models EKOS exported are skipped ([`is_ekos_export`]).
pub fn recover_ossie(file_key: &str, text: &str, attach_file: bool) -> Result<KirGraph, String> {
    let mut models = ekos_ossie::parse(text).map_err(|e| e.to_string())?;
    let all = models.len();
    models.retain(|m| !is_ekos_export(m));
    if models.len() < all {
        tracing::info!("{file_key}: an `ekos export ossie` file, not a source; skipped");
    }
    let mut b = Builder {
        file_key,
        text,
        graph: KirGraph::new(),
    };
    let file_id = attach_file.then(|| crate::sql_objects::file_kir_id(file_key));
    for m in &models {
        b.model(m, file_id);
    }
    Ok(b.graph)
}

/// The pass for one Ossie file.
pub struct OssieAnalyzerPass {
    pass_id: String,
    file_key: String,
    text: String,
}

impl OssieAnalyzerPass {
    /// `file_key` is the project-qualified path `build` keys the file's `File` object by;
    /// `text` is already redacted.
    pub fn new(file_key: impl Into<String>, text: impl Into<String>) -> Self {
        let file_key = file_key.into();
        Self {
            pass_id: format!("ossie-analyzer:{file_key}"),
            file_key,
            text: text.into(),
        }
    }
}

#[async_trait]
impl CompilerPass for OssieAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn version(&self) -> &str {
        LOGIC_VERSION
    }

    fn cache_inputs(&self) -> Vec<String> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(self.file_key.as_bytes());
        hasher.update(self.text.as_bytes());
        vec![hex::encode(hasher.finalize())]
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let graph = match recover_ossie(&self.file_key, &self.text, true) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(pass = %self.pass_id, "ossie-analyzer: not a readable Ossie model: {e}");
                return Ok(());
            }
        };
        if graph.objects.is_empty() {
            return Ok(());
        }
        let models = graph
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == OSSIE_MODEL))
            .count();
        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;
        tracing::info!(pass = %self.pass_id, models, "ossie-analyzer complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FLIGHTS: &str = include_str!("../../ossie/tests/fixtures/flights.semantic_model.yaml");
    const TPCDS: &str = include_str!("../../ossie/tests/fixtures/tpcds_semantic_model.yaml");
    const TPCDS_011: &str =
        include_str!("../../ossie/tests/fixtures/tpcds_semantic_model.v0_1_1.yaml");

    fn kinds(g: &KirGraph, kind: &str) -> Vec<String> {
        let mut v: Vec<String> = g
            .objects
            .iter()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == kind))
            .map(|o| o.name.clone())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn the_official_examples_become_models_datasets_and_metrics() {
        let g = recover_ossie("p/flights.yaml", FLIGHTS, true).unwrap();
        assert_eq!(kinds(&g, OSSIE_MODEL), ["Flights semantic model"]);
        assert_eq!(
            kinds(&g, OSSIE_DATASET),
            [
                "AIRCRAFT", "AIRPORT", "CARRIER", "FLIGHT", "ROUTE", "RUNWAY"
            ]
        );
        let flight = g.objects.iter().find(|o| o.name == "FLIGHT").unwrap();
        assert_eq!(flight.properties["source"], "DATABASE.SCHEMA.FLIGHTS");
        assert_eq!(flight.properties["fields"].as_array().unwrap().len(), 21);
        assert_eq!(flight.properties["line"], 98);
        // File → model → six datasets.
        assert_eq!(g.relationships.len(), 7);

        let g = recover_ossie("p/tpcds.yaml", TPCDS, true).unwrap();
        let metrics = kinds(&g, OSSIE_METRIC);
        assert!(!metrics.is_empty());
        let m = g
            .objects
            .iter()
            .find(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == OSSIE_METRIC))
            .unwrap();
        assert!(
            m.properties["expression"]["sql"].is_string()
                || m.properties["expression"]["dialects"].is_array()
        );
        assert!(!m.evidence.is_empty());
    }

    #[test]
    fn a_0_1_1_file_reads_and_ids_are_stable() {
        let a = recover_ossie("p/t.yaml", TPCDS_011, false).unwrap();
        let b = recover_ossie("p/t.yaml", TPCDS_011, false).unwrap();
        assert_eq!(kinds(&a, OSSIE_MODEL), ["tpcds_retail_model"]);
        let ids = |g: &KirGraph| {
            let mut v: Vec<String> = g.objects.iter().map(|o| o.id.to_string()).collect();
            v.extend(g.relationships.iter().map(|r| r.id.to_string()));
            v.sort();
            v
        };
        assert_eq!(ids(&a), ids(&b));
        // Same dataset name in another file is another object.
        let c = recover_ossie("q/t.yaml", TPCDS_011, false).unwrap();
        assert!(ids(&a).iter().all(|i| !ids(&c).contains(i)));
    }

    #[test]
    fn ekos_s_own_export_is_never_read_back_as_a_source() {
        let mut m = ekos_ossie::parse(FLIGHTS).unwrap().remove(0);
        m.custom_extensions = vec![ekos_ossie::CustomExtension::ekos(
            &json!({"export_status": "all", "ekos_version": "1"}),
        )];
        let g = recover_ossie("p/export.yaml", &m.to_yaml(), true).unwrap();
        assert!(g.objects.is_empty());
        // Another tool's EKOS-free file, or one whose EKOS extension is not an export's, is read.
        m.custom_extensions = vec![ekos_ossie::CustomExtension::ekos(&json!({"note": "x"}))];
        assert!(
            !recover_ossie("p/m.yaml", &m.to_yaml(), true)
                .unwrap()
                .objects
                .is_empty()
        );
    }

    #[test]
    fn a_broken_file_is_an_error_not_objects() {
        assert!(recover_ossie("x.yaml", "version: '0.2.0.dev0'\nname: m\n", true).is_err());
    }
}
