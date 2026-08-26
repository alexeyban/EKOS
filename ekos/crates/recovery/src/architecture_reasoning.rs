//! `ArchitectureReasoningPass` — RFC 0065 Phase 2. Reads the `Custom("Crate")` objects and
//! `DependsOn` edges `CrateTopologyAnalyzerPass` (RFC 0042) already produced, and puts a real
//! semantic classification question to an LLM: what role does each crate play in the workspace?
//!
//! Follows `document_semantics_analyzer.rs`'s exact shape (RFC 0026) — the working precedent for
//! RFC 0065 §46's "LLM Output Contract": one strict-JSON-schema prompt via `LlmProvider`, response
//! validated against the real object set before anything is written, output landing as new
//! evidence-linked `KirObject`s, never a direct mutation of existing state. Per RFC 0065 §4.5
//! ("Deterministic Analysis Before LLM Reasoning"), the LLM is given signals a deterministic pass
//! already computed for free (dependency fan-in/fan-out) rather than asked to derive them itself.
//!
//! Batched calls of up to `MAX_CRATES_PER_CALL` crates each, not one call per crate (RFC 0065
//! §42's own cost table: "entity classification: local LLM", but nothing requires paying for N
//! round trips when one prompt can carry many crates' worth of context) — but also not one call
//! for the *entire* crate set regardless of size: found live against a real small local model
//! that a single oversized prompt can exceed its context window and silently degrade to free-text
//! prose instead of the requested JSON. See `MAX_CRATES_PER_CALL`'s own doc comment.
//!
//! Opt-in via `[architecture-reasoning] enabled = true` — it makes one real LLM call per `recover`
//! run.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, ObjectKind, RelationshipKind, SourceLocation,
};
use uuid::Uuid;

use crate::llm::{LlmProvider, LlmRequest};
use crate::llm_json::strip_json_fences;

const SYSTEM_PROMPT: &str = r#"You are a software architecture reviewer. Given a list of crates in
a Rust workspace — each with its name, manifest path, its own description, and how many other
crates depend on it (fan-in) vs. how many it depends on (fan-out) — classify each crate's
architectural role and give one sentence explaining why.

Respond ONLY with valid JSON in this exact schema — no markdown fences, no commentary:
{
  "crates": [{"name": "<crate name, copied exactly from the input>", "role": "<short role label, e.g. 'core library', 'CLI entry point', 'plugin/connector', 'test support', 'shared utility'>", "reason": "<one sentence>"}]
}

Only include crates from the input. Do not invent crates not in the input."#;

const PROMPT_VERSION: &str = "arch-reasoning-v1";

/// Crates per LLM call. Found live against a real small local model (qwen2.5:1.5b, 4096-token
/// context): batching every crate into one call risked exceeding the model's context window and
/// silently truncating the system prompt. Conservative enough to leave real headroom even with a
/// `doc_comment` appended per entry on a targeted re-run, while still batching well below
/// one-call-per-crate.
const MAX_CRATES_PER_CALL: usize = 12;

/// Deterministic id for a `Custom("Claim")` role-classification object (RFC 0065 Phase 2), keyed
/// by the crate's own manifest directory — the same claim re-derived on a re-run resolves to the
/// same object, not a duplicate (RFC 0066 §52 idempotency). `pub` (not `pub(crate)`) because
/// `architecture_drift.rs`'s `detect_role_drift` (RFC 0068 §31-32) needs this exact id to look up
/// a role claim's version history — the two must always agree on the same id scheme.
pub fn role_claim_kir_id(crate_dir: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("claim:role:{crate_dir}").as_bytes(),
    ))
}

#[derive(Debug, serde::Deserialize)]
struct LlmOutput {
    #[serde(default)]
    crates: Vec<LlmCrateRole>,
}

#[derive(Debug, serde::Deserialize)]
struct LlmCrateRole {
    name: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    reason: String,
}

/// One crate as this pass sees it: identity plus deterministic signal already computed from the
/// upstream `CrateTopologyAnalyzerPass` output, plus optional extra context from a targeted
/// re-collection round (RFC 0065 §36 — a crate's own leading `//!` doc comment, read by
/// `ekos architecture investigate`'s `crate_doc_comment_collector` after a first pass left this
/// crate unclassified).
struct CrateInput {
    id: KirId,
    dir: String,
    name: String,
    description: String,
    fan_in: usize,
    fan_out: usize,
    extra_context: Option<String>,
}

