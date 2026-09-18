//! `BinaryAnalyzerPass` — RFC 0148 Phase 2: `DecompiledAst` into KIR and Transformation IR.
//!
//! Wholly deterministic and LLM-free, like every other analyzer in this crate. It reads the
//! `DecompiledAst` JSON `plugins/binary` produced and emits:
//!
//! - `Custom("BinaryAssembly")` — one per compiled file, with its real SHA-256 and format version.
//! - `Custom("BinaryType")` / `("BinaryMethod")` / `("BinaryField")` — the declared structure.
//! - `Custom("ExternalIoBoundary")` — one per call site that leaves the process for a database,
//!   HTTP endpoint, file, message queue or subprocess.
//! - `Contains` / `Extends` / `Calls` / `References` / `DependsOn` edges.
//! - One `TransformGraph` per method with real data flow, lowered through RFC 0027's own
//!   `lower_to_kir`, so binary-derived logic is diffable against Pentaho, SQL and stored-procedure
//!   logic through the existing `ekos_transformation_diff` with no new consumer tooling.
//!
//! # What is deliberately *not* claimed
//!
//! **`Calls` edges are emitted only to methods recovered in the same run.** A compiled method
//! calls hundreds of framework methods; materializing an object for each would bury the real
//! business graph under `java.lang.StringBuilder.append`, and inventing objects for code EKOS has
//! never seen would be fabrication. Unresolved call targets survive as the method's own
//! `call_targets` property — evidence, without a fake edge. This is the same "resolve what is
//! really here, skip the rest" line RFC 0091/0092 drew for Python.
//!
//! **A method's Transformation IR is data flow, not control flow.** `Fidelity::Structural` gives
//! field reads/writes, I/O boundaries and branch *counts*, not statement trees, so the graph says
//! what a method reads, what it decides on, and what it writes — and marks the rest `Unmapped`
//! rather than inventing structure. `TransformNode::Unmapped` exists in RFC 0027 for exactly this.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_binary::{
    AccessMode, Branch, CallSite, DecompiledAst, DecompiledMethod, DecompiledType, MethodBody,
};
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{
    KirEvidence, KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind,
    SourceLocation,
};
use ekos_semantic::transform_ir::{NodeId, TransformGraph, TransformNode, TransformOrigin};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

/// Per-method caps on how many literals reach the ledger.
///
/// A single generated method can carry thousands of string constants (resource tables, generated
/// parsers), and all of them would land in the searchable `excerpt`. The cap keeps one pathological
/// method from dominating search results for an entire workspace. Truncation is recorded on the
/// object rather than silent.
const MAX_LITERALS_PER_METHOD: usize = 32;
/// Longest single literal kept, in characters. Longer ones are truncated with an ellipsis.
const MAX_LITERAL_CHARS: usize = 160;

#[derive(Debug, Deserialize)]
struct BinaryArtifactData {
    path: String,
    ast: DecompiledAst,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace. Qualifies id hashing only.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run.
#[derive(Debug, Clone, Copy, Default)]
pub struct BinaryStats {
    pub binaries: usize,
    pub types: usize,
    pub methods: usize,
    pub fields: usize,
    pub io_boundaries: usize,
    /// `Calls` edges that resolved onto a method recovered in this same run.
    pub calls_resolved: usize,
    /// Call sites whose target was not recovered here (framework/third-party code). Reported so
    /// "few call edges" is visibly a resolution outcome, not a parse failure.
    pub calls_unresolved: usize,
    pub transform_nodes: usize,
}

pub struct BinaryAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<BinaryStats>>,
}

