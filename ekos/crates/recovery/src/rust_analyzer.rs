//! `RustAnalyzerPass` — converts Rust (`.rs`) observation artifacts (RFC 0041) into plain KIR
//! objects and relationships via real AST parsing (`syn`, no LLM in the loop), same shape as
//! `PythonAnalyzerPass`/`PentahoAnalyzerPass`.
//!
//! Unlike Python/PySpark, Rust source has no honest analog of a DataFrame method chain to lower
//! into the Transformation IR (RFC 0027) — this analyzer does not attempt one. Its headline
//! capability is instead a real, if intentionally narrow, function-call graph:
//! `RelationshipKind::Calls` is populated for the first time in this project. Scope, deliberately
//! narrow and honest rather than a claim of full interprocedural resolution:
//! - `use` imports become `KirObject(Custom("RustModule"))` + `DependsOn` edges from the file's
//!   own `KirId` (same UUIDv5-over-relative-path scheme `build.rs`/`python_analyzer.rs` use).
//! - `fn`/`struct`/`enum`/`trait` items, and associated `fn`s inside `impl` blocks, become
//!   `KirObject(Custom("RustSymbol"))` + `Contains` edges from the file.
//! - `Calls` edges are recognized only for call expressions resolvable to a symbol defined in the
//!   *same file*: a bare function call (`foo(...)`) resolved against top-level `fn`s in the file;
//!   an associated-function call (`Type::method(...)` or `Self::method(...)`) resolved against
//!   `impl` methods in the same file; a method call (`x.method(...)`) resolved only when the
//!   method name is unambiguous among all methods recognized in the file (if two different types
//!   in the same file define a same-named method, neither call site is guessed at). Calls to
//!   anything outside the file (external crates, std library, trait default methods) are simply
//!   not recorded — no placeholder needed, since `Calls` is a plain relationship, not a
//!   `TransformNode` graph requiring every step accounted for.