/// Counts from one run, readable by the caller after the pass has been consumed by the
/// `PassManager`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ArchitectureReasoningStats {
    pub crates_considered: usize,
    pub roles_assigned: usize,
    pub rejected_unknown_crate: usize,
}

pub struct ArchitectureReasoningPass {
    pass_id: String,
    crate_topology_pass_id: String,
    deps: Vec<&'static str>,
    llm: Arc<dyn LlmProvider>,
    /// Extra per-crate context from a targeted re-collection round, keyed by manifest directory
    /// (RFC 0065 §36). Empty on a first, broad pass.
    crate_context: HashMap<String, String>,
    /// When set, only these crate directories are considered — a targeted re-run (RFC 0066
    /// §9 "Targeted collection") instead of re-classifying every crate again.
    only_dirs: Option<Vec<String>>,
    stats: Arc<Mutex<ArchitectureReasoningStats>>,
}

impl ArchitectureReasoningPass {
    pub fn new(crate_topology_pass_id: impl Into<String>, llm: Arc<dyn LlmProvider>) -> Self {
        let crate_topology_pass_id = crate_topology_pass_id.into();
        let dep: &'static str = Box::leak(crate_topology_pass_id.clone().into_boxed_str());
        Self {
            pass_id: format!("architecture-reasoning:{crate_topology_pass_id}"),
            crate_topology_pass_id,
            deps: vec![dep],
            llm,
            crate_context: HashMap::new(),
            only_dirs: None,
            stats: Arc::new(Mutex::new(ArchitectureReasoningStats::default())),
        }
    }

    /// RFC 0065 §36's targeted re-collection: extra context (a crate's own doc comment) for a
    /// crate the previous iteration left unclassified, keyed by manifest directory.
    pub fn with_crate_context(mut self, context: HashMap<String, String>) -> Self {
        self.crate_context = context;
        self
    }

    /// Restrict this run to only the named crate directories — used for a targeted re-run rather
    /// than re-classifying (and re-spending LLM budget on) crates already classified.
    pub fn with_only_dirs(mut self, dirs: Vec<String>) -> Self {
        self.only_dirs = Some(dirs);
        self
    }

    pub fn stats_handle(&self) -> Arc<Mutex<ArchitectureReasoningStats>> {
        Arc::clone(&self.stats)
    }

    /// Locate this pass's input by scanning the artifact store for the `KnowledgeArtifact` tagged
    /// with the `CrateTopologyAnalyzerPass` instance this pass depends on — same lookup shape
    /// `document_semantics_analyzer.rs::collect_sections` uses for its own upstream pass.
    fn collect_crates(&self, ctx: &PassContext) -> Vec<CrateInput> {
        let ids = match ctx.artifact_store.list() {
            Ok(ids) => ids,
            Err(e) => {
                ctx.diagnostics
                    .lock()
                    .unwrap()
                    .warning("ARCHREASON001", format!("cannot list artifact store: {e}"));
                return Vec::new();
            }
        };

        // The artifact store is content-addressed and additive (RFC 0015): every past *uncached*
        // `recover` run left its own `crate-topology-analyzer` `KnowledgeArtifact` behind, all
        // sharing this `pass_name`, none superseding the others on disk. `Crate`/`Technology`
        // object ids are deterministic (keyed by manifest dir / name), so the same real crate
        // reappears with the *same* `KirId` across every one of those historical copies —
        // deduped below by id. Relationship ids are **not** deterministic (`KirRelationship::new`
        // mints a fresh random id every run), so those are deduped by `(from, to, kind)` instead;
        // without this, fan-in/fan-out would double- (or quadruple-, ...) count every real edge
        // once per historical run this pass name has ever produced.
        let mut objects_by_id: HashMap<KirId, KirObject> = HashMap::new();
        let mut relationships_seen: std::collections::HashSet<(KirId, KirId, String)> =
            std::collections::HashSet::new();
        let mut fan_in: HashMap<KirId, usize> = HashMap::new();
        let mut fan_out: HashMap<KirId, usize> = HashMap::new();

        for id in ids {
            let Ok(Some(json)) = ctx.artifact_store.read(&id) else {
                continue;
            };
            if json.get("pass_name").and_then(|v| v.as_str()) != Some(&self.crate_topology_pass_id)
            {
                continue;
            }
            let Ok(artifact) = serde_json::from_value::<ekos_artifact::KnowledgeArtifact>(json)
            else {
                ctx.diagnostics.lock().unwrap().warning(
                    "ARCHREASON002",
                    format!("malformed KnowledgeArtifact {id} from {}", self.pass_id),
                );
                continue;
            };
            for obj in artifact.content.kir.objects {
                objects_by_id.insert(obj.id, obj);
            }
            for rel in artifact.content.kir.relationships {
                if rel.kind != RelationshipKind::DependsOn {
                    continue;
                }
                let key = (rel.from, rel.to, format!("{:?}", rel.kind));
                if relationships_seen.insert(key) {
                    *fan_out.entry(rel.from).or_insert(0) += 1;
                    *fan_in.entry(rel.to).or_insert(0) += 1;
                }
            }
        }

        let crates: Vec<&KirObject> = objects_by_id
            .values()
            .filter(|o| matches!(&o.kind, ObjectKind::Custom(s) if s == "Crate"))
            .collect();

        let mut inputs: Vec<CrateInput> = crates
            .into_iter()
            .filter_map(|c| {
                let dir = c.properties.get("path")?.as_str()?.to_string();
                if let Some(only) = &self.only_dirs
                    && !only.contains(&dir)
                {
                    return None;
                }
                Some(CrateInput {
                    id: c.id,
                    extra_context: self.crate_context.get(&dir).cloned(),
                    dir,
                    name: c.name.clone(),
                    description: c
                        .properties
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    fan_in: fan_in.get(&c.id).copied().unwrap_or(0),
                    fan_out: fan_out.get(&c.id).copied().unwrap_or(0),
                })
            })
            .collect();

        inputs.sort_by(|a, b| a.dir.cmp(&b.dir));
        inputs
    }
}