impl BinaryAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("binary-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(BinaryStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<BinaryStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for BinaryAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    /// Bump on any change to this pass's output shape — `cache_inputs` alone cannot catch a logic
    /// change, as `rust_analyzer`'s own `version` doc records at length.
    fn version(&self) -> &str {
        "v1"
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = BinaryStats::default();

        // One assembly object per compiled file, shared by every type artifact from it — a jar
        // yields thousands of artifacts naming one archive.
        let mut assemblies: HashMap<KirId, ()> = HashMap::new();
        // `qualified type.method descriptor` → the method object it resolved to, used to turn call
        // sites into real `Calls` edges. First occurrence wins: a workspace can legitimately hold
        // both a facade and the real implementation of one assembly (wine-mono ships exactly
        // that), and guessing which of the two a call meant would be worse than picking one and
        // saying so here.
        let mut method_index: HashMap<String, KirId> = HashMap::new();
        // Thin objects already materialized for entities referenced but not compiled here, so a
        // type inherited from by 500 classes yields one object rather than 500.
        let mut external: HashMap<KirId, ()> = HashMap::new();
        // Deferred until every artifact has been read, because a call very often precedes its
        // target in artifact order.
        let mut pending_calls: Vec<(KirId, String)> = Vec::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("BIN001", format!("cannot read artifact {artifact_id}: {e}"));
                    continue;
                }
            };
            let data: BinaryArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "BIN002",
                        format!("malformed binary payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let assembly_id = kir_id("assembly", &id_path, "");

            if assemblies.insert(assembly_id, ()).is_none() {
                let ev = combined_ev(
                    &mut combined,
                    &data.path,
                    &data.ast.assembly_name,
                    "assembly",
                    &id_path,
                    "",
                );
                combined.add_object(assembly_object(&data, assembly_id, ev));
                stats.binaries += 1;
                for reference in &data.ast.references {
                    let ref_id = kir_id("assembly-ref", reference, "");
                    add_external(
                        &mut combined,
                        &mut external,
                        ref_id,
                        reference,
                        "BinaryAssembly",
                        "assembly-ref",
                    );
                    combined.add_relationship(KirRelationship::deterministic(
                        RelationshipKind::DependsOn,
                        assembly_id,
                        ref_id,
                        "binary-reference",
                    ));
                }
            }

            for ty in &data.ast.types {
                let type_id = kir_id("type", &id_path, &ty.locator);
                stats.types += 1;

                let ev = combined_ev(
                    &mut combined,
                    &data.path,
                    &ty.qualified_name(),
                    "type",
                    &id_path,
                    &ty.locator,
                );
                combined.add_object(type_object(ty, type_id, &data, ev));
                combined.add_relationship(KirRelationship::deterministic(
                    RelationshipKind::Contains,
                    assembly_id,
                    type_id,
                    "binary-type",
                ));

                // Inheritance and interface implementation are both `Extends` — the taxonomy
                // draws no distinction, and neither do most of the questions asked of it.
                for parent in ty.super_type.iter().chain(ty.interfaces.iter()) {
                    let parent_id = kir_id("type-ref", parent, "");
                    add_external(
                        &mut combined,
                        &mut external,
                        parent_id,
                        parent,
                        "BinaryType",
                        "type-ref",
                    );
                    combined.add_relationship(KirRelationship::deterministic(
                        RelationshipKind::Extends,
                        type_id,
                        parent_id,
                        "binary-extends",
                    ));
                }

                for field in &ty.fields {
                    let field_id = kir_id("field", &id_path, &field.locator);
                    stats.fields += 1;
                    let ev = combined_ev(
                        &mut combined,
                        &data.path,
                        &format!("{} {}", field.type_name, field.name),
                        "field",
                        &id_path,
                        &field.locator,
                    );
                    let mut obj = KirObject::new(
                        field.name.clone(),
                        ObjectKind::Custom("BinaryField".into()),
                    );
                    obj.id = field_id;
                    obj.evidence.push(ev);
                    set(&mut obj, "type_name", &field.type_name);
                    set(&mut obj, "visibility", field.visibility.as_str());
                    obj.properties
                        .insert("is_static".into(), field.is_static.into());
                    obj.properties
                        .insert("is_final".into(), field.is_final.into());
                    obj.properties
                        .insert("compiler_generated".into(), field.compiler_generated.into());
                    set(&mut obj, "owner", &ty.qualified_name());
                    set(&mut obj, "binary_path", &data.path);
                    set(&mut obj, "extractor", &data.ast.source.extractor);
                    combined.add_object(obj);
                    combined.add_relationship(KirRelationship::deterministic(
                        RelationshipKind::Contains,
                        type_id,
                        field_id,
                        "binary-field",
                    ));
                }

                for method in &ty.methods {
                    let method_id = kir_id("method", &id_path, &method.locator);
                    stats.methods += 1;
                    let ev = combined_ev(
                        &mut combined,
                        &data.path,
                        &format!("{}{}", method.name, method.descriptor),
                        "method",
                        &id_path,
                        &method.locator,
                    );
                    combined.add_object(method_object(method, method_id, ty, &data, ev));
                    combined.add_relationship(KirRelationship::deterministic(
                        RelationshipKind::Contains,
                        type_id,
                        method_id,
                        "binary-method",
                    ));
                    method_index
                        .entry(call_key(
                            &ty.qualified_name(),
                            &method.name,
                            &method.descriptor,
                        ))
                        .or_insert(method_id);

                    let Some(body) = &method.body else { continue };

                    for call in &body.calls {
                        pending_calls.push((
                            method_id,
                            call_key(&call.owner, &call.method, &call.descriptor),
                        ));
                        if let Some(io) = call.io {
                            let io_id = kir_id(
                                "io",
                                &id_path,
                                &format!("{}@{}", method.locator, call.offset),
                            );
                            stats.io_boundaries += 1;
                            let ev = combined_ev(
                                &mut combined,
                                &data.path,
                                &format!("{}.{}", call.owner, call.method),
                                "io",
                                &id_path,
                                &format!("{}@{}", method.locator, call.offset),
                            );
                            let mut obj = KirObject::new(
                                format!("{}.{}", call.owner, call.method),
                                ObjectKind::Custom("ExternalIoBoundary".into()),
                            );
                            obj.id = io_id;
                            obj.evidence.push(ev);
                            set(&mut obj, "io_kind", io.as_str());
                            set(&mut obj, "owner", &call.owner);
                            set(&mut obj, "method", &call.method);
                            set(&mut obj, "binary_path", &data.path);
                            set(&mut obj, "extractor", &data.ast.source.extractor);
                            obj.properties
                                .insert("bytecode_offset".into(), call.offset.into());
                            combined.add_object(obj);
                            combined.add_relationship(KirRelationship::deterministic(
                                RelationshipKind::References,
                                method_id,
                                io_id,
                                "binary-io",
                            ));
                        }
                    }

                    let graph = method_transform_graph(method, ty, body, &id_path);
                    if !graph.nodes.is_empty() {
                        let lowered = ekos_semantic::transform_ir::lower_to_kir(&graph);
                        stats.transform_nodes += lowered.objects.len();
                        // The method owns the steps recovered from it, so the transformation
                        // graph is reachable from the method rather than floating free.
                        for obj in &lowered.objects {
                            combined.add_relationship(KirRelationship::deterministic(
                                RelationshipKind::Contains,
                                method_id,
                                obj.id,
                                "binary-transform-node",
                            ));
                        }
                        combined.objects.extend(lowered.objects);
                        combined.relationships.extend(lowered.relationships);
                        combined.evidence.extend(lowered.evidence);
                    }
                }
            }
        }

        for (from, key) in pending_calls {
            match method_index.get(&key) {
                Some(&to) => {
                    stats.calls_resolved += 1;
                    combined.add_relationship(KirRelationship::deterministic(
                        RelationshipKind::Calls,
                        from,
                        to,
                        "binary-call",
                    ));
                }
                None => stats.calls_unresolved += 1,
            }
        }

        *self.stats.lock().unwrap() = stats;

        if combined.objects.is_empty() {
            return Ok(());
        }

        let knowledge = ekos_artifact::KnowledgeArtifact::new(&self.pass_id, vec![], combined);
        let json = serde_json::to_value(&knowledge)
            .map_err(|e| PassError::failed(format!("serialize KnowledgeArtifact: {e}")))?;
        ctx.artifact_store
            .write(&knowledge.id, &json)
            .map_err(|e| PassError::failed(format!("write KnowledgeArtifact: {e}")))?;
        Ok(())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Object construction