use async_trait::async_trait;
use ekos_artifact::ArtifactId;
use ekos_compiler_core::pass::{CompilerPass, PassContext, PassError};
use ekos_kir::{KirGraph, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use syn::visit::Visit;
use syn::{ImplItem, Item, UseTree};
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct RustArtifactData {
    path: String,
    source: String,
    /// RFC 0079: present only in a multi-`[observe] paths` workspace (`build.rs`'s own choke
    /// point). Qualifies id hashing only — `path` above stays bare everywhere it's displayed.
    #[serde(default)]
    project: Option<String>,
}

/// Coverage counters from one run, mirroring `PythonStats`.
#[derive(Debug, Clone, Copy, Default)]
pub struct RustStats {
    pub files_processed: usize,
    pub symbols_total: usize,
    pub calls_total: usize,
}

pub struct RustAnalyzerPass {
    pass_id: String,
    artifact_ids: Vec<ArtifactId>,
    stats: Arc<Mutex<RustStats>>,
}

impl RustAnalyzerPass {
    pub fn new(workspace_name: impl Into<String>, artifact_ids: Vec<ArtifactId>) -> Self {
        Self {
            pass_id: format!("rust-analyzer:{}", workspace_name.into()),
            artifact_ids,
            stats: Arc::new(Mutex::new(RustStats::default())),
        }
    }

    pub fn stats_handle(&self) -> Arc<Mutex<RustStats>> {
        Arc::clone(&self.stats)
    }
}

#[async_trait]
impl CompilerPass for RustAnalyzerPass {
    fn name(&self) -> &str {
        &self.pass_id
    }

    fn cache_inputs(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.artifact_ids.iter().map(|id| id.to_string()).collect();
        ids.sort();
        ids
    }

    async fn run(&mut self, ctx: &mut PassContext) -> Result<(), PassError> {
        let mut combined = KirGraph::new();
        let mut stats = RustStats::default();
        // Dedup Rust-module target objects across files within this one run — many files can
        // `use` the same module; mirrors `python_analyzer.rs`'s `PythonModule` dedup discipline.
        let mut seen_modules: HashSet<KirId> = HashSet::new();

        for artifact_id in &self.artifact_ids {
            let json = match ctx.artifact_store.read(artifact_id) {
                Ok(Some(j)) => j,
                Ok(None) => continue,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "RUST001",
                        format!("cannot read artifact {artifact_id}: {e}"),
                    );
                    continue;
                }
            };
            let data: RustArtifactData = match serde_json::from_value(json["data"].clone()) {
                Ok(d) => d,
                Err(e) => {
                    ctx.diagnostics.lock().unwrap().warning(
                        "RUST002",
                        format!("malformed rust payload in {artifact_id}: {e}"),
                    );
                    continue;
                }
            };

            // RFC 0079: `parse_rust_file`'s own `path` parameter is used *only* as an id-hash
            // ingredient inside it (`add_symbol`'s `"rust-symbol:{path}:{name}"`) — this crate
            // emits no evidence/`SourceLocation` at all, and every displayed `KirObject` name
            // below is the bare symbol/module name, never `path` — so the qualified id path is
            // the correct thing to pass in, not the bare one. (`data.path` itself is still used
            // for the diagnostic message below, where a human does read it.)
            let id_path =
                ekos_common::project::project_qualify(&data.path, data.project.as_deref());
            let file_id = KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, id_path.as_bytes()));
            let result = match parse_rust_file(&id_path, &data.source, file_id) {
                Ok(r) => r,
                Err(e) => {
                    ctx.diagnostics
                        .lock()
                        .unwrap()
                        .warning("RUST003", format!("cannot parse {}: {e}", data.path));
                    continue;
                }
            };

            stats.files_processed += 1;
            stats.symbols_total += result.symbol_count;
            stats.calls_total += result
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Calls)
                .count();

            for obj in result.objects {
                if seen_modules.insert(obj.id)
                    || !matches!(obj.kind, ObjectKind::Custom(ref k) if k == "RustModule")
                {
                    combined.add_object(obj);
                }
            }
            for rel in result.relationships {
                combined.add_relationship(rel);
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
            .map_err(|e| PassError::failed(format!("write artifact: {e}")))?;

        tracing::info!(
            pass = %self.pass_id,
            files = stats.files_processed,
            symbols = stats.symbols_total,
            calls = stats.calls_total,
            "rust-analyzer complete"
        );
        Ok(())
    }
}

// ── Parsing ──────────────────────────────────────────────────────────────────

struct RustFileResult {
    objects: Vec<KirObject>,
    relationships: Vec<KirRelationship>,
    symbol_count: usize,
}