/// Renders the batched user-message prompt: one line per crate, deterministic signal included so
/// the LLM isn't asked to guess at something already known.
fn build_prompt(crates: &[CrateInput]) -> String {
    let entries: Vec<serde_json::Value> = crates
        .iter()
        .map(|c| {
            let mut entry = serde_json::json!({
                "name": c.name,
                "path": c.dir,
                "description": c.description,
                "fan_in": c.fan_in,
                "fan_out": c.fan_out,
            });
            if let Some(ctx) = &c.extra_context {
                entry["doc_comment"] = serde_json::json!(ctx);
            }
            entry
        })
        .collect();
    serde_json::json!({ "crates": entries }).to_string()
}

#[async_trait]
impl CompilerPass for ArchitectureReasoningPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn dependencies(&self) -> &[&str] {
        &self.deps
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let crates = self.collect_crates(ctx);
        let mut stats = ArchitectureReasoningStats {
            crates_considered: crates.len(),
            ..Default::default()
        };

        if crates.is_empty() {
            *self.stats.lock().unwrap() = stats;
            return Ok(());
        }

        // Chunked rather than one giant call for the whole crate set: found live against a real
        // small local model (qwen2.5:1.5b, 4096-token context) — a single prompt covering every
        // crate (plus each one's `doc_comment` context on a targeted re-run) can exceed the
        // model's context window, silently truncating the system prompt's JSON-schema
        // instructions and producing free-text prose instead of the requested JSON. Still far
        // fewer calls than one-per-crate (RFC 0065 §42's cost discipline), just bounded per call.
        let by_name: HashMap<&str, &CrateInput> =
            crates.iter().map(|c| (c.name.as_str(), c)).collect();
        let mut graph = KirGraph::new();