// ────────────────────────────────────────────────────────────────────────────

/// Deterministic id for anything this pass emits.
///
/// Every kind is keyed structurally — `(binary path, locator)` — which is why all of them are
/// registered `structurally_keyed: true` in `custom_kinds::REGISTRY`. Two `BinaryMethod`s from
/// different assemblies are never the same entity, and identity resolution must not be allowed to
/// decide otherwise on a name-prefix match (RFC 0135 Part D).
fn kir_id(kind: &str, scope: &str, locator: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("binary:{kind}:{scope}:{locator}").as_bytes(),
    ))
}

/// The key a call site and a method declaration must agree on for a `Calls` edge to resolve.
fn call_key(owner: &str, method: &str, descriptor: &str) -> String {
    format!("{owner}.{method}{descriptor}")
}

/// Add one deterministic evidence record and return its id.
///
/// The id must be stable across re-runs: `KirObject::evidence` is part of what the ledger's
/// `content_signature` hashes, so a random evidence id would make every re-compile of an unchanged
/// binary look like a content change. This mirrors `transform_ir::transform_evidence_kir_id`'s own
/// reasoning exactly.
fn combined_ev(
    kir: &mut KirGraph,
    display_path: &str,
    fragment: &str,
    kind: &str,
    scope: &str,
    locator: &str,
) -> KirId {
    let mut ev = KirEvidence::new(SourceLocation::file(display_path.to_string()), fragment);
    ev.id = kir_id(&format!("{kind}-evidence"), scope, locator);
    kir.add_evidence(ev)
}