fn parse_rust_file(path: &str, source: &str, file_id: KirId) -> Result<RustFileResult, syn::Error> {
    let file = syn::parse_file(source)?;
    let mut result = RustFileResult {
        objects: Vec::new(),
        relationships: Vec::new(),
        symbol_count: 0,
    };

    // Top-level function name -> KirId, for resolving bare `foo(...)` calls.
    let mut functions: HashMap<String, KirId> = HashMap::new();
    // Method simple name -> all (qualified name, KirId) candidates in this file, for resolving
    // `x.method(...)` calls only when the name is unambiguous within the file.
    let mut methods_by_name: HashMap<String, Vec<(String, KirId)>> = HashMap::new();
    // (Type, method) -> KirId, for resolving `Type::method(...)`/`Self::method(...)` calls.
    let mut methods_exact: HashMap<(String, String), KirId> = HashMap::new();
    // Function/method bodies to walk for Calls edges once every symbol in the file is known,
    // so a call to a symbol defined later in the file still resolves.
    let mut bodies: Vec<(KirId, &syn::Block, Option<String>)> = Vec::new();

    for item in &file.items {
        match item {
            Item::Use(u) => {
                let mut modules = Vec::new();
                flatten_use_tree(&u.tree, String::new(), &mut modules);
                for m in modules {
                    add_import(&m, file_id, &mut result);
                }
            }
            Item::Fn(f) => {
                let name = f.sig.ident.to_string();
                let doc = extract_doc_comment(&f.attrs);
                let id = add_symbol(
                    &name,
                    "function",
                    path,
                    file_id,
                    &mut result,
                    doc,
                    item_span(f),
                );
                result.symbol_count += 1;
                functions.insert(name, id);
                bodies.push((id, f.block.as_ref(), None));
            }
            Item::Struct(s) => {
                let doc = extract_doc_comment(&s.attrs);
                add_symbol(
                    &s.ident.to_string(),
                    "struct",
                    path,
                    file_id,
                    &mut result,
                    doc,
                    item_span(s),
                );
                result.symbol_count += 1;
            }
            Item::Enum(e) => {
                let doc = extract_doc_comment(&e.attrs);
                add_symbol(
                    &e.ident.to_string(),
                    "enum",
                    path,
                    file_id,
                    &mut result,
                    doc,
                    item_span(e),
                );
                result.symbol_count += 1;
            }
            Item::Trait(t) => {
                let doc = extract_doc_comment(&t.attrs);
                add_symbol(
                    &t.ident.to_string(),
                    "trait",
                    path,
                    file_id,
                    &mut result,
                    doc,
                    item_span(t),
                );
                result.symbol_count += 1;
            }
            Item::Impl(imp) => {
                let Some(type_name) = type_name(&imp.self_ty) else {
                    continue;
                };
                for ii in &imp.items {
                    if let ImplItem::Fn(m) = ii {
                        let method_name = m.sig.ident.to_string();
                        let qualified = format!("{type_name}::{method_name}");
                        let doc = extract_doc_comment(&m.attrs);
                        let id = add_symbol(
                            &qualified,
                            "method",
                            path,
                            file_id,
                            &mut result,
                            doc,
                            item_span(m),
                        );
                        result.symbol_count += 1;
                        methods_by_name
                            .entry(method_name.clone())
                            .or_default()
                            .push((qualified, id));
                        methods_exact.insert((type_name.clone(), method_name), id);
                        bodies.push((id, &m.block, Some(type_name.clone())));
                    }
                }
            }
            _ => {}
        }
    }

    for (caller_id, block, self_type) in bodies {
        let mut visitor = CallVisitor {
            caller_id,
            self_type,
            functions: &functions,
            methods_by_name: &methods_by_name,
            methods_exact: &methods_exact,
            edges: HashSet::new(),
        };
        visitor.visit_block(block);
        for (from, to) in visitor.edges {
            result
                .relationships
                .push(KirRelationship::new(RelationshipKind::Calls, from, to));
        }
    }

    Ok(result)
}

fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(tp) => tp.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// Flattens a `UseTree` (`Path`/`Name`/`Rename`/`Group`/`Glob`) into fully-qualified module path
/// strings, e.g. `use ekos_kir::{KirObject, KirId};` -> `["ekos_kir::KirObject",
/// "ekos_kir::KirId"]`.
fn flatten_use_tree(tree: &UseTree, prefix: String, out: &mut Vec<String>) {
    match tree {
        UseTree::Path(p) => {
            let next = if prefix.is_empty() {
                p.ident.to_string()
            } else {
                format!("{prefix}::{}", p.ident)
            };
            flatten_use_tree(&p.tree, next, out);
        }
        UseTree::Name(n) => {
            out.push(if prefix.is_empty() {
                n.ident.to_string()
            } else {
                format!("{prefix}::{}", n.ident)
            });
        }
        UseTree::Rename(r) => {
            out.push(if prefix.is_empty() {
                r.ident.to_string()
            } else {
                format!("{prefix}::{}", r.ident)
            });
        }
        UseTree::Glob(_) => {
            if !prefix.is_empty() {
                out.push(format!("{prefix}::*"));
            }
        }
        UseTree::Group(g) => {
            for t in &g.items {
                flatten_use_tree(t, prefix.clone(), out);
            }
        }
    }
}