        for chunk in crates.chunks(MAX_CRATES_PER_CALL) {
            let user_message = build_prompt(chunk);
            let req = LlmRequest {
                system: SYSTEM_PROMPT,
                user: &user_message,
                prompt_version: PROMPT_VERSION,
                max_tokens: 4096,
                history: &[],
            };

            let resp = match self.llm.complete(&req).await {
                Ok(resp) => resp,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "ARCHREASON003",
                        format!("LLM call failed for architecture reasoning (skipped): {e}"),
                    );
                    continue;
                }
            };

            let output: LlmOutput = match serde_json::from_str(strip_json_fences(&resp.content)) {
                Ok(o) => o,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "ARCHREASON004",
                        format!(
                            "LLM response parse failed for architecture reasoning (skipped): {e}"
                        ),
                    );
                    continue;
                }
            };

            for role in &output.crates {
                let Some(&crate_input) = by_name.get(role.name.as_str()) else {
                    // RFC 0065 §46's schema-validation step: never write a claim about a crate the
                    // LLM named that isn't actually in the real input — a hallucinated or
                    // malformed name, rejected rather than trusted.
                    stats.rejected_unknown_crate += 1;
                    ctx.diagnostics.lock().unwrap().warning(
                        "ARCHREASON005",
                        format!(
                            "LLM named a crate not present in the real input (rejected): '{}'",
                            role.name
                        ),
                    );
                    continue;
                };
                if role.role.trim().is_empty() {
                    continue;
                }

                let ev = KirEvidence::new(
                    SourceLocation::file(format!("{}/Cargo.toml", crate_input.dir)),
                    format!(
                        "'{}' classified as '{}' by {PROMPT_VERSION}: {}",
                        crate_input.name, role.role, role.reason
                    ),
                );
                let ev_id = graph.add_evidence(ev);

                let mut claim = KirObject::new(
                    format!("{} has_role {}", crate_input.name, role.role),
                    ObjectKind::Custom("Claim".to_string()),
                )
                .with_property("subject_id", serde_json::json!(crate_input.id.to_string()))
                .with_property("predicate", serde_json::json!("has_role"))
                .with_property("value", serde_json::json!(role.role))
                .with_property("claim_type", serde_json::json!("inference"))
                .with_property("reason", serde_json::json!(role.reason))
                .with_evidence(ev_id);
                claim.id = role_claim_kir_id(&crate_input.dir);
                graph.objects.push(claim);
                stats.roles_assigned += 1;
            }
        }

        *self.stats.lock().unwrap() = stats;

        if graph.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], graph);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            crates_considered = stats.crates_considered,
            roles_assigned = stats.roles_assigned,
            rejected_unknown_crate = stats.rejected_unknown_crate,
            "architecture-reasoning complete"
        );
        Ok(())
    }
}