/// Materialize a thin object for an entity this run references but did not compile — a
/// superclass in the framework, an assembly that was not observed.
///
/// Without it the edge dangles and `compile` warns once per edge (2,639 `SEM002` warnings on a
/// two-binary workspace, because `java.lang.Comparable` is inherited from everywhere). The
/// alternative — dropping the edge — would delete the answer to "what implements this interface",
/// which is one of the questions a recovered binary graph exists to answer.
///
/// This is `perl_analyzer`'s established shape for a `use` target: one deterministically-keyed
/// object, marked `external` so nothing mistakes it for something EKOS actually read. Keyed by
/// qualified name rather than by binary path, because a reference names only the name — so if the
/// defining binary is later observed, its own path-scoped object is a *separate*, richer object,
/// and this one stays the reference stub it always was.
fn add_external(
    kir: &mut KirGraph,
    seen: &mut HashMap<KirId, ()>,
    id: KirId,
    name: &str,
    kind: &str,
    scope: &str,
) {
    if seen.insert(id, ()).is_some() {
        return;
    }
    let mut obj = KirObject::new(name, ObjectKind::Custom(kind.into()));
    obj.id = id;
    obj.properties.insert("external".into(), true.into());
    set(&mut obj, "locator", name);
    let mut ev = KirEvidence::new(SourceLocation::file(scope.to_string()), name);
    ev.id = kir_id(&format!("{scope}-evidence"), name, "");
    let ev_id = kir.add_evidence(ev);
    obj.evidence.push(ev_id);
    kir.add_object(obj);
}

fn set(obj: &mut KirObject, key: &str, value: &str) {
    obj.properties.insert(key.into(), value.into());
}

fn assembly_object(data: &BinaryArtifactData, id: KirId, evidence: KirId) -> KirObject {
    let mut obj = KirObject::new(
        data.ast.assembly_name.clone(),
        ObjectKind::Custom("BinaryAssembly".into()),
    );
    obj.id = id;
    obj.evidence.push(evidence);
    set(&mut obj, "binary_path", &data.path);
    set(&mut obj, "binary_kind", data.ast.source.kind.as_str());
    set(&mut obj, "format_version", &data.ast.source.format_version);
    set(&mut obj, "extractor", &data.ast.source.extractor);
    // The far end of RFC 0148's two-hop provenance chain: every fact below traces through its
    // locator to this hash, which identifies the exact build it came from.
    set(&mut obj, "binary_sha256", &data.ast.source.sha256);
    set(
        &mut obj,
        "fidelity",
        match data.ast.fidelity {
            ekos_binary::Fidelity::Structural => "structural",
            ekos_binary::Fidelity::Statements => "statements",
        },
    );
    obj
}

fn type_object(
    ty: &DecompiledType,
    id: KirId,
    data: &BinaryArtifactData,
    evidence: KirId,
) -> KirObject {
    let mut obj = KirObject::new(ty.qualified_name(), ObjectKind::Custom("BinaryType".into()));
    obj.id = id;
    obj.evidence.push(evidence);
    set(&mut obj, "namespace", &ty.namespace);
    set(
        &mut obj,
        "category",
        match ty.category {
            ekos_binary::TypeCategory::Class => "class",
            ekos_binary::TypeCategory::Interface => "interface",
            ekos_binary::TypeCategory::Enum => "enum",
            ekos_binary::TypeCategory::Struct => "struct",
        },
    );
    set(&mut obj, "visibility", ty.visibility.as_str());
    set(&mut obj, "locator", &ty.locator);
    set(&mut obj, "binary_path", &data.path);
    set(&mut obj, "extractor", &data.ast.source.extractor);
    set(&mut obj, "binary_sha256", &data.ast.source.sha256);
    if let Some(s) = &ty.super_type {
        set(&mut obj, "super_type", s);
    }
    obj.properties
        .insert("is_abstract".into(), ty.is_abstract.into());
    obj.properties
        .insert("compiler_generated".into(), ty.compiler_generated.into());
    obj.properties
        .insert("method_count".into(), ty.methods.len().into());
    obj.properties
        .insert("field_count".into(), ty.fields.len().into());
    obj
}