fn rust_module_kir_id(module: &str) -> KirId {
    KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("rust-module:{module}").as_bytes(),
    ))
}

fn add_import(module: &str, file_id: KirId, result: &mut RustFileResult) {
    let target_id = rust_module_kir_id(module);
    let mut obj = KirObject::new(module, ObjectKind::Custom("RustModule".to_string()));
    obj.id = target_id;
    result.objects.push(obj);
    result.relationships.push(KirRelationship::new(
        RelationshipKind::DependsOn,
        file_id,
        target_id,
    ));
}

/// RFC 0088: real 1-indexed `(start_line, end_line)` from `syn`'s own joined span — `(0, 0)`
/// (never a valid real line number) when `proc-macro2`'s fallback span resolution couldn't
/// locate it, so the caller can tell "genuinely unresolved" apart from a real one-line item.
fn item_span<T: syn::spanned::Spanned>(item: &T) -> (u32, u32) {
    let span = item.span();
    (span.start().line as u32, span.end().line as u32)
}

fn add_symbol(
    name: &str,
    kind: &str,
    path: &str,
    file_id: KirId,
    result: &mut RustFileResult,
    doc: Option<String>,
    span: (u32, u32),
) -> KirId {
    let target_id = KirId(Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("rust-symbol:{path}:{name}").as_bytes(),
    ));
    let mut obj = KirObject::new(name, ObjectKind::Custom("RustSymbol".to_string()))
        .with_property("kind", serde_json::Value::String(kind.to_string()));
    obj.id = target_id;
    // Real, only when the item actually has a real `///` doc comment — never fabricated.
    if let Some(doc) = doc {
        obj.properties
            .insert("description".into(), serde_json::json!(doc));
    }
    // RFC 0088: real source span from `syn`'s own joined item span (`proc-macro2`'s
    // `span-locations` feature — without it every span silently reports line 0). `(0, 0)` (real
    // line numbers are always >= 1) means the span couldn't be resolved; skip rather than write
    // a fabricated `{0, 0}` a downstream reader could mistake for a real one-line item.
    if span != (0, 0) {
        obj.properties.insert(
            "source_span".into(),
            serde_json::json!({"start_line": span.0, "end_line": span.1}),
        );
    }
    result.objects.push(obj);
    result.relationships.push(KirRelationship::new(
        RelationshipKind::Contains,
        file_id,
        target_id,
    ));
    target_id
}

/// Real `///` doc-comment extraction (Phase 1 of the "Real Descriptions, Purpose, and Links"
/// plan) — `syn` already desugars each `///` line into its own `#[doc = "..."]` attribute, so
/// this is a real, direct read of the AST `syn::parse_file` already built, not new parsing.
/// Consecutive `#[doc = "..."]` attributes (one per real source line) are joined with a space
/// into one real description. `None` when the item has no real doc attribute at all — never
/// fabricated.
fn extract_doc_comment(attrs: &[syn::Attribute]) -> Option<String> {
    let lines: Vec<String> = attrs
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
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" ").trim().to_string())
    }
}

// ── Call-expression recognition ─────────────────────────────────────────────

struct CallVisitor<'a> {
    caller_id: KirId,
    /// The `impl Self`'s own type name, for resolving `Self::method(...)` calls; `None` for a
    /// top-level free function.
    self_type: Option<String>,
    functions: &'a HashMap<String, KirId>,
    methods_by_name: &'a HashMap<String, Vec<(String, KirId)>>,
    methods_exact: &'a HashMap<(String, String), KirId>,
    edges: HashSet<(KirId, KirId)>,
}