/// RFC 0065 §36's one concrete targeted collector: a crate's entry file's leading `//!` module
/// doc comment, read directly (not through an `Observer`/artifact round-trip — same direct-file-
/// read shape `crate_topology_analyzer`'s own manifest collection already uses, per CLAUDE.md's
/// `common` crate-map entry on this pattern). Tries `src/lib.rs` then `src/main.rs`; `None` if
/// neither exists or neither has a leading doc comment — never fabricated.
pub fn read_crate_doc_comment(workspace_root: &std::path::Path, crate_dir: &str) -> Option<String> {
    for entry_file in ["src/lib.rs", "src/main.rs"] {
        let path = workspace_root.join(crate_dir).join(entry_file);
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(file) = syn::parse_file(&source) else {
            continue;
        };
        let doc_lines: Vec<String> = file
            .attrs
            .iter()
            .filter_map(|attr| {
                if !attr.path().is_ident("doc") {
                    return None;
                }
                let syn::Meta::NameValue(nv) = &attr.meta else {
                    return None;
                };
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                else {
                    return None;
                };
                Some(s.value().trim().to_string())
            })
            .collect();
        if !doc_lines.is_empty() {
            return Some(doc_lines.join(" "));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crate_topology_analyzer::CrateTopologyAnalyzerPass;
    use crate::llm::{LlmError, LlmResponse, MockLlmProvider};
    use ekos_compiler_core::{EkosConfig, pass::PassContext};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const CRATE_TOPOLOGY_PASS_ID: &str = "crate-topology-analyzer:test";

    /// Counts real `complete()` calls, always returning an empty (but valid) classification —
    /// only the call count matters to `chunks_more_than_max_crates_per_call_into_multiple_calls`.
    struct CountingLlmProvider {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl LlmProvider for CountingLlmProvider {
        fn model_name(&self) -> &str {
            "counting-mock"
        }

        async fn complete(&self, _req: &LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(LlmResponse {
                content: serde_json::json!({ "crates": [] }).to_string(),
                model: "counting-mock".to_string(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    fn ctx() -> (PassContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        (
            PassContext::new(Arc::new(EkosConfig::default()), dir.path().to_path_buf()),
            dir,
        )
    }

    const ROOT: &str = r#"
[workspace]
members = ["crates/kir", "crates/consumer"]
"#;

    const KIR: &str = r#"
[package]
name = "ekos-kir"
version = "0.1.0"
description = "Knowledge IR types"
"#;

    const CONSUMER: &str = r#"
[package]
name = "ekos-consumer"
version = "0.1.0"

[dependencies]
ekos-kir = { path = "../kir" }
"#;

    async fn seed_crates(ctx: &PassContext, manifests: Vec<(&str, &str)>) {
        let manifests = manifests
            .into_iter()
            .map(|(p, s)| (p.to_string(), s.to_string(), String::new()))
            .collect();
        let mut pass = CrateTopologyAnalyzerPass::new("test", manifests);
        let mut c2 = ctx.clone();
        pass.run(&mut c2).await.unwrap();
    }

    fn read_output(ctx: &PassContext, pass_id: &str) -> Option<KirGraph> {
        let id = ctx.artifact_store.list().unwrap().into_iter().find(|id| {
            ctx.artifact_store
                .read(id)
                .unwrap()
                .and_then(|j| j.get("pass_name")?.as_str().map(|s| s == pass_id))
                .unwrap_or(false)
        })?;
        let json = ctx.artifact_store.read(&id).unwrap().unwrap();
        let artifact: ekos_artifact::KnowledgeArtifact = serde_json::from_value(json).unwrap();
        Some(artifact.content.kir)
    }

    fn role_response() -> String {
        serde_json::json!({
            "crates": [
                {"name": "ekos-kir", "role": "core library", "reason": "Depended on by other crates, no dependencies of its own."},
                {"name": "ekos-consumer", "role": "CLI entry point", "reason": "Depends on ekos-kir, nothing depends on it."}
            ]
        })
        .to_string()
    }

    #[tokio::test]
    async fn classifies_real_crates_with_evidence_and_deterministic_signals() {
        let (c, _dir) = ctx();
        seed_crates(
            &c,
            vec![
                ("Cargo.toml", ROOT),
                ("crates/kir/Cargo.toml", KIR),
                ("crates/consumer/Cargo.toml", CONSUMER),
            ],
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new(role_response()));
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, mock);
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert!(!c.diagnostics.lock().unwrap().has_errors());
        let graph = read_output(&c, &pass_id).expect("pass must write a KnowledgeArtifact");

        let claims: Vec<_> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Claim".into()))
            .collect();
        assert_eq!(claims.len(), 2);
        assert!(claims.iter().all(|c| !c.evidence.is_empty()));

        let kir_claim = claims
            .iter()
            .find(|c| c.name == "ekos-kir has_role core library")
            .expect("a role claim for ekos-kir");
        assert_eq!(
            kir_claim.properties["value"],
            serde_json::json!("core library")
        );
        assert_eq!(
            kir_claim.properties["claim_type"],
            serde_json::json!("inference")
        );

        let stats = *stats.lock().unwrap();
        assert_eq!(stats.crates_considered, 2);
        assert_eq!(stats.roles_assigned, 2);
        assert_eq!(stats.rejected_unknown_crate, 0);
    }

    #[tokio::test]
    async fn duplicate_historical_crate_topology_artifacts_are_deduplicated() {
        // Real bug, found live: the artifact store is content-addressed and additive (RFC 0015)
        // — every past *uncached* `recover` run leaves its own crate-topology-analyzer
        // KnowledgeArtifact behind, none superseding the others. Seeding twice reproduces that:
        // two separate artifacts, same real crates (deterministic ids), but a fresh random
        // relationship id each time (`KirRelationship::new` never got an explicit id override).
        let (c, _dir) = ctx();
        let manifests = vec![
            ("Cargo.toml", ROOT),
            ("crates/kir/Cargo.toml", KIR),
            ("crates/consumer/Cargo.toml", CONSUMER),
        ];
        seed_crates(&c, manifests.clone()).await;
        seed_crates(&c, manifests).await;

        let mock = Arc::new(MockLlmProvider::new(role_response()));
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, mock);
        let stats = pass.stats_handle();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        let s = *stats.lock().unwrap();
        assert_eq!(
            s.crates_considered, 2,
            "two real crates, regardless of how many historical artifacts describe them"
        );
    }

    #[tokio::test]
    async fn chunks_more_than_max_crates_per_call_into_multiple_calls() {
        // Real fix, found live: a single prompt covering every crate can exceed a small local
        // model's context window. `MAX_CRATES_PER_CALL` + 1 real crates must produce at least 2
        // LLM calls, not 1.
        let (c, _dir) = ctx();
        let mut root = String::from("[workspace]\nmembers = [");
        let mut manifests: Vec<(String, String)> = Vec::new();
        let n = MAX_CRATES_PER_CALL + 1;
        for i in 0..n {
            root.push_str(&format!("\"crates/c{i}\","));
            manifests.push((
                format!("crates/c{i}/Cargo.toml"),
                format!("[package]\nname = \"c{i}\"\nversion = \"0.1.0\"\n"),
            ));
        }
        root.push_str("]\n");
        manifests.insert(0, ("Cargo.toml".to_string(), root));

        seed_crates(
            &c,
            manifests
                .iter()
                .map(|(p, s)| (p.as_str(), s.as_str()))
                .collect(),
        )
        .await;

        let counting = Arc::new(CountingLlmProvider {
            calls: AtomicUsize::new(0),
        });
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, counting.clone());
        let stats = pass.stats_handle();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert_eq!(stats.lock().unwrap().crates_considered, n);
        assert!(
            counting.calls.load(Ordering::SeqCst) >= 2,
            "expected at least 2 LLM calls for {n} crates with MAX_CRATES_PER_CALL={MAX_CRATES_PER_CALL}, got {}",
            counting.calls.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn hallucinated_crate_name_is_rejected_not_written() {
        let (c, _dir) = ctx();
        seed_crates(&c, vec![("crates/kir/Cargo.toml", KIR)]).await;

        let resp = serde_json::json!({
            "crates": [
                {"name": "ekos-kir", "role": "core library", "reason": "r"},
                {"name": "totally-made-up-crate", "role": "core library", "reason": "r"}
            ]
        })
        .to_string();
        let mock = Arc::new(MockLlmProvider::new(resp));
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, mock);
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert!(
            c.diagnostics.lock().unwrap().has_warnings(),
            "a hallucinated crate name must produce a warning"
        );
        let graph = read_output(&c, &pass_id).unwrap();
        let claims: Vec<_> = graph
            .objects
            .iter()
            .filter(|o| o.kind == ObjectKind::Custom("Claim".into()))
            .collect();
        assert_eq!(claims.len(), 1, "only the real crate gets a claim written");

        let stats = *stats.lock().unwrap();
        assert_eq!(stats.rejected_unknown_crate, 1);
    }

    #[tokio::test]
    async fn pass_tolerates_bad_llm_json() {
        let (c, _dir) = ctx();
        seed_crates(&c, vec![("crates/kir/Cargo.toml", KIR)]).await;

        let mock = Arc::new(MockLlmProvider::new("not valid json at all!!"));
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, mock);
        let stats = pass.stats_handle();
        let pass_id = pass.name().to_string();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert!(!c.diagnostics.lock().unwrap().has_errors());
        assert!(c.diagnostics.lock().unwrap().has_warnings());
        assert_eq!(stats.lock().unwrap().roles_assigned, 0);
        assert!(read_output(&c, &pass_id).is_none());
    }

    #[tokio::test]
    async fn with_only_dirs_restricts_to_the_named_crates() {
        let (c, _dir) = ctx();
        seed_crates(
            &c,
            vec![
                ("Cargo.toml", ROOT),
                ("crates/kir/Cargo.toml", KIR),
                ("crates/consumer/Cargo.toml", CONSUMER),
            ],
        )
        .await;

        let mock = Arc::new(MockLlmProvider::new(role_response()));
        let mut pass = ArchitectureReasoningPass::new(CRATE_TOPOLOGY_PASS_ID, mock)
            .with_only_dirs(vec!["crates/kir".to_string()]);
        let stats = pass.stats_handle();
        let mut c2 = c.clone();
        pass.run(&mut c2).await.unwrap();

        assert_eq!(stats.lock().unwrap().crates_considered, 1);
    }

    #[test]
    fn read_crate_doc_comment_reads_a_real_leading_module_comment() {
        let dir = tempfile::tempdir().unwrap();
        let crate_dir = dir.path().join("crates/example");
        std::fs::create_dir_all(crate_dir.join("src")).unwrap();
        std::fs::write(
            crate_dir.join("src/lib.rs"),
            "//! Real example crate for testing doc-comment extraction.\n//! Second line.\n\npub fn f() {}\n",
        )
        .unwrap();

        let comment = read_crate_doc_comment(dir.path(), "crates/example")
            .expect("a doc comment must be found");
        assert!(comment.contains("Real example crate"));
        assert!(comment.contains("Second line"));
    }

    #[test]
    fn read_crate_doc_comment_is_none_when_no_entry_file_exists() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_crate_doc_comment(dir.path(), "crates/nonexistent").is_none());
    }
}