fn method_object(
    method: &DecompiledMethod,
    id: KirId,
    ty: &DecompiledType,
    data: &BinaryArtifactData,
    evidence: KirId,
) -> KirObject {
    let mut obj = KirObject::new(
        method.name.clone(),
        ObjectKind::Custom("BinaryMethod".into()),
    );
    obj.id = id;
    obj.evidence.push(evidence);
    set(&mut obj, "signature", &method.signature);
    set(&mut obj, "descriptor", &method.descriptor);
    set(&mut obj, "visibility", method.visibility.as_str());
    set(&mut obj, "owner", &ty.qualified_name());
    set(&mut obj, "locator", &method.locator);
    set(&mut obj, "binary_path", &data.path);
    set(&mut obj, "extractor", &data.ast.source.extractor);
    set(&mut obj, "binary_sha256", &data.ast.source.sha256);
    obj.properties
        .insert("is_static".into(), method.is_static.into());
    obj.properties
        .insert("is_abstract".into(), method.is_abstract.into());
    obj.properties.insert(
        "compiler_generated".into(),
        method.compiler_generated.into(),
    );

    if let Some(body) = &method.body {
        obj.properties
            .insert("code_size".into(), body.code_size.into());
        // Counted from real branch opcodes, so this is the compiled method's actual cyclomatic
        // complexity rather than an estimate from source text.
        obj.properties.insert(
            "cyclomatic_complexity".into(),
            body.cyclomatic_complexity().into(),
        );
        obj.properties
            .insert("call_count".into(), body.calls.len().into());
        obj.properties
            .insert("branch_count".into(), body.branches.len().into());

        let strings = capped(body.string_literals.iter().map(|l| truncate(&l.value)));
        let numbers = capped(body.numeric_literals.iter().map(|l| l.value.clone()));
        if !strings.is_empty() {
            obj.properties
                .insert("string_literals".into(), strings.clone().into());
        }
        if !numbers.is_empty() {
            obj.properties
                .insert("numeric_literals".into(), numbers.clone().into());
        }
        // `excerpt` is the one property `KirObject::indexed_content` reads, so putting the
        // signature and the literals here is what makes a compiled method findable by the
        // error message or the threshold it contains — through `ekos_search` and `ekos ask`,
        // with no embeddings and no extra index (the same trick RFC 0026 relies on for
        // `Concept` text and RFC 0027 for filter predicates).
        let mut excerpt = format!("{} {}", method.name, method.signature);
        for s in strings.iter().chain(numbers.iter()) {
            excerpt.push(' ');
            excerpt.push_str(s);
        }
        set(&mut obj, "excerpt", &excerpt);

        // Unresolved call targets are kept as evidence even though they get no edge — see the
        // module docs on why no object is invented for framework code.
        let targets = capped(
            body.calls
                .iter()
                .map(|c| format!("{}.{}", c.owner, c.method)),
        );
        if !targets.is_empty() {
            obj.properties.insert("call_targets".into(), targets.into());
        }
    }
    obj
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_LITERAL_CHARS {
        return s.to_string();
    }
    let head: String = s.chars().take(MAX_LITERAL_CHARS).collect();
    format!("{head}…")
}

/// Cap a literal list, preserving order and dropping exact duplicates.
///
/// Duplicates are extremely common (the same message loaded on several branches) and carry no
/// extra information once the method already cites the value.
fn capped(values: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in values {
        if v.is_empty() || out.contains(&v) {
            continue;
        }
        out.push(v);
        if out.len() >= MAX_LITERALS_PER_METHOD {
            break;
        }
    }
    out
}

// ────────────────────────────────────────────────────────────────────────────
// Transformation IR
// ────────────────────────────────────────────────────────────────────────────