impl<'a, 'ast> Visit<'ast> for CallVisitor<'a> {
    fn visit_expr_call(&mut self, node: &'ast syn::ExprCall) {
        if let syn::Expr::Path(p) = node.func.as_ref() {
            let segs: Vec<String> = p
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            match segs.len() {
                1 => {
                    if let Some(&id) = self.functions.get(&segs[0]) {
                        self.edges.insert((self.caller_id, id));
                    }
                }
                2 => {
                    let ty = if segs[0] == "Self" {
                        self.self_type.clone().unwrap_or_else(|| segs[0].clone())
                    } else {
                        segs[0].clone()
                    };
                    if let Some(&id) = self.methods_exact.get(&(ty, segs[1].clone())) {
                        self.edges.insert((self.caller_id, id));
                    }
                }
                _ => {}
            }
        }
        syn::visit::visit_expr_call(self, node);
    }

    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        let name = node.method.to_string();
        if let Some(candidates) = self.methods_by_name.get(&name)
            && candidates.len() == 1
        {
            self.edges.insert((self.caller_id, candidates[0].1));
        }
        syn::visit::visit_expr_method_call(self, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> RustFileResult {
        parse_rust_file("test.rs", source, KirId(Uuid::new_v4())).unwrap()
    }

    #[test]
    fn recognizes_use_imports_as_depends_on() {
        let result = parse("use std::collections::HashMap;\nuse ekos_kir::{KirObject, KirId};\n");
        let names: Vec<&str> = result.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"std::collections::HashMap"));
        assert!(names.contains(&"ekos_kir::KirObject"));
        assert!(names.contains(&"ekos_kir::KirId"));
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind == RelationshipKind::DependsOn)
        );
    }

    #[test]
    fn recognizes_fn_struct_enum_trait_as_symbols() {
        let result =
            parse("fn foo() {}\nstruct Bar;\nenum Baz { A, B }\ntrait Qux { fn q(&self); }\n");
        let names: Vec<&str> = result.objects.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"foo"));
        assert!(names.contains(&"Bar"));
        assert!(names.contains(&"Baz"));
        assert!(names.contains(&"Qux"));
        let foo = result.objects.iter().find(|o| o.name == "foo").unwrap();
        assert_eq!(foo.properties["kind"], "function");
    }

    #[test]
    fn a_multi_line_function_gets_a_real_source_span() {
        let result = parse("fn foo() {\n    let x = 1;\n    x + 1\n}\n");
        let foo = result.objects.iter().find(|o| o.name == "foo").unwrap();
        assert_eq!(
            foo.properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 4})
        );
    }

    #[test]
    fn a_real_doc_comment_is_included_in_the_symbols_own_source_span() {
        // Confirmed live rather than assumed: `syn`'s own joined span for `ItemFn` covers every
        // real token belonging to the item, including its leading `#[doc = "..."]` attributes
        // (what `///` desugars to) — the span starts at the comment, not the `fn` keyword. Kept
        // rather than trimmed: RFC 0088's `ai_comment_check` step wants the LLM to see a real
        // existing comment right alongside the real code it documents, not stripped away.
        let result = parse("/// Hashes a password.\nfn hash(pw: &str) {\n    pw.to_string()\n}\n");
        let hash_fn = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(
            hash_fn.properties["source_span"],
            serde_json::json!({"start_line": 1, "end_line": 4})
        );
    }

    #[test]
    fn an_impl_method_gets_a_real_source_span() {
        let result = parse("struct S;\nimpl S {\n    fn m(&self) {\n        1\n    }\n}\n");
        let m = result.objects.iter().find(|o| o.name == "S::m").unwrap();
        assert_eq!(
            m.properties["source_span"],
            serde_json::json!({"start_line": 3, "end_line": 5})
        );
    }

    #[test]
    fn a_real_doc_comment_becomes_a_real_description() {
        let result = parse("/// Hashes a password using bcrypt.\nfn hash(pw: &str) {}\n");
        let hash_fn = result.objects.iter().find(|o| o.name == "hash").unwrap();
        assert_eq!(
            hash_fn.properties["description"],
            "Hashes a password using bcrypt."
        );
    }

    #[test]
    fn a_multi_line_doc_comment_joins_into_one_real_description() {
        let result = parse("/// Line one.\n/// Line two.\nstruct Widget;\n");
        let widget = result.objects.iter().find(|o| o.name == "Widget").unwrap();
        assert_eq!(widget.properties["description"], "Line one. Line two.");
    }

    #[test]
    fn a_method_with_a_real_doc_comment_gets_a_real_description() {
        let result =
            parse("struct Foo;\nimpl Foo {\n    /// Does the thing.\n    fn bar(&self) {}\n}\n");
        let method = result
            .objects
            .iter()
            .find(|o| o.name == "Foo::bar")
            .unwrap();
        assert_eq!(method.properties["description"], "Does the thing.");
    }

    #[test]
    fn an_item_with_no_real_doc_comment_has_no_description_property_at_all() {
        let result = parse("fn plain() {}\n");
        let plain = result.objects.iter().find(|o| o.name == "plain").unwrap();
        assert!(!plain.properties.contains_key("description"));
    }

    #[test]
    fn same_file_function_call_becomes_calls_edge() {
        let result = parse("fn helper() {}\nfn main() {\n    helper();\n}\n");
        assert_eq!(
            result
                .relationships
                .iter()
                .filter(|r| r.kind == RelationshipKind::Calls)
                .count(),
            1
        );
    }

    #[test]
    fn call_to_external_or_std_function_is_not_recorded() {
        let result = parse("fn main() {\n    println!(\"hi\");\n    std::process::exit(0);\n}\n");
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind != RelationshipKind::Calls)
        );
    }

    #[test]
    fn same_file_method_call_becomes_calls_edge() {
        let result = parse(
            "struct Foo;\nimpl Foo {\n    fn new() -> Self { Self }\n    fn run(&self) {\n        self.helper();\n    }\n    fn helper(&self) {}\n}\n",
        );
        let calls: Vec<&KirRelationship> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        let helper = result
            .objects
            .iter()
            .find(|o| o.name == "Foo::helper")
            .unwrap();
        assert_eq!(calls[0].to, helper.id);
    }

    #[test]
    fn self_colon_colon_associated_call_resolves_via_impl_type() {
        let result = parse(
            "struct Foo;\nimpl Foo {\n    fn new() -> Self { Self }\n    fn make() -> Self {\n        Self::new()\n    }\n}\n",
        );
        let new_symbol = result
            .objects
            .iter()
            .find(|o| o.name == "Foo::new")
            .unwrap();
        let calls: Vec<&KirRelationship> = result
            .relationships
            .iter()
            .filter(|r| r.kind == RelationshipKind::Calls)
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].to, new_symbol.id);
    }

    #[test]
    fn ambiguous_method_name_across_two_types_is_not_recorded() {
        let result = parse(
            "struct A;\nimpl A {\n    fn run(&self) {}\n}\nstruct B;\nimpl B {\n    fn run(&self) {}\n}\nfn call_it(a: &A) {\n    a.run();\n}\n",
        );
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind != RelationshipKind::Calls),
            "method name `run` is ambiguous between A::run and B::run in this file"
        );
    }

    #[test]
    fn call_inside_macro_invocation_is_not_recorded() {
        // `syn` never expands macros — a macro's arguments are an opaque `TokenStream`, not
        // parsed `Expr` nodes, so `helper()` inside `vec![...]` is invisible to this visitor.
        // Documents the real, honest limitation rather than claiming macro-transparent coverage.
        let result =
            parse("fn helper() {}\nfn main() {\n    let v = vec![helper()];\n    let _ = v;\n}\n");
        assert!(
            result
                .relationships
                .iter()
                .all(|r| r.kind != RelationshipKind::Calls)
        );
    }
}