/// Build one method's data-flow graph in RFC 0027's shared IR.
///
/// Reads become `Source`, writes become `Sink`, I/O calls become whichever of the two their
/// direction implies, and each conditional branch becomes a `Filter` naming the decision. Nodes
/// are emitted in bytecode order, and every node is fed from the one before it — a compiled
/// method is a straight-line instruction sequence at this fidelity, so a linear chain is the
/// honest edge set rather than a guessed DAG.
fn method_transform_graph(
    method: &DecompiledMethod,
    ty: &DecompiledType,
    body: &MethodBody,
    scope: &str,
) -> TransformGraph {
    let mut nodes: Vec<TransformNode> = Vec::new();

    // Merge every event into one bytecode-ordered sequence, so the graph reads in the order the
    // method actually executes rather than grouped by category.
    enum Event<'a> {
        Field(&'a ekos_binary::FieldAccess),
        Call(&'a CallSite),
        Branch(&'a Branch),
    }
    let mut events: Vec<(u32, Event<'_>)> = Vec::new();
    events.extend(
        body.field_access
            .iter()
            .map(|f| (f.offset, Event::Field(f))),
    );
    events.extend(
        body.calls
            .iter()
            .filter(|c| c.io.is_some())
            .map(|c| (c.offset, Event::Call(c))),
    );
    events.extend(body.branches.iter().map(|b| (b.offset, Event::Branch(b))));
    events.sort_by_key(|(offset, _)| *offset);

    for (_, event) in &events {
        nodes.push(match event {
            Event::Field(f) => {
                let object_name = if f.owner.is_empty() {
                    format!("{}.{}", ty.qualified_name(), f.name)
                } else {
                    format!("{}.{}", f.owner, f.name)
                };
                let columns = vec![f.type_name.clone()];
                match f.mode {
                    AccessMode::Read => TransformNode::Source {
                        object_name,
                        columns,
                    },
                    AccessMode::Write => TransformNode::Sink {
                        object_name,
                        columns,
                    },
                }
            }
            Event::Call(c) => {
                let object_name = format!("{}.{}", c.owner, c.method);
                let columns = vec![c.io.map(|i| i.as_str().to_string()).unwrap_or_default()];
                // Direction is read from the callee's own name, which is the only evidence
                // available at `Structural` fidelity. A name that says neither is treated as a
                // read, because a call whose result is discarded is rarer than one whose result
                // is used.
                if is_write_call(&c.method) {
                    TransformNode::Sink {
                        object_name,
                        columns,
                    }
                } else {
                    TransformNode::Source {
                        object_name,
                        columns,
                    }
                }
            }
            // The predicate itself is not recoverable at `Structural` fidelity — only that a
            // decision happens here, on which opcode, at which offset. Naming exactly that is
            // the honest `Filter`; inventing a condition would not be.
            Event::Branch(b) => TransformNode::Filter {
                condition: format!("{} at offset {}", b.opcode, b.offset),
            },
        });
    }

    // A method with a body but no recovered data flow is real and common (pure computation on
    // locals). `Unmapped` records that something is here and was not resolved, which RFC 0027
    // introduced precisely so a gap is visible rather than absent.
    if nodes.is_empty() && body.code_size > 0 {
        nodes.push(TransformNode::Unmapped {
            raw: format!("{} {}", method.name, method.signature),
            reason: format!(
                "{} bytes of bytecode with no field access, I/O boundary or branch recovered at \
                 structural fidelity",
                body.code_size
            ),
        });
    }

    let edges = (1..nodes.len())
        .map(|i| (NodeId(i as u32 - 1), NodeId(i as u32)))
        .collect();

    TransformGraph {
        nodes,
        edges,
        origin: TransformOrigin {
            source_path: format!("{scope}:{}", method.locator),
            source_kind: "binary-method".into(),
            // `lower_to_kir` derives every node and evidence id from `(source_kind, source_path,
            // index)` only, so this timestamp never reaches the ledger and cannot make an
            // unchanged binary look changed. It is set from the epoch rather than `now()` so
            // that remains true even if a future caller decides to serialize the graph itself.
            extracted_at: chrono::DateTime::UNIX_EPOCH,
        },
    }
}

/// Whether a callee's name says it writes rather than reads.
///
/// Deliberately a small, fixed list of the verbs that actually appear in JDBC/ADO.NET, file and
/// HTTP APIs. It is a naming heuristic and is confined to choosing between `Source` and `Sink`
/// on an I/O node — it never creates or suppresses a fact.
///
/// A bare `execute` is **not** on the list, and that is the whole reason the list is spelled out
/// rather than matched loosely: JDBC's `executeQuery` reads and `executeUpdate` writes, so a
/// prefix match on `execute` calls every SELECT in a codebase a write. The write forms are named
/// individually instead.
fn is_write_call(method: &str) -> bool {
    const WRITE_VERBS: &[&str] = &[
        "write",
        "insert",
        "update",
        "delete",
        "save",
        "store",
        "put",
        "post",
        "send",
        "executeupdate",
        "executenonquery",
        "executebatch",
        "flush",
        "append",
        "create",
        "commit",
        "persist",
        "merge",
        "remove",
        "add",
        "set",
        "upload",
    ];
    let lower = method.to_ascii_lowercase();
    WRITE_VERBS.iter().any(|v| lower.starts_with(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ekos_binary::*;

    fn method(name: &str, body: Option<MethodBody>) -> DecompiledMethod {
        DecompiledMethod {
            name: name.into(),
            locator: format!("com/acme/Billing.{name}:()V"),
            signature: "() -> void".into(),
            descriptor: "()V".into(),
            visibility: Visibility::Public,
            is_static: false,
            is_abstract: false,
            compiler_generated: false,
            body,
        }
    }

    fn ty(methods: Vec<DecompiledMethod>) -> DecompiledType {
        DecompiledType {
            namespace: "com.acme".into(),
            name: "Billing".into(),
            locator: "com/acme/Billing".into(),
            category: TypeCategory::Class,
            visibility: Visibility::Public,
            is_abstract: false,
            super_type: None,
            interfaces: vec![],
            compiler_generated: false,
            fields: vec![],
            methods,
        }
    }

    fn field_access(offset: u32, mode: AccessMode, name: &str) -> FieldAccess {
        FieldAccess {
            offset,
            mode,
            owner: "com.acme.Billing".into(),
            name: name.into(),
            type_name: "int".into(),
            is_static: false,
        }
    }

    fn io_call(offset: u32, method: &str) -> CallSite {
        CallSite {
            offset,
            kind: CallKind::Virtual,
            owner: "java.sql.PreparedStatement".into(),
            method: method.into(),
            descriptor: "()V".into(),
            io: Some(IoBoundary::Database),
        }
    }

    #[test]
    fn ids_are_deterministic_and_scoped_by_binary_and_locator() {
        assert_eq!(
            kir_id("method", "a.jar", "X.m"),
            kir_id("method", "a.jar", "X.m")
        );
        // The same locator in a different binary is a different entity — this is exactly the
        // over-merge `structurally_keyed` exists to prevent.
        assert_ne!(
            kir_id("method", "a.jar", "X.m"),
            kir_id("method", "b.jar", "X.m")
        );
        assert_ne!(
            kir_id("method", "a.jar", "X.m"),
            kir_id("type", "a.jar", "X.m")
        );
    }

    #[test]
    fn a_field_read_becomes_a_source_and_a_write_becomes_a_sink() {
        let body = MethodBody {
            code_size: 40,
            field_access: vec![
                field_access(0, AccessMode::Read, "rate"),
                field_access(8, AccessMode::Write, "total"),
            ],
            ..Default::default()
        };
        let m = method("calculate", Some(body.clone()));
        let g = method_transform_graph(&m, &ty(vec![]), &body, "billing.jar");

        assert_eq!(g.nodes.len(), 2);
        assert!(matches!(
            &g.nodes[0],
            TransformNode::Source { object_name, .. } if object_name == "com.acme.Billing.rate"
        ));
        assert!(matches!(
            &g.nodes[1],
            TransformNode::Sink { object_name, .. } if object_name == "com.acme.Billing.total"
        ));
        assert_eq!(g.edges, vec![(NodeId(0), NodeId(1))]);
    }

    /// Nodes must follow bytecode order, not the order the categories happen to be collected in,
    /// or the recovered graph misrepresents what the method does first.
    #[test]
    fn nodes_are_emitted_in_bytecode_order_across_categories() {
        let body = MethodBody {
            code_size: 64,
            field_access: vec![field_access(30, AccessMode::Write, "total")],
            calls: vec![io_call(10, "getString")],
            branches: vec![Branch {
                offset: 20,
                opcode: "ifeq".into(),
                case_count: 1,
            }],
            ..Default::default()
        };
        let g = method_transform_graph(&method("run", Some(body.clone())), &ty(vec![]), &body, "b");
        let kinds: Vec<&str> = g
            .nodes
            .iter()
            .map(|n| match n {
                TransformNode::Source { .. } => "source",
                TransformNode::Sink { .. } => "sink",
                TransformNode::Filter { .. } => "filter",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, vec!["source", "filter", "sink"]);
    }

    #[test]
    fn a_branch_names_the_decision_without_inventing_a_predicate() {
        let body = MethodBody {
            code_size: 10,
            branches: vec![Branch {
                offset: 4,
                opcode: "if_icmpgt".into(),
                case_count: 1,
            }],
            ..Default::default()
        };
        let g = method_transform_graph(
            &method("check", Some(body.clone())),
            &ty(vec![]),
            &body,
            "b",
        );
        match &g.nodes[0] {
            TransformNode::Filter { condition } => {
                assert_eq!(condition, "if_icmpgt at offset 4");
                assert!(
                    !condition.contains('='),
                    "a predicate must not be fabricated at structural fidelity"
                );
            }
            other => panic!("expected a Filter, got {other:?}"),
        }
    }

    /// A body with no recoverable data flow must say so, not silently produce nothing.
    #[test]
    fn a_body_with_no_recovered_flow_is_unmapped_not_empty() {
        let body = MethodBody {
            code_size: 24,
            ..Default::default()
        };
        let g = method_transform_graph(
            &method("compute", Some(body.clone())),
            &ty(vec![]),
            &body,
            "b",
        );
        assert!(
            matches!(&g.nodes[0], TransformNode::Unmapped { reason, .. } if reason.contains("24 bytes"))
        );
    }

    #[test]
    fn an_empty_body_produces_no_nodes_at_all() {
        let body = MethodBody::default();
        let g = method_transform_graph(
            &method("abstract_like", Some(body.clone())),
            &ty(vec![]),
            &body,
            "b",
        );
        assert!(g.nodes.is_empty());
    }

    #[test]
    fn io_call_direction_follows_the_callee_name() {
        for (name, expect_sink) in [
            ("executeUpdate", true),
            ("insertRow", true),
            ("sendMessage", true),
            ("executeQuery", false), // a SELECT reads; only the `executeUpdate` family writes
            ("executeNonQuery", true),
            ("getString", false),
            ("readLine", false),
            ("next", false),
        ] {
            let body = MethodBody {
                code_size: 8,
                calls: vec![io_call(0, name)],
                ..Default::default()
            };
            let g =
                method_transform_graph(&method("m", Some(body.clone())), &ty(vec![]), &body, "b");
            let is_sink = matches!(&g.nodes[0], TransformNode::Sink { .. });
            assert_eq!(is_sink, expect_sink, "{name} classified wrongly");
        }
    }

    #[test]
    fn transform_graphs_lower_to_stable_kir_ids() {
        let body = MethodBody {
            code_size: 40,
            field_access: vec![field_access(0, AccessMode::Read, "rate")],
            ..Default::default()
        };
        let m = method("calculate", Some(body.clone()));
        let a = ekos_semantic::transform_ir::lower_to_kir(&method_transform_graph(
            &m,
            &ty(vec![]),
            &body,
            "billing.jar",
        ));
        let b = ekos_semantic::transform_ir::lower_to_kir(&method_transform_graph(
            &m,
            &ty(vec![]),
            &body,
            "billing.jar",
        ));
        assert_eq!(
            a.objects.iter().map(|o| o.id).collect::<Vec<_>>(),
            b.objects.iter().map(|o| o.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn literals_are_deduplicated_capped_and_truncated() {
        let many = (0..100).map(|i| format!("msg{}", i % 5));
        let out = capped(many);
        assert_eq!(out.len(), 5, "duplicates collapse");

        let unique = (0..100).map(|i| format!("msg{i}"));
        assert_eq!(capped(unique).len(), MAX_LITERALS_PER_METHOD);

        let long = "x".repeat(500);
        let t = truncate(&long);
        assert_eq!(
            t.chars().count(),
            MAX_LITERAL_CHARS + 1,
            "plus the ellipsis"
        );
        assert!(t.ends_with('…'));
        assert_eq!(truncate("short"), "short");
    }

    #[test]
    fn empty_literals_are_dropped() {
        assert!(capped(["".to_string(), "".to_string()].into_iter()).is_empty());
    }

    /// The literals a business rule turns on must reach `excerpt`, because that is the only
    /// property fed to full-text search — it is what makes "which compiled method mentions
    /// 'insufficient funds'?" answerable at all.
    #[test]
    fn method_excerpt_carries_the_literals_search_needs() {
        let body = MethodBody {
            code_size: 40,
            string_literals: vec![StringLiteral {
                offset: 0,
                value: "insufficient funds".into(),
            }],
            numeric_literals: vec![NumericLiteral {
                offset: 4,
                value: "10000".into(),
            }],
            ..Default::default()
        };
        let t = ty(vec![]);
        let m = method("withdraw", Some(body));
        let data: BinaryArtifactData = serde_json::from_value(serde_json::json!({
            "path": "bank.jar",
            "ast": {
                "source": {
                    "kind": "jvm", "path": "bank.jar", "sha256": "abc",
                    "format_version": "52.0", "extractor": "ekos-jvm-classfile/v1"
                },
                "fidelity": "structural",
                "assembly_name": "bank.jar",
                "types": []
            }
        }))
        .unwrap();

        let obj = method_object(
            &m,
            kir_id("method", "bank.jar", "x"),
            &t,
            &data,
            KirId::new(),
        );
        let excerpt = obj.properties["excerpt"].as_str().unwrap();
        assert!(excerpt.contains("insufficient funds"));
        assert!(excerpt.contains("10000"));
        assert!(excerpt.contains("withdraw"));

        // The provenance chain must be materialized on the object itself, not merely implied.
        assert_eq!(obj.properties["binary_sha256"], "abc");
        assert_eq!(obj.properties["extractor"], "ekos-jvm-classfile/v1");
        assert_eq!(obj.properties["locator"], m.locator);
    }

    #[test]
    fn call_keys_match_between_a_declaration_and_a_call_site() {
        assert_eq!(
            call_key("com.acme.Billing", "calculate", "()V"),
            call_key("com.acme.Billing", "calculate", "()V")
        );
        assert_ne!(
            call_key("com.acme.Billing", "calculate", "()V"),
            call_key("com.acme.Billing", "calculate", "(I)V"),
            "overloads must stay distinct"
        );
    }
}
